//! Gerenciador de sessões
//!
//! Fornece operações para salvar, carregar e gerenciar sessões do terminal.

use super::{
    error::{SessionError, SessionResult},
    serialization::SessionData,
    AvailableSession, Session, SessionMetadata, SESSIONS_DIR,
};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Configuração do gerenciador de sessões
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Diretório onde as sessões são salvas
    pub sessions_dir: PathBuf,
    /// Nome da sessão padrão
    pub default_session_name: String,
    /// Número máximo de backups
    pub max_backups: usize,
    /// Compressão de sessões
    pub compression: bool,
    /// Incluir histórico de comandos
    pub include_command_history: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            sessions_dir: default_sessions_dir().unwrap_or_else(|_| PathBuf::from(".sessions")),
            default_session_name: "default".to_string(),
            max_backups: 5,
            compression: false,
            include_command_history: true,
        }
    }
}

impl SessionConfig {
    /// Cria configuração com diretório específico
    pub fn with_directory(dir: impl Into<PathBuf>) -> Self {
        Self {
            sessions_dir: dir.into(),
            ..Default::default()
        }
    }

    /// Define o nome da sessão padrão
    pub fn with_default_name(mut self, name: impl Into<String>) -> Self {
        self.default_session_name = name.into();
        self
    }

    /// Define o número máximo de backups
    pub fn with_max_backups(mut self, max: usize) -> Self {
        self.max_backups = max;
        self
    }

    /// Habilita/desabilita compressão
    pub fn with_compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }
}

/// Informações sobre uma sessão
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    /// Nome da sessão
    pub name: String,
    /// Caminho do arquivo
    pub path: PathBuf,
    /// Data de criação
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Data de modificação
    pub modified_at: chrono::DateTime<chrono::Utc>,
    /// Tamanho em bytes
    pub size: u64,
    /// Versão do formato
    pub version: u32,
}

impl From<AvailableSession> for SessionInfo {
    fn from(session: AvailableSession) -> Self {
        Self {
            name: session.metadata.name,
            path: session.path,
            created_at: session.metadata.created_at,
            modified_at: session.metadata.modified_at,
            size: session.size,
            version: session.metadata.version,
        }
    }
}

/// Gerenciador de sessões
#[derive(Debug, Clone)]
pub struct SessionManager {
    config: SessionConfig,
}

impl SessionManager {
    /// Cria um novo gerenciador de sessões
    pub fn new(config: SessionConfig) -> Self {
        Self { config }
    }

    /// Cria gerenciador com configuração padrão
    pub fn default() -> SessionResult<Self> {
        let config = SessionConfig::default();
        Ok(Self::new(config))
    }

    /// Inicializa o diretório de sessões
    pub fn initialize(&self) -> SessionResult<()> {
        if !self.config.sessions_dir.exists() {
            info!(
                "Criando diretório de sessões: {:?}",
                self.config.sessions_dir
            );
            fs::create_dir_all(&self.config.sessions_dir).map_err(|_e| {
                SessionError::CreateDirectoryFailed(self.config.sessions_dir.clone())
            })?;
        }
        Ok(())
    }

    /// Salva uma sessão
    pub fn save(&self, name: impl AsRef<str>, data: &SessionData) -> SessionResult<()> {
        let name = name.as_ref();
        self.validate_name(name)?;

        let path = self.session_path(name);
        let metadata = SessionMetadata::new(name);
        let session = Session {
            metadata,
            data: data.clone(),
        };

        // Cria backup se a sessão já existe
        if path.exists() {
            self.create_backup(name)?;
        }

        // Serializa
        let json = serde_json::to_string_pretty(&session)
            .map_err(|e| SessionError::Serialization(e.to_string()))?;

        // Salva no arquivo
        fs::write(&path, json).map_err(|e| SessionError::SaveFailed(e.to_string()))?;

        info!("Sessão salva: {} em {:?}", name, path);
        Ok(())
    }

    /// Salva sessão padrão
    pub fn save_default(&self, data: &SessionData) -> SessionResult<()> {
        self.save(&self.config.default_session_name, data)
    }

    /// Carrega uma sessão
    pub fn load(&self, name: impl AsRef<str>) -> SessionResult<Session> {
        let name = name.as_ref();
        let path = self.session_path(name);

        if !path.exists() {
            return Err(SessionError::NotFound(name.to_string()));
        }

        let content =
            fs::read_to_string(&path).map_err(|e| SessionError::LoadFailed(e.to_string()))?;

        let session: Session = serde_json::from_str(&content)
            .map_err(|e| SessionError::Deserialization(e.to_string()))?;

        // Verifica versão
        if session.metadata.version > super::SESSION_FORMAT_VERSION {
            return Err(SessionError::UnsupportedVersion {
                version: session.metadata.version,
            });
        }

        debug!("Sessão carregada: {} de {:?}", name, path);
        Ok(session)
    }

