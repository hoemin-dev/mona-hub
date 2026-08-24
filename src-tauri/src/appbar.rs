#![cfg(target_os = "windows")]

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    mem::{size_of, transmute},
    sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering},
};
use tauri::WebviewWindow;
use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{
                SHAppBarMessage, ABE_RIGHT, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
                ABM_WINDOWPOSCHANGED, ABN_POSCHANGED, APPBARDATA,
            },
            WindowsAndMessaging::{
                CallWindowProcW, GetClientRect, GetWindowLongPtrW, GetWindowRect, PostMessageW,
                RegisterWindowMessageW, SetWindowLongPtrW, SetWindowPos, GWLP_WNDPROC, GWL_EXSTYLE,
                SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
                WM_ACTIVATE, WM_DESTROY, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WM_DPICHANGED,
                WM_SETTINGCHANGE, WM_WINDOWPOSCHANGED, WNDPROC, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
            },
        },
    },
};

pub const LOGICAL_WIDTH: i32 = 36;
const DEFAULT_DPI: u32 = 96;
const WM_MONA_REPOSITION: u32 = 0x8000 + 0x04d;

static APPBAR_HWND: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static CALLBACK_MESSAGE: AtomicU32 = AtomicU32::new(0);
static REGISTERED: AtomicBool = AtomicBool::new(false);
static REPOSITION_QUEUED: AtomicBool = AtomicBool::new(false);
static POSITIONING: AtomicBool = AtomicBool::new(false);
static CALLBACK_COUNT: AtomicU32 = AtomicU32::new(0);
static REPOSITION_COUNT: AtomicU32 = AtomicU32::new(0);
static ACTIVATE_COUNT: AtomicU32 = AtomicU32::new(0);
static WINDOWPOS_COUNT: AtomicU32 = AtomicU32::new(0);
static QUERY_COUNT: AtomicU32 = AtomicU32::new(0);
static SETPOS_COUNT: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
pub struct DiagnosticCounts {
    pub callback: u32,
    pub reposition: u32,
    pub activate: u32,
    pub windowpos: u32,
    pub query: u32,
    pub setpos: u32,
}

pub fn diagnostic_counts() -> DiagnosticCounts {
    DiagnosticCounts {
        callback: CALLBACK_COUNT.load(Ordering::Relaxed),
        reposition: REPOSITION_COUNT.load(Ordering::Relaxed),
        activate: ACTIVATE_COUNT.load(Ordering::Relaxed),
        windowpos: WINDOWPOS_COUNT.load(Ordering::Relaxed),
        query: QUERY_COUNT.load(Ordering::Relaxed),
        setpos: SETPOS_COUNT.load(Ordering::Relaxed),
    }
}

fn hwnd_value(hwnd: HWND) -> isize {
    hwnd.0 as isize
}
fn owns(hwnd: HWND) -> bool {
    APPBAR_HWND.load(Ordering::SeqCst) == hwnd_value(hwnd)
}

fn hwnd(window: &WebviewWindow) -> Result<HWND, String> {
    let handle = window
        .window_handle()
        .map_err(|e| format!("윈도우 핸들을 얻지 못했습니다: {e}"))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut std::ffi::c_void)),
        _ => Err("Windows HWND가 아닙니다.".into()),
    }
}

fn data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        ..Default::default()
    }
}

fn monitor_rect(hwnd: HWND) -> Result<RECT, String> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("모니터 정보를 얻지 못했습니다.".into());
        }
        Ok(info.rcMonitor)
    }
}

fn window_dpi(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        DEFAULT_DPI
    } else {
        dpi
    }
}

fn physical_width(dpi: u32) -> i32 {
    ((i64::from(LOGICAL_WIDTH) * i64::from(dpi) + i64::from(DEFAULT_DPI / 2))
        / i64::from(DEFAULT_DPI)) as i32
}

fn rect_text(r: RECT) -> String {
    format!(
        "({}, {})-({}, {}) [{}x{}]",
        r.left,
        r.top,
        r.right,
        r.bottom,
        r.right - r.left,
        r.bottom - r.top
    )
}

