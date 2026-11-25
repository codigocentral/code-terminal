# Plugin API Documentation

**Versão:** 2.0.0  
**Última Atualização:** Março 2026  
**Status:** Implementado

---

## Índice

1. [Visão Geral](#visão-geral)
2. [Arquitetura](#arquitetura)
3. [Sistema de Permissões](#sistema-de-permissões)
4. [Host Functions (80+)](#host-functions-80)
5. [Como Criar um Plugin](#como-criar-um-plugin)
6. [Sandbox de Processos](#sandbox-de-processos)
7. [Exemplos de Plugins](#exemplos-de-plugins)
8. [API de Ciclo de Vida](#api-de-ciclo-de-vida)
9. [Tratamento de Erros](#tratamento-de-erros)
10. [Referência Rápida](#referência-rápida)

---

## Visão Geral

O sistema de plugins do Code Terminal permite estender a funcionalidade através de módulos carregados dinamicamente. Os plugins são executados em sandbox WASM para garantir segurança e isolamento.

### Características Principais

- **Sandbox WASM**: Plugins executam em ambiente isolado via wasmtime
- **80+ Host Functions**: API extensiva para interação com o terminal
- **Sistema de Permissões Granular**: Safe/Restricted/Dangerous
- **Hot Reload**: Recarregamento em tempo de execução
- **Plugin Manifest**: Configuração declarativa via TOML
- **Process Sandbox**: Isolamento de processos externos

---

## Arquitetura

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         Code Terminal Host                              │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │
│  │   Plugin    │  │   Plugin    │  │   Plugin    │  │   Plugin    │   │
│  │  Manager    │  │  Lifecycle  │  │   Sandbox   │  │   Loader    │   │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘   │
│         │                │                │                │          │
│         └────────────────┴────────────────┴────────────────┘          │
│                                  │                                     │
│                                  ▼                                     │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │                    Host Functions API (80+)                      │  │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │  │
│  │  │Terminal │ │  Config │ │  Events │ │  Search │ │ Process │   │  │
│  │  │  (12)   │ │  (8)    │ │  (10)   │ │  (6)    │ │  (8)    │   │  │
│  │  ├─────────┤ ├─────────┤ ├─────────┤ ├─────────┤ ├─────────┤   │  │
│  │  │   UI    │ │ Logging │ │  Mouse  │ │ System  │ │  Exec   │   │  │
│  │  │  (10)   │ │  (8)    │ │  (6)    │ │  (8)    │ │  (4)    │   │  │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘   │  │
│  └─────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                      WASM Runtime (wasmtime)                            │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐   │
│  │  Plugin A   │  │  Plugin B   │  │  Plugin C   │  │  Plugin D   │   │
│  │  .wasm      │  │  .wasm      │  │  .wasm      │  │  .wasm      │   │
│  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘   │
│                                                                         │
│  Cada plugin:                                                           │
│  - Memória isolada (WASM linear memory)                                │
│  - Acesso apenas via Host Functions                                    │
│  - Rate limiting em operações                                          │
│  - Timeout em execuções                                                │
└─────────────────────────────────────────────────────────────────────────┘
```

### Módulos do Sistema

| Módulo | Arquivo | Descrição |
|--------|---------|-----------|
| `plugin::mod` | `src/plugin/mod.rs` | Trait principal e tipos |
| `plugin::api` | `src/plugin/api.rs` | Versionamento e capabilities |
| `plugin::host` | `src/plugin/host/` | Host Functions (80+) |
| `plugin::sandbox` | `src/plugin/sandbox.rs` | Isolamento e rate limiting |
| `plugin::wasm_runtime` | `src/plugin/wasm_runtime.rs` | wasmtime integration |
| `plugin::manager` | `src/plugin/manager.rs` | PluginManager |
| `plugin::loader` | `src/plugin/loader.rs` | Carregamento de WASM |

---

## Sistema de Permissões

O sistema de permissões controla o acesso a Host Functions potencialmente perigosas.

### Níveis de Permissão

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFunctionPermission {
    /// Totalmente seguro, não requer permissão
    Safe,
    
    /// Restrito, pode ter side effects
    Restricted,
    
    /// Requer permissão explícita do usuário
    RequiresPermission,
    
    /// Perigoso, requer confirmação e sandbox
    Dangerous,
}
```

### Categorias de Permissão

| Categoria | Nível | Descrição |
|-----------|-------|-----------|
| `terminal.read` | Safe | Ler do terminal |
| `terminal.write` | Safe | Escrever no terminal |
| `config.read` | Safe | Ler configurações |
| `logging` | Safe | Enviar logs |
| `ui.notification` | Restricted | Mostrar notificações |
| `clipboard` | Restricted | Acesso ao clipboard |
| `filesystem.read` | RequiresPermission | Ler arquivos |
| `filesystem.write` | Dangerous | Escrever arquivos |
| `network` | Dangerous | Acesso à rede |
| `shell.exec` | Dangerous | Executar comandos |
| `process.spawn` | Dangerous | Spawn de processos |

### Manifest de Permissões

```toml
# plugin.toml
[plugin]
name = "meu-plugin"
version = "1.0.0"

[permissions]
# Lista de permissões necessárias
allow = [
    "terminal.read",
    "terminal.write",
    "config.read",
    "ui.notification",
]

# Permissões que requerem confirmação
confirm = [
    "filesystem.read",
]

# Permissões negadas (mesmo que o plugin peça)
deny = [
    "shell.exec",
    "network",
]
```

### Verificação em Tempo de Execução

```rust
impl HostFunction {
    fn check_permission(&self, plugin: &PluginInstance) -> Result<()> {
        match self.permission() {
            HostFunctionPermission::Safe => Ok(()),
            
            HostFunctionPermission::Restricted => {
                // Verifica rate limiting
                if plugin.sandbox.check_rate_limit(self.id()) {
                    Ok(())
                } else {
                    Err(PluginError::RateLimited)
                }
            }
            
            HostFunctionPermission::RequiresPermission => {
                if plugin.manifest.permissions.contains(&self.required_permission()) {
                    Ok(())
                } else {
                    Err(PluginError::PermissionDenied)
                }
            }
            
            HostFunctionPermission::Dangerous => {
                // Requer confirmação do usuário
                if plugin.user_confirmed(self.id()) {
                    Ok(())
                } else {
                    Err(PluginError::PermissionDenied)
                }
            }
        }
    }
}
```

---

## Host Functions (80+)

As Host Functions são funções Rust expostas aos plugins WASM.

### Categorias

#### 1. Terminal Functions (12)

```rust
// Escrita no terminal
fn terminal_write(pane_id: u64, data: &[u8]) -> Result<()>;
fn terminal_write_line(pane_id: u64, line: &str) -> Result<()>;

// Leitura do terminal
fn terminal_read(pane_id: u64, buffer: &mut [u8]) -> Result<usize>;
fn terminal_read_line(pane_id: u64) -> Result<String>;
fn terminal_get_content(pane_id: u64) -> Result<String>;

// Controle do terminal
fn terminal_clear(pane_id: u64, mode: ClearMode) -> Result<()>;
fn terminal_scroll_up(pane_id: u64, lines: u32) -> Result<()>;
fn terminal_scroll_down(pane_id: u64, lines: u32) -> Result<()>;
fn terminal_resize(pane_id: u64, cols: u16, rows: u16) -> Result<()>;

// Cursor
fn terminal_get_cursor_pos(pane_id: u64) -> Result<(u16, u16)>;
fn terminal_set_cursor_pos(pane_id: u64, col: u16, row: u16) -> Result<()>;
fn terminal_get_size(pane_id: u64) -> Result<(u16, u16)>;
```

#### 2. Config Functions (8)

```rust
fn config_get(key: &str) -> Result<Option<String>>;
fn config_get_bool(key: &str) -> Result<bool>;
fn config_get_int(key: &str) -> Result<i64>;
fn config_get_float(key: &str) -> Result<f64>;
fn config_set(key: &str, value: &str) -> Result<()>;
fn config_save() -> Result<()>;
fn config_reload() -> Result<()>;
fn config_get_all() -> Result<HashMap<String, String>>;
```

#### 3. Events Functions (10)

```rust
fn events_subscribe(event_type: EventType) -> Result<u64>;
fn events_unsubscribe(subscription_id: u64) -> Result<()>;
fn events_poll() -> Result<Vec<Event>>;
fn events_wait(timeout_ms: u32) -> Result<Vec<Event>>;
fn events_emit(event_type: EventType, data: &str) -> Result<()>;

// Eventos específicos
fn events_on_input(callback: fn(&str)) -> Result<u64>;
fn events_on_tab_created(callback: fn(u64)) -> Result<u64>;
fn events_on_tab_closed(callback: fn(u64)) -> Result<u64>;
fn events_on_pane_activated(callback: fn(u64)) -> Result<u64>;
fn events_on_theme_changed(callback: fn(&str)) -> Result<u64>;
```

#### 4. Search Functions (6)

```rust
fn search_find(query: &str, options: SearchOptions) -> Result<Vec<SearchResult>>;
fn search_find_next() -> Result<Option<SearchResult>>;
fn search_find_prev() -> Result<Option<SearchResult>>;
fn search_clear() -> Result<()>;
fn search_get_results() -> Result<Vec<SearchResult>>;
fn search_highlight(result: &SearchResult) -> Result<()>;
```

#### 5. Process Functions (8)

```rust
fn process_spawn(cmd: &str, args: &[&str]) -> Result<ProcessHandle>;
fn process_spawn_with_env(cmd: &str, args: &[&str], env: &[(str, str)]) -> Result<ProcessHandle>;
fn process_kill(handle: ProcessHandle) -> Result<()>;
fn process_wait(handle: ProcessHandle, timeout_ms: u32) -> Result<ProcessStatus>;
fn process_get_status(handle: ProcessHandle) -> Result<ProcessStatus>;
fn process_get_output(handle: ProcessHandle) -> Result<CommandOutput>;
fn process_write_stdin(handle: ProcessHandle, data: &[u8]) -> Result<()>;
fn process_list() -> Result<Vec<ProcessInfo>>;
```

#### 6. UI Functions (10)

```rust
fn ui_show_notification(title: &str, message: &str, level: NotificationLevel) -> Result<()>;
fn ui_show_message(message: &str, level: MessageLevel) -> Result<()>;
fn ui_show_input_dialog(prompt: &str, default: Option<&str>) -> Result<Option<String>>;
fn ui_show_confirm_dialog(title: &str, message: &str) -> Result<bool>;
fn ui_show_file_picker() -> Result<Option<String>>;
fn ui_show_folder_picker() -> Result<Option<String>>;
fn ui_set_status_bar(text: &str) -> Result<()>;
fn ui_get_theme() -> Result<String>;
fn ui_set_theme(theme: &str) -> Result<()>;
fn ui_highlight_line(pane_id: u64, line: u32, color: &str) -> Result<()>;
```

#### 7. Logging Functions (8)

```rust
fn log_trace(message: &str) -> Result<()>;
fn log_debug(message: &str) -> Result<()>;
fn log_info(message: &str) -> Result<()>;
fn log_warn(message: &str) -> Result<()>;
fn log_error(message: &str) -> Result<()>;
fn log_with_target(level: LogLevel, target: &str, message: &str) -> Result<()>;
fn log_get_level() -> Result<LogLevel>;
fn log_set_level(level: LogLevel) -> Result<()>;
```

#### 8. Mouse Functions (6)

```rust
fn mouse_get_position() -> Result<(i32, i32)>;
fn mouse_set_position(x: i32, y: i32) -> Result<()>;
fn mouse_click(button: MouseButton) -> Result<()>;
fn mouse_double_click(button: MouseButton) -> Result<()>;
fn mouse_scroll(lines: i32) -> Result<()>;
fn mouse_drag(from: (i32, i32), to: (i32, i32)) -> Result<()>;
```

#### 9. System Functions (8)

```rust
fn system_get_platform() -> Result<String>;
fn system_get_version() -> Result<String>;
fn system_get_env(key: &str) -> Result<Option<String>>;
fn system_set_env(key: &str, value: &str) -> Result<()>;
fn system_get_cwd() -> Result<String>;
fn system_set_cwd(path: &str) -> Result<()>;
fn system_get_time() -> Result<u64>;
fn system_sleep(ms: u32) -> Result<()>;
```

#### 10. Exec Functions (4)

```rust
fn exec_shell(cmd: &str) -> Result<CommandOutput>;
fn exec_shell_async(cmd: &str) -> Result<ProcessHandle>;
fn exec_command(cmd: &str, args: &[&str]) -> Result<CommandOutput>;
fn exec_script(path: &str, args: &[&str]) -> Result<CommandOutput>;
```

### Registro de Host Functions

```rust
pub struct HostFunctions {
    functions: Vec<Box<dyn HostFunction>>,
}

impl HostFunctions {
    pub fn new() -> Self {
        let mut functions = Vec::new();
        functions.extend(terminal::all_functions());
        functions.extend(config::all_functions());
        functions.extend(events::all_functions());
        functions.extend(logging::all_functions());
        functions.extend(ui::all_functions());
        functions.extend(system::all_functions());
        functions.extend(search::all_functions());
        functions.extend(process::all_functions());
        functions.extend(mouse::all_functions());
        
        Self { functions }
    }
}
```

---

## Como Criar um Plugin

### 1. Estrutura do Projeto

```
meu-plugin/
├── Cargo.toml
├── plugin.toml          # Manifest
├── src/
│   └── lib.rs
└── examples/
```

### 2. Cargo.toml

```toml
[package]
name = "meu-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
# Para plugins WASM
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = 3
lto = true
```

### 3. plugin.toml (Manifest)

```toml
[plugin]
name = "meu-plugin"
version = "0.1.0"
description = "Meu plugin incrível"
author = "Seu Nome"
homepage = "https://github.com/user/meu-plugin"
repository = "https://github.com/user/meu-plugin"
license = "MIT"
keywords = ["terminal", "productivity"]

[api]
version = "1.0.0"
min_version = "1.0.0"

[permissions]
allow = [
    "terminal.read",
    "terminal.write",
    "ui.notification",
]

[config]
timeout = "5000"
max_results = "10"

entry = "meu_plugin.wasm"
```

### 4. Código do Plugin

```rust
// src/lib.rs
use serde::{Deserialize, Serialize};

// Importa bindings gerados
mod bindings;
use bindings::*;

#[derive(Serialize, Deserialize)]
struct Config {
    timeout: u64,
    max_results: u32,
}

#[no_mangle]
pub extern "C" fn plugin_init() {
    log_info("MeuPlugin inicializado!");
    
    // Lê configuração
    let config_str = config_get("timeout").unwrap_or_default();
    let timeout = config_str.parse::<u64>().unwrap_or(5000);
    
    log_info(&format!("Timeout configurado: {}", timeout));
}

#[no_mangle]
pub extern "C" fn plugin_update() {
    // Código de atualização periódica
}

#[no_mangle]
pub extern "C" fn plugin_shutdown() {
    log_info("MeuPlugin encerrado!");
}

#[no_mangle]
pub extern "C" fn on_input(input_ptr: i32, input_len: i32) -> i32 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(input_ptr as *const u8, input_len as usize);
        std::str::from_utf8_unchecked(slice)
    };
    
    // Intercepta comando especial
    if input.starts_with("/hello") {
        terminal_write_line(0, "Olá do plugin!");
        return 1; // Consumiu o input
    }
    
    0 // Não consumiu
}

// Exporta funções para o host
bindings::export!(MeuPlugin);
```

### 5. Build

```bash
# Compila para WASM
cargo build --release --target wasm32-unknown-unknown

# Copia para pasta de plugins
cp target/wasm32-unknown-unknown/release/meu_plugin.wasm ~/.config/codeterm/plugins/
```

### 6. Instalação

```bash
# Copia manifest e WASM
cp plugin.toml ~/.config/codeterm/plugins/meu-plugin/
cp meu_plugin.wasm ~/.config/codeterm/plugins/meu-plugin/

# Ou instala via CLI
codeterm plugin install ./meu-plugin
```

---

## Sandbox de Processos

O sandbox de processos isola plugins que executam código externo.

### Configuração do Sandbox

```rust
pub struct SandboxConfig {
    /// Tempo máximo de execução (ms)
    pub timeout_ms: u64,
    /// Memória máxima (bytes)
    pub max_memory: usize,
    /// Número máximo de processos
    pub max_processes: usize,
    /// Diretórios permitidos
    pub allowed_dirs: Vec<PathBuf>,
    /// Diretórios negados
    pub denied_dirs: Vec<PathBuf>,
    /// Rate limit: chamadas por segundo
    pub rate_limit: u32,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 30000,
            max_memory: 128 * 1024 * 1024, // 128MB
            max_processes: 4,
            allowed_dirs: vec![],
            denied_dirs: vec![PathBuf::from("/etc"), PathBuf::from("/root")],
            rate_limit: 100,
        }
    }
}
```

### Rate Limiting

```rust
pub struct RateLimiter {
    calls: Vec<Instant>,
    limit: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn check(&mut self) -> bool {
        let now = Instant::now();
        let window_start = now - self.window;
        
        // Remove chamadas antigas
        self.calls.retain(|&t| t > window_start);
        
        if self.calls.len() >= self.limit as usize {
            false // Rate limit excedido
        } else {
            self.calls.push(now);
            true
        }
    }
}
```

### Monitoramento de Recursos

```rust
pub struct ResourceUsage {
    pub memory_bytes: usize,
    pub cpu_percent: f32,
    pub active_processes: usize,
}

impl Sandbox {
    pub fn check_resources(&self) -> Result<()> {
        let usage = self.get_resource_usage()?;
        
        if usage.memory_bytes > self.config.max_memory {
            return Err(PluginError::ResourceExceeded("memory".to_string()));
        }
        
        if usage.active_processes > self.config.max_processes {
            return Err(PluginError::ResourceExceeded("processes".to_string()));
        }
        
        Ok(())
    }
}
```

---

## Exemplos de Plugins

### Plugin Mínimo

```rust
#[no_mangle]
pub extern "C" fn plugin_init() {
    log_info("Plugin mínimo carregado!");
}

#[no_mangle]
pub extern "C" fn plugin_update() {}

#[no_mangle]
pub extern "C" fn plugin_shutdown() {}
```

### Plugin de Autocomplete

```rust
static mut SUGGESTIONS: Vec<String> = vec![];

#[no_mangle]
pub extern "C" fn plugin_init() {
    unsafe {
        SUGGESTIONS = vec![
            "git".to_string(),
            "cargo".to_string(),
            "docker".to_string(),
        ];
    }
}

#[no_mangle]
pub extern "C" fn on_input(input_ptr: i32, input_len: i32) -> i32 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(input_ptr as *const u8, input_len as usize);
        std::str::from_utf8_unchecked(slice)
    };
    
    unsafe {
        for suggestion in &SUGGESTIONS {
            if suggestion.starts_with(input) {
                terminal_write_line(0, &format!("Sugestão: {}", suggestion));
            }
        }
    }
    
    0
}
```

### Plugin de Notificações

```rust
#[no_mangle]
pub extern "C" fn plugin_init() {
    // Subscreve em eventos de tab
    events_on_tab_created(on_tab_created);
    events_on_tab_closed(on_tab_closed);
}

extern "C" fn on_tab_created(tab_id: u64) {
    ui_show_notification(
        "Tab Criada",
        &format!("Nova tab criada: {}", tab_id),
        NotificationLevel::Info,
    );
}

extern "C" fn on_tab_closed(tab_id: u64) {
    ui_show_notification(
        "Tab Fechada",
        &format!("Tab {} foi fechada", tab_id),
        NotificationLevel::Info,
    );
}
```

---

## API de Ciclo de Vida

### Estados do Plugin

```rust
pub enum PluginState {
    Created,
    Initializing,
    Running,
    Paused,
    Stopping,
    Stopped,
    Error,
}
```

### Transições

```
┌─────────┐    init()     ┌──────────────┐    success    ┌─────────┐
│ Created │ ────────────▶ │ Initializing │ ─────────────▶ │ Running │
└─────────┘               └──────────────┘               └────┬────┘
                                                               │
                              ┌────────────────────────────────┤
                              │                                │
                              ▼                                ▼
                        ┌─────────┐                     ┌─────────┐
                        │  Paused │                     │ Stopping│
                        └────┬────┘                     └────┬────┘
                             │                               │
                             │         ┌─────────────────────┘
                             │         │
                             ▼         ▼
                          ┌──────────────┐
                          │   Stopped    │
                          └──────────────┘
                                 ▲
                                 │
                          ┌──────┴──────┐
                          │    Error    │
                          └─────────────┘
```

### Gerenciamento

```rust
pub struct PluginLifecycle {
    state: Arc<RwLock<PluginState>>,
}

impl PluginLifecycle {
    pub fn transition(&self, new_state: PluginState) -> Result<()> {
        let mut state = self.state.write();
        
        // Valida transição
        match (*state, new_state) {
            (PluginState::Created, PluginState::Initializing) => Ok(()),
            (PluginState::Initializing, PluginState::Running) => Ok(()),
            (PluginState::Running, PluginState::Paused) => Ok(()),
            (PluginState::Running, PluginState::Stopping) => Ok(()),
            (PluginState::Paused, PluginState::Running) => Ok(()),
            (PluginState::Paused, PluginState::Stopping) => Ok(()),
            (PluginState::Stopping, PluginState::Stopped) => Ok(()),
            _ => Err(PluginError::InvalidStateTransition),
        }?;
        
        *state = new_state;
        Ok(())
    }
}
```

---

## Tratamento de Erros

### Tipos de Erro

```rust
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound { name: String },
    
    #[error("Plugin already loaded: {0}")]
    AlreadyLoaded { name: String },
    
    #[error("Incompatible API: host={host}, plugin={plugin}, reason={reason}")]
    IncompatibleApi { host: String, plugin: String, reason: String },
    
    #[error("Initialization failed: {name}, {reason}")]
    InitFailed { name: String, reason: String },
    
    #[error("Sandbox violation: {operation}, {reason}")]
    SandboxViolation { operation: String, reason: String },
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Rate limited")]
    RateLimited,
    
    #[error("Resource exceeded: {0}")]
    ResourceExceeded(String),
    
    #[error("Timeout: {operation}, {duration_ms}ms")]
    Timeout { operation: String, duration_ms: u64 },
    
    #[error("WASM error: {0}")]
    Wasm(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

### Recuperação de Erros

```rust
impl PluginError {
    /// Verifica se o erro é recuperável
    pub fn is_recoverable(&self) -> bool {
        match self {
            PluginError::Timeout { .. } => true,
            PluginError::RateLimited => true,
            PluginError::SandboxViolation { .. } => false,
            PluginError::PermissionDenied(_) => false,
            _ => false,
        }
    }
    
    /// Retorna nome do plugin relacionado
    pub fn plugin_name(&self) -> Option<&str> {
        match self {
            PluginError::NotFound { name } => Some(name),
            PluginError::AlreadyLoaded { name } => Some(name),
            PluginError::InitFailed { name, .. } => Some(name),
            _ => None,
        }
    }
}
```

---

## Referência Rápida

### Comandos de Gerenciamento

```bash
# Listar plugins
codeterm plugin list

# Instalar plugin
codeterm plugin install <path>

# Remover plugin
codeterm plugin remove <name>

# Ativar/Desativar
codeterm plugin enable <name>
codeterm plugin disable <name>

# Hot reload
codeterm plugin reload <name>

# Verificar
codeterm plugin check <name>
```

### Tabela de Host Functions

| Categoria | Quantidade | Nível de Permissão |
|-----------|------------|-------------------|
| Terminal | 12 | Safe |
| Config | 8 | Safe/Restricted |
| Events | 10 | Safe |
| Search | 6 | Safe |
| Process | 8 | Dangerous |
| UI | 10 | Restricted |
| Logging | 8 | Safe |
| Mouse | 6 | Restricted |
| System | 8 | Restricted |
| Exec | 4 | Dangerous |
| **Total** | **80** | - |

---

**Maintained by:** Development Team  
**Last Updated:** Março 2026
