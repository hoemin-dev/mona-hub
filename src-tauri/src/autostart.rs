//! The Run value is the only source of truth. Installation supplies the default.
use std::os::windows::ffi::OsStrExt;
use tauri::{
    menu::{CheckMenuItem, Menu},
    AppHandle, Manager,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::ERROR_FILE_NOT_FOUND,
        System::Registry::{
            RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
            RRF_RT_REG_SZ,
        },
    },
};

const RUN: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const NAME: PCWSTR = w!("MONA-HUB");
struct StartupMenu(CheckMenuItem<tauri::Wry>);

fn enabled() -> windows::core::Result<bool> {
    let mut bytes = 0;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN,
            NAME,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
    };
    if result == ERROR_FILE_NOT_FOUND {
        return Ok(false);
    }
    result.ok()?;
    Ok(bytes > 2)
}

fn set_enabled(on: bool) -> Result<(), Box<dyn std::error::Error>> {
    if on {
        // Quote the complete executable token, including paths containing spaces.
        let executable = std::env::current_exe()?;
        let command: Vec<u16> = std::iter::once(b'"' as u16)
            .chain(executable.as_os_str().encode_wide())
            .chain([b'"' as u16, 0])
            .collect();
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN,
                NAME,
                REG_SZ.0,
                Some(command.as_ptr().cast()),
                (command.len() * 2) as u32,
            )
        }
        .ok()?;
    } else {
        let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN, NAME) };
        if result != ERROR_FILE_NOT_FOUND {
            result.ok()?;
        }
    }
    Ok(())
}

pub fn add_menu(app: &AppHandle, menu: &Menu<tauri::Wry>) -> tauri::Result<()> {
    let item = CheckMenuItem::with_id(
        app,
        "windows-autostart",
        "Windows 시작 시 자동 실행",
        true,
        false,
        None::<&str>,
    )?;
    menu.insert(&item, 2)?;
    app.manage(StartupMenu(item));
    sync(app);
    Ok(())
}

pub fn sync(app: &AppHandle) {
    let Some(item) = app.try_state::<StartupMenu>() else {
        return;
    };
    match enabled() {
        Ok(on) => {
            if let Err(error) = item.0.set_checked(on) {
                log::error!("[autostart] menu sync failed: {error}");
            }
            let _ = item.0.set_enabled(true);
        }
        Err(error) => {
            log::error!("[autostart] registry read failed: {error}");
            let _ = item.0.set_enabled(false);
        }
    }
}

pub fn toggle(app: &AppHandle) {
    let result = enabled()
        .map_err(|e| Box::<dyn std::error::Error>::from(e))
        .and_then(|on| set_enabled(!on));
    if let Err(error) = result {
        log::error!("[autostart] registry update failed: {error}");
    }
    // Native check items toggle themselves; reconcile even after a failed write.
    sync(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes HKCU Run; requires the MONA-HUB value to be absent"]
    fn windows_run_round_trip() {
        let existing = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                RUN,
                NAME,
                windows::Win32::System::Registry::RRF_RT_ANY,
                None,
                None,
                None,
            )
        };
        assert_eq!(
            existing, ERROR_FILE_NOT_FOUND,
            "preserve existing registration: remove it explicitly before this test"
        );
        struct Cleanup;
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = set_enabled(false);
            }
        }
        let _cleanup = Cleanup;
        set_enabled(true).expect("enable");
        assert!(enabled().unwrap());
        let mut value = vec![0u16; 32768];
        let mut bytes = (value.len() * 2) as u32;
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                RUN,
                NAME,
                RRF_RT_REG_SZ,
                None,
                Some(value.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        }
        .ok()
        .unwrap();
        let expected = format!("\"{}\"", std::env::current_exe().unwrap().display());
        assert_eq!(
            String::from_utf16(&value[..bytes as usize / 2 - 1]).unwrap(),
            expected
        );
        set_enabled(false).expect("disable");
        assert!(!enabled().unwrap());
        set_enabled(false).expect("disable twice");
    }
}
