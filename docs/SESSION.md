# Sistema de Sessões do Codeterm

Documentação completa do sistema de persistência e restauração de sessões do terminal.

## Visão Geral

O sistema de sessões permite salvar e restaurar o estado completo do terminal, incluindo:

- **Layout de abas e splits**: Configuração visual das abas e divisões
- **Diretórios de trabalho**: Caminhos ativos em cada aba
- **Scroll position**: Posição de scroll em cada aba
- **Títulos**: Nomes das abas
- **Histórico de comandos**: (quando habilitado)

## Arquitetura

```
session/
├── mod.rs           # Tipos principais e exports
├── error.rs         # Erros do sistema de sessão
├── manager.rs       # Gerenciador de sessões (CRUD)
├── serialization.rs # Estruturas de dados serializáveis
├── restore.rs       # Captura e restauração de estado
├── migration.rs     # Migração entre versões
├── auto_save.rs     # Auto-save periódico
└── backup.rs        # Gerenciamento avançado de backups
```

## Fluxo de Dados

```
AppState
    │
    ▼
SessionRestorer::capture()
    │
    ▼
SessionData (serialização)
    │
    ▼
SessionManager::save() → JSON no disco
    │
    ▼
SessionManager::load() ← JSON do disco
    │
    ▼
SessionRestorer::restore()
    │
    ▼
AppState (restaurado)
```

## Uso Básico

### Configuração

```rust
use code_terminal::session::{SessionManager, SessionConfig};

// Configuração padrão
let config = SessionConfig::default();
let manager = SessionManager::new(config);

// Configuração personalizada
let config = SessionConfig::with_directory("/caminho/customizado");
```

### Salvar Sessão

```rust
use code_terminal::session::SessionRestorer;
use code_terminal::app::AppState;

// Captura e salva o estado atual
let app_state: AppState = /* obter estado */;
SessionRestorer::capture(&app_state, "minha-sessao", &manager)?;
```

### Restaurar Sessão

```rust
// Restaura uma sessão salva
let mut app_state = AppState::new(80, 24)?;
SessionRestorer::restore(&mut app_state, "minha-sessao", &manager)?;
```

### Sessão Padrão

```rust
// Salvar como sessão padrão (inicialização automática)
SessionRestorer::save_default_session(&app_state, &manager)?;

// Tentar restaurar sessão padrão na inicialização
let restored = SessionRestorer::try_restore_last_session(&mut app_state, &manager)?;
if restored {
    println!("Sessão anterior restaurada!");
}
```

## Auto-Save

O auto-save salva automaticamente o estado do terminal em intervalos regulares ou em eventos específicos.

### Configuração

```rust
use code_terminal::session::{AutoSaveManager, AutoSaveConfig};
use std::time::Duration;

// Configuração padrão (5 minutos, eventos habilitados)
let config = AutoSaveConfig::default();

// Configuração personalizada
let config = AutoSaveConfig {
    enabled: true,
    interval: Duration::from_secs(60), // 1 minuto
    on_tab_created: true,
    on_tab_closed: true,
    on_layout_changed: true,
    ..Default::default()
};

// Apenas timer (sem eventos)
let config = AutoSaveConfig::timer_only();

// Todos os eventos
let config = AutoSaveConfig::all_events();
```

### Uso

```rust
use code_terminal::session::AutoSaveEvent;

let auto_save = AutoSaveManager::new(config, manager);

// Iniciar com callback de salvamento
let handle = auto_save.start(|name, manager| async move {
    SessionRestorer::capture(&app_state, name, manager)
}).expect("Auto-save iniciado");

// Notificar eventos
handle.notify_event(AutoSaveEvent::TabCreated);
handle.notify_event(AutoSaveEvent::TabClosed);
handle.notify_event(AutoSaveEvent::LayoutChanged);

// Pausar/Resumir
handle.pause();
handle.resume();

// Forçar salvamento
handle.force_save();

// Parar no shutdown
handle.shutdown().await;
```

