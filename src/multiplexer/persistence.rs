//! Session Persistence - Persistência de sessões do multiplexer

use super::session::{MultiplexerSession, SessionId, SessionState};
use super::{MultiplexerError, MultiplexerResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Configuração de persistência
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceConfig {
    /// Diretório de persistência
    pub persistence_dir: PathBuf,
    /// Intervalo de auto-save (segundos)
    pub auto_save_interval: u64,
    /// Número máximo de sessões salvas
    pub max_saved_sessions: usize,
    /// Persistir histórico de saída
    pub persist_output_history: bool,
    /// Limite de sessões inativas (dias)
    pub inactive_session_ttl_days: u32,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            persistence_dir: dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("codeterm")
                .join("mux-sessions"),
            auto_save_interval: 300, // 5 minutos
            max_saved_sessions: 100,
            persist_output_history: true,
            inactive_session_ttl_days: 30,
        }
    }
}

/// Dados persistidos de uma sessão
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSessionData {
    /// ID da sessão
    pub id: SessionId,
    /// Nome da sessão
    pub name: String,
    /// Diretório de trabalho
    pub working_dir: String,
    /// Shell em uso
    pub shell: String,
    /// Comando sendo executado
    pub command: Option<String>,
    /// Flags da sessão
    pub flags: super::session::SessionFlags,
    /// Metadados
    pub metadata: HashMap<String, String>,
    /// Tamanho do terminal
    pub size: (u16, u16),
    /// Quando foi criada
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Snapshot do buffer (se persistido)
    pub buffer_snapshot: Option<String>,
    /// Posição do scroll
    pub scroll_position: usize,
}

impl From<&MultiplexerSession> for PersistedSessionData {
    fn from(session: &MultiplexerSession) -> Self {
        Self {
            id: session.id,
            name: session.name.clone(),
            working_dir: session.working_dir.clone(),
            shell: session.shell.clone(),
            command: session.command.clone(),
            flags: session.flags.clone(),
            metadata: session.metadata.clone(),
            size: session.size,
            created_at: session.created_at,
            last_activity: session.last_activity,
            buffer_snapshot: None, // Seria preenchido com dados reais
            scroll_position: 0,
        }
    }
}

/// Gerenciador de persistência
#[derive(Debug)]
pub struct SessionPersistence {
    config: PersistenceConfig,
}

impl SessionPersistence {
    /// Cria um novo gerenciador de persistência
    pub fn new() -> Self {
        Self::with_config(PersistenceConfig::default())
    }

    /// Cria com configuração customizada
    pub fn with_config(config: PersistenceConfig) -> Self {
        // Garante que o diretório existe
        if let Err(e) = std::fs::create_dir_all(&config.persistence_dir) {
            eprintln!("Warning: Failed to create persistence directory: {}", e);
        }

        Self { config }
    }

    /// Retorna o caminho do arquivo de uma sessão
    fn session_file_path(&self, id: SessionId) -> PathBuf {
        self.config.persistence_dir.join(format!("{}.json", id.0))
    }

    /// Salva uma sessão em disco
    pub fn save_session(&self, session: &MultiplexerSession) -> MultiplexerResult<()> {
        let data = PersistedSessionData::from(session);
        let json = serde_json::to_string_pretty(&data).map_err(MultiplexerError::Serialization)?;

        let path = self.session_file_path(session.id);
        std::fs::write(&path, json).map_err(MultiplexerError::Io)?;

        Ok(())
    }

    /// Carrega uma sessão do disco
    pub fn load_session(&self, id: SessionId) -> MultiplexerResult<PersistedSessionData> {
        let path = self.session_file_path(id);
        let json = std::fs::read_to_string(&path).map_err(MultiplexerError::Io)?;

        let data: PersistedSessionData =
            serde_json::from_str(&json).map_err(MultiplexerError::Serialization)?;

        Ok(data)
    }

    /// Remove uma sessão do disco
    pub fn remove_session(&self, id: SessionId) -> MultiplexerResult<()> {
        let path = self.session_file_path(id);
        if path.exists() {
            std::fs::remove_file(&path).map_err(MultiplexerError::Io)?;
        }
        Ok(())
    }

    /// Lista todas as sessões persistidas
    pub fn list_saved_sessions(&self) -> MultiplexerResult<Vec<PersistedSessionData>> {
        let mut sessions = Vec::new();

        let entries =
            std::fs::read_dir(&self.config.persistence_dir).map_err(MultiplexerError::Io)?;

        for entry in entries {
            let entry = entry.map_err(MultiplexerError::Io)?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "json") {
                if let Ok(json) = std::fs::read_to_string(&path) {
                    if let Ok(data) = serde_json::from_str::<PersistedSessionData>(&json) {
                        sessions.push(data);
                    }
                }
            }
        }

