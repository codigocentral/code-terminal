# Exemplos de Controle Remoto

Esta pasta contém exemplos de clientes para o sistema de controle remoto do Code Terminal.

## 📁 Exemplos

### Python (`python_client.py`)

Cliente completo em Python para interagir com o Code Terminal via JSON-RPC.

**Instalação:**
```bash
# Não requer dependências além do Python padrão
```

**Uso:**
```bash
# Listar abas
python python_client.py --socket /tmp/code-terminal.sock list-tabs

# Criar nova aba
python python_client.py --socket /tmp/code-terminal.sock new-tab --title "Build"

# Enviar comando
python python_client.py --socket /tmp/code-terminal.sock input "ls -la"

# Via TCP
python python_client.py --host 127.0.0.1 --port 9876 ping
```

**Comandos suportados:**
- `ping` - Testa conexão
- `list-tabs` - Lista abas
- `new-tab` - Cria aba
- `close-tab` - Fecha aba
- `goto-tab` - Muda para aba
- `input` - Envia texto
- `size` - Tamanho do terminal
- `methods` - Lista métodos disponíveis

### Uso como Biblioteca

```python
from python_client import CodeTerminalClient

client = CodeTerminalClient(socket_path="/tmp/code-terminal.sock")
client.connect()

# Listar abas
tabs = client.list_tabs()
for tab in tabs:
    print(f"{tab['index']}: {tab['title']}")

# Criar nova aba
index = client.new_tab("Build")

# Enviar comando
client.send_input("cargo build --release\n")

client.disconnect()
```

---

### Outras Linguagens

#### Bash (com netcat)

```bash
# Ping
printf '{"jsonrpc":"2.0","id":1,"method":"ping"}\n' | nc -U /tmp/code-terminal.sock

# Listar tabs
printf '{"jsonrpc":"2.0","id":1,"method":"tab.list"}\n' | nc -U /tmp/code-terminal.sock

# Nova tab
printf '{"jsonrpc":"2.0","id":1,"method":"tab.new","params":{"title":"Test"}}\n' | nc -U /tmp/code-terminal.sock
```

#### JavaScript (Node.js)

```javascript
const net = require('net');

const client = net.createConnection('/tmp/code-terminal.sock', () => {
    const request = {
        jsonrpc: "2.0",
        id: 1,
        method: "tab.list",
        params: {}
    };
    client.write(JSON.stringify(request) + '\n');
});

client.on('data', (data) => {
    console.log('Response:', JSON.parse(data));
    client.end();
});
```

#### Rust

Veja o código do CLI em `src/cli/` para um exemplo completo em Rust.

---

## 🔧 Configuração

Antes de usar os exemplos, certifique-se de que:

1. O servidor remoto está habilitado no Code Terminal:
   ```toml
   [remote]
   enabled = true
   socket_path = "/tmp/code-terminal.sock"
   ```

2. O Code Terminal está rodando

3. Você tem permissões para acessar o socket

---

## 📝 API JSON-RPC

Todos os exemplos usam a API JSON-RPC do Code Terminal:

### Formato da Requisição
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "nome.do.metodo",
  "params": {}
}
```

### Formato da Resposta
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {}
}
```

### Métodos Disponíveis

Veja a documentação completa em `src/remote/README.md`.

---

## 🔒 Autenticação

Se o servidor requer autenticação, adicione o token às requisições:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tab.list",
  "params": {},
  "token": "seu-token-aqui"
}
```

Ou configure no cliente:

```python
client = CodeTerminalClient(socket_path="/tmp/code-terminal.sock")
client.token = "seu-token-aqui"
client.connect()
```

---

## 🐛 Debug

Para debugar a comunicação:

```bash
# Verifique se o socket existe
ls -la /tmp/code-terminal.sock

# Teste com netcat
nc -U /tmp/code-terminal.sock

# Veja os logs do servidor
# (configure RUST_LOG=debug no Code Terminal)
```
