use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering},
        Mutex, OnceLock,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::webview::PageLoadEvent;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, LogicalSize, Manager, PhysicalPosition, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_log::{Target, TargetKind};

#[cfg(target_os = "windows")]
mod appbar;

const LOGIN_WINDOW_LABEL: &str = "login";
const PROFILE_POPUP_LABEL: &str = "profile-popup";
const PDFYS_WINDOW_LABEL: &str = "webapp-pdfys";
const PDFYS_URL: &str = "https://pdfys.pages.dev/";
static PDFYS_WINDOW_LOCK: Mutex<()> = Mutex::new(());
const APP_BASE_URL: &str = "https://mona-hub.pages.dev";
const ENTRA_TENANT_ID: &str = "40248705-eb98-485c-b761-ac9fd07e2baa";
const ACCESS_APP_PATH: &str = "/app/";
const LOGIN_START_PATH: &str = "/login/";
const PRELOGIN_PATH: &str = "/prelogin/";
const ACCESS_LOGOUT_PATH: &str = "/cdn-cgi/access/logout";
const LOGOUT_COMPLETE_PATH: &str = "/logout-complete/";
const AUTH_IDLE: u8 = 0;
const AUTH_WAITING_FOR_LOGIN: u8 = 1;
const AUTH_WAITING_FOR_MAIN: u8 = 2;
const AUTHENTICATED: u8 = 3;
const AUTH_LOGGING_OUT_CLOUDFLARE: u8 = 4;
const AUTH_LOGGING_OUT_ENTRA: u8 = 5;
const AUTH_CHECKING_SESSION: u8 = 6;
static LOGIN_REQUEST_ID: AtomicU64 = AtomicU64::new(0);
static AUTH_FLOW_STATE: AtomicU8 = AtomicU8::new(AUTH_IDLE);
static LOGIN_PAGE_LOAD: OnceLock<Mutex<Option<(u64, Instant)>>> = OnceLock::new();
static LOGIN_START_URL: OnceLock<Mutex<Option<Url>>> = OnceLock::new();
static PROFILE_POPUP_BLURRED_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static MAIN_FIRST_NAV_STARTED_LOGGED: AtomicBool = AtomicBool::new(false);
static MAIN_FIRST_NAV_FINISHED_LOGGED: AtomicBool = AtomicBool::new(false);

struct TrayAuthMenuItem(MenuItem<tauri::Wry>);

fn auth_state_name(state: u8) -> &'static str {
    match state {
        AUTH_IDLE => "PRELOGIN",
        AUTH_WAITING_FOR_LOGIN | AUTH_WAITING_FOR_MAIN => "AUTHENTICATING",
        AUTHENTICATED => "AUTHENTICATED",
        AUTH_LOGGING_OUT_CLOUDFLARE => "AUTH_LOGGING_OUT_CLOUDFLARE",
        AUTH_LOGGING_OUT_ENTRA => "AUTH_LOGGING_OUT_ENTRA",
        AUTH_CHECKING_SESSION => "CHECKING_SESSION",
        _ => "UNKNOWN",
    }
}

fn sync_tray_auth_menu(app: &AppHandle, state: u8) {
    let Some(item) = app.try_state::<TrayAuthMenuItem>() else {
        return;
    };
    let text = if matches!(
        state,
        AUTHENTICATED | AUTH_LOGGING_OUT_CLOUDFLARE | AUTH_LOGGING_OUT_ENTRA
    ) {
        "로그아웃"
    } else {
        "로그인"
    };
    if let Err(error) = item.0.set_text(text) {
        log::error!("[auth] failed to update tray menu: {error}");
    } else {
        log::info!("[tray] auth label -> {text}");
    }
}

fn set_auth_state(app: &AppHandle, state: u8) {
    let previous = AUTH_FLOW_STATE.swap(state, Ordering::AcqRel);
    if previous != state {
        log::info!("[auth] state {}", auth_state_name(state));
    }
    sync_tray_auth_menu(app, state);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginPresentationMode {
    MonaHubLogin,
    ExternalAuth,
}

fn login_presentation_mode(url: &Url) -> Option<LoginPresentationMode> {
    match classify_url(url) {
        UrlRole::MonaHubLogin => Some(LoginPresentationMode::MonaHubLogin),
        UrlRole::CloudflareAuth | UrlRole::EntraAuth => Some(LoginPresentationMode::ExternalAuth),
        _ => None,
    }
}

fn set_login_window_mode(window: &WebviewWindow, mode: LoginPresentationMode) -> tauri::Result<()> {
    let decorations = mode == LoginPresentationMode::ExternalAuth;
    window.set_decorations(decorations)?;
    window.set_resizable(false)?;
    window.set_maximizable(false)?;
    window.set_minimizable(true)?;
    window.set_closable(true)?;
    window.set_size(LogicalSize::new(420.0, 500.0))?;
    log::info!(
        "[auth-window] mode={} decorations={} size=420x500 maximizable=false",
        if decorations {
            "EXTERNAL_AUTH"
        } else {
            "MONAHUB_LOGIN"
        },
        decorations
    );
    Ok(())
}

fn set_login_window_local_mode(window: &WebviewWindow) -> tauri::Result<()> {
    set_login_window_mode(window, LoginPresentationMode::MonaHubLogin)
}

fn set_login_window_external_mode(window: &WebviewWindow) -> tauri::Result<()> {
    set_login_window_mode(window, LoginPresentationMode::ExternalAuth)
}

fn complete_logout(app: &AppHandle) {
    // The external logout has already succeeded at this point. Commit the shared
    // state first so every UI surface observes PRELOGIN even if a later window
    // operation fails.
    set_auth_state(app, AUTH_IDLE);
    log::info!("[logout] clearing authenticated app state");
    if let Some(pdfys) = app.get_webview_window(PDFYS_WINDOW_LABEL) {
        if let Err(error) = pdfys.destroy() {
            log::error!("[logout] failed to destroy PDFYS window: {error}");
        }
    }
    if let Some(popup) = app.get_webview_window(PROFILE_POPUP_LABEL) {
        if let Err(error) = popup.hide() {
            log::error!("[logout] failed to hide profile popup: {error}");
        }
    }

    if let Some(main) = app.get_webview_window("main") {
        match main.navigate(app_url(PRELOGIN_PATH)) {
            Ok(()) => log::info!("[main] -> /prelogin/"),
            Err(error) => log::error!("[logout] main -> prelogin failed: {error}"),
        }
    } else {
        log::error!("[logout] main -> prelogin failed: main window not found");
    }

    if let Some(login) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        if let Err(error) = set_login_window_local_mode(&login) {
            log::error!("[logout] failed to restore local login chrome: {error}");
        }
        if let Some(url) = resolved_login_start_url(app) {
            if let Err(error) = login.navigate(url) {
                log::error!("[logout] failed to navigate to local login: {error}");
            }
        } else {
            log::error!("[logout] local login URL is unavailable");
        }
        if let Err(error) = login
            .unminimize()
            .and_then(|_| login.show())
            .and_then(|_| login.set_focus())
        {
            log::error!("[logout] failed to show local login window: {error}");
        }
    }
    log::info!("[logout] completed");
}

fn login_page_load() -> &'static Mutex<Option<(u64, Instant)>> {
    LOGIN_PAGE_LOAD.get_or_init(|| Mutex::new(None))
}

fn login_start_url() -> &'static Mutex<Option<Url>> {
    LOGIN_START_URL.get_or_init(|| Mutex::new(None))
}

fn resolved_login_start_url(_app: &AppHandle) -> Option<Url> {
    Some(app_url(LOGIN_START_PATH))
}

