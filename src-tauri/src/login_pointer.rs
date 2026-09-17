//! Synchronize the windowed WebView2's pointer state across minimization.
//! Do not move the system cursor or change the page's DOM/CSS.
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::WebviewWindow;
use windows::{
    core::BOOL,
    Win32::{
        Foundation::{HWND, LPARAM, POINT, WPARAM},
        Graphics::Gdi::ScreenToClient,
        UI::WindowsAndMessaging::*,
    },
};

const WM_MOUSELEAVE: u32 = 0x02a3;
static LOCAL_LOGIN_MINIMIZED: AtomicBool = AtomicBool::new(false);

pub fn reset() {
    LOCAL_LOGIN_MINIMIZED.store(false, Ordering::Release);
}

fn is_local_login(window: &WebviewWindow) -> bool {
    window.url().is_ok_and(|url| super::is_login_start_url(&url))
}

pub fn on_resize(window: &WebviewWindow) {
    // The login HWND also hosts Cloudflare/Entra pages. Never inject pointer
    // messages into those pages, even if navigation happened while minimized.
    if !is_local_login(window) {
        reset();
        return;
    }
    if window.is_minimized().unwrap_or(false) {
        LOCAL_LOGIN_MINIMIZED.store(true, Ordering::Release);
        if let Ok(hwnd) = window.hwnd() {
            unsafe { synchronize(HWND(hwnd.0), false) };
        }
    } else if LOCAL_LOGIN_MINIMIZED.swap(false, Ordering::AcqRel) {
        // Leave the resize callback and let the normal WebView layout finish.
        let window = window.clone();
        tauri::async_runtime::spawn(async move {
            let target = window.clone();
            if let Err(error) = window.run_on_main_thread(move || {
                if is_local_login(&target) && !target.is_minimized().unwrap_or(true) {
                    if let Ok(hwnd) = target.hwnd() {
                        unsafe { synchronize(HWND(hwnd.0), true) };
                    }
                }
            }) {
                log::warn!("[login-pointer] restore dispatch failed: {error}");
            }
        });
    }
}

unsafe extern "system" fn collect_renderer(hwnd: HWND, data: LPARAM) -> BOOL {
    let mut class = [0u16; 256];
    let len = GetClassNameW(hwnd, &mut class).max(0) as usize;
    // This is Chromium's native input window, not Tauri's top-level HWND.
    // The prefix also accepts WebView2's optional unique-class suffix.
    if String::from_utf16_lossy(&class[..len]).starts_with("Chrome_RenderWidgetHostHWND") {
        (*(data.0 as *mut Vec<HWND>)).push(hwnd);
    }
    BOOL(1)
}

/// UI-thread only. Rediscover HWNDs each time; navigation can replace them.
unsafe fn synchronize(parent: HWND, restoring: bool) {
    let mut renderers = Vec::<HWND>::new();
    let _ = EnumChildWindows(
        Some(parent),
        Some(collect_renderer),
        LPARAM(&mut renderers as *mut Vec<HWND> as isize),
    );
    if renderers.is_empty() {
        log::warn!("[login-pointer] WebView2 input HWND not found");
        return;
    }
    let mut cursor = POINT::default();
    let cursor_known = restoring && GetCursorPos(&mut cursor).is_ok();
    let under_cursor = if cursor_known { WindowFromPoint(cursor) } else { HWND::default() };
    for hwnd in renderers {
        let inside = cursor_known
            && IsWindowVisible(hwnd).as_bool()
            && (under_cursor == hwnd || IsChild(hwnd, under_cursor).as_bool());
        let (message, position) = if inside {
            let mut local = cursor;
            if !ScreenToClient(hwnd, &mut local).as_bool() {
                continue;
            }
            (WM_MOUSEMOVE, LPARAM(((local.x as u16 as u32)
                | ((local.y as u16 as u32) << 16)) as isize))
        } else {
            (WM_MOUSELEAVE, LPARAM(0))
        };
        // Asynchronous native input avoids blocking/reentering the renderer.
        if let Err(error) = PostMessageW(Some(hwnd), message, WPARAM(0), position) {
            log::warn!("[login-pointer] pointer synchronization failed: {error}");
        }
    }
}
