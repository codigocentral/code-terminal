//! Sistema de persistência e restauração de sessões
//!
//! Este módulo implementa a serialização e desserialização do estado
//! do terminal, permitindo salvar e restaurar:
//! - Layout de abas e splits
//! - Diretórios de trabalho
//! - Configurações ativas
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::app::AppState;
//! use code_terminal::session::{SessionConfig, SessionManager, SessionRestorer};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Criar gerenciador de sessão
//! let config = SessionConfig::default();
//! let session_manager = SessionManager::new(config);
//!
//! // Salvar sessão atual (como sessão padrão)
//! let mut app_state = AppState::new(80, 24)?;
//! SessionRestorer::save_default_session(&app_state, &session_manager)?;
//!
//! // Restaurar a última sessão salva, se existir
//! if SessionRestorer::try_restore_last_session(&mut app_state, &session_manager)? {
//!     println!("Sessão restaurada");
//! }
//! # Ok(())
//! # }
//! ```

pub mod auto_save;
pub mod backup;
pub mod error;
pub mod manager;
pub mod migration;
pub mod restore;
pub mod serialization;
pub mod workspace;

pub use auto_save::{
    create_auto_save, create_auto_save_with_interval, AutoSaveConfig, AutoSaveEvent,
    AutoSaveHandle, AutoSaveManager, AutoSaveStats, AUTO_SAVE_SESSION_NAME,
};
pub use backup::{
    BackupConfig, BackupInfo, BackupManager, BackupMetadata, BackupOptions, BackupRetentionPolicy,
    BackupStats,
};
pub use error::{SessionError, SessionResult};
pub use manager::{SessionConfig, SessionInfo, SessionManager};
pub use migration::{
    MigrationExt, MigrationRegistry, VersionInfo, VersionManager, VersionMigrator,
};
pub use restore::{SessionRestorer, SessionSnapshot};
pub use serialization::{LayoutData, PaneData, SessionData, SplitData, TabData};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Versão do formato de sessão
pub const SESSION_FORMAT_VERSION: u32 = 1;

/// Nome do arquivo de sessão padrão
pub const DEFAULT_SESSION_FILE: &str = "session.json";

/// Nome do diretório de sessões
pub const SESSIONS_DIR: &str = "sessions";

/// Metadados de uma sessão salva
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Nome da sessão
    pub name: String,
    /// Timestamp de criação
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Timestamp da última modificação
    pub modified_at: chrono::DateTime<chrono::Utc>,
    /// Versão do formato
    pub version: u32,
    /// Descrição opcional
    pub description: Option<String>,
}

impl SessionMetadata {
    /// Cria novos metadados para uma sessão
    pub fn new(name: impl Into<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            name: name.into(),
            created_at: now,
            modified_at: now,
            version: SESSION_FORMAT_VERSION,
            description: None,
        }
    }

    /// Define a descrição
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Atualiza o timestamp de modificação
    pub fn touch(&mut self) {
        self.modified_at = chrono::Utc::now();
    }
}

/// Informações sobre uma sessão disponível
#[derive(Debug, Clone)]
pub struct AvailableSession {
    /// Metadados da sessão
    pub metadata: SessionMetadata,
    /// Caminho do arquivo
    pub path: PathBuf,
    /// Tamanho do arquivo em bytes
    pub size: u64,
}

/// Estrutura completa de uma sessão para salvar/carregar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Metadados da sessão
    pub metadata: SessionMetadata,
    /// Dados da sessão
    pub data: SessionData,
}

impl Session {
    /// Cria uma nova sessão
    pub fn new(name: impl Into<String>, data: SessionData) -> Self {
        Self {
            metadata: SessionMetadata::new(name),
            data,
        }
    }

    /// Atualiza o timestamp de modificação
    pub fn touch(&mut self) {
        self.metadata.touch();
    }
}