fn safe_url_for_log(url: &Url) -> String {
    match (url.host_str(), url.path()) {
        (Some(host), path) => format!("{}://{host}{path}", url.scheme()),
        (None, path) if !path.is_empty() => format!("{}:{path}", url.scheme()),
        _ => url.scheme().to_string(),
    }
}

fn validate_web_app_request(label: &str, caller: &Url, id: &str, url: &str) -> Result<(), String> {
    if label != "main"
        || caller.origin().ascii_serialization() != APP_BASE_URL
        || caller.path() != ACCESS_APP_PATH
        || !caller.username().is_empty()
        || caller.password().is_some()
    {
        return Err("PDFYS can only be opened from the trusted main /app/ page.".into());
    }
    // Compare the original string before parsing: aliases, queries and fragments are rejected.
    if id != "pdfys" || url != PDFYS_URL {
        return Err("Only the configured PDFYS app and URL are allowed.".into());
    }
    Ok(())
}

#[tauri::command]
async fn open_web_app(window: WebviewWindow, id: String, url: String) -> Result<(), String> {
    log::info!("[PDFYS] command entered");
    // Async commands run off the Windows UI thread. Serialize lookup + build, not just clicks.
    log::info!("[PDFYS] window lock waiting");
    let _guard = PDFYS_WINDOW_LOCK.lock().map_err(|error| pdfys_error("window lock", error))?;
    let caller = window.url().map_err(|error| pdfys_error("caller URL", error))?;
    log::info!("[PDFYS] caller label={} url={} query={} fragment={}",
        window.label(), safe_url_for_log(&caller), caller.query().is_some(), caller.fragment().is_some());
    let state = AUTH_FLOW_STATE.load(Ordering::Acquire);
    log::info!("[PDFYS] auth state={} ({})", state, auth_state_name(state));
    log::info!("[PDFYS] validation start id={id:?} target_matches={}", url == PDFYS_URL);
    validate_web_app_request(window.label(), &caller, &id, &url)
        .map_err(|error| pdfys_error("caller/id/url validation", error))?;
    log::info!("[PDFYS] caller/id/url validation passed target={PDFYS_URL}");
    if state != AUTHENTICATED {
        log::warn!("[PDFYS] auth rejected");
        return Err("PDFYS requires an authenticated session.".into());
    }
    let app = window.app_handle();
    let pdfys = if let Some(existing) = app.get_webview_window(PDFYS_WINDOW_LABEL) {
        log::info!("[PDFYS] existing window lookup found=true");
        existing
    } else {
        log::info!("[PDFYS] existing window lookup found=false");
        log::info!("[PDFYS] build start");
        let created = WebviewWindowBuilder::new(
            app,
            PDFYS_WINDOW_LABEL,
            WebviewUrl::External(Url::parse(&url).map_err(|error| pdfys_error("target parse", error))?),
        )
        .title("PDFYS")
        .inner_size(1200.0, 800.0)
        .resizable(true)
        .decorations(true)
        .skip_taskbar(false)
        .always_on_top(false)
        .visible(false)
        .build()
        .map_err(|error| pdfys_error("build", error))?;
        log::info!("[PDFYS] build success");
        created
    };
    // Logout may finish while WebView creation is in progress; do not leave a late window open.
    if AUTH_FLOW_STATE.load(Ordering::Acquire) != AUTHENTICATED {
        log::warn!("[PDFYS] session ended after lookup/build; destroying window");
        pdfys.destroy().map_err(|error| pdfys_error("destroy", error))?;
        return Err("PDFYS opening cancelled because the session ended.".into());
    }
    let minimized = pdfys.is_minimized().map_err(|error| pdfys_error("is_minimized", error))?;
    log::info!("[PDFYS] restore minimized={minimized}");
    if minimized {
        pdfys.unminimize().map_err(|error| pdfys_error("restore", error))?;
        log::info!("[PDFYS] restore success");
    }
    pdfys.show().map_err(|error| pdfys_error("show", error))?;
    log::info!("[PDFYS] show success");
    pdfys.set_focus().map_err(|error| pdfys_error("focus", error))?;
    log::info!("[PDFYS] focus success; command completed");
    Ok(())
}

fn pdfys_error(stage: &str, error: impl std::fmt::Display) -> String {
    let message = format!("[PDFYS] {stage} failed: {error}");
    log::error!("{message}");
    message
}

#[cfg(test)]
mod pdfys_tests {
    use super::*;

    #[test]
    fn capability_accepts_ipc_origin_but_command_still_requires_app_page() {
        use tauri::utils::acl::RemoteUrlPattern;

        let origin = Url::parse(APP_BASE_URL).unwrap();
        let old_pattern: RemoteUrlPattern = format!("{APP_BASE_URL}/app/").parse().unwrap();
        assert!(!old_pattern.test(&origin), "custom-protocol IPC loses the page path");

        let capability: serde_json::Value = serde_json::from_str(include_str!(
            "../capabilities/pdfys-launcher.json"
        )).unwrap();
        assert_eq!(capability["windows"], serde_json::json!(["main"]));
        assert_eq!(capability["local"], false);
        assert_eq!(capability["permissions"], serde_json::json!(["allow-open-web-app"]));
        let patterns: Vec<RemoteUrlPattern> = capability["remote"]["urls"].as_array().unwrap()
            .iter().map(|value| value.as_str().unwrap().parse().unwrap()).collect();
        for source in [APP_BASE_URL, "https://mona-hub.pages.dev/app/", "https://mona-hub.pages.dev/app"] {
            assert!(patterns.iter().any(|pattern| pattern.test(&Url::parse(source).unwrap())));
        }
        for source in ["https://mona-hub.pages.dev/app", "https://mona-hub.pages.dev/prelogin/", APP_BASE_URL] {
            assert!(validate_web_app_request("main", &Url::parse(source).unwrap(), "pdfys", PDFYS_URL).is_err());
        }
        for source in ["http://mona-hub.pages.dev/", "https://mona-hub.pages.dev:444/", "https://mona-hub.pages.dev.evil.example/", PDFYS_URL] {
            assert!(!patterns.iter().any(|pattern| pattern.test(&Url::parse(source).unwrap())));
        }
    }

    #[test]
    fn only_exact_pdfys_target_is_allowed() {
        let caller = Url::parse("https://mona-hub.pages.dev/app/").unwrap();
        assert!(validate_web_app_request("main", &caller, "pdfys", PDFYS_URL).is_ok());
        for id in ["radar", "flex", "admin", "PDFYS", ""] {
            assert!(validate_web_app_request("main", &caller, id, PDFYS_URL).is_err());
        }
        for target in [
            "https://pdfys.pages.dev",
            "http://pdfys.pages.dev/",
            "https://pdfys.pages.dev/?query=1",
            "https://pdfys.pages.dev/#fragment",
            "https://pdfys.pages.dev/other",
            "https://pdfys.pages.dev.evil.example/",
            "https://pdfys.pages.dev@evil.example/",
            "https://example.com/",
        ] {
            assert!(validate_web_app_request("main", &caller, "pdfys", target).is_err());
        }
    }

    #[test]
    fn only_trusted_main_app_caller_is_allowed() {
        let caller = Url::parse("https://mona-hub.pages.dev/app/").unwrap();
        for label in ["webapp-pdfys", "login", "profile-popup", "help"] {
            assert!(validate_web_app_request(label, &caller, "pdfys", PDFYS_URL).is_err());
        }
        for source in [
            "https://mona-hub.pages.dev/prelogin/",
            "https://mona-hub.pages.dev/app/other",
            "http://mona-hub.pages.dev/app/",
            "https://mona-hub.pages.dev:444/app/",
            "https://mona-hub.pages.dev.evil.example/app/",
            "https://user@mona-hub.pages.dev/app/",
            "https://pdfys.pages.dev/",
            "http://localhost:1420/app/",
        ] {
            let source = Url::parse(source).unwrap();
            assert!(validate_web_app_request("main", &source, "pdfys", PDFYS_URL).is_err());
        }
    }
}

