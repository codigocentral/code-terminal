# Host Functions Executáveis - Documentação

Este documento descreve as 5 host functions implementadas para permitir que plugins WASM interajam com o terminal de forma segura e controlada.

## Visão Geral

As host functions são funções do host (terminal) que podem ser chamadas pelos plugins WASM. Cada função tem um nível de permissão que controla quem pode executá-la.

### Níveis de Permissão

- **Safe**: Sem restrições, qualquer plugin pode usar
- **Restricted**: Requer capacidade específica no manifesto
- **RequiresPermission**: Requer permissão explícita do usuário
- **Dangerous**: Requer confirmação do usuário (file system, exec)

---

## 1. `terminal_write` - Escrever no Terminal

**Categoria**: `Terminal`  
**Permissão**: `Restricted`  
**Requer**: `terminal:write`

Escreve dados no terminal (envia para o stdin do processo filho).

### Parâmetros
```
[data_ptr: u32] [data_len: u32]  // Ponteiro e tamanho da string
```

### Retorno
```
i32  // Bytes escritos (>= 0) ou -1 em erro
```

### Exemplo (WAT)
```wat
;; Escrever "Hello" no terminal
(call $terminal_write
  (i32.const 1024)    ;; ponteiro para dados
  (i32.const 5)       ;; tamanho (5 bytes: "Hello")
)
```

### Exemplo (Rust no plugin)
```rust
#[link(wasm_import_module = "host")]
extern "C" {
    fn terminal_write(ptr: *const u8, len: usize) -> i32;
}

fn write_to_terminal(text: &str) -> i32 {
    unsafe { terminal_write(text.as_ptr(), text.len()) }
}
```

---

## 2. `terminal_read` - Ler do Terminal

**Categoria**: `Terminal`  
**Permissão**: `Restricted`  
**Requer**: `terminal:read`

Lê input pendente do terminal (o que o usuário digitou).

### Parâmetros
```
[out_ptr: u32] [max_len: i32]  // Buffer de saída e tamanho máximo
```

### Retorno
```
[bytes_read: i32] [data...]  // Bytes lidos seguidos dos dados
```

### Exemplo (WAT)
```wat
;; Ler até 256 bytes do terminal
(call $terminal_read
  (i32.const 2048)    ;; buffer de saída
  (i32.const 256)     ;; máximo de bytes
)
;; Resultado em 2048: [bytes_read: 4 bytes] [dados: bytes_read bytes]
```

### Exemplo (Rust no plugin)
```rust
#[link(wasm_import_module = "host")]
extern "C" {
    fn terminal_read(buf: *mut u8, max_len: usize) -> i32;
}

fn read_from_terminal(buf: &mut [u8]) -> i32 {
    unsafe { terminal_read(buf.as_mut_ptr(), buf.len()) }
}
```

---

## 3. `terminal_clear` - Limpar Terminal

**Categoria**: `Terminal`  
**Permissão**: `Restricted`  
**Requer**: `terminal:clear`

Limpa o conteúdo do terminal.

### Modos de Limpeza

| Valor | Constante | Descrição |
|-------|-----------|-----------|
| 0 | `CLEAR_SCREEN` | Limpa apenas a tela visível |
| 1 | `CLEAR_SCROLLBACK` | Limpa apenas o scrollback |
| 2 | `CLEAR_ALL` | Limpa tudo (tela + scrollback) |

### Parâmetros
```
mode: i32  // 0=screen, 1=scrollback, 2=all
```

### Retorno
```
i32  // 0 em sucesso, -1 em erro
```

### Exemplo (WAT)
```wat
;; Limpar tudo
(call $terminal_clear (i32.const 2))
```

### Exemplo (Rust no plugin)
```rust
#[link(wasm_import_module = "host")]
extern "C" {
    fn terminal_clear(mode: i32) -> i32;
}

const CLEAR_SCREEN: i32 = 0;
const CLEAR_SCROLLBACK: i32 = 1;
const CLEAR_ALL: i32 = 2;

fn clear_terminal(mode: i32) -> i32 {
    unsafe { terminal_clear(mode) }
}
```

---

## 4. `ui_show_notification` - Notificação na UI