### Eventos Suportados

| Evento | Descrição | Padrão |
|--------|-----------|--------|
| `TimerTick` | Intervalo periódico | ✅ |
| `TabCreated` | Nova aba criada | ✅ |
| `TabClosed` | Aba fechada | ✅ |
| `LayoutChanged` | Split criado/removido | ✅ |
| `SshSessionChanged` | Conexão SSH alterada | ✅ |
| `TabTitleChanged` | Título da aba alterado | ❌ |
| `WorkingDirectoryChanged` | Diretório alterado | ❌ |
| `TerminalResized` | Terminal redimensionado | ❌ |
| `Shutdown` | Aplicação fechando | ✅ |

## Gerenciamento de Sessões

### Operações CRUD

```rust
// Salvar
manager.save("nome", &session_data)?;

// Carregar
let session = manager.load("nome")?;

// Deletar
manager.delete("nome")?;

// Renomear
manager.rename("antigo", "novo")?;

// Listar
let sessions = manager.list_sessions()?;
for session in sessions {
    println!("{} - {}", session.name, session.modified_at);
}
```

## Sistema Avançado de Backup

O módulo `backup` fornece funcionalidades completas para gerenciamento de backups de sessões.

### BackupManager

```rust
use code_terminal::session::{BackupManager, BackupConfig, BackupOptions, BackupRetentionPolicy};

// Configuração padrão
let config = BackupConfig::default();
let backup_manager = BackupManager::new(config);
backup_manager.initialize()?;

// Configuração personalizada
let config = BackupConfig::with_directory("/path/to/backups")
    .with_retention_policy(BackupRetentionPolicy::KeepCount(10));
let backup_manager = BackupManager::new(config);
```

### Criando Backups

```rust
// Backup simples
let data = SessionData::new();
let backup = backup_manager.backup("my-session", &data)?;

// Backup com opções
let options = BackupOptions::with_name("pre-release")
    .with_description("Backup antes da release v1.0")
    .with_compression();

let backup = backup_manager.create_backup("my-session", &data, options)?;
```

### Políticas de Retenção

```rust
use code_terminal::session::BackupRetentionPolicy;

// Manter apenas os últimos N backups
let policy = BackupRetentionPolicy::KeepCount(5);

// Manter backups por um período
let policy = BackupRetentionPolicy::KeepDuration(
    chrono::Duration::days(30)
);

// Combinação de ambos
let policy = BackupRetentionPolicy::KeepCountAndDuration(
    10,                         // Máximo 10 backups
    chrono::Duration::days(7)   // Mantém por 7 dias
);

// Manter todos os backups
let policy = BackupRetentionPolicy::KeepAll;
```

### Gerenciando Backups

```rust
// Listar backups de uma sessão
let backups = backup_manager.list_backups("my-session")?;
for backup in backups {
    println!("{} - {} bytes - {}",
        backup.metadata.backup_name,
        backup.metadata.size,
        backup.metadata.created_at
    );
}

// Obter backup mais recente
if let Some(latest) = backup_manager.get_latest_backup("my-session")? {
    println!("Último backup: {:?}", latest.path);
}

// Restaurar backup
let restored_data = backup_manager.restore_backup(&backups[0])?;

// Deletar backup específico
backup_manager.delete_backup(&backups[0])?;

// Deletar todos os backups de uma sessão
let count = backup_manager.delete_all_backups("my-session")?;
println!("{} backups deletados", count);
```

### Estatísticas de Backup

```rust
let stats = backup_manager.stats()?;
println!("Total de backups: {}", stats.total_backups);
println!("Sessões com backup: {}", stats.total_sessions);
println!("Espaço total: {} bytes", stats.total_size);

for (session, size) in &stats.size_per_session {
    println!("  {}: {} bytes", session, size);
}
```

### Backup Automático

Cada vez que uma sessão é salva, um backup é criado automaticamente:

```rust
// Backup automático (até 5 versões)
manager.save("minha-sessao", &data)?; // Cria backup da versão anterior
```

