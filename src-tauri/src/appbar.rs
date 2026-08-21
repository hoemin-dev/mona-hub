#![cfg(target_os = "windows")]

use std::{
    mem::{size_of, transmute},
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering},
        OnceLock,
    },
};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use tauri::{AppHandle, Manager, WebviewWindow};

use windows::{
    core::w,
    Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
        UI::{
            Shell::{
                SHAppBarMessage, APPBARDATA, ABE_RIGHT, ABM_ACTIVATE, ABM_NEW,
                ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS, ABM_WINDOWPOSCHANGED,
                ABN_POSCHANGED,
            },
            WindowsAndMessaging::{
                CallWindowProcW, MoveWindow, RegisterWindowMessageW, SetWindowLongPtrW,
                GWLP_WNDPROC, WM_ACTIVATE, WM_DESTROY, WM_DISPLAYCHANGE,
                WM_DPICHANGED, WM_WINDOWPOSCHANGED, WNDPROC,
                GetWindowLongPtrW, 
    SetWindowPos,
    GWL_EXSTYLE,
    SWP_FRAMECHANGED,
    SWP_NOMOVE,
    SWP_NOSIZE,
    SWP_NOZORDER,
    WS_EX_APPWINDOW,
    WS_EX_TOOLWINDOW,
            },
        },
    },
};

const COLLAPSED_WIDTH: i32 = 36;
const MIN_WIDTH: i32 = 36;
const MAX_WIDTH: i32 = 800;

/*
 * MONA-HUB는 AppBar 창을 하나만 사용하므로 전역 상태 하나로 관리한다.
 *
 * WndProc 안에서 Mutex를 잡은 채 MoveWindow를 호출하면
 * Windows 메시지가 재진입할 수 있으므로 Atomic을 사용한다.
 */
static APPBAR_HWND: AtomicIsize = AtomicIsize::new(0);
static ORIGINAL_WNDPROC: AtomicIsize = AtomicIsize::new(0);
static CALLBACK_MESSAGE: AtomicU32 = AtomicU32::new(0);
static APPBAR_WIDTH: AtomicI32 = AtomicI32::new(COLLAPSED_WIDTH);
static REGISTERED: AtomicBool = AtomicBool::new(false);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
static WORK_SCHEDULED: AtomicBool = AtomicBool::new(false);
static REPOSITION_REQUESTED: AtomicBool = AtomicBool::new(false);
static ACTIVATE_NOTIFICATION_REQUESTED: AtomicBool = AtomicBool::new(false);
static SHELL_NOTIFICATION_REQUESTED: AtomicBool = AtomicBool::new(false);

/// WndProc에서는 Shell 호출이나 창 이동을 직접 실행하지 않고 작업만 예약한다.
/// 여러 Windows 메시지가 연속으로 들어오면 하나의 작업으로 합쳐 처리한다.
fn queue_appbar_work(
    hwnd: HWND,
    reposition: bool,
    notify_activate: bool,
    notify_position: bool,
) {
    if reposition {
        REPOSITION_REQUESTED.store(true, Ordering::SeqCst);
    }

    if notify_activate {
        ACTIVATE_NOTIFICATION_REQUESTED.store(true, Ordering::SeqCst);
    }

    if notify_position {
        SHELL_NOTIFICATION_REQUESTED.store(true, Ordering::SeqCst);
    }

    if WORK_SCHEDULED.swap(true, Ordering::SeqCst) {
        return;
    }

    let Some(app) = APP_HANDLE.get().cloned() else {
        WORK_SCHEDULED.store(false, Ordering::SeqCst);
        return;
    };

    let hwnd_value = hwnd_to_isize(hwnd);
    let schedule_result = app.run_on_main_thread(move || {
        let hwnd = HWND(hwnd_value as *mut std::ffi::c_void);

        if REGISTERED.load(Ordering::SeqCst) && is_appbar_hwnd(hwnd) {
            if REPOSITION_REQUESTED.swap(false, Ordering::SeqCst) {
                let width = APPBAR_WIDTH.load(Ordering::SeqCst);
                let _ = position_appbar(hwnd, width);
            }

            if ACTIVATE_NOTIFICATION_REQUESTED.swap(false, Ordering::SeqCst) {
                unsafe {
                    let mut data = appbar_data(hwnd);
                    SHAppBarMessage(ABM_ACTIVATE, &mut data);
                }
            }

            if SHELL_NOTIFICATION_REQUESTED.swap(false, Ordering::SeqCst) {
                unsafe {
                    let mut data = appbar_data(hwnd);
                    SHAppBarMessage(ABM_WINDOWPOSCHANGED, &mut data);
                }
            }
        } else {
            REPOSITION_REQUESTED.store(false, Ordering::SeqCst);
            ACTIVATE_NOTIFICATION_REQUESTED.store(false, Ordering::SeqCst);
            SHELL_NOTIFICATION_REQUESTED.store(false, Ordering::SeqCst);
        }

        WORK_SCHEDULED.store(false, Ordering::SeqCst);

        // position_appbar의 MoveWindow 중 새 요청이 들어온 경우 다음 tick에서 처리한다.
        if REPOSITION_REQUESTED.load(Ordering::SeqCst)
            || ACTIVATE_NOTIFICATION_REQUESTED.load(Ordering::SeqCst)
            || SHELL_NOTIFICATION_REQUESTED.load(Ordering::SeqCst)
        {
            queue_appbar_work(hwnd, false, false, false);
        }
    });

    if schedule_result.is_err() {
        WORK_SCHEDULED.store(false, Ordering::SeqCst);
    }
}