    /// Carrega sessão padrão
    pub fn load_default(&self) -> SessionResult<Option<Session>> {
        match self.load(&self.config.default_session_name) {
            Ok(session) => Ok(Some(session)),
            Err(SessionError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Carrega apenas os dados da sessão
    pub fn load_data(&self, name: impl AsRef<str>) -> SessionResult<SessionData> {
        let session = self.load(name)?;
        Ok(session.data)
    }

    /// Lista todas as sessões disponíveis
    pub fn list_sessions(&self) -> SessionResult<Vec<SessionInfo>> {
        let mut sessions = Vec::new();

        if !self.config.sessions_dir.exists() {
            return Ok(sessions);
        }

        for entry in fs::read_dir(&self.config.sessions_dir).map_err(SessionError::Io)? {
            let entry = entry.map_err(SessionError::Io)?;
            let path = entry.path();

            if path.extension().map(|e| e == "json").unwrap_or(false) {
                match self.load_session_info(&path) {
                    Ok(info) => sessions.push(info),
                    Err(e) => {
                        warn!("Falha ao carregar info da sessão {:?}: {}", path, e);
                    }
                }
            }
        }

        // Ordena por data de modificação (mais recente primeiro)
        sessions.sort_by_key(|s| std::cmp::Reverse(s.modified_at));

        Ok(sessions)
    }

    /// Carrega informações de uma sessão sem carregar todos os dados
    fn load_session_info(&self, path: &Path) -> SessionResult<SessionInfo> {
        let content = fs::read_to_string(path).map_err(SessionError::Io)?;

        let session: Session = serde_json::from_str(&content)
            .map_err(|e| SessionError::Deserialization(e.to_string()))?;

        let metadata = session.metadata;
        let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        Ok(SessionInfo {
            name: metadata.name,
            path: path.to_path_buf(),
            created_at: metadata.created_at,
            modified_at: metadata.modified_at,
            size,
            version: metadata.version,
        })
    }

    /// Verifica se uma sessão existe
    pub fn exists(&self, name: impl AsRef<str>) -> bool {
        self.session_path(name).exists()
    }

    /// Deleta uma sessão
    pub fn delete(&self, name: impl AsRef<str>) -> SessionResult<()> {
        let name = name.as_ref();
        let path = self.session_path(name);

        if !path.exists() {
            return Err(SessionError::NotFound(name.to_string()));
        }

        fs::remove_file(&path).map_err(SessionError::Io)?;

        info!("Sessão deletada: {}", name);
        Ok(())
    }

    /// Renomeia uma sessão
    pub fn rename(
        &self,
        old_name: impl AsRef<str>,
        new_name: impl AsRef<str>,
    ) -> SessionResult<()> {
        let old_name = old_name.as_ref();
        let new_name = new_name.as_ref();

        self.validate_name(new_name)?;

        let old_path = self.session_path(old_name);
        let new_path = self.session_path(new_name);

        if !old_path.exists() {
            return Err(SessionError::NotFound(old_name.to_string()));
        }

        if new_path.exists() {
            return Err(SessionError::AlreadyExists(new_name.to_string()));
        }

        fs::rename(&old_path, &new_path).map_err(SessionError::Io)?;

        info!("Sessão renomeada: {} -> {}", old_name, new_name);
        Ok(())
    }

    /// Cria um backup da sessão
    fn create_backup(&self, name: impl AsRef<str>) -> SessionResult<()> {
        let name = name.as_ref();
        let source = self.session_path(name);

        if !source.exists() {
            return Ok(());
        }

        // Gera nome do backup com timestamp
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let backup_name = format!("{}_{}.json", name, timestamp);
        let backup_dir = self.config.sessions_dir.join("backups");

        if !backup_dir.exists() {
            fs::create_dir_all(&backup_dir)
                .map_err(|_e| SessionError::CreateDirectoryFailed(backup_dir.clone()))?;
        }

        let backup_path = backup_dir.join(&backup_name);

        fs::copy(&source, &backup_path).map_err(SessionError::Io)?;

        // Limpa backups antigos
        self.cleanup_old_backups(name)?;

        debug!("Backup criado: {:?}", backup_path);
        Ok(())
    }

    /// Limpa backups antigos
    fn cleanup_old_backups(&self, name: impl AsRef<str>) -> SessionResult<()> {
        let name = name.as_ref();
        let backup_dir = self.config.sessions_dir.join("backups");

        if !backup_dir.exists() {
            return Ok(());
        }

        let prefix = format!("{}_", name);
        let mut backups: Vec<_> = fs::read_dir(&backup_dir)
            .map_err(SessionError::Io)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with(&prefix) && n.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();

        // Ordena por data (mais recente primeiro)
        backups.sort_by(|a, b| {
            let time_a = a
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let time_b = b
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            time_b.cmp(&time_a)
        });

        // Remove backups antigos
        if backups.len() > self.config.max_backups {
            for entry in backups.iter().skip(self.config.max_backups) {
                if let Err(e) = fs::remove_file(entry.path()) {
                    warn!("Falha ao remover backup antigo: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Retorna o caminho para uma sessão
    fn session_path(&self, name: impl AsRef<str>) -> PathBuf {
        let name = name.as_ref();
        self.config.sessions_dir.join(format!("{}.json", name))
    }

    /// Valida o nome de uma sessão
    fn validate_name(&self, name: impl AsRef<str>) -> SessionResult<()> {
        let name = name.as_ref();

        if name.is_empty() {
            return Err(SessionError::InvalidName("nome vazio".to_string()));
        }

        if name.contains('/') || name.contains('\\') || name.contains(':') {
            return Err(SessionError::InvalidName(
                "nome contém caracteres inválidos".to_string(),
            ));
        }

        Ok(())
    }

    /// Retorna o diretório de sessões
    pub fn sessions_dir(&self) -> &Path {
        &self.config.sessions_dir
    }
}

/// Retorna o diretório padrão de sessões
fn default_sessions_dir() -> SessionResult<PathBuf> {
    let home = dirs::home_dir().ok_or(SessionError::HomeNotFound)?;
    Ok(home
        .join(".config")
        .join("code-terminal")
        .join(SESSIONS_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manager() -> (SessionManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionConfig::with_directory(temp_dir.path());
        let manager = SessionManager::new(config);
        manager.initialize().unwrap();
        (manager, temp_dir)
    }

    #[test]
    fn test_session_save_and_load() {
        let (manager, _temp) = create_test_manager();

        let data = SessionData::new();
        manager.save("test", &data).unwrap();

        let loaded = manager.load("test").unwrap();
        assert_eq!(loaded.metadata.name, "test");
        assert_eq!(loaded.data.version, data.version);
    }

    #[test]
    fn test_session_not_found() {
        let (manager, _temp) = create_test_manager();

        let result = manager.load("nonexistent");
        assert!(matches!(result, Err(SessionError::NotFound(_))));
    }

    #[test]
    fn test_session_exists() {
        let (manager, _temp) = create_test_manager();

        assert!(!manager.exists("test"));

        let data = SessionData::new();
        manager.save("test", &data).unwrap();

        assert!(manager.exists("test"));
    }

    #[test]
    fn test_session_delete() {
        let (manager, _temp) = create_test_manager();

        let data = SessionData::new();
        manager.save("test", &data).unwrap();
        assert!(manager.exists("test"));

        manager.delete("test").unwrap();
        assert!(!manager.exists("test"));
    }

    #[test]
    fn test_session_rename() {
        let (manager, _temp) = create_test_manager();

        let data = SessionData::new();
        manager.save("old", &data).unwrap();
        manager.rename("old", "new").unwrap();

        assert!(!manager.exists("old"));
        assert!(manager.exists("new"));
    }

    #[test]
    fn test_session_rename_already_exists() {
        let (manager, _temp) = create_test_manager();

        let data = SessionData::new();
        manager.save("old", &data).unwrap();
        manager.save("new", &data).unwrap();

        let result = manager.rename("old", "new");
        assert!(matches!(result, Err(SessionError::AlreadyExists(_))));
    }

    #[test]
    fn test_list_sessions() {
        let (manager, _temp) = create_test_manager();

        let data = SessionData::new();
        manager.save("session1", &data).unwrap();
        manager.save("session2", &data).unwrap();

        let sessions = manager.list_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_invalid_name() {
        let (manager, _temp) = create_test_manager();
        let data = SessionData::new();

        assert!(manager.save("", &data).is_err());
        assert!(manager.save("test/test", &data).is_err());
        assert!(manager.save("test:test", &data).is_err());
    }

    #[test]
    fn test_default_session() {
        let (manager, _temp) = create_test_manager();

        assert!(manager.load_default().unwrap().is_none());

        let data = SessionData::new();
        manager.save_default(&data).unwrap();

        let loaded = manager.load_default().unwrap();
        assert!(loaded.is_some());
    }
}