fn negotiate(hwnd: HWND) -> Result<(), String> {
    if !REGISTERED.load(Ordering::SeqCst) || !owns(hwnd) || POSITIONING.swap(true, Ordering::SeqCst)
    {
        return Ok(());
    }
    let result = (|| unsafe {
        let monitor = monitor_rect(hwnd)?;
        let dpi = window_dpi(hwnd);
        let width = physical_width(dpi);
        let mut bar = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uEdge: ABE_RIGHT,
            rc: RECT {
                left: monitor.right - width,
                top: monitor.top,
                right: monitor.right,
                bottom: monitor.bottom,
            },
            ..Default::default()
        };
        QUERY_COUNT.fetch_add(1, Ordering::Relaxed);
        SHAppBarMessage(ABM_QUERYPOS, &mut bar);
        let queried = bar.rc;
        bar.rc.left = bar.rc.right - width;
        SETPOS_COUNT.fetch_add(1, Ordering::Relaxed);
        if SHAppBarMessage(ABM_SETPOS, &mut bar) == 0 {
            return Err("Windows AppBar 위치 예약에 실패했습니다.".into());
        }
        let final_rect = bar.rc;
        let (w, h) = (
            final_rect.right - final_rect.left,
            final_rect.bottom - final_rect.top,
        );
        if w <= 0 || h <= 0 {
            return Err(format!("잘못된 AppBar 영역: {}", rect_text(final_rect)));
        }
        SetWindowPos(
            hwnd,
            None,
            final_rect.left,
            final_rect.top,
            w,
            h,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
        .map_err(|e| format!("AppBar 창 이동 실패: {e}"))?;

        #[cfg(debug_assertions)]
        {
            let mut wr = RECT::default();
            let mut cr = RECT::default();
            let _ = GetWindowRect(hwnd, &mut wr);
            let _ = GetClientRect(hwnd, &mut cr);
            log::info!("AppBar measurement: monitor={}, dpi={}, scale_factor={:.2}, logical_width={}, physical_width={}, query={}, setpos={}, window={}, client={}x{}", rect_text(monitor), dpi, dpi as f64/DEFAULT_DPI as f64, LOGICAL_WIDTH, width, rect_text(queried), rect_text(final_rect), rect_text(wr), cr.right-cr.left, cr.bottom-cr.top);
        }
        Ok(())
    })();
    POSITIONING.store(false, Ordering::SeqCst);
    result
}

fn queue_reposition(hwnd: HWND) {
    if REGISTERED.load(Ordering::SeqCst)
        && owns(hwnd)
        && !REPOSITION_QUEUED.swap(true, Ordering::SeqCst)
    {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_MONA_REPOSITION, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn call_original(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let original = ORIGINAL_WNDPROC.load(Ordering::SeqCst);
    if original == 0 {
        return LRESULT(0);
    }
    CallWindowProcW(transmute::<isize, WNDPROC>(original), hwnd, msg, wp, lp)
}

unsafe extern "system" fn appbar_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let callback = CALLBACK_MESSAGE.load(Ordering::SeqCst);
    if callback != 0 && msg == callback {
        let count = CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        log::info!(
            "[APPBAR MSG] callback notification={} count={} thread={:?}",
            wp.0,
            count,
            std::thread::current().id()
        );
        if wp.0 as u32 == ABN_POSCHANGED {
            queue_reposition(hwnd);
        }
        return LRESULT(0);
    }
    match msg {
        WM_MONA_REPOSITION => {
            let count = REPOSITION_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            log::info!(
                "[APPBAR MSG] reposition count={} thread={:?}",
                count,
                std::thread::current().id()
            );
            REPOSITION_QUEUED.store(false, Ordering::SeqCst);
            if let Err(e) = negotiate(hwnd) {
                log::error!("AppBar 재협상 실패: {e}");
            }
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE | WM_DEVICECHANGE => {
            queue_reposition(hwnd)
        }
        WM_ACTIVATE if REGISTERED.load(Ordering::SeqCst) && owns(hwnd) => {
            let count = ACTIVATE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            log::info!(
                "[APPBAR MSG] WM_ACTIVATE state={} count={} thread={:?} (ABM_ACTIVATE suppressed)",
                wp.0 & 0xffff,
                count,
                std::thread::current().id()
            );
        }
        WM_WINDOWPOSCHANGED
            if REGISTERED.load(Ordering::SeqCst)
                && owns(hwnd)
                && !POSITIONING.load(Ordering::SeqCst) =>
        {
            let count = WINDOWPOS_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            log::info!(
                "[APPBAR MSG] WM_WINDOWPOSCHANGED count={} thread={:?}",
                count,
                std::thread::current().id()
            );
            let mut bar = data(hwnd);
            SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut bar);
        }
        WM_DESTROY if REGISTERED.swap(false, Ordering::SeqCst) && owns(hwnd) => {
            let mut bar = data(hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut bar);
            APPBAR_HWND.store(0, Ordering::SeqCst);
            CALLBACK_MESSAGE.store(0, Ordering::SeqCst);
        }
        _ => {}
    }
    call_original(hwnd, msg, wp, lp)
}

fn install_wndproc(hwnd: HWND) -> Result<(), String> {
    if ORIGINAL_WNDPROC.load(Ordering::SeqCst) != 0 {
        return Ok(());
    }
    unsafe {
        let old = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, appbar_wndproc as *const () as isize);
        if old == 0 {
            return Err("AppBar Window Procedure 설치에 실패했습니다.".into());
        }
        ORIGINAL_WNDPROC.store(old, Ordering::SeqCst);
    }
    Ok(())
}

