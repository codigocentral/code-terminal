# Checklist de Teste Manual - Code Terminal

Este roteiro assume que os binários já estão disponíveis no PATH:

```bash
code-terminal
code-terminal-cli
```

No Windows, se estiver executando direto da pasta de build, os equivalentes podem ser:

```bash
target\release\code-terminal.exe
target\release\code-terminal-cli.exe
```

## Execução release - 12/06/2026

Ambiente: Windows, PowerShell, build local release.

Comandos executados:

```powershell
cargo build --release --bins -j 1
.\target\release\code-terminal-cli.exe --help
.\target\release\code-terminal-cli.exe version
$p = Start-Process -FilePath .\target\release\code-terminal.exe -PassThru -WindowStyle Normal
Start-Sleep -Seconds 5
Stop-Process -Id $p.Id -Force
```

Resultado:

- [x] Build release dos binários `code-terminal.exe` e `code-terminal-cli.exe` concluído sem erros.
- [x] `code-terminal-cli.exe --help` respondeu e listou os comandos disponíveis.
- [x] `code-terminal-cli.exe version` respondeu `code-terminal 0.1.1`.
- [x] `code-terminal.exe` iniciou e permaneceu em execução por 5 segundos antes do encerramento controlado.
- [x] Binários release gerados em `target\release`.

## Execução instalador MSI - 12/06/2026

Ambiente: Windows, PowerShell, instalador per-user.

Comandos executados:

```powershell
.\scripts\build-windows-installer.ps1
msiexec /i .\dist\CodeTerminal-0.1.1-x64.msi /qn /norestart
& "$env:LOCALAPPDATA\Code Terminal\Code Terminal\code-terminal-cli.exe" version
```

Resultado:

- [x] Versao do pacote atualizada para `0.1.1`.
- [x] MSI gerado em `dist\CodeTerminal-0.1.1-x64.msi`.
- [x] Instalacao silenciosa retornou `ExitCode 0`.
- [x] Arquivos instalados em `%LOCALAPPDATA%\Code Terminal\Code Terminal`.
- [x] CLI instalado respondeu `code-terminal 0.1.1`.
- [x] GUI instalado iniciou e permaneceu em execução por 5 segundos antes do encerramento controlado.

## Execução release sem console extra - 12/06/2026

Ambiente: Windows, PowerShell, instalador per-user.

Comandos/validacoes executados:

```powershell
cargo build --release --bin code-terminal -j 1
.\scripts\build-windows-installer.ps1
msiexec /x {77DABDCD-8F74-445C-AF9C-02A6B808FAFF} /qn /norestart
msiexec /i .\dist\CodeTerminal-0.1.1-x64.msi /qn /norestart
```

Resultado:

- [x] `target\release\code-terminal.exe` validado como PE subsystem `Windows GUI`.
- [x] `%LOCALAPPDATA%\Code Terminal\Code Terminal\code-terminal.exe` validado como PE subsystem `Windows GUI`.
- [x] GUI instalado iniciou e permaneceu vivo por 5 segundos.
- [x] Logs escritos em `%LOCALAPPDATA%\Code Terminal\logs\code-terminal.log.<data>`.
- [x] `code-terminal-cli.exe version` respondeu `code-terminal 0.1.1`.

## 1. Inicialização

Abrir o terminal principal:

```bash
code-terminal
```

Validar:

- [ ] A janela abre
- [ ] O terminal renderiza texto
- [ ] O cursor aparece
- [ ] A aplicação não fecha imediatamente
- [ ] Redimensionar a janela não quebra a UI

## 2. Terminal Básico

Executar dentro da janela:

```bash
echo hello
pwd
dir
git --version
cargo --version
```

Gerar muito output:

```bash
for /L %i in (1,1,100) do @echo linha-%i
```

Validar:

- [ ] Enter funciona
- [ ] Backspace funciona
- [ ] O texto aparece em tempo real
- [ ] Scroll funciona
- [ ] O terminal continua responsivo com muito output

## 3. Tabs

Atalhos:

- `Ctrl+Shift+T`: nova tab
- `Ctrl+Shift+W`: fechar tab
- `Ctrl+1` até `Ctrl+9`: ir para tab específica

Validar:

- [ ] Criar nova tab
- [ ] Criar 3 tabs
- [ ] Alternar entre tabs
- [ ] Fechar uma tab intermediária
- [ ] Cada tab mantém estado independente

## 4. Splits / Panes

Atalhos:

- `Ctrl+Shift+E`: split horizontal
- `Ctrl+Shift+O`: split vertical
- `Ctrl+Shift+X`: fechar pane
- `Alt+Up`: navegar para cima
- `Alt+Down`: navegar para baixo
- `Alt+Left`: navegar para esquerda
- `Alt+Right`: navegar para direita
- `Ctrl+Tab`: próximo pane
- `Ctrl+Shift+Tab`: pane anterior
- `Ctrl+Shift++`: aumentar tamanho
- `Ctrl+Shift+-`: diminuir tamanho

