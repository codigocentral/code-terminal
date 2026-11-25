# 💾 Session Management

Documentação completa do sistema de persistência e restauração de sessões do Codeterm.

## Visão Geral

O módulo de sessões (`src/session/`) implementa um sistema completo de persistência do estado do terminal, permitindo salvar, restaurar e gerenciar sessões de trabalho.

### Funcionalidades

- **Persistência completa**: Salva abas, splits, diretórios de trabalho e estado
- **Restauração automática**: Restaura sessão ao iniciar o terminal
- **Múltiplas sessões**: Suporte a várias sessões nomeadas
- **Backup automático**: Mantém histórico de backups (padrão: 5)
- **Auto-save**: Salvamento periódico automático (opcional)
- **Formato JSON**: Arquivos legíveis e versionados

## Arquitetura

```
┌─────────────────────────────────────────────────────────────┐
│                   Session Management                         │
├─────────────┬─────────────┬─────────────┬────────────────────┤
│    Session  │  Layout     │   Manager   │     Restore        │
│    Data     │   Data      │             │                    │
├─────────────┼─────────────┼─────────────┼────────────────────┤
│ SessionData │  LayoutData │  save()     │  restore_session() │
│ PaneData    │  PaneData   │  load()     │  capture_state()   │
│ TabData     │  SplitData  │  list()     │  auto_restore()    │
│ Metadata    │             │  delete()   │                    │
└─────────────┴─────────────┴─────────────┴────────────────────┘
```

## Estruturas de Dados

### SessionData

Dados completos de uma sessão do terminal.

```rust
use code_terminal::session::SessionData;

// Criar nova sessão
let mut session = SessionData::new();
session.working_directory = Some(PathBuf::from("/home/user/projects"));
session.environment.insert("EDITOR".to_string(), "vim".to_string());
```

**Campos:**

| Campo | Tipo | Descrição |
|-------|------|-----------|
| `version` | `u32` | Versão do formato (atual: 1) |
| `layout` | `LayoutData` | Layout de panes e splits |
| `working_directory` | `Option<PathBuf>` | Diretório de trabalho padrão |
| `environment` | `HashMap<String, String>` | Variáveis de ambiente |

### LayoutData

Representa o layout completo do terminal com múltiplos panes.

```rust
use code_terminal::session::{LayoutData, PaneData};

// Criar layout com um pane
let mut layout = LayoutData::default();
layout.active_pane = 1;
layout.panes.push(PaneData {
    id: 1,
    rect: Rect { x: 0, y: 0, width: 800, height: 600 },
    tabs: vec![],
    active_tab: 1,
});
```

**Campos:**

| Campo | Tipo | Descrição |
|-------|------|-----------|
| `active_pane` | `u32` | ID do pane ativo |
| `panes` | `Vec<PaneData>` | Lista de panes |

### PaneData

Representa um pane (área do terminal) com suas abas.

```rust
use code_terminal::session::PaneData;

let pane = PaneData {
    id: 1,
    rect: Rect { x: 0, y: 0, width: 800, height: 600 },
    tabs: vec![],  // TabData items
    active_tab: 1,
};
```

### TabData

Representa uma aba (tab) dentro de um pane.

```rust
use code_terminal::session::TabData;

let tab = TabData {
    id: 1,
    title: "bash".to_string(),
    working_directory: Some(PathBuf::from("/home/user")),
    command: Some("vim main.rs".to_string()),
    scroll_position: 0,
    state: TabState::Active,
};
```

**Campos:**

| Campo | Tipo | Descrição |
|-------|------|-----------|
| `id` | `u32` | Identificador único da tab |
| `title` | `String` | Título exibido na aba |
| `working_directory` | `Option<PathBuf>` | Diretório de trabalho |
| `command` | `Option<String>` | Comando em execução |
| `scroll_position` | `u32` | Posição do scroll |
| `state` | `TabState` | Estado (Active, Inactive, Closed) |

### SplitData

Representa a divisão de panes (horizontal ou vertical).

