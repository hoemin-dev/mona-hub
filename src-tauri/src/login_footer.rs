//! Native chrome: never inject recovery controls into an identity provider's DOM.
use tauri::{LogicalPosition, LogicalSize, WebviewWindow};
use webview2_com::{
    Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT, COREWEBVIEW2_MOVE_FOCUS_REASON_PREVIOUS,
    },
    MoveFocusRequestedEventHandler,
};
use windows::{
    core::w,
    Win32::{
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::*,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_RETURN, VK_SHIFT, VK_TAB},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

pub const HEIGHT: f64 = 56.0;
const BUTTON_ID: usize = 0x4d01;
const SUBCLASS_ID: usize = 0x4d02;
const WM_LAYOUT_WEBVIEW: u32 = WM_APP + 0x4d;

struct Footer {
    window: WebviewWindow,
    label: HWND,
    button: HWND,
    line: HWND,
    font: HFONT,
}

unsafe fn layout(footer: &mut Footer) {
    let scale = GetDpiForWindow(HWND(footer.window.hwnd().unwrap_or_default().0)) as f64 / 96.0;
    let px = |value: f64| (value * scale).round() as i32;
    let width = footer
        .window
        .inner_size()
        .map(|s| s.width as i32)
        .unwrap_or(px(420.0));
    let _ = SetWindowPos(
        footer.label,
        None,
        px(16.0),
        px(518.0),
        px(100.0),
        px(22.0),
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    let _ = SetWindowPos(
        footer.button,
        None,
        width - px(148.0),
        px(512.0),
        px(132.0),
        px(32.0),
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    let _ = SetWindowPos(
        footer.line,
        None,
        0,
        px(500.0),
        width,
        px(1.0).max(1),
        SWP_NOZORDER | SWP_NOACTIVATE,
    );
    let next = CreateFontW(
        -px(13.0),
        0,
        0,
        0,
        400,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        0,
        w!("Malgun Gothic"),
    );
    if !next.is_invalid() {
        for control in [footer.label, footer.button] {
            SendMessageW(
                control,
                WM_SETFONT,
                Some(WPARAM(next.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        if !footer.font.is_invalid() {
            let _ = DeleteObject(footer.font.into());
        }
        footer.font = next;
    }
}

unsafe extern "system" fn procedure(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    match msg {
        WM_COMMAND if wp.0 == BUTTON_ID && lp.0 == (*(data as *const Footer)).button.0 as isize => {
            let window = (*(data as *const Footer)).window.clone();
            if let Err(error) = super::restart_login(&window) {
                log::error!("[auth-window] restart failed: {error}");
                let _ = MessageBoxW(
                    Some(hwnd),
                    w!("로그인 시작 화면을 열지 못했습니다. 다시 시도해 주세요."),
                    w!("MonaHub 로그인"),
                    MB_OK | MB_ICONERROR,
                );
            }
            return LRESULT(0);
        }
        WM_SIZE | WM_DPICHANGED => {
            layout(&mut *(data as *mut Footer));
            // Wry's own WM_SIZE handler expands the webview to the full client
            // area. Reapply our content bounds after that handler has run.
            let _ = PostMessageW(Some(hwnd), WM_LAYOUT_WEBVIEW, WPARAM(0), LPARAM(0));
        }
        WM_LAYOUT_WEBVIEW => {
            let window = (*(data as *const Footer)).window.clone();
            let _ = content_bounds(&window);
            return LRESULT(0);
        }
        WM_CTLCOLORSTATIC => {
            let dc = HDC(wp.0 as *mut _);
            SetTextColor(dc, COLORREF(GetSysColor(COLOR_GRAYTEXT)));
            SetBkColor(dc, COLORREF(GetSysColor(COLOR_WINDOW)));
            return LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize);
        }
        WM_NCDESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID);
            let footer = Box::from_raw(data as *mut Footer);
            if !footer.font.is_invalid() {
                let _ = DeleteObject(footer.font.into());
            }
            return DefSubclassProc(hwnd, msg, wp, lp);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

unsafe extern "system" fn button_procedure(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN && wp.0 == VK_TAB.0 as usize {
        let window = (*(data as *const Footer)).window.clone();
        let backwards = GetKeyState(VK_SHIFT.0 as i32) < 0;
        let _ = window.with_webview(move |view| {
            let reason = if backwards {
                COREWEBVIEW2_MOVE_FOCUS_REASON_PREVIOUS
            } else {
                COREWEBVIEW2_MOVE_FOCUS_REASON_NEXT
            };
            let _ = view.controller().MoveFocus(reason);
        });
        return LRESULT(0);
    }
    if msg == WM_KEYDOWN && wp.0 == VK_RETURN.0 as usize {
        let _ = SendMessageW(hwnd, BM_CLICK, None, None);
        return LRESULT(0);
    }
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(button_procedure), SUBCLASS_ID);
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

// All HWND work stays on Tauri's UI thread. The original WebviewWindow remains
// the sole managed webview, preserving existing auth/session command routing.
pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
    let hwnd = HWND(window.hwnd()?.0);
    unsafe {
        let label = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!("MonaHub"),
            WS_CHILD,
            0,
            0,
            0,
            0,
            Some(hwnd),
            None,
            None,
            None,
        )
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
        let button = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            w!("로그인 처음으로"),
            WS_CHILD | WS_TABSTOP,
            0,
            0,
            0,
            0,
            Some(hwnd),
            Some(HMENU(BUTTON_ID as *mut _)),
            None,
            None,
        )
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
        let line = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            w!(""),
            WS_CHILD | WINDOW_STYLE(0x10),
            0,
            0,
            0,
            0,
            Some(hwnd),
            None,
            None,
            None,
        )
        .map_err(|e| tauri::Error::Anyhow(e.into()))?;
        let mut footer = Box::new(Footer {
            window: window.clone(),
            label,
            button,
            line,
            font: HFONT::default(),
        });
        layout(&mut footer);
        let data = Box::into_raw(footer);
        if !SetWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID, data as usize).as_bool() {
            let footer = Box::from_raw(data);
            let _ = DeleteObject(footer.font.into());
            for control in [label, button, line] {
                let _ = DestroyWindow(control);
            }
            return Err(tauri::Error::Anyhow(std::io::Error::last_os_error().into()));
        }
        if !SetWindowSubclass(button, Some(button_procedure), SUBCLASS_ID, data as usize).as_bool()
        {
            return Err(tauri::Error::Anyhow(std::io::Error::last_os_error().into()));
        }
        let button_address = button.0 as usize;
        window.with_webview(move |view| {
            let handler = MoveFocusRequestedEventHandler::create(Box::new(move |_, args| {
                let button = HWND(button_address as *mut _);
                if IsWindowVisible(button).as_bool() {
                    if let Some(args) = args {
                        args.SetHandled(true)?;
                    }
                    let _ = SetFocus(Some(button));
                }
                Ok(())
            }));
            let mut token = 0;
            if let Err(error) = view
                .controller()
                .add_MoveFocusRequested(&handler, &mut token)
            {
                log::error!("[auth-window] footer keyboard navigation unavailable: {error}");
            }
        })?;
    }
    Ok(())
}

fn content_bounds(window: &WebviewWindow) -> tauri::Result<()> {
    let webview: &tauri::Webview = window.as_ref();
    webview.set_auto_resize(false)?;
    webview.set_bounds(tauri::Rect {
        position: LogicalPosition::new(0.0, 0.0).into(),
        size: LogicalSize::new(420.0, 500.0).into(),
    })
}

pub fn set_visible(window: &WebviewWindow, visible: bool) -> tauri::Result<()> {
    content_bounds(window)?;
    let window = window.clone();
    window.clone().run_on_main_thread(move || unsafe {
        if let Ok(hwnd) = window.hwnd() {
            let mut data = 0;
            if windows::Win32::UI::Shell::GetWindowSubclass(
                HWND(hwnd.0),
                Some(procedure),
                SUBCLASS_ID,
                Some(&mut data),
            )
            .as_bool()
            {
                let footer = &mut *(data as *mut Footer);
                layout(footer);
                for control in [footer.label, footer.button, footer.line] {
                    let _ = ShowWindow(control, if visible { SW_SHOWNA } else { SW_HIDE });
                }
            }
        }
    })
}
