# Windows automatic startup

The NSIS installer uses Tauri's default `currentUser` install mode and the existing
`windows/installer-hooks.nsh`. Post-install writes the quoted installed executable
path to `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\MONA-HUB`.
Pre-uninstall deletes that value. Installation, including reinstall/upgrade, enables
startup by default. Merely launching the app never re-enables it after OFF.

The tray reads the actual Run value at initialization and on tray events. Clicking
the check item reads the registry, writes/deletes the value, then reads again to
reconcile the native check state, including after write errors. No separate setting,
HKLM entry, or new Startup-folder shortcut is used. Existing legacy shortcut and
Run/RunOnce migration remains intact.

Windows registry integration test (explicit opt-in; refuses to overwrite any
existing MONA-HUB value):

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --lib autostart::tests::windows_run_round_trip -- --ignored --exact
```

It exercises the same functions used by the tray: ON, read back the complete quoted
current executable path, OFF, and repeated OFF. Cleanup removes the test value on
both success and assertion failure. Since this runs in the Rust test executable,
the expected path is the test executable's path, not the packaged binary.

For a manual installed-app smoke test:

1. Install the generated NSIS package and verify Run MONA-HUB contains the quoted
   installed MONA-HUB.exe path. Open the tray and check that automatic startup is ON.
2. Click OFF; verify the value is absent. Restart the app and confirm OFF persists.
3. Click ON; verify the actual installed path is registered. Externally delete the
   value and reopen the tray to confirm that its check state refreshes.
4. Verify login/logout, help, left-click restore, and quit still work.
5. Uninstall and verify the MONA-HUB Run value is absent.

The native tray interaction and full install/uninstall smoke sequence require a
desktop session and are not covered by the registry integration test.
