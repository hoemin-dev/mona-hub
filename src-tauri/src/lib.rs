use std::{
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Mutex, OnceLock,
    },
    time::Instant,
};
use tauri::webview::PageLoadEvent;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_log::{Target, TargetKind};

#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
#[cfg(target_os = "windows")]
use std::mem::size_of;
#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    UI::HiDpi::GetDpiForWindow,
};

#[cfg(target_os = "windows")]
mod appbar;

const LOGIN_WINDOW_LABEL: &str = "login";
const PROFILE_POPUP_LABEL: &str = "profile-popup";
const ACCESS_APP_URL: &str = "https://mona-hub.pages.dev/app/";
const PRELOGIN_URL: &str = "https://mona-hub.pages.dev/prelogin/";
const AUTH_WINDOW_MARGIN_LOGICAL: f64 = 12.0;
const DEFAULT_DPI: u32 = 96;
const AUTH_IDLE: u8 = 0;
const AUTH_WAITING_FOR_LOGIN: u8 = 1;
const AUTH_WAITING_FOR_MAIN: u8 = 2;
const AUTHENTICATED: u8 = 3;
const AUTH_LOGOUT_NAVIGATING: u8 = 4;
const AUTH_VERIFYING_LOGOUT: u8 = 5;
static LOGIN_REQUEST_ID: AtomicU64 = AtomicU64::new(0);
static AUTH_FLOW_STATE: AtomicU8 = AtomicU8::new(AUTH_IDLE);
static LOGIN_PAGE_LOAD: OnceLock<Mutex<Option<(u64, Instant)>>> = OnceLock::new();
static LOGIN_START_URL: OnceLock<Mutex<Option<Url>>> = OnceLock::new();
static PROFILE_POPUP_BLURRED_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn login_page_load() -> &'static Mutex<Option<(u64, Instant)>> {
    LOGIN_PAGE_LOAD.get_or_init(|| Mutex::new(None))
}

fn login_start_url() -> &'static Mutex<Option<Url>> {
    LOGIN_START_URL.get_or_init(|| Mutex::new(None))
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

    log::info!("[LOGIN] close requested: hide existing window");
    window.hide().map_err(|error| error.to_string())
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

    AUTH_FLOW_STATE.store(AUTH_WAITING_FOR_LOGIN, Ordering::Release);
    log::info!("[ACCESS] login started in login WebView: {ACCESS_APP_URL}");
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

fn is_access_app_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("mona-hub.pages.dev")
        && (url.path() == "/app" || url.path().starts_with("/app/"))
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

#[cfg(target_os = "windows")]
fn native_hwnd(window: &WebviewWindow) -> Result<HWND, String> {
    let handle = window
        .window_handle()
        .map_err(|error| format!("윈도우 핸들을 얻지 못했습니다: {error}"))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut std::ffi::c_void)),
        _ => Err("Windows HWND가 아닙니다.".into()),
    }
}

