# 🤖 AI Agent Hooks

Sistema de hooks para integração de AI agents (Claude Code, OpenCode, Aider) com o terminal.

## Visão Geral

O sistema de hooks permite que AI agents se registrem no terminal para:
- Receber eventos do terminal (comandos, erros, mudanças)
- Executar ações (editar arquivos, sugerir comandos, analisar código)
- Interagir com o usuário via notificações

## Arquitetura

```
┌─────────────────────────────────────────────────────────────┐
│                     Hook Manager                            │
├─────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │ Claude Code  │  │   OpenCode   │  │    Aider     │      │
│  │    Hook      │  │    Hook      │  │    Hook      │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
├─────────────────────────────────────────────────────────────┤
│                     Hook Registry                           │
├─────────────────────────────────────────────────────────────┤
│  AgentRegistration │ HookHandler │ AgentInfo │ AgentType    │
└─────────────────────────────────────────────────────────────┘
```

## Componentes

### Core Types (`types.rs`)

```rust
/// Tipos de agentes suportados
pub enum AgentType {
    ClaudeCode,  // Anthropic Claude Code
    OpenCode,    // OpenCode SaaS
    Aider,       // Aider (open source)
    Generic,     // Agente personalizado
}

/// Status do agente
pub enum AgentStatus {
    NotDetected,
    Inactive,
    Active,
    Busy,
    Error,
    Disconnected,
}

/// Tipos de hooks
pub enum HookType {
    OnTerminalStart,
    OnTerminalExit,
    OnTabCreated,
    OnTabClosed,
    OnContentChange,
    OnCommandExecute,
    OnCommandComplete,
    OnError,
    OnSuggestionRequest,
}
```

### Event System (`event.rs`)

```rust
/// Evento enviado para agentes
pub struct AgentEvent {
    pub id: Uuid,
    pub timestamp: SystemTime,
    pub event_type: EventType,
    pub priority: EventPriority,
    pub data: EventData,
    pub tab_id: Option<u64>,
}

/// Criar evento
let event = AgentEvent::command_executed(1, "cargo build");
let error = AgentEvent::error_detected(1, "CompileError", "...", ErrorSeverity::Error);
```

### Hook Manager (`manager.rs`)

```rust
// Criar gerenciador
let manager = HookManager::default();

// Registrar agente
let info = AgentInfo::new(AgentType::ClaudeCode, "My Claude");
let reg = AgentRegistration::new(info);
manager.register_agent(reg).await?;

// Emitir evento
let event = AgentEvent::terminal_start();
manager.emit_event(event).await?;

// Enviar requisição
let request = AgentRequest::execute_command("ls -la");
let response = manager.send_request(&agent_id, request).await?;
```

## Agentes Específicos

### Claude Code

```rust
use code_terminal::ai::hooks::{ClaudeCodeHook, ClaudeCodeConfig};

// Detectar
if ClaudeCodeHook::detect() {
    println!("Claude Code disponível!");
}

// Configurar
let config = ClaudeCodeConfig {
    model: "claude-sonnet-4-20250514".to_string(),
    enable_tools: true,
    ..Default::default()
};

let hook = ClaudeCodeHook::new(config);

// Usar
let version = hook.get_version().await?;
let result = hook.send_prompt("Explique este código").await?;
```

**Capacidades:**
- Edição de arquivos
- Execução de comandos
- Análise de código
- Geração de testes
- Explicação de erros

### OpenCode

```rust
use code_terminal::ai::hooks::{OpenCodeHook, OpenCodeConfig};

// Configurar com API key
let config = OpenCodeConfig {
    api_key: env!("OPENCODE_API_KEY").to_string(),
    model: "opencode-pro".to_string(),
    streaming: true,
    ..Default::default()
};

let hook = OpenCodeHook::new(config)?;

// Validar
let valid = hook.validate_api_key().await?;

// Usar
let suggestions = hook.get_suggestions("contexto", 5).await?;
```

**Capacidades:**
- Sugestões de código
- Análise de erros
- Chat interativo
- Streaming de respostas

### Aider

```rust
use code_terminal::ai::hooks::{AiderHook, AiderConfigBuilder};

// Configurar com builder
let config = AiderConfigBuilder::new()
    .model("sonnet")
    .auto_commits(true)
    .lint_cmd("cargo clippy")
    .test_cmd("cargo test")
    .build();

let mut hook = AiderHook::new(config);

// Usar
hook.add_file("src/main.rs").await?;
let result = hook.code_change("Adicione documentação").await?;
hook.commit().await?;
```

**Capacidades:**
- Edição de arquivos via chat
- Git commits automáticos
- Linting integrado
- Execução de testes
- Undo de mudanças

### Agente Genérico