**Categoria**: `UI`  
**Permissão**: `Restricted`  
**Requer**: `ui:notification`

Exibe uma notificação/toast na interface do terminal.

### Níveis de Notificação

| Valor | Constante | Uso |
|-------|-----------|-----|
| 0 | `INFO` | Informações gerais |
| 1 | `SUCCESS` | Operação concluída com sucesso |
| 2 | `WARNING` | Aviso que requer atenção |
| 3 | `ERROR` | Erro que precisa ser corrigido |

### Parâmetros
```
[level: i32]              // Nível (0-3)
[title_len: i32]          // Tamanho do título
[title: bytes]            // Título da notificação
[message_len: i32]        // Tamanho da mensagem
[message: bytes]          // Corpo da mensagem
```

### Retorno
```
i32  // 0 em sucesso, -1 em erro
```

### Exemplo (WAT)
```wat
;; Mostrar notificação de sucesso
(call $ui_show_notification
  (i32.const 1)         ;; SUCCESS
  (i32.const 1024) (i32.const 7)  ;; "Sucesso"
  (i32.const 1032) (i32.const 20) ;; "Plugin carregado!"
)
```

### Exemplo (Rust no plugin)
```rust
#[link(wasm_import_module = "host")]
extern "C" {
    fn ui_show_notification(
        level: i32,
        title_ptr: *const u8,
        title_len: usize,
        msg_ptr: *const u8,
        msg_len: usize
    ) -> i32;
}

fn notify_success(title: &str, message: &str) -> i32 {
    const SUCCESS: i32 = 1;
    unsafe {
        ui_show_notification(
            SUCCESS,
            title.as_ptr(), title.len(),
            message.as_ptr(), message.len()
        )
    }
}
```

---

## 5. `system_exec` - Executar Comando (Sandboxed)

**Categoria**: `System`  
**Permissão**: `Dangerous`  
**Requer**: `system:exec` + confirmação do usuário

Executa comandos do sistema em um sandbox seguro.

### Sandbox

O comando é executado com as seguintes restrições:

- **Whitelist de comandos**: Apenas comandos específicos são permitidos (echo, ls, cat, grep, etc.)
- **Timeout**: 30 segundos padrão (configurável)
- **Limite de memória**: 64MB padrão (configurável)
- **Sem acesso à rede**: Por padrão
- **Diretórios restritos**: Apenas `/tmp`, `/var/tmp` por padrão

### Comandos Permitidos (padrão)

- `echo`, `cat`, `ls`, `pwd`
- `grep`, `head`, `tail`, `wc`
- `sort`, `uniq`, `date`, `whoami`, `which`

### Parâmetros
```
[cmd_len: i32]            // Tamanho do comando
[cmd: bytes]              // Comando a executar
[args_len: i32]           // Tamanho do JSON de argumentos
[args: bytes]             // Array JSON: ["arg1", "arg2", ...]
```

### Retorno (JSON)
```json
{
  "stdout": "...",      // Saída padrão (base64 se binário)
  "stderr": "...",      // Saída de erro
  "exit_code": 0,       // Código de saída
  "status": "..."       // "completed", "timeout", "killed"
}
```

### Exemplo (WAT)
```wat
;; Executar "echo Hello World"
(call $system_exec
  (i32.const 1024) (i32.const 4)   ;; "echo"
  (i32.const 1028) (i32.const 21)  ;; "[\"Hello World\"]"
)
;; Resultado em memória: JSON com stdout, stderr, exit_code
```

### Exemplo (Rust no plugin)
```rust
#[link(wasm_import_module = "host")]
extern "C" {
    fn system_exec(
        cmd_ptr: *const u8, cmd_len: usize,
        args_ptr: *const u8, args_len: usize
    ) -> i32;  // Retorna ponteiro para resultado JSON
}

fn exec_command(cmd: &str, args: &[&str]) -> Result<CommandOutput, String> {
    let args_json = serde_json::to_string(args).unwrap();
    
    let result_ptr = unsafe {
        system_exec(
            cmd.as_ptr(), cmd.len(),
            args_json.as_ptr(), args_json.len()
        )
    };
    
    // Ler resultado da memória e fazer parse do JSON
    // ...
}
```

### Tratamento de Erros

| Exit Code | Significado |
|-----------|-------------|
| 0 | Sucesso |
| 126 | Comando não permitido no sandbox |
| 127 | Comando não encontrado |
| -1 | Erro de execução |
| 124 | Timeout |
| 137 | Killed (out of memory) |

---

## Registro no Manifesto

Para usar essas funções, o plugin deve declarar as permissões no `plugin.toml`:

```toml
[permissions]
# Terminal
terminal_write = true
terminal_read = true
terminal_clear = true

# UI
ui_notification = true

# System (requer confirmação)
system_exec = true

[sandbox]
allowed_commands = ["echo", "ls", "cat", "grep"]  # Subset do padrão
timeout_secs = 30
max_memory_mb = 64
allow_network = false
allowed_dirs = ["/tmp"]
```

---

## Exemplo Completo: Plugin de Status

```rust
// plugin.rs - Código do plugin WASM

#[link(wasm_import_module = "host")]
extern "C" {
    fn terminal_write(ptr: *const u8, len: usize) -> i32;
    fn ui_show_notification(
        level: i32,
        title_ptr: *const u8, title_len: usize,
        msg_ptr: *const u8, msg_len: usize
    ) -> i32;
    fn system_exec(
        cmd_ptr: *const u8, cmd_len: usize,
        args_ptr: *const u8, args_len: usize
    ) -> i32;
}

static INFO: i32 = 0;
static SUCCESS: i32 = 1;

#[no_mangle]
pub extern "C" fn on_plugin_load() {
    // Mostrar notificação
    let title = "Status Plugin";
    let msg = "Plugin de status carregado";
    unsafe {
        ui_show_notification(
            SUCCESS,
            title.as_ptr(), title.len(),
            msg.as_ptr(), msg.len()
        );
    }
}

#[no_mangle]
pub extern "C" fn show_git_status() {
    // Executar git status no sandbox
    let cmd = "echo";
    let args = "[\"Git status placeholder\"]";
    
    let result = unsafe {
        system_exec(
            cmd.as_ptr(), cmd.len(),
            args.as_ptr(), args.len()
        )
    };
    
    // Escrever resultado no terminal
    let output = format!("Git status: {}\r\n", result);
    unsafe {
        terminal_write(output.as_ptr(), output.len());
    }
}
```

---

## Integração com Runtime WASM

Para usar essas funções no runtime:

```rust
use code_terminal::plugin::host::{
    ExecutionContext, ExecutableHostFunctions,
    TerminalWrite, TerminalRead, TerminalClear,
    UiShowNotification, SystemExec,
};
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc};

// Setup
let (command_tx, mut command_rx) = mpsc::channel(100);
let ctx = Arc::new(RwLock::new(ExecutionContext::new("my-plugin")));

// Criar todas as funções
let functions = ExecutableHostFunctions::create_all(ctx, input_buffer);

// Usar no linker wasmtime
let mut linker = Linker::new(&engine);
for func in functions {
    // Registrar cada função no linker
    // ...
}
```

---

## Referência Rápida

| Função | Permissão | Requer | Descrição |
|--------|-----------|--------|-----------|
| `terminal_write` | Restricted | `terminal:write` | Envia dados para o terminal |
| `terminal_read` | Restricted | `terminal:read` | Lê input do usuário |
| `terminal_clear` | Restricted | `terminal:clear` | Limpa tela/scrollback |
| `ui_show_notification` | Restricted | `ui:notification` | Mostra notificação |
| `system_exec` | **Dangerous** | `system:exec` | Executa comando sandboxed |

---

## Segurança

1. **Validação de parâmetros**: Todas as funções validam tamanhos e ponteiros
2. **Verificação de permissões**: Cada função verifica se o plugin tem a permissão necessária
3. **Sandbox de execução**: Comandos são filtrados por whitelist
4. **Timeout e limites**: Previne execução infinita e uso excessivo de recursos
5. **Async seguro**: Todas as operações são async-safe usando tokio

---

## Testes

Execute o exemplo completo:

```bash
cargo run --example plugin_host_functions
```

Isso demonstra todas as 5 funções em ação com cenários de sucesso e falha.
