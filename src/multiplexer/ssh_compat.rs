//! SSH Compatibility - Integração do multiplexador com SSH
//!
//! Este módulo implementa a integração entre o sistema de multiplexador
//! e o módulo SSH, permitindo sessões remotas compartilhadas via SSH.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::session::SessionId;
use super::shared::SharedSessionConfig;
use super::{MultiplexerError, MultiplexerResult};
use crate::ssh::{SshHost, SshManager};

/// Tipo de transporte para sessão SSH
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SshTransport {
    /// Conexão SSH direta
    Direct,
    /// Via jump host
    ProxyJump,
    /// Via SOCKS proxy
    SocksProxy,
    /// Via SSH tunnel
    Tunnel,
}

impl std::fmt::Display for SshTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshTransport::Direct => write!(f, "direct"),
            SshTransport::ProxyJump => write!(f, "proxy-jump"),
            SshTransport::SocksProxy => write!(f, "socks-proxy"),
            SshTransport::Tunnel => write!(f, "tunnel"),
        }
    }
}

/// Configuração de compatibilidade SSH
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshCompatConfig {
    /// Usar configuração do ~/.ssh/config
    pub use_ssh_config: bool,
    /// Usar agente SSH
    pub use_ssh_agent: bool,
    /// Identidade padrão
    pub default_identity: Option<PathBuf>,
    /// Timeout de conexão (segundos)
    pub connection_timeout: u64,
    /// Keepalive interval (segundos)
    pub keepalive_interval: u64,
    /// Strict host key checking
    pub strict_host_key_checking: bool,
    /// Forward agent
    pub forward_agent: bool,
    /// Forward X11
    pub forward_x11: bool,
    /// Portas para forwarding
    pub port_forwards: Vec<PortForwardConfig>,
}

impl Default for SshCompatConfig {
    fn default() -> Self {
        Self {
            use_ssh_config: true,
            use_ssh_agent: true,
            default_identity: None,
            connection_timeout: 30,
            keepalive_interval: 60,
            strict_host_key_checking: true,
            forward_agent: false,
            forward_x11: false,
            port_forwards: Vec::new(),
        }
    }
}

/// Configuração de port forwarding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortForwardConfig {
    /// Tipo de forward
    pub forward_type: PortForwardType,
    /// Porta local
    pub local_port: u16,
    /// Host remoto
    pub remote_host: String,
    /// Porta remota
    pub remote_port: u16,
}

/// Tipo de port forwarding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PortForwardType {
    /// Forward local
    Local,
    /// Forward remoto
    Remote,
    /// Dynamic (SOCKS)
    Dynamic,
}

/// Informações de uma sessão SSH remota
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshSessionInfo {
    /// ID da sessão
    pub session_id: SessionId,
    /// Host SSH
    pub host: String,
    /// Usuário
    pub user: String,
    /// Porta
    pub port: u16,
    /// Método de autenticação
    pub auth_method: AuthMethod,
    /// Transporte usado
    pub transport: SshTransport,
    /// Se está conectado
    pub is_connected: bool,
    /// Latência da conexão (ms)
    pub latency_ms: Option<u64>,
    /// Bytes enviados
    pub bytes_sent: u64,
    /// Bytes recebidos
    pub bytes_received: u64,
    /// Quando foi criada
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Método de autenticação
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthMethod {
    /// Chave pública
    PublicKey { key_path: PathBuf },
    /// Senha
    Password,
    /// Agente SSH
    Agent,
    /// Kerberos
    Kerberos,
    /// Teclado-interativo
    KeyboardInteractive,
}

impl std::fmt::Display for AuthMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthMethod::PublicKey { key_path } => write!(f, "publickey({})", key_path.display()),
            AuthMethod::Password => write!(f, "password"),
            AuthMethod::Agent => write!(f, "agent"),
            AuthMethod::Kerberos => write!(f, "kerberos"),
            AuthMethod::KeyboardInteractive => write!(f, "keyboard-interactive"),
        }
    }
}

