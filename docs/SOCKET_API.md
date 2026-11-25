# Socket API Documentation

Documentação completa da API de controle do Code Terminal via socket.

## Overview

A Socket API permite controle externo do terminal via JSON-RPC sobre TCP ou Unix sockets. Isso possibilita:

- Automação de tabs e panes
- Scripts de integração
- Controle remoto via CLI
- Integração com editores e IDEs

## Protocolo

A API usa JSON-RPC 2.0 sobre conexões de socket:

- **Formato**: JSON, uma mensagem por linha (newline-delimited)
- **Encoding**: UTF-8
- **Transporte**: TCP ou Unix socket

### Endereços Padrão

```
Linux/macOS: Unix socket em $XDG_RUNTIME_DIR/code-terminal.sock (ou /tmp), permissão 0600
Windows:     TCP 127.0.0.1:9123
```

No Linux/macOS não há porta TCP aberta por padrão: só o dono do socket
consegue mandar comandos ao terminal. É o mesmo caminho que o
`code-terminal-cli` usa sem configuração.

## Exemplos de Uso

### Bash + netcat (Unix)

```bash
# Listar tabs
echo '{"jsonrpc":"2.0","id":1,"method":"tab.list","params":{}}' | nc -U "$XDG_RUNTIME_DIR/code-terminal.sock"

# Criar nova tab
echo '{"jsonrpc":"2.0","id":2,"method":"tab.create","params":{"title":"Dev","command":"bash"}}' | nc -U "$XDG_RUNTIME_DIR/code-terminal.sock"

# Enviar texto
echo '{"jsonrpc":"2.0","id":3,"method":"keyboard.send","params":{"keys":"ls -la\n"}}' | nc -U "$XDG_RUNTIME_DIR/code-terminal.sock"
```

### Python

```python
import json
import socket

def call(method, params=None, id=1):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect('"$XDG_RUNTIME_DIR/code-terminal.sock"')
    
    request = {
        'jsonrpc': '2.0',
        'id': id,
        'method': method,
        'params': params or {}
    }
    
    sock.send(json.dumps(request).encode() + b'\n')
    response = sock.recv(4096).decode()
    sock.close()
    
    return json.loads(response)

# Usar API
tabs = call('tab.list')
print(tabs)

call('tab.create', {'title': 'Build', 'command': 'npm run build'})
call('keyboard.send', {'keys': 'clear\n'})
```

### JavaScript/Node.js

```javascript
const net = require('net');

function call(method, params = {}, id = 1) {
    return new Promise((resolve, reject) => {
        const client = net.createConnection('"$XDG_RUNTIME_DIR/code-terminal.sock"');
        
        const request = {
            jsonrpc: '2.0',
            id,
            method,
            params
        };
        
        client.write(JSON.stringify(request) + '\n');
        
        let data = '';
        client.on('data', chunk => data += chunk);
        client.on('end', () => resolve(JSON.parse(data)));
        client.on('error', reject);
    });
}

// Usar API
async function main() {
    const tabs = await call('tab.list');
    console.log(tabs);
    
    await call('tab.create', { title: 'Server', command: 'npm start' });
}

main();
```

## Referência da API

### Workspace API

#### `workspace.list`
Lista todos os workspaces.

**Parâmetros**: Nenhum

**Resposta**:
```json
[
    {
        "id": "default",
        "name": "Default",
        "is_current": true,
        "tab_count": 3,
        "working_dir": "/home/user"
    }
]
```

---

#### `workspace.current`
Retorna o workspace atual.

**Parâmetros**: Nenhum

**Resposta**:
```json
{
    "id": "default",
    "name": "Default",
    "is_current": true,
    "tab_count": 3,
    "working_dir": "/home/user"
}
```

---

#### `workspace.get`
Retorna um workspace específico.

**Parâmetros**:
- `id` (string): ID do workspace

**Resposta**: WorkspaceInfo

---

#### `workspace.create`
Cria um novo workspace.

**Parâmetros**:
- `name` (string, opcional): Nome do workspace

**Resposta**: WorkspaceInfo do workspace criado

---

#### `workspace.switch`
Muda para outro workspace.

**Parâmetros**:
- `id` (string): ID do workspace

**Resposta**: WorkspaceInfo

---

#### `workspace.rename`
Renomeia um workspace.

**Parâmetros**:
- `id` (string): ID do workspace
- `name` (string): Novo nome