### Validação de Nomes

```rust
// Nomes válidos
manager.save("sessao-1", &data)?;
manager.save("minha_sessao", &data)?;
manager.save("sessao.v2", &data)?;

// Nomes inválidos (retornam erro)
manager.save("", &data)?;           // Vazio
manager.save("com/espacos", &data)?; // Contém /
manager.save("a..b", &data)?;        // Pontos consecutivos
```

## Migração de Versões

O formato de sessão pode evoluir. O sistema de migração permite atualizar sessões antigas.

```rust
use code_terminal::session::{VersionManager, VersionInfo};

let version_manager = VersionManager::new();

// Verificar necessidade de migração
if let Some(info) = version_manager.needs_migration(&session) {
    println!("Migração necessária: {} -> {}", info.from, info.to);
    
    // Migrar para versão mais recente
    let migrated = version_manager.migrate(session)?;
}

// Migração automática ao carregar
let session = manager.load_and_migrate("nome", &version_manager)?;
```

## Formato de Arquivo

As sessões são salvas como JSON no diretório de sessões:

```json
{
  "metadata": {
    "name": "minha-sessao",
    "created_at": "2026-03-13T18:00:00Z",
    "modified_at": "2026-03-13T18:30:00Z",
    "version": 1,
    "description": "Sessão de trabalho"
  },
  "data": {
    "version": 1,
    "config": {
      "terminal_size": [80, 24],
      "working_directory": "/home/user/projetos"
    },
    "layout": {
      "panes": [
        {
          "id": 1,
          "tabs": [
            {
              "id": 1,
              "title": "bash",
              "working_directory": "/home/user",
              "scroll_position": 0
            }
          ],
          "active_tab": 1
        }
      ],
      "active_pane": 1
    }
  }
}
```

## Integração com AppState

### Captura Completa

```rust
impl SessionSnapshot for AppState {
    fn capture(&self) -> SessionData {
        SessionRestorer::capture_state(self)
    }
    
    fn restore(&mut self, data: &SessionData) -> SessionResult<()> {
        SessionRestorer::restore_state(self, data)
    }
}
```

### Ciclo de Vida

```rust
// Inicialização
fn on_startup() {
    let manager = SessionManager::default();
    
    // Tentar restaurar sessão anterior
    if let Ok(true) = SessionRestorer::try_restore_last_session(&mut app_state, &manager) {
        info!("Sessão anterior restaurada");
    }
    
    // Iniciar auto-save
    let auto_save = AutoSaveManager::with_default_config(manager);
    let handle = auto_save.start(callback);
}

// Encerramento
fn on_shutdown(handle: AutoSaveHandle) {
    // Forçar salvamento final
    handle.force_save();
    handle.shutdown().await;
}
```

## Configuração em TOML

```toml
[session]
# Diretório de sessões
sessions_dir = "~/.config/codeterm/sessions"

# Sessão padrão
default_session = "default"

# Número máximo de backups
max_backups = 5

# Comprimir sessões
compression = false

# Incluir histórico de comandos
include_command_history = true

[session.auto_save]
# Habilitar auto-save
enabled = true

# Intervalo em segundos
interval = 300

# Eventos que disparam auto-save
on_tab_created = true
on_tab_closed = true
on_layout_changed = true
on_ssh_changed = true
on_title_changed = false
on_working_dir_changed = false
on_resize = false

# Debounce entre salvamentos
debounce_seconds = 2

# Máximo de sessões de auto-save
max_auto_save_sessions = 3
```

## Erros Comuns

### Sessão Não Encontrada

```rust
match manager.load("sessao-inexistente") {
    Ok(session) => { /* usar sessão */ },
    Err(SessionError::NotFound(name)) => {
        println!("Sessão '{}' não encontrada", name);
    }
    Err(e) => { /* outros erros */ }
}
```

### Versão Incompatível

