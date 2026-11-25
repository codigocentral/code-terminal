# AI Integration - Codeterm

**Status:** ✅ Implemented (TASK-123)  
**Sprint:** 16 - AI Integration  
**Last Updated:** Março 2026

---

## 📋 Overview

O Codeterm possui integração com AI para fornecer sugestões de comandos, explicações de erros e assistência inteligente no terminal. A implementação é **privacy-first**, opcional e totalmente configurável.

### Features Principais

- ✅ **OpenAI Integration** - Suporte completo à API OpenAI
- ✅ **Command Suggestions** - Sugestões inteligentes de comandos
- ✅ **Error Explanation** - Explicação de mensagens de erro
- ✅ **Privacy Controls** - Controles rigorosos de privacidade
- ✅ **Provider Abstraction** - Arquitetura extensível para múltiplos providers

---

## 🏗️ Architecture

### Module Structure

```
src/ai/
├── mod.rs           # Module exports and factory
├── config.rs        # Configuration and privacy settings
├── error.rs         # AI-specific error types
├── message.rs       # Message and conversation types
├── provider.rs      # AIProvider trait and registry
├── openai.rs        # OpenAI implementation
└── privacy.rs       # Privacy controls and auditing
```

### Data Flow

```
┌─────────────┐
│   User      │
│   Input     │
└──────┬──────┘
       │
       ▼
┌─────────────┐      ┌──────────────┐
│  Terminal   │─────▶│   Context    │
│   Context   │      │   Builder    │
└─────────────┘      └──────┬───────┘
                            │
                            ▼
                     ┌──────────────┐
                     │   Privacy    │
                     │   Filter     │
                     └──────┬───────┘
                            │
                            ▼
┌─────────────┐      ┌──────────────┐
│   OpenAI    │◀─────│   Provider   │
│     API     │      │   Factory    │
└─────────────┘      └──────────────┘
       │
       ▼
┌─────────────┐
│  Response   │
│  Handler    │
└─────────────┘
```

---

## 🚀 Quick Start

### 1. Configuration

Adicione ao seu `config.toml`:

```toml
[ai]
enabled = true
provider = "openai"
api_key = "sk-your-api-key-here"
model = "gpt-4o-mini"
max_tokens = 1024
temperature = 0.7

[ai.privacy]
send_shell_history = false
send_file_paths = false
send_environment_vars = false
require_confirmation = true
log_requests = false
```

### 2. Programmatic Usage

```rust
use code_terminal::ai::{
    AiConfig, OpenAIProvider, TerminalContext, AIProvider
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create configuration
    let config = AiConfig::openai("sk-your-api-key")
        .with_model("gpt-4o-mini")
        .with_max_tokens(1024)
        .with_temperature(0.7);
    
    // Create provider
    let provider = OpenAIProvider::new(config)?;
    
    // Build terminal context
    let context = TerminalContext::new()
        .with_directory("/home/user/projects")
        .with_shell("bash")
        .with_recent_commands(vec!["git status".to_string()]);
    
    // Get command suggestion
    let suggestion = provider
        .suggest_command(&context, "how do I commit changes?")
        .await?;
    
    println!("Command: {}", suggestion.command);
    println!("Explanation: {}", suggestion.explanation);
    
    Ok(())
}
```

---

## 🔧 Configuration Options

### AiConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | false | Enable/disable AI features |
| `provider` | AiProviderType | OpenAI | AI provider to use |
| `api_key` | Option<String> | None | API key for authentication |
| `base_url` | Option<String> | None | Custom API endpoint |
| `model` | String | "gpt-4o-mini" | Model to use |
| `max_tokens` | u32 | 1024 | Maximum tokens in response |
| `temperature` | f32 | 0.7 | Response randomness (0.0-2.0) |
| `timeout_seconds` | u64 | 30 | Request timeout |
| `privacy` | PrivacyConfig | - | Privacy settings |

### PrivacyConfig

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `send_shell_history` | bool | false | Send recent commands to AI |
| `send_file_paths` | bool | false | Send current directory path |
| `send_environment_vars` | bool | false | Send environment variables |
| `redact_patterns` | Vec<String> | [...] | Patterns to redact from data |
| `require_confirmation` | bool | true | Require user confirmation |
| `log_requests` | bool | false | Log all AI requests |

