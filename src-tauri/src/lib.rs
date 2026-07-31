use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

#[cfg(target_os = "windows")]
mod appbar;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            /*
             * 개발 모드 로그
             */
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            /*
             * Windows AppBar 등록
             */
            #[cfg(target_os = "windows")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    if let Err(error) = appbar::register_collapsed(&window) {
                        eprintln!("AppBar 등록 실패: {error}");
                    }
                } else {
                    eprintln!("main 창을 찾을 수 없습니다.");
                }
            }

            /*
             * 트레이 메뉴
             */
            let open_item =
                MenuItem::with_id(app, "open", "MONA-HUB 열기", true, None::<&str>)?;

            let help_item =
                MenuItem::with_id(app, "help", "도움말", true, None::<&str>)?;

            let quit_item =
                MenuItem::with_id(app, "quit", "종료", true, None::<&str>)?;

            let tray_menu =
                Menu::with_items(app, &[&open_item, &help_item, &quit_item])?;

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
                                if let Err(error) =
                                    appbar::register_collapsed(&window)
                                {
                                    eprintln!("AppBar 재등록 실패: {error}");
                                }
                            }

                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
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

                        if let Err(error) = WebviewWindowBuilder::new(
                            app,
                            "help",
                            WebviewUrl::External(help_url),
                        )
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
                        #[cfg(target_os = "windows")]
                        {
                            if let Some(window) =
                                app.get_webview_window("main")
                            {
                                if let Err(error) =
                                    appbar::unregister(&window)
                                {
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

                        if let Some(window) =
                            app.get_webview_window("main")
                        {
                            #[cfg(target_os = "windows")]
                            {
                                if let Err(error) =
                                    appbar::register_collapsed(&window)
                                {
                                    eprintln!(
                                        "AppBar 재등록 실패: {error}"
                                    );
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
                /*
                 * main 창만 트레이로 숨긴다.
                 * 도움말 창은 정상적으로 닫히게 둔다.
                 */
                if window.label() == "main" {
                    api.prevent_close();

                    #[cfg(target_os = "windows")]
{
    if let Some(webview_window) =
        window.app_handle().get_webview_window("main")
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