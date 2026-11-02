; Installer hooks (see `bundle.windows.nsis.installerHooks` in tauri.conf.json).
;
; Uninstalling removes what mouce left outside its own folders. None of this runs when a
; newer installer replaces mouce (`$UpdateMode`), so "Start with Windows" and saved keys
; survive an update.

!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
    ; "Start with Windows" (tauri-plugin-autostart, named after the product) would point
    ; at the exe that was just removed.
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "mouce"
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "mouce"

    ; The API keys live in Windows Credential Manager as `<provider>.com.mouce.app`
    ; (see secrets.rs). They go with the rest of the app's data, when the user ticks
    ; "Delete the application data".
    ${If} $DeleteAppDataCheckboxState = 1
      nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:openai.com.mouce.app'
      Pop $0
      nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:groq.com.mouce.app'
      Pop $0
      nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:gemini.com.mouce.app'
      Pop $0
      nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:anthropic.com.mouce.app'
      Pop $0
    ${EndIf}
  ${EndIf}
!macroend