#[cfg(target_os = "windows")]
fn expand_auth_window_to_work_area(
    app: &AppHandle,
    login_window: &WebviewWindow,
    stage: &str,
) -> Result<(), String> {
    let monitor_window = app
        .get_webview_window("main")
        .unwrap_or_else(|| login_window.clone());
    let hwnd = native_hwnd(&monitor_window)?;

    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("모니터 작업영역을 얻지 못했습니다.".into());
        }

        let dpi = match GetDpiForWindow(hwnd) {
            0 => DEFAULT_DPI,
            value => value,
        };
        let scale = dpi as f64 / DEFAULT_DPI as f64;
        let margin = (AUTH_WINDOW_MARGIN_LOGICAL * scale).round() as i32;
        let rc_work = info.rcWork;
        let desired_outer_width = (rc_work.right - rc_work.left - margin * 2).max(1) as u32;
        let desired_outer_height = (rc_work.bottom - rc_work.top - margin * 2).max(1) as u32;
        let current_inner = login_window
            .inner_size()
            .map_err(|error| format!("inner-size measurement failed: {error}"))?;
        let current_outer = login_window
            .outer_size()
            .map_err(|error| format!("outer-size measurement failed: {error}"))?;
        let frame_width = current_outer.width.saturating_sub(current_inner.width);
        let frame_height = current_outer.height.saturating_sub(current_inner.height);
        let inner_width = desired_outer_width.saturating_sub(frame_width).max(1);
        let inner_height = desired_outer_height.saturating_sub(frame_height).max(1);
        let x = rc_work.left + margin;
        let y = rc_work.top + margin;

        log::info!(
            "[AUTH WINDOW] stage={stage} monitor_rcWork=({},{})-({},{}) dpi={dpi} scale_factor={scale:.2}",
            rc_work.left, rc_work.top, rc_work.right, rc_work.bottom
        );
        login_window
            .set_size(PhysicalSize::new(inner_width, inner_height))
            .map_err(|error| format!("resize failed: {error}"))?;
        login_window
            .set_position(PhysicalPosition::new(x, y))
            .map_err(|error| format!("position failed: {error}"))?;
    }

    Ok(())
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
    log_login_window_metrics(&login_window, stage, "before-resize");

    #[cfg(target_os = "windows")]
    if let Err(error) = expand_auth_window_to_work_area(window.app_handle(), &login_window, stage) {
        log::error!("[AUTH WINDOW] stage={stage} expansion failed: {error}");
    }
    log_login_window_metrics(&login_window, stage, "after-resize");
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
    if label == PROFILE_POPUP_LABEL {
        log::info!("[profile-popup] page load {event}: {url}");
    }
    log_auth_navigation(label, event, url, state);

    if label == "main"
        && state == AUTH_VERIFYING_LOGOUT
        && payload.event() == PageLoadEvent::Started
    {
        if !is_access_app_url(url)
            && matches!(
                auth_navigation_stage(url),
                "cloudflare-access" | "microsoft-authorize" | "microsoft-authentication"
            )
        {
            log::info!("[ACCESS LOGOUT] protected /app/ requires authentication; logout verified");
            AUTH_FLOW_STATE.store(AUTH_IDLE, Ordering::Release);
            if let Some(popup) = window.app_handle().get_webview_window(PROFILE_POPUP_LABEL) {
                let _ = popup.hide();
            }
            if let Some(login) = window.app_handle().get_webview_window(LOGIN_WINDOW_LABEL) {
                if let Ok(start) = login_start_url().lock() {
                    if let Some(url) = start.clone() {
                        let _ = login.navigate(url);
                    }
                }
                let _ = login.hide();
            }
            let prelogin_url = PRELOGIN_URL.parse().expect("invalid prelogin URL");
            if let Some(main) = window.app_handle().get_webview_window("main") {
                if let Err(error) = main.navigate(prelogin_url) {
                    log::error!("[ACCESS LOGOUT] prelogin navigation failed: {error}");
                }
            }
            return;
        }
    }

    // Resize on navigation start so the external identity UI receives the larger
    // viewport before it completes its first render. No script is injected there.
    if payload.event() == PageLoadEvent::Started {
        ensure_auth_window_size(window, url);
    }

    if payload.event() != PageLoadEvent::Finished {
        return;
    }

    if label == "main" && state == AUTH_LOGOUT_NAVIGATING {
        AUTH_FLOW_STATE.store(AUTH_VERIFYING_LOGOUT, Ordering::Release);
        log::info!("[ACCESS LOGOUT] logout navigation completed; verifying protected /app/");
        if let Some(main) = window.app_handle().get_webview_window("main") {
            let app_url = ACCESS_APP_URL.parse().expect("invalid Access app URL");
            if let Err(error) = main.navigate(app_url) {
                AUTH_FLOW_STATE.store(AUTHENTICATED, Ordering::Release);
                log::error!("[ACCESS LOGOUT] verification navigation failed: {error}");
            }
        }
        return;
    }

    if label == "main" && state == AUTH_VERIFYING_LOGOUT && is_access_app_url(url) {
        AUTH_FLOW_STATE.store(AUTHENTICATED, Ordering::Release);
        log::error!("[ACCESS LOGOUT] verification failed: protected /app/ still loaded without Access authentication");
        return;
    }

    if label == LOGIN_WINDOW_LABEL && state == AUTH_WAITING_FOR_LOGIN && is_access_app_url(url) {
        AUTH_FLOW_STATE.store(AUTH_WAITING_FOR_MAIN, Ordering::Release);
        log::info!(
            "[ACCESS] protected app loaded in login WebView; navigating existing main WebView"
        );

        if let Some(main) = window.app_handle().get_webview_window("main") {
            let app_url = ACCESS_APP_URL.parse().expect("invalid Access app URL");
            if let Err(error) = main.navigate(app_url) {
                AUTH_FLOW_STATE.store(AUTH_WAITING_FOR_LOGIN, Ordering::Release);
                log::error!("[ACCESS] main WebView navigation failed: {error}");
            }
        } else {
            AUTH_FLOW_STATE.store(AUTH_WAITING_FOR_LOGIN, Ordering::Release);
            log::error!("[ACCESS] main WebView not found");
        }
        return;
    }

    if label == "main" && state == AUTH_WAITING_FOR_MAIN && is_access_app_url(url) {
        AUTH_FLOW_STATE.store(AUTHENTICATED, Ordering::Release);
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
    show_or_create_login_window(&app, "prelogin command").map_err(|error| error.to_string())
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
fn begin_access_logout(window: WebviewWindow, logout_url: String) -> Result<(), String> {
    if window.label() != "main" {
        return Err("잘못된 창에서 로그아웃을 요청했습니다.".into());
    }
    let url: Url = logout_url
        .parse()
        .map_err(|_| "잘못된 logout URL입니다.".to_string())?;
    if url.scheme() != "https"
        || url.host_str() != Some("mona-hub.pages.dev")
        || url.path() != "/cdn-cgi/access/logout"
    {
        return Err("현재 Access origin의 logout endpoint가 아닙니다.".into());
    }
    AUTH_FLOW_STATE.store(AUTH_LOGOUT_NAVIGATING, Ordering::Release);
    log::info!(
        "[ACCESS LOGOUT] navigating current main WebView to official Access logout endpoint"
    );
    window.navigate(url).map_err(|error| {
        AUTH_FLOW_STATE.store(AUTHENTICATED, Ordering::Release);
        log::error!("[ACCESS LOGOUT] navigation failed: {error}");
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
        if let Ok(start) = login_start_url().lock() {
            if let Some(url) = start.clone() {
                login_window.navigate(url)?;
                AUTH_FLOW_STATE.store(AUTH_IDLE, Ordering::Release);
                log::info!("[LOGIN] existing window reset to MONA Hub login start page");
            }
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
    let login_window = WebviewWindowBuilder::new(
        app,
        LOGIN_WINDOW_LABEL,
        WebviewUrl::App("login/login.html".into()),
    )
    .title("MONA-HUB 로그인")
    .inner_size(380.0, 400.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(false)
    .skip_taskbar(false)
    .visible(false)
    .build()?;
    if let Ok(mut start) = login_start_url().lock() {
        *start = login_window.url().ok();
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
            begin_access_logout
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
                    if let Err(error) = appbar::register(&window) {
                        eprintln!("AppBar 등록 실패: {error}");
                    }
                } else {
                    eprintln!("main 창을 찾을 수 없습니다.");
                }
            }

            show_or_create_login_window(app.handle(), "startup")?;

            /*
             * 트레이 메뉴
             */
            let open_item = MenuItem::with_id(app, "open", "MONA-HUB 열기", true, None::<&str>)?;

            let login_item = MenuItem::with_id(app, "login", "로그인", true, None::<&str>)?;

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
                        log::info!(
                            "[LOGIN TRAY] event thread={:?}",
                            std::thread::current().id()
                        );
                        if let Err(error) = show_or_create_login_window(app, "tray menu") {
                            eprintln!("로그인 창 표시 실패: {error}");
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
                    log::info!("[LOGIN] native close requested: hide existing window");
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
