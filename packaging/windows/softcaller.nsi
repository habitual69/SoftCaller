; Softcaller NSIS installer, per user (no administrator rights).
;
; Build with packaging/windows/build-nsis.ps1, or directly:
;   makensis /DVERSION=0.1.0 /DBINARY_DIR=..\..\target\release ^
;            /DOUTFILE=..\..\dist\Softcaller-0.1.0-x64-setup.exe softcaller.nsi
;
; The app stores its settings, recordings and logs in
; %APPDATA%\habitual69\Softcaller and %LOCALAPPDATA%\habitual69\Softcaller.
; Uninstalling leaves those folders in place.

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"
!include "x64.nsh"

!ifndef VERSION
    !define VERSION "0.1.0"
!endif
!ifndef BINARY_DIR
    !define BINARY_DIR "..\..\target\release"
!endif
!ifndef OUTFILE
    !define OUTFILE "..\..\dist\Softcaller-${VERSION}-x64-setup.exe"
!endif

!define PRODUCT_NAME "Softcaller"
!define PUBLISHER "habitual69"
!define APP_EXE "softcaller.exe"
!define APP_KEY "Software\Softcaller"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Softcaller"
!define APP_PATH_KEY "Software\Microsoft\Windows\CurrentVersion\App Paths\softcaller.exe"
!define PROJECT_URL "https://github.com/habitual69/softcaller"

Name "${PRODUCT_NAME}"
Caption "${PRODUCT_NAME} ${VERSION} Setup"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${PRODUCT_NAME}"
InstallDirRegKey HKCU "${APP_KEY}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 64
BrandingText "${PRODUCT_NAME} ${VERSION}"
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${PRODUCT_NAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "${PRODUCT_NAME} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "GPL-3.0-only"

!define MUI_ICON "${__FILEDIR__}\..\icons\softcaller.ico"
!define MUI_UNICON "${__FILEDIR__}\..\icons\softcaller.ico"
!define MUI_ABORTWARNING
!define MUI_WELCOMEPAGE_TITLE "Welcome to ${PRODUCT_NAME}"
!define MUI_WELCOMEPAGE_TEXT "This installs ${PRODUCT_NAME} ${VERSION}, a SIP softphone for standard VoIP providers and JioFiber/AirFiber lines.$\r$\n$\r$\nIt installs for the current user only and does not need administrator rights."
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"
!define MUI_FINISHPAGE_RUN_TEXT "Launch ${PRODUCT_NAME}"
!define MUI_FINISHPAGE_LINK "${PRODUCT_NAME} on GitHub"
!define MUI_FINISHPAGE_LINK_LOCATION "${PROJECT_URL}"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "${__FILEDIR__}\..\..\LICENSE"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

; ---------------------------------------------------------------------------
; Installation

Section "Softcaller (required)" SEC_MAIN
    SectionIn RO
    SetOutPath "$INSTDIR"
    File "${BINARY_DIR}\${APP_EXE}"
    ; Runtime DLLs, when the pjproject build needs them (static builds have none).
    File /nonfatal "${BINARY_DIR}\*.dll"
    File "${__FILEDIR__}\..\..\LICENSE"
    WriteUninstaller "$INSTDIR\uninstall.exe"

    WriteRegStr HKCU "${APP_KEY}" "InstallDir" "$INSTDIR"
    WriteRegStr HKCU "${APP_PATH_KEY}" "" "$INSTDIR\${APP_EXE}"
    WriteRegStr HKCU "${APP_PATH_KEY}" "Path" "$INSTDIR"

    CreateDirectory "$SMPROGRAMS\${PRODUCT_NAME}"
    CreateShortcut "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
    CreateShortcut "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall ${PRODUCT_NAME}.lnk" "$INSTDIR\uninstall.exe"

    WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${PRODUCT_NAME} ${VERSION}"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
    WriteRegStr HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
    WriteRegStr HKCU "${UNINST_KEY}" "URLInfoAbout" "${PROJECT_URL}"
    WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE},0"
    WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
    WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
    WriteRegStr HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
    WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
    WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1
    ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
    IntFmt $0 "0x%08X" $0
    WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" "$0"
SectionEnd