```rust
match manager.load("sessao-antiga") {
    Ok(session) => { /* usar sessão */ },
    Err(SessionError::UnsupportedVersion { version }) => {
        println!("Formato de versão {} não suportado", version);
        // Tentar migrar
        let migrated = version_manager.migrate(session)?;
    }
    Err(e) => { /* outros erros */ }
}
```

### Erro de IO

```rust
match manager.save("sessao", &data) {
    Ok(()) => { /* sucesso */ },
    Err(SessionError::Io(e)) => {
        eprintln!("Erro de disco: {}", e);
    }
    Err(e) => { /* outros erros */ }
}
```

## Performance

### Otimizações

- **Lazy loading**: Sessões são carregadas apenas quando necessário
- **Debouncing**: Evita salvamentos excessivos em eventos rápidos
- **Cleanup automático**: Remove sessões de auto-save antigas
- **Backup incremental**: Mantém apenas as últimas 5 versões

### Métricas

```rust
// Estatísticas do auto-save
let stats = auto_save.stats().await;
println!("Salvamentos: {}", stats.save_count);
println!("Erros consecutivos: {}", stats.consecutive_errors);
```

## Exemplos

### Exemplo 1: Sessão de Trabalho

```rust
use code_terminal::session::{SessionManager, SessionRestorer};

fn save_work_session(app_state: &AppState) -> Result<(), Box<dyn std::error::Error>> {
    let manager = SessionManager::default();
    
    // Salvar com timestamp
    let name = format!("trabalho-{}", chrono::Local::now().format("%Y%m%d"));
    SessionRestorer::capture(app_state, &name, &manager)?;
    
    println!("Sessão salva: {}", name);
    Ok(())
}
```

### Exemplo 2: Auto-save Robusto

```rust
use code_terminal::session::{
    AutoSaveManager, AutoSaveConfig, AutoSaveEvent
};

fn setup_robust_auto_save(
    app_state: AppState,
    manager: SessionManager,
) -> Option<AutoSaveHandle> {
    let config = AutoSaveConfig {
        enabled: true,
        interval: Duration::from_secs(60),
        on_tab_created: true,
        on_tab_closed: true,
        on_layout_changed: true,
        on_ssh_changed: true,
        debounce_duration: Duration::from_secs(5),
        max_auto_save_sessions: 5,
        ..Default::default()
    };
    
    let auto_save = AutoSaveManager::new(config, manager);
    
    auto_save.start(|name, manager| async move {
        // Callback que captura estado
        SessionRestorer::capture(&app_state, name, manager)
    })
}
```

### Exemplo 3: Gerenciador de Projetos

```rust
use code_terminal::session::{SessionManager, SessionRestorer};
use std::collections::HashMap;

struct ProjectManager {
    manager: SessionManager,
    projects: HashMap<String, String>, // nome -> diretório
}

impl ProjectManager {
    fn save_project(&self, name: &str, app_state: &AppState) -> SessionResult<()> {
        SessionRestorer::capture(app_state, &format!("proj-{}", name), &self.manager)
    }
    
    fn load_project(&self, name: &str, app_state: &mut AppState) -> SessionResult<()> {
        SessionRestorer::restore(app_state, &format!("proj-{}", name), &self.manager)
    }
    
    fn list_projects(&self) -> SessionResult<Vec<String>> {
        let all = self.manager.list_sessions()?;
        Ok(all
            .into_iter()
            .filter(|s| s.name.starts_with("proj-"))
            .map(|s| s.name.trim_start_matches("proj-").to_string())
            .collect())
    }
}
```

## Referência da API

### SessionManager

```rust
impl SessionManager {
    pub fn new(config: SessionConfig) -> Self;
    pub fn initialize(&self) -> SessionResult<()>;
    pub fn save(&self, name: impl AsRef<str>, data: &SessionData) -> SessionResult<()>;
    pub fn load(&self, name: impl AsRef<str>) -> SessionResult<Session>;
    pub fn delete(&self, name: impl AsRef<str>) -> SessionResult<()>;
    pub fn rename(&self, old: impl AsRef<str>, new: impl AsRef<str>) -> SessionResult<()>;
    pub fn list_sessions(&self) -> SessionResult<Vec<SessionInfo>>;
    pub fn exists(&self, name: impl AsRef<str>) -> SessionResult<bool>;
    pub fn create_backup(&self, name: impl AsRef<str>) -> SessionResult<PathBuf>;
}
```