        // Ordena por última atividade (mais recente primeiro)
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_activity));

        Ok(sessions)
    }

    /// Restaura uma sessão a partir dos dados persistidos
    pub fn restore_session(&self, data: &PersistedSessionData) -> MultiplexerSession {
        let mut session = MultiplexerSession::new(Some(data.name.clone()), data.size);
        session.id = data.id;
        session.working_dir = data.working_dir.clone();
        session.shell = data.shell.clone();
        session.command = data.command.clone();
        session.flags = data.flags.clone();
        session.metadata = data.metadata.clone();
        session.created_at = data.created_at;
        session.last_activity = data.last_activity;
        session.state = SessionState::Background; // Restaura em background

        session
    }

    /// Limpa sessões antigas
    pub fn cleanup_old_sessions(&self, max_age_days: u32) -> MultiplexerResult<usize> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(max_age_days as i64);
        let mut removed = 0;

        let saved = self.list_saved_sessions()?;
        for data in saved {
            if data.last_activity < cutoff {
                self.remove_session(data.id)?;
                removed += 1;
            }
        }

        Ok(removed)
    }

    /// Retorna estatísticas de persistência
    pub fn stats(&self) -> MultiplexerResult<PersistenceStats> {
        let saved = self.list_saved_sessions()?;
        let total_count = saved.len();

        let total_size: u64 = saved
            .iter()
            .map(|s| {
                self.session_file_path(s.id)
                    .metadata()
                    .map(|m| m.len())
                    .unwrap_or(0)
            })
            .sum();

        let oldest = saved.iter().map(|s| s.created_at).min();
        let newest = saved.iter().map(|s| s.last_activity).max();

        Ok(PersistenceStats {
            total_sessions: total_count,
            total_size_bytes: total_size,
            oldest_session: oldest,
            newest_session: newest,
        })
    }

    /// Verifica se uma sessão existe em disco
    pub fn session_exists(&self, id: SessionId) -> bool {
        self.session_file_path(id).exists()
    }

    /// Exporta todas as sessões para um arquivo
    pub fn export_sessions(&self, path: &Path) -> MultiplexerResult<usize> {
        let sessions = self.list_saved_sessions()?;
        let count = sessions.len();

        let json =
            serde_json::to_string_pretty(&sessions).map_err(MultiplexerError::Serialization)?;

        std::fs::write(path, json).map_err(MultiplexerError::Io)?;

        Ok(count)
    }

    /// Importa sessões de um arquivo
    pub fn import_sessions(&self, path: &Path) -> MultiplexerResult<usize> {
        let json = std::fs::read_to_string(path).map_err(MultiplexerError::Io)?;

        let sessions: Vec<PersistedSessionData> =
            serde_json::from_str(&json).map_err(MultiplexerError::Serialization)?;

        let count = sessions.len();

        // Salva cada sessão
        for data in &sessions {
            let path = self.session_file_path(data.id);
            let json =
                serde_json::to_string_pretty(data).map_err(MultiplexerError::Serialization)?;
            std::fs::write(&path, json).map_err(MultiplexerError::Io)?;
        }

        Ok(count)
    }
}

impl Default for SessionPersistence {
    fn default() -> Self {
        Self::new()
    }
}

/// Estatísticas de persistência
#[derive(Debug, Clone)]
pub struct PersistenceStats {
    /// Total de sessões salvas
    pub total_sessions: usize,
    /// Tamanho total em bytes
    pub total_size_bytes: u64,
    /// Sessão mais antiga
    pub oldest_session: Option<chrono::DateTime<chrono::Utc>>,
    /// Sessão mais recente
    pub newest_session: Option<chrono::DateTime<chrono::Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_persistence() -> (SessionPersistence, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = PersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let persistence = SessionPersistence::with_config(config);
        (persistence, temp_dir)
    }

    #[test]
    fn test_save_and_load_session() {
        let (persistence, _temp) = temp_persistence();

        let mut session = MultiplexerSession::new(Some("test".to_string()), (80, 24));
        session.working_dir = "/home/user".to_string();
        session.shell = "/bin/zsh".to_string();

        persistence.save_session(&session).unwrap();

        let loaded = persistence.load_session(session.id).unwrap();
        assert_eq!(loaded.name, "test");
        assert_eq!(loaded.working_dir, "/home/user");
        assert_eq!(loaded.shell, "/bin/zsh");
    }

    #[test]
    fn test_remove_session() {
        let (persistence, _temp) = temp_persistence();

        let session = MultiplexerSession::new(Some("test".to_string()), (80, 24));
        persistence.save_session(&session).unwrap();

        assert!(persistence.session_exists(session.id));

        persistence.remove_session(session.id).unwrap();

        assert!(!persistence.session_exists(session.id));
    }

    #[test]
    fn test_list_saved_sessions() {
        let (persistence, _temp) = temp_persistence();

        let session1 = MultiplexerSession::new(Some("test1".to_string()), (80, 24));
        let session2 = MultiplexerSession::new(Some("test2".to_string()), (80, 24));

        persistence.save_session(&session1).unwrap();
        persistence.save_session(&session2).unwrap();

        let list = persistence.list_saved_sessions().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_restore_session() {
        let (persistence, _temp) = temp_persistence();

        let mut original = MultiplexerSession::new(Some("test".to_string()), (80, 24));
        original.working_dir = "/tmp".to_string();
        original
            .metadata
            .insert("key".to_string(), "value".to_string());

        let data = PersistedSessionData::from(&original);
        let restored = persistence.restore_session(&data);

        assert_eq!(restored.name, "test");
        assert_eq!(restored.working_dir, "/tmp");
        assert_eq!(restored.metadata.get("key"), Some(&"value".to_string()));
        assert_eq!(restored.state, SessionState::Background);
    }

    #[test]
    fn test_export_import() {
        let (persistence1, _temp1) = temp_persistence();
        let (persistence2, _temp2) = temp_persistence();

        let session = MultiplexerSession::new(Some("export_test".to_string()), (80, 24));
        persistence1.save_session(&session).unwrap();

        let export_path = _temp2.path().join("export.json");
        let count = persistence1.export_sessions(&export_path).unwrap();
        assert_eq!(count, 1);

        let imported = persistence2.import_sessions(&export_path).unwrap();
        assert_eq!(imported, 1);

        let list = persistence2.list_saved_sessions().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "export_test");
    }
}
