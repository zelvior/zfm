; Per-user installer: no administrator access and no third-party runtime required.
Unicode true
!include "MUI2.nsh"
!include "x64.nsh"
!include "LogicLib.nsh"
!ifndef APP_BINARY
  !error "APP_BINARY must name the built zfm.exe"
!endif
!ifndef OUTFILE
  !error "OUTFILE must name the installer to create"
!endif
!ifndef VERSION
  !error "VERSION must contain the release tag"
!endif
!ifndef ARCH
  !error "ARCH must be x86 or x64"
!endif
Name "ZFM File Manager"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\ZFM"
InstallDirRegKey HKCU "Software\ZFM" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
ShowUninstDetails show

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function .onInit
  !if "${ARCH}" == "x64"
    ${IfNot} ${RunningX64}
      MessageBox MB_ICONSTOP "This installer is for 64-bit Windows. Download the 32-bit (i686) installer instead."
      Abort
    ${EndIf}
  !endif
FunctionEnd

Section "Install"
  SetShellVarContext current
  ; Upgrade only a directory previously registered by ZFM. Refuse to replace
  ; unrelated files if the user selects some other existing directory.
  ReadRegStr $0 HKCU "Software\ZFM" "InstallDir"
  StrCmp $0 $INSTDIR zfm_safe_install
  IfFileExists "$INSTDIR\zfm.exe" zfm_install_conflict
  IfFileExists "$INSTDIR\Uninstall.exe" zfm_install_conflict
  IfFileExists "$INSTDIR\zfm.ico" zfm_install_conflict
  IfFileExists "$INSTDIR\LICENSE.txt" zfm_install_conflict
  IfFileExists "$INSTDIR\README.txt" zfm_install_conflict zfm_safe_install
zfm_install_conflict:
  MessageBox MB_ICONSTOP|MB_OK "The selected folder already contains files that ZFM would replace. Choose an empty folder." /SD IDOK
  SetErrorLevel 1
  Abort
zfm_safe_install:
  SetOutPath "$INSTDIR"
  File /oname=zfm.exe "${APP_BINARY}"
  File /oname=zfm.ico "packaging\zfm.ico"
  File /oname=LICENSE.txt "LICENSE"
  File /oname=README.txt "README.md"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\ZFM"
  CreateShortCut "$SMPROGRAMS\ZFM\ZFM File Manager.lnk" "$INSTDIR\zfm.exe" "" "$INSTDIR\zfm.ico"
  CreateShortCut "$SMPROGRAMS\ZFM\Uninstall ZFM.lnk" "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\ZFM" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM" "DisplayName" "ZFM File Manager"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM" "Publisher" "ZFM contributors"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM" "DisplayIcon" "$INSTDIR\zfm.ico"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM" "QuietUninstallString" '"$INSTDIR\Uninstall.exe" /S'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  Delete "$SMPROGRAMS\ZFM\ZFM File Manager.lnk"
  Delete "$SMPROGRAMS\ZFM\Uninstall ZFM.lnk"
  RMDir "$SMPROGRAMS\ZFM"
  Delete "$INSTDIR\zfm.exe"
  Delete "$INSTDIR\zfm.ico"
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\README.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\ZFM"
  DeleteRegValue HKCU "Software\ZFM" "InstallDir"
  DeleteRegKey /ifempty HKCU "Software\ZFM"
SectionEnd
