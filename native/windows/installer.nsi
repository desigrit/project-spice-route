Unicode true
ManifestDPIAware true
ManifestDPIAwareness PerMonitorV2
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 16

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "x64.nsh"
!include "WinVer.nsh"

!ifndef VERSION
  !error "Build with scripts/build-native-windows.ps1. VERSION is required."
!endif
!ifndef PUBLISH_DIR
  !error "PUBLISH_DIR is required."
!endif
!ifndef OUTPUT_FILE
  !error "OUTPUT_FILE is required."
!endif

!define PRODUCT_NAME "Spice Route"
; Keep the original 0.4.0 install identity so 1.4.0 upgrades it in place.
; These internal identifiers are intentionally not user-facing product names.
!define INSTALL_FOLDER "Spice Route Native Preview"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Spice Route Native Preview"
!define PRODUCT_MARKER "Spice Route Native Preview"
!define LEGACY_SHORTCUT "Spice Route Native Preview"

!macro RequireAppClosed executable
  ${If} ${FileExists} "$INSTDIR\app\${executable}"
    ; A running image can still allow an exclusive read handle. Request write
    ; access so an in-use executable reliably blocks upgrade and uninstall.
    System::Call 'kernel32::CreateFileW(w "$INSTDIR\app\${executable}", i 0x40000000, i 0, p 0, i 3, i 0, p 0) p.r0'
    ${If} $0 == -1
      MessageBox MB_OK|MB_ICONSTOP "Close Spice Route before changing its installation, then try again."
      Abort
    ${EndIf}
    System::Call 'kernel32::CloseHandle(p r0)'
  ${EndIf}
!macroend

Name "${PRODUCT_NAME}"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\${INSTALL_FOLDER}"
BrandingText "Spice Route"
VIProductVersion "${VERSION_QUAD}"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "Spice Route Windows installer"
VIAddVersionKey "LegalCopyright" "Copyright 2026 Spice Route contributors"

!define MUI_ICON "${PROJECT_ROOT}\src-tauri\icons\icon.ico"
!define MUI_UNICON "${PROJECT_ROOT}\src-tauri\icons\icon.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "Welcome to Spice Route"
!define MUI_WELCOMEPAGE_TEXT "Install Spice Route for your Windows account.$\r$\n$\r$\nAn existing 0.4.0 installation is updated in place. Your Codex history, workspaces, settings, and recovery data stay in place."
!define MUI_FINISHPAGE_TITLE "Spice Route is ready"
!define MUI_FINISHPAGE_TEXT "Open Spice Route from Start when you are ready.$\r$\n$\r$\nUse one Spice Route app at a time."
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  SetShellVarContext current
  SetRegView 64
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "Spice Route needs an x64 version of Windows."
    Abort
  ${EndIf}
  ${IfNot} ${AtLeastWin10}
    MessageBox MB_OK|MB_ICONSTOP "Spice Route needs Windows 10 version 1809 or later."
    Abort
  ${EndIf}
  ReadRegStr $0 HKLM "SOFTWARE\Microsoft\Windows NT\CurrentVersion" "CurrentBuildNumber"
  ${If} $0 < 17763
    MessageBox MB_OK|MB_ICONSTOP "Spice Route needs Windows 10 version 1809 or later."
    Abort
  ${EndIf}
  ; Use a dedicated fixed folder. Never replace the existing Tauri installation.
  StrCpy $INSTDIR "$LOCALAPPDATA\${INSTALL_FOLDER}"
  System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR") i.r0'
  ${If} $0 != -1
    IntOp $1 $0 & 0x400
    ${If} $1 != 0
      MessageBox MB_OK|MB_ICONSTOP "The Spice Route installation folder is a filesystem link. Choose a regular installation folder before continuing."
      Abort
    ${EndIf}
  ${EndIf}
  System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR\app") i.r0'
  ${If} $0 != -1
    IntOp $1 $0 & 0x400
    ${If} $1 != 0
      MessageBox MB_OK|MB_ICONSTOP "The Spice Route app folder is a filesystem link. No files were installed."
      Abort
    ${EndIf}
  ${EndIf}
  !insertmacro RequireAppClosed "SpiceRoute.exe"
  !insertmacro RequireAppClosed "SpiceRoute.Engine.exe"
FunctionEnd

Section "Spice Route"
  SetShellVarContext current
  SetRegView 64
  SetOutPath "$INSTDIR\app"
  SetOverwrite on
  ClearErrors
  File /r "${PUBLISH_DIR}\*"
  IfErrors installation_failed
  SetOutPath "$INSTDIR"
  FileOpen $0 "$INSTDIR\.spice-route-native-installed" w
  FileWrite $0 "${PRODUCT_MARKER}"
  FileClose $0
  WriteUninstaller "$INSTDIR\uninstall.exe"
  Delete "$SMPROGRAMS\${LEGACY_SHORTCUT}.lnk"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}.lnk" "$INSTDIR\app\SpiceRoute.exe" "" "$INSTDIR\app\SpiceRoute.exe" 0
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Spice Route"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\app\SpiceRoute.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "https://github.com/desigrit/project-spice-route"
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" ${ESTIMATED_SIZE_KB}
  Goto installation_complete
installation_failed:
  MessageBox MB_OK|MB_ICONSTOP "The app files could not be installed. Close Spice Route and run this installer again."
  Abort
installation_complete:
SectionEnd

Function un.onInit
  SetShellVarContext current
  SetRegView 64
  ; An uninstall only removes this product's own fixed, marked program folder.
  GetFullPathName $0 "$INSTDIR"
  GetFullPathName $1 "$LOCALAPPDATA\${INSTALL_FOLDER}"
  StrCmp $0 $1 0 unsafe_uninstall
  System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR") i.r0'
  IntOp $1 $0 & 0x400
  IntCmp $1 0 0 unsafe_uninstall unsafe_uninstall
  System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR\app") i.r0'
  ${If} $0 != -1
    IntOp $1 $0 & 0x400
    IntCmp $1 0 0 unsafe_uninstall unsafe_uninstall
  ${EndIf}
  ClearErrors
  FileOpen $0 "$INSTDIR\.spice-route-native-installed" r
  IfErrors unsafe_uninstall
  FileRead $0 $1
  FileClose $0
  StrCmp $1 "${PRODUCT_MARKER}" 0 unsafe_uninstall
  !insertmacro RequireAppClosed "SpiceRoute.exe"
  !insertmacro RequireAppClosed "SpiceRoute.Engine.exe"
  Return
unsafe_uninstall:
  MessageBox MB_OK|MB_ICONSTOP "The Spice Route installation folder could not be confirmed. No files were removed."
  Abort
FunctionEnd

Section "Uninstall"
  ; The shared com.spiceroute.codexsync profile and cloud folders are never removed.
  ClearErrors
  RMDir /r "$INSTDIR\app"
  ${If} ${Errors}
    MessageBox MB_OK|MB_ICONSTOP "Some app files could not be removed. Close Spice Route and try uninstalling again."
    Abort
  ${EndIf}
  Delete "$SMPROGRAMS\${PRODUCT_NAME}.lnk"
  Delete "$SMPROGRAMS\${LEGACY_SHORTCUT}.lnk"
  Delete "$INSTDIR\.spice-route-native-installed"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
SectionEnd
