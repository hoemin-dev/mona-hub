//! Offline native-chrome smoke test: cargo run --example login_footer_preview
//! Uses a local fixture, never Microsoft or a user's authentication session.
#[path = "../src/login_footer.rs"]
mod login_footer;

fn fixture_url() -> tauri::Url {
    tauri::Url::from_file_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/login_footer_preview.html"),
    )
    .unwrap()
}

fn restart_login(window: &tauri::WebviewWindow) -> Result<(), String> {
    let mut url = fixture_url();
    url.set_query(Some("start"));
    window.navigate(url).map_err(|e| e.to_string())?;
    window
        .set_size(tauri::LogicalSize::new(420.0, 500.0))
        .map_err(|e| e.to_string())?;
    login_footer::set_visible(window, false).map_err(|e| e.to_string())
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let _ = std::fs::write(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/footer-preview-error.txt"),
            info.to_string(),
        );
    }));
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .setup(|app| {
            let window = tauri::WebviewWindowBuilder::new(
                app,
                "footer-preview",
                tauri::WebviewUrl::External(fixture_url()),
            )
            .title("MonaHub footer preview (offline)")
            .inner_size(420.0, 500.0 + login_footer::HEIGHT)
            .resizable(false)
            .maximizable(false)
            .build()?;
            login_footer::install(&window)?;
            login_footer::set_visible(&window, true)?;
            Ok(())
        })
        .run(context)
        .expect("preview failed");
}