```rust
use code_terminal::ai::hooks::{
    GenericAgentHook, GenericAgentConfigBuilder,
    ProtocolType, DataFormat
};

// Configurar agente personalizado
let config = GenericAgentConfigBuilder::new()
    .command("/path/to/my-agent")
    .arg("--json")
    .env("API_KEY", "secret")
    .protocol(ProtocolType::Stdio)
    .input_format(DataFormat::Json)
    .output_format(DataFormat::Json)
    .capability("custom-task")
    .build();

let mut hook = GenericAgentHook::new("my-agent", config);

// Registrar handlers
hook.register_handler(
    HookType::OnCommandExecute,
    "handle_command",
    HookPriority::Normal
);
```

## API de Requisições

### Comandos Disponíveis

```rust
pub enum AgentCommand {
    ExecuteCommand { command: String, args: Vec<String>, cwd: Option<String> },
    GetSuggestion { context: String, max_suggestions: usize },
    EditFile { path: String, instructions: String },
    ReadFile { path: String, offset: Option<usize>, limit: Option<usize> },
    WriteFile { path: String, content: String },
    SearchCode { query: String, language: Option<String>, path: Option<String> },
    ExplainError { error_output: String, command: Option<String> },
    FixError { error_output: String, auto_apply: bool },
    GitCommit { message: String, files: Vec<String> },
    AnalyzeCode { path: String, analysis_type: AnalysisType },
    GenerateTests { path: String, max_tests: usize },
    Refactor { path: String, instructions: String },
    Ping,
    GetCapabilities,
}
```

### Exemplo de Uso

```rust
// Requisição de execução
let request = AgentRequest::execute_command("cargo build");

// Requisição de sugestão
let request = AgentRequest::get_suggestion("Erro de compilação em...");

// Requisição de edição
let request = AgentRequest::edit_file(
    "src/main.rs",
    "Adicione tratamento de erro"
);

// Enviar
let response = manager.send_request(&agent_id, request).await?;
```

## Contexto

```rust
use code_terminal::ai::hooks::AgentContext;

let context = AgentContext::new("session-1", 1)
    .with_workspace(WorkspaceInfo::with_directory("/project"))
    .with_env_var("EDITOR", "vim")
    .with_config("theme", "dark")
    .with_selection("texto selecionado");

// Informações disponíveis
println!("Diretório: {}", context.current_directory());
println!("Git repo: {}", context.is_git_repo());
```

## Configuração

### Arquivo de Configuração

```toml
[ai.hooks]
enabled = true
auto_detect = true
min_priority = "normal"
default_timeout = 30

[ai.hooks.claude_code]
enabled = true
model = "claude-sonnet-4-20250514"
enable_tools = true

[ai.hooks.opencode]
enabled = true
api_key = "${OPENCODE_API_KEY}"
model = "opencode-pro"
streaming = true

[ai.hooks.aider]
enabled = true
model = "sonnet"
auto_commits = true
lint_cmd = "cargo clippy"
test_cmd = "cargo test"
```

## Eventos

### Tipos de Eventos

| Evento | Descrição | Prioridade |
|--------|-----------|------------|
| `TerminalStart` | Terminal iniciou | High |
| `TerminalExit` | Terminal vai fechar | High |
| `TabCreated` | Nova aba criada | Normal |
| `TabClosed` | Aba fechada | Normal |
| `ContentChanged` | Conteúdo mudou | Low |
| `CommandExecuted` | Comando executado | Normal |
| `CommandCompleted` | Comando terminou | Normal |
| `ErrorDetected` | Erro detectado | High |
| `SuggestionRequested` | Sugestão solicitada | Normal |

### Filtros

```rust
use code_terminal::ai::hooks::EventFilter;

// Apenas eventos de alta prioridade
let filter = EventFilter::min_priority(EventPriority::High);

// Apenas eventos de uma aba específica
let filter = EventFilter::tab(1);

// Eventos específicos
let filter = EventFilter::types(vec![
    EventType::CommandExecuted { .. },
    EventType::ErrorDetected { .. },
]);
```

## Exemplos

Veja a pasta `examples/hooks/` para exemplos completos:

- `basic_hook.rs` - Uso básico do sistema de hooks
- `claude_code_example.rs` - Integração com Claude Code
- `opencode_example.rs` - Integração com OpenCode
- `aider_example.rs` - Integração com Aider
- `generic_agent_example.rs` - Agente genérico personalizado
- `event_handling.rs` - Tratamento de eventos

## Testes

```bash
# Rodar testes do módulo
cargo test ai::hooks

# Testes específicos
cargo test test_agent_registration
cargo test test_event_emission
cargo test test_claude_code
```

## Referência

- [Claude Code](https://docs.anthropic.com/en/docs/claude-code)
- [OpenCode](https://opencode.ai)
- [Aider](https://aider.chat)