#[tauri::command]
fn notify_login_page_ready(window: WebviewWindow) -> Result<(), String> {
    if window.label() != LOGIN_WINDOW_LABEL {
        return Err("잘못된 창에서 로그인 준비 알림을 보냈습니다.".to_string());
    }

    let ready_at = Instant::now();
    log::info!(
        "[LOGIN PAGE] ready invoke thread={:?}",
        std::thread::current().id()
    );
    if let Ok(mut pending) = login_page_load().lock() {
        if let Some((request_id, load_started)) = pending.take() {
            log::info!(
                "[LOGIN PERF #{request_id}] page ready wait: {:.1}ms",
                load_started.elapsed().as_secs_f64() * 1000.0
            );
        } else {
            log::info!("[LOGIN PERF] notify_login_page_ready arrived without pending creation");
        }
    }

    if !matches!(
        AUTH_FLOW_STATE.load(Ordering::Acquire),
        AUTH_IDLE | AUTH_WAITING_FOR_LOGIN | AUTH_WAITING_FOR_MAIN
    ) {
        log::info!("[auth] login page ready while inactive; keeping window hidden");
        return Ok(());
    }
    window.show().map_err(|error| error.to_string())?;
    log::info!(
        "[LOGIN PERF] page-ready show: {:.1}ms",
        ready_at.elapsed().as_secs_f64() * 1000.0
    );
    let focus_started = Instant::now();
    window.set_focus().map_err(|error| error.to_string())?;
    log::info!(
        "[LOGIN PERF] page-ready focus: {:.1}ms",
        focus_started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

#[tauri::command]
fn close_login_window(window: WebviewWindow) -> Result<(), String> {
    if window.label() != LOGIN_WINDOW_LABEL {
        return Err("잘못된 창에서 로그인 닫기를 요청했습니다.".to_string());
    }

    handle_login_window_close(&window);
    window.hide().map_err(|error| error.to_string())
}

fn handle_login_window_close(window: &WebviewWindow) {
    let state = AUTH_FLOW_STATE.load(Ordering::Acquire);
    if state == AUTH_LOGGING_OUT_CLOUDFLARE {
        set_auth_state(window.app_handle(), AUTHENTICATED);
        if let Ok(start) = login_start_url().lock() {
            if let Some(url) = start.clone() {
                let _ = window.navigate(url);
            }
        }
        log::warn!(
            "[logout] cancelled by user; authenticated UI retained so logout can be retried"
        );
    } else if state == AUTH_LOGGING_OUT_ENTRA {
        set_auth_state(window.app_handle(), AUTH_IDLE);
        if let Some(main) = window.app_handle().get_webview_window("main") {
            let _ = main.navigate(app_url(PRELOGIN_PATH));
        }
        let _ = set_login_window_local_mode(window);
        if let Some(url) = resolved_login_start_url(window.app_handle()) {
            let _ = window.navigate(url);
        }
        log::warn!(
            "[logout] Entra logout window closed after Cloudflare logout; state kept PRELOGIN"
        );
    } else if matches!(state, AUTH_WAITING_FOR_LOGIN | AUTH_WAITING_FOR_MAIN) {
        set_auth_state(window.app_handle(), AUTH_IDLE);
        log::info!("[auth] login cancelled; state=PRELOGIN");
    }
    log::info!("[auth] login window hidden");
}

#[tauri::command]
fn minimize_login_window(window: WebviewWindow) -> Result<(), String> {
    if window.label() != LOGIN_WINDOW_LABEL {
        return Err("잘못된 창에서 로그인 최소화를 요청했습니다.".to_string());
    }

    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
fn begin_access_login(window: WebviewWindow) -> Result<(), String> {
    if window.label() != LOGIN_WINDOW_LABEL {
        return Err("잘못된 창에서 Access 로그인을 요청했습니다.".to_string());
    }

    if is_logging_out() {
        return Err("로그아웃이 진행 중입니다.".to_string());
    }
    set_auth_state(window.app_handle(), AUTH_WAITING_FOR_LOGIN);
    log::info!("[ACCESS] login started in login WebView");
    Ok(())
}

#[tauri::command]
fn log_login_viewport(
    window: WebviewWindow,
    event: String,
    inner_width: f64,
    inner_height: f64,
    client_width: f64,
    client_height: f64,
    device_pixel_ratio: f64,
) -> Result<(), String> {
    if window.label() != LOGIN_WINDOW_LABEL {
        return Err("잘못된 창에서 viewport 진단을 요청했습니다.".to_string());
    }

    log::info!(
        "[LOGIN VIEWPORT] event={event} window.inner={}x{} document.client={}x{} device_pixel_ratio={device_pixel_ratio:.2}",
        inner_width, inner_height, client_width, client_height
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UrlRole {
    MonaHubLogin,
    MonaHubPrelogin,
    ProtectedApp,
    CloudflareAuth,
    EntraAuth,
    LogoutComplete,
    Other,
}

fn path_matches_prefix(path: &str, prefix: &str) -> bool {
    path.trim_end_matches('/') == prefix.trim_end_matches('/') || path.starts_with(prefix)
}

fn classify_url(url: &Url) -> UrlRole {
    let Some(host) = url.host_str() else {
        return UrlRole::Other;
    };
    if url.scheme() != "https" {
        return UrlRole::Other;
    }
    let path = url.path();
    if host == "mona-hub.pages.dev" {
        if path_matches_prefix(path, LOGIN_START_PATH) {
            UrlRole::MonaHubLogin
        } else if path_matches_prefix(path, PRELOGIN_PATH) {
            UrlRole::MonaHubPrelogin
        } else if path_matches_prefix(path, ACCESS_APP_PATH) {
            UrlRole::ProtectedApp
        } else if path_matches_prefix(path, LOGOUT_COMPLETE_PATH) {
            UrlRole::LogoutComplete
        } else if path.starts_with("/cdn-cgi/access/") || path.starts_with("/auth/") {
            UrlRole::CloudflareAuth
        } else {
            UrlRole::Other
        }
    } else if host.ends_with(".cloudflareaccess.com") {
        UrlRole::CloudflareAuth
    } else if host == "login.microsoftonline.com" || host.ends_with(".microsoftonline.com") {
        UrlRole::EntraAuth
    } else {
        UrlRole::Other
    }
}

fn is_access_app_url(url: &Url) -> bool {
    classify_url(url) == UrlRole::ProtectedApp
}

fn is_logout_complete_url(url: &Url) -> bool {
    classify_url(url) == UrlRole::LogoutComplete
}

fn is_login_start_url(url: &Url) -> bool {
    classify_url(url) == UrlRole::MonaHubLogin
}

fn is_external_auth_url(url: &Url) -> bool {
    matches!(
        classify_url(url),
        UrlRole::CloudflareAuth | UrlRole::EntraAuth
    )
}

fn is_active_login_url(url: &Url) -> bool {
    is_login_start_url(url) || is_access_app_url(url) || is_external_auth_url(url)
}

#[cfg(test)]
mod login_url_tests {
    use super::*;

    fn url(value: &str) -> Url {
        value.parse().expect("test URL must be valid")
    }

    #[test]
    fn only_remote_mona_hub_login_is_the_login_start() {
        assert!(is_active_login_url(&url(
            "https://mona-hub.pages.dev/login/"
        )));
        assert!(!is_active_login_url(&url(
            "http://localhost:1420/login/"
        )));
        assert!(!is_active_login_url(&url(
            "https://tauri.localhost/login/"
        )));
    }

    #[test]
    fn external_auth_documents_are_active() {
        assert!(is_active_login_url(&url(
            "https://example.cloudflareaccess.com/cdn-cgi/access/login"
        )));
        assert!(is_active_login_url(&url(
            "https://login.microsoftonline.com/example/oauth2/v2.0/authorize"
        )));
        assert!(is_active_login_url(&url("https://mona-hub.pages.dev/app/")));
    }

    #[test]
    fn blank_and_logout_complete_are_terminal() {
        assert!(!is_active_login_url(&url("about:blank")));
        assert!(!is_active_login_url(&url(
            "https://mona-hub.pages.dev/logout-complete/"
        )));
    }

    #[test]
    fn presentation_mode_is_based_on_origin_and_path() {
        assert_eq!(
            login_presentation_mode(&url("https://mona-hub.pages.dev/login/")),
            Some(LoginPresentationMode::MonaHubLogin)
        );
        assert_eq!(
            login_presentation_mode(&url("https://mona-hub.pages.dev/app/")),
            None
        );
        assert_eq!(
            login_presentation_mode(&url(
                "https://login.microsoftonline.com/example/oauth2/v2.0/authorize?code=secret"
            )),
            Some(LoginPresentationMode::ExternalAuth)
        );
        assert_eq!(
            login_presentation_mode(&url("https://mona-hub.pages.dev/logout-complete/")),
            None
        );
    }
}

fn app_url(path: &str) -> Url {
    format!("{APP_BASE_URL}{path}")
        .parse()
        .expect("invalid MonaHub URL configuration")
}

fn entra_logout_url() -> Url {
    let mut url: Url =
        format!("https://login.microsoftonline.com/{ENTRA_TENANT_ID}/oauth2/v2.0/logout")
            .parse()
            .expect("invalid Entra logout URL configuration");
    url.query_pairs_mut().append_pair(
        "post_logout_redirect_uri",
        &format!("{APP_BASE_URL}{LOGOUT_COMPLETE_PATH}"),
    );
    url
}

fn is_logging_out() -> bool {
    matches!(
        AUTH_FLOW_STATE.load(Ordering::Acquire),
        AUTH_LOGGING_OUT_CLOUDFLARE | AUTH_LOGGING_OUT_ENTRA
    )
}

fn auth_navigation_stage(url: &Url) -> &'static str {
    match (url.host_str(), url.path()) {
        (Some("mona-hub.pages.dev"), path) if path == "/app" || path.starts_with("/app/") => {
            "protected-app"
        }
        (Some(host), path)
            if host.ends_with(".cloudflareaccess.com")
                && path.starts_with("/cdn-cgi/access/callback") =>
        {
            "cloudflare-callback"
        }
        (Some(host), path)
            if host.ends_with(".cloudflareaccess.com") && path.starts_with("/cdn-cgi/access/") =>
        {
            "cloudflare-access"
        }
        (Some("login.microsoftonline.com"), path) if path.contains("/oauth2/authorize") => {
            "microsoft-authorize"
        }
        (Some("login.microsoftonline.com"), _) => "microsoft-authentication",
        _ => "other",
    }
}

fn requires_auth_window(url: &Url) -> bool {
    matches!(
        auth_navigation_stage(url),
        "cloudflare-access"
            | "cloudflare-callback"
            | "microsoft-authorize"
            | "microsoft-authentication"
    )
}

fn login_target_monitor(app: &AppHandle, login_window: &WebviewWindow) -> Option<tauri::Monitor> {
    app.get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| login_window.current_monitor().ok().flatten())
}

fn center_login_window(app: &AppHandle, login_window: &WebviewWindow) -> tauri::Result<()> {
    if let Some(monitor) = login_target_monitor(app, login_window) {
        let monitor_position = monitor.position();
        let monitor_size = monitor.size();
        let window_size = login_window.outer_size()?;
        let x =
            monitor_position.x + (monitor_size.width.saturating_sub(window_size.width) / 2) as i32;
        let y = monitor_position.y
            + (monitor_size.height.saturating_sub(window_size.height) / 2) as i32;
        login_window.set_position(PhysicalPosition::new(x, y))?;
    } else {
        login_window.center()?;
    }
    Ok(())
}

fn log_login_window_metrics(window: &WebviewWindow, stage: &str, moment: &str) {
    match (window.inner_size(), window.outer_size(), window.scale_factor()) {
        (Ok(inner), Ok(outer), Ok(scale)) => log::info!(
            "[AUTH WINDOW] stage={stage} moment={moment} inner_physical={}x{} inner_logical={:.1}x{:.1} outer_physical={}x{} scale_factor={scale:.2}",
            inner.width,
            inner.height,
            inner.width as f64 / scale,
            inner.height as f64 / scale,
            outer.width,
            outer.height,
        ),
        (inner, outer, scale) => log::warn!(
            "[AUTH WINDOW] stage={stage} moment={moment} measurement_failed inner={inner:?} outer={outer:?} scale={scale:?}"
        ),
    }
}

fn ensure_auth_window_size(window: &tauri::Webview, url: &Url) {
    if window.label() != LOGIN_WINDOW_LABEL || !requires_auth_window(url) {
        return;
    }

    let Some(login_window) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) else {
        log::error!("[AUTH WINDOW] login window not found");
        return;
    };
    let stage = auth_navigation_stage(url);
    log_login_window_metrics(&login_window, stage, "fixed-size");
}

fn oauth_prompt(url: &Url) -> &'static str {
    let prompt = url
        .query_pairs()
        .find_map(|(key, value)| (key == "prompt").then(|| value.into_owned()));

    match prompt.as_deref() {
        Some("select_account") => "select_account",
        Some("login") => "login",
        Some("none") => "none",
        Some("consent") => "consent",
        Some(_) => "other",
        None => "absent",
    }
}

fn has_query_parameter(url: &Url, expected: &str) -> bool {
    url.query_pairs().any(|(key, _)| key == expected)
}

fn log_auth_navigation(label: &str, event: &str, url: &Url, state: u8) {
    // OAuth query values can contain state, authorization codes, and other secrets.
    // Log only the route and the presence/value category of account-selection hints.
    log::info!(
        "[AUTH NAVIGATION] event={event} window={label} stage={} scheme={} host={} path={} prompt={} login_hint={} domain_hint={} auth_state={state}",
        auth_navigation_stage(url),
        url.scheme(),
        url.host_str().unwrap_or("<none>"),
        url.path(),
        oauth_prompt(url),
        has_query_parameter(url, "login_hint"),
        has_query_parameter(url, "domain_hint"),
    );
}

fn handle_page_load(window: &tauri::Webview, payload: &tauri::webview::PageLoadPayload<'_>) {
    let label = window.label();
    let url = payload.url();
    let state = AUTH_FLOW_STATE.load(Ordering::Acquire);
    let event = match payload.event() {
        PageLoadEvent::Started => "started",
        PageLoadEvent::Finished => "finished",
    };
    if label == "main" {
        let first_event = match payload.event() {
            PageLoadEvent::Started => !MAIN_FIRST_NAV_STARTED_LOGGED.swap(true, Ordering::AcqRel),
            PageLoadEvent::Finished => {
                !MAIN_FIRST_NAV_FINISHED_LOGGED.swap(true, Ordering::AcqRel)
            }
        };
        if first_event {
            match payload.event() {
                PageLoadEvent::Started => {
                    log::info!("MAIN_FIRST_NAV_STARTED url={}", safe_url_for_log(url))
                }
                PageLoadEvent::Finished => {
                    log::info!("MAIN_FIRST_NAV_FINISHED url={}", safe_url_for_log(url))
                }
            }
            #[cfg(target_os = "windows")]
            if let Some(main) = window.app_handle().get_webview_window("main") {
                appbar::log_window_state(
                    &main,
                    if payload.event() == PageLoadEvent::Started {
                        "MAIN_FIRST_NAV_STARTED_STATE"
                    } else {
                        "MAIN_FIRST_NAV_FINISHED_STATE"
                    },
                );
            }
        }
    }
    if label == PROFILE_POPUP_LABEL {
        log::info!("[profile-popup] page load {event}: {url}");
    }
    log_auth_navigation(label, event, url, state);

    if label == LOGIN_WINDOW_LABEL && is_login_start_url(url) {
        if let Ok(mut start) = login_start_url().lock() {
            *start = Some(url.clone());
        }
    }

    if label == LOGIN_WINDOW_LABEL && payload.event() == PageLoadEvent::Started {
        if let Some(login_window) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) {
            let result = match login_presentation_mode(url) {
                Some(LoginPresentationMode::MonaHubLogin) => {
                    set_login_window_local_mode(&login_window)
                }
                Some(LoginPresentationMode::ExternalAuth) => {
                    set_login_window_external_mode(&login_window)
                }
                None => Ok(()),
            };
            if let Err(error) = result {
                log::error!("[auth-window] presentation update failed: {error}");
            }
        }
    }

    // After Access completes, the login WebView itself is redirected to the
    // protected app. Hide it at the navigation boundary so /app/ can finish
    // loading without ever being painted in the standalone login window. The
    // Finished event below still gates navigation of the existing main WebView.
    if label == LOGIN_WINDOW_LABEL
        && state == AUTH_WAITING_FOR_LOGIN
        && payload.event() == PageLoadEvent::Started
        && is_access_app_url(url)
    {
        if let Some(login_window) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) {
            match window.app_handle().get_webview_window("main") {
                Some(main) => match main.url() {
                    Ok(main_url) => log::info!(
                        "[ACCESS] protected app navigation started in login WebView; hiding login window before paint; main current url={}",
                        safe_url_for_log(&main_url)
                    ),
                    Err(error) => log::warn!(
                        "[ACCESS] protected app navigation started in login WebView; hiding login window before paint; main current url=<unavailable: {error}>"
                    ),
                },
                None => log::warn!(
                    "[ACCESS] protected app navigation started in login WebView; hiding login window before paint; main window missing"
                ),
            }
            if let Err(error) = login_window.hide() {
                log::error!(
                    "[ACCESS] failed to hide login window before protected app paint: {error}"
                );
            }
        }
    }

    if label == LOGIN_WINDOW_LABEL
        && state == AUTH_CHECKING_SESSION
        && payload.event() == PageLoadEvent::Started
        && is_external_auth_url(url)
    {
        log::info!("[auth] startup session missing; preparing local login");
        set_auth_state(window.app_handle(), AUTH_IDLE);
        if let Some(login_window) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) {
            if let Err(error) = set_login_window_local_mode(&login_window) {
                log::error!("[auth] startup local mode failed: {error}");
            }
            if let Some(start_url) = resolved_login_start_url(window.app_handle()) {
                if let Err(error) = login_window.navigate(start_url) {
                    log::error!("[auth] startup local login navigation failed: {error}");
                }
            }
            if let Err(error) = login_window
                .unminimize()
                .and_then(|_| login_window.show())
                .and_then(|_| login_window.set_focus())
            {
                log::error!("[auth] startup local login show failed: {error}");
            }
        }
        return;
    }

    if label == LOGIN_WINDOW_LABEL
        && state == AUTH_LOGGING_OUT_ENTRA
        && payload.event() == PageLoadEvent::Started
        && is_logout_complete_url(url)
    {
        log::info!("[logout] logout-complete reached");
        complete_logout(window.app_handle());
        return;
    }

    // Keep a diagnostic measurement at external-auth navigation boundaries. The
    // login window itself stays at its fixed logical size across these pages.
    if payload.event() == PageLoadEvent::Started {
        ensure_auth_window_size(window, url);
    }

    if payload.event() != PageLoadEvent::Finished {
        return;
    }

    if label == LOGIN_WINDOW_LABEL && state == AUTH_LOGGING_OUT_CLOUDFLARE {
        set_auth_state(window.app_handle(), AUTH_LOGGING_OUT_ENTRA);
        log::info!("[logout] cloudflare logout navigation completed");
        log::info!("[logout] entra logout");
        if let Err(error) = window.navigate(entra_logout_url()) {
            set_auth_state(window.app_handle(), AUTHENTICATED);
            log::error!("[logout] Entra navigation failed; logout can be retried: {error}");
        } else if let Some(login_window) = window
            .app_handle()
            .get_webview_window(LOGIN_WINDOW_LABEL)
        {
            log::info!("[logout] opening login window");
            if let Err(error) = login_window
                .unminimize()
                .and_then(|_| login_window.show())
                .and_then(|_| login_window.set_focus())
            {
                log::error!("[logout] failed to show logout window: {error}");
            }
        }
        return;
    }

    if label == LOGIN_WINDOW_LABEL
        && matches!(state, AUTH_CHECKING_SESSION | AUTH_WAITING_FOR_LOGIN)
        && is_access_app_url(url)
    {
        set_auth_state(window.app_handle(), AUTH_WAITING_FOR_MAIN);
        log::info!(
            "[ACCESS] protected app loaded in login WebView; navigating existing main WebView"
        );

        if let Some(main) = window.app_handle().get_webview_window("main") {
            if let Err(error) = main.navigate(app_url(ACCESS_APP_PATH)) {
                set_auth_state(window.app_handle(), AUTH_WAITING_FOR_LOGIN);
                log::error!("[ACCESS] main WebView navigation failed: {error}");
            }
        } else {
            set_auth_state(window.app_handle(), AUTH_WAITING_FOR_LOGIN);
            log::error!("[ACCESS] main WebView not found");
        }
        return;
    }

    if label == "main" && state == AUTH_WAITING_FOR_MAIN && is_access_app_url(url) {
        set_auth_state(window.app_handle(), AUTHENTICATED);
        log::info!("[ACCESS] protected app loaded in main WebView; hiding login window");
        if let Some(login) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) {
            if let Err(error) = login.hide() {
                log::error!("[ACCESS] failed to hide login window: {error}");
            }
        }
    }
}

