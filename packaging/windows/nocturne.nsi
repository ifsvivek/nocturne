; NSIS installer for NOCTURNE (built by scripts/package-windows.sh; makensis runs in a container).
; Per-user by default: no administrator prompt, installs under %LOCALAPPDATA%\Programs.
; Defines passed on the command line: VERSION, STAGE (dir with the files), OUTFILE.
Unicode true
!include "MUI2.nsh"

Name "NOCTURNE"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\NOCTURNE"
InstallDirRegKey HKCU "Software\NOCTURNE" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
SetCompressorDictSize 64
VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "NOCTURNE"
VIAddVersionKey "FileDescription" "NOCTURNE installer"
VIAddVersionKey "LegalCopyright" "GPL-3.0-or-later"
VIAddVersionKey "FileVersion" "${VERSION}"

!define MUI_ICON "${STAGE}\nocturne.ico"
!define MUI_UNICON "${STAGE}\nocturne.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\nocturne.exe"
!insertmacro MUI_PAGE_LICENSE "${STAGE}\LICENSE.txt"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "NOCTURNE"
  SetOutPath "$INSTDIR"
  File "${STAGE}\nocturne.exe"
  File "${STAGE}\mpv.exe"
  File "${STAGE}\nocturne.ico"
  File "${STAGE}\LICENSE.txt"
  File "${STAGE}\THIRD_PARTY_NOTICES.md"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  CreateDirectory "$SMPROGRAMS\NOCTURNE"
  CreateShortcut "$SMPROGRAMS\NOCTURNE\NOCTURNE.lnk" "$INSTDIR\nocturne.exe" "" "$INSTDIR\nocturne.ico"

  WriteRegStr HKCU "Software\NOCTURNE" "InstallDir" "$INSTDIR"
  !define UNKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\NOCTURNE"
  WriteRegStr HKCU "${UNKEY}" "DisplayName" "NOCTURNE"
  WriteRegStr HKCU "${UNKEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNKEY}" "Publisher" "ifsvivek"
  WriteRegStr HKCU "${UNKEY}" "DisplayIcon" "$INSTDIR\nocturne.ico"
  WriteRegStr HKCU "${UNKEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNKEY}" "NoRepair" 1
SectionEnd

; Settings, the library index and tokens (Credential Manager) are the user's and are left in place.
Section "Uninstall"
  Delete "$INSTDIR\nocturne.exe"
  Delete "$INSTDIR\mpv.exe"
  Delete "$INSTDIR\nocturne.ico"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\THIRD_PARTY_NOTICES.md"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\NOCTURNE\NOCTURNE.lnk"
  RMDir "$SMPROGRAMS\NOCTURNE"
  DeleteRegKey HKCU "${UNKEY}"
  DeleteRegKey HKCU "Software\NOCTURNE"
SectionEnd
