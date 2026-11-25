//! Session Server - Servidor de sessões persistentes

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::client::{ClientConnection, ClientId};
use super::session::{MultiplexerSession, SessionId};
use super::{MultiplexerError, MultiplexerResult};

/// Servidor de sessões
#[derive(Debug)]
pub struct SessionServer {
    /// Sessões ativas
    sessions: Arc<RwLock<HashMap<SessionId, MultiplexerSession>>>,
    /// Clientes conectados
    clients: Arc<RwLock<HashMap<ClientId, ClientConnection>>>,
}

impl SessionServer {
    /// Cria um novo servidor de sessões
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Cria uma nova sessão
    pub fn create_session(
        &self,
        name: Option<String>,
        size: (u16, u16),
    ) -> MultiplexerResult<SessionId> {
        let session = MultiplexerSession::new(name, size);
        let id = session.id;

        let mut sessions = self.sessions.write();
        sessions.insert(id, session);

        Ok(id)
    }

    /// Obtém uma sessão pelo ID
    pub fn get_session(&self, id: SessionId) -> MultiplexerResult<MultiplexerSession> {
        let sessions = self.sessions.read();
        sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| MultiplexerError::SessionNotFound(id.to_string()))
    }

    /// Remove uma sessão
    pub fn remove_session(&self, id: SessionId) -> MultiplexerResult<()> {
        let mut sessions = self.sessions.write();
        sessions
            .remove(&id)
            .map(|_| ())
            .ok_or_else(|| MultiplexerError::SessionNotFound(id.to_string()))
    }

    /// Lista todas as sessões
    pub fn list_sessions(&self) -> Vec<MultiplexerSession> {
        let sessions = self.sessions.read();
        sessions.values().cloned().collect()
    }

    /// Adiciona um cliente
    pub fn add_client(&self, client: ClientConnection) -> MultiplexerResult<ClientId> {
        let id = client.id;
        let mut clients = self.clients.write();
        clients.insert(id, client);
        Ok(id)
    }

    /// Remove um cliente
    pub fn remove_client(&self, id: ClientId) -> MultiplexerResult<()> {
        let mut clients = self.clients.write();
        clients
            .remove(&id)
            .map(|_| ())
            .ok_or_else(|| MultiplexerError::ClientNotFound(id.to_string()))
    }

    /// Obtém informações de um cliente
    pub fn get_client(&self, id: ClientId) -> MultiplexerResult<ClientConnection> {
        let clients = self.clients.read();
        clients
            .get(&id)
            .cloned()
            .ok_or_else(|| MultiplexerError::ClientNotFound(id.to_string()))
    }

    /// Anexa um cliente a uma sessão
    pub fn attach_client(
        &self,
        client_id: ClientId,
        session_id: SessionId,
    ) -> MultiplexerResult<()> {
        let mut sessions = self.sessions.write();

        if let Some(session) = sessions.get_mut(&session_id) {
            session.add_client();

            let mut clients = self.clients.write();
            if let Some(client) = clients.get_mut(&client_id) {
                client.attach(session_id);
                Ok(())
            } else {
                Err(MultiplexerError::ClientNotFound(client_id.to_string()))
            }
        } else {
            Err(MultiplexerError::SessionNotFound(session_id.to_string()))
        }
    }

    /// Desanexa um cliente da sessão atual
    pub fn detach_client(&self, client_id: ClientId) -> MultiplexerResult<()> {
        let mut clients = self.clients.write();

        if let Some(client) = clients.get_mut(&client_id) {
            if let Some(session_id) = client.current_session {
                client.detach();

                drop(clients); // Release lock

                let mut sessions = self.sessions.write();
                if let Some(session) = sessions.get_mut(&session_id) {
                    session.remove_client();
                }
                Ok(())
            } else {
                Err(MultiplexerError::NotAttached)
            }
        } else {
            Err(MultiplexerError::ClientNotFound(client_id.to_string()))
        }
    }

    /// Renomeia uma sessão
    pub fn rename_session(&self, id: SessionId, new_name: String) -> MultiplexerResult<()> {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get_mut(&id) {
            session.rename(new_name);
            Ok(())
        } else {
            Err(MultiplexerError::SessionNotFound(id.to_string()))
        }
    }
}

impl Default for SessionServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_create_session() {
        let server = SessionServer::new();
        let id = server
            .create_session(Some("test".to_string()), (80, 24))
            .unwrap();

        let session = server.get_session(id).unwrap();
        assert_eq!(session.name, "test");
    }

    #[test]
    fn test_server_list_sessions() {
        let server = SessionServer::new();
        server
            .create_session(Some("test1".to_string()), (80, 24))
            .unwrap();
        server
            .create_session(Some("test2".to_string()), (80, 24))
            .unwrap();

        let sessions = server.list_sessions();
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_server_remove_session() {
        let server = SessionServer::new();
        let id = server
            .create_session(Some("test".to_string()), (80, 24))
            .unwrap();

        server.remove_session(id).unwrap();
        assert!(server.get_session(id).is_err());
    }

    #[test]
    fn test_server_attach_detach() {
        let server = SessionServer::new();
        let session_id = server
            .create_session(Some("test".to_string()), (80, 24))
            .unwrap();

        let client = ClientConnection::new((80, 24));
        let client_id = server.add_client(client).unwrap();

        server.attach_client(client_id, session_id).unwrap();

        let session = server.get_session(session_id).unwrap();
        assert_eq!(session.client_count, 1);

        server.detach_client(client_id).unwrap();

        let session = server.get_session(session_id).unwrap();
        assert_eq!(session.client_count, 0);
    }
}
