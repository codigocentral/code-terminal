//! SSH Manager - Gerenciador de conexões SSH
//!
//! Este módulo implementa funcionalidades para gerenciar conexões SSH,
//! incluindo:
//! - Parser completo de ~/.ssh/config
//! - Gerenciamento de hosts SSH
//! - Integração com ~/.ssh/config
//! - Gerenciamento de chaves SSH
//! - Persistência de sessões
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::ssh::{SshConfigParser, SshHost};
//!
//! // Parse do arquivo ~/.ssh/config
//! let parser = SshConfigParser::parse_default_config().unwrap();
//!
//! // Buscar configuração efetiva para um host
//! if let Some(host) = parser.get_effective_config("myserver") {
//!     println!("Conectar em: {}", host.connection_string());
//!
//!     // Gerar comando SSH
//!     let args = parser.to_ssh_command(&host);
//!     println!("ssh {}", args.join(" "));
//! }
//! ```

pub mod agent;
pub mod config;
pub mod config_edit;
pub mod config_parser;
pub mod error;
pub mod host;
pub mod key;
pub mod key_manager;
pub mod quick_connect;
pub mod security;
pub mod session;
pub mod storage;

pub use agent::{AgentClient, AgentKey, SshAgent};
pub use config::{SshConfig, SshHost as LegacySshHost, SshIdentityFile};
pub use config_parser::SshConfigParser;
pub use error::{SshError, SshKeyError, SshKeyResult, SshResult};
pub use host::{DynamicForward, LogLevel, PortForward, SshHost, StrictHostKeyChecking};
pub use key::{KeyFingerprint, KeyType, PrivateKey, PublicKey, SshKey};
pub use key_manager::{KeyGenerationOptions, KeyManager};
pub use quick_connect::{
    FilterMode, QuickConnectDialog, QuickConnectError, QuickConnectField, QuickConnectParams,
    QuickConnectRecent, QuickConnectSelection, QuickConnectState,
};
pub use security::{
    RecommendedAlgorithms, SecurityAuditor, SecurityIssue, SecurityReport, SecurityValidator,
    Severity,
};
pub use session::{
    AutoSaveHandle, PersistedSession, SessionBackup, SessionId, SessionManager,
    SessionPersistenceConfig, SessionState, SessionType, SshSession,
};
pub use storage::{EncryptedStorage, KeyStorage};

use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct RecentConnection {
    pub user: Option<String>,
    pub host: String,
    pub port: u16,
    pub last_used: std::time::Instant,
}

/// Duas conexões são a mesma quando usuário, host e porta coincidem; o
/// horário de uso não entra na comparação (senão a deduplicação de
/// `add_recent_connection` nunca casava).
impl PartialEq for RecentConnection {
    fn eq(&self, other: &Self) -> bool {
        self.user == other.user && self.host == other.host && self.port == other.port
    }
}

impl RecentConnection {
    pub fn new(connection_string: &str, port: u16) -> Self {
        let (user, host) = if let Some(at_pos) = connection_string.find('@') {
            (
                Some(connection_string[..at_pos].to_string()),
                connection_string[at_pos + 1..].to_string(),
            )
        } else {
            (None, connection_string.to_string())
        };

        Self {
            user,
            host,
            port,
            last_used: std::time::Instant::now(),
        }
    }
}

impl From<QuickConnectRecent> for RecentConnection {
    fn from(recent: QuickConnectRecent) -> Self {
        Self {
            user: recent.user,
            host: recent.hostname,
            port: recent.port,
            last_used: std::time::Instant::now(),
        }
    }
}

/// Gerenciador SSH central
#[derive(Debug)]
pub struct SshManager {
    /// Parser de configuração SSH
    config_parser: Arc<RwLock<Option<SshConfigParser>>>,
    /// Configuração legada (compatibilidade)
    legacy_config: Arc<RwLock<SshConfig>>,
    /// Conexões recentes
    recent_connections: Arc<RwLock<Vec<RecentConnection>>>,
    /// Máximo de conexões recentes a manter
    max_recent: usize,
}

