; Instalador do Code Terminal para Windows (NSIS).
;
; Instala por usuario, sem pedir administrador, em
; %LOCALAPPDATA%\Programs\Code Terminal: os tres .exe, atalho no menu Iniciar,
; desinstalador (Configuracoes > Apps) e, opcional, a pasta no PATH do
; usuario (para o code-terminal-cli no terminal).
;
; Gerado por scripts/build-windows-installer.ps1:
;   makensis /DVERSION=0.1.1 /DSOURCE_DIR=...\target\release /DOUT_FILE=...\setup.exe code-terminal.nsi
;
; Este arquivo fica em ASCII (o makensis le a pagina de codigo do sistema).

Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"

!ifndef VERSION
  !define VERSION "0.0.0"
!endif
!ifndef SOURCE_DIR
  !define SOURCE_DIR "..\..\target\release"
!endif
!ifndef OUT_FILE
  !define OUT_FILE "CodeTerminal-${VERSION}-windows-x64-setup.exe"
!endif

!define APP_NAME "Code Terminal"
!define APP_KEY "CodeTerminal"
!define PUBLISHER "Codigo Central"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_KEY}"

Name "${APP_NAME}"
OutFile "${OUT_FILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "Software\${APP_KEY}" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
ShowInstDetails show
BrandingText "${APP_NAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "FileDescription" "Instalador do ${APP_NAME}"
VIAddVersionKey "LegalCopyright" "GPL-3.0-or-later"

!define MUI_ICON "code-terminal.ico"
!define MUI_UNICON "code-terminal.ico"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_SMALLDESC
!define MUI_FINISHPAGE_RUN "$INSTDIR\code-terminal.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Abrir o ${APP_NAME}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "PortugueseBR"
!insertmacro MUI_LANGUAGE "English"

Section "${APP_NAME}" SecMain
  SectionIn RO
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  ; O app aberto segura os .exe: fecha antes de copiar (atualizacao).
  nsExec::Exec 'taskkill /IM code-terminal.exe /F'
  nsExec::Exec 'taskkill /IM code-terminal-browser.exe /F'
  File "${SOURCE_DIR}\code-terminal.exe"
  File "${SOURCE_DIR}\code-terminal-cli.exe"
  File "${SOURCE_DIR}\code-terminal-browser.exe"
  File "code-terminal.ico"

  CreateDirectory "$SMPROGRAMS\${APP_NAME}"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk" "$INSTDIR\code-terminal.exe" "" "$INSTDIR\code-terminal.ico"
  CreateShortcut "$SMPROGRAMS\${APP_NAME}\Desinstalar ${APP_NAME}.lnk" "$INSTDIR\uninstall.exe"

  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\${APP_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\code-terminal.ico"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

; Opcional (vem marcado): a pasta no PATH do usuario, para usar o
; code-terminal-cli de qualquer terminal. Feito pelo PowerShell para nao
; truncar PATH longo (limite das strings do NSIS) nem depender de plugin.
Section "Adicionar ao PATH do usuario" SecPath
  nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d = '$INSTDIR'; $$p = [Environment]::GetEnvironmentVariable('Path', 'User'); $$parts = @($$p -split ';' | Where-Object { $$_ -ne '' }); if ($$parts -notcontains $$d) { [Environment]::SetEnvironmentVariable('Path', (($$parts + $$d) -join ';'), 'User') }"`
  WriteRegDWORD HKCU "Software\${APP_KEY}" "AddedToPath" 1
SectionEnd

!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecMain} "O terminal, a CLI (code-terminal-cli) e a janela do navegador."
  !insertmacro MUI_DESCRIPTION_TEXT ${SecPath} "Permite rodar code-terminal-cli em qualquer terminal."
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  SetShellVarContext current
  nsExec::Exec 'taskkill /IM code-terminal.exe /F'
  nsExec::Exec 'taskkill /IM code-terminal-browser.exe /F'

  ReadRegDWORD $0 HKCU "Software\${APP_KEY}" "AddedToPath"
  ${If} $0 == 1
    nsExec::ExecToLog `powershell -NoProfile -ExecutionPolicy Bypass -Command "$$d = '$INSTDIR'; $$p = [Environment]::GetEnvironmentVariable('Path', 'User'); $$parts = @($$p -split ';' | Where-Object { $$_ -ne '' -and $$_ -ne $$d }); [Environment]::SetEnvironmentVariable('Path', ($$parts -join ';'), 'User')"`
  ${EndIf}

  Delete "$INSTDIR\code-terminal.exe"
  Delete "$INSTDIR\code-terminal-cli.exe"
  Delete "$INSTDIR\code-terminal-browser.exe"
  Delete "$INSTDIR\code-terminal.ico"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\${APP_NAME}\${APP_NAME}.lnk"
  Delete "$SMPROGRAMS\${APP_NAME}\Desinstalar ${APP_NAME}.lnk"
  RMDir "$SMPROGRAMS\${APP_NAME}"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  DeleteRegKey HKCU "Software\${APP_KEY}"
SectionEnd