/// Tauri 창에서 Windows HWND를 얻는다.
fn get_hwnd(window: &WebviewWindow) -> Result<HWND, String> {
    let handle = window
        .window_handle()
        .map_err(|error| format!("윈도우 핸들을 얻지 못했습니다: {error}"))?;

    match handle.as_raw() {
        RawWindowHandle::Win32(handle) => {
            Ok(HWND(handle.hwnd.get() as *mut std::ffi::c_void))
        }

        _ => Err("Windows HWND가 아닙니다.".to_string()),
    }
}

/// 창은 화면에 표시하되 작업표시줄과 Alt+Tab에서는 숨긴다.
fn apply_tool_window_style(hwnd: HWND) -> Result<(), String> {
    unsafe {
        let current_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);

        let new_style =
            (current_style | WS_EX_TOOLWINDOW.0 as isize)
                & !(WS_EX_APPWINDOW.0 as isize);

        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_style);

        /*
         * 확장 스타일 변경 사항을 Windows Shell에 즉시 반영한다.
         * 위치와 크기는 변경하지 않는다.
         */
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE
                | SWP_NOSIZE
                | SWP_NOZORDER
                | SWP_FRAMECHANGED,
        )
        .map_err(|error| {
            format!("작업표시줄/Alt+Tab 숨김 스타일 적용 실패: {error}")
        })?;
    }

    Ok(())
}

/// HWND를 Atomic에 저장할 수 있는 정수형으로 변환한다.
fn hwnd_to_isize(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

/// 현재 AppBar로 등록된 HWND인지 확인한다.
fn is_appbar_hwnd(hwnd: HWND) -> bool {
    APPBAR_HWND.load(Ordering::SeqCst) == hwnd_to_isize(hwnd)
}

/// APPBARDATA 기본 구조체를 만든다.
fn appbar_data(hwnd: HWND) -> APPBARDATA {
    APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        ..Default::default()
    }
}

/// 창이 위치한 모니터의 전체 영역을 얻는다.
///
/// 작업표시줄과 다른 AppBar를 피하는 처리는
/// ABM_QUERYPOS가 담당하므로 rcMonitor를 사용한다.
fn monitor_rect(hwnd: HWND) -> Result<RECT, String> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);

        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return Err("모니터 정보를 얻지 못했습니다.".to_string());
        }

        Ok(info.rcMonitor)
    }
}

/// Windows Shell과 위치를 협상하고 실제 창을 이동한다.
///
/// ABN_POSCHANGED를 받을 때마다 다시 호출된다.
fn position_appbar(hwnd: HWND, width: i32) -> Result<(), String> {
    let monitor = monitor_rect(hwnd)?;

    unsafe {
        let mut data = APPBARDATA {
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

        /*
         * Windows가 작업표시줄 및 다른 AppBar의 위치를 고려해
         * 사용 가능한 영역으로 data.rc를 수정한다.
         */
        SHAppBarMessage(ABM_QUERYPOS, &mut data);

        /*
         * 오른쪽 AppBar의 원하는 폭만 다시 적용한다.
         *
         * top, right, bottom은 Windows가 조정한 값을 유지해야 한다.
         * 예를 들어 작업표시줄이 아래에 있으면 bottom이 위로 올라간다.
         */
        data.rc.left = data.rc.right - width;

        let result = SHAppBarMessage(ABM_SETPOS, &mut data);

        if result == 0 {
            return Err("Windows AppBar 위치 예약에 실패했습니다.".to_string());
        }

        let actual_width = data.rc.right - data.rc.left;
        let actual_height = data.rc.bottom - data.rc.top;

        if actual_width <= 0 || actual_height <= 0 {
            return Err(format!(
                "Windows가 잘못된 AppBar 영역을 반환했습니다: \
                 left={}, top={}, right={}, bottom={}",
                data.rc.left, data.rc.top, data.rc.right, data.rc.bottom
            ));
        }

        /*
         * Tauri API가 아니라 Win32 MoveWindow를 사용한다.
         *
         * Shell이 반환한 좌표를 같은 Win32 메시지 흐름 안에서
         * 즉시 적용할 수 있다.
         */
        MoveWindow(
            hwnd,
            data.rc.left,
            data.rc.top,
            actual_width,
            actual_height,
            true,
        )
        .map_err(|error| format!("AppBar 창 이동 실패: {error}"))?;

    }

    Ok(())
}

/// 원래 Tauri/Wry WndProc로 메시지를 전달한다.
unsafe fn call_original_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let original = ORIGINAL_WNDPROC.load(Ordering::SeqCst);

    if original == 0 {
        return LRESULT(0);
    }

    /*
     * SetWindowLongPtrW가 반환한 원래 WndProc 주소를
     * Windows의 WNDPROC 함수 포인터 형식으로 복원한다.
     */
    let original_proc: WNDPROC = transmute(original);

    CallWindowProcW(original_proc, hwnd, message, wparam, lparam)
}

