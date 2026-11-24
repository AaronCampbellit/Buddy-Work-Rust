@echo off
setlocal
cd /d "%~dp0"
if not defined VSCMD_ARG_TGT_ARCH (
  if not defined BUDDY_VCVARS set "BUDDY_VCVARS=C:\BuildTools\VS2022\VC\Auxiliary\Build\vcvars64.bat"
)
if not defined VSCMD_ARG_TGT_ARCH call "%BUDDY_VCVARS%"
if errorlevel 1 exit /b 1
set "RUSTFLAGS=-C target-feature=+crt-static"
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
 "%USERPROFILE%\.cargo\bin\cargo.exe" %*
) else (
 cargo %*
)
exit /b %errorlevel%
