# Code Terminal CLI

Ferramenta de linha de comando para controle remoto do Code Terminal.

## Instalação

```bash
# Compilar
cargo build --release --bin code-terminal-cli

# Opcional: instalar no PATH
cp target/release/code-terminal-cli ~/.local/bin/
```

## Uso Rápido

```bash
# Listar tabs
code-terminal-cli list-tabs

# Nova tab
code-terminal-cli new-tab --title "My Project"

# Enviar comando
code-terminal-cli send-keys "ls -la<Enter>"

# Notificação
code-terminal-cli notify "Build complete!"
```

## Comandos

### Gerenciamento de Tabs

| Comando | Descrição | Exemplo |
|---------|-----------|---------|
| `list-tabs` | Lista todas as tabs | `cli ls` |
| `new-tab` | Cria nova tab | `cli new -t "Title"` |
| `close-tab` | Fecha uma tab | `cli close 0` |
| `goto-tab` | Muda para tab | `cli goto 1` |

### Envio de Entrada

| Comando | Descrição | Exemplo |
|---------|-----------|---------|
| `send-keys` | Envia teclas especiais | `cli send "<C-c>"` |
| `input` | Envia texto literal | `cli input "hello"` |

### Informações

| Comando | Descrição | Exemplo |
|---------|-----------|---------|
| `info` | Informações do terminal | `cli info` |
| `size` | Tamanho do terminal | `cli size` |
| `ping` | Testa conexão | `cli ping` |

### Outros

| Comando | Descrição | Exemplo |
|---------|-----------|---------|
| `notify` | Envia notificação | `cli notify "Done"` |
| `list-panes` | Lista panes | `cli list-panes` |
| `split` | Divide pane | `cli split vertical` |

## Opções Globais

```
-s, --socket <SOCKET>   Caminho do socket ou endereço TCP
-c, --config <FILE>     Arquivo de configuração
-f, --format <FORMAT>   Formato de saída: text, json, yaml
-v, --verbose           Modo verbose (repetir para mais detalhes)
-q, --quiet             Modo silencioso
```

## Configuração

Arquivo: `~/.config/code-terminal/cli.toml`

```toml
socket_path = "/tmp/code-terminal.sock"
format = "text"
timeout_secs = 30
retry_attempts = 3
```

## Teclas Especiais

| Notação | Descrição |
|---------|-----------|
| `<Enter>` | Enter |
| `<Tab>` | Tab |
| `<Esc>` | Escape |
| `<Space>` | Espaço |
| `<Up>` `<Down>` `<Left>` `<Right>` | Setas |
| `<F1>` - `<F12>` | Teclas de função |
| `<C-a>` - `<C-z>` | Ctrl + letra |
| `<Home>` `<End>` | Início/Fim |
| `<PageUp>` `<PageDown>` | PgUp/PgDn |
| `<Backspace>` `<Delete>` | Backspace/Delete |

### Exemplos de Teclas

```bash
# Cancelar comando
cli send "<C-c>"

# Sair do vim
cli send "<Esc>:wq<Enter>"

# Histórico
cli send "<Up>"

# Limpar tela
cli send "<C-l>"
```

## Exemplos

### Workflow de Desenvolvimento

```bash
#!/bin/bash
# dev-workflow.sh

# Nova tab para servidor
cli new -t "Server" -d ~/project
cli input "npm run dev"

# Nova tab para editor
cli new -t "Editor" -d ~/project
cli input "vim src/main.rs"

# Tab para logs
cli new -t "Logs" -d ~/project
cli input "tail -f logs/app.log"
```

### Build e Notificação

```bash
#!/bin/bash
# build.sh

cargo build --release 2>&1 | tee build.log

if [ ${PIPESTATUS[0]} -eq 0 ]; then
    cli notify -t "Build" "✅ Success!"
else
    cli notify -t "Build" "❌ Failed!"
    cli send "<C-c>"
fi
```

### Automação de Testes

```bash
#!/bin/bash
# test.sh

cli notify -t "Tests" "Running..."
cargo test --all-features

if [ $? -eq 0 ]; then
    cli notify -t "Tests" "✅ All passed"
else
    cli notify -t "Tests" "❌ Failed"
fi
```

## Formato de Saída

### Texto (padrão)
```
* 0: bash
  1: vim
  2: node
```

### JSON
```bash
cli ls --format json
```
```json
[{"index":0,"title":"bash","is_active":true}]
```

### YAML
```bash
cli ls --format yaml
```
```yaml
---
- index: 0
  title: bash
  is_active: true
```

## Conexão

### Unix Socket (Linux/macOS)
```bash
cli --socket /tmp/code-terminal.sock info
```

### TCP
```bash
cli --socket 127.0.0.1:9999 info
```

### Variável de Ambiente
```bash
export CODE_TERMINAL_SOCKET=/tmp/custom.sock
cli info
```

## Troubleshooting

### Erro: "Server not running"
```bash
# Verificar se o terminal está rodando
ps aux | grep code-terminal

# Verificar socket
ls -la /tmp/code-terminal.sock
```

### Erro: "Connection refused"
```bash
# Verificar porta TCP
cli --socket 127.0.0.1:9999 ping

# Verificar firewall
netstat -tlnp | grep 9999
```

## Veja Também

- [Documentação Principal](../../README.md)
- [Exemplos de Plugins](../../examples/plugins/)
