//! Shared Sessions - Sessões compartilhadas entre múltiplos clientes
//!
//! Este módulo implementa sessões compartilhadas onde múltiplos clientes
//! podem se conectar simultaneamente, visualizando e interagindo com a
//! mesma sessão (similar ao screen/tmux attach).

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use super::client::ClientId;
use super::session::SessionId;
use super::{MultiplexerError, MultiplexerResult};

/// Permissões de um cliente em uma sessão compartilhada
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientPermissions {
    /// Pode escrever (enviar input)
    pub write: bool,
    /// Pode redimensionar
    pub resize: bool,
    /// Pode executar comandos
    pub execute_commands: bool,
    /// Pode gerenciar outros clientes
    pub manage_clients: bool,
    /// É somente leitura
    pub read_only: bool,
}

impl Default for ClientPermissions {
    fn default() -> Self {
        Self {
            write: true,
            resize: true,
            execute_commands: true,
            manage_clients: false,
            read_only: false,
        }
    }
}

impl ClientPermissions {
    /// Cria permissões de somente leitura
    pub fn read_only() -> Self {
        Self {
            write: false,
            resize: false,
            execute_commands: false,
            manage_clients: false,
            read_only: true,
        }
    }

    /// Cria permissões de administrador
    pub fn admin() -> Self {
        Self {
            write: true,
            resize: true,
            execute_commands: true,
            manage_clients: true,
            read_only: false,
        }
    }
}

/// Um cliente em uma sessão compartilhada
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedClient {
    /// ID do cliente
    pub client_id: ClientId,
    /// Permissões
    pub permissions: ClientPermissions,
    /// Quando entrou na sessão
    pub joined_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Se está atualmente ativo
    pub is_active: bool,
}

impl SharedClient {
    /// Cria um novo cliente compartilhado
    pub fn new(client_id: ClientId, permissions: ClientPermissions) -> Self {
        let now = chrono::Utc::now();
        Self {
            client_id,
            permissions,
            joined_at: now,
            last_activity: now,
            is_active: true,
        }
    }

    /// Atualiza a última atividade
    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now();
    }
}

/// Modo de sincronização de input
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    /// Input independente (cada cliente controla seu cursor)
    Independent,
    /// Input sincronizado (todos os clientes recebem o mesmo input)
    Synchronized,
    /// Input broadcast (input de um cliente vai para todos)
    Broadcast,
}

impl std::fmt::Display for SyncMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SyncMode::Independent => write!(f, "independent"),
            SyncMode::Synchronized => write!(f, "synchronized"),
            SyncMode::Broadcast => write!(f, "broadcast"),
        }
    }
}

/// Configuração de sessão compartilhada
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSessionConfig {
    /// Máximo de clientes simultâneos
    pub max_clients: usize,
    /// Modo de sincronização
    pub sync_mode: SyncMode,
    /// Permissões padrão para novos clientes
    pub default_permissions: ClientPermissions,
    /// Se requer senha para entrar
    pub require_password: bool,
    /// Hash da senha (se houver)
    pub password_hash: Option<String>,
    /// Se permite clientes somente leitura
    pub allow_readonly: bool,
    /// Timeout de inatividade (segundos)
    pub idle_timeout: Option<u64>,
}

impl Default for SharedSessionConfig {
    fn default() -> Self {
        Self {
            max_clients: 10,
            sync_mode: SyncMode::Independent,
            default_permissions: ClientPermissions::default(),
            require_password: false,
            password_hash: None,
            allow_readonly: true,
            idle_timeout: Some(3600), // 1 hora
        }
    }
}

impl SharedSessionConfig {
    /// Define o máximo de clientes
    pub fn with_max_clients(mut self, max: usize) -> Self {
        self.max_clients = max;
        self
    }

    /// Define o modo de sincronização
    pub fn with_sync_mode(mut self, mode: SyncMode) -> Self {
        self.sync_mode = mode;
        self
    }

    /// Define permissões padrão
    pub fn with_default_permissions(mut self, perms: ClientPermissions) -> Self {
        self.default_permissions = perms;
        self
    }