/// Uma sessão SSH compatível com multiplexador
#[derive(Debug)]
pub struct SshCompatibleSession {
    /// ID da sessão
    pub session_id: SessionId,
    /// Informações SSH
    pub ssh_info: SshSessionInfo,
    /// Configuração SSH
    pub config: SshCompatConfig,
    /// Host SSH (do módulo SSH)
    pub ssh_host: Option<SshHost>,
    /// Buffer de I/O
    pub io_buffer: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
}

impl SshCompatibleSession {
    /// Cria uma nova sessão SSH compatível
    pub fn new(
        session_id: SessionId,
        host: impl Into<String>,
        user: impl Into<String>,
        config: SshCompatConfig,
    ) -> Self {
        let host_str = host.into();
        let user_str = user.into();

        let ssh_info = SshSessionInfo {
            session_id,
            host: host_str.clone(),
            user: user_str.clone(),
            port: 22,
            auth_method: AuthMethod::Agent, // Default
            transport: SshTransport::Direct,
            is_connected: false,
            latency_ms: None,
            bytes_sent: 0,
            bytes_received: 0,
            created_at: chrono::Utc::now(),
            last_activity: chrono::Utc::now(),
        };

        Self {
            session_id,
            ssh_info,
            config,
            ssh_host: None,
            io_buffer: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Define o método de autenticação
    pub fn with_auth_method(mut self, method: AuthMethod) -> Self {
        self.ssh_info.auth_method = method;
        self
    }

    /// Define a porta
    pub fn with_port(mut self, port: u16) -> Self {
        self.ssh_info.port = port;
        self
    }

    /// Define o transporte
    pub fn with_transport(mut self, transport: SshTransport) -> Self {
        self.ssh_info.transport = transport;
        self
    }

    /// Conecta à sessão SSH
    pub fn connect(&mut self) -> MultiplexerResult<()> {
        // Em produção, isso estabeleceria a conexão SSH real
        // Por enquanto, apenas simulamos

        self.ssh_info.is_connected = true;
        self.ssh_info.latency_ms = Some(50); // Simulado
        self.touch();

        Ok(())
    }

    /// Desconecta da sessão SSH
    pub fn disconnect(&mut self) {
        self.ssh_info.is_connected = false;
    }

    /// Envia dados
    pub fn send(&mut self, data: &[u8]) -> MultiplexerResult<()> {
        if !self.ssh_info.is_connected {
            return Err(MultiplexerError::ServerError("Not connected".to_string()));
        }

        self.ssh_info.bytes_sent += data.len() as u64;
        self.touch();

        Ok(())
    }

    /// Recebe dados
    pub fn receive(&mut self) -> MultiplexerResult<Vec<u8>> {
        if !self.ssh_info.is_connected {
            return Err(MultiplexerError::ServerError("Not connected".to_string()));
        }

        let data = {
            // Um mutex envenenado ainda guarda bytes válidos: não derruba o leitor
            let mut buffer = self
                .io_buffer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let data = std::mem::take(&mut *buffer);
            self.ssh_info.bytes_received += data.len() as u64;
            data
        };
        self.touch();

        Ok(data)
    }

    /// Atualiza a última atividade
    fn touch(&mut self) {
        self.ssh_info.last_activity = chrono::Utc::now();
    }

    /// Retorna as informações da sessão
    pub fn info(&self) -> &SshSessionInfo {
        &self.ssh_info
    }

    /// Configura port forwarding
    pub fn setup_port_forward(&mut self, _config: PortForwardConfig) -> MultiplexerResult<()> {
        // Em produção, configuraria o forwarding real
        self.touch();
        Ok(())
    }

    /// Cria um túnel SOCKS
    pub fn create_socks_proxy(&mut self, local_port: u16) -> MultiplexerResult<()> {
        let config = PortForwardConfig {
            forward_type: PortForwardType::Dynamic,
            local_port,
            remote_host: String::new(),
            remote_port: 0,
        };

        self.setup_port_forward(config)
    }
}

/// Gerenciador de compatibilidade SSH
#[derive(Debug, Default)]
pub struct SshCompatManager {
    /// Sessões SSH ativas
    sessions: HashMap<SessionId, SshCompatibleSession>,
    /// Configuração padrão
    default_config: SshCompatConfig,
    /// Gerenciador SSH (do módulo ssh)
    ssh_manager: Option<SshManager>,
}

impl SshCompatManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            default_config: SshCompatConfig::default(),
            ssh_manager: None,
        }
    }

    /// Define o gerenciador SSH
    pub fn with_ssh_manager(mut self, manager: SshManager) -> Self {
        self.ssh_manager = Some(manager);
        self
    }

    /// Define a configuração padrão
    pub fn with_default_config(mut self, config: SshCompatConfig) -> Self {
        self.default_config = config;
        self
    }

    /// Cria uma nova sessão SSH
    pub fn create_session(
        &mut self,
        session_id: SessionId,
        host: impl Into<String>,
        user: impl Into<String>,
    ) -> &mut SshCompatibleSession {
        let session =
            SshCompatibleSession::new(session_id, host, user, self.default_config.clone());

        self.sessions
            .entry(session_id)
            .insert_entry(session)
            .into_mut()
    }

    /// Obtém uma sessão
    pub fn get_session(&self, session_id: SessionId) -> Option<&SshCompatibleSession> {
        self.sessions.get(&session_id)
    }

    /// Obtém uma sessão mutável
    pub fn get_session_mut(&mut self, session_id: SessionId) -> Option<&mut SshCompatibleSession> {
        self.sessions.get_mut(&session_id)
    }

    /// Remove uma sessão
    pub fn remove_session(
        &mut self,
        session_id: SessionId,
    ) -> MultiplexerResult<SshCompatibleSession> {
        let mut session = self
            .sessions
            .remove(&session_id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(session_id.to_string()))?;

        session.disconnect();
        Ok(session)
    }

    /// Lista todas as sessões SSH
    pub fn list_sessions(&self) -> Vec<&SshSessionInfo> {
        self.sessions.values().map(|s| &s.ssh_info).collect()
    }

    /// Lista sessões conectadas
    pub fn list_connected(&self) -> Vec<&SshSessionInfo> {
        self.sessions
            .values()
            .filter(|s| s.ssh_info.is_connected)
            .map(|s| &s.ssh_info)
            .collect()
    }

    /// Conecta a uma sessão
    pub fn connect(&mut self, session_id: SessionId) -> MultiplexerResult<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(session_id.to_string()))?;

        session.connect()
    }

    /// Desconecta de uma sessão
    pub fn disconnect(&mut self, session_id: SessionId) -> MultiplexerResult<()> {
        let session = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(session_id.to_string()))?;

        session.disconnect();
        Ok(())
    }

    /// Cria uma sessão a partir de um host do ~/.ssh/config
    pub fn create_from_ssh_config(
        &mut self,
        session_id: SessionId,
        host_pattern: &str,
    ) -> MultiplexerResult<&mut SshCompatibleSession> {
        // Em produção, usaria o SshManager para buscar a config
        // Por enquanto, criamos uma sessão básica

        let session = SshCompatibleSession::new(
            session_id,
            host_pattern,
            std::env::var("USER").unwrap_or_else(|_| "user".to_string()),
            self.default_config.clone(),
        );

        Ok(self
            .sessions
            .entry(session_id)
            .insert_entry(session)
            .into_mut())
    }

    /// Retorna estatísticas de todas as sessões
    pub fn stats(&self) -> SshCompatStats {
        let total = self.sessions.len();
        let connected = self
            .sessions
            .values()
            .filter(|s| s.ssh_info.is_connected)
            .count();

        let total_bytes_sent: u64 = self.sessions.values().map(|s| s.ssh_info.bytes_sent).sum();

        let total_bytes_received: u64 = self
            .sessions
            .values()
            .map(|s| s.ssh_info.bytes_received)
            .sum();

        SshCompatStats {
            total_sessions: total,
            connected_sessions: connected,
            total_bytes_sent,
            total_bytes_received,
        }
    }

    /// Limpa sessões desconectadas
    pub fn cleanup_disconnected(&mut self) -> usize {
        let to_remove: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| !s.ssh_info.is_connected)
            .map(|(id, _)| *id)
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            self.sessions.remove(&id);
        }

        count
    }

    /// Cria uma sessão compartilhada via SSH
    pub fn create_shared_ssh_session(
        &mut self,
        session_id: SessionId,
        host: impl Into<String>,
        user: impl Into<String>,
        _shared_config: SharedSessionConfig,
    ) -> MultiplexerResult<()> {
        // Cria a sessão SSH
        let session = self.create_session(session_id, host, user);
        session.connect()?;

        // Em produção, configuraria o compartilhamento remoto
        // usando o protocolo do multiplexador via SSH

        Ok(())
    }

    /// Verifica conectividade com um host
    pub fn check_host(&self, _host: &str, _port: u16) -> MultiplexerResult<u64> {
        // Em produção, faria um ping/pong real
        // Retorna latência simulada
        Ok(45)
    }

    /// Resolve um host usando ~/.ssh/config
    pub fn resolve_host(&self, host_pattern: &str) -> MultiplexerResult<Option<SshHost>> {
        // Em produção, usaria o SshManager para resolver
        if let Some(ref manager) = self.ssh_manager {
            Ok(manager.get_effective_config(host_pattern))
        } else {
            Ok(None)
        }
    }
}