#[tauri::command]
fn log_login_diagnostic(message: String) {
    log::info!("{message}");
}

#[tauri::command]
fn show_login_window(app: AppHandle) -> Result<(), String> {
    request_login(&app, "profile").map_err(|error| error.to_string())
}

fn position_profile_popup(app: &AppHandle, popup: &WebviewWindow) -> tauri::Result<()> {
    let Some(main) = app.get_webview_window("main") else {
        return Ok(());
    };
    let main_position = main.outer_position()?;
    let main_size = main.outer_size()?;
    let popup_size = popup.outer_size()?;
    let footer = (38.0 * main.scale_factor()?).round() as i32;
    let desired_x = main_position.x - popup_size.width as i32;
    let desired_y = main_position.y + main_size.height as i32 - popup_size.height as i32 - footer;
    let monitor = main.current_monitor()?.or(main.primary_monitor()?);
    let (x, y) = if let Some(monitor) = monitor {
        let work_position = monitor.position();
        let work_size = monitor.size();
        let max_x = work_position.x + work_size.width as i32 - popup_size.width as i32;
        let max_y = work_position.y + work_size.height as i32 - popup_size.height as i32;
        (
            desired_x.clamp(work_position.x, max_x.max(work_position.x)),
            desired_y.clamp(work_position.y, max_y.max(work_position.y)),
        )
    } else {
        (desired_x, desired_y.max(main_position.y))
    };
    log::info!(
        "[profile-popup] position = ({x}, {y}), main_rect=({}, {}) {}x{}, popup_size={}x{}, scale_factor={:.2}",
        main_position.x,
        main_position.y,
        main_size.width,
        main_size.height,
        popup_size.width,
        popup_size.height,
        main.scale_factor()?
    );
    popup.set_position(PhysicalPosition::new(x, y))
}

