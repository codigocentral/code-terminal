# 🔐 SSH Manager

Documentação completa do gerenciador de conexões SSH do Codeterm.

## Visão Geral

O módulo SSH (`src/ssh/`) implementa um gerenciador completo de conexões SSH com as seguintes funcionalidades:

- **Parser de configuração**: Parsing completo de `~/.ssh/config`
- **Gerenciamento de hosts**: Hosts com suporte a patterns, includes e herança
- **Gerenciamento de chaves**: Discovery, geração e integração com SSH agent
- **Quick Connect UI**: Interface rápida para conexões SSH
- **Persistência de sessões**: Salvamento e restauração de sessões
- **Storage seguro**: Armazenamento criptografado de chaves

## Arquitetura

```
┌─────────────────────────────────────────────────────────────┐
│                      SSH Module                              │
├─────────────┬─────────────┬─────────────┬────────────────────┤
│   Config    │    Host     │    Key      │     Session        │
│   Parser    │ Management  │ Management  │   Persistence      │
├─────────────┼─────────────┼─────────────┼────────────────────┤
│ SshConfig   │  SshHost    │  KeyManager │  SessionManager    │
│   Parser    │   PortForward│ SshKey     │   SshSession       │
│ SshPattern  │DynamicForward│PublicKey   │   SessionId        │
└─────────────┴─────────────┴─────────────┴────────────────────┘
                              │
                    ┌─────────┴──────────┐
                    │   Quick Connect UI  │
                    │  QuickConnectDialog │
                    └─────────────────────┘
```

## Componentes

### 1. Config Parser (`config_parser.rs`)

Parser completo para arquivos de configuração SSH.

```rust
use code_terminal::ssh::SshConfigParser;

// Parse do arquivo ~/.ssh/config padrão
let parser = SshConfigParser::parse_default_config()?;

// Ou parse de um arquivo específico
let parser = SshConfigParser::parse_file("/path/to/config")?;

// Buscar configuração efetiva para um host
if let Some(host) = parser.get_effective_config("myserver") {
    println!("Host: {}", host.host_name);
    println!("User: {}", host.user);
    println!("Port: {}", host.port);
}
```

#### Diretivas Suportadas

| Diretiva | Descrição | Exemplo |
|----------|-----------|---------|
| `Host` | Define um padrão de host | `Host *.example.com` |
| `HostName` | Nome real do host | `HostName 192.168.1.100` |
| `User` | Usuário para conexão | `User admin` |
| `Port` | Porta SSH | `Port 2222` |
| `IdentityFile` | Arquivo de chave privada | `IdentityFile ~/.ssh/id_rsa` |
| `IdentityAgent` | Agente SSH personalizado | `IdentityAgent ~/.ssh/agent` |
| `ForwardAgent` | Forwarding do agente | `ForwardAgent yes` |
| `LocalForward` | Port forwarding local | `LocalForward 8080 localhost:80` |
| `RemoteForward` | Port forwarding remoto | `RemoteForward 9090 localhost:3000` |
| `DynamicForward` | SOCKS proxy | `DynamicForward 1080` |
| `ServerAliveInterval` | Keepalive interval | `ServerAliveInterval 60` |
| `ServerAliveCountMax` | Max keepalive failures | `ServerAliveCountMax 3` |
| `StrictHostKeyChecking` | Verificação de host key | `StrictHostKeyChecking ask` |
| `ProxyJump` | Jump host | `ProxyJump bastion` |
| `Include` | Incluir outros arquivos | `Include ~/.ssh/config.d/*` |

### 2. Host Management (`host.rs`)

Gerenciamento de hosts SSH com suporte a herança e patterns.

```rust
use code_terminal::ssh::SshHost;

// Criar um novo host
let host = SshHost::new("myserver")
    .with_hostname("192.168.1.100")
    .with_user("admin")
    .with_port(2222)
    .with_identity_file("~/.ssh/id_ed25519");

// String de conexão
println!("{}", host.connection_string()); // admin@192.168.1.100:2222

// Gerar comando SSH
let args = host.to_ssh_args();
```

#### Port Forwarding

```rust
use code_terminal::ssh::{PortForward, DynamicForward};

// Local forwarding
let local = PortForward::local(8080, "localhost", 80);

// Remote forwarding
let remote = PortForward::remote(9090, "localhost", 3000);

// Dynamic forwarding (SOCKS)
let dynamic = DynamicForward::new(1080);
```

### 3. Key Management (`key_manager.rs`)

Gerenciamento completo de chaves SSH.

```rust
use code_terminal::ssh::{KeyManager, KeyGenerationOptions, KeyType};

// Criar gerenciador
let mut manager = KeyManager::new();

// Descobrir chaves existentes
let keys = manager.discover_keys()?;
for key in keys {
    println!("Chave: {} ({})", key.name, key.key_type);
}

// Gerar nova chave
let options = KeyGenerationOptions::new(KeyType::Ed25519)
    .with_comment("user@example.com".to_string());

let new_key = manager.generate_key(options)?;

// Adicionar ao agent
manager.add_to_agent(&new_key, Some("passphrase"))?;

// Listar chaves no agent
let agent_keys = manager.list_agent_keys()?;
```

