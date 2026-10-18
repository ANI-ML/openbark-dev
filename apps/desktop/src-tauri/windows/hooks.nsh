; OpenBark's hooks for the Windows installer (Tauri NSIS hook macros).
;
; Uninstalling keeps the clinic's data unless the uninstaller's "Delete the
; application data" box is ticked. Then, after one more confirmation, it also
; removes %LOCALAPPDATA%\OpenBark (encounters, recordings, notes, models,
; templates, team members, the key file) and the data key in Windows
; Credential Manager ("data-key.OpenBark", see engine/os-engine/src/vault.rs).
; An update (/UPDATE) or a silent uninstall never deletes data.
;
; The Start menu folder also gets "Uninstall OpenBark", so the way out is as
; easy to find as the way in.

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro MUI_STARTMENU_WRITE_BEGIN Application
    CreateDirectory "$SMPROGRAMS\$AppStartMenuFolder"
    CreateShortcut "$SMPROGRAMS\$AppStartMenuFolder\Uninstall ${PRODUCTNAME}.lnk" "$INSTDIR\uninstall.exe"
  !insertmacro MUI_STARTMENU_WRITE_END
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    !insertmacro MUI_STARTMENU_GETFOLDER Application $R9
    Delete "$SMPROGRAMS\$R9\Uninstall ${PRODUCTNAME}.lnk"
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
  ${AndIfNot} ${Silent}
    MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 \
      "Also delete everything OpenBark stored on this computer?$\r$\n$\r$\nThis deletes every encounter, recording, note, model, template and team member in$\r$\n$LOCALAPPDATA\OpenBark$\r$\nand the encryption key. Copies of encounters kept elsewhere can then only be opened with your recovery key (Settings, Encryption).$\r$\n$\r$\nChoose No to keep your data." \
      IDYES openbark_delete_data IDNO openbark_keep_data
    openbark_delete_data:
      nsExec::Exec 'cmdkey /delete:data-key.OpenBark'
      Pop $0
      RMDir /r "$LOCALAPPDATA\OpenBark"
      Goto openbark_done
    openbark_keep_data:
      DetailPrint "Kept your data in $LOCALAPPDATA\OpenBark"
    openbark_done:
  ${EndIf}
!macroend