/// Estatísticas de compatibilidade SSH
#[derive(Debug, Clone)]
pub struct SshCompatStats {
    /// Total de sessões
    pub total_sessions: usize,
    /// Sessões conectadas
    pub connected_sessions: usize,
    /// Total de bytes enviados
    pub total_bytes_sent: u64,
    /// Total de bytes recebidos
    pub total_bytes_received: u64,
}

/// Utilitários de conversão entre formatos
pub mod converters {
    use super::*;

    /// Converte SshHost para configuração de compatibilidade
    pub fn host_to_config(host: &SshHost) -> SshCompatConfig {
        SshCompatConfig {
            use_ssh_config: true,
            use_ssh_agent: true,
            default_identity: host.identity_file.clone(),
            connection_timeout: host.connection_timeout.unwrap_or(30),
            keepalive_interval: host.server_alive_interval.unwrap_or(60),
            strict_host_key_checking: true,
            forward_agent: host.forward_agent.unwrap_or(false),
            forward_x11: host.forward_x11.unwrap_or(false),
            port_forwards: Vec::new(), // Converter se necessário
        }
    }

    /// Cria SshCompatibleSession a partir de SshHost
    pub fn session_from_host(session_id: SessionId, host: &SshHost) -> SshCompatibleSession {
        let config = host_to_config(host);

        let user = host
            .user
            .clone()
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_else(|| "user".to_string());

        let hostname = host
            .host_name
            .clone()
            .unwrap_or_else(|| host.pattern.clone());

        let port = host.port.unwrap_or(22);

        let mut session = SshCompatibleSession::new(session_id, hostname, user, config);

        session.ssh_info.port = port;
        session
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_session_creation() {
        let session_id = SessionId::new();
        let session = SshCompatibleSession::new(
            session_id,
            "example.com",
            "user",
            SshCompatConfig::default(),
        );

        assert_eq!(session.ssh_info.host, "example.com");
        assert_eq!(session.ssh_info.user, "user");
        assert!(!session.ssh_info.is_connected);
    }

    #[test]
    fn test_ssh_session_connect() {
        let mut session = SshCompatibleSession::new(
            SessionId::new(),
            "example.com",
            "user",
            SshCompatConfig::default(),
        );

        assert!(session.connect().is_ok());
        assert!(session.ssh_info.is_connected);
    }

    #[test]
    fn test_ssh_compat_manager() {
        let mut manager = SshCompatManager::new();
        let session_id = SessionId::new();

        manager.create_session(session_id, "example.com", "user");
        assert_eq!(manager.list_sessions().len(), 1);

        manager.connect(session_id).unwrap();
        assert_eq!(manager.list_connected().len(), 1);

        manager.disconnect(session_id).unwrap();
        assert_eq!(manager.list_connected().len(), 0);
    }

    #[test]
    fn test_auth_method_display() {
        assert_eq!(AuthMethod::Password.to_string(), "password");
        assert_eq!(AuthMethod::Agent.to_string(), "agent");
        assert_eq!(AuthMethod::Kerberos.to_string(), "kerberos");
    }

    #[test]
    fn test_transport_display() {
        assert_eq!(SshTransport::Direct.to_string(), "direct");
        assert_eq!(SshTransport::ProxyJump.to_string(), "proxy-jump");
        assert_eq!(SshTransport::SocksProxy.to_string(), "socks-proxy");
    }

    #[test]
    fn test_session_stats() {
        let mut manager = SshCompatManager::new();

        let id1 = SessionId::new();
        manager.create_session(id1, "host1.com", "user");
        manager.connect(id1).unwrap();

        let id2 = SessionId::new();
        manager.create_session(id2, "host2.com", "user");
        // Não conecta id2

        let stats = manager.stats();
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.connected_sessions, 1);
    }