### Privacy Redaction Patterns

Por padrão, os seguintes padrões são redacionados:

```regex
[A-Za-z0-9]{32,}              # Long alphanumeric strings
sk-[A-Za-z0-9]{20,}           # API keys
password\s*[=:]\s*\S+         # Password assignments
token\s*[=:]\s*\S+            # Token assignments
secret\s*[=:]\s*\S+           # Secret assignments
api[_-]?key\s*[=:]\s*\S+      # API key assignments
```

---

## 📚 API Reference

### AIProvider Trait

```rust
#[async_trait]
pub trait AIProvider: Send + Sync {
    /// Send a chat completion request
    async fn chat(&self, request: ChatRequest) -> AiResult<ChatResponse>;
    
    /// Get command suggestion based on context
    async fn suggest_command(
        &self,
        context: &TerminalContext,
        user_input: &str,
    ) -> AiResult<CommandSuggestion>;
    
    /// Explain an error message
    async fn explain_error(
        &self,
        error_message: &str,
        context: &TerminalContext,
    ) -> AiResult<String>;
    
    /// Get provider name
    fn name(&self) -> &str;
    
    /// Check if provider is properly configured
    fn is_configured(&self) -> bool;
}
```

### TerminalContext

```rust
let context = TerminalContext::new()
    .with_directory("/path/to/dir")
    .with_shell("bash")
    .with_recent_commands(vec!["cmd1".to_string()])
    .with_last_output("some output");

// Or build incrementally
let mut context = TerminalContext::new();
context.add_command("new command");
```

### CommandSuggestion

```rust
pub struct CommandSuggestion {
    pub command: String,           // The suggested command
    pub explanation: String,       // Why this command
    pub alternatives: Option<Vec<String>>, // Alternative commands
    pub confidence: f32,           // 0.0 to 1.0
}
```

---

## 🔒 Security & Privacy

### Privacy-First Design

1. **Opt-in by default** - AI features are disabled unless explicitly enabled
2. **Data minimization** - Only send what's necessary
3. **Pattern redaction** - Automatic redaction of sensitive data
4. **User confirmation** - Require confirmation before sending data
5. **Audit logging** - Optional logging of all requests

### Best Practices

```rust
// ✅ Good: Minimal context
let context = TerminalContext::new()
    .with_shell("bash");

// ❌ Avoid: Sending sensitive data
let context = TerminalContext::new()
    .with_directory("/home/user/.ssh")  // Don't send sensitive paths
    .with_environment_vars(sensitive_vars);  // Don't send env vars

// ✅ Better: Use privacy controls
let mut config = AiConfig::openai("key");
config.privacy.send_file_paths = false;
config.privacy.require_confirmation = true;
```

### Data Sent to AI

When privacy settings are at defaults:

| Data | Sent? | Notes |
|------|-------|-------|
| Current directory | ❌ | Redacted |
| Shell type | ✅ | Safe metadata |
| Recent commands | ❌ | Disabled by default |
| Last output | ❌ | Redacted |
| Environment vars | ❌ | Never sent |

---

## 🧪 Testing

### Unit Tests

```bash
# Run AI module tests
cargo test --lib ai::

# Run specific test
cargo test test_openai_provider_creation

# Run all AI tests
cargo test --all-features ai
```

### Integration Tests

```bash
# Run AI integration tests
cargo test --test ai_integration_test
```

### Test Coverage

- ✅ Configuration parsing and validation
- ✅ Provider creation and initialization
- ✅ Privacy redaction patterns
- ✅ Terminal context building
- ✅ Error handling
- ✅ Serialization/deserialization

---

## 📊 Performance

### Benchmarks

| Operation | Target | Typical |
|-----------|--------|---------|
| Provider initialization | < 10ms | ~5ms |
| Command suggestion | < 2s | ~1.2s |
| Error explanation | < 2s | ~1.5s |
| Privacy redaction | < 5ms | ~2ms |

### Optimizations

1. **Request Caching**
   - Cache frequent command suggestions
   - TTL-based invalidation
   - LRU eviction policy

2. **Async Operations**
   - Non-blocking AI requests
   - Background context building
   - Streaming responses (planned)

