fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new()
                .commands(&[
                    "startup_rendered",
                    "notify_login_page_ready",
                    "close_login_window",
                    "minimize_login_window",
                    "begin_access_login",
                    "log_login_viewport",
                    "log_login_diagnostic",
                    "show_login_window",
                    "toggle_profile_popup",
                    "hide_profile_popup",
                    "confirm_access_logout",
                    "begin_access_logout",
                    "open_web_app",
                    "log_acdc_diagnostic",
                    "sync_acdc_identity",
                ]),
        ),
    )
    .expect("failed to build Tauri application manifest");
}