**Resposta**: WorkspaceInfo atualizado

---

#### `workspace.close`
Fecha um workspace.

**Parâmetros**:
- `id` (string): ID do workspace
- `force` (boolean, opcional): Forçar fechamento mesmo com tabs

**Resposta**: null

---

### Tab API

#### `tab.list`
Lista todas as tabs.

**Parâmetros**: Nenhum

**Resposta**:
```json
[
    {
        "id": 1,
        "index": 0,
        "title": "bash",
        "is_active": true,
        "working_dir": "/home/user",
        "process": "bash",
        "pid": 1234
    }
]
```

---

#### `tab.get`
Retorna informações de uma tab.

**Parâmetros**:
- `id` (number): ID da tab (opcional, retorna ativa se omitido)

**Resposta**: TabInfo

---

#### `tab.create`
Cria uma nova tab.

**Parâmetros**:
- `title` (string, opcional): Título da tab (fica fixo, como em `tab.rename`)
- `working_dir` (string, opcional): Diretório de trabalho
- `command` (array<string>, opcional): Comando para executar

**Resposta**: TabInfo

---

#### `tab.close`
Fecha uma tab.

**Parâmetros**:
- `id` (number, opcional): ID da tab (fecha ativa se omitido)
- `force` (boolean, opcional): Forçar fechamento com processo rodando

**Resposta**: null

---

#### `tab.switch`
Muda para uma tab.

**Parâmetros** (um dos dois):
- `id` (number): ID da tab
- `index` (number): Índice da tab

**Resposta**: TabInfo

---

#### `tab.next`
Vai para a próxima tab.

**Parâmetros**: Nenhum

**Resposta**: TabInfo

---

#### `tab.previous`
Vai para a tab anterior.

**Parâmetros**: Nenhum

**Resposta**: TabInfo

---

#### `tab.move`
Move uma tab para outra posição.

**Parâmetros**:
- `from` (number): Índice atual
- `to` (number): Novo índice

**Resposta**: TabInfo

---

#### `tab.rename`
Renomeia uma tab. O título fica fixo: o programa (OSC 0/2) não o troca.

**Parâmetros**:
- `id` ou `index` (number, opcional): a tab (a ativa se omitido)
- `title` (string ou null): novo título; vazio ou `null` volta ao automático

**Resposta**: TabInfo (com `custom_title`, `parent_id` e `collapsed`)

CLI: `code-terminal-cli rename-tab <índice> [título]`.

---

#### `tab.nest`
Põe uma tab dentro do grupo de outra na sidebar (um nível; soltar sobre uma
tab de dentro entra no mesmo grupo).

**Parâmetros**:
- `id` ou `index` (number, opcional): a tab a aninhar (a ativa se omitido)
- `parent_id` ou `parent_index` (number): a tab de cima

**Resposta**: `{"changed": bool, "id": number, "tabs": [TabInfo...]}`

---

#### `tab.unnest`
Tira a tab do grupo (ela vai para logo abaixo do grupo).

**Parâmetros**:
- `id` ou `index` (number, opcional): a tab (a ativa se omitido)

**Resposta**: `{"changed": bool, "id": number, "tabs": [TabInfo...]}`

---

#### `tab.duplicate`
Duplica uma tab.

**Parâmetros**:
- `id` (number): ID da tab

**Resposta**: TabInfo da nova tab

---

### Pane API

#### `pane.list`
Lista todos os panes.

**Parâmetros**:
- `tab_id` (number, opcional): Filtrar por tab

**Resposta**:
```json
[
    {
        "id": 1,
        "index": 0,
        "tab_id": 1,
        "title": "Pane 1",
        "is_active": true,
        "working_dir": "/home/user",
        "size": {
            "cols": 80,
            "rows": 24,
            "width_percent": 50.0,
            "height_percent": 100.0
        },
        "position": {
            "x": 0,
            "y": 0
        }
    }
]
```

---

#### `pane.get`
Retorna informações de um pane.

**Parâmetros**:
- `id` (number): ID do pane (opcional, retorna ativo se omitido)

**Resposta**: PaneInfo

---

#### `pane.split`
Divide um pane.

**Parâmetros**:
- `params.direction` (string): `"horizontal"` ou `"vertical"`
- `params.size_percent` (number, opcional): Porcentagem do tamanho (0-100)
- `params.command` (array<string>, opcional): Comando para executar
- `tab_id` (number, opcional): ID da tab

