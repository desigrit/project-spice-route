@echo off
setlocal
set "SPICE_VSDEVCMD="
if defined VSINSTALLDIR if exist "%VSINSTALLDIR%\Common7\Tools\VsDevCmd.bat" set "SPICE_VSDEVCMD=%VSINSTALLDIR%\Common7\Tools\VsDevCmd.bat"
if not defined SPICE_VSDEVCMD if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" (
  for /f "usebackq tokens=*" %%I in (`"%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "SPICE_VSDEVCMD=%%I\Common7\Tools\VsDevCmd.bat"
)
if not defined SPICE_VSDEVCMD if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" set "SPICE_VSDEVCMD=%ProgramFiles(x86)%\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat"
if not defined SPICE_VSDEVCMD (
  echo Visual Studio C++ x64 tools were not found. 1>&2
  exit /b 1
)
set "SPICE_HOST_ARCH=x64"
if /I "%PROCESSOR_ARCHITECTURE%"=="ARM64" set "SPICE_HOST_ARCH=arm64"
call "%SPICE_VSDEVCMD%" -no_logo -arch=x64 -host_arch=%SPICE_HOST_ARCH%
if errorlevel 1 exit /b %errorlevel%
set "RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc"
%*
