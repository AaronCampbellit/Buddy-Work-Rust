!macro customInstall
  ; Upgrade a previously installed optional camera component with the app's pair.
  Push $R0
  Push $R1
  SetRegView 64
  ReadRegStr $R0 HKLM "Software\Classes\CLSID\{92449F29-F0C0-48FA-A725-5F51EB1D9486}\InprocServer32" ""
  SetRegView lastused
  ${If} $R0 != ""
    nsExec::ExecToStack '"$INSTDIR\resources\camera\BuddyCameraHost.exe" --install'
    Pop $R0
    Pop $R1
    ${If} $R0 != "0"
      DetailPrint "Buddy Camera upgrade failed: $R1"
      MessageBox MB_OK|MB_ICONSTOP "Buddy Camera could not be upgraded. Quit Buddy and other camera apps, then run this installer again." /SD IDOK
      SetErrorLevel 2
      Abort
    ${EndIf}
  ${EndIf}
  ClearErrors
  Pop $R1
  Pop $R0
!macroend

!macro customUnInstall
  nsExec::ExecToLog '"$INSTDIR\resources\camera\BuddyCameraHost.exe" --remove-available'
!macroend