    #[test]
    fn test_cleanup_disconnected() {
        let mut manager = SshCompatManager::new();

        let id1 = SessionId::new();
        manager.create_session(id1, "host1.com", "user");
        manager.connect(id1).unwrap();

        let id2 = SessionId::new();
        manager.create_session(id2, "host2.com", "user");
        // Não conecta

        assert_eq!(manager.cleanup_disconnected(), 1);
        assert_eq!(manager.list_sessions().len(), 1);
    }

    #[test]
    fn test_send_receive_not_connected() {
        let mut session = SshCompatibleSession::new(
            SessionId::new(),
            "example.com",
            "user",
            SshCompatConfig::default(),
        );

        // Não deve funcionar sem conexão
        assert!(session.send(b"test").is_err());
        assert!(session.receive().is_err());
    }

    #[test]
    fn test_display_of_remaining_variants() {
        assert_eq!(SshTransport::Tunnel.to_string(), "tunnel");
        assert_eq!(
            AuthMethod::PublicKey {
                key_path: PathBuf::from("/k/id_ed25519")
            }
            .to_string(),
            "publickey(/k/id_ed25519)"
        );
        assert_eq!(
            AuthMethod::KeyboardInteractive.to_string(),
            "keyboard-interactive"
        );
    }

    #[test]
    fn test_send_receive_and_port_forwards_when_connected() {
        let mut session =
            SshCompatibleSession::new(SessionId::new(), "srv", "ci", SshCompatConfig::default())
                .with_port(2222);
        assert_eq!(session.info().port, 2222);
        session.connect().unwrap();

        session.send(b"ls\n").unwrap();
        session
            .io_buffer
            .lock()
            .unwrap()
            .extend_from_slice(b"saida");
        assert_eq!(session.receive().unwrap(), b"saida");
        assert!(session.receive().unwrap().is_empty());
        assert_eq!(session.info().bytes_sent, 3);
        assert_eq!(session.info().bytes_received, 5);

        session.create_socks_proxy(1080).unwrap();
        session
            .setup_port_forward(PortForwardConfig {
                forward_type: PortForwardType::Local,
                local_port: 8080,
                remote_host: "localhost".to_string(),
                remote_port: 80,
            })
            .unwrap();
    }