    /// Habilita senha
    pub fn with_password(mut self, password: impl Into<String>) -> Self {
        // Em produção, usar hash adequado (bcrypt/argon2)
        self.require_password = true;
        self.password_hash = Some(password.into());
        self
    }
}

/// Informações de uma sessão compartilhada
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedSessionInfo {
    /// ID da sessão
    pub session_id: SessionId,
    /// Número de clientes conectados
    pub client_count: usize,
    /// Capacidade máxima
    pub max_clients: usize,
    /// Modo de sincronização
    pub sync_mode: SyncMode,
    /// Se requer senha
    pub password_protected: bool,
    /// Quando foi criada
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Uma sessão compartilhada
#[derive(Debug)]
pub struct SharedSession {
    /// ID da sessão
    pub session_id: SessionId,
    /// Clientes conectados
    clients: Arc<RwLock<HashMap<ClientId, SharedClient>>>,
    /// Configuração
    pub config: SharedSessionConfig,
    /// Buffer de input para sincronização
    input_buffer: Arc<Mutex<Vec<SharedInputEvent>>>,
    /// Quando foi criada
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    last_activity: Arc<RwLock<chrono::DateTime<chrono::Utc>>>,
}

/// Evento de input compartilhado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedInputEvent {
    /// ID do cliente que enviou
    pub client_id: ClientId,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Dados do input
    pub data: InputData,
}

/// Dados de input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputData {
    /// Input de teclado
    Keyboard { bytes: Vec<u8> },
    /// Input de mouse
    Mouse { x: u16, y: u16, button: u8 },
    /// Comando
    Command { command: String, args: Vec<String> },
    /// Redimensionamento
    Resize { cols: u16, rows: u16 },
}

impl SharedSession {
    /// Cria uma nova sessão compartilhada
    pub fn new(session_id: SessionId, config: SharedSessionConfig) -> Self {
        let now = chrono::Utc::now();
        Self {
            session_id,
            clients: Arc::new(RwLock::new(HashMap::new())),
            config,
            input_buffer: Arc::new(Mutex::new(Vec::new())),
            created_at: now,
            last_activity: Arc::new(RwLock::new(now)),
        }
    }

    /// Adiciona um cliente à sessão
    pub fn join(
        &self,
        client_id: ClientId,
        password: Option<&str>,
    ) -> MultiplexerResult<ClientPermissions> {
        // Verifica senha
        if self.config.require_password {
            let provided = password.unwrap_or("");
            let expected = self.config.password_hash.as_deref().unwrap_or("");
            if provided != expected {
                return Err(MultiplexerError::ServerError(
                    "Invalid password".to_string(),
                ));
            }
        }

        // Verifica capacidade e adiciona sob o mesmo lock de escrita: com a
        // checagem num read lock separado, dois joins simultâneos passavam do
        // limite. Um cliente que já está na sessão pode reentrar.
        let mut clients = self.clients.write();
        if !clients.contains_key(&client_id) && clients.len() >= self.config.max_clients {
            return Err(MultiplexerError::ServerError("Session is full".to_string()));
        }

        let permissions = self.config.default_permissions;
        let shared_client = SharedClient::new(client_id, permissions);

        clients.insert(client_id, shared_client);
        self.touch();

        Ok(permissions)
    }

    /// Remove um cliente da sessão
    pub fn leave(&self, client_id: ClientId) -> MultiplexerResult<()> {
        let mut clients = self.clients.write();

        clients
            .remove(&client_id)
            .ok_or_else(|| MultiplexerError::ClientNotFound(client_id.to_string()))?;

        self.touch();
        Ok(())
    }

    /// Obtém informações de um cliente
    pub fn get_client(&self, client_id: ClientId) -> Option<SharedClient> {
        self.clients.read().get(&client_id).cloned()
    }

    /// Lista todos os clientes
    pub fn list_clients(&self) -> Vec<SharedClient> {
        self.clients.read().values().cloned().collect()
    }

    /// Retorna o número de clientes
    pub fn client_count(&self) -> usize {
        self.clients.read().len()
    }

    /// Verifica se um cliente está na sessão
    pub fn has_client(&self, client_id: ClientId) -> bool {
        self.clients.read().contains_key(&client_id)
    }