impl Default for SshManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SshManager {
    /// Cria um novo gerenciador SSH
    pub fn new() -> Self {
        Self {
            config_parser: Arc::new(RwLock::new(None)),
            legacy_config: Arc::new(RwLock::new(SshConfig::default())),
            recent_connections: Arc::new(RwLock::new(Vec::new())),
            max_recent: 20,
        }
    }

    /// Carrega configuração do arquivo ~/.ssh/config
    pub fn load_config(&self) -> Result<(), SshManagerError> {
        match SshConfigParser::parse_default_config() {
            Ok(parser) => {
                *self.config_parser.write() = Some(parser);
                Ok(())
            }
            Err(SshError::ConfigNotFound) => {
                *self.config_parser.write() = None;
                Ok(())
            }
            Err(e) => Err(SshManagerError::ConfigLoad(e.to_string())),
        }
    }

    /// Carrega configuração legada (compatibilidade)
    pub fn load_legacy_config(&self) -> Result<(), SshManagerError> {
        let mut config = self.legacy_config.write();
        *config = SshConfig::load_from_default()
            .map_err(|e| SshManagerError::ConfigLoad(e.to_string()))?;
        Ok(())
    }

    /// Retorna o parser de configuração
    pub fn get_config_parser(&self) -> Option<SshConfigParser> {
        self.config_parser.read().clone()
    }

    /// Retorna a configuração legada
    pub fn get_config(&self) -> Arc<RwLock<SshConfig>> {
        Arc::clone(&self.legacy_config)
    }

    /// Adiciona uma conexão recente
    pub fn add_recent_connection(&self, conn: RecentConnection) {
        let mut recent = self.recent_connections.write();

        recent.retain(|c| c != &conn);
        recent.insert(0, conn);

        if recent.len() > self.max_recent {
            recent.truncate(self.max_recent);
        }
    }

    /// Retorna conexões recentes
    pub fn get_recent_connections(&self) -> Vec<RecentConnection> {
        self.recent_connections.read().clone()
    }

    /// Limpa conexões recentes
    pub fn clear_recent_connections(&self) {
        self.recent_connections.write().clear();
    }

