; XT Screen Translator Installer
!include "MUI2.nsh"
!include "x64.nsh"

; Basic Settings
Name "XT Screen Translator"
OutFile "target\release\xt-screen-translator-installer.exe"
InstallDir "$PROGRAMFILES\XTScreenTranslator"
RequestExecutionLevel admin
Icon ".\assets\app.ico"

; MUI Settings
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

; Installer Sections
Section "Install Application"
  SetOutPath "$INSTDIR"
  
  ; Copy main executable
  File "target\release\xt-screen-translator.exe"
  
  ; Copy Visual C++ Runtime and install it
  File "vc_redist.x64.exe"
  DetailPrint "Installing Visual C++ Runtime..."
  ExecWait "$INSTDIR\vc_redist.x64.exe /quiet /norestart" $0
  Delete "$INSTDIR\vc_redist.x64.exe"
  
  ; Create Start Menu shortcut
  CreateDirectory "$SMPROGRAMS\XT Screen Translator"
  CreateShortcut "$SMPROGRAMS\XT Screen Translator\XT Screen Translator.lnk" "$INSTDIR\xt-screen-translator.exe"
  CreateShortcut "$SMPROGRAMS\XT Screen Translator\Uninstall.lnk" "$INSTDIR\uninstall.exe"
  
  ; Create Desktop shortcut (optional, uncomment if desired)
  ; CreateShortcut "$DESKTOP\XT Screen Translator.lnk" "$INSTDIR\xt-screen-translator.exe"
  
  ; Write uninstaller
  WriteUninstaller "$INSTDIR\uninstall.exe"
  
  ; Write registry entry for Add/Remove Programs
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XTScreenTranslator" "DisplayName" "XT Screen Translator"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XTScreenTranslator" "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XTScreenTranslator" "InstallLocation" "$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XTScreenTranslator" "DisplayVersion" "1.6"
SectionEnd

; Uninstaller Section
Section "Uninstall"
  Delete "$INSTDIR\xt-screen-translator.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  
  Delete "$SMPROGRAMS\XT Screen Translator\XT Screen Translator.lnk"
  Delete "$SMPROGRAMS\XT Screen Translator\Uninstall.lnk"
  RMDir "$SMPROGRAMS\XT Screen Translator"
  
  Delete "$DESKTOP\XT Screen Translator.lnk"
  
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\XTScreenTranslator"
SectionEnd