fn profile_popup(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(popup) = app.get_webview_window(PROFILE_POPUP_LABEL) {
        log::info!("[profile-popup] existing window = true");
        return Ok(popup);
    }
    log::info!("[profile-popup] existing window = false; creating hidden window");
    let popup = WebviewWindowBuilder::new(
        app,
        PROFILE_POPUP_LABEL,
        WebviewUrl::App("profile-popup/index.html".into()),
    )
    .title("MONA Hub 프로필")
    .inner_size(176.0, 126.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .shadow(false)
    .visible(false)
    .build()?;
    log::info!(
        "[profile-popup] created: visible={:?}, outer_size={:?}",
        popup.is_visible(),
        popup.outer_size()
    );
    Ok(popup)
}

#[tauri::command]
fn toggle_profile_popup(window: WebviewWindow) -> Result<bool, String> {
    log::info!(
        "[profile-popup] rust command entered: source={}",
        window.label()
    );
    if window.label() != "main" {
        return Err("잘못된 창에서 프로필 메뉴를 요청했습니다.".into());
    }
    if is_logging_out() {
        return Ok(false);
    }
    let popup = profile_popup(window.app_handle()).map_err(|e| e.to_string())?;
    if let Ok(mut blurred_at) = PROFILE_POPUP_BLURRED_AT
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        if blurred_at
            .take()
            .is_some_and(|at| at.elapsed().as_millis() < 350)
        {
            let result = popup.hide();
            log::info!("[profile-popup] recent blur guard hide result = {result:?}");
            return Ok(false);
        }
    }
    let visible = popup.is_visible().map_err(|e| e.to_string())?;
    log::info!("[profile-popup] is_visible before toggle = {visible}");
    if visible {
        let result = popup.hide();
        log::info!("[profile-popup] hide result = {result:?}");
        result.map_err(|e| e.to_string())?;
        return Ok(false);
    }
    position_profile_popup(window.app_handle(), &popup).map_err(|e| e.to_string())?;
    let unminimize_result = popup.unminimize();
    log::info!("[profile-popup] unminimize result = {unminimize_result:?}");
    unminimize_result.map_err(|e| e.to_string())?;
    let show_result = popup.show();
    log::info!("[profile-popup] show result = {show_result:?}");
    show_result.map_err(|e| e.to_string())?;
    let focus_result = popup.set_focus();
    log::info!("[profile-popup] focus result = {focus_result:?}");
    focus_result.map_err(|e| e.to_string())?;
    log::info!(
        "[profile-popup] after show: visible={:?}, position={:?}, outer_size={:?}, minimized={:?}",
        popup.is_visible(),
        popup.outer_position(),
        popup.outer_size(),
        popup.is_minimized()
    );
    Ok(true)
}