    #[test]
    fn test_receive_survives_poisoned_buffer() {
        let mut session =
            SshCompatibleSession::new(SessionId::new(), "srv", "ci", SshCompatConfig::default());
        session.connect().unwrap();
        session.io_buffer.lock().unwrap().push(b'x');
        let buffer = std::sync::Arc::clone(&session.io_buffer);
        let _ = std::thread::spawn(move || {
            let _guard = buffer.lock().unwrap();
            panic!("envenena o mutex");
        })
        .join();
        assert!(session.io_buffer.is_poisoned());
        assert_eq!(session.receive().unwrap(), b"x");
    }

    #[test]
    fn test_manager_builders_and_session_management() {
        let config = SshCompatConfig {
            connection_timeout: 5,
            ..SshCompatConfig::default()
        };
        let mut manager = SshCompatManager::new()
            .with_ssh_manager(SshManager::new())
            .with_default_config(config);
        let id = SessionId::new();
        manager.create_session(id, "host", "user");
        assert_eq!(
            manager.get_session(id).unwrap().config.connection_timeout,
            5
        );
        manager.get_session_mut(id).unwrap().ssh_info.port = 2200;

        manager.connect(id).unwrap();
        assert_eq!(manager.list_connected().len(), 1);
        manager.disconnect(id).unwrap();
        assert!(manager.list_connected().is_empty());

        let ghost = SessionId::new();
        assert!(manager.connect(ghost).is_err());
        assert!(manager.disconnect(ghost).is_err());
        assert!(manager.remove_session(ghost).is_err());

        manager.connect(id).unwrap();
        let removed = manager.remove_session(id).unwrap();
        assert!(!removed.info().is_connected, "remover desconecta");
        assert_eq!(removed.info().port, 2200);
        assert!(manager.list_sessions().is_empty());
    }

