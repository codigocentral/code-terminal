# Instaladores por plataforma

Este projeto gera instaladores nativos a partir da versao declarada em
`Cargo.toml`. Para publicar uma nova versao, altere `package.version`, gere os
artefatos de cada plataforma e mantenha os identificadores de produto estaveis.

## Artefatos

| Plataforma | Script | Artefatos |
|---|---|---|
| Windows | `scripts/build-windows-installer.ps1` | `dist\CodeTerminal-<versao>-windows-x64-setup.exe` (NSIS) e `dist\CodeTerminal-<versao>-windows-x64-portable.zip` |
| Linux | `scripts/build-linux-installer.sh` | `dist/code-terminal-<versao>-linux-<arch>.tar.gz` e, se `dpkg-deb` existir, `dist/code-terminal_<versao>_<arch>.deb` |
| macOS | `scripts/build-macos-installer.sh` | `dist/CodeTerminal-<versao>-macos-<arch>.pkg`, `dist/CodeTerminal-<versao>-macos-<arch>.dmg` e `dist/CodeTerminal-<versao>-macos-<arch>-app.tar.gz` |

## Windows

Requisitos:

- Windows 10 ou superior.
- Rust 1.85+.
- PowerShell.
- NSIS (`makensis`): o instalado, o do `PATH`, o cache que o build do Tauri
  deixa em `%LOCALAPPDATA%\tauri\NSIS` ou o caminho em `MAKENSIS`.

Comando:

```powershell
.\scripts\build-installers.ps1 -Target windows
```

Ou diretamente (`-SkipBuild` reaproveita o `target\release`):

```powershell
.\scripts\build-windows-installer.ps1
```

O instalador (`packaging\windows\code-terminal.nsi`):

- instala por usuario, sem pedir administrador, em
  `%LOCALAPPDATA%\Programs\Code Terminal`;
- copia `code-terminal.exe`, `code-terminal-cli.exe` e
  `code-terminal-browser.exe`, com o icone `packaging\windows\code-terminal.ico`;
- cria o atalho no menu Iniciar e o desinstalador (aparece em
  Configuracoes > Apps);
- opcional (vem marcado): poe a pasta no `PATH` do usuario, para usar o
  `code-terminal-cli` em qualquer terminal; o desinstalador tira.
- Atualizar: rodar o `setup.exe` novo por cima. Ele fecha o Code Terminal
  aberto e troca os arquivos.

Na CI, o job manual `package:windows` gera os dois arquivos na VM de build
(`scripts/ci/test-windows-vm.sh package`) e os deixa no job por 7 dias. Ate
21/11/2026 o instalador era um `.msi` pelo WiX (dotnet); foi trocado por NSIS
para nao exigir o .NET na VM.

### Microsoft Store

Depois de reativar a conta, reserve o nome **Code Terminal** e copie os três
valores de identidade em **Product management → Product identity** no Partner
Center. Em um Windows x64 com o Windows SDK, gere o MSIX com:

```powershell
.\scripts\build-windows-installer.ps1 -Store -PackageName '<Name>' `
  -PackagePublisher '<Publisher>' -PublisherDisplayName '<PublisherDisplayName>'
