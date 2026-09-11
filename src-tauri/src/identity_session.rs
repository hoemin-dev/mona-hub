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
    SESSION.lock().unwrap().generation == generation
        && matches!(AUTH_FLOW_STATE.load(Ordering::Acquire), AUTH_RESOLVING_IDENTITY | AUTH_WAITING_FOR_MAIN)
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
    }
}
pub fn start(app: &AppHandle) {
    clear();
    let generation = SESSION.lock().unwrap().generation;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // Remove the old application cookie so switching MonaHub accounts cannot
        // silently reuse the previous AC/DC account. Keep Access global SSO.
        let result = (|| -> Result<(), String> {
            let login = handle.get_webview_window(LOGIN_WINDOW_LABEL).ok_or("login-window-missing")?;
            for cookie in login.cookies_for_url(Url::parse(ORIGIN).unwrap()).map_err(|_| "access-session-unavailable")? {
                if cookie.name() == "CF_Authorization" {
                    login.delete_cookie(cookie).map_err(|_| "access-session-unavailable")?;
                }
            }
            Ok(())
        })();
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if !active(generation) { return; }
            let result = result.and_then(|_| app.get_webview_window(LOGIN_WINDOW_LABEL)
                .ok_or("login-window-missing".to_string())
                .and_then(|login| login.navigate(Url::parse(&format!("{LANDING}?mona_identity_attempt={generation}")).unwrap()).map_err(|_| "identity-navigation-failed".into())));
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
        let result = resolve(&app, generation).await;
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if !active(generation) { return; }
            match result {
                Ok(identity) => {
                    SESSION.lock().unwrap().identity = Some(identity);
                    acdc_diagnostic::record("[AC/DC] resolve succeeded person_id_present=true");
                    if let Some(main) = handle.get_webview_window("main") {
                        set_auth_state(&handle, AUTH_WAITING_FOR_MAIN);
                        if main.navigate(app_url(ACCESS_APP_PATH)).is_ok() { return; }
                        set_auth_state(&handle, AUTH_RESOLVING_IDENTITY);
                    }
                    fail(&handle, generation, "main-navigation-failed".into());
                }
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
    let response = client.get(format!("{origin}/cdn-cgi/access/get-identity"))
        .header("cookie", format!("CF_Authorization={credential}"))
        .send().await.map_err(|_| "access-identity-unavailable")?;
    if !response.status().is_success() { return Err("access-session-unavailable".into()); }
    let value = response.json().await.map_err(|_| "identity-invalid")?;
    acdc_identity::normalize(&value, ENTRA_TENANT_ID).map_err(String::from)
}
async fn resolve(app: &AppHandle, generation: u64) -> Result<AcDcIdentityResponse, String> {
    let login = app.get_webview_window(LOGIN_WINDOW_LABEL).ok_or("login-window-missing")?;
    let mona_cookie = cookie(&login, APP_BASE_URL)?;
    let acdc_cookie = cookie(&login, ORIGIN)?;
    let client = reqwest::Client::builder().retry(reqwest::retry::never()).redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10)).build().map_err(|_| "identity-client-failed")?;
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
    if mona.tid != acdc.tid || mona.oid != acdc.oid { return Err("identity-account-mismatch".into()); }
    if !active(generation) || cookie(&login, APP_BASE_URL)? != mona_cookie || cookie(&login, ORIGIN)? != acdc_cookie {
        return Err("access-session-changed".into());
    }
    let result = get_me(&client, &format!("{ORIGIN}/api/me"), &acdc_cookie).await?;
    if !active(generation) || cookie(&login, APP_BASE_URL)? != mona_cookie || cookie(&login, ORIGIN)? != acdc_cookie {
        return Err("access-session-changed".into());
    }
    Ok(result)
}
async fn get_me(client: &reqwest::Client, endpoint: &str, credential: &str) -> Result<AcDcIdentityResponse, String> {
    // Exactly one GET. No redirect following, retries, caller claims or local bridge.
    log::info!("[AC/DC] GET /api/me once");
    let response = client.get(endpoint).header("cookie", format!("CF_Authorization={credential}"))
        .header("accept", "application/json").send().await.map_err(|_| "identity-request-failed")?;
    if !response.status().is_success() { return Err(format!("identity-http-{}", response.status().as_u16())); }
    let identity: AcDcIdentityResponse = response.json().await.map_err(|_| "identity-response-invalid")?;
    if !is_valid_person_id(&identity.person_id) || identity.tenant_id.trim().is_empty() || identity.status != "ACTIVE" {
        return Err("identity-response-invalid".into());
    }
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
