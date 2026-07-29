use tauri::{
  menu::{Menu, MenuItem},
  tray::{
    MouseButton,
    MouseButtonState,
    TrayIconBuilder,
    TrayIconEvent,
  },
  Manager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      let open_item = MenuItem::with_id(
        app,
        "open",
        "MONA-HUB 열기",
        true,
        None::<&str>,
      )?;

      let quit_item = MenuItem::with_id(
        app,
        "quit",
        "종료",
        true,
        None::<&str>,
      )?;

      let tray_menu = Menu::with_items(
        app,
        &[&open_item, &quit_item],
      )?;

      TrayIconBuilder::new()
        .icon(
          app.default_window_icon()
            .expect("default window icon not found")
            .clone(),
        )
        .tooltip("MONA-HUB")
        .menu(&tray_menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
          match event.id().as_ref() {
            "open" => {
              if let Some(window) =
                app.get_webview_window("main")
              {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
              }
            }

            "quit" => {
              app.exit(0);
            }

            _ => {}
          }
        })
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
              let _ = window.unminimize();
              let _ = window.show();
              let _ = window.set_focus();
            }
          }
        })
        .build(app)?;

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}