//! Temporary local harness only. Never attach capabilities to these labels.
#[cfg(target_os = "windows")]
use std::sync::{atomic::AtomicBool, Arc};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};
use tauri::{
    webview::{DownloadEvent, NewWindowResponse},
    AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent, Wry,
};

pub const URL: &str = "http://127.0.0.1:8088/";
pub const LABEL: &str = "webapp-popup-test";
const CROSS_URL: &str = "https://example.com/";
static NEXT: AtomicU64 = AtomicU64::new(1);
static REGISTRY: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

#[cfg(target_os = "windows")]
fn attach_window_close_requested(window: &tauri::WebviewWindow<Wry>) {
    use webview2_com::WindowCloseRequestedEventHandler;

    let app = window.app_handle().clone();
    let label = window.label().to_owned();
    debug_assert!(label.starts_with("popup-test-popup-"));
    let close_dispatched = Arc::new(AtomicBool::new(false));

    let hook_label = label.clone();
    if let Err(error) = window.with_webview(move |platform_webview| {
        let webview = match unsafe { platform_webview.controller().CoreWebView2() } {
            Ok(webview) => webview,
            Err(error) => {
                log::error!(
                    "[popup-test] close adapter label={} install=false reason=core-webview2 error={error}",
                    hook_label
                );
                return;
            }
        };
        let callback_label = hook_label.clone();
        let callback_app = app.clone();
        let callback_guard = close_dispatched.clone();
        let handler = WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
            if callback_guard.swap(true, Ordering::AcqRel) {
                return Ok(());
            }
            log::info!(
                "[popup-test] webview close requested label={} source=webview2",
                callback_label
            );
            let dispatch_app = callback_app.clone();
            let dispatch_label = callback_label.clone();
            if let Err(error) = callback_app.run_on_main_thread(move || {
                let Some(popup) = dispatch_app.get_webview_window(&dispatch_label) else {
                    return;
                };
                if let Err(error) = popup.close() {
                    log::warn!(
                        "[popup-test] webview close ignored label={} reason=tauri-close-failed error={error}",
                        dispatch_label
                    );
                }
            }) {
                log::warn!(
                    "[popup-test] webview close ignored label={} reason=dispatch-failed error={error}",
                    callback_label
                );
            }
            Ok(())
        }));
        let mut token = 0;
        if let Err(error) = unsafe { webview.add_WindowCloseRequested(&handler, &mut token) } {
            log::error!(
                "[popup-test] close adapter label={} install=false reason=event-hook error={error}",
                hook_label
            );
        }
    }) {
        log::error!(
            "[popup-test] close adapter label={} install=false reason=webview-dispatch error={error}",
            label
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_window_close_requested(_: &tauri::WebviewWindow<Wry>) {}

fn registry() -> &'static Mutex<HashSet<String>> {
    REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

fn allowed(url: &Url) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    (url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port_or_known_default() == Some(8088))
        || url.as_str() == "about:blank"
        || (url.scheme() == "https"
            && url.host_str() == Some("example.com")
            && url.port_or_known_default() == Some(443)
            && url.as_str() == CROSS_URL)
}

// Never emit credentials, query, fragment, arbitrary paths or opaque URL payloads.
fn safe_url(url: &Url) -> String {
    if url.as_str() == "about:blank" {
        return "about:blank".into();
    }
    if matches!(url.scheme(), "http" | "https") {
        format!(
            "{}/{} query={} fragment={}",
            url.origin().ascii_serialization(),
            if url.path() == "/" {
                ""
            } else {
                "[path-redacted]"
            },
            url.query().is_some(),
            url.fragment().is_some()
        )
    } else {
        format!("{}:[redacted]", url.scheme())
    }
}

pub fn configure<'a>(
    builder: WebviewWindowBuilder<'a, Wry, AppHandle>,
    app: AppHandle,
    parent: String,
) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
    let navigation_label = parent.clone();
    let title_label = parent.clone();
    builder
        .on_navigation(move |url| {
            let accept = allowed(url);
            log::info!("[popup-test] navigation label={} url={} decision={}",
                navigation_label, safe_url(url), if accept { "Allow" } else { "Deny" });
            accept
        })
        .on_document_title_changed(move |window, title| {
            // Diagnostic-only title markers, not IPC commands. Only log a fixed vocabulary.
            let Some(event) = title.strip_prefix("popup-test:").and_then(|s| s.split(':').next()) else { return; };
            if !matches!(event, "ready" | "close-requested" | "pagehide" | "postMessage"
                | "drag-drop" | "file-input" | "download" | "nested-popup") { return; }
            log::info!("[popup-test] page-signal label={} event={} (untrusted diagnostic)", title_label, event);
            if event == "close-requested" {
                let app = window.app_handle().clone();
                let label = title_label.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    let registered = registry().lock().map(|r| r.contains(&label)).unwrap_or(false);
                    log::info!("[popup-test] close-observation label={} tauri_webview_window_present={} registry_present={} native_webview_closed=UNOBSERVABLE",
                        label, app.get_webview_window(&label).is_some(), registered);
                });
            }
        })
        .on_download(|webview, event| {
            match event {
                DownloadEvent::Requested { url, .. } => log::info!(
                    "[popup-test] download requested label={} url={}", webview.label(), safe_url(&url)),
                DownloadEvent::Finished { success, .. } => log::info!(
                    "[popup-test] download finished label={} success={success}", webview.label()),
                _ => {}
            }
            true
        })
        .on_new_window(move |url, features| {
            let label = format!("popup-test-popup-{}", NEXT.fetch_add(1, Ordering::Relaxed));
            let nested = parent != LABEL;
            if !allowed(&url) {
                log::warn!("[popup-test] popup request parent={} popup={} nested={} url={} decision=Deny",
                    parent, label, nested, safe_url(&url));
                return NewWindowResponse::Deny;
            }
            log::info!("[popup-test] popup request parent={} popup={} nested={} url={} policy=Allow build=pending",
                parent, label, nested, safe_url(&url));
            let builder = WebviewWindowBuilder::new(&app, &label,
                WebviewUrl::External(Url::parse("about:blank").expect("constant URL")))
                .title("Popup Test — child")
                .window_features(features)
                .disable_drag_drop_handler();
            // Same handler at every depth; Create preserves the browser's opener relationship.
            let window = match configure(builder, app.clone(), label.clone()).build() {
                Ok(window) => window,
                Err(_) => {
                    log::error!("[popup-test] popup request parent={} popup={} url={} decision=Deny reason=build-failed",
                        parent, label, safe_url(&url));
                    return NewWindowResponse::Deny;
                }
            };
            registry().lock().expect("popup registry").insert(label.clone());
            let event_label = label.clone();
            window.on_window_event(move |event| match event {
                WindowEvent::CloseRequested { .. } => log::info!(
                    "[popup-test] popup close requested label={} source=tauri-window", event_label),
                WindowEvent::Destroyed => {
                    let removed = registry().lock().map(|mut r| r.remove(&event_label)).unwrap_or(false);
                    log::info!("[popup-test] popup destroyed label={} registry_removed={}", event_label, removed);
                }
                _ => {}
            });
            attach_window_close_requested(&window);
            log::info!("[popup-test] popup request parent={} popup={} nested={} url={} decision=Create popup-created=true registry_inserted=true",
                parent, label, nested, safe_url(&url));
            NewWindowResponse::Create { window }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn popup_url_policy() {
        for value in [
            URL,
            "http://127.0.0.1:8088/popup.html?mode=child#test",
            "about:blank",
            CROSS_URL,
        ] {
            assert!(allowed(&Url::parse(value).unwrap()), "{value}");
        }
        for value in [
            "file:///C:/test.txt",
            "javascript:alert(1)",
            "data:text/html,test",
            "custom://test",
            "http://localhost:8088/",
            "http://127.0.0.1:8089/",
            "http://127.0.0.1/",
            "http://192.168.1.1:8088/",
            "https://127.0.0.1:8088/",
            "https://example.com/other",
            "https://example.com/?secret=1",
            "https://example.com/#x",
            "https://example.com:444/",
            "https://example.com.evil.test/",
            "https://other.example/",
            "http://user:secret@127.0.0.1:8088/",
            "about:blank?x",
            "about:srcdoc",
        ] {
            assert!(!allowed(&Url::parse(value).unwrap()), "{value}");
        }
    }
    #[test]
    fn exact_launch_pair_and_caller() {
        let caller = Url::parse("https://mona-hub.pages.dev/app/").unwrap();
        assert!(crate::validate_web_app_request("main", &caller, "popup-test", URL).is_ok());
        for (id, url) in [
            ("pdfys", URL),
            ("popup-test", crate::PDFYS_URL),
            ("popup-test", "http://localhost:8088/"),
            ("popup-test", "http://127.0.0.1:8088"),
            ("popup-test", "http://127.0.0.1:8088/?x=1"),
            ("popup-test", "http://127.0.0.1:8089/"),
        ] {
            assert!(crate::validate_web_app_request("main", &caller, id, url).is_err());
        }
        assert!(crate::validate_web_app_request(LABEL, &caller, "popup-test", URL).is_err());
        assert!(crate::validate_web_app_request(
            "main",
            &Url::parse(URL).unwrap(),
            "popup-test",
            URL
        )
        .is_err());
    }
    #[test]
    fn diagnostic_redaction() {
        for url in [
            "http://user:secret@127.0.0.1:8088/secret?secret#secret",
            "data:text/plain,secret",
        ] {
            assert!(!safe_url(&Url::parse(url).unwrap()).contains("secret"));
        }
    }
    #[test]
    fn harness_has_no_capabilities() {
        for source in [
            include_str!("../capabilities/default.json"),
            include_str!("../capabilities/pdfys-launcher.json"),
            include_str!("../capabilities/remote-auth.json"),
            include_str!("../capabilities/remote-login.json"),
        ] {
            let cap: serde_json::Value = serde_json::from_str(source).unwrap();
            assert!(cap.get("webviews").is_none());
            for window in cap["windows"].as_array().unwrap() {
                assert!(["main", "login", "profile-popup"].contains(&window.as_str().unwrap()));
            }
        }
    }
}
