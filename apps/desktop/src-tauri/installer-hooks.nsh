; The Windows installer's hooks (D70 §5), included by Tauri's NSIS template.

; Before uninstalling, the browser helper undoes what the desktop's "Allow
; browser extensions to unlock with Windows Hello" switch did: the browsers'
; registry keys, the manifests, and every enrolment with its Windows Hello
; key (D69 §4).
;
; Not when this version is being replaced: the updater passes /UPDATE, and an
; installer of another version runs this uninstaller in place, from the
; install folder ($EXEDIR is $INSTDIR), then installs again. Uninstalling from
; Windows' Settings runs a copy from the temporary folder.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $EXEDIR != $INSTDIR
  ${AndIf} ${FileExists} "$INSTDIR\pwm-browser-helper.exe"
    nsExec::Exec '"$INSTDIR\pwm-browser-helper.exe" --uninstall'
    Pop $0
  ${EndIf}
!macroend