fn restore_wndproc(hwnd: HWND) -> Result<(), String> {
    let old = ORIGINAL_WNDPROC.swap(0, Ordering::SeqCst);
    if old == 0 {
        return Ok(());
    }
    unsafe {
        if SetWindowLongPtrW(hwnd, GWLP_WNDPROC, old) == 0 {
            ORIGINAL_WNDPROC.store(old, Ordering::SeqCst);
            return Err("기존 Window Procedure 복원에 실패했습니다.".into());
        }
    }
    Ok(())
}

fn apply_tool_style(hwnd: HWND) -> Result<(), String> {
    unsafe {
        let style = (GetWindowLongPtrW(hwnd, GWL_EXSTYLE) | WS_EX_TOOLWINDOW.0 as isize)
            & !(WS_EX_APPWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        )
        .map_err(|e| format!("Tool window 스타일 적용 실패: {e}"))?;
    }
    Ok(())
}

pub fn register(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = hwnd(window)?;
    apply_tool_style(hwnd)?;
    if REGISTERED.load(Ordering::SeqCst) {
        return if owns(hwnd) {
            negotiate(hwnd)
        } else {
            Err("다른 AppBar 창이 이미 등록되어 있습니다.".into())
        };
    }
    let callback = unsafe { RegisterWindowMessageW(w!("MONA_HUB_APPBAR_CALLBACK")) };
    if callback == 0 {
        return Err("AppBar 콜백 메시지 등록에 실패했습니다.".into());
    }
    APPBAR_HWND.store(hwnd_value(hwnd), Ordering::SeqCst);
    CALLBACK_MESSAGE.store(callback, Ordering::SeqCst);
    if let Err(e) = install_wndproc(hwnd) {
        APPBAR_HWND.store(0, Ordering::SeqCst);
        CALLBACK_MESSAGE.store(0, Ordering::SeqCst);
        return Err(e);
    }
    unsafe {
        let mut bar = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: callback,
            ..Default::default()
        };
        if SHAppBarMessage(ABM_NEW, &mut bar) == 0 {
            let _ = restore_wndproc(hwnd);
            APPBAR_HWND.store(0, Ordering::SeqCst);
            CALLBACK_MESSAGE.store(0, Ordering::SeqCst);
            return Err("Windows Shell에 AppBar 등록을 실패했습니다.".into());
        }
    }
    REGISTERED.store(true, Ordering::SeqCst);
    if let Err(e) = negotiate(hwnd) {
        let _ = unregister(window);
        return Err(e);
    }
    #[cfg(debug_assertions)]
    if let (Ok(inner), Ok(outer), Ok(scale)) = (
        window.inner_size(),
        window.outer_size(),
        window.scale_factor(),
    ) {
        log::info!("Tauri measurement: scale_factor={scale:.2}, inner_physical={}x{}, inner_logical={:.2}x{:.2}, outer_physical={}x{}", inner.width,inner.height,inner.width as f64/scale,inner.height as f64/scale,outer.width,outer.height);
    }
    Ok(())
}

pub fn unregister(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = hwnd(window)?;
    if REGISTERED.swap(false, Ordering::SeqCst) && owns(hwnd) {
        unsafe {
            let mut bar = data(hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut bar);
        }
    }
    let result = restore_wndproc(hwnd);
    APPBAR_HWND.store(0, Ordering::SeqCst);
    CALLBACK_MESSAGE.store(0, Ordering::SeqCst);
    REPOSITION_QUEUED.store(false, Ordering::SeqCst);
    POSITIONING.store(false, Ordering::SeqCst);
    result
}

#[cfg(test)]
mod tests {
    use super::physical_width;

    #[test]
    fn converts_small_width_at_supported_scales() {
        assert_eq!(physical_width(96), 36);
        assert_eq!(physical_width(120), 45);
        assert_eq!(physical_width(144), 54);
        assert_eq!(physical_width(168), 63);
        assert_eq!(physical_width(192), 72);
    }
}
