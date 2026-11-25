# Guia de build, instalacao e validacao

Este guia descreve o caminho minimo para gerar uma build release do Codeterm,
instalar os binarios localmente e executar um smoke test por plataforma.

## Requisitos

### Todos os sistemas

- Rust 1.85+ via `rustup`.
- Git.
- Driver grafico atualizado com suporte a API nativa usada pelo `wgpu`.
- Terminal/shell nativo disponivel para o PTY.

### Windows

- Windows 10 ou superior.
- Microsoft Edge WebView2 Runtime instalado.
- GPU/driver com DirectX 12 ou DirectX 11 funcional.
- PowerShell 5+ ou PowerShell 7+.

### Linux

- Driver Mesa/NVIDIA/AMD com Vulkan ou OpenGL funcional.
- `pkg-config`, toolchain C/C++ e bibliotecas de desenvolvimento do WebKitGTK.
- Em distribuicoes Debian/Ubuntu, os nomes comuns sao `build-essential`,
  `pkg-config`, `libwebkit2gtk-4.1-dev` ou equivalente da distro.
- Ambiente X11 ou Wayland com clipboard funcional.

### macOS

- macOS com suporte a Metal.
- Xcode Command Line Tools (`xcode-select --install`).

## Build local

```bash
rustup update stable
cargo check --lib -j 1
cargo clippy --all-features --all-targets -- -D warnings
cargo test --all-features --all-targets -j 1
cargo build --release
```

Nesta maquina Windows, a auditoria registrou corrupcao reprodutivel do cache de
build em execucoes paralelas completas. Enquanto o ambiente nao for corrigido,
use `-j 1` nos comandos de validacao.

## Artefatos esperados

Depois de `cargo build --release`, os binarios ficam em:

- Windows: `target\release\code-terminal.exe` e
  `target\release\code-terminal-cli.exe`.
- Linux/macOS: `target/release/code-terminal` e
  `target/release/code-terminal-cli`.

Para instaladores nativos, veja `docs/INSTALLERS.md`.

## Instalacao local

### Windows PowerShell

```powershell
$InstallDir = "$env:LOCALAPPDATA\Codeterm\bin"
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
Copy-Item .\target\release\code-terminal.exe $InstallDir
Copy-Item .\target\release\code-terminal-cli.exe $InstallDir

$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
}
```

Abra um novo terminal depois de alterar o `PATH`.

### Windows (instalador NSIS)

Para gerar o instalador por usuario (sem admin) e o `.zip` portatil:

```powershell
.\scripts\build-windows-installer.ps1
```

Os artefatos ficam em `dist\CodeTerminal-<versao>-windows-x64-setup.exe` e
`dist\CodeTerminal-<versao>-windows-x64-portable.zip`. Detalhes em
`docs/INSTALLERS.md`.

### Linux/macOS

```bash
install -d "$HOME/.local/bin"
install -m 755 target/release/code-terminal "$HOME/.local/bin/code-terminal"
install -m 755 target/release/code-terminal-cli "$HOME/.local/bin/code-terminal-cli"
```

Garanta que `~/.local/bin` esteja no `PATH` da sessao.

Para gerar pacotes nativos:

```bash
./scripts/build-linux-installer.sh
./scripts/build-macos-installer.sh
```

Execute cada script na plataforma correspondente. O Linux gera `.tar.gz` e
`.deb` quando `dpkg-deb` esta instalado; o macOS gera `.pkg`, `.dmg` e `.app`
compactado quando as ferramentas da Apple estao disponiveis.

## Configuracao

O parser principal usa o diretorio `code-terminal`:

- Windows: `%APPDATA%\code-terminal\config.toml`
- Linux/macOS: `~/.config/code-terminal/config.toml`

Alguns modulos e documentos historicos ainda mencionam `codeterm` para plugins,
socket e persistencia. Antes de empacotar um release publico, consolide esses
caminhos no item F.3 da auditoria.

## Smoke test manual

1. Execute `code-terminal`.
2. Abra uma nova aba e confirme que um shell aparece.
3. Crie split horizontal e vertical.
4. Digite comandos simples no PTY, por exemplo `pwd`/`cd` ou `dir`/`cd`.
5. Feche uma aba com processo ativo e verifique a confirmacao.
6. Execute `code-terminal-cli --help`.
7. Com a Socket API habilitada, execute:

```bash
code-terminal-cli info
code-terminal-cli list-tabs
```

Se usar socket TCP explicitamente:

```bash
code-terminal-cli --socket 127.0.0.1:9123 info
```

## Checklist de release

- `cargo check --lib -j 1` passa.
- `cargo clippy --all-features --all-targets -- -D warnings` passa.
- `cargo test --all-features --all-targets -j 1` passa.
- Build release validada em Windows, Linux e macOS.
- Smoke test manual executado em build release.
- GPU, WebView, PTY, clipboard, tabs, splits e CLI verificados.
- Benchmarks registrados contra os alvos declarados:
  startup abaixo de 50 ms, latencia abaixo de 5 ms, memoria abaixo de 30 MB.
- Notas de release e changelog atualizados.
- Empacotamento/assinatura definidos para cada plataforma.

## Troubleshooting

### Windows

- Se aparecerem erros como `E0786 invalid metadata` ou `can't find crate` em
  builds paralelos, limpe o `target/`, rode com `-j 1` e adicione exclusoes do
  antivirus para o repositorio e `C:\Users\<usuario>\.cargo`.
- Evite compilar em pastas sincronizadas por OneDrive/Drive.
- Confirme que o WebView2 Runtime esta instalado.

### Linux

- Falhas de `pkg-config` ou WebKit normalmente indicam dependencias de
  desenvolvimento ausentes.
- Falhas de GPU podem exigir atualizacao de Mesa/NVIDIA/AMD ou instalacao de
  pacotes Vulkan/OpenGL da distribuicao.

### macOS

- Falha de linker ou compilador geralmente exige `xcode-select --install`.
- Confirme que a maquina tem suporte a Metal.
