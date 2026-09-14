mod popup;

struct ConsoleLogger;
impl log::Log for ConsoleLogger {
    fn enabled(&self, _: &log::Metadata) -> bool { true }
    fn log(&self, record: &log::Record) { eprintln!("{} {}", record.level(), record.args()); }
    fn flush(&self) {}
}
static LOGGER: ConsoleLogger = ConsoleLogger;

fn main() {
    log::set_logger(&LOGGER).expect("console logger");
    log::set_max_level(log::LevelFilter::Info);
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let builder = tauri::WebviewWindowBuilder::new(
                &handle, popup::LABEL,
                tauri::WebviewUrl::External(popup::URL.parse().expect("fixture URL")),
            )
            .title("WebView Test")
            .inner_size(1200.0, 800.0)
            .disable_drag_drop_handler();
            popup::configure(builder, handle.clone(), popup::LABEL.into()).build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("WebView test app failed");
}