```rust
use code_terminal::session::{SplitData, SplitDirection, SplitChild};

let split = SplitData {
    direction: SplitDirection::Horizontal,
    children: vec![
        SplitChild::Pane(1),
        SplitChild::Pane(2),
    ],
};
```

### SessionMetadata

Metadados de uma sessão salva.

```rust
use code_terminal::session::SessionMetadata;

let metadata = SessionMetadata {
    name: "my-project".to_string(),
    created_at: chrono::Utc::now(),
    modified_at: chrono::Utc::now(),
    version: 1,
    description: Some("Sessão do projeto principal".to_string()),
};
```

## SessionManager

O gerenciador de sessões é o ponto principal de interação com o sistema.

### Criação e Inicialização

```rust
use code_terminal::session::{SessionManager, SessionConfig};

// Configuração padrão
let config = SessionConfig::default();
let manager = SessionManager::new(config)?;
manager.initialize()?;

// Configuração customizada
let config = SessionConfig {
    sessions_dir: PathBuf::from("/custom/path/sessions"),
    default_session_name: "default".to_string(),
    max_backups: 3,
    compression: false,
    include_command_history: true,
};
```

### Salvando Sessões

```rust
use code_terminal::session::SessionData;

// Criar dados da sessão
let mut data = SessionData::new();
data.working_directory = Some(PathBuf::from("/home/user/projects"));

// Adicionar pane com tabs
let pane = PaneData {
    id: 1,
    rect: Rect { x: 0, y: 0, width: 800, height: 600 },
    tabs: vec![
        TabData {
            id: 1,
            title: "bash".to_string(),
            working_directory: Some(PathBuf::from("/home/user")),
            command: None,
            scroll_position: 0,
            state: TabState::Active,
        },
    ],
    active_tab: 1,
};
data.layout.panes.push(pane);

// Salvar sessão
manager.save("my-project", &data)?;
```

### Carregando Sessões

```rust
// Carregar sessão específica
let session = manager.load("my-project")?;
println!("Sessão: {}", session.metadata.name);
println!("Criada em: {}", session.metadata.created_at);

// Acessar dados
for pane in &session.data.layout.panes {
    for tab in &pane.tabs {
        println!("Tab {}: {}", tab.id, tab.title);
    }
}
```

### Listando Sessões

```rust
// Listar todas as sessões
for info in manager.list_sessions()? {
    println!("{} - modificado: {}", info.name, info.modified_at);
    if let Some(desc) = &info.description {
        println!("  Descrição: {}", desc);
    }
}
```

### Gerenciamento de Sessões

```rust
// Verificar se sessão existe
if manager.exists("my-project")? {
    println!("Sessão existe!");
}

// Renomear sessão
manager.rename("my-project", "my-project-v2")?;

// Deletar sessão
manager.delete("my-project-v2")?;

// Carregar sessão padrão
let default = manager.load_default()?;
```

## Sessão Padrão

O Codeterm automaticamente gerencia uma sessão padrão chamada "default".

```rust
// Carregar ou criar sessão padrão
let session = manager.load_default()?;

// A sessão padrão é criada automaticamente se não existir
// com um único pane e tab no diretório home
```

## Restauração de Sessão

O módulo `restore` fornece funcionalidades para capturar e restaurar o estado atual do terminal.

### Captura de Estado

```rust
use code_terminal::session::{SessionRestorer, AppState};

// Capturar estado atual
let restorer = SessionRestorer::new();
let session_data = restorer.capture_state(&app_state)?;

// Salvar estado capturado
manager.save("current", &session_data)?;
```

### Restauração Automática

```rust
use code_terminal::session::AutoRestore;

// Configurar restauração automática
let auto_restore = AutoRestore::new(manager.clone());

// Restaurar sessão padrão ao iniciar
if let Some(session) = auto_restore.restore_default()? {
    println!("Sessão restaurada: {}", session.metadata.name);
}

// Ou restaurar última sessão ativa
if let Some(session) = auto_restore.restore_last_active()? {
    println!("Última sessão restaurada");
}
```