```

O pacote sai em `dist\CodeTerminal-<versão>-windows-x64.msix`. O manifesto pede
`runFullTrust` para rodar o terminal Win32 e seus processos filhos. Teste
instalação, shell e WebView2 no Windows antes da submissão. A Microsoft assina
o MSIX durante a publicação. A referência para os campos está em
[Identidade do produto](https://learn.microsoft.com/windows/apps/publish/view-app-identity-details).

## Linux

Requisitos Debian/Ubuntu:

```bash
sudo apt update
sudo apt install -y build-essential pkg-config dpkg-dev libwebkit2gtk-4.1-dev
rustup update stable
```

Comando:

```bash
chmod +x scripts/build-linux-installer.sh
./scripts/build-linux-installer.sh
```

Instalacao do `.deb`:

```bash
sudo apt install ./dist/code-terminal_<versao>_<arch>.deb
```

Atualizacao correta:

- O nome do pacote Debian e sempre `code-terminal`.
- A versao vem de `Cargo.toml`.
- Instalar `code-terminal_0.1.2_amd64.deb` sobre `0.1.1` atualiza o mesmo pacote.
- Nao mude o nome do pacote se quiser manter upgrades pelo gerenciador de pacotes.

Se a distribuicao nao usar `.deb`, use o `.tar.gz` gerado e instale manualmente
os binarios em `/usr/local/bin` ou empacote com a ferramenta nativa da distro.

## macOS

Requisitos:

```bash
xcode-select --install
rustup update stable
```

Comando:

```bash
chmod +x scripts/build-macos-installer.sh
./scripts/build-macos-installer.sh
```

O script gera:

- `.pkg`: instalador nativo que coloca o app em `/Applications` e o CLI em
  `/usr/local/bin`.
- `.dmg`: imagem com o `.app` para instalacao visual.
- `.tar.gz`: bundle `.app` compactado.

Atualizacao correta:

- O bundle identifier e o package identifier sao `com.codeterm.codeterminal`.
- A versao vem de `Cargo.toml`.
- Um `.pkg` novo com o mesmo identifier e versao maior atualiza a instalacao anterior.
- Para distribuicao publica, assine e notarize o app/pkg com uma conta Apple Developer.

### Distribuição direta e Gatekeeper

O DMG Developer ID é o instalador para abrir no Finder e arrastar o app
para Aplicativos. O script assina o app e os auxiliares com Hardened Runtime,
envia a imagem ao serviço de notarização da Apple e valida o tíquete anexado
com o Gatekeeper. Os plugins WASM usam o interpretador Pulley nessa edição.

```bash
MACOS_APP_SIGN_IDENTITY='Developer ID Application: NOME DA EMPRESA (TEAMID)' \
MACOS_NOTARY_PROFILE='nome-do-perfil-no-Chaves' \
./scripts/build-macos-installer.sh --developer-id
```

O arquivo final é `dist/CodeTerminal-<versão>-macos-<arch>-notarized.dmg`.
A assinatura ad hoc serve para builds de desenvolvimento. O Gatekeeper
bloqueou uma instalação anterior do Code Terminal assinada desse modo.
[Developer ID e Gatekeeper](https://developer.apple.com/developer-id/)

### Mac App Store

Use o App ID explícito `com.codeterm.codeterminal`, um certificado **Apple
Distribution** para o app, um **Mac Installer Distribution** para o pacote e
um perfil **Mac App Store Connect** correspondente ao App ID e ao certificado
do app. Confirme que cada certificado aparece com sua chave privada:

```bash
security find-identity -v -p codesigning
security find-identity -v
```

Se o certificado do instalador estiver faltando, gere um pedido de assinatura
(CSR) no Acesso às Chaves. Em Apple Developer, abra **Certificates →
Create a New Certificate → Mac Installer Distribution**, envie o CSR, baixe
o `.cer` e importe-o no Mac que gerou a chave.
[Gerar um CSR](https://developer.apple.com/help/account/certificates/create-a-certificate-signing-request) ·
[Criar o perfil](https://developer.apple.com/help/account/provisioning-profiles/create-an-app-store-provisioning-profile)

```bash
MACOS_APP_SIGN_IDENTITY='Apple Distribution: NOME DA EMPRESA (TEAMID)' \
MACOS_INSTALLER_SIGN_IDENTITY='3rd Party Mac Developer Installer: NOME DA EMPRESA (TEAMID)' \
MACOS_APP_STORE_PROFILE='/caminho/CodeTerminal.provisionprofile' \
MACOS_BUILD_NUMBER=1 \
./scripts/build-macos-installer.sh --app-store
```

Use o nome exato mostrado pelo Chaves e incremente `MACOS_BUILD_NUMBER` a
cada envio da mesma versão. O script aplica App Sandbox e gera um pacote
`dist/CodeTerminal-<versão>-mac-app-store-<arch>-<build>.pkg` com o app e seus
executáveis auxiliares no bundle.

**Bloqueio da edição para a loja:** em macOS 27.0.1 com App Sandbox, o par
PTY abriu, mas `TIOCSCTTY` e `tcsetattr` no lado slave retornaram `EPERM`.
Um teste isolado executou comandos básicos quando dispensou o terminal
controlador. Controle de jobs e programas que precisam de modo raw
continuam limitados. O suporte técnico da Apple confirmou a restrição de
`tcsetattr` e indicou o bug `FB25065427`. A publicação aguarda uma solução
que preserve o terminal completo. O `.pkg` da loja serve para validar a
assinatura e o empacotamento.
[Resposta do suporte técnico da Apple](https://developer.apple.com/forums/thread/709444)

### Assinatura no GitLab

Na branch padrão protegida, `package:macos:developer-id` e
`package:macos:app-store:validation` são jobs manuais. O runner protegido
`mac-diogo` usa a tag `macos` e roda como o usuário das identidades no Chaves.
As chaves privadas ficam nesse Mac. O job Developer ID usa o perfil local
`scrutin-notary` da mesma conta de notarização. O job da loja usa o perfil em
`~/Library/Application Support/Code Terminal/Signing/Code-Terminal-Mac-App-Store.provisionprofile`.
Defina `MACOS_BUILD_NUMBER` no job da loja ao gerar um novo build da mesma
versão para o App Store Connect.

O Linux continua nos runners do projeto. O Windows usa a VM compartilhada
`win-build` no pc1 e os jobs com a tag `windows-desktop`.

## Orquestrador

No Windows:

```powershell
.\scripts\build-installers.ps1 -Target all
```

Esse comando gera o instalador do Windows e imprime as instrucoes para Linux/macOS. Ele nao tenta
gerar `.deb`, `.pkg` ou `.dmg` no Windows porque esses formatos dependem de
ferramentas nativas da plataforma.
