; noah — Windows installer (NSIS). Per-user install: no admin prompt, like the
; Cursor / VS Code user installers. Double-click -> installs -> launches.
;
; Build (from Linux or Windows):
;   makensis -DVERSION=0.1.0 -DSTAGE=<dir with noah.exe> -DOUTFILE=<out.exe> script/installer/noah.nsi

Target amd64-unicode
SetCompressor /SOLID lzma

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef STAGE
  !error "STAGE (directory containing noah.exe) must be defined"
!endif
!ifndef OUTFILE
  !define OUTFILE "noah-windows-x86_64.exe"
!endif

!define APPNAME "noah"
!define PUBLISHER "#houseofasher"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APPNAME}"

Name "${APPNAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
RequestExecutionLevel user
BrandingText "${PUBLISHER}"
ShowInstDetails nevershow
ShowUninstDetails nevershow

!include "MUI2.nsh"
!include "FileFunc.nsh"

!define MUI_ICON "noah.ico"
!define MUI_UNICON "noah.ico"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\noah.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch noah"
!define MUI_FINISHPAGE_TITLE "noah is installed"
!define MUI_FINISHPAGE_TEXT "A shortcut is on your desktop and in the Start menu."

!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "${APPNAME} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "GPL-3.0 / Apache-2.0"

Section "Install"
  ; noah's own updater runs this with /S /UPDATE while noah is open: it has
  ; already moved its running files aside, so noah must not be closed, and the
  ; shortcuts the person arranged are left alone.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  IfErrors 0 updating
    nsExec::Exec 'taskkill /F /IM noah.exe'
    nsExec::Exec 'taskkill /F /IM agent-browser.exe'
  updating:

  SetOutPath "$INSTDIR"
  File /r "${STAGE}\*.*"
  File "noah.ico"

  ClearErrors
  ${GetOptions} $R0 "/UPDATE" $R1
  IfErrors 0 shortcuts_done
    CreateShortCut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\noah.exe" "" "$INSTDIR\noah.ico" 0
    CreateShortCut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\noah.exe" "" "$INSTDIR\noah.ico" 0
  shortcuts_done:

  WriteUninstaller "$INSTDIR\uninstall.exe"

  WriteRegStr HKCU "${UNINSTKEY}" "DisplayName" "${APPNAME}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTKEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTKEY}" "DisplayIcon" "$INSTDIR\noah.ico"
  WriteRegStr HKCU "${UNINSTKEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  nsExec::Exec 'taskkill /F /IM noah.exe'
  nsExec::Exec 'taskkill /F /IM agent-browser.exe'
  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  Delete "$DESKTOP\${APPNAME}.lnk"
  RMDir /r "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTKEY}"
SectionEnd
