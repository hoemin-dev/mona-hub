# Windows executable-name migration validation

Verified on Windows x64, 2026-09-14, with application version 0.1.2.

## Implementation

`src-tauri/windows/installer-hooks.nsh` is loaded through
`bundle.windows.nsis.installerHooks`. Before copying the new application, it
checks the version-resource ProductName of exactly `$INSTDIR\app.exe`. Only
`MONA-HUB` is accepted for deletion. An unidentified or locked file aborts the
installation with a nonzero exit code. No wildcard or recursive deletion is used.

After installation, existing HKCU Run/RunOnce commands whose executable token
matches the old full path are rewritten, preserving arguments. No autostart entry
is created. Links in the start-menu root/configured folder, desktop and startup
folder are checked by target; old targets and explicit old executable icons are
updated. Tauri continues to write the uninstall entry, DisplayIcon and
MainBinaryName using the new name.

## Actual installation tests

Executed installers against the real current-user installation at
`%LOCALAPPDATA%\MONA-HUB`, using `/S /UPDATE`. The installed application was
stopped before testing and the final installed application was launched afterward.

The original historical setup executable was not available. For the legacy
installation step, a setup executable was rebuilt from the pre-migration generated
NSIS script and the actual old `app.exe` recovered from the installed folder.
This installer was executed to install the legacy binary and its registry/shortcut
entries; the upgrade was not simulated by merely copying files.

Passed scenarios:

- Repair the reported mixed installation: both EXEs present, with MainBinaryName
  already set to MONA-HUB.exe.
- Execute the legacy installer, then execute the new installer over it.
- Repeat after removing the legacy MainBinaryName registry value.
- Hold the legacy EXE open without delete sharing: installer fails, leaving the
  legacy file intact; close the handle and retry successfully.
- Put an unrelated file named app.exe in a separate destination: installation
  fails and preserves the file byte for byte.
- Preserve an unrelated sentinel file in the installation directory throughout
  the successful upgrades. Remove only that test-created file afterward.
- Migrate a test startup shortcut's target and explicit icon while preserving its
  arguments; migrate a test Run command while preserving its arguments. Remove
  these test-created autostart entries afterward, restoring the user's previous
  autostart state (no MONA-HUB registration).
- Inspect start-menu and desktop links, uninstall metadata, and Run/RunOnce values
  for the exact legacy executable path: no remaining reference.

Final installed files: `MONA-HUB.exe`, `uninstall.exe` only.
The launched installed process reports `MONA-HUB.exe` at the installation path.

Actual installed EXE Version Info:

| Field | Value |
| --- | --- |
| FileDescription | MONA-HUB |
| ProductName | MONA-HUB |
| FileVersion / ProductVersion | 0.1.2 / 0.1.2 |
| LegalCopyright | Copyright © 2026 MONAS Pump Co., Ltd. |
| Installed-app Publisher | MONAS Pump Co., Ltd. |

Build: `npm run tauri:build`, followed by `npm run tauri -- bundle --bundles nsis`
after the final hook edit. Both succeeded. Two existing appbar Rust warnings remain.
Local test harness and detailed results are in ignored `tmp/upgrade-validation/`.
No Git staging, commit, push or deployment was performed.
