# Guia de Desenvolvimento de Plugins - Codeterm

**Versao:** 1.0.0  
**Ultima Atualizacao:** Marco 2026  
**Aplica-se:** Sprint 11+

---

## Sumario

1. [Visao Geral](#1-visao-geral)
2. [Arquitetura do Sistema de Plugins](#2-arquitetura-do-sistema-de-plugins)
3. [Primeiros Passos](#3-primeiros-passos)
4. [Plugin Trait e Lifecycle](#4-plugin-trait-e-lifecycle)
5. [Host Functions API](#5-host-functions-api)
6. [Seguranca e Sandbox](#6-seguranca-e-sandbox)
7. [Exemplos Praticos](#7-exemplos-praticos)
8. [Testando Plugins](#8-testando-plugins)
9. [Boas Praticas](#9-boas-praticas)
10. [Troubleshooting](#10-troubleshooting)
11. [API Reference](#11-api-reference)

---

## 1. Visao Geral

### 1.1 O que sao Plugins?

Plugins no Codeterm sao modulos WASM (WebAssembly) que estendem a funcionalidade do terminal de forma segura e isolada. Eles rodam em uma sandbox com acesso controlado a APIs do host.

### 1.2 Por que WASM?

| Beneficio | Descricao |
|-----------|-----------|
| **Seguranca** | Sandbox isolada, sem acesso direto ao sistema |
| **Portabilidade** | Funciona em Windows, Linux e macOS |
| **Performance** | Quase-native speed |
| **Linguagens** | Rust, C/C++, AssemblyScript e outras |
| **Versionamento** | API versionada com compatibilidade garantida |

### 1.3 O que Plugins Podem Fazer

- Manipular output do terminal
- Adicionar comandos customizados
- Integrar com ferramentas externas
- Modificar visualizacao de dados
- Implementar syntax highlighting
- Criar notificacoes e alertas
- Interagir com AI Agents

### 1.4 Limitacoes

- Sem acesso direto ao filesystem (exceto via host functions)
- Sem acesso a network direto (exceto via host functions aprovadas)
- Memoria limitada (configuravel, default: 64MB)
- Tempo de execucao limitado por operacao

---

## 2. Arquitetura do Sistema de Plugins

### 2.1 Visao Geral

```
+------------------+
|   Codeterm Host  |
+------------------+
        |
        | WASMI/Wasmtime
        v
+------------------+
|   Plugin Loader  |
+------------------+
        |
        v
+------------------+     +------------------+
|  Plugin Instance |---->|   Host Functions |
|  (WASM Module)   |     |   (API Bridge)   |
+------------------+     +------------------+
        |                        |
        v                        v
+------------------+     +------------------+
|  Plugin Memory   |     |  Terminal State  |
|  (Sandboxed)     |     |  (Controlled)    |
+------------------+     +------------------+
```

### 2.2 Componentes

#### Plugin Loader
Responsavel por carregar, validar e instanciar plugins WASM.

```rust
pub struct PluginLoader {
    engine: wasmtime::Engine,
    host_functions: HostFunctions,
    config: PluginConfig,
}
```

#### Host Functions
Ponte entre plugins e o host (Codeterm). Controla acesso a recursos.

```rust
pub struct HostFunctions {
    terminal: TerminalAccess,
    logger: LoggerAccess,
    config: ConfigAccess,
}
```

#### Plugin Instance
Instancia individual de um plugin em execucao.

```rust
pub struct PluginInstance {
    id: PluginId,
    name: String,
    version: ApiVersion,
    state: PluginState,
    memory: MemoryHandle,
}
```

### 2.3 Fluxo de Execucao

```
1. Host recebe evento (input/output/timer)
2. Host notifica plugins interessados
3. Plugin processa em sua sandbox
4. Plugin chama host functions se necessario
5. Plugin retorna resultado/modificacao
6. Host aplica modificacoes (se seguras)
```

---

## 3. Primeiros Passos

### 3.1 Requisitos

- Rust 1.85+ (recomendado)
- `wasm32-unknown-unknown` target
- Codeterm SDK crate

```bash
# Adicionar target WASM
rustup target add wasm32-unknown-unknown

# Adicionar dependencia
cargo add codeterm-plugin-sdk
```

### 3.2 Estrutura Minima

```
meu-plugin/
├── Cargo.toml
├── src/
│   └── lib.rs
└── plugin.toml      # Metadados do plugin
```

### 3.3 Cargo.toml

```toml
[package]
name = "codeterm-plugin-meu"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
codeterm-plugin-sdk = "0.1"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

[profile.release]
opt-level = "s"      # Otimizar para tamanho
lto = true           # Link-time optimization
```

### 3.4 plugin.toml

```toml
[plugin]
name = "meu-plugin"
version = "0.1.0"
api_version = "1.0.0"
author = "Seu Nome"
description = "Descricao do plugin"
license = "MIT"

[permissions]
terminal_read = true
terminal_write = false
logger = true
config_read = true
config_write = false
```

### 3.5 Hello World

```rust
// src/lib.rs
use codeterm_plugin_sdk::{plugin, Plugin, PluginContext, TerminalEvent};

pub struct MeuPlugin;

impl Plugin for MeuPlugin {
    fn name(&self) -> &str {
        "meu-plugin"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    fn on_load(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        ctx.log_info("MeuPlugin carregado!");
        Ok(())
    }

    fn on_terminal_event(
        &mut self,
        event: &TerminalEvent,
        ctx: &mut PluginContext,
    ) -> Result<(), String> {
        if let TerminalEvent::Output(line) = event {
            ctx.log_debug(&format!("Linha recebida: {}", line));
        }
        Ok(())
    }
}

plugin!(MeuPlugin);
```

### 3.6 Compilar e Instalar

```bash
# Compilar para WASM
cargo build --release --target wasm32-unknown-unknown

# Copiar para diretorio de plugins
cp target/wasm32-unknown-unknown/release/meu_plugin.wasm \
   ~/.config/codeterm/plugins/

# Reiniciar Codeterm ou recarregar plugins
codeterm --reload-plugins
```

---

## 4. Plugin Trait e Lifecycle

### 4.1 Trait Principal

```rust
pub trait Plugin {
    // Metadados obrigatorios
    fn name(&self) -> &str;
    fn version(&self) -> &str;

    // Lifecycle hooks (opcionais)
    fn on_load(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        Ok(())
    }

    fn on_unload(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        Ok(())
    }

    fn on_enable(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        Ok(())
    }

    fn on_disable(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        Ok(())
    }

    // Event handlers (opcionais)
    fn on_terminal_event(
        &mut self,
        event: &TerminalEvent,
        ctx: &mut PluginContext,
    ) -> Result<(), String> {
        Ok(())
    }

    fn on_input(
        &mut self,
        input: &str,
        ctx: &mut PluginContext,
    ) -> Result<InputAction, String> {
        Ok(InputAction::Pass)
    }

    fn on_command(
        &mut self,
        command: &str,
        args: &[&str],
        ctx: &mut PluginContext,
    ) -> Result<CommandResult, String> {
        Ok(CommandResult::NotFound)
    }

    fn on_timer(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        Ok(())
    }
}
```

### 4.2 Lifecycle Completo

```
+----------+     +----------+     +----------+
|  Loaded  |---->| Enabled  |---->| Disabled |
+----------+     +----------+     +----------+
     |                |                |
     v                v                v
 on_load()      on_enable()     on_disable()
                      |
                      v
               [Em Execucao]
                      |
                      v
                 on_unload()
```

### 4.3 Estados do Plugin

| Estado | Descricao |
|--------|-----------|
| `Loaded` | Plugin carregado na memoria |
| `Enabled` | Plugin ativo e recebendo eventos |
| `Disabled` | Plugin inativo |
| `Error` | Plugin em estado de erro |

### 4.4 Macro de Registro

```rust
// Registra o plugin no modulo WASM
plugin!(MeuPlugin);

// Com configuracao customizada
plugin_with_config!(MeuPlugin, PluginConfig {
    max_memory: 32 * 1024 * 1024,  // 32MB
    timeout_ms: 1000,
});
```

---

## 5. Host Functions API

### 5.1 Terminal Access

```rust
impl PluginContext {
    // Ler conteudo do terminal
    pub fn get_line(&self, row: usize) -> Option<String>;
    pub fn get_lines(&self, start: usize, end: usize) -> Vec<String>;
    pub fn get_cursor_position(&self) -> (usize, usize);
    pub fn get_terminal_size(&self) -> (usize, usize);

    // Escrever no terminal (requer permissao)
    pub fn write_output(&mut self, text: &str) -> Result<(), String>;
    pub fn write_error(&mut self, text: &str) -> Result<(), String>;
    pub fn send_input(&mut self, text: &str) -> Result<(), String>;

    // Manipulacao de scrollback
    pub fn get_scrollback(&self, lines: usize) -> Vec<String>;
    pub fn search_scrollback(&self, pattern: &str) -> Vec<SearchResult>;
}
```

### 5.2 Logger

```rust
impl PluginContext {
    pub fn log_trace(&mut self, msg: &str);
    pub fn log_debug(&mut self, msg: &str);
    pub fn log_info(&mut self, msg: &str);
    pub fn log_warn(&mut self, msg: &str);
    pub fn log_error(&mut self, msg: &str);
}
```

### 5.3 Configuration

```rust
impl PluginContext {
    // Ler configuracao
    pub fn get_config(&self, key: &str) -> Option<String>;
    pub fn get_config_int(&self, key: &str) -> Option<i64>;
    pub fn get_config_bool(&self, key: &str) -> Option<bool>;

    // Escrever configuracao (requer permissao)
    pub fn set_config(&mut self, key: &str, value: &str) -> Result<(), String>;
}
```

### 5.4 UI Interactions

```rust
impl PluginContext {
    // Notificacoes
    pub fn show_notification(&mut self, title: &str, body: &str) -> Result<(), String>;
    pub fn show_error(&mut self, title: &str, body: &str) -> Result<(), String>;

    // Dialogs
    pub fn show_dialog(&mut self, config: DialogConfig) -> Result<DialogResult, String>;
    pub fn prompt_input(&mut self, prompt: &str, default: &str) -> Result<String, String>;
}
```

### 5.5 Timers e Scheduling

```rust
impl PluginContext {
    // Agendar callback
    pub fn set_timer(&mut self, delay_ms: u64) -> Result<TimerHandle, String>;
    pub fn set_interval(&mut self, interval_ms: u64) -> Result<IntervalHandle, String>;
    pub fn cancel_timer(&mut self, handle: TimerHandle) -> Result<(), String>;
}
```

### 5.6 Storage

```rust
impl PluginContext {
    // Armazenamento persistente do plugin
    pub fn storage_get(&self, key: &str) -> Option<Vec<u8>>;
    pub fn storage_set(&mut self, key: &str, value: &[u8]) -> Result<(), String>;
    pub fn storage_delete(&mut self, key: &str) -> Result<(), String>;
    pub fn storage_clear(&mut self) -> Result<(), String>;
}
```

---

## 6. Seguranca e Sandbox

### 6.1 Modelo de Permissoes

Permissoes sao declaradas em `plugin.toml`:

```toml
[permissions]
# Terminal
terminal_read = true       # Ler output do terminal
terminal_write = false     # Escrever no terminal

# Logger (sempre permitido)
logger = true

# Configuracao
config_read = true         # Ler config do plugin
config_write = false       # Escrever config global

# UI
notifications = true       # Mostrar notificacoes
dialogs = false            # Mostrar dialogs

# Storage
storage = true             # Storage persistente do plugin

# Network (requer aprovacao explicita do usuario)
network = false

# Filesystem (requer aprovacao explicita do usuario)
filesystem_read = false
filesystem_write = false
```

### 6.2 Limites de Recursos

```toml
[limits]
memory_mb = 64             # Memoria maxima (MB)
cpu_percent = 25           # CPU maximo (%)
execution_timeout_ms = 5000 # Timeout por operacao
```

### 6.3 Validacao de Input

Todos os dados passados entre host e plugin sao validados:

```rust
// Host valida antes de passar para plugin
pub fn validate_terminal_input(input: &str) -> Result<ValidatedInput, ValidationError> {
    // Verificar tamanho maximo
    if input.len() > MAX_INPUT_SIZE {
        return Err(ValidationError::InputTooLarge);
    }

    // Verificar caracteres validos
    if !input.chars().all(|c| c.is_ascii() || c.is_utf8_valid()) {
        return Err(ValidationError::InvalidCharacters);
    }

    Ok(ValidatedInput(input.to_string()))
}
```

### 6.4 Isolamento de Memoria

- Cada plugin roda em sua propria instancia WASM
- Memoria nao e compartilhada entre plugins
- Host functions sao a unica forma de comunicacao

### 6.5 Auditoria e Logging

Todas as acoes de plugins sao logadas:

```
[PLUGIN] meu-plugin: terminal_read(line=42)
[PLUGIN] meu-plugin: config_get(key="theme")
[PLUGIN] meu-plugin: storage_set(key="history", size=1024)
```

---

## 7. Exemplos Praticos

### 7.1 Syntax Highlighter

Plugin que adiciona syntax highlighting para JSON:

```rust
use codeterm_plugin_sdk::{plugin, Plugin, PluginContext, TerminalEvent};
use serde_json::Value;

pub struct JsonHighlighter;

impl Plugin for JsonHighlighter {
    fn name(&self) -> &str { "json-highlighter" }
    fn version(&self) -> &str { "0.1.0" }

    fn on_terminal_event(
        &mut self,
        event: &TerminalEvent,
        ctx: &mut PluginContext,
    ) -> Result<(), String> {
        if let TerminalEvent::Output(line) = event {
            if self.looks_like_json(line) {
                if let Ok(highlighted) = self.highlight_json(line) {
                    ctx.log_debug(&format!("JSON detectado: {} bytes", line.len()));
                }
            }
        }
        Ok(())
    }
}

impl JsonHighlighter {
    fn looks_like_json(&self, text: &str) -> bool {
        let trimmed = text.trim();
        trimmed.starts_with('{') || trimmed.starts_with('[')
    }

    fn highlight_json(&self, text: &str) -> Result<String, String> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| format!("JSON invalido: {}", e))?;
        Ok(serde_json::to_string_pretty(&value).unwrap())
    }
}

plugin!(JsonHighlighter);
```

### 7.2 Command Plugin

Plugin que adiciona comandos customizados:

```rust
use codeterm_plugin_sdk::{
    plugin, Plugin, PluginContext,
    CommandResult, InputAction,
};

pub struct GitCommands {
    current_branch: Option<String>,
}

impl Plugin for GitCommands {
    fn name(&self) -> &str { "git-commands" }
    fn version(&self) -> &str { "0.1.0" }

    fn on_load(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        ctx.log_info("GitCommands carregado");
        Ok(())
    }

    fn on_command(
        &mut self,
        command: &str,
        args: &[&str],
        ctx: &mut PluginContext,
    ) -> Result<CommandResult, String> {
        match command {
            "/branch" => self.cmd_branch(ctx),
            "/status" => self.cmd_status(ctx),
            "/commit" => self.cmd_commit(args, ctx),
            _ => Ok(CommandResult::NotFound),
        }
    }
}

impl GitCommands {
    fn cmd_branch(&mut self, ctx: &mut PluginContext) -> Result<CommandResult, String> {
        ctx.send_input("git branch --show-current\n")?;
        Ok(CommandResult::Handled)
    }

    fn cmd_status(&self, ctx: &mut PluginContext) -> Result<CommandResult, String> {
        ctx.send_input("git status -s\n")?;
        Ok(CommandResult::Handled)
    }

    fn cmd_commit(
        &self,
        args: &[&str],
        ctx: &mut PluginContext,
    ) -> Result<CommandResult, String> {
        if args.is_empty() {
            return Ok(CommandResult::Error("Uso: /commit <mensagem>".into()));
        }
        let message = args.join(" ");
        ctx.send_input(&format!("git commit -m \"{}\"\n", message))?;
        Ok(CommandResult::Handled)
    }
}

plugin!(GitCommands);
```

### 7.3 Notification Plugin

Plugin que monitora output e notifica:

```rust
use codeterm_plugin_sdk::{plugin, Plugin, PluginContext, TerminalEvent};

pub struct ErrorNotifier {
    patterns: Vec<String>,
}

impl Plugin for ErrorNotifier {
    fn name(&self) -> &str { "error-notifier" }
    fn version(&self) -> &str { "0.1.0" }

    fn on_load(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        self.patterns = vec![
            "error".to_string(),
            "failed".to_string(),
            "exception".to_string(),
        ];
        ctx.log_info("ErrorNotifier ativo");
        Ok(())
    }

    fn on_terminal_event(
        &mut self,
        event: &TerminalEvent,
        ctx: &mut PluginContext,
    ) -> Result<(), String> {
        if let TerminalEvent::Output(line) = event {
            let line_lower = line.to_lowercase();
            
            for pattern in &self.patterns {
                if line_lower.contains(pattern) {
                    ctx.show_notification(
                        "Erro Detectado",
                        &format!("Padrao '{}' encontrado", pattern),
                    )?;
                    break;
                }
            }
        }
        Ok(())
    }
}

plugin!(ErrorNotifier);
```

### 7.4 Timer Plugin

Plugin que executa acoes periodicamente:

```rust
use codeterm_plugin_sdk::{
    plugin, Plugin, PluginContext, TimerHandle,
};

pub struct AutoSave {
    timer: Option<TimerHandle>,
    counter: u64,
}

impl Plugin for AutoSave {
    fn name(&self) -> &str { "auto-save" }
    fn version(&self) -> &str { "0.1.0" }

    fn on_enable(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        // Salvar a cada 5 minutos
        self.timer = Some(ctx.set_interval(5 * 60 * 1000)?);
        self.counter = 0;
        ctx.log_info("AutoSave ativado (5 min)");
        Ok(())
    }

    fn on_disable(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        if let Some(timer) = self.timer.take() {
            ctx.cancel_timer(timer)?;
        }
        ctx.log_info("AutoSave desativado");
        Ok(())
    }

    fn on_timer(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        self.counter += 1;
        ctx.log_info(&format!("Timer tick #{}", self.counter));
        
        // Executar acao
        ctx.send_input("echo 'Auto-save triggered'\n")?;
        
        Ok(())
    }
}

plugin!(AutoSave);
```

### 7.5 Config-Driven Plugin

Plugin com configuracao customizada:

```rust
use codeterm_plugin_sdk::{plugin, Plugin, PluginContext, TerminalEvent};
use serde::Deserialize;

#[derive(Deserialize)]
struct Config {
    enabled: bool,
    max_lines: usize,
    keywords: Vec<String>,
}

pub struct FilterPlugin {
    config: Config,
}

impl Plugin for FilterPlugin {
    fn name(&self) -> &str { "filter-plugin" }
    fn version(&self) -> &str { "0.1.0" }

    fn on_load(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
        // Carregar configuracao
        let config_str = ctx.get_config("filter")
            .unwrap_or_else(|| r#"{"enabled":true,"max_lines":100,"keywords":[]}"#.to_string());
        
        self.config = serde_json::from_str(&config_str)
            .map_err(|e| format!("Config invalida: {}", e))?;
        
        ctx.log_info(&format!("FilterPlugin carregado: {} keywords", 
            self.config.keywords.len()));
        Ok(())
    }

    fn on_terminal_event(
        &mut self,
        event: &TerminalEvent,
        ctx: &mut PluginContext,
    ) -> Result<(), String> {
        if !self.config.enabled {
            return Ok(());
        }

        if let TerminalEvent::Output(line) = event {
            for keyword in &self.config.keywords {
                if line.contains(keyword) {
                    ctx.log_debug(&format!("Keyword '{}' encontrada", keyword));
                }
            }
        }
        Ok(())
    }
}

plugin!(FilterPlugin);
```

---

## 8. Testando Plugins

### 8.1 Testes Unitarios

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use codeterm_plugin_sdk::testing::*;

    #[test]
    fn test_plugin_loads() {
        let mut plugin = JsonHighlighter;
        let mut ctx = MockPluginContext::new();
        
        assert!(plugin.on_load(&mut ctx).is_ok());
        assert!(ctx.logs().iter().any(|l| l.contains("carregado")));
    }

    #[test]
    fn test_json_detection() {
        let plugin = JsonHighlighter;
        
        assert!(plugin.looks_like_json("{\"key\": \"value\"}"));
        assert!(plugin.looks_like_json("[1, 2, 3]"));
        assert!(!plugin.looks_like_json("not json"));
    }

    #[test]
    fn test_highlighting() {
        let plugin = JsonHighlighter;
        let result = plugin.highlight_json("{\"a\":1}");
        
        assert!(result.is_ok());
        assert!(result.unwrap().contains("\"a\""));
    }
}
```

### 8.2 Testes de Integracao

```rust
#[cfg(test)]
mod integration_tests {
    use super::*;
    use codeterm_plugin_sdk::testing::*;

    #[test]
    fn test_full_lifecycle() {
        let mut harness = PluginTestHarness::new(JsonHighlighter);
        
        // Carregar
        assert!(harness.load().is_ok());
        
        // Enviar evento
        let event = TerminalEvent::Output("{\"test\": 123}".to_string());
        assert!(harness.send_event(event).is_ok());
        
        // Descarregar
        assert!(harness.unload().is_ok());
    }
}
```

### 8.3 Testando com o Host Real

```bash
# Modo de desenvolvimento com logs detalhados
codeterm --plugin-debug meu-plugin

# Verificar logs
tail -f ~/.local/share/codeterm/logs/plugin-meu-plugin.log
```

### 8.4 Mock Context

```rust
pub struct MockPluginContext {
    logs: Vec<(LogLevel, String)>,
    outputs: Vec<String>,
    config: HashMap<String, String>,
}

impl MockPluginContext {
    pub fn new() -> Self {
        Self {
            logs: Vec::new(),
            outputs: Vec::new(),
            config: HashMap::new(),
        }
    }

    pub fn logs(&self) -> &[(LogLevel, String)] {
        &self.logs
    }

    pub fn outputs(&self) -> &[String] {
        &self.outputs
    }

    pub fn set_config(&mut self, key: &str, value: &str) {
        self.config.insert(key.to_string(), value.to_string());
    }
}

impl PluginContext for MockPluginContext {
    fn log_info(&mut self, msg: &str) {
        self.logs.push((LogLevel::Info, msg.to_string()));
    }

    fn write_output(&mut self, text: &str) -> Result<(), String> {
        self.outputs.push(text.to_string());
        Ok(())
    }

    fn get_config(&self, key: &str) -> Option<String> {
        self.config.get(key).cloned()
    }

    // ... outros metodos com implementacoes mock
}
```

---

## 9. Boas Praticas

### 9.1 Performance

```rust
// BOM: Processar apenas quando necessario
fn on_terminal_event(&mut self, event: &TerminalEvent, ctx: &mut PluginContext) {
    if let TerminalEvent::Output(line) = event {
        if self.should_process(line) {  // Check rapido primeiro
            self.process(line);
        }
    }
}

// RUIM: Processar tudo
fn on_terminal_event(&mut self, event: &TerminalEvent, ctx: &mut PluginContext) {
    if let TerminalEvent::Output(line) = event {
        self.heavy_process(line);  // Sempre executa
    }
}
```

### 9.2 Tratamento de Erros

```rust
// BOM: Erros explicitos com contexto
fn on_command(&mut self, cmd: &str, args: &[&str], ctx: &mut PluginContext) -> Result<CommandResult, String> {
    let value = ctx.get_config("key")
        .ok_or("Configuracao 'key' nao encontrada")?;
    
    let parsed: i64 = value.parse()
        .map_err(|e| format!("Valor invalido: {}", e))?;
    
    Ok(CommandResult::Handled)
}

// RUIM: unwrap/expect em producao
fn on_command(&mut self, cmd: &str, args: &[&str], ctx: &mut PluginContext) -> Result<CommandResult, String> {
    let value = ctx.get_config("key").unwrap();  // Pode panicar
    Ok(CommandResult::Handled)
}
```

### 9.3 Logging

```rust
// BOM: Logs estruturados e uteis
fn on_load(&mut self, ctx: &mut PluginContext) -> Result<(), String> {
    ctx.log_info(&format!("Carregando plugin v{}", self.version()));
    ctx.log_debug(&format!("Config: {:?}", self.config));
    Ok(())
}

// RUIM: Logs excessivos ou inuteis
fn on_terminal_event(&mut self, event: &TerminalEvent, ctx: &mut PluginContext) {
    ctx.log_trace("Evento recebido");  // Muito verboso
    ctx.log_info(&format!("{:?}", event));  // Desnecessario em info
}
```

### 9.4 Configuracao

```rust
// BOM: Defaults sensatos
impl MyPlugin {
    fn load_config(&self, ctx: &PluginContext) -> Config {
        Config {
            enabled: ctx.get_config_bool("enabled").unwrap_or(true),
            timeout: ctx.get_config_int("timeout").unwrap_or(5000),
        }
    }
}

// RUIM: Requer todas as configs
impl MyPlugin {
    fn load_config(&self, ctx: &PluginContext) -> Result<Config, String> {
        Ok(Config {
            enabled: ctx.get_config_bool("enabled")
                .ok_or("enabled required")?,  // Muito restritivo
        })
    }
}
```

### 9.5 Memoria

```rust
// BOM: Limitar uso de memoria
fn process_large_output(&mut self, data: &str, ctx: &mut PluginContext) {
    const MAX_LINES: usize = 1000;
    
    let lines: Vec<_> = data.lines().take(MAX_LINES).collect();
    // Processar lines...
}

// RUIM: Carregar tudo na memoria
fn process_large_output(&mut self, data: &str, ctx: &mut PluginContext) {
    let all_lines: Vec<_> = data.lines().collect();  // Pode estourar memoria
}
```

---

## 10. Troubleshooting

### 10.1 Plugin nao Carrega

```
Erro: Failed to load plugin: InvalidWasm
```

**Solucao:** Verificar se compilou para o target correto:
```bash
cargo build --release --target wasm32-unknown-unknown
```

### 10.2 Permissao Negada

```
Erro: Permission denied: terminal_write
```

**Solucao:** Adicionar permissao no `plugin.toml`:
```toml
[permissions]
terminal_write = true
```

### 10.3 Timeout

```
Erro: Execution timeout exceeded (5000ms)
```

**Solucao:** Otimizar codigo ou aumentar limite:
```toml
[limits]
execution_timeout_ms = 10000
```

### 10.4 Memoria Insuficiente

```
Erro: Memory limit exceeded (64MB)
```

**Solucao:** Reduzir uso de memoria ou aumentar limite:
```toml
[limits]
memory_mb = 128
```

### 10.5 API Incompativel

```
Erro: API version mismatch: expected 1.0.0, got 0.9.0
```

**Solucao:** Atualizar versao da API no `plugin.toml`:
```toml
[plugin]
api_version = "1.0.0"
```

### 10.6 Debug Mode

```bash
# Executar com debug de plugins
codeterm --plugin-debug

# Verificar logs
codeterm --log-level debug 2>&1 | grep PLUGIN
```

---

## 11. API Reference

### 11.1 Tipos Principais

```rust
pub enum TerminalEvent {
    Output(String),
    Input(String),
    Resize { rows: usize, cols: usize },
    TitleChange(String),
    Bell,
}

pub enum InputAction {
    Pass,
    Consume,
    Replace(String),
}

pub enum CommandResult {
    NotFound,
    Handled,
    Output(String),
    Error(String),
}

pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

pub struct DialogConfig {
    pub title: String,
    pub message: String,
    pub buttons: Vec<String>,
    pub default_button: Option<usize>,
}

pub enum DialogResult {
    Button(usize),
    Cancelled,
}

pub struct SearchResult {
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub context: String,
}
```

### 11.2 Trait Plugin Completo

```rust
pub trait Plugin {
    // Metadados
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn author(&self) -> &str { "Unknown" }
    fn description(&self) -> &str { "" }

    // Lifecycle
    fn on_load(&mut self, ctx: &mut PluginContext) -> Result<(), String> { Ok(()) }
    fn on_unload(&mut self, ctx: &mut PluginContext) -> Result<(), String> { Ok(()) }
    fn on_enable(&mut self, ctx: &mut PluginContext) -> Result<(), String> { Ok(()) }
    fn on_disable(&mut self, ctx: &mut PluginContext) -> Result<(), String> { Ok(()) }

    // Eventos
    fn on_terminal_event(&mut self, event: &TerminalEvent, ctx: &mut PluginContext) -> Result<(), String> { Ok(()) }
    fn on_input(&mut self, input: &str, ctx: &mut PluginContext) -> Result<InputAction, String> { Ok(InputAction::Pass) }
    fn on_command(&mut self, command: &str, args: &[&str], ctx: &mut PluginContext) -> Result<CommandResult, String> { Ok(CommandResult::NotFound) }
    fn on_timer(&mut self, ctx: &mut PluginContext) -> Result<(), String> { Ok(()) }

    // Configuracao
    fn get_config_schema(&self) -> Option<ConfigSchema> { None }
}
```

### 11.3 Host Functions Completas

```rust
pub trait PluginContext {
    // === Terminal ===
    fn get_line(&self, row: usize) -> Option<String>;
    fn get_lines(&self, start: usize, end: usize) -> Vec<String>;
    fn get_cursor_position(&self) -> (usize, usize);
    fn get_terminal_size(&self) -> (usize, usize);
    fn get_scrollback(&self, lines: usize) -> Vec<String>;
    fn search_scrollback(&self, pattern: &str) -> Vec<SearchResult>;
    fn write_output(&mut self, text: &str) -> Result<(), String>;
    fn write_error(&mut self, text: &str) -> Result<(), String>;
    fn send_input(&mut self, text: &str) -> Result<(), String>;

    // === Logging ===
    fn log_trace(&mut self, msg: &str);
    fn log_debug(&mut self, msg: &str);
    fn log_info(&mut self, msg: &str);
    fn log_warn(&mut self, msg: &str);
    fn log_error(&mut self, msg: &str);

    // === Config ===
    fn get_config(&self, key: &str) -> Option<String>;
    fn get_config_int(&self, key: &str) -> Option<i64>;
    fn get_config_bool(&self, key: &str) -> Option<bool>;
    fn set_config(&mut self, key: &str, value: &str) -> Result<(), String>;

    // === UI ===
    fn show_notification(&mut self, title: &str, body: &str) -> Result<(), String>;
    fn show_error(&mut self, title: &str, body: &str) -> Result<(), String>;
    fn show_dialog(&mut self, config: DialogConfig) -> Result<DialogResult, String>;
    fn prompt_input(&mut self, prompt: &str, default: &str) -> Result<String, String>;

    // === Timers ===
    fn set_timer(&mut self, delay_ms: u64) -> Result<TimerHandle, String>;
    fn set_interval(&mut self, interval_ms: u64) -> Result<IntervalHandle, String>;
    fn cancel_timer(&mut self, handle: TimerHandle) -> Result<(), String>;

    // === Storage ===
    fn storage_get(&self, key: &str) -> Option<Vec<u8>>;
    fn storage_set(&mut self, key: &str, value: &[u8]) -> Result<(), String>;
    fn storage_delete(&mut self, key: &str) -> Result<(), String>;
    fn storage_clear(&mut self) -> Result<(), String>;
    fn storage_keys(&self) -> Vec<String>;

    // === Plugin Info ===
    fn plugin_id(&self) -> &str;
    fn plugin_dir(&self) -> &Path;
}
```

---

## Apendice A: Estrutura de Diretorios

```
~/.config/codeterm/
├── plugins/
│   ├── meu-plugin/
│   │   ├── plugin.wasm
│   │   ├── plugin.toml
│   │   └── config.toml
│   └── outro-plugin/
│       └── ...
├── config.toml
└── keybindings.toml

~/.local/share/codeterm/
├── plugins/
│   └── storage/
│       └── meu-plugin/
│           └── data.db
└── logs/
    └── plugin-meu-plugin.log
```

## Apendice B: Comandos Uteis

```bash
# Listar plugins instalados
codeterm --list-plugins

# Verificar permissoes de um plugin
codeterm --plugin-permissions meu-plugin

# Recarregar plugins sem reiniciar
codeterm --reload-plugins

# Executar com plugin especifico em modo debug
codeterm --plugin-debug meu-plugin

# Validar plugin antes de instalar
codeterm --validate-plugin ./meu-plugin.wasm
```

## Apendice C: Recursos

- **SDK Repository:** https://github.com/codeterm/plugin-sdk
- **API Docs:** https://docs.codeterm.dev/plugin-api
- **Examples:** https://github.com/codeterm/plugin-examples
- **Community:** https://discord.gg/codeterm

---

**Documento mantido pela equipe de desenvolvimento do Codeterm**  
**Contribuicoes sao bem-vindas via GitHub Issues/PRs**
