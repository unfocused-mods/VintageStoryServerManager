; Refresh only this application's shell items after replacing its executable.
; SHCNE_UPDATEITEM | SHCNF_PATHW | SHCNF_FLUSHNOWAIT does not clear global caches.
!macro VSM_REFRESH_SHELL_ITEM PATH
  IfFileExists "${PATH}" 0 +2
    System::Call 'shell32::SHChangeNotify(i 0x00002000, i 0x00003005, w "${PATH}", p 0)'
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro VSM_REFRESH_SHELL_ITEM "$INSTDIR\${MAINBINARYNAME}.exe"
  !if "${STARTMENUFOLDER}" != ""
    !insertmacro VSM_REFRESH_SHELL_ITEM "$SMPROGRAMS\$AppStartMenuFolder\${PRODUCTNAME}.lnk"
  !else
    !insertmacro VSM_REFRESH_SHELL_ITEM "$SMPROGRAMS\${PRODUCTNAME}.lnk"
  !endif
  !insertmacro VSM_REFRESH_SHELL_ITEM "$DESKTOP\${PRODUCTNAME}.lnk"
!macroend
