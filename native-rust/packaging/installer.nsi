Unicode true
!include LogicLib.nsh
!include Win\COM.nsh
Name "Buddy"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Buddy"
!define Registration "Software\Microsoft\Windows\CurrentVersion\Uninstall\BuddyWorkRustPrototype"
!define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"
; Keep the existing startup identifier so Windows retains the user's startup approval.
!define RunValue "Buddy Work Rust"
InstallDirRegKey HKCU "${Registration}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
VIProductVersion "${BUDDYFILEVERSION}"
VIAddVersionKey /LANG=1033 "ProductName" "Buddy"
VIAddVersionKey /LANG=1033 "FileDescription" "Buddy"
VIAddVersionKey /LANG=1033 "FileVersion" "${BUDDYVERSION}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${BUDDYVERSION}"
Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles

Var ShortcutPath
Var ShortcutTarget

; Other editions may have their own Buddy.lnk. Check its target without resolving
; or launching it. An unreadable shortcut belongs to someone else.
!macro ShortcutFunctions Prefix
Function ${Prefix}ReadShortcutTarget
  StrCpy $ShortcutTarget ""
  IfFileExists "$ShortcutPath" 0 done
  !insertmacro ComHlpr_CreateInProcInstance ${CLSID_ShellLink} ${IID_IShellLink} r0 ""
  ${If} $0 P<> 0
    ${IUnknown::QueryInterface} $0 '("${IID_IPersistFile}",.r1)'
    ${If} $1 P<> 0
      ${IPersistFile::Load} $1 '("$ShortcutPath",0).r2'
      ${If} $2 = 0
        ${IShellLink::GetPath} $0 '(.r3,${NSIS_MAX_STRLEN},0,0).r2'
        ${If} $2 = 0
          StrCpy $ShortcutTarget $3
        ${EndIf}
      ${EndIf}
      ${IUnknown::Release} $1 ""
    ${EndIf}
    ${IUnknown::Release} $0 ""
  ${EndIf}
  done:
FunctionEnd

Function ${Prefix}DeleteOwnedShortcut
  Call ${Prefix}ReadShortcutTarget
  ${If} $ShortcutTarget == "$INSTDIR\Buddy.exe"
  ${OrIf} $ShortcutTarget == "$INSTDIR\BuddyRust.exe"
    Delete "$ShortcutPath"
  ${EndIf}
FunctionEnd
!macroend
!insertmacro ShortcutFunctions ""
!insertmacro ShortcutFunctions "un."

!macro EnsureClosed
  System::Call 'kernel32::OpenMutexW(i 0x100000, i 0, w "Local\BuddyWorkRust.SingleInstance") p.r0'
  StrCmp $0 0 closed
  System::Call 'kernel32::CloseHandle(p r0)'
  MessageBox MB_ICONSTOP "Quit Buddy from its tray menu before continuing." /SD IDOK
  SetErrorLevel 2
  Abort
  closed:
!macroend
Function .onInit
  !insertmacro EnsureClosed
  ReadRegStr $0 HKCU "${Registration}" "InstallLocation"
  ${If} $0 == ""
  ${AndIf} $INSTDIR == "$LOCALAPPDATA\Programs\Buddy"
  ${AndIf} ${FileExists} "$INSTDIR\*.*"
  ${AndIfNot} ${FileExists} "$INSTDIR\BuddyWorkRust.marker"
    StrCpy $INSTDIR "$LOCALAPPDATA\Programs\Buddy-Work-Rust"
  ${EndIf}
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
    MessageBox MB_ICONSTOP "Choose an empty directory or an existing Buddy installation." /SD IDOK
    SetErrorLevel 3
    Abort
  nextEntry:
    ClearErrors
    FindNext $0 $1
    IfErrors emptyDirectory checkEntry
  emptyDirectory:
    FindClose $0
  install:
  StrCpy $ShortcutPath "$SMPROGRAMS\Buddy.lnk"
  ${If} ${FileExists} "$ShortcutPath"
    Call ReadShortcutTarget
    ${If} $ShortcutTarget != "$INSTDIR\Buddy.exe"
    ${AndIf} $ShortcutTarget != "$INSTDIR\BuddyRust.exe"
      StrCpy $ShortcutPath "$SMPROGRAMS\Buddy-Work-Rust\Buddy.lnk"
      ${If} ${FileExists} "$ShortcutPath"
        Call ReadShortcutTarget
        ${If} $ShortcutTarget != "$INSTDIR\Buddy.exe"
          MessageBox MB_ICONSTOP "The Buddy Start menu shortcut is already in use." /SD IDOK
          SetErrorLevel 4
          Abort
        ${EndIf}
      ${EndIf}
      CreateDirectory "$SMPROGRAMS\Buddy-Work-Rust"
    ${EndIf}
  ${EndIf}

  SetOutPath "$INSTDIR"
  File /r "${PAYLOAD}\*.*"
  FileOpen $0 "$INSTDIR\BuddyWorkRust.marker" w
  FileWrite $0 "BuddyWorkRustPrototype"
  FileClose $0
  WriteUninstaller "$INSTDIR\Uninstall Buddy.exe"
  CreateShortcut "$ShortcutPath" "$INSTDIR\Buddy.exe"
  ${If} $ShortcutPath == "$SMPROGRAMS\Buddy.lnk"
    StrCpy $ShortcutPath "$SMPROGRAMS\Buddy-Work-Rust\Buddy.lnk"
    Call DeleteOwnedShortcut
    RMDir "$SMPROGRAMS\Buddy-Work-Rust"
  ${EndIf}
  StrCpy $ShortcutPath "$SMPROGRAMS\Buddy Work Rust.lnk"
  Call DeleteOwnedShortcut
  ReadRegStr $0 HKCU "${RunKey}" "${RunValue}"
  ${If} $0 == '"$INSTDIR\BuddyRust.exe"'
    WriteRegStr HKCU "${RunKey}" "${RunValue}" '"$INSTDIR\Buddy.exe"'
  ${EndIf}
  Delete "$INSTDIR\BuddyRust.exe"
  Delete "$INSTDIR\Uninstall Buddy Work Rust.exe"
  WriteRegStr HKCU "${Registration}" "DisplayName" "Buddy"
  WriteRegStr HKCU "${Registration}" "DisplayVersion" "${BUDDYVERSION}"
  WriteRegStr HKCU "${Registration}" "DisplayIcon" "$INSTDIR\Buddy.exe"
  WriteRegStr HKCU "${Registration}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${Registration}" "UninstallString" '"$INSTDIR\Uninstall Buddy.exe"'
SectionEnd

Section "Uninstall"
  StrCpy $ShortcutPath "$SMPROGRAMS\Buddy.lnk"
  Call un.DeleteOwnedShortcut
  StrCpy $ShortcutPath "$SMPROGRAMS\Buddy-Work-Rust\Buddy.lnk"
  Call un.DeleteOwnedShortcut
  RMDir "$SMPROGRAMS\Buddy-Work-Rust"
  ReadRegStr $0 HKCU "${RunKey}" "${RunValue}"
  ${If} $0 == '"$INSTDIR\Buddy.exe"'
    DeleteRegValue HKCU "${RunKey}" "${RunValue}"
  ${EndIf}
  !include "${DELETEFILES}"
  Delete "$INSTDIR\BuddyWorkRust.marker"
  Delete "$INSTDIR\Uninstall Buddy.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${Registration}"
SectionEnd