    /// Busca hosts por query usando novo parser
    pub fn search_hosts(&self, query: &str) -> Vec<SshHost> {
        if let Some(ref parser) = *self.config_parser.read() {
            parser
                .hosts()
                .iter()
                .filter(|h| {
                    h.pattern.to_lowercase().contains(&query.to_lowercase())
                        || h.host_name
                            .as_ref()
                            .is_some_and(|hn| hn.to_lowercase().contains(&query.to_lowercase()))
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Busca hosts legados por query
    pub fn search_legacy_hosts(&self, query: &str) -> Vec<LegacySshHost> {
        let config = self.legacy_config.read();
        config.search_hosts(query)
    }

    /// Retorna todos os hosts
    pub fn get_all_hosts(&self) -> Vec<SshHost> {
        if let Some(ref parser) = *self.config_parser.read() {
            parser.hosts().to_vec()
        } else {
            Vec::new()
        }
    }

    /// Retorna um host pelo pattern
    pub fn get_host(&self, pattern: &str) -> Option<SshHost> {
        if let Some(ref parser) = *self.config_parser.read() {
            parser.find_host(pattern).cloned()
        } else {
            None
        }
    }

    /// Retorna configuração efetiva para um hostname
    pub fn get_effective_config(&self, hostname: &str) -> Option<SshHost> {
        if let Some(ref parser) = *self.config_parser.read() {
            parser.get_effective_config(hostname)
        } else {
            None
        }
    }

    /// Gera argumentos para comando SSH
    pub fn to_ssh_command(&self, host: &SshHost) -> Vec<String> {
        if let Some(ref parser) = *self.config_parser.read() {
            parser.to_ssh_command(host)
        } else {
            let mut args = Vec::new();
            if let Some(ref user) = host.user {
                args.push("-l".to_string());
                args.push(user.clone());
            }
            if let Some(port) = host.port {
                args.push("-p".to_string());
                args.push(port.to_string());
            }
            args.push(host.effective_host().to_string());
            args
        }
    }
}

/// Erros do gerenciador SSH
#[derive(Debug, thiserror::Error)]
pub enum SshManagerError {
    #[error("Failed to load SSH config: {0}")]
    ConfigLoad(String),

    #[error("Failed to save SSH config: {0}")]
    ConfigSave(String),

    #[error("Invalid host configuration: {0}")]
    InvalidHost(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_manager_creation() {
        let manager = SshManager::new();
        assert!(manager.get_all_hosts().is_empty());
    }

    #[test]
    fn test_add_recent_connection() {
        let manager = SshManager::new();
        let conn = RecentConnection::new("user@example.com", 22);

        manager.add_recent_connection(conn.clone());

        let recent = manager.get_recent_connections();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].host, conn.host);
    }

    #[test]
    fn test_recent_connections_limit() {
        let manager = SshManager::new();

        for i in 0..25 {
            let conn = RecentConnection::new(&format!("user{}@example.com", i), 22);
            manager.add_recent_connection(conn);
        }

        let recent = manager.get_recent_connections();
        assert_eq!(recent.len(), 20);
    }

    #[test]
    fn test_clear_recent_connections() {
        let manager = SshManager::new();
        let conn = RecentConnection::new("user@example.com", 22);

        manager.add_recent_connection(conn);
        assert!(!manager.get_recent_connections().is_empty());

        manager.clear_recent_connections();
        assert!(manager.get_recent_connections().is_empty());
    }

    #[test]
    fn test_parse_sample_config() {
        let config = r#"
Host test-server
    HostName example.com
    User testuser
    Port 22
"#;
        let parser = SshConfigParser::from_str(config).unwrap();
        assert_eq!(parser.hosts().len(), 1);

        let host = parser.hosts().first().unwrap();
        assert_eq!(host.pattern, "test-server");
        assert_eq!(host.host_name, Some("example.com".to_string()));
        assert_eq!(host.user, Some("testuser".to_string()));
        assert_eq!(host.port, Some(22));
    }

    #[test]
    fn test_get_effective_config() {
        let config = r#"
Host *
    User defaultuser

Host myserver
    HostName example.com
    Port 2222
"#;
        let parser = SshConfigParser::from_str(config).unwrap();
        let effective = parser.get_effective_config("myserver");

        assert!(effective.is_some());
        let effective = effective.unwrap();
        assert_eq!(effective.host_name, Some("example.com".to_string()));
        assert_eq!(effective.port, Some(2222));
    }

    const SAMPLE: &str = "\
Host web
    HostName web.example.com
    User deploy
    Port 2222

Host db-prod
    HostName 10.0.0.9
";

    fn manager_with(config: &str) -> SshManager {
        let m = SshManager::default();
        *m.config_parser.write() = Some(SshConfigParser::from_str(config).unwrap());
        m
    }

    #[test]
    fn test_recent_connection_parsing_and_conversion() {
        let c = RecentConnection::new("ana@srv", 2200);
        assert_eq!(c.user.as_deref(), Some("ana"));
        assert_eq!(c.host, "srv");
        assert_eq!(c.port, 2200);
        let c = RecentConnection::new("srv", 22);
        assert_eq!(c.user, None);

        let qc = QuickConnectRecent {
            connection_string: "bob@box:2022".into(),
            hostname: "box".into(),
            user: Some("bob".into()),
            port: 2022,
            identity_file: None,
            last_used: chrono::Utc::now(),
            use_count: 3,
        };
        let rc = RecentConnection::from(qc);
        assert_eq!(rc, RecentConnection::new("bob@box", 2022));
    }

    #[test]
    fn test_add_recent_connection_deduplicates_and_moves_to_front() {
        // Regressão: o PartialEq derivado comparava também o Instant, então a
        // mesma conexão adicionada de novo virava duplicata
        let m = SshManager::new();
        m.add_recent_connection(RecentConnection::new("a@h1", 22));
        m.add_recent_connection(RecentConnection::new("b@h2", 22));
        m.add_recent_connection(RecentConnection::new("a@h1", 22));
        let recent = m.get_recent_connections();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].host, "h1");
        assert_eq!(recent[1].host, "h2");
        // porta diferente é outra conexão
        m.add_recent_connection(RecentConnection::new("a@h1", 2222));
        assert_eq!(m.get_recent_connections().len(), 3);
    }

    #[test]
    fn test_queries_with_loaded_parser() {
        let m = manager_with(SAMPLE);
        assert!(m.get_config_parser().is_some());
        assert_eq!(m.get_all_hosts().len(), 2);

        let found: Vec<_> = m
            .search_hosts("WEB")
            .into_iter()
            .map(|h| h.pattern)
            .collect();
        assert_eq!(found, vec!["web"]);
        // busca também pelo HostName
        let found: Vec<_> = m
            .search_hosts("10.0.0")
            .into_iter()
            .map(|h| h.pattern)
            .collect();
        assert_eq!(found, vec!["db-prod"]);
        assert!(m.search_hosts("zzz").is_empty());

        assert_eq!(m.get_host("web").unwrap().port, Some(2222));
        assert!(m.get_host("nao").is_none());
        let eff = m.get_effective_config("web").unwrap();
        assert_eq!(eff.user.as_deref(), Some("deploy"));

        let host = m.get_host("web").unwrap();
        let args = m.to_ssh_command(&host);
        assert!(args.windows(2).any(|w| w == ["-l", "deploy"]));
        assert!(args.windows(2).any(|w| w == ["-p", "2222"]));
    }

    #[test]
    fn test_queries_without_parser_fall_back() {
        let m = SshManager::new();
        assert!(m.get_config_parser().is_none());
        assert!(m.search_hosts("x").is_empty());
        assert!(m.get_host("x").is_none());
        assert!(m.get_effective_config("x").is_none());

        let mut host = SshHost::new("alias");
        host.host_name = Some("real.example".into());
        host.user = Some("u".into());
        host.port = Some(2022);
        assert_eq!(
            m.to_ssh_command(&host),
            vec!["-l", "u", "-p", "2022", "real.example"]
        );
        assert_eq!(
            m.to_ssh_command(&SshHost::new("so-alias")),
            vec!["so-alias"]
        );
    }

    #[test]
    fn test_legacy_config_search() {
        let m = SshManager::new();
        {
            let cfg = m.get_config();
            let mut cfg = cfg.write();
            let mut h = LegacySshHost::new("legado");
            h.hostname = Some("old.example".into());
            cfg.add_host(h);
        }
        assert_eq!(m.search_legacy_hosts("").len(), 1);
        assert_eq!(m.search_legacy_hosts("legado")[0].alias, "legado");
        assert!(m.search_legacy_hosts("zzz").is_empty());
    }

    #[test]
    fn test_manager_error_messages() {
        assert_eq!(
            SshManagerError::ConfigLoad("x".into()).to_string(),
            "Failed to load SSH config: x"
        );
        assert_eq!(
            SshManagerError::ConfigSave("y".into()).to_string(),
            "Failed to save SSH config: y"
        );
        assert_eq!(
            SshManagerError::InvalidHost("z".into()).to_string(),
            "Invalid host configuration: z"
        );
        let io: SshManagerError = std::io::Error::other("disco").into();
        assert_eq!(io.to_string(), "IO error: disco");
    }
}