#### Tipos de Chave Suportados

| Tipo | Bits Padrão | Uso Recomendado |
|------|-------------|-----------------|
| `Ed25519` | 256 bits | **Recomendado** - Moderno e seguro |
| `RSA` | 4096 bits | Compatibilidade legada |
| `Ecdsa` | 256 bits | Ambientes específicos |
| `Dsa` | 1024 bits | ⚠️ Obsoleto, evitar |

#### Validação de Permissões

```rust
// Validar permissões Unix (0o600 para privadas, 0o644 para públicas)
if let Err(e) = manager.validate_key_permissions(&key) {
    // Corrigir automaticamente
    manager.fix_key_permissions(&key)?;
}
```

### 4. Quick Connect UI (`quick_connect.rs`)

Interface rápida para conexões SSH com fuzzy search.

```rust
use code_terminal::ssh::{QuickConnectDialog, QuickConnectParams};

// Criar diálogo de conexão rápida
let dialog = QuickConnectDialog::new()
    .with_recent_connections(vec![
        "user@server1".to_string(),
        "admin@192.168.1.100".to_string(),
    ]);

// Parse de string de conexão
let params = QuickConnectParams::parse("user@host:2222").unwrap();
println!("User: {}, Host: {}, Port: {:?}", params.user, params.host, params.port);
```

#### Formatos Suportados

| Formato | Exemplo |
|---------|---------|
| Simples | `host` |
| Com usuário | `user@host` |
| Com porta | `host:2222` |
| Completo | `user@host:2222` |

### 5. Session Management (`session.rs`)

Gerenciamento e persistência de sessões SSH.

```rust
use code_terminal::ssh::{SessionManager, SshSession, SessionType};

// Criar gerenciador de sessões
let config = SessionPersistenceConfig::default()
    .with_auto_save(true)
    .with_max_sessions(10);

let mut manager = SessionManager::with_config(config);

// Criar nova sessão
let session = SshSession::new("minha-sessao", "server.com", "user");
let id = session.id;

// Iniciar sessão
manager.start_session(id, &host)?;

// Listar sessões ativas
for session in manager.list_active_sessions() {
    println!("{}: {} - {:?}", session.id, session.name, session.state);
}

// Persistir sessões
manager.save_to_disk()?;
```

#### Estados de Sessão

```rust
pub enum SessionState {
    Connecting,
    Connected,
    Disconnected,
    Error,
}
```

#### Tipos de Sessão

```rust
pub enum SessionType {
    Interactive,  // Shell interativo
    Command,      // Execução de comando único
    Tunnel,       // Port forwarding
    Sftp,         // Transferência de arquivos
}
```

### 6. Secure Storage (`storage.rs`)

Armazenamento criptografado para chaves e configurações.

```rust
use code_terminal::ssh::{KeyStorage, EncryptedStorage};

// Storage criptografado
let storage = EncryptedStorage::new()?;

// Armazenar chave com senha
storage.store_key(&key, Some("master_password"))?;

// Recuperar chave
let key = storage.retrieve_key(&key_name, Some("master_password"))?;
```

### 7. Security Validator (`security.rs`)

Validações e auditoria de segurança para chaves e configurações SSH.

```rust
use code_terminal::ssh::{KeyManager, security::SecurityValidator};

let manager = KeyManager::new();
let validator = SecurityValidator::new();

// Validar chaves
let keys = manager.discover_keys()?;
for key in keys {
    let report = validator.validate_key(&key);
    println!("Score de segurança: {}/100", report.score);
    
    for issue in &report.issues {
        println!("[{}] {}: {}", issue.severity, issue.code, issue.message);
    }
}

// Validar configuração de host
if let Some(host) = parser.get_effective_config("myserver") {
    let report = validator.validate_host(&host);
    if report.has_critical() {
        println!("⚠️  Problemas críticos encontrados!");
    }
}
```

#### Algoritmos Recomendados

| Categoria | Algoritmos |
|-----------|------------|
| **KEX** | curve25519-sha256, ecdh-sha2-nistp521 |
| **Host Key** | ssh-ed25519, ecdsa-sha2-nistp521, rsa-sha2-512 |
| **Ciphers** | aes256-gcm@openssh.com, chacha20-poly1305 |
| **MACs** | hmac-sha2-512-etm@openssh.com |

#### Auditoria de Segurança

```rust
use code_terminal::ssh::security::SecurityAuditor;

let auditor = SecurityAuditor::new();
let report = auditor_audit(Path::new("~/.ssh"))?;

println!("Score geral: {}/100", report.overall_score);

if !report.passed() {
    println!("Atenção: Problemas de segurança encontrados!");
}

for issue in &report.config_issues {
    println!("🔧 {}", issue);
}
```

## Exemplos Completos

### Exemplo 1: Conexão Rápida

