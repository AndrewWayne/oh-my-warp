Unicode true

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinVer.nsh"
!include "x64.nsh"

!ifndef VERSION
  !error "VERSION is required"
!endif
!ifndef NUMERIC_VERSION
  !error "NUMERIC_VERSION is required"
!endif
!ifndef PAYLOAD_DIR
  !error "PAYLOAD_DIR is required"
!endif
!ifndef OUTPUT_FILE
  !error "OUTPUT_FILE is required"
!endif
!ifndef LICENSE_PATH
  !error "LICENSE_PATH is required"
!endif
!ifndef INSTALLER_ICON
  !error "INSTALLER_ICON is required"
!endif
!ifndef ESTIMATED_SIZE_KB
  !error "ESTIMATED_SIZE_KB is required"
!endif

!define PRODUCT_NAME "omw"
!define PRODUCT_EXE "omw-warp-oss.exe"
!define PRODUCT_ID "omw.local.warpOss"
!define PRODUCT_PUBLISHER "omw contributors"
!define PRODUCT_URL "https://github.com/AndrewWayne/oh-my-warp"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${PRODUCT_ID}"

Name "${PRODUCT_NAME}"
OutFile "${OUTPUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\omw"
InstallDirRegKey HKCU "${UNINSTALL_KEY}" "InstallLocation"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 64
SetOverwrite on
ShowInstDetails show
ShowUninstDetails show
BrandingText "omw"
ManifestSupportedOS all
ManifestDPIAware true
Icon "${INSTALLER_ICON}"
UninstallIcon "${INSTALLER_ICON}"

VIProductVersion "${NUMERIC_VERSION}"
VIAddVersionKey /LANG=1033 "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey /LANG=1033 "ProductVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "CompanyName" "${PRODUCT_PUBLISHER}"
VIAddVersionKey /LANG=1033 "FileDescription" "omw Windows installer"
VIAddVersionKey /LANG=1033 "FileVersion" "${VERSION}"
VIAddVersionKey /LANG=1033 "LegalCopyright" "Copyright (C) 2026 omw contributors"

!define MUI_ABORTWARNING
!define MUI_ICON "${INSTALLER_ICON}"
!define MUI_UNICON "${INSTALLER_ICON}"
!define MUI_FINISHPAGE_RUN "$INSTDIR\app\${PRODUCT_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch omw"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${LICENSE_PATH}"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

Var PreviousAppMoved

Function .onInit
  SetShellVarContext current
  SetRegView 64

  ${IfNot} ${RunningX64}
    IfSilent 0 +3
      SetErrorLevel 10
      Quit
    MessageBox MB_OK|MB_ICONSTOP "omw requires 64-bit Windows."
    Quit
  ${EndIf}

  ${IfNot} ${AtLeastWin10}
    IfSilent 0 +3
      SetErrorLevel 11
      Quit
    MessageBox MB_OK|MB_ICONSTOP "omw requires Windows 10 or newer."
    Quit
  ${EndIf}
FunctionEnd

Function EnsureMainExecutableWritable
check_lock:
  IfFileExists "$INSTDIR\app\${PRODUCT_EXE}" 0 not_locked
  ClearErrors
  FileOpen $0 "$INSTDIR\app\${PRODUCT_EXE}" a
  IfErrors locked
  FileClose $0

not_locked:
  Return

locked:
  IfSilent silent_locked
  MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION \
    "omw is still running. Close it and choose Retry to continue the upgrade." \
    IDRETRY check_lock
  SetErrorLevel 12
  Abort

silent_locked:
  SetErrorLevel 12
  Quit
FunctionEnd

