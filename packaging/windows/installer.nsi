Unicode true
!include "MUI2.nsh"

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef SRCDIR
  !define SRCDIR "..\..\target\release"
!endif
!ifndef VENDORDIR
  !define VENDORDIR "..\..\vendor\windows"
!endif

!define APPNAME "Markd"

Name "${APPNAME} ${VERSION}"
OutFile "..\..\Markd-${VERSION}-windows-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\Markd"
InstallDirRegKey HKCU "Software\Markd" "InstallDir"
RequestExecutionLevel user
Icon "..\\..\\assets\\icon.ico"
UninstallIcon "..\\..\\assets\\icon.ico"

!define MUI_ABORTWARNING
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Install"
  SetOutPath "$INSTDIR"
  File "${SRCDIR}\markd.exe"
  ; 随包分发 Pandoc：exe 同目录的 vendor\pandoc.exe 会被程序自动找到。
  CreateDirectory "$INSTDIR\vendor\licenses"
  SetOutPath "$INSTDIR\vendor"
  File "${VENDORDIR}\pandoc.exe"
  SetOutPath "$INSTDIR\vendor\licenses"
  File "..\..\packaging\third-party\PANDOC-LICENSE.md"
  SetOutPath "$INSTDIR"
  CreateShortCut "$SMPROGRAMS\Markd.lnk" "$INSTDIR\markd.exe"
  CreateShortCut "$DESKTOP\Markd.lnk" "$INSTDIR\markd.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Markd" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Markd" "DisplayName" "Markd"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Markd" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Markd" "UninstallString" "$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "$INSTDIR\markd.exe"
  Delete "$INSTDIR\vendor\pandoc.exe"
  Delete "$INSTDIR\vendor\licenses\PANDOC-LICENSE.md"
  RMDir "$INSTDIR\vendor\licenses"
  RMDir "$INSTDIR\vendor"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Markd.lnk"
  Delete "$DESKTOP\Markd.lnk"
  DeleteRegKey HKCU "Software\Markd"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Markd"
SectionEnd