**Resposta**: PaneInfo

---

#### `pane.close`
Fecha um pane.

**Parâmetros**:
- `id` (number): ID do pane

**Resposta**: null

---

#### `pane.focus`
Foca um pane.

**Parâmetros** (um dos dois):
- `id` (number): ID do pane
- `direction` (string): `"up"`, `"down"`, `"left"`, `"right"`, `"next"`, `"previous"`
- `tab_id` (number, opcional): ID da tab (se usando direction)

**Resposta**: PaneInfo

---

#### `pane.resize`
Redimensiona um pane.

**Parâmetros**:
- `id` (number): ID do pane
- `params.direction` (string): Direção do resize
- `params.amount` (number, opcional): Quantidade de células
- `params.absolute_size` (number, opcional): Tamanho absoluto em células

**Resposta**: PaneInfo

---

#### `pane.swap`
Troca dois panes de posição.

**Parâmetros**:
- `id1` (number): ID do primeiro pane
- `id2` (number): ID do segundo pane

**Resposta**: Array com os dois PaneInfo

---

#### `pane.layout`
Define o layout dos panes.

**Parâmetros**:
- `layout` (string): `"even-horizontal"`, `"even-vertical"`, `"main-horizontal"`, `"main-vertical"`, `"tiled"`
- `tab_id` (number, opcional): ID da tab

**Resposta**: Array de PaneInfo

---

### Keyboard API

#### `keyboard.send`
Envia keystrokes para o terminal.

**Parâmetros**:
- `keys` (string): Texto/sequência a enviar
- `tab_id` (number, opcional): ID da tab
- `pane_id` (number, opcional): ID do pane

**Resposta**: null

**Sequências especiais**:
- `\n` ou `\r`: Enter
- `\t`: Tab
- `\\`: Backslash literal

---

#### `keyboard.key`
Envia uma tecla específica com modificadores.

**Parâmetros**:
- `event.key` (string): Nome da tecla
- `event.modifiers` (array<string>): Modificadores (`"ctrl"`, `"shift"`, `"alt"`, `"meta"`)
- `tab_id` (number, opcional): ID da tab

**Resposta**: null

**Teclas especiais**:
- `"enter"`, `"tab"`, `"backspace"`, `"escape"`
- `"up"`, `"down"`, `"left"`, `"right"`
- `"home"`, `"end"`, `"pageup"`, `"pagedown"`
- `"delete"`, `"insert"`
- `"f1"` a `"f12"`

---

#### `keyboard.text`
Envia texto literal (sem processar sequências).

**Parâmetros**:
- `text` (string): Texto a enviar
- `tab_id` (number, opcional): ID da tab

**Resposta**: null

---

#### `keyboard.chord`
Envia uma sequência de teclas.

**Parâmetros**:
- `keys` (array<string>): Sequência de teclas
- `tab_id` (number, opcional): ID da tab

**Resposta**: null

---

#### `keyboard.shortcut`
Executa um atalho pelo nome.

**Parâmetros**:
- `name` (string): Nome do atalho

**Resposta**: null

**Atalhos disponíveis**:
- `new_tab`: Ctrl+Shift+T
- `close_tab`: Ctrl+Shift+W
- `next_tab`: Ctrl+Tab
- `previous_tab`: Ctrl+Shift+Tab
- `copy`: Ctrl+Shift+C
- `paste`: Ctrl+Shift+V
- `search`: Ctrl+Shift+F
- `zoom_in`: Ctrl+Plus
- `zoom_out`: Ctrl+Minus

---

#### `keyboard.list_shortcuts`
Lista todos os atalhos disponíveis.

**Parâmetros**: Nenhum

**Resposta**: Array de KeyboardShortcut

---

### State API

#### `state.get`
Retorna o estado completo do terminal.

**Parâmetros**: Nenhum

**Resposta**: TerminalState

---

#### `state.query`
Realiza uma query de estado específica.

**Parâmetros**:
- `type` (string): Tipo de query (`"tabs"`, `"panes"`, `"workspace"`, `"selection"`, `"scrollback"`, `"config"`, `"all"`)

**Resposta**: StateQueryResult

---

#### `state.subscribe`
Subscreve a eventos de estado.

**Parâmetros**:
- `type` (string): Tipo de evento a subscrever

