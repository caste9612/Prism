; Prism: hooks of the NSIS installer made by Tauri (T10, F22).
;
; - prism-shell.exe, the small non-elevated helper of the Explorer menu, goes next to prism.exe
;   (it is built separately: see scripts\build-installer.ps1);
; - after installing, it registers the Explorer menu entries for the user and the scheduled
;   task that starts Prism at logon and on demand without a UAC prompt (F18-AC3, F22-AC1);
; - before uninstalling, it removes them (F22-AC2). Whether the catalog is kept is asked by the
;   uninstaller of Tauri itself ("Delete the application data").
;
; Paths in File are relative to the generated installer.nsi (target\release\nsis\x64).

!macro NSIS_HOOK_POSTINSTALL
  File "/oname=$INSTDIR\prism-shell.exe" "..\..\prism-shell.exe"
  ExecWait '"$INSTDIR\prism-shell.exe" --install'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ExecWait '"$INSTDIR\prism-shell.exe" --uninstall'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete "$INSTDIR\prism-shell.exe"
!macroend