#[tauri::command]
fn hide_profile_popup(app: AppHandle) -> Result<(), String> {
    if let Some(popup) = app.get_webview_window(PROFILE_POPUP_LABEL) {
        popup.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn confirm_access_logout(window: WebviewWindow) -> Result<(), String> {
    if window.label() != PROFILE_POPUP_LABEL {
        return Err("잘못된 창에서 로그아웃을 확인했습니다.".into());
    }
    let Some(main) = window.app_handle().get_webview_window("main") else {
        return Err("main 창을 찾을 수 없습니다.".into());
    };
    window.hide().map_err(|e| e.to_string())?;
    main.eval("window.dispatchEvent(new CustomEvent('mona:logout-confirmed'))")
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn begin_access_logout(window: WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("잘못된 창에서 로그아웃을 요청했습니다.".into());
    }
    AUTH_FLOW_STATE
        .compare_exchange(
            AUTHENTICATED,
            AUTH_LOGGING_OUT_CLOUDFLARE,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .map_err(|state| format!("로그아웃을 시작할 수 없는 인증 상태입니다: {state}"))?;
    sync_tray_auth_menu(window.app_handle(), AUTH_LOGGING_OUT_CLOUDFLARE);
    let Some(login) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) else {
        set_auth_state(window.app_handle(), AUTHENTICATED);
        return Err("login 창을 찾을 수 없습니다.".into());
    };
    log::info!("[logout] started");
    match login.url() {
        Ok(url) => log::info!(
            "[logout] existing login window current url={}",
            safe_url_for_log(&url)
        ),
        Err(error) => {
            log::warn!("[logout] existing login window current url=<unavailable: {error}>")
        }
    }
    if let Err(error) = set_login_window_external_mode(&login) {
        set_auth_state(window.app_handle(), AUTHENTICATED);
        return Err(format!("외부 인증 창 모드 설정에 실패했습니다: {error}"));
    }
    log::info!("[logout] cloudflare access logout");
    login
        .navigate(app_url(ACCESS_LOGOUT_PATH))
        .map_err(|error| {
            set_auth_state(window.app_handle(), AUTHENTICATED);
            let _ = login.hide();
            log::error!("[logout] Cloudflare navigation failed; logout can be retried: {error}");
            error.to_string()
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LoginDiagnosticOperation {
    Normal,
    Noop,
    Lookup,
    Show,
    Focus,
    Unminimize,
    ShowFocus,
}

fn login_diagnostic_operation(origin: &str) -> LoginDiagnosticOperation {
    if origin == "startup" {
        return LoginDiagnosticOperation::Normal;
    }

    match std::env::var("MONA_LOGIN_DIAGNOSTIC_OP")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "noop" => LoginDiagnosticOperation::Noop,
        "lookup" => LoginDiagnosticOperation::Lookup,
        "show" => LoginDiagnosticOperation::Show,
        "focus" => LoginDiagnosticOperation::Focus,
        "unminimize" => LoginDiagnosticOperation::Unminimize,
        "show-focus" => LoginDiagnosticOperation::ShowFocus,
        _ => LoginDiagnosticOperation::Normal,
    }
}

fn request_login(app: &AppHandle, origin: &str) -> tauri::Result<()> {
    let state = AUTH_FLOW_STATE.load(Ordering::Acquire);
    log::info!(
        "[auth] request_login state={} source={origin}",
        auth_state_name(state)
    );
    let login_window = app.get_webview_window(LOGIN_WINDOW_LABEL);
    log::info!("[auth] login window exists={}", login_window.is_some());
    match login_window.as_ref().map(WebviewWindow::url) {
        Some(Ok(url)) => log::info!("[auth] login current url={}", safe_url_for_log(&url)),
        Some(Err(error)) => log::warn!("[auth] login current url=<unavailable: {error}>"),
        None => log::info!("[auth] login current url=<no-window>"),
    }
    match state {
        AUTHENTICATED => {
            log::info!("[auth] navigation decision=no-op authenticated");
            log::info!("[auth] skipping login window because already authenticated");
            return Ok(());
        }
        AUTH_LOGGING_OUT_CLOUDFLARE | AUTH_LOGGING_OUT_ENTRA => {
            log::info!("[auth] navigation decision=no-op logging-out");
            log::info!("[auth] login request ignored while logout is in progress");
            return Ok(());
        }
        AUTH_WAITING_FOR_LOGIN | AUTH_WAITING_FOR_MAIN => {
            log::info!("[auth] navigation decision=focus-only authenticating");
            if let Some(login_window) = login_window {
                log::info!("[auth] login window already active; focus only");
                login_window.unminimize()?;
                login_window.show()?;
                login_window.set_focus()?;
            }
            return Ok(());
        }
        _ => set_auth_state(app, AUTH_WAITING_FOR_LOGIN),
    }
    if let Err(error) = show_or_create_login_window(app, origin) {
        set_auth_state(app, AUTH_IDLE);
        return Err(error);
    }
    Ok(())
}

fn show_or_create_login_window(app: &AppHandle, origin: &str) -> tauri::Result<()> {
    let request_id = LOGIN_REQUEST_ID.fetch_add(1, Ordering::Relaxed) + 1;
    let total_started = Instant::now();
    let thread_id = format!("{:?}", std::thread::current().id());
    let operation = login_diagnostic_operation(origin);
    log::info!(
        "[LOGIN #{request_id}] show requested ({origin}) operation={operation:?} thread={thread_id}"
    );

    #[cfg(target_os = "windows")]
    let appbar_before = appbar::diagnostic_counts();

    if operation == LoginDiagnosticOperation::Noop {
        log::info!(
            "[LOGIN PERF #{request_id}] no-op return: {:.1}ms",
            total_started.elapsed().as_secs_f64() * 1000.0
        );
        return Ok(());
    }

    let lookup_started = Instant::now();
    let existing_window = app.get_webview_window(LOGIN_WINDOW_LABEL);
    log::info!(
        "[LOGIN PERF #{request_id}] get_webview_window(\"login\"): {:.1}ms",
        lookup_started.elapsed().as_secs_f64() * 1000.0
    );
    log::info!(
        "[LOGIN #{request_id}] existing window: {}",
        existing_window.is_some()
    );

    if operation == LoginDiagnosticOperation::Lookup {
        log::info!(
            "[LOGIN PERF #{request_id}] lookup-only return: {:.1}ms",
            total_started.elapsed().as_secs_f64() * 1000.0
        );
        return Ok(());
    }

    if let Some(login_window) = existing_window {
        let start_url = resolved_login_start_url(app);
        let current_url = login_window.url()?;
        if is_active_login_url(&current_url) {
            log::info!("[auth] navigation decision=reuse active login document");
        } else if let Some(url) = start_url {
            log::info!("[auth] stored login start url={}", safe_url_for_log(&url));
            log::info!("[auth] navigation decision=navigate login-start from terminal document");
            login_window.navigate(url)?;
            log::info!("[auth] prepared login start page for a new login attempt");
        } else {
            log::warn!(
                "[auth] navigation decision=blocked; terminal document with no known login start URL"
            );
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "login start URL is unavailable",
            )
            .into());
        }
        if operation == LoginDiagnosticOperation::Show {
            let started = Instant::now();
            login_window.show()?;
            log::info!(
                "[LOGIN PERF #{request_id}] show-only: {:.1}ms",
                started.elapsed().as_secs_f64() * 1000.0
            );
        } else if operation == LoginDiagnosticOperation::Focus {
            let started = Instant::now();
            login_window.set_focus()?;
            log::info!(
                "[LOGIN PERF #{request_id}] focus-only: {:.1}ms",
                started.elapsed().as_secs_f64() * 1000.0
            );
        } else if operation == LoginDiagnosticOperation::Unminimize {
            let started = Instant::now();
            login_window.unminimize()?;
            log::info!(
                "[LOGIN PERF #{request_id}] unminimize-only: {:.1}ms",
                started.elapsed().as_secs_f64() * 1000.0
            );
        } else if operation == LoginDiagnosticOperation::ShowFocus {
            let show_started = Instant::now();
            login_window.show()?;
            log::info!(
                "[LOGIN PERF #{request_id}] show (show-focus): {:.1}ms",
                show_started.elapsed().as_secs_f64() * 1000.0
            );
            let focus_started = Instant::now();
            login_window.set_focus()?;
            log::info!(
                "[LOGIN PERF #{request_id}] focus (show-focus): {:.1}ms",
                focus_started.elapsed().as_secs_f64() * 1000.0
            );
        } else {
            let state_started = Instant::now();
            let visible = login_window.is_visible()?;
            let minimized = login_window.is_minimized()?;
            log::info!("[LOGIN #{request_id}] visible: {visible}");
            log::info!("[LOGIN #{request_id}] minimized: {minimized}");
            log::info!(
                "[LOGIN PERF #{request_id}] state query: {:.1}ms",
                state_started.elapsed().as_secs_f64() * 1000.0
            );

            if minimized {
                let unminimize_started = Instant::now();
                login_window.unminimize()?;
                log::info!(
                    "[LOGIN PERF #{request_id}] unminimize: {:.1}ms",
                    unminimize_started.elapsed().as_secs_f64() * 1000.0
                );
            }
            if !visible {
                let show_started = Instant::now();
                login_window.show()?;
                log::info!(
                    "[LOGIN PERF #{request_id}] show: {:.1}ms",
                    show_started.elapsed().as_secs_f64() * 1000.0
                );
            }
            let focus_started = Instant::now();
            login_window.set_focus()?;
            log::info!(
                "[LOGIN PERF #{request_id}] focus: {:.1}ms",
                focus_started.elapsed().as_secs_f64() * 1000.0
            );
        }
        log::info!(
            "[LOGIN PERF #{request_id}] total show_login_window: {:.1}ms",
            total_started.elapsed().as_secs_f64() * 1000.0
        );
        #[cfg(target_os = "windows")]
        {
            let after = appbar::diagnostic_counts();
            log::info!(
                "[LOGIN APPBAR #{request_id}] callback={} reposition={} WM_ACTIVATE={} WM_WINDOWPOSCHANGED={} ABM_QUERYPOS={} ABM_SETPOS={}",
                after.callback.wrapping_sub(appbar_before.callback),
                after.reposition.wrapping_sub(appbar_before.reposition),
                after.activate.wrapping_sub(appbar_before.activate),
                after.windowpos.wrapping_sub(appbar_before.windowpos),
                after.query.wrapping_sub(appbar_before.query),
                after.setpos.wrapping_sub(appbar_before.setpos),
            );
        }
        return Ok(());
    }

    log::info!("[LOGIN #{request_id}] visible: false");
    log::info!("[LOGIN #{request_id}] minimized: false");
    log::info!("[LOGIN PERF #{request_id}] WebviewWindowBuilder start");
    let build_started = Instant::now();
    if let Ok(mut pending) = login_page_load().lock() {
        *pending = Some((request_id, build_started));
    }
    let initial_url = WebviewUrl::External(if origin == "startup" {
        app_url(ACCESS_APP_PATH)
    } else {
        app_url(LOGIN_START_PATH)
    });
    let login_window = WebviewWindowBuilder::new(app, LOGIN_WINDOW_LABEL, initial_url)
        .title("MONA-HUB 로그인")
        .inner_size(420.0, 500.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(true)
        .closable(true)
        .decorations(false)
        .always_on_top(false)
        .skip_taskbar(false)
        .visible(false)
        .build()?;
    if let Ok(url) = login_window.url() {
        log::info!(
            "[auth] login initial url after build={}",
            safe_url_for_log(&url)
        );
    }
    log::info!(
        "[LOGIN PERF #{request_id}] WebviewWindowBuilder build: {:.1}ms",
        build_started.elapsed().as_secs_f64() * 1000.0
    );

    // AppBar와 같은 모니터의 전체 영역을 기준으로 로그인 창을 중앙 배치한다.
    let position_started = Instant::now();
    center_login_window(app, &login_window)?;
    log::info!(
        "[LOGIN PERF #{request_id}] center/set_position: {:.1}ms",
        position_started.elapsed().as_secs_f64() * 1000.0
    );
    log::info!(
        "[LOGIN PERF #{request_id}] total show_login_window (build returned): {:.1}ms",
        total_started.elapsed().as_secs_f64() * 1000.0
    );

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let build_start_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    eprintln!("[{build_start_ms}] MAIN_BUILD_START source=tauri.conf");
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            notify_login_page_ready,
            close_login_window,
            minimize_login_window,
            begin_access_login,
            log_login_viewport,
            log_login_diagnostic,
            show_login_window,
            toggle_profile_popup,
            hide_profile_popup,
            confirm_access_logout,
            begin_access_logout,
            open_web_app
        ])
        .on_page_load(handle_page_load)
        .setup(|app| {
            /*
             * 개발 모드 로그
             */
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(log::LevelFilter::Info)
                    .targets([
                        Target::new(TargetKind::LogDir {
                            file_name: Some("mona-hub".into()),
                        }),
                        Target::new(TargetKind::Stdout),
                    ])
                    .build(),
            )?;

            log::info!("MONA-HUB startup");

            // `main` is config-generated, so Tauri has completed its native/WebView
            // build before setup is entered. MAIN_BUILD_START is emitted immediately
            // before Tauri run; this is the first post-build callback.
            if let Some(main) = app.get_webview_window("main") {
                log::info!("MAIN_BUILD_DONE source=tauri.conf");
                log::info!("MAIN_HWND_READY");
                log::info!(
                    "MAIN_VISIBLE_{}",
                    if main.is_visible().unwrap_or(false) {
                        "TRUE"
                    } else {
                        "FALSE"
                    }
                );
                #[cfg(target_os = "windows")]
                appbar::log_window_state(&main, "MAIN_HWND_READY_STATE");
            }

            // WebView IPC command 안에서 새 WebView를 동기 생성하면 Windows에서
            // 생성 완료를 기다리며 교착될 수 있으므로 hidden popup을 setup에서 준비한다.
            let popup = profile_popup(app.handle())?;
            log::info!(
                "[profile-popup] setup ready: visible={:?}, outer_size={:?}",
                popup.is_visible(),
                popup.outer_size()
            );

            /*
             * Windows AppBar 등록
             */
            #[cfg(target_os = "windows")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    match appbar::register_and_show(&window) {
                        Ok(()) => {
                            appbar::log_window_state(&window, "MAIN_NATIVE_SHOW_DONE");
                        }
                        Err(error) => {
                            eprintln!("AppBar 등록 실패: {error}");
                        }
                    }
                } else {
                    eprintln!("main 창을 찾을 수 없습니다.");
                }
            }

            set_auth_state(app.handle(), AUTH_CHECKING_SESSION);
            show_or_create_login_window(app.handle(), "startup")?;

            /*
             * 트레이 메뉴
             */
            let open_item = MenuItem::with_id(app, "open", "MONA-HUB 열기", true, None::<&str>)?;

            let login_item = MenuItem::with_id(app, "login", "로그인", true, None::<&str>)?;
            app.manage(TrayAuthMenuItem(login_item.clone()));
            sync_tray_auth_menu(app.handle(), AUTH_FLOW_STATE.load(Ordering::Acquire));

            let help_item = MenuItem::with_id(app, "help", "도움말", true, None::<&str>)?;

            let quit_item = MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;

            let tray_menu =
                Menu::with_items(app, &[&open_item, &login_item, &help_item, &quit_item])?;

            /*
             * 시스템 트레이
             */
            TrayIconBuilder::new()
                .icon(
                    app.default_window_icon()
                        .expect("default window icon not found")
                        .clone(),
                )
                .tooltip("MONA-HUB")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    /*
                     * MONA-HUB 열기
                     */
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            #[cfg(target_os = "windows")]
                            {
                                if let Err(error) = appbar::register(&window) {
                                    eprintln!("AppBar 재등록 실패: {error}");
                                }
                            }

                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }

                    /*
                     * 로그인 창 열기
                     */
                    "login" => {
                        let state = AUTH_FLOW_STATE.load(Ordering::Acquire);
                        if matches!(
                            state,
                            AUTHENTICATED | AUTH_LOGGING_OUT_CLOUDFLARE | AUTH_LOGGING_OUT_ENTRA
                        ) {
                            if state == AUTHENTICATED {
                                if let Some(main) = app.get_webview_window("main") {
                                    if let Err(error) = begin_access_logout(main) {
                                        log::error!("[auth] tray logout failed: {error}");
                                    }
                                }
                            } else {
                                log::info!(
                                    "[auth] tray logout ignored; logout already in progress"
                                );
                            }
                        } else if let Err(error) = request_login(app, "tray") {
                            log::error!("[auth] tray login failed: {error}");
                        }
                    }

                    /*
                     * 도움말
                     */
                    "help" => {
                        if let Some(window) = app.get_webview_window("help") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                            return;
                        }

                        let help_url = "https://mona-hub.pages.dev/help/"
                            .parse()
                            .expect("invalid help URL");

                        if let Err(error) =
                            WebviewWindowBuilder::new(app, "help", WebviewUrl::External(help_url))
                                .title("MONA-HUB 도움말")
                                .inner_size(900.0, 700.0)
                                .min_inner_size(640.0, 480.0)
                                .resizable(true)
                                .center()
                                .build()
                        {
                            eprintln!("도움말 창 생성 실패: {error}");
                        }
                    }

                    /*
                     * 완전 종료
                     */
                    "quit" => {
                        log::info!("tray quit requested");

                        #[cfg(target_os = "windows")]
                        {
                            if let Some(window) = app.get_webview_window("main") {
                                if let Err(error) = appbar::unregister(&window) {
                                    eprintln!("AppBar 해제 실패: {error}");
                                }
                            }
                        }

                        app.exit(0);
                    }

                    _ => {}
                })
                /*
                 * 트레이 아이콘 왼쪽 클릭
                 */
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();

                        if let Some(window) = app.get_webview_window("main") {
                            #[cfg(target_os = "windows")]
                            {
                                if let Err(error) = appbar::register(&window) {
                                    eprintln!("AppBar 재등록 실패: {error}");
                                }
                            }

                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        /*
         * 창 닫기 처리
         */
        .on_window_event(|window, event| {
            if window.label() == PROFILE_POPUP_LABEL {
                if let WindowEvent::Focused(false) = event {
                    if !window.is_visible().unwrap_or(false) {
                        log::info!("[profile-popup] ignored blur while hidden");
                        return;
                    }
                    if let Ok(mut blurred_at) = PROFILE_POPUP_BLURRED_AT
                        .get_or_init(|| Mutex::new(None))
                        .lock()
                    {
                        *blurred_at = Some(Instant::now());
                    }
                    let result = window.hide();
                    log::info!("[profile-popup] focus lost; hide result = {result:?}");
                }
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == LOGIN_WINDOW_LABEL {
                    api.prevent_close();
                    if let Some(login) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL)
                    {
                        handle_login_window_close(&login);
                    }
                    let _ = window.hide();
                    return;
                }

                /*
                 * main 창만 트레이로 숨긴다.
                 * 도움말 창은 정상적으로 닫히게 둔다.
                 */
                if window.label() == "main" {
                    api.prevent_close();

                    #[cfg(target_os = "windows")]
                    {
                        if let Some(webview_window) = window.app_handle().get_webview_window("main")
                        {
                            if let Err(error) = appbar::unregister(&webview_window) {
                                eprintln!("AppBar 해제 실패: {error}");
                            }
                        }
                    }

                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
