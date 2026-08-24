use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
    time::Instant,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use tauri_plugin_log::{Target, TargetKind};

#[cfg(target_os = "windows")]
mod appbar;

const LOGIN_WINDOW_LABEL: &str = "login";
static LOGIN_REQUEST_ID: AtomicU64 = AtomicU64::new(0);
static LOGIN_PAGE_LOAD: OnceLock<Mutex<Option<(u64, Instant)>>> = OnceLock::new();

fn login_page_load() -> &'static Mutex<Option<(u64, Instant)>> {
    LOGIN_PAGE_LOAD.get_or_init(|| Mutex::new(None))
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
fn log_login_diagnostic(message: String) {
    log::info!("{message}");
}

#[tauri::command]
fn show_login_window(app: AppHandle) -> Result<(), String> {
    show_or_create_login_window(&app, "prelogin command").map_err(|error| error.to_string())
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
    log::info!(
        "[LOGIN PERF #{request_id}] WebviewWindowBuilder build: {:.1}ms",
        build_started.elapsed().as_secs_f64() * 1000.0
    );

    // AppBar와 같은 모니터의 전체 영역을 기준으로 로그인 창을 중앙 배치한다.
    let target_monitor = app
        .get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| login_window.current_monitor().ok().flatten());

    let position_started = Instant::now();
    if let Some(monitor) = target_monitor {
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
            log_login_diagnostic,
            show_login_window
        ])
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

                        let help_url = "https://hub.monas.co.kr/help/"
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
