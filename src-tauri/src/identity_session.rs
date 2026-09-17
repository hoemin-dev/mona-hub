use super::*;
use std::time::Duration;

const ORIGIN: &str = "https://mona-acdc.pages.dev";
// A protected, script-free resource: visiting the AC/DC UI would run its own API calls.
const LANDING: &str = "https://mona-acdc.pages.dev/app/css/style.css";
#[derive(Default)]
struct Session {
    generation: u64,
    claimed: bool,
    identity: Option<AcDcIdentityResponse>,
    error: Option<String>,
}
impl Session {
    fn claim(&mut self) -> Option<u64> {
        if self.claimed { return None; }
        self.claimed = true;
        Some(self.generation)
    }
    fn is_active(&self, generation: u64, state: u8) -> bool {
        self.generation == generation && matches!(state, AUTH_RESOLVING_IDENTITY | AUTH_WAITING_FOR_MAIN)
    }
    fn clear(&mut self) {
        self.generation += 1;
        self.claimed = false;
        self.identity = None;
        self.error = None;
    }
}
static SESSION: Mutex<Session> = Mutex::new(Session { generation: 0, claimed: false, identity: None, error: None });

pub fn clear() {
    SESSION.lock().unwrap().clear();
}
pub fn identity() -> Result<AcDcIdentityResponse, String> {
    SESSION.lock().unwrap().identity.clone().ok_or("access-session-unavailable".into())
}
fn active(generation: u64) -> bool {
    SESSION.lock().unwrap().is_active(generation, AUTH_FLOW_STATE.load(Ordering::Acquire))
}
fn fail(app: &AppHandle, generation: u64, code: String) {
    if !active(generation) { return; }
    log::error!("[AC/DC] login blocked code={code}");
    set_auth_state(app, AUTH_IDLE);
    SESSION.lock().unwrap().error = Some(code);
    if let Some(main) = app.get_webview_window("main") { let _ = main.navigate(app_url(PRELOGIN_PATH)); }
    if let Some(login) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        let _ = set_login_window_local_mode(&login);
        let _ = login.navigate(app_url(LOGIN_START_PATH));
        let _ = login.show();
    } else {
        // Fast path has no login WebView yet; use the existing login entry point.
        let _ = show_or_create_login_window(app, "session-ui-failed");
    }
}
pub fn ui_failed(app: &AppHandle) {
    let generation = SESSION.lock().unwrap().generation;
    fail(app, generation, "identity-ui-unavailable".into());
}
// Startup only: use the existing main profile before allocating a login WebView.
// No cookie deletion or navigation is needed when both Access sessions are valid.
pub fn try_startup(app: &AppHandle) {
    clear();
    set_auth_state(app, AUTH_RESOLVING_IDENTITY);
    let generation = SESSION.lock().unwrap().generation;
    startup_trace::mark("fast-path.begin; cookie-source=main; login-window-absent");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = resolve(&app, generation, "main", Duration::from_secs(5)).await;
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if !active(generation) { return; }
            match result {
                Ok(identity) => {
                    startup_trace::mark("fast-path.hit; login-and-sso.skipped");
                    finish(&handle, generation, identity);
                }
                Err(code) => {
                    startup_trace::mark(&format!("fast-path.miss code={code}"));
                    fallback_startup(&handle);
                }
            }
        });
    });
}

// UI-thread only. Clearing the generation prevents late results from signing in.
pub fn fallback_startup(app: &AppHandle) {
    clear();
    set_auth_state(app, AUTH_CHECKING_SESSION);
    startup_trace::mark("fast-path.fallback; existing-login-flow.begin");
    if let Err(error) = show_or_create_login_window(app, "startup") {
        set_auth_state(app, AUTH_IDLE);
        log::error!("[auth] startup fallback window failed: {error}");
    }
}