3. **Token Efficiency**
   - Compress context before sending
   - Use efficient prompts
   - Reuse conversation history

### Resource Usage

| Resource | Usage | Notes |
|----------|-------|-------|
| Memory | ~5-10 MB | Provider instance |
| CPU | Minimal | Async operations |
| Network | Variable | Depends on API calls |

---

## 🎯 Use Cases

### 1. Command Suggestions

```rust
// User asks: "How do I find large files?"
let suggestion = provider
    .suggest_command(&context, "find large files over 100MB")
    .await?;

// AI returns: "find . -type f -size +100M -exec ls -lh {} \;"
println!("{}", suggestion.command);
println!("{}", suggestion.explanation);
```

### 2. Error Explanation

```rust
// Command failed with error
let error = "bash: docker: command not found";

let explanation = provider
    .explain_error(error, &context)
    .await?;

// AI explains the error and suggests fixes
println!("{}", explanation);
// Output: "Docker is not installed. To install on Ubuntu: sudo apt install docker.io"
```

### 3. Natural Language to Command

```rust
// Convert natural language to shell command
let query = "show me all python files modified in the last week";

let suggestion = provider
    .suggest_command(&context, query)
    .await?;

// AI returns: "find . -name '*.py' -mtime -7"
```

### 4. Git Workflow Assistance

```rust
let context = TerminalContext::new()
    .with_directory("/project")
    .with_recent_commands(vec![
        "git status".to_string(),
        "git diff".to_string(),
    ]);

let suggestion = provider
    .suggest_command(&context, "commit these changes")
    .await?;

// AI suggests: "git add -A && git commit -m 'message'"
```

---

## 🔧 Advanced Configuration

### Multiple Providers

```toml
[ai]
enabled = true
provider = "openai"

[ai.providers.openai]
api_key = "${OPENAI_API_KEY}"
model = "gpt-4o-mini"

[ai.providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"
model = "claude-3-haiku-20240307"

[ai.providers.local]
base_url = "http://localhost:11434"
model = "llama2"
```

### Custom Prompts

```toml
[ai.prompts]
system = "You are a terminal assistant for a software developer."
suggestion_prefix = "Suggest a shell command to: "
error_prefix = "Explain this terminal error: "
```

### Rate Limiting

```toml
[ai.rate_limiting]
enabled = true
requests_per_minute = 20
tokens_per_day = 100000
retry_after_seconds = 60
```

---

## 🐛 Troubleshooting

### Common Issues

#### 1. API Key Invalid

```
Error: AiError::ApiError("Invalid API key")
```

**Solution:**
```bash
# Check API key is set
echo $OPENAI_API_KEY

# Set in config.toml
[ai]
api_key = "sk-your-key-here"
```

#### 2. Connection Timeout

```
Error: AiError::Timeout(30s)
```

**Solution:**
```toml
[ai]
timeout_seconds = 60
```

#### 3. Rate Limited

```
Error: AiError::RateLimited { retry_after: 60s }
```

**Solution:**
- Wait for the retry period
- Use a different provider
- Upgrade your API plan

#### 4. Privacy Violation

```
Error: PrivacyError::SensitiveData("API key detected")
```

**Solution:**
```toml
[ai.privacy]
require_confirmation = false
log_requests = true  # Debug what's being sent
```

#### 5. Model Not Available

```
Error: AiError::ModelNotFound("gpt-5")
```

**Solution:**
```toml
[ai]
model = "gpt-4o-mini"  # Use available model
```

### Debug Mode

```bash
# Enable AI debug logging
export RUST_LOG=code_terminal::ai=debug

# Run with debug
codeterm --ai-debug

# Check logs
tail -f ~/.local/share/codeterm/logs/ai.log
```

### Verbose Logging

```toml
[ai.privacy]
log_requests = true

[ai.debug]
log_prompts = true
log_responses = true
log_timing = true
```

---

## 📖 API Examples

### Basic Usage

```rust
use code_terminal::ai::{AiConfig, OpenAIProvider, AIProvider, TerminalContext};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AiConfig::openai("sk-key")
        .with_model("gpt-4o-mini")
        .with_max_tokens(1024);
    
    let provider = OpenAIProvider::new(config)?;
    
    let context = TerminalContext::new()
        .with_shell("bash");
    
    let suggestion = provider
        .suggest_command(&context, "list all docker containers")
        .await?;
    
    println!("Command: {}", suggestion.command);
    Ok(())
}
```