    /// Atualiza permissões de um cliente
    pub fn update_permissions(
        &self,
        client_id: ClientId,
        permissions: ClientPermissions,
    ) -> MultiplexerResult<()> {
        let mut clients = self.clients.write();

        let client = clients
            .get_mut(&client_id)
            .ok_or_else(|| MultiplexerError::ClientNotFound(client_id.to_string()))?;

        client.permissions = permissions;
        client.touch();
        self.touch();

        Ok(())
    }

    /// Verifica se um cliente tem uma permissão específica
    pub fn has_permission(
        &self,
        client_id: ClientId,
        check: impl Fn(&ClientPermissions) -> bool,
    ) -> bool {
        self.clients
            .read()
            .get(&client_id)
            .map(|c| check(&c.permissions))
            .unwrap_or(false)
    }

    /// Envia input para a sessão
    pub fn send_input(&self, client_id: ClientId, data: InputData) -> MultiplexerResult<()> {
        // Verifica se o cliente tem permissão de escrita
        if !self.has_permission(client_id, |p| p.write) {
            return Err(MultiplexerError::ServerError(
                "No write permission".to_string(),
            ));
        }

        let event = SharedInputEvent {
            client_id,
            timestamp: chrono::Utc::now(),
            data,
        };

        let mut buffer = self.input_buffer.lock();
        buffer.push(event);

        // Limita o tamanho do buffer
        if buffer.len() > 1000 {
            buffer.remove(0);
        }

        // Atualiza atividade do cliente
        if let Some(client) = self.clients.write().get_mut(&client_id) {
            client.touch();
        }

        self.touch();
        Ok(())
    }

    /// Obtém e limpa o buffer de input
    pub fn drain_input(&self) -> Vec<SharedInputEvent> {
        let mut buffer = self.input_buffer.lock();
        std::mem::take(&mut *buffer)
    }

    /// Obtém informações da sessão
    pub fn info(&self) -> SharedSessionInfo {
        SharedSessionInfo {
            session_id: self.session_id,
            client_count: self.client_count(),
            max_clients: self.config.max_clients,
            sync_mode: self.config.sync_mode,
            password_protected: self.config.require_password,
            created_at: self.created_at,
            last_activity: *self.last_activity.read(),
        }
    }

    /// Verifica se a sessão está vazia
    pub fn is_empty(&self) -> bool {
        self.client_count() == 0
    }

    /// Atualiza a última atividade
    fn touch(&self) {
        *self.last_activity.write() = chrono::Utc::now();
    }

    /// Verifica clientes inativos
    pub fn check_idle_clients(&self) -> Vec<ClientId> {
        let timeout = match self.config.idle_timeout {
            Some(t) => chrono::Duration::seconds(t as i64),
            None => return Vec::new(),
        };

        let now = chrono::Utc::now();
        let clients = self.clients.read();

        clients
            .values()
            .filter(|c| now.signed_duration_since(c.last_activity) > timeout)
            .map(|c| c.client_id)
            .collect()
    }

    /// Expulsa um cliente
    pub fn kick_client(
        &self,
        client_id: ClientId,
        reason: impl Into<String>,
    ) -> MultiplexerResult<()> {
        let mut clients = self.clients.write();

        clients
            .remove(&client_id)
            .ok_or_else(|| MultiplexerError::ClientNotFound(client_id.to_string()))?;

        self.touch();

        // Em produção, notificaria o cliente do motivo
        let _ = reason.into();

        Ok(())
    }
}

/// Gerenciador de sessões compartilhadas
#[derive(Debug, Default)]
pub struct SharedSessionManager {
    /// Sessões compartilhadas por ID
    sessions: HashMap<SessionId, Arc<SharedSession>>,
    /// Mapeamento cliente -> sessão
    client_sessions: HashMap<ClientId, SessionId>,
}