## Backup e Auto-save

### Sistema de Backup

O sistema mantém automaticamente backups das sessões (padrão: 5 backups).

```rust
// Backup é criado automaticamente antes de sobrescrever
manager.save("my-project", &data)?;  // Cria backup da versão anterior

// Configurar número de backups
let config = SessionConfig {
    max_backups: 10,  // Manter 10 backups
    ..Default::default()
};
```

### Auto-save

```rust
use code_terminal::ssh::{SessionManager, SessionPersistenceConfig};

// Configurar auto-save
let config = SessionPersistenceConfig {
    auto_save: true,
    auto_save_interval: 300,  // 5 minutos
    ..Default::default()
};

let mut manager = SessionManager::with_config(config);

// Iniciar auto-save
manager.start_auto_save(None);  // Usa intervalo da config

// Parar auto-save
manager.stop_auto_save();
```

## Formato de Arquivo

As sessões são salvas em formato JSON com a seguinte estrutura:

```json
{
  "metadata": {
    "name": "my-project",
    "created_at": "2026-03-13T21:20:00Z",
    "modified_at": "2026-03-13T21:20:00Z",
    "version": 1,
    "description": "Projeto principal"
  },
  "data": {
    "version": 1,
    "layout": {
      "active_pane": 1,
      "panes": [
        {
          "id": 1,
          "rect": { "x": 0, "y": 0, "width": 800, "height": 600 },
          "tabs": [
            {
              "id": 1,
              "title": "bash",
              "working_directory": "/home/user/projects",
              "command": null,
              "scroll_position": 0,
              "state": "active"
            }
          ],
          "active_tab": 1
        }
      ]
    },
    "working_directory": "/home/user/projects",
    "environment": {
      "EDITOR": "vim",
      "NODE_ENV": "development"
    }
  }
}
```

## Configuração

### SessionConfig

```rust
pub struct SessionConfig {
    /// Diretório onde as sessões são salvas
    pub sessions_dir: PathBuf,
    
    /// Nome da sessão padrão
    pub default_session_name: String,
    
    /// Número máximo de backups por sessão
    pub max_backups: usize,
    
    /// Habilitar compressão (não implementado)
    pub compression: bool,
    
    /// Incluir histórico de comandos (não implementado)
    pub include_command_history: bool,
}
```

### Configuração Padrão

```rust
impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            sessions_dir: dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("code-terminal")
                .join("sessions"),
            default_session_name: "default".to_string(),
            max_backups: 5,
            compression: false,
            include_command_history: true,
        }
    }
}
```

## Exemplos Completos

### Exemplo 1: Salvar Sessão Atual

```rust
use code_terminal::session::{SessionManager, SessionConfig, SessionData};
use std::path::PathBuf;

fn save_current_session() -> Result<(), Box<dyn std::error::Error>> {
    // Criar gerenciador
    let config = SessionConfig::default();
    let manager = SessionManager::new(config)?;
    manager.initialize()?;
    
    // Criar dados da sessão
    let mut data = SessionData::new();
    data.working_directory = Some(PathBuf::from("/home/user/project"));
    
    // Adicionar tabs
    let pane = PaneData {
        id: 1,
        rect: Rect { x: 0, y: 0, width: 800, height: 600 },
        tabs: vec![
            TabData {
                id: 1,
                title: "editor".to_string(),
                working_directory: Some(PathBuf::from("/home/user/project/src")),
                command: Some("vim main.rs".to_string()),
                scroll_position: 0,
                state: TabState::Active,
            },
            TabData {
                id: 2,
                title: "terminal".to_string(),
                working_directory: Some(PathBuf::from("/home/user/project")),
                command: None,
                scroll_position: 0,
                state: TabState::Inactive,
            },
        ],
        active_tab: 1,
    };
    data.layout.panes.push(pane);
    
    // Salvar
    manager.save("project-dev", &data)?;
    println!("Sessão salva com sucesso!");
    
    Ok(())
}
```