fn finish(app: &AppHandle, generation: u64, identity: AcDcIdentityResponse) {
    if !active(generation) { return; }
    SESSION.lock().unwrap().identity = Some(identity);
    acdc_diagnostic::record("[AC/DC] resolve succeeded person_id_present=true");
    if let Some(main) = app.get_webview_window("main") {
        set_auth_state(app, AUTH_WAITING_FOR_MAIN);
        if main.navigate(app_url(ACCESS_APP_PATH)).is_ok() { return; }
        set_auth_state(app, AUTH_RESOLVING_IDENTITY);
    }
    fail(app, generation, "main-navigation-failed".into());
}

pub fn start(app: &AppHandle) {
    startup_trace::mark("session.existing.confirmed; acdc.sso.begin");
    clear();
    let generation = SESSION.lock().unwrap().generation;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Remove the old application cookie so switching MonaHub accounts cannot
        // silently reuse the previous AC/DC account. Keep Access global SSO.
        startup_trace::mark("acdc.cookie-reset.begin");
        let result = (|| -> Result<(), String> {
            let login = handle.get_webview_window(LOGIN_WINDOW_LABEL).ok_or("login-window-missing")?;
            for cookie in login.cookies_for_url(Url::parse(ORIGIN).unwrap()).map_err(|_| "access-session-unavailable")? {
                if cookie.name() == "CF_Authorization" {
                    login.delete_cookie(cookie).map_err(|_| "access-session-unavailable")?;
                }
            }
            Ok(())
        })();
        startup_trace::mark("acdc.cookie-reset.end; navigation.dispatch");
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            startup_trace::mark("acdc.navigation.main-thread.enter");
            if !active(generation) { return; }
            let result = result.and_then(|_| app.get_webview_window(LOGIN_WINDOW_LABEL)
                .ok_or("login-window-missing".to_string())
                .and_then(|login| login.navigate(Url::parse(&format!("{LANDING}?mona_identity_attempt={generation}")).unwrap()).map_err(|_| "identity-navigation-failed".into())));
            startup_trace::mark("acdc.navigation.returned");
            if let Err(code) = result { fail(&app, generation, code); }
        });
    });
    let app = app.clone();
    // One deadline per attempt, no navigation polling or automatic retry.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || fail(&handle, generation, "identity-timeout".into()));
    });
}
pub fn page_load(window: &tauri::Webview, payload: &tauri::webview::PageLoadPayload<'_>) -> bool {
    if window.label() != LOGIN_WINDOW_LABEL { return false; }
    let state = AUTH_FLOW_STATE.load(Ordering::Acquire);
    if state == AUTH_IDLE && is_login_start_url(payload.url()) && payload.event() == PageLoadEvent::Finished {
        if let Some(code) = SESSION.lock().unwrap().error.take() {
            let message = serde_json::to_string(&format!("AC/DC Identity 확인 실패 ({code}). 다시 로그인해 주세요.")).unwrap();
            let _ = window.eval(&format!("document.getElementById('loginStatus').textContent={message};document.getElementById('loginButton').disabled=false;"));
        }
    }
    if state != AUTH_RESOLVING_IDENTITY { return false; }
    if payload.event() == PageLoadEvent::Started && is_external_auth_url(payload.url()) {
        if let Some(login) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) {
            let _ = set_login_window_external_mode(&login);
            let _ = login.show();
        }
    }
    let generation = SESSION.lock().unwrap().generation;
    if payload.url().as_str() != format!("{LANDING}?mona_identity_attempt={generation}") { return false; }
    if let Some(login) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) { let _ = login.hide(); }
    if payload.event() != PageLoadEvent::Finished { return true; }
    let generation = {
        let Some(generation) = SESSION.lock().unwrap().claim() else { return true; };
        generation
    };
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        let result = resolve(&app, generation, LOGIN_WINDOW_LABEL, Duration::from_secs(10)).await;
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if !active(generation) { return; }
            match result {
                Ok(identity) => finish(&handle, generation, identity),
                Err(code) => fail(&handle, generation, code),
            }
        });
    });
    true
}
fn cookie(window: &WebviewWindow, origin: &str) -> Result<String, String> {
    window.cookies_for_url(Url::parse(origin).unwrap()).map_err(|_| "access-session-unavailable")?
        .into_iter().find(|c| c.name() == "CF_Authorization" && !c.value().is_empty())
        .map(|c| c.value().to_owned()).ok_or("access-session-unavailable".into())
}
async fn access_identity(client: &reqwest::Client, origin: &str, credential: &str) -> Result<acdc_identity::Identity, String> {
    startup_trace::mark(&format!("access.request.begin {origin}"));
    let response = client.get(format!("{origin}/cdn-cgi/access/get-identity"))
        .header("cookie", format!("CF_Authorization={credential}"))
        .send().await.map_err(|error| {
            startup_trace::mark(&format!("access.request.failed {origin} timeout={}", error.is_timeout()));
            "access-identity-unavailable"
        })?;
    startup_trace::mark(&format!("access.response {origin} status={}", response.status().as_u16()));
    if !response.status().is_success() { return Err("access-session-unavailable".into()); }
    let value = response.json().await.map_err(|_| "identity-invalid")?;
    startup_trace::mark(&format!("access.body.end {origin}"));
    acdc_identity::normalize(&value, ENTRA_TENANT_ID).map_err(String::from)
}
fn ensure_same_account(mona: &acdc_identity::Identity, acdc: &acdc_identity::Identity) -> Result<(), String> {
    if mona.tid != acdc.tid || mona.oid != acdc.oid { return Err("identity-account-mismatch".into()); }
    Ok(())
}
async fn resolve(app: &AppHandle, generation: u64, source: &str, timeout: Duration) -> Result<AcDcIdentityResponse, String> {
    let session_window = app.get_webview_window(source).ok_or("session-window-missing")?;
    startup_trace::mark("identity.resolve.begin; cookies.begin");
    let mona_cookie = cookie(&session_window, APP_BASE_URL)?;
    let acdc_cookie = cookie(&session_window, ORIGIN)?;
    startup_trace::mark("identity.cookies.end; client.begin");
    let client = reqwest::Client::builder().retry(reqwest::retry::never()).redirect(reqwest::redirect::Policy::none())
        .timeout(timeout).build().map_err(|_| "identity-client-failed")?;
    startup_trace::mark("identity.client.end");
    let started = Instant::now();
    let mona_client = client.clone();
    let mona_credential = mona_cookie.clone();
    let mona_request = tauri::async_runtime::spawn(async move {
        access_identity(&mona_client, APP_BASE_URL, &mona_credential).await
    });
    let acdc_result = access_identity(&client, ORIGIN, &acdc_cookie).await;
    let mona = mona_request.await.map_err(|_| "access-identity-unavailable")??;
    let acdc = acdc_result?;
    log::info!("[LOGIN PERF] parallel Access identity checks: {:.1}ms", started.elapsed().as_secs_f64() * 1000.0);
    ensure_same_account(&mona, &acdc)?;
    if !active(generation) || cookie(&session_window, APP_BASE_URL)? != mona_cookie || cookie(&session_window, ORIGIN)? != acdc_cookie {
        return Err("access-session-changed".into());
    }
    let result = get_me(&client, &format!("{ORIGIN}/api/me"), &acdc_cookie).await?;
    if !active(generation) || cookie(&session_window, APP_BASE_URL)? != mona_cookie || cookie(&session_window, ORIGIN)? != acdc_cookie {
        return Err("access-session-changed".into());
    }
    Ok(result)
}
async fn get_me(client: &reqwest::Client, endpoint: &str, credential: &str) -> Result<AcDcIdentityResponse, String> {
    // Exactly one GET. No redirect following, retries, caller claims or local bridge.
    startup_trace::mark("acdc.api-me.request.begin");
    log::info!("[AC/DC] GET /api/me once");
    let response = client.get(endpoint).header("cookie", format!("CF_Authorization={credential}"))
        .header("accept", "application/json").send().await.map_err(|_| "identity-request-failed")?;
    startup_trace::mark(&format!("acdc.api-me.response status={}", response.status().as_u16()));
    if !response.status().is_success() { return Err(format!("identity-http-{}", response.status().as_u16())); }
    let identity: AcDcIdentityResponse = response.json().await.map_err(|_| "identity-response-invalid")?;
    if !is_valid_person_id(&identity.person_id) || identity.tenant_id.trim().is_empty() || identity.status != "ACTIVE" {
        return Err("identity-response-invalid".into());
    }
    startup_trace::mark("acdc.api-me.body.end");
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_events_and_logout_invalidate_attempt() {
        let mut session = Session::default();
        let first = session.claim().unwrap();
        assert!(session.claim().is_none());
        session.identity = Some(AcDcIdentityResponse { person_id: "PER-ABCDEFGH".into(), tenant_id: "TEN-TEST".into(), status: "ACTIVE".into() });
        session.clear();
        assert!(session.identity.is_none());
        assert_ne!(first, session.generation);
        assert_eq!(session.claim(), Some(session.generation));
        assert!(session.claim().is_none());
    }
    #[test]
    fn startup_results_cannot_outlive_logout_cancel_or_fallback() {
        let mut session = Session::default();
        let generation = session.generation;
        assert!(session.is_active(generation, AUTH_RESOLVING_IDENTITY));
        assert!(session.is_active(generation, AUTH_WAITING_FOR_MAIN));
        for state in [AUTH_IDLE, AUTH_LOGGING_OUT_CLOUDFLARE, AUTH_LOGGING_OUT_ENTRA, AUTH_CHECKING_SESSION, AUTHENTICATED] {
            assert!(!session.is_active(generation, state));
        }
        session.clear();
        assert!(!session.is_active(generation, AUTH_RESOLVING_IDENTITY));
    }
    #[test]
    fn cached_sessions_must_belong_to_the_same_account() {
        let id = |tid: &str, oid: &str| acdc_identity::Identity { tid: tid.into(), oid: oid.into(), name: String::new() };
        assert!(ensure_same_account(&id("tenant-a", "user-a"), &id("tenant-a", "user-a")).is_ok());
        assert!(ensure_same_account(&id("tenant-a", "user-a"), &id("tenant-a", "user-b")).is_err());
        assert!(ensure_same_account(&id("tenant-a", "user-a"), &id("tenant-b", "user-a")).is_err());
    }
    #[test]
    fn get_is_single_authenticated_request_and_fails_closed() {
        use std::{io::{Read, Write}, net::TcpListener};
        for (status, body, expected) in [
            ("200 OK", r#"{"person_id":"PER-ABCDEFGH","tenant_id":"TEN-TEST","status":"ACTIVE","permissions":[]}"#, None),
            ("200 OK", r#"{"person_id":"PER-ABCDEFGH","tenant_id":"TEN-TEST","status":"BLOCKED"}"#, Some("identity-response-invalid")),
            ("200 OK", r#"{"person_id":"invalid","tenant_id":"TEN-TEST","status":"ACTIVE"}"#, Some("identity-response-invalid")),
            ("403 Forbidden", "{}", Some("identity-http-403")),
            ("302 Found", "{}", Some("identity-http-302")),
            ("200 OK", "<html>login</html>", Some("identity-response-invalid")),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/api/me", listener.local_addr().unwrap());
            let response = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).unwrap();
                let request = String::from_utf8_lossy(&buffer[..count]);
                assert!(request.starts_with("GET /api/me "));
                assert!(request.contains("cookie: CF_Authorization=fixture"));
                assert!(!request.contains("x-mona-local-bridge-token"));
                stream.write_all(response.as_bytes()).unwrap();
            });
            let client = reqwest::Client::builder().retry(reqwest::retry::never()).no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(2)).build().unwrap();
            let result = tauri::async_runtime::block_on(get_me(&client, &endpoint, "fixture"));
            assert_eq!(result.err().as_deref(), expected);
            server.join().unwrap();
        }
        assert!(!is_valid_person_id("PER-ABCDEF01"));
    }
}