    #[test]
    fn test_create_from_config_shared_session_and_host_checks() {
        let mut manager = SshCompatManager::new();
        let id = SessionId::new();
        let session = manager.create_from_ssh_config(id, "bastion").unwrap();
        assert_eq!(session.info().host, "bastion");

        let shared = SessionId::new();
        manager
            .create_shared_ssh_session(shared, "db", "admin", SharedSessionConfig::default())
            .unwrap();
        assert!(manager.get_session(shared).unwrap().info().is_connected);

        assert_eq!(manager.check_host("db", 22).unwrap(), 45);
        // Sem SshManager configurado não há como resolver
        assert!(manager.resolve_host("db").unwrap().is_none());
        let with_manager = SshCompatManager::new().with_ssh_manager(SshManager::new());
        assert!(with_manager
            .resolve_host("nao-configurado")
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_converters_use_host_settings() {
        let host = SshHost {
            pattern: "prod".to_string(),
            host_name: Some("10.0.0.5".to_string()),
            user: Some("deploy".to_string()),
            port: Some(2022),
            identity_file: Some(PathBuf::from("/keys/prod")),
            forward_agent: Some(true),
            forward_x11: Some(true),
            server_alive_interval: Some(15),
            connection_timeout: Some(7),
            ..SshHost::default()
        };
        let config = converters::host_to_config(&host);
        assert_eq!(config.default_identity, Some(PathBuf::from("/keys/prod")));
        assert_eq!(
            (config.connection_timeout, config.keepalive_interval),
            (7, 15)
        );
        assert!(config.forward_agent && config.forward_x11);

        let session = converters::session_from_host(SessionId::new(), &host);
        assert_eq!(session.info().host, "10.0.0.5");
        assert_eq!(session.info().user, "deploy");
        assert_eq!(session.info().port, 2022);

        // Sem host_name/porta: usa o padrão e a porta 22
        let minimal = SshHost {
            pattern: "alias".to_string(),
            ..SshHost::default()
        };
        let session = converters::session_from_host(SessionId::new(), &minimal);
        assert_eq!(session.info().host, "alias");
        assert_eq!(session.info().port, 22);
        let config = converters::host_to_config(&minimal);
        assert_eq!(
            (config.connection_timeout, config.keepalive_interval),
            (30, 60)
        );
    }
}