**Resposta**: ID da subscrição (string)

---

#### `state.unsubscribe`
Cancela uma subscrição.

**Parâmetros**:
- `id` (string): ID da subscrição

**Resposta**: null

---

#### `state.list_subscriptions`
Lista subscrições ativas.

**Parâmetros**: Nenhum

**Resposta**: Array de (id, type)

---

#### `state.dump`
Gera um dump completo do estado para debug.

**Parâmetros**: Nenhum

**Resposta**: JSON arbitrário

---

#### `state.api_info`
Retorna informações sobre a API.

**Parâmetros**: Nenhum

**Resposta**: ApiCapabilities

---

## Tipos

### TabInfo
```typescript
{
    id: number;
    index: number;
    title: string;
    is_active: boolean;
    working_dir?: string;
    process?: string;
    pid?: number;
}
```

### PaneInfo
```typescript
{
    id: number;
    index: number;
    tab_id: number;
    title: string;
    is_active: boolean;
    working_dir?: string;
    size: {
        cols: number;
        rows: number;
        width_percent: number;
        height_percent: number;
    };
    position: {
        x: number;
        y: number;
    };
}
```

### WorkspaceInfo
```typescript
{
    id: string;
    name: string;
    is_current: boolean;
    tab_count: number;
    working_dir?: string;
}
```

### TerminalState
```typescript
{
    version: string;
    workspace: WorkspaceInfo;
    tabs: TabInfo[];
    panes: PaneInfo[];
    active_tab: number;
    active_pane: number;
    terminal_size: {
        cols: number;
        rows: number;
    };
    config: Record<string, any>;
}
```

## Códigos de Erro

| Código | Descrição |
|--------|-----------|
| -32601 | Method not found |
| -32602 | Invalid params |
| -32603 | Internal error |
| -32001 | Not found |
| -32002 | Permission denied |
| -32003 | Invalid state |
| -32004 | Timeout |
| -32005 | Not implemented |

## Exemplos Avançados

### Script de Build

```bash
#!/bin/bash

SOCKET=""$XDG_RUNTIME_DIR/code-terminal.sock""
API='{"jsonrpc":"2.0","id":1}'

call() {
    echo "$1" | nc -U "$SOCKET" -q 0
}

# Cria tab para build
call "$API" -s method="tab.create" -s params='{"title":"Build","command":"bash"}'
sleep 0.5

# Executa build
call "$API" -s method="keyboard.send" -s params='{"keys":"npm run build\n"}'

# Aguarda
sleep 10

# Fecha tab
call "$API" -s method="tab.close" -s params='{"index":1}'
```

### Integração com Neovim

```lua
-- nvim/lua/codeterm.lua
local M = {}

function M.send_to_terminal(text)
    local sock = vim.loop.new_tcp()
    sock:connect('"$XDG_RUNTIME_DIR/code-terminal.sock"')
    
    local request = vim.fn.json_encode({
        jsonrpc = '2.0',
        id = 1,
        method = 'keyboard.send',
        params = { keys = text .. '\n' }
    })
    
    sock:write(request .. '\n')
    sock:close()
end

-- Comando :TermSend
vim.api.nvim_create_user_command('TermSend', function(opts)
    M.send_to_terminal(opts.args)
end, { nargs = 1 })

return M
```

### WebSocket Bridge (Node.js)

```javascript
const WebSocket = require('ws');
const net = require('net');

// Bridge WebSocket -> Unix Socket
const wss = new WebSocket.Server({ port: 8080 });

wss.on('connection', (ws) => {
    const socket = net.createConnection('"$XDG_RUNTIME_DIR/code-terminal.sock"');
    
    ws.on('message', (data) => {
        socket.write(data + '\n');
    });
    
    socket.on('data', (data) => {
        ws.send(data.toString());
    });
});
```

## Configuração

Para habilitar a Socket API, adicione ao `config.toml`:

```toml
[socket_api]
enabled = true
tcp_addr = "127.0.0.1:9123"  # Ou null para desabilitar TCP
unix_socket = ""$XDG_RUNTIME_DIR/code-terminal.sock""  # Ou null para desabilitar Unix
```

## Segurança

- A API só escuta em interfaces locais (127.0.0.1)
- Unix sockets têm permissões do sistema de arquivos
- Não exponha a API TCP em redes públicas
- Use com cuidado o `keyboard.send` - valida inputs