### SessionRestorer

```rust
impl SessionRestorer {
    pub fn capture(app_state: &AppState, name: impl AsRef<str>, manager: &SessionManager) -> SessionResult<()>;
    pub fn restore(app_state: &mut AppState, name: impl AsRef<str>, manager: &SessionManager) -> SessionResult<()>;
    pub fn capture_state(app_state: &AppState) -> SessionData;
    pub fn restore_state(app_state: &mut AppState, data: &SessionData) -> SessionResult<()>;
    pub fn try_restore_last_session(app_state: &mut AppState, manager: &SessionManager) -> SessionResult<bool>;
    pub fn save_default_session(app_state: &AppState, manager: &SessionManager) -> SessionResult<()>;
}
```

### BackupManager

```rust
impl BackupManager {
    pub fn new(config: BackupConfig) -> Self;
    pub fn initialize(&self) -> SessionResult<()>;
    pub fn backup(&self, session_name: impl AsRef<str>, session_data: &SessionData) -> SessionResult<BackupInfo>;
    pub fn create_backup(&self, session_name: impl AsRef<str>, session_data: &SessionData, options: BackupOptions) -> SessionResult<BackupInfo>;
    pub fn list_backups(&self, session_name: impl AsRef<str>) -> SessionResult<Vec<BackupInfo>>;
    pub fn list_all_backups(&self) -> SessionResult<Vec<BackupInfo>>;
    pub fn restore_backup(&self, backup: &BackupInfo) -> SessionResult<SessionData>;
    pub fn delete_backup(&self, backup: &BackupInfo) -> SessionResult<()>;
    pub fn delete_all_backups(&self, session_name: impl AsRef<str>) -> SessionResult<usize>;
    pub fn has_backup(&self, session_name: impl AsRef<str>) -> SessionResult<bool>;
    pub fn get_latest_backup(&self, session_name: impl AsRef<str>) -> SessionResult<Option<BackupInfo>>;
    pub fn get_oldest_backup(&self, session_name: impl AsRef<str>) -> SessionResult<Option<BackupInfo>>;
    pub fn stats(&self) -> SessionResult<BackupStats>;
    pub fn export_backup(&self, backup: &BackupInfo, destination: impl AsRef<Path>) -> SessionResult<PathBuf>;
    pub fn import_backup(&self, source: impl AsRef<Path>, options: BackupOptions) -> SessionResult<BackupInfo>;
}
```

### AutoSaveManager

```rust
impl AutoSaveManager {
    pub fn new(config: AutoSaveConfig, session_manager: SessionManager) -> Self;
    pub fn with_default_config(session_manager: SessionManager) -> Self;
    pub fn start<F, Fut>(self, save_callback: F) -> Option<AutoSaveHandle>
    where
        F: Fn(&str, &SessionManager) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = SessionResult<PathBuf>> + Send + 'static;
    pub fn has_auto_save_session(&self) -> SessionResult<bool>;
}
```

### AutoSaveHandle

```rust
impl AutoSaveHandle {
    pub fn notify_event(&self, event: AutoSaveEvent);
    pub fn force_save(&self);
    pub fn pause(&self);
    pub fn resume(&self);
    pub async fn shutdown(self);
    pub fn shutdown_now(&self);
}
```

## Changelog

### Versão 1.0

- ✨ Suporte a salvamento e restauração de sessões
- ✨ Gerenciamento de múltiplas sessões
- ✨ Auto-save com timer e eventos
- ✨ Backup automático
- ✨ Migração de versões
- ✨ Validação de nomes
- ✨ Configuração via TOML
