; The old filename can survive after MainBinaryName has already been updated.
; Only remove this exact file, and only when its version resource identifies MONA-HUB.
!macro NSIS_HOOK_PREINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  IfFileExists "$INSTDIR\app.exe" 0 mona_legacy_done
  StrCpy $6 ""
  System::Call 'version::GetFileVersionInfoSizeW(w "$INSTDIR\app.exe", p 0) i .r1'
  ${If} $1 > 0
    System::Alloc $1
    Pop $2
    ${If} $2 P<> 0
      System::Call 'version::GetFileVersionInfoW(w "$INSTDIR\app.exe", i 0, i r1, p r2) i .r0'
      ${If} $0 <> 0
        System::Call 'version::VerQueryValueW(p r2, w "\VarFileInfo\Translation", *p .r3, *i .r4) i .r0'
        ${If} $0 <> 0
        ${AndIf} $4 >= 4
          System::Call '*$3(&i2 .r4, &i2 .r5)'
          IntFmt $4 "%04x" $4
          IntFmt $5 "%04x" $5
          System::Call 'version::VerQueryValueW(p r2, w "\StringFileInfo\$4$5\ProductName", *p .r3, *i .r1) i .r0'
          ${If} $0 <> 0
            System::Call '*$3(&w${NSIS_MAX_STRLEN} .r6)'
          ${EndIf}
        ${EndIf}
      ${EndIf}
      System::Free $2
    ${EndIf}
  ${EndIf}
  ${If} $6 != "MONA-HUB"
    DetailPrint "Refusing to remove an unidentified file: $INSTDIR\app.exe"
    MessageBox MB_ICONSTOP|MB_OK "Cannot identify $INSTDIR\app.exe as an old MONA-HUB executable. Installation stopped; the file was preserved." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  ClearErrors
  Delete "$INSTDIR\app.exe"
  ${If} ${Errors}
    DetailPrint "Cannot remove the old MONA-HUB executable: $INSTDIR\app.exe"
    MessageBox MB_ICONSTOP|MB_OK "Close the old MONA-HUB app ($INSTDIR\app.exe), then run this installer again. The old executable could not be removed." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  mona_legacy_done:
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

; Match the complete executable token and preserve startup arguments and opt-in state.
; Never create a startup registration when none existed.
!macro MonaMigrateRunKey KEY
  StrCpy $0 0
  mona_run_next_${KEY}:
    EnumRegValue $1 HKCU "Software\Microsoft\Windows\CurrentVersion\${KEY}" $0
    StrCmp $1 "" mona_run_done_${KEY}
    IntOp $0 $0 + 1
    ReadRegStr $2 HKCU "Software\Microsoft\Windows\CurrentVersion\${KEY}" $1
    StrCpy $3 '$\"$INSTDIR\app.exe$\"'
    StrLen $4 $3
    StrCpy $5 $2 $4
    ${If} $5 != $3
      StrCpy $3 "$INSTDIR\app.exe"
      StrLen $4 $3
      StrCpy $5 $2 $4
    ${EndIf}
    ${If} $5 == $3
      StrCpy $5 $2 1 $4
      ${If} $5 == ""
      ${OrIf} $5 == " "
        StrCpy $5 $2 "" $4
        WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\${KEY}" $1 '$\"$INSTDIR\${MAINBINARYNAME}.exe$\"$5'
      ${EndIf}
    ${EndIf}
    Goto mona_run_next_${KEY}
  mona_run_done_${KEY}:
!macroend

; Inspect link targets, not link names; leave unrelated user shortcuts untouched.
!macro MonaMigrateLinks DIRECTORY ID
  FindFirst $7 $8 "${DIRECTORY}\*.lnk"
  mona_link_next_${ID}:
    StrCmp $8 "" mona_link_done_${ID}
    !insertmacro IsShortcutTarget "${DIRECTORY}\$8" "$INSTDIR\app.exe"
    Pop $9
    ${If} $9 == 1
      !insertmacro SetShortcutTarget "${DIRECTORY}\$8" "$INSTDIR\${MAINBINARYNAME}.exe"
    ${EndIf}
    ; Tauri may already have updated the target above. Repair an explicit old icon too.
    !insertmacro IsShortcutTarget "${DIRECTORY}\$8" "$INSTDIR\${MAINBINARYNAME}.exe"
    Pop $9
    ${If} $9 == 1
      !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
      ${If} $0 P<> 0
        ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}", .r1)'
        ${If} $1 P<> 0
          ${IPersistFile::Load} $1 '("${DIRECTORY}\$8", ${STGM_READWRITE})'
          ${IShellLink::GetIconLocation} $0 '(.r2, ${NSIS_MAX_STRLEN}, .r3)'
          ${If} $2 == "$INSTDIR\app.exe"
            ${IShellLink::SetIconLocation} $0 '("$INSTDIR\${MAINBINARYNAME}.exe", r3)'
            ${IPersistFile::Save} $1 '("${DIRECTORY}\$8", 1)'
          ${EndIf}
          ${IUnknown::Release} $1 ""
        ${EndIf}
        ${IUnknown::Release} $0 ""
      ${EndIf}
    ${EndIf}
    FindNext $7 $8
    Goto mona_link_next_${ID}
  mona_link_done_${ID}:
  FindClose $7
!macroend

!macro NSIS_HOOK_POSTINSTALL
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $7
  Push $8
  Push $9
  !insertmacro MonaMigrateRunKey Run
  !insertmacro MonaMigrateRunKey RunOnce
  !insertmacro MonaMigrateLinks "$SMPROGRAMS" startmenu
  !insertmacro MonaMigrateLinks "$SMPROGRAMS\$AppStartMenuFolder" startmenufolder
  !insertmacro MonaMigrateLinks "$DESKTOP" desktop
  !insertmacro MonaMigrateLinks "$SMSTARTUP" startup
  ; Per-user default: no elevation and no Startup-folder shortcut.
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "MONA-HUB" '$\"$INSTDIR\${MAINBINARYNAME}.exe$\"'
  Pop $9
  Pop $8
  Pop $7
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "MONA-HUB"
!macroend