/// MONA-HUB AppBar 전용 Window Procedure.
///
/// 처리하지 않는 메시지는 반드시 기존 Tauri WndProc에 전달한다.
unsafe extern "system" fn appbar_wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let callback_message = CALLBACK_MESSAGE.load(Ordering::SeqCst);

    /*
     * Windows Shell이 보내는 AppBar 알림 메시지
     */
    if callback_message != 0 && message == callback_message {
        let notification = wparam.0 as u32;

        if notification == ABN_POSCHANGED {
            /*
             * 작업표시줄의 위치, 크기, 표시 상태가 변경되거나
             * 다른 AppBar가 추가·삭제·변경되면 다시 협상한다.
             */
            queue_appbar_work(hwnd, true, false, false);
        }

        return LRESULT(0);
    }

    match message {
        WM_ACTIVATE => {
            /*
             * AppBar 활성화 상태를 Shell에 알린다.
             */
            if REGISTERED.load(Ordering::SeqCst) && is_appbar_hwnd(hwnd) {
                queue_appbar_work(hwnd, false, true, false);
            }
        }

        WM_WINDOWPOSCHANGED => {
            /*
             * AppBar 창 위치가 변경되었음을 Shell에 알린다.
             */
            if REGISTERED.load(Ordering::SeqCst) && is_appbar_hwnd(hwnd) {
                queue_appbar_work(hwnd, false, false, true);
            }
        }

        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            /*
             * 해상도, 배율 또는 모니터 구성이 바뀌었을 때도
             * AppBar 영역을 다시 계산한다.
             */
            if REGISTERED.load(Ordering::SeqCst) && is_appbar_hwnd(hwnd) {
                queue_appbar_work(hwnd, true, false, false);
            }
        }

        WM_DESTROY => {
            /*
             * 비정상적인 종료 경로에서도 예약 영역을 최대한 해제한다.
             *
             * 정상 종료 때는 unregister()에서 먼저 해제된다.
             */
            if REGISTERED.swap(false, Ordering::SeqCst) && is_appbar_hwnd(hwnd) {
                let mut data = appbar_data(hwnd);
                SHAppBarMessage(ABM_REMOVE, &mut data);
            }
        }

        _ => {}
    }

    call_original_wndproc(hwnd, message, wparam, lparam)
}

/// Tauri 창의 WndProc를 MONA-HUB AppBar WndProc로 교체한다.
fn install_wndproc(hwnd: HWND) -> Result<(), String> {
    if ORIGINAL_WNDPROC.load(Ordering::SeqCst) != 0 {
        return Ok(());
    }

    unsafe {
        let previous = SetWindowLongPtrW(
            hwnd,
            GWLP_WNDPROC,
            appbar_wndproc as *const () as isize,
        );

        if previous == 0 {
            return Err("AppBar Window Procedure 설치에 실패했습니다.".to_string());
        }

        ORIGINAL_WNDPROC.store(previous, Ordering::SeqCst);
    }

    Ok(())
}

/// 기존 Tauri WndProc를 복원한다.
fn restore_wndproc(hwnd: HWND) -> Result<(), String> {
    let previous = ORIGINAL_WNDPROC.swap(0, Ordering::SeqCst);

    if previous == 0 {
        return Ok(());
    }

    unsafe {
        let result = SetWindowLongPtrW(hwnd, GWLP_WNDPROC, previous);

        if result == 0 {
            /*
             * 복원 실패 시 원래 주소를 다시 저장해 둔다.
             * 잘못된 상태에서 함수 포인터를 잃지 않기 위함이다.
             */
            ORIGINAL_WNDPROC.store(previous, Ordering::SeqCst);

            return Err("기존 Window Procedure 복원에 실패했습니다.".to_string());
        }
    }

    Ok(())
}