```rust
use code_terminal::ssh::{SshConfigParser, SshHost};

fn quick_connect(host_pattern: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Parse da configuração
    let parser = SshConfigParser::parse_default_config()?;
    
    // Buscar host
    if let Some(host) = parser.get_effective_config(host_pattern) {
        // Gerar comando SSH
        let args = host.to_ssh_args();
        println!("ssh {}", args.join(" "));
        
        // Ou executar diretamente
        // std::process::Command::new("ssh").args(&args).spawn()?;
    } else {
        println!("Host não encontrado: {}", host_pattern);
    }
    
    Ok(())
}
```

### Exemplo 2: Gerenciamento de Chaves

```rust
use code_terminal::ssh::{KeyManager, KeyGenerationOptions, KeyType};

fn rotate_ssh_keys() -> Result<(), Box<dyn std::error::Error>> {
    let mut manager = KeyManager::new();
    
    // Descobrir chaves existentes
    let keys = manager.discover_keys()?;
    
    for key in keys {
        // Verificar idade da chave
        if key.is_old(Duration::days(365)) {
            println!("Chave {} precisa ser rotacionada", key.name);
            
            // Gerar nova chave
            let new_key = manager.generate_key(
                KeyGenerationOptions::new(key.key_type.clone())
                    .with_filename(format!("{}_new", key.name))
                    .with_comment(key.comment.clone().unwrap_or_default())
            )?;
            
            println!("Nova chave gerada: {:?}", new_key.private_key_path);
        }
        
        // Validar permissões
        if let Err(_) = manager.validate_key_permissions(&key) {
            manager.fix_key_permissions(&key)?;
            println!("Permissões corrigidas para {}", key.name);
        }
    }
    
    Ok(())
}
```

### Exemplo 3: Port Forwarding

```rust
use code_terminal::ssh::{SshHost, PortForward, DynamicForward};

fn setup_tunnel() -> SshHost {
    SshHost::new("tunnel")
        .with_hostname("bastion.example.com")
        .with_user("tunnel")
        // Local forward: porta 8080 local -> 80 no servidor
        .with_local_forward(PortForward::local(8080, "web.internal", 80))
        // Remote forward: porta 9090 remoto -> 3000 local
        .with_remote_forward(PortForward::remote(9090, "localhost", 3000))
        // Dynamic forward: SOCKS proxy na porta 1080
        .with_dynamic_forward(DynamicForward::new(1080))
}
```

## Integração com Terminal

O módulo SSH se integra com o terminal através de comandos:

```rust
// Abrir nova aba com conexão SSH
terminal.open_ssh_tab("user@host", Some(2222));

// Quick connect com fuzzy finder
terminal.quick_connect_dialog();

// Conectar a host do ~/.ssh/config
terminal.connect_ssh_host("myserver");
```

## Configuração

### Variáveis de Ambiente

| Variável | Descrição | Padrão |
|----------|-----------|--------|
| `SSH_CONFIG` | Caminho do arquivo config | `~/.ssh/config` |
| `SSH_DIR` | Diretório de chaves SSH | `~/.ssh` |
| `SSH_AGENT_LAUNCHER` | Launcher do agente | `ssh-agent` |

### Arquivos de Configuração

```toml
# ~/.config/codeterm/ssh.toml
[ssh]
default_user = "admin"
default_port = 22
strict_host_key_checking = "ask"
forward_agent = false

[ssh.persistence]
enabled = true
max_sessions = 10
auto_save = true
save_interval = 300

[ssh.keys]
default_type = "ed25519"
default_bits = 256
auto_add_to_agent = true
```

## Segurança

### Boas Práticas

1. **Chaves Ed25519**: Prefira Ed25519 sobre RSA
2. **Passphrases**: Sempre use passphrases para chaves privadas
3. **Permissões**: Mantenha 0o600 para chaves privadas
4. **Agent Forwarding**: Use com cautela, apenas em hosts confiáveis
5. **Host Keys**: Verifique fingerprints em primeira conexão

### Validações Automáticas

- Permissões de arquivo Unix
- Passphrase em chaves privadas
- Algoritmos de chave seguros
- Host key verification
- Certificate validation

## Erros Comuns

| Erro | Causa | Solução |
|------|-------|---------|
| `SshDirNotFound` | Diretório ~/.ssh não existe | Criar diretório |
| `InvalidPermissions` | Permissões incorretas | `fix_key_permissions()` |
| `KeyEncrypted` | Chave requer passphrase | Fornecer passphrase |
| `AgentNotAvailable` | SSH agent não rodando | Iniciar ssh-agent |
| `ConfigNotFound` | Arquivo config não existe | Criar ou usar padrão |

## Referências

- [OpenSSH Config](https://man.openbsd.org/ssh_config)
- [SSH Agent](https://man.openbsd.org/ssh-agent)
- [SSH Key Management](https://www.ssh.com/academy/ssh/keygen)

## Veja Também

- [`src/ssh/config_parser.rs`](../src/ssh/config_parser.rs) - Implementação do parser
- [`src/ssh/key_manager.rs`](../src/ssh/key_manager.rs) - Gerenciamento de chaves
- [`src/ssh/session.rs`](../src/ssh/session.rs) - Persistência de sessões