### With Privacy Controls

```rust
let mut config = AiConfig::openai("sk-key");
config.privacy.send_file_paths = false;
config.privacy.send_shell_history = false;
config.privacy.require_confirmation = true;

let provider = OpenAIProvider::new(config)?;
```

### Error Handling

```rust
match provider.suggest_command(&context, "query").await {
    Ok(suggestion) => {
        println!("{}", suggestion.command);
    }
    Err(AiError::ApiError(msg)) => {
        eprintln!("API Error: {}", msg);
    }
    Err(AiError::Timeout(d)) => {
        eprintln!("Request timed out after {:?}", d);
    }
    Err(AiError::RateLimited { retry_after }) => {
        eprintln!("Rate limited. Retry in {:?}", retry_after);
    }
    Err(e) => {
        eprintln!("Error: {}", e);
    }
}
```

---

## 🔮 Future Enhancements

### Planned Features

- [ ] **Streaming Responses** - Real-time response streaming
- [ ] **Conversation Memory** - Multi-turn conversations
- [ ] **Local Models** - Ollama, llama.cpp integration
- [ ] **Custom Training** - Fine-tune on user's command history
- [ ] **Multi-Modal** - Image input for error screenshots
- [ ] **Command History Learning** - Learn from user patterns
- [ ] **Contextual Awareness** - Git, Docker, Kubernetes context
- [ ] **Voice Commands** - Speech-to-command integration

### Experimental Features

```toml
[ai.experimental]
streaming = true
conversation_memory = true
local_fallback = true
auto_execute_safe_commands = false
```

---

## 📚 References

### Documentation

- [OpenAI API Reference](https://platform.openai.com/docs/api-reference)
- [Anthropic API Reference](https://docs.anthropic.com/claude/reference)
- [Ollama Documentation](https://ollama.ai/docs)

### Related Tasks

| Task | Description | Status |
|------|-------------|--------|
| TASK-122 | AI provider abstraction | ✅ Complete |
| TASK-123 | OpenAI integration | ✅ Complete |
| TASK-124 | Command suggestions | ✅ Complete |
| TASK-125 | Error explanation | ✅ Complete |
| TASK-126 | Privacy controls | ✅ Complete |
| TASK-127 | AI configuration | ✅ Complete |
| TASK-128 | AI tests | ✅ Complete |
| TASK-129 | AI documentation | ✅ Complete |

---

## 🤝 Contributing

When contributing to AI features:

1. **Test thoroughly** - AI features affect user experience significantly
2. **Consider privacy** - Always respect privacy settings
3. **Handle errors gracefully** - AI APIs can fail in many ways
4. **Document prompts** - Keep system prompts maintainable
5. **Add tests** - Cover new functionality with tests

---

**Maintained by:** Development Team  
**Last Updated:** Março 2026  
**Sprint:** 16 - AI Integration

## Assistente no terminal (Ctrl+Shift+A / Ctrl+Shift+K)

- **Ctrl+Shift+A** (ou "AI: Explain Last Output" na paleta) manda as últimas
  80 linhas do painel em foco e mostra a explicação: o que aconteceu, a
  causa provável e o próximo passo.
- **Ctrl+Shift+K** (ou "AI: Suggest Command") abre um campo para descrever o
  que você quer; a resposta traz um comando, e Enter o insere no terminal
  **sem executar**, para você revisar.

Antes de enviar, tokens com cara de segredo (`ghp_…`, `glpat-…`, `sk-…`,
`AKIA…`, `password=…`) viram `[redacted]` e a home vira `~`. A chamada roda em
segundo plano; a interface não trava.

```toml
[ai]
enabled = true
provider = "anthropic"          # ou "openai", "ollama", "local", "custom"

[ai.model]
name = "claude-opus-5-5"        # padrão da Anthropic
api_key_env = "ANTHROPIC_API_KEY"
# base_url = "http://localhost:11434/api"   # Ollama, proxy, etc.
```

Com a Anthropic o pedido usa a Messages API com esforço baixo (resposta
rápida) e o fallback do servidor para quando o modelo recusa.