/// 오른쪽 AppBar를 등록한다.
pub fn register(window: &WebviewWindow, width: i32) -> Result<(), String> {
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return Err(format!(
            "AppBar 폭은 {MIN_WIDTH}~{MAX_WIDTH}px 범위여야 합니다."
        ));
    }

    let hwnd = get_hwnd(window)?;
    let _ = APP_HANDLE.set(window.app_handle().clone());

    apply_tool_window_style(hwnd)?;

    /*
     * 이미 등록된 같은 창이면 폭만 변경한다.
     */
    if REGISTERED.load(Ordering::SeqCst) && is_appbar_hwnd(hwnd) {
        APPBAR_WIDTH.store(width, Ordering::SeqCst);
        return position_appbar(hwnd, width);
    }

    /*
     * 혹시 다른 등록 정보가 남아 있으면 먼저 정리한다.
     */
    if REGISTERED.load(Ordering::SeqCst) {
        return Err("다른 AppBar 창이 이미 등록되어 있습니다.".to_string());
    }

    let callback_message = unsafe {
        RegisterWindowMessageW(w!("MONA_HUB_APPBAR_CALLBACK"))
    };

    if callback_message == 0 {
        return Err("AppBar 콜백 메시지 등록에 실패했습니다.".to_string());
    }

    CALLBACK_MESSAGE.store(callback_message, Ordering::SeqCst);
    APPBAR_HWND.store(hwnd_to_isize(hwnd), Ordering::SeqCst);
    APPBAR_WIDTH.store(width, Ordering::SeqCst);

    /*
     * ABM_NEW 전에 WndProc를 설치해야
     * 등록 직후 들어오는 Shell 메시지도 받을 수 있다.
     */
    install_wndproc(hwnd)?;

    unsafe {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: hwnd,
            uCallbackMessage: callback_message,
            ..Default::default()
        };

        let result = SHAppBarMessage(ABM_NEW, &mut data);

        if result == 0 {
            let _ = restore_wndproc(hwnd);

            CALLBACK_MESSAGE.store(0, Ordering::SeqCst);
            APPBAR_HWND.store(0, Ordering::SeqCst);

            return Err("Windows Shell에 AppBar 등록을 실패했습니다.".to_string());
        }
    }

    REGISTERED.store(true, Ordering::SeqCst);

    if let Err(error) = position_appbar(hwnd, width) {
        let _ = unregister(window);
        return Err(error);
    }

    Ok(())
}

/// 최초 실행은 36px(Small), 이후 재등록은 현재 선택 폭을 유지한다.
pub fn register_collapsed(window: &WebviewWindow) -> Result<(), String> {
    register(window, APPBAR_WIDTH.load(Ordering::SeqCst))
}

/// AppBar 폭 변경
///
/// 기존처럼 unregister/register를 반복하지 않고
/// 등록 상태를 유지한 채 위치만 다시 협상한다.
pub fn resize(window: &WebviewWindow, width: i32) -> Result<(), String> {
    if !(MIN_WIDTH..=MAX_WIDTH).contains(&width) {
        return Err(format!(
            "AppBar 폭은 {MIN_WIDTH}~{MAX_WIDTH}px 범위여야 합니다."
        ));
    }

    let hwnd = get_hwnd(window)?;

    if !REGISTERED.load(Ordering::SeqCst) || !is_appbar_hwnd(hwnd) {
        return register(window, width);
    }

    APPBAR_WIDTH.store(width, Ordering::SeqCst);
    position_appbar(hwnd, width)
}

/// Windows Shell에서 AppBar 등록을 해제하고 기존 WndProc를 복원한다.
pub fn unregister(window: &WebviewWindow) -> Result<(), String> {
    let hwnd = get_hwnd(window)?;

    if REGISTERED.swap(false, Ordering::SeqCst) {
        unsafe {
            let mut data = appbar_data(hwnd);
            SHAppBarMessage(ABM_REMOVE, &mut data);
        }
    }

    /*
     * 서브클래싱은 설치의 역순으로 제거해야 한다.
     */
    let restore_result = restore_wndproc(hwnd);

    APPBAR_HWND.store(0, Ordering::SeqCst);
    CALLBACK_MESSAGE.store(0, Ordering::SeqCst);
    REPOSITION_REQUESTED.store(false, Ordering::SeqCst);
    ACTIVATE_NOTIFICATION_REQUESTED.store(false, Ordering::SeqCst);
    SHELL_NOTIFICATION_REQUESTED.store(false, Ordering::SeqCst);

    restore_result
}
