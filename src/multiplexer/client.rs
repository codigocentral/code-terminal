//! Multiplexer Client - Cliente conectado ao servidor

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use uuid::Uuid;

/// ID único de um cliente
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClientId(pub Uuid);

impl ClientId {
    /// Cria um novo ID de cliente
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ClientId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string().split('-').next().unwrap_or(""))
    }
}

/// Informações de uma conexão de cliente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConnection {
    /// ID único do cliente
    pub id: ClientId,
    /// Endereço do cliente
    pub address: Option<SocketAddr>,
    /// PID do processo cliente
    pub pid: Option<u32>,
    /// Nome do terminal
    pub terminal_name: String,
    /// Tamanho do terminal (cols, rows)
    pub terminal_size: (u16, u16),
    /// Quando conectou
    pub connected_at: chrono::DateTime<chrono::Utc>,
    /// Sessão atual (se houver)
    pub current_session: Option<super::session::SessionId>,
    /// Se está em modo de leitura apenas
    pub read_only: bool,
    /// Se é o cliente controlador (master)
    pub is_controller: bool,
}

impl ClientConnection {
    /// Cria uma nova conexão de cliente
    pub fn new(terminal_size: (u16, u16)) -> Self {
        Self {
            id: ClientId::new(),
            address: None,
            pid: None,
            terminal_name: std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_string()),
            terminal_size,
            connected_at: chrono::Utc::now(),
            current_session: None,
            read_only: false,
            is_controller: false,
        }
    }

    /// Define o endereço
    pub fn with_address(mut self, addr: SocketAddr) -> Self {
        self.address = Some(addr);
        self
    }

    /// Define o PID
    pub fn with_pid(mut self, pid: u32) -> Self {
        self.pid = Some(pid);
        self
    }

    /// Define como leitura apenas
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// Define como controlador
    pub fn as_controller(mut self) -> Self {
        self.is_controller = true;
        self
    }

    /// Anexa a uma sessão
    pub fn attach(&mut self, session_id: super::session::SessionId) {
        self.current_session = Some(session_id);
    }

    /// Desanexa da sessão atual
    pub fn detach(&mut self) {
        self.current_session = None;
    }

    /// Retorna se está anexado a uma sessão
    pub fn is_attached(&self) -> bool {
        self.current_session.is_some()
    }

    /// Retorna o tempo de conexão
    pub fn connection_time(&self) -> chrono::DateTime<chrono::Utc> {
        self.connected_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_id_creation() {
        let id1 = ClientId::new();
        let id2 = ClientId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_client_creation() {
        let client = ClientConnection::new((80, 24));
        assert_eq!(client.terminal_size, (80, 24));
        assert!(!client.read_only);
        assert!(!client.is_controller);
        assert!(!client.is_attached());
    }

    #[test]
    fn test_client_attach_detach() {
        let mut client = ClientConnection::new((80, 24));
        let session_id = super::super::session::SessionId::new();

        client.attach(session_id);
        assert!(client.is_attached());
        assert_eq!(client.current_session, Some(session_id));

        client.detach();
        assert!(!client.is_attached());
        assert_eq!(client.current_session, None);
    }
}