### Exemplo 2: Restaurar Sessão

```rust
use code_terminal::session::{SessionManager, SessionConfig};

fn restore_session(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = SessionConfig::default();
    let manager = SessionManager::new(config)?;
    manager.initialize()?;
    
    // Carregar sessão
    let session = manager.load(name)?;
    
    println!("Restaurando sessão: {}", session.metadata.name);
    println!("Criada em: {}", session.metadata.created_at);
    
    // Restaurar cada pane
    for pane in &session.data.layout.panes {
        println!("Pane {} ({}x{})", 
            pane.id, 
            pane.rect.width, 
            pane.rect.height
        );
        
        // Restaurar tabs
        for tab in &pane.tabs {
            println!("  Tab {}: {} ({:?})", 
                tab.id, 
                tab.title,
                tab.working_directory
            );
        }
    }
    
    Ok(())
}
```

### Exemplo 3: Listar e Gerenciar Sessões

```rust
use code_terminal::session::{SessionManager, SessionConfig};

fn list_and_manage() -> Result<(), Box<dyn std::error::Error>> {
    let config = SessionConfig::default();
    let manager = SessionManager::new(config)?;
    manager.initialize()?;
    
    // Listar todas as sessões
    println!("Sessões disponíveis:");
    for info in manager.list_sessions()? {
        let size_kb = info.size / 1024;
        println!("  {} - {} KB - modificado: {}",
            info.name, size_kb, info.modified_at
        );
    }
    
    // Deletar sessões antigas
    let max_age = chrono::Duration::days(30);
    let cutoff = chrono::Utc::now() - max_age;
    
    for info in manager.list_sessions()? {
        if info.modified_at < cutoff {
            println!("Deletando sessão antiga: {}", info.name);
            manager.delete(&info.name)?;
        }
    }
    
    Ok(())
}
```

## Erros Comuns

| Erro | Descrição | Solução |
|------|-----------|---------|
| `SessionNotFound` | Sessão não existe | Verifique o nome ou crie uma nova |
| `InvalidName` | Nome inválido | Use apenas caracteres alfanuméricos, hífen e underscore |
| `AlreadyExists` | Sessão já existe | Use outro nome ou delete a existente |
| `IoError` | Erro de I/O | Verifique permissões do diretório |
| `SerializationError` | Erro de serialização | Verifique se os dados são válidos |

## Integração com AppState

Para capturar o estado atual do terminal:

```rust
use code_terminal::session::SessionRestorer;
use code_terminal::app::AppState;

fn auto_save_session(app_state: &AppState) -> Result<(), SessionError> {
    let restorer = SessionRestorer::new();
    let data = restorer.capture_state(app_state)?;
    
    let config = SessionConfig::default();
    let manager = SessionManager::new(config)?;
    manager.initialize()?;
    
    manager.save("auto-save", &data)?;
    Ok(())
}
```

## Referência Rápida

### Métodos do SessionManager

| Método | Assinatura | Descrição |
|--------|------------|-----------|
| `new` | `new(config: SessionConfig) -> Result<Self>` | Cria novo gerenciador |
| `initialize` | `initialize(&self) -> Result<()>` | Cria diretórios necessários |
| `save` | `save(&self, name: &str, data: &SessionData) -> Result<()>` | Salva sessão |
| `load` | `load(&self, name: &str) -> Result<PersistedSession>` | Carrega sessão |
| `load_default` | `load_default(&self) -> Result<PersistedSession>` | Carrega sessão padrão |
| `exists` | `exists(&self, name: &str) -> Result<bool>` | Verifica existência |
| `delete` | `delete(&self, name: &str) -> Result<()>` | Deleta sessão |
| `rename` | `rename(&self, from: &str, to: &str) -> Result<()>` | Renomeia sessão |
| `list_sessions` | `list_sessions(&self) -> Result<Vec<SessionInfo>>` | Lista sessões |

---

**Versão da Documentação:** 1.0  
**Última Atualização:** Março 2026
