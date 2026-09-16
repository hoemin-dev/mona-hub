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
        Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::*,
        UI::{
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{
                GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_RETURN,
                VK_SHIFT, VK_TAB,
            },
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

pub const HEIGHT: f64 = 48.0;
pub const OUTER_WIDTH: f64 = 420.0;
pub const OUTER_HEIGHT: f64 = 480.0;
const BUTTON_ID: usize = 0x4d01;
const SUBCLASS_ID: usize = 0x4d02;
const WM_LAYOUT_WEBVIEW: u32 = WM_APP + 0x4d;
const WM_MOUSELEAVE: u32 = 0x02a3;

struct Footer {
    window: WebviewWindow,
    label: HWND,
    button: HWND,
    font: HFONT,
    visible: bool,
}

unsafe fn layout(footer: &mut Footer) {
    let scale = GetDpiForWindow(HWND(footer.window.hwnd().unwrap_or_default().0)) as f64 / 96.0;
    let px = |value: f64| (value * scale).round() as i32;
    let size = footer
        .window
        .inner_size()
        .unwrap_or(tauri::PhysicalSize::new(
            px(OUTER_WIDTH) as u32,
            px(OUTER_HEIGHT) as u32,
        ));
    let width = size.width as i32;
    let top = size.height as i32 - px(HEIGHT);
    let _ = SetWindowPos(
        footer.label,
        Some(HWND_BOTTOM),
        0,
        top,
        width,
        px(HEIGHT),
        SWP_NOACTIVATE,
    );
    let _ = SetWindowPos(
        footer.button,
        Some(HWND_TOP),
        width - px(140.0),
        top + px((HEIGHT - 32.0) / 2.0),
        px(124.0),
        px(32.0),
        SWP_NOACTIVATE,
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

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | ((g as u32) << 8) | ((b as u32) << 16))
}

// Paint the entire footer, including the area between controls. Leaving that
// area to the WebView host produces a black strip on Windows.
unsafe fn paint(hwnd: HWND, font: HFONT, button: bool) {
    let mut ps = PAINTSTRUCT::default();
    let dc = BeginPaint(hwnd, &mut ps);
    let mut rect = RECT::default();
    let _ = GetClientRect(hwnd, &mut rect);
    let scale = GetDpiForWindow(hwnd) as f64 / 96.0;
    let px = |n: f64| (n * scale).round() as i32;
    let background = CreateSolidBrush(rgb(248, 249, 250));
    FillRect(dc, &rect, background);
    let _ = DeleteObject(background.into());
    let old_font = SelectObject(dc, font.into());
    SetBkMode(dc, TRANSPARENT);
    if button {
        let state = SendMessageW(hwnd, BM_GETSTATE, None, None).0 as u32;
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let _ = ScreenToClient(hwnd, &mut cursor);
        let hot = PtInRect(&rect, cursor).as_bool();
        let pressed = state & BST_PUSHED != 0;
        let focused = state & BST_FOCUS != 0;
        let fill = CreateSolidBrush(if pressed {
            rgb(50, 126, 92)
        } else if hot {
            rgb(54, 139, 101)
        } else if focused {
            rgb(54, 139, 101)
        } else {
            rgb(69, 156, 113)
        });
        let old_brush = SelectObject(dc, fill.into());
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        let inset = px(1.0).max(1);
        let _ = RoundRect(
            dc,
            inset,
            inset,
            rect.right - inset,
            rect.bottom - inset,
            px(8.0),
            px(8.0),
        );
        SelectObject(dc, old_pen);
        SelectObject(dc, old_brush);
        let _ = DeleteObject(fill.into());
        SetTextColor(dc, rgb(255, 255, 255));
        DrawTextW(
            dc,
            &mut "로그인 처음으로".encode_utf16().collect::<Vec<_>>(),
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    } else {
        let line = CreateSolidBrush(rgb(226, 230, 234));
        FillRect(
            dc,
            &RECT {
                bottom: px(1.0).max(1),
                ..rect
            },
            line,
        );
        let _ = DeleteObject(line.into());
        rect.left += px(16.0);
        SetTextColor(dc, rgb(111, 120, 130));
        DrawTextW(
            dc,
            &mut "비밀번호 초기화는 관리자에게 문의하세요.".encode_utf16().collect::<Vec<_>>(),
            &mut rect,
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
    }
    SelectObject(dc, old_font);
    let _ = EndPaint(hwnd, &ps);
}

unsafe extern "system" fn surface_procedure(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    if msg == WM_PAINT {
        paint(hwnd, (*(data as *const Footer)).font, false);
        return LRESULT(0);
    }
    if msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if msg == WM_NCDESTROY {
        let _ = RemoveWindowSubclass(hwnd, Some(surface_procedure), SUBCLASS_ID);
    }
    DefSubclassProc(hwnd, msg, wp, lp)
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
                    w!("MONA-HUB 로그인"),
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
            // Repaint after WebView bounds and z-order settle, even when the
            // mouse never enters the footer.
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW,
            );
            return LRESULT(0);
        }
        WM_SHOWWINDOW if wp.0 != 0 => {
            let _ = PostMessageW(Some(hwnd), WM_LAYOUT_WEBVIEW, WPARAM(0), LPARAM(0));
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
    if msg == WM_PAINT {
        paint(hwnd, (*(data as *const Footer)).font, true);
        return LRESULT(0);
    }
    if msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    if msg == WM_MOUSEMOVE {
        let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        });
    }
    if matches!(
        msg,
        WM_MOUSEMOVE
            | WM_MOUSELEAVE
            | WM_SETFOCUS
            | WM_KILLFOCUS
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_KEYUP
    ) {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
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
            WS_CHILD | WS_CLIPSIBLINGS,
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
            WS_CHILD | WS_TABSTOP | WS_CLIPSIBLINGS,
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
        let mut footer = Box::new(Footer {
            window: window.clone(),
            label,
            button,
            font: HFONT::default(),
            visible: false,
        });
        layout(&mut footer);
        let data = Box::into_raw(footer);
        if !SetWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID, data as usize).as_bool() {
            let footer = Box::from_raw(data);
            let _ = DeleteObject(footer.font.into());
            for control in [label, button] {
                let _ = DestroyWindow(control);
            }
            return Err(tauri::Error::Anyhow(std::io::Error::last_os_error().into()));
        }
        if !SetWindowSubclass(label, Some(surface_procedure), SUBCLASS_ID, data as usize).as_bool()
        {
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
    let size = window.inner_size()?;
    let scale = window.scale_factor()?;
    let mut data = 0;
    let visible = unsafe {
        windows::Win32::UI::Shell::GetWindowSubclass(
            HWND(window.hwnd()?.0),
            Some(procedure),
            SUBCLASS_ID,
            Some(&mut data),
        )
        .as_bool()
            && (*(data as *const Footer)).visible
    };
    let webview: &tauri::Webview = window.as_ref();
    webview.set_auto_resize(false)?;
    webview.set_bounds(tauri::Rect {
        position: LogicalPosition::new(0.0, 0.0).into(),
        size: LogicalSize::new(
            size.width as f64 / scale,
            (size.height as f64 / scale - if visible { HEIGHT } else { 0.0 }).max(1.0),
        )
        .into(),
    })
}

pub fn set_visible(window: &WebviewWindow, visible: bool) -> tauri::Result<()> {
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
                footer.visible = visible;
                layout(footer);
                let controls = [footer.label, footer.button];
                for control in controls {
                    let _ = ShowWindow(control, if visible { SW_SHOWNA } else { SW_HIDE });
                    let _ = InvalidateRect(Some(control), None, false);
                }
                let _ = PostMessageW(Some(HWND(hwnd.0)), WM_LAYOUT_WEBVIEW, WPARAM(0), LPARAM(0));
            }
        }
    })
}

/// Keep the same outer dimensions with either native or HTML title bars.
pub fn set_outer_size(window: &WebviewWindow) -> tauri::Result<()> {
    let window = window.clone();
    window.clone().run_on_main_thread(move || unsafe {
        if let Ok(hwnd) = window.hwnd() {
            let hwnd = HWND(hwnd.0);
            let scale = GetDpiForWindow(hwnd) as f64 / 96.0;
            if let Err(error) = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                (OUTER_WIDTH * scale).round() as i32,
                (OUTER_HEIGHT * scale).round() as i32,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                log::error!("[auth-window] outer size failed: {error}");
            }
        }
    })
}
