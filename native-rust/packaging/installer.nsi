Unicode true
Name "Buddy Work (Rust Prototype)"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Buddy Work Rust"
!define Registration "Software\Microsoft\Windows\CurrentVersion\Uninstall\BuddyWorkRustPrototype"
InstallDirRegKey HKCU "${Registration}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

!macro EnsureClosed
  System::Call 'kernel32::OpenMutexW(i 0x100000, i 0, w "Local\BuddyWorkRust.SingleInstance") p.r0'
  StrCmp $0 0 closed
  System::Call 'kernel32::CloseHandle(p r0)'
  MessageBox MB_ICONSTOP "Quit Buddy Work Rust from its tray menu before continuing." /SD IDOK
  SetErrorLevel 2
  Abort
  closed:
!macroend
Function .onInit
  !insertmacro EnsureClosed
FunctionEnd
Function un.onInit
  !insertmacro EnsureClosed
FunctionEnd

Section
  IfFileExists "$INSTDIR\BuddyWorkRust.marker" install
  ClearErrors
  FindFirst $0 $1 "$INSTDIR\*"
  IfErrors install
  checkEntry:
    StrCmp $1 "." nextEntry
    StrCmp $1 ".." nextEntry
    FindClose $0
    MessageBox MB_ICONSTOP "Choose an empty directory or an existing Buddy Work Rust installation." /SD IDOK
    SetErrorLevel 3
    Abort
  nextEntry:
    ClearErrors
    FindNext $0 $1
    IfErrors emptyDirectory checkEntry
  emptyDirectory:
    FindClose $0
  install:
  SetOutPath "$INSTDIR"
  File /r "${PAYLOAD}\*.*"
  FileOpen $0 "$INSTDIR\BuddyWorkRust.marker" w
  FileWrite $0 "BuddyWorkRustPrototype"
  FileClose $0
  WriteUninstaller "$INSTDIR\Uninstall Buddy Work Rust.exe"
  CreateShortcut "$SMPROGRAMS\Buddy Work Rust.lnk" "$INSTDIR\BuddyRust.exe"
  WriteRegStr HKCU "${Registration}" "DisplayName" "Buddy Work (Rust Prototype)"
  WriteRegStr HKCU "${Registration}" "DisplayVersion" "${BUDDYVERSION}"
  WriteRegStr HKCU "${Registration}" "DisplayIcon" "$INSTDIR\BuddyRust.exe"
  WriteRegStr HKCU "${Registration}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${Registration}" "UninstallString" '"$INSTDIR\Uninstall Buddy Work Rust.exe"'
SectionEnd

Section "Uninstall"
  Delete "$SMPROGRAMS\Buddy Work Rust.lnk"
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Buddy Work Rust"
  StrCmp $0 '"$INSTDIR\BuddyRust.exe"' 0 +2
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Buddy Work Rust"
  !include "${DELETEFILES}"
  Delete "$INSTDIR\BuddyWorkRust.marker"
  Delete "$INSTDIR\Uninstall Buddy Work Rust.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${Registration}"
SectionEnd