impl SharedSessionManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            client_sessions: HashMap::new(),
        }
    }

    /// Cria uma nova sessão compartilhada
    pub fn create_shared_session(
        &mut self,
        session_id: SessionId,
        config: SharedSessionConfig,
    ) -> Arc<SharedSession> {
        let session = Arc::new(SharedSession::new(session_id, config));
        self.sessions.insert(session_id, Arc::clone(&session));
        session
    }

    /// Obtém uma sessão compartilhada
    pub fn get_session(&self, session_id: SessionId) -> Option<Arc<SharedSession>> {
        self.sessions.get(&session_id).cloned()
    }

    /// Remove uma sessão compartilhada
    pub fn remove_session(
        &mut self,
        session_id: SessionId,
    ) -> MultiplexerResult<Arc<SharedSession>> {
        let session = self
            .sessions
            .remove(&session_id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(session_id.to_string()))?;

        // Remove mapeamentos de clientes
        let clients: Vec<_> = session.list_clients();
        for client in clients {
            self.client_sessions.remove(&client.client_id);
        }

        Ok(session)
    }

    /// Conecta um cliente a uma sessão
    pub fn connect_client(
        &mut self,
        client_id: ClientId,
        session_id: SessionId,
        password: Option<&str>,
    ) -> MultiplexerResult<ClientPermissions> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(session_id.to_string()))?;

        // Entra na nova sessão primeiro: se o join falhar (senha, lotação)
        // o cliente continua na sessão anterior, em vez de ficar fora de
        // todas com o mapeamento ainda apontando para a antiga.
        let permissions = session.join(client_id, password)?;

        if let Some(prev_session) = self.client_sessions.insert(client_id, session_id) {
            if prev_session != session_id {
                if let Some(prev) = self.sessions.get(&prev_session) {
                    let _ = prev.leave(client_id);
                }
            }
        }

        Ok(permissions)
    }

    /// Desconecta um cliente
    pub fn disconnect_client(&mut self, client_id: ClientId) -> MultiplexerResult<()> {
        let session_id = self
            .client_sessions
            .remove(&client_id)
            .ok_or_else(|| MultiplexerError::ClientNotFound(client_id.to_string()))?;

        if let Some(session) = self.sessions.get(&session_id) {
            session.leave(client_id)?;

            // Remove sessão se ficou vazia
            if session.is_empty() {
                self.sessions.remove(&session_id);
            }
        }

        Ok(())
    }

    /// Obtém a sessão de um cliente
    pub fn get_client_session(&self, client_id: ClientId) -> Option<Arc<SharedSession>> {
        let session_id = self.client_sessions.get(&client_id)?;
        self.sessions.get(session_id).cloned()
    }

    /// Lista todas as sessões compartilhadas
    pub fn list_sessions(&self) -> Vec<SharedSessionInfo> {
        self.sessions.values().map(|s| s.info()).collect()
    }

    /// Lista clientes de uma sessão
    pub fn list_session_clients(
        &self,
        session_id: SessionId,
    ) -> MultiplexerResult<Vec<SharedClient>> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(session_id.to_string()))?;

        Ok(session.list_clients())
    }

    /// Verifica se um cliente está em alguma sessão
    pub fn is_client_connected(&self, client_id: ClientId) -> bool {
        self.client_sessions.contains_key(&client_id)
    }

    /// Retorna o número de sessões
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Limpa sessões vazias
    pub fn cleanup_empty_sessions(&mut self) -> usize {
        let empty: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.is_empty())
            .map(|(id, _)| *id)
            .collect();

        let count = empty.len();
        for id in empty {
            self.sessions.remove(&id);
        }

        count
    }

    /// Expulsa um cliente de uma sessão
    pub fn kick_client_from_session(
        &mut self,
        session_id: SessionId,
        client_id: ClientId,
        reason: impl Into<String>,
    ) -> MultiplexerResult<()> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(session_id.to_string()))?;

        session.kick_client(client_id, reason)?;
        self.client_sessions.remove(&client_id);

        // Remove sessão se ficou vazia
        if session.is_empty() {
            self.sessions.remove(&session_id);
        }

        Ok(())
    }

    /// Verifica clientes inativos em todas as sessões
    pub fn check_all_idle_clients(&self) -> HashMap<SessionId, Vec<ClientId>> {
        let mut idle = HashMap::new();

        for (id, session) in &self.sessions {
            let idle_clients = session.check_idle_clients();
            if !idle_clients.is_empty() {
                idle.insert(*id, idle_clients);
            }
        }

        idle
    }

    /// Expulsa todos os clientes inativos
    pub fn kick_idle_clients(&mut self) -> usize {
        let idle = self.check_all_idle_clients();
        let mut count = 0;

        for (session_id, clients) in idle {
            for client_id in clients {
                let _ = self.kick_client_from_session(session_id, client_id, "Idle timeout");
                count += 1;
            }
        }

        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_session_creation() {
        let session_id = SessionId::new();
        let session = SharedSession::new(session_id, SharedSessionConfig::default());

        assert_eq!(session.session_id, session_id);
        assert!(session.is_empty());
        assert_eq!(session.config.max_clients, 10);
    }

    #[test]
    fn test_client_join_leave() {
        let session_id = SessionId::new();
        let session = SharedSession::new(session_id, SharedSessionConfig::default());

        let client_id = ClientId::new();
        let perms = session.join(client_id, None).unwrap();

        assert!(perms.write);
        assert_eq!(session.client_count(), 1);

        session.leave(client_id).unwrap();
        assert!(session.is_empty());
    }

    #[test]
    fn test_password_protection() {
        let config = SharedSessionConfig::default().with_password("secret123");

        let session = SharedSession::new(SessionId::new(), config);
        let client_id = ClientId::new();

        // Sem senha deve falhar
        assert!(session.join(client_id, None).is_err());

        // Com senha errada deve falhar
        assert!(session.join(client_id, Some("wrong")).is_err());

        // Com senha correta deve funcionar
        assert!(session.join(client_id, Some("secret123")).is_ok());
    }

    #[test]
    fn test_max_clients() {
        let config = SharedSessionConfig::default().with_max_clients(2);

        let session = SharedSession::new(SessionId::new(), config);

        let client1 = ClientId::new();
        let client2 = ClientId::new();
        let client3 = ClientId::new();

        assert!(session.join(client1, None).is_ok());
        assert!(session.join(client2, None).is_ok());
        assert!(session.join(client3, None).is_err()); // Deve falhar, sessão cheia
    }

    #[test]
    fn test_shared_session_manager() {
        let mut manager = SharedSessionManager::new();
        let session_id = SessionId::new();

        // Cria sessão
        manager.create_shared_session(session_id, SharedSessionConfig::default());
        assert_eq!(manager.session_count(), 1);

        // Conecta cliente
        let client_id = ClientId::new();
        manager.connect_client(client_id, session_id, None).unwrap();

        assert!(manager.is_client_connected(client_id));
        assert_eq!(manager.list_session_clients(session_id).unwrap().len(), 1);

        // Desconecta cliente
        manager.disconnect_client(client_id).unwrap();
        assert!(!manager.is_client_connected(client_id));
    }

    #[test]
    fn test_permissions() {
        let session = SharedSession::new(SessionId::new(), SharedSessionConfig::default());
        let client_id = ClientId::new();

        session.join(client_id, None).unwrap();

        assert!(session.has_permission(client_id, |p| p.write));

        // Atualiza para somente leitura
        session
            .update_permissions(client_id, ClientPermissions::read_only())
            .unwrap();

        assert!(!session.has_permission(client_id, |p| p.write));
        assert!(session.has_permission(client_id, |p| p.read_only));
    }

    #[test]
    fn test_input_buffer() {
        let session = SharedSession::new(SessionId::new(), SharedSessionConfig::default());
        let client_id = ClientId::new();

        session.join(client_id, None).unwrap();

        let input = InputData::Keyboard { bytes: vec![0x41] };
        session.send_input(client_id, input.clone()).unwrap();

        let events = session.drain_input();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].data, InputData::Keyboard { .. }));
    }

    #[test]
    fn test_readonly_no_write_permission() {
        let config =
            SharedSessionConfig::default().with_default_permissions(ClientPermissions::read_only());

        let session = SharedSession::new(SessionId::new(), config);
        let client_id = ClientId::new();

        session.join(client_id, None).unwrap();

        let input = InputData::Keyboard { bytes: vec![0x41] };
        assert!(session.send_input(client_id, input).is_err());
    }

    fn make_idle(session: &SharedSession, client_id: ClientId) {
        if let Some(client) = session.clients.write().get_mut(&client_id) {
            client.last_activity = chrono::Utc::now() - chrono::Duration::hours(2);
        }
    }

    #[test]
    fn test_config_builders_and_display() {
        assert_eq!(SyncMode::Independent.to_string(), "independent");
        assert_eq!(SyncMode::Synchronized.to_string(), "synchronized");
        assert_eq!(SyncMode::Broadcast.to_string(), "broadcast");

        let config = SharedSessionConfig::default()
            .with_max_clients(2)
            .with_sync_mode(SyncMode::Broadcast)
            .with_default_permissions(ClientPermissions::read_only());
        let session = SharedSession::new(SessionId::new(), config);
        let info = session.info();
        assert_eq!(info.max_clients, 2);
        assert_eq!(info.sync_mode, SyncMode::Broadcast);
        assert!(!info.password_protected);
        assert!(!session.join(ClientId::new(), None).unwrap().write);
    }

    #[test]
    fn test_client_queries_and_kick() {
        let session = SharedSession::new(SessionId::new(), SharedSessionConfig::default());
        let client = ClientId::new();
        session.join(client, None).unwrap();
        assert!(session.has_client(client));
        assert_eq!(session.get_client(client).unwrap().client_id, client);
        assert!(session.get_client(ClientId::new()).is_none());

        session.kick_client(client, "teste").unwrap();
        assert!(!session.has_client(client));
        assert!(matches!(
            session.kick_client(client, "de novo"),
            Err(MultiplexerError::ClientNotFound(_))
        ));
        assert!(session.leave(client).is_err());
        assert!(session
            .update_permissions(client, ClientPermissions::admin())
            .is_err());
    }

    /// Regressão: reentrar com a sessão cheia era recusado, e a checagem de
    /// lotação feita fora do lock de escrita permitia passar do limite.
    #[test]
    fn test_rejoin_when_full_and_concurrent_joins_respect_limit() {
        let session = Arc::new(SharedSession::new(
            SessionId::new(),
            SharedSessionConfig::default().with_max_clients(1),
        ));
        let client = ClientId::new();
        session.join(client, None).unwrap();
        assert!(
            session.join(client, None).is_ok(),
            "mesmo cliente pode reentrar"
        );
        assert_eq!(session.client_count(), 1);

        let session = Arc::new(SharedSession::new(
            SessionId::new(),
            SharedSessionConfig::default().with_max_clients(4),
        ));
        let handles: Vec<_> = (0..16)
            .map(|_| {
                let session = Arc::clone(&session);
                std::thread::spawn(move || session.join(ClientId::new(), None).is_ok())
            })
            .collect();
        let joined = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|ok| *ok)
            .count();
        assert_eq!(joined, 4);
        assert_eq!(session.client_count(), 4);
    }

    #[test]
    fn test_input_buffer_is_capped() {
        let session = SharedSession::new(SessionId::new(), SharedSessionConfig::default());
        let client = ClientId::new();
        session.join(client, None).unwrap();
        for i in 0..1005u16 {
            session
                .send_input(client, InputData::Resize { cols: i, rows: 1 })
                .unwrap();
        }
        let events = session.drain_input();
        assert_eq!(events.len(), 1000);
        assert!(matches!(events[0].data, InputData::Resize { cols: 5, .. }));
        assert!(session.drain_input().is_empty());
    }

    #[test]
    fn test_idle_clients_detection() {
        let session = SharedSession::new(SessionId::new(), SharedSessionConfig::default());
        let idle = ClientId::new();
        let fresh = ClientId::new();
        session.join(idle, None).unwrap();
        session.join(fresh, None).unwrap();
        make_idle(&session, idle);
        assert_eq!(session.check_idle_clients(), vec![idle]);

        let config = SharedSessionConfig {
            idle_timeout: None,
            ..SharedSessionConfig::default()
        };
        let no_timeout = SharedSession::new(SessionId::new(), config);
        no_timeout.join(idle, None).unwrap();
        make_idle(&no_timeout, idle);
        assert!(no_timeout.check_idle_clients().is_empty());
    }

    #[test]
    fn test_manager_session_lifecycle() {
        let mut manager = SharedSessionManager::default();
        let id = SessionId::new();
        manager.create_shared_session(id, SharedSessionConfig::default());
        assert!(manager.get_session(id).is_some());
        assert_eq!(manager.list_sessions().len(), 1);

        let client = ClientId::new();
        manager.connect_client(client, id, None).unwrap();
        assert!(manager.is_client_connected(client));
        assert_eq!(manager.get_client_session(client).unwrap().session_id, id);
        assert_eq!(manager.list_session_clients(id).unwrap().len(), 1);

        let removed = manager.remove_session(id).unwrap();
        assert_eq!(removed.session_id, id);
        assert!(!manager.is_client_connected(client));
        assert!(manager.get_client_session(client).is_none());
        assert!(manager.remove_session(id).is_err());
        assert!(manager.list_session_clients(id).is_err());
        assert!(manager.connect_client(client, id, None).is_err());
        assert!(manager.disconnect_client(client).is_err());
    }

    #[test]
    fn test_connect_moves_client_between_sessions() {
        let mut manager = SharedSessionManager::new();
        let a = SessionId::new();
        let b = SessionId::new();
        manager.create_shared_session(a, SharedSessionConfig::default());
        manager.create_shared_session(b, SharedSessionConfig::default());
        let client = ClientId::new();

        manager.connect_client(client, a, None).unwrap();
        manager.connect_client(client, b, None).unwrap();
        assert!(!manager.get_session(a).unwrap().has_client(client));
        assert!(manager.get_session(b).unwrap().has_client(client));

        // Reconectar na mesma sessão não tira o cliente dela
        manager.connect_client(client, b, None).unwrap();
        assert!(manager.get_session(b).unwrap().has_client(client));
    }

    /// Regressão: um connect que falhava (senha errada) já tinha tirado o
    /// cliente da sessão anterior, deixando o mapeamento inconsistente.
    #[test]
    fn test_failed_connect_keeps_previous_session() {
        let mut manager = SharedSessionManager::new();
        let open = SessionId::new();
        let locked = SessionId::new();
        manager.create_shared_session(open, SharedSessionConfig::default());
        manager.create_shared_session(locked, SharedSessionConfig::default().with_password("x"));
        let client = ClientId::new();
        manager.connect_client(client, open, None).unwrap();

        assert!(manager
            .connect_client(client, locked, Some("errada"))
            .is_err());
        assert!(manager.get_session(open).unwrap().has_client(client));
        assert_eq!(manager.get_client_session(client).unwrap().session_id, open);
    }

    #[test]
    fn test_cleanup_kick_and_idle_across_sessions() {
        let mut manager = SharedSessionManager::new();
        let empty = SessionId::new();
        let busy = SessionId::new();
        manager.create_shared_session(empty, SharedSessionConfig::default());
        manager.create_shared_session(busy, SharedSessionConfig::default());
        let a = ClientId::new();
        let b = ClientId::new();
        manager.connect_client(a, busy, None).unwrap();
        manager.connect_client(b, busy, None).unwrap();

        assert_eq!(manager.cleanup_empty_sessions(), 1);
        assert_eq!(manager.session_count(), 1);

        manager.kick_client_from_session(busy, a, "fora").unwrap();
        assert!(!manager.is_client_connected(a));
        assert!(manager
            .kick_client_from_session(SessionId::new(), b, "x")
            .is_err());

        make_idle(&manager.get_session(busy).unwrap(), b);
        let idle = manager.check_all_idle_clients();
        assert_eq!(idle.get(&busy), Some(&vec![b]));
        assert_eq!(manager.kick_idle_clients(), 1);
        // A sessão ficou vazia e foi removida
        assert_eq!(manager.session_count(), 0);
        assert!(manager.check_all_idle_clients().is_empty());
    }

    #[test]
    fn test_disconnect_removes_empty_session() {
        let mut manager = SharedSessionManager::new();
        let id = SessionId::new();
        manager.create_shared_session(id, SharedSessionConfig::default());
        let client = ClientId::new();
        manager.connect_client(client, id, None).unwrap();
        manager.disconnect_client(client).unwrap();
        assert_eq!(manager.session_count(), 0);
    }
}
