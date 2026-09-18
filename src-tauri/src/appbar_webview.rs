use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
use windows_core_61::Interface;

const APPBAR_WINDOW_LABEL: &str = "main";

/// Remove browser chrome behavior from the AppBar WebView only.
///
/// Login, profile-popup, and business-app WebViews keep WebView2's defaults.
pub fn configure(window: &WebviewWindow) -> tauri::Result<()> {
    if window.label() != APPBAR_WINDOW_LABEL {
        log::warn!(
            "[appbar-webview] refused browser-chrome configuration for label={}",
            window.label()
        );
        return Ok(());
    }

    window.with_webview(|view| unsafe {
        let result = (|| -> windows_core_61::Result<()> {
            let webview = view.controller().CoreWebView2()?;
            let settings = webview.Settings()?;

            // Removes the native right-click menu even before AppBar JavaScript runs.
            settings.SetAreDefaultContextMenusEnabled(false)?;

            // Covers WebView2 browser commands such as F5, Ctrl+R,
            // Ctrl+Shift+R, Ctrl+P, Ctrl+S, find, zoom, and DevTools shortcuts.
            let settings3 = settings.cast::<ICoreWebView2Settings3>()?;
            settings3.SetAreBrowserAcceleratorKeysEnabled(false)?;
            Ok(())
        })();

        match result {
            Ok(()) => log::info!(
                "[appbar-webview] default context menus and browser accelerators disabled"
            ),
            Err(error) => log::error!("[appbar-webview] failed to disable browser chrome: {error}"),
        }
    })
}