Section "Desktop shortcut" SEC_DESKTOP
    CreateShortcut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\${APP_EXE}" "" "$INSTDIR\${APP_EXE}" 0
SectionEnd

Section "Open sip:, tel: and callto: links" SEC_PROTOCOLS
    ; HKCU only: the app can also change these from Settings. The per-user
    ; registration needs no administrator rights and matches the install mode.
    !macro RegisterProtocol scheme description
        WriteRegStr HKCU "Software\Classes\${scheme}" "" "${description}"
        WriteRegStr HKCU "Software\Classes\${scheme}" "URL Protocol" ""
        WriteRegStr HKCU "Software\Classes\${scheme}\DefaultIcon" "" "$INSTDIR\${APP_EXE},0"
        WriteRegStr HKCU "Software\Classes\${scheme}\shell\open\command" "" '"$INSTDIR\${APP_EXE}" "%1"'
    !macroend
    !insertmacro RegisterProtocol "sip" "URL:Softcaller SIP call"
    !insertmacro RegisterProtocol "tel" "URL:Softcaller telephone call"
    !insertmacro RegisterProtocol "callto" "URL:Softcaller call"
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
    !insertmacro MUI_DESCRIPTION_TEXT ${SEC_MAIN} "The Softcaller application, Start Menu shortcuts and uninstaller."
    !insertmacro MUI_DESCRIPTION_TEXT ${SEC_DESKTOP} "A shortcut on the desktop."
    !insertmacro MUI_DESCRIPTION_TEXT ${SEC_PROTOCOLS} "Let sip:, tel: and callto: links open Softcaller."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Function .onInit
    ${IfNot} ${RunningX64}
        MessageBox MB_ICONSTOP "Softcaller requires 64-bit Windows."
        Abort
    ${EndIf}
    Call CheckSoftcallerClosed
FunctionEnd

Function un.onInit
    Call un.CheckSoftcallerClosed
FunctionEnd

; The application locks softcaller.exe while it runs; replacing it would fail.
Function CheckSoftcallerClosed
    nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq softcaller.exe" /NH | find /I "softcaller.exe"'
    Pop $0
    Pop $1
    ${If} $0 == 0
        MessageBox MB_ICONSTOP "Softcaller is running. Close it, then run this again."
        Abort
    ${EndIf}
FunctionEnd

Function un.CheckSoftcallerClosed
    nsExec::ExecToStack 'cmd /c tasklist /FI "IMAGENAME eq softcaller.exe" /NH | find /I "softcaller.exe"'
    Pop $0
    Pop $1
    ${If} $0 == 0
        MessageBox MB_ICONSTOP "Softcaller is running. Close it, then uninstall again."
        Abort
    ${EndIf}
FunctionEnd


; ---------------------------------------------------------------------------
; Uninstallation

Section "Uninstall"
    Delete "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk"
    Delete "$SMPROGRAMS\${PRODUCT_NAME}\Uninstall ${PRODUCT_NAME}.lnk"
    RMDir "$SMPROGRAMS\${PRODUCT_NAME}"
    Delete "$DESKTOP\${PRODUCT_NAME}.lnk"

    ; Remove a protocol only when it still points at this installation.
    !macro RemoveProtocol scheme
        ReadRegStr $0 HKCU "Software\Classes\${scheme}\shell\open\command" ""
        StrCmp $0 '$\"$INSTDIR\${APP_EXE}$\" $\"%1$\"' 0 +2
            DeleteRegKey HKCU "Software\Classes\${scheme}"
    !macroend
    !insertmacro RemoveProtocol "sip"
    !insertmacro RemoveProtocol "tel"
    !insertmacro RemoveProtocol "callto"

    DeleteRegKey HKCU "${APP_PATH_KEY}"
    DeleteRegKey HKCU "${UNINST_KEY}"
    DeleteRegKey HKCU "${APP_KEY}"

    Delete "$INSTDIR\${APP_EXE}"
    Delete "$INSTDIR\*.dll"
    Delete "$INSTDIR\LICENSE"
    Delete "$INSTDIR\uninstall.exe"
    RMDir "$INSTDIR"

    ; %APPDATA%\habitual69\Softcaller and %LOCALAPPDATA%\habitual69\Softcaller
    ; stay: settings, recordings and logs belong to the user.
SectionEnd