Validar:

- [ ] Criar split horizontal
- [ ] Criar split vertical
- [ ] Navegar entre panes por seta
- [ ] Navegar entre panes por ciclo
- [ ] Redimensionar panes por teclado
- [ ] Fechar pane sem derrubar a janela

## 5. Busca

Atalhos:

- `Ctrl+Shift+F`: abrir busca
- `F3`: próximo resultado
- `Shift+F3`: resultado anterior
- `Esc`: fechar busca

Validar:

- [ ] Abrir barra de busca
- [ ] Pesquisar texto já exibido no terminal
- [ ] Avançar para próximo match
- [ ] Voltar para match anterior
- [ ] Fechar busca com Esc

## 6. Clipboard

Atalhos:

- `Ctrl+Shift+C`: copiar a seleção (`Ctrl+C` interrompe o programa)
- `Ctrl+Shift+V`: colar

Validar:

- [ ] Selecionar/copiar texto
- [ ] Colar texto no terminal
- [ ] Colar comando e executar normalmente

## 7. Janela

Atalho:

- `Ctrl+Shift+N`: nova janela

Validar:

- [ ] Abrir nova janela
- [ ] Alternar foco entre janelas
- [ ] Fechar uma janela sem derrubar a outra

## 8. CLI Remota

Com o `code-terminal` aberto, executar em outro terminal:

```bash
code-terminal-cli ping
code-terminal-cli info
code-terminal-cli list-tabs
code-terminal-cli new-tab --title "Teste CLI"
code-terminal-cli goto-tab 0
code-terminal-cli input "echo teste-via-cli"
code-terminal-cli send-keys "<Enter>"
code-terminal-cli notify "Teste" "Mensagem da CLI"
code-terminal-cli list-panes
code-terminal-cli split horizontal
code-terminal-cli split vertical
code-terminal-cli size
code-terminal-cli size --cols 120 --rows 40
```

Validar:

- [ ] `ping` responde
- [ ] `info` responde
- [ ] `list-tabs` lista tabs reais
- [ ] `new-tab` cria tab visível na UI
- [ ] `input` envia comando para a UI
- [ ] `notify` gera notificação
- [ ] `split` afeta a UI
- [ ] `size` consulta e altera tamanho

## 9. CLI com Saída Estruturada

```bash
code-terminal-cli -f json info
code-terminal-cli -f json list-tabs
code-terminal-cli -f json list-panes
code-terminal-cli -f yaml info
```

Validar:

- [ ] JSON válido
- [ ] YAML válido
- [ ] Estrutura coerente com a UI atual

## 10. SSH

Se `~/.ssh/config` estiver configurado:

```bash
code-terminal-cli ssh-hosts
code-terminal-cli ssh-connect meu-host
code-terminal-cli ssh-sessions
```

Validar:

- [ ] Lista hosts corretamente
- [ ] Conecta ao host
- [ ] Sessão aparece listada

## 11. Multiplexer

```bash
code-terminal-cli mux-list
code-terminal-cli mux-create minha-sessao
code-terminal-cli mux-attach minha-sessao
code-terminal-cli mux-stats
code-terminal-cli mux-detach
code-terminal-cli mux-save minha-sessao
code-terminal-cli mux-saved
code-terminal-cli mux-restore minha-sessao
```

Validar:

- [ ] Cria sessão
- [ ] Attach funciona
- [ ] Detach funciona
- [ ] Save funciona
- [ ] Restore funciona
- [ ] Stats responde

## 12. Configuração

```bash
code-terminal-cli config show
code-terminal-cli config get window.width
code-terminal-cli config set window.width 1200
code-terminal-cli config reset
```

Validar:

- [ ] `config show` responde
- [ ] `config get` responde
- [ ] `config set` altera valor
- [ ] `config reset` volta ao padrão

## 13. Recursos Parciais / Suspeitos

Esses testes valem a pena, mas podem falhar no estado atual:

```bash
code-terminal-cli screenshot --output teste.png
```

Validar:

- [ ] Screenshot realmente gera arquivo válido
- [ ] Browser embutido funciona sem travar
- [ ] Automação do browser executa ações reais
- [ ] Integrações de AI hooks respondem
- [ ] Multiplexer reflete corretamente na renderização

## 14. Registro de Resultado

Use este formato para cada item testado:

```text
[OK] Tabs - criar/alternar/fechar
[OK] Search - Ctrl+Shift+F, F3, Shift+F3
[FALHOU] screenshot - comando existe mas não gera imagem real
[FALHOU] mux-attach - comportamento inconsistente na UI
[OBS] Vulkan mostra warnings no startup, mas a app continua rodando
```