Section "omw application (required)" SecApplication
  SectionIn RO
  Call EnsureMainExecutableWritable

  RMDir /r "$INSTDIR\app.new"
  RMDir /r "$INSTDIR\app.old"
  CreateDirectory "$INSTDIR\app.new"
  SetOutPath "$INSTDIR\app.new"
  ClearErrors
  File /r "${PAYLOAD_DIR}\*"
  IfErrors install_failed

  IfFileExists "$INSTDIR\app.new\${PRODUCT_EXE}" 0 install_failed
  IfFileExists "$INSTDIR\app.new\SHA256SUMS" 0 install_failed

  # Windows cannot rename a directory that is still the process working
  # directory after extraction.
  SetOutPath "$INSTDIR"
  StrCpy $PreviousAppMoved "0"
  IfFileExists "$INSTDIR\app\${PRODUCT_EXE}" 0 activate_new
  ClearErrors
  Rename "$INSTDIR\app" "$INSTDIR\app.old"
  IfErrors install_failed
  StrCpy $PreviousAppMoved "1"

activate_new:
  ClearErrors
  Rename "$INSTDIR\app.new" "$INSTDIR\app"
  IfErrors activate_failed
  RMDir /r "$INSTDIR\app.old"

  SetOutPath "$INSTDIR"
  WriteUninstaller "$INSTDIR\Uninstall.exe"

  CreateDirectory "$SMPROGRAMS\omw"
  CreateShortcut "$SMPROGRAMS\omw\omw.lnk" \
    "$INSTDIR\app\${PRODUCT_EXE}" "" "$INSTDIR\app\${PRODUCT_EXE}" 0
  CreateShortcut "$SMPROGRAMS\omw\Uninstall omw.lnk" \
    "$INSTDIR\Uninstall.exe" "" "$INSTDIR\Uninstall.exe" 0

  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\app\${PRODUCT_EXE},0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PRODUCT_PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "URLInfoAbout" "${PRODUCT_URL}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '$\"$INSTDIR\Uninstall.exe$\"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '$\"$INSTDIR\Uninstall.exe$\" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" ${ESTIMATED_SIZE_KB}
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1

  SetErrorLevel 0
  Goto install_done

activate_failed:
  ${If} $PreviousAppMoved == "1"
    Rename "$INSTDIR\app.old" "$INSTDIR\app"
  ${EndIf}

install_failed:
  RMDir /r "$INSTDIR\app.new"
  SetErrorLevel 13
  Abort "The omw payload could not be installed. The previous installation was preserved."

install_done:
SectionEnd

Section /o "Desktop shortcut" SecDesktopShortcut
  CreateShortcut "$DESKTOP\omw.lnk" \
    "$INSTDIR\app\${PRODUCT_EXE}" "" "$INSTDIR\app\${PRODUCT_EXE}" 0
SectionEnd

LangString DESC_SecApplication ${LANG_ENGLISH} \
  "Install the self-contained omw terminal, local agent, and runtime dependencies."
LangString DESC_SecDesktopShortcut ${LANG_ENGLISH} \
  "Create an omw shortcut on the current user's desktop."

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApplication} $(DESC_SecApplication)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecDesktopShortcut} $(DESC_SecDesktopShortcut)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Function un.onInit
  SetShellVarContext current
  SetRegView 64
FunctionEnd

Function un.EnsureMainExecutableWritable
check_lock:
  IfFileExists "$INSTDIR\app\${PRODUCT_EXE}" 0 not_locked
  ClearErrors
  FileOpen $0 "$INSTDIR\app\${PRODUCT_EXE}" a
  IfErrors locked
  FileClose $0

not_locked:
  Return

locked:
  IfSilent silent_locked
  MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION \
    "omw is still running. Close it and choose Retry to continue uninstalling." \
    IDRETRY check_lock
  SetErrorLevel 14
  Abort

silent_locked:
  SetErrorLevel 14
  Quit
FunctionEnd

Section "Uninstall"
  Call un.EnsureMainExecutableWritable

  Delete "$DESKTOP\omw.lnk"
  Delete "$SMPROGRAMS\omw\omw.lnk"
  Delete "$SMPROGRAMS\omw\Uninstall omw.lnk"
  RMDir "$SMPROGRAMS\omw"

  RMDir /r "$INSTDIR\app"
  RMDir /r "$INSTDIR\app.new"
  RMDir /r "$INSTDIR\app.old"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"

  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"

  DetailPrint "User settings, session data, and Credential Manager entries were preserved."
  SetErrorLevel 0
SectionEnd
