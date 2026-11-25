# Controle Remoto - Code Terminal

Sistema de controle remoto via JSON-RPC over socket para o Code Terminal.

## 📋 Índice

- [Visão Geral](#visão-geral)
- [Arquitetura](#arquitetura)
- [Módulos](#módulos)
- [API JSON-RPC](#api-json-rpc)
- [CLI](#cli)
- [Configuração](#configuração)
- [Segurança](#segurança)
- [Exemplos](#exemplos)

---

## Visão Geral

O sistema de controle remoto permite controlar o Code Terminal de forma programática através de:

- **Socket Unix** (Linux/macOS): `/tmp/code-terminal.sock`
- **TCP** (cross-platform): `127.0.0.1:9876`

### Features

- ✅ JSON-RPC 2.0 compliant
- ✅ 15+ comandos disponíveis
- ✅ Autenticação por token Bearer
- ✅ IP whitelist/blacklist
- ✅ Rate limiting
- ✅ Logging de auditoria
- ✅ CLI completo

---

## Arquitetura

```
┌─────────────┐     JSON-RPC      ┌─────────────┐
│  CLI Client │◄────over socket──►│    Server   │
└─────────────┘                   └──────┬──────┘
                                         │
                              ┌──────────┼──────────┐
                              │          │          │
                       ┌──────▼───┐ ┌────▼────┐ ┌───▼────┐
                       │  Handler │ │   Auth  │ │Security│
                       └──────────┘ └─────────┘ └────────┘
```

### Fluxo de Comunicação

1. Cliente conecta ao socket (TCP ou Unix)
2. Envia requisição JSON-RPC (newline-delimited)
3. Servidor processa e retorna resposta
4. Conexão pode ser mantida para múltiplas chamadas

---

## Módulos

| Módulo | Descrição | Arquivo |
|--------|-----------|---------|
| `server` | Servidor de socket | `server.rs` |
| `handler` | Processamento JSON-RPC | `handler.rs` |
| `protocol` | Tipos JSON-RPC | `protocol.rs` |
| `commands` | Implementação dos comandos | `commands.rs` |
| `auth` | Autenticação | `auth.rs` |
| `security` | Auditoria de segurança | `security.rs` |
| `config` | Configuração de rede | `config.rs` |
| `connection` | Gerenciamento de conexões | `connection.rs` |

---

## API JSON-RPC

### Formatos

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tab.list",
  "params": {}
}
```

**Response (sucesso):**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": [...]
}
```

**Response (erro):**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32601,
    "message": "Method not found"
  }
}
```

### Comandos Disponíveis

#### Info

| Método | Descrição | Exemplo |
|--------|-----------|---------|
| `ping` | Testa conexão | `{"method":"ping"}` |
| `server.info` | Informações do servidor | `{"method":"server.info"}` |
| `rpc.listMethods` | Lista métodos | `{"method":"rpc.listMethods"}` |

#### Tabs

| Método | Parâmetros | Descrição |
|--------|------------|-----------|
| `tab.list` | - | Lista todas as abas |
| `tab.new` | `title` | Cria nova aba |
| `tab.close` | `index` | Fecha aba |
| `tab.goto` | `index` | Muda para aba |

**Exemplo:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tab.new",
  "params": {"title": "Build"}
}
```

#### Terminal

| Método | Parâmetros | Descrição |
|--------|------------|-----------|
| `terminal.size` | - | Retorna tamanho (cols, rows) |
| `terminal.input` | `text` | Envia texto |
| `terminal.sendKeys` | `keys` | Envia teclas especiais |
| `terminal.info` | - | Informações do terminal |
| `terminal.notify` | `title`, `message` | Envia notificação |

**Exemplo:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "terminal.input",
  "params": {"text": "ls -la\n"}
}
```

#### Config

| Método | Parâmetros | Descrição |
|--------|------------|-----------|
| `config.get` | `key` | Lê valor |
| `config.set` | `key`, `value` | Escreve valor |
| `config.reset` | - | Reseta tudo |

**Exemplo:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "config.set",
  "params": {"key": "font.size", "value": "14"}
}
```

### Códigos de Erro

| Código | Nome | Descrição |
|--------|------|-----------|
| -32700 | Parse Error | JSON inválido |
| -32600 | Invalid Request | Requisição malformada |
| -32601 | Method Not Found | Método não existe |
| -32602 | Invalid Params | Parâmetros inválidos |
| -32603 | Internal Error | Erro interno |
| -32000 | Server Error | Erro do servidor |

---

## CLI

### Instalação

```bash
cargo build --release --bin code-terminal-cli
```

### Uso Básico

```bash
# Listar abas
code-terminal-cli list-tabs

# Criar nova aba
code-terminal-cli new-tab --title "Build"

# Enviar comando
code-terminal-cli input "ls -la"

# Ping no servidor
code-terminal-cli ping
```

### Conexão

```bash
# Via TCP
code-terminal-cli --socket 127.0.0.1:9876 list-tabs

# Via Unix socket (Linux/macOS)
code-terminal-cli --socket /tmp/code-terminal.sock list-tabs
```

### Formatos de Saída

```bash
# Texto (padrão)
code-terminal-cli list-tabs

# JSON
code-terminal-cli --format json list-tabs

# YAML
code-terminal-cli --format yaml list-tabs
```

### Comandos

```
list-tabs      Lista todas as abas
new-tab        Cria nova aba
close-tab      Fecha aba
goto-tab       Muda para aba
send-keys      Envia teclas especiais
input          Envia texto
size           Lê/define tamanho
info           Informações do terminal
notify         Envia notificação
list-panes     Lista panes
split          Divide pane
config         Gerencia configuração
ping           Testa conexão
version        Mostra versão
```

---

## Configuração

### Configuração do Servidor

Arquivo: `~/.config/code-terminal/config.toml`

```toml
[remote]
enabled = true
socket_path = "/tmp/code-terminal.sock"
tcp_addr = "127.0.0.1:9876"
max_connections = 10
auth_token = "seu-token-secreto"
allow_localhost = true
allowed_ips = ["192.168.1.0/24"]
blocked_ips = ["10.0.0.50"]
idle_timeout_secs = 300
max_auth_attempts = 5
auth_attempt_window = 300
```

### Configuração do CLI

Arquivo: `~/.config/code-terminal/cli.toml`

```toml
socket_path = "/tmp/code-terminal.sock"
format = "text"
timeout_secs = 30
retry_attempts = 3
```

---

## Segurança

### Autenticação

- **Token Bearer**: Envie no header ou parâmetro `token`
- **Localhost bypass**: Conexões locais podem ignorar auth
- **Rate limiting**: Máximo 5 tentativas em 5 minutos

### Bloqueio de IPs

IPs são bloqueados automaticamente após falhas consecutivas de autenticação.

### Comandos Perigosos

Os seguintes comandos são detectados e bloqueados:
- `rm -rf /`
- Fork bombs (`:(){ :|:& };:`)
- `dd if=/dev/zero of=/dev/sda`
- `mkfs`, `format c:`

### Auditoria

Todos os eventos de segurança são logados:
- Tentativas de autenticação
- Falhas e sucessos
- Comandos perigosos
- IPs bloqueados

---

## Exemplos

### Python Client

```python
import json
import socket

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect("/tmp/code-terminal.sock")

# Listar tabs
request = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tab.list",
    "params": {}
}
sock.send(json.dumps(request).encode() + b"\n")
response = sock.recv(4096)
print(json.loads(response))

sock.close()
```

### JavaScript/Node.js

```javascript
const net = require('net');

const client = net.createConnection('/tmp/code-terminal.sock', () => {
    const request = {
        jsonrpc: "2.0",
        id: 1,
        method: "terminal.input",
        params: { text: "ls -la\n" }
    };
    client.write(JSON.stringify(request) + '\n');
});

client.on('data', (data) => {
    console.log(JSON.parse(data));
    client.end();
});
```

### curl (via TCP)

```bash
# Nota: requer servidor em modo TCP
printf '{"jsonrpc":"2.0","id":1,"method":"ping"}\n' | nc 127.0.0.1 9876
```

---

## Desenvolvimento

### Adicionar Novo Comando

1. Implemente no `commands.rs`:
```rust
handler.register("meu.comando", |params| async move {
    // Implementação
    Ok(serde_json::json!(result))
}).await;
```

2. Adicione testes no mesmo arquivo

3. Atualize a documentação

### Testes

```bash
# Rodar todos os testes de remote
cargo test --lib remote::

# Testes específicos
cargo test --lib remote::security
cargo test --lib remote::commands
```

---

## Licença

GPL-3.0-or-later (veja o [LICENSE](../../LICENSE) na raiz)
