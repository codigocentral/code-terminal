//! Multiplexer Session - Sessão persistente de terminal

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use uuid::Uuid;

/// ID único de uma sessão
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Cria um novo ID de sessão
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string().split('-').next().unwrap_or(""))
    }
}

/// Estado de uma sessão
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Criando
    Creating,
    /// Ativa (com pelo menos um cliente)
    Active,
    /// Em background (sem clientes)
    Background,
    /// Destruindo
    Destroying,
    /// Destruída
    Destroyed,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionState::Creating => write!(f, "creating"),
            SessionState::Active => write!(f, "active"),
            SessionState::Background => write!(f, "background"),
            SessionState::Destroying => write!(f, "destroying"),
            SessionState::Destroyed => write!(f, "destroyed"),
        }
    }
}

/// Informações de uma sessão
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplexerSession {
    /// ID único
    pub id: SessionId,
    /// Nome da sessão
    pub name: String,
    /// Estado atual
    pub state: SessionState,
    /// Número de clientes conectados
    pub client_count: usize,
    /// Tamanho do terminal (cols, rows)
    pub size: (u16, u16),
    /// Diretório de trabalho
    pub working_dir: String,
    /// Shell em execução
    pub shell: String,
    /// Comando sendo executado (se houver)
    pub command: Option<String>,
    /// Quando foi criada
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Flags
    pub flags: SessionFlags,
    /// Metadados
    pub metadata: HashMap<String, String>,
}

/// Flags de uma sessão
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionFlags {
    /// Sessão pode ser destruída quando vazia
    pub destroy_on_exit: bool,
    /// Sessão é anexável (pode receber attach)
    pub attachable: bool,
    /// Sessão é compartilhada (múltiplos clientes)
    pub shared: bool,
    /// Sessão está bloqueada (não permite novos attaches)
    pub locked: bool,
}

/// Contador global para nomes de sessão
static SESSION_COUNTER: AtomicUsize = AtomicUsize::new(0);

impl MultiplexerSession {
    /// Cria uma nova sessão
    pub fn new(name: Option<String>, size: (u16, u16)) -> Self {
        let id = SessionId::new();
        let name = name.unwrap_or_else(|| {
            let num = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
            format!("{}", num)
        });

        Self {
            id,
            name,
            state: SessionState::Creating,
            client_count: 0,
            size,
            working_dir: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
            command: None,
            created_at: chrono::Utc::now(),
            last_activity: chrono::Utc::now(),
            flags: SessionFlags {
                attachable: true,
                ..Default::default()
            },
            metadata: HashMap::new(),
        }
    }

    /// Retorna se a sessão está ativa
    pub fn is_active(&self) -> bool {
        self.state == SessionState::Active
    }

    /// Retorna se a sessão pode receber attach
    pub fn can_attach(&self) -> bool {
        self.flags.attachable
            && !self.flags.locked
            && matches!(self.state, SessionState::Active | SessionState::Background)
    }

    /// Atualiza a última atividade
    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now();
    }

    /// Adiciona um cliente
    pub fn add_client(&mut self) {
        self.client_count += 1;
        if self.client_count > 0 {
            self.state = SessionState::Active;
        }
        self.touch();
    }

    /// Remove um cliente
    pub fn remove_client(&mut self) {
        if self.client_count > 0 {
            self.client_count -= 1;
        }
        if self.client_count == 0 {
            self.state = SessionState::Background;
        }
        self.touch();
    }

    /// Retorna o tempo de vida da sessão
    pub fn age(&self) -> Duration {
        let now = chrono::Utc::now();
        now.signed_duration_since(self.created_at)
            .to_std()
            .unwrap_or(Duration::from_secs(0))
    }

    /// Retorna o tempo desde a última atividade
    pub fn idle_time(&self) -> Duration {
        let now = chrono::Utc::now();
        now.signed_duration_since(self.last_activity)
            .to_std()
            .unwrap_or(Duration::from_secs(0))
    }

    /// Renomeia a sessão
    pub fn rename(&mut self, new_name: String) {
        self.name = new_name;
        self.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_creation() {
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_session_creation() {
        let session = MultiplexerSession::new(Some("test".to_string()), (80, 24));
        assert_eq!(session.name, "test");
        assert_eq!(session.size, (80, 24));
        assert_eq!(session.state, SessionState::Creating);
        assert_eq!(session.client_count, 0);
    }

    #[test]
    fn test_session_auto_name() {
        let session1 = MultiplexerSession::new(None, (80, 24));
        let session2 = MultiplexerSession::new(None, (80, 24));
        assert_ne!(session1.name, session2.name);
    }

    #[test]
    fn test_session_clients() {
        let mut session = MultiplexerSession::new(Some("test".to_string()), (80, 24));

        session.add_client();
        assert_eq!(session.client_count, 1);
        assert_eq!(session.state, SessionState::Active);

        session.add_client();
        assert_eq!(session.client_count, 2);

        session.remove_client();
        assert_eq!(session.client_count, 1);

        session.remove_client();
        assert_eq!(session.client_count, 0);
        assert_eq!(session.state, SessionState::Background);
    }

    #[test]
    fn test_session_can_attach() {
        let mut session = MultiplexerSession::new(Some("test".to_string()), (80, 24));
        session.state = SessionState::Active;

        assert!(session.can_attach());

        session.flags.locked = true;
        assert!(!session.can_attach());

        session.flags.locked = false;
        session.state = SessionState::Destroyed;
        assert!(!session.can_attach());
    }

    #[test]
    fn test_session_rename() {
        let mut session = MultiplexerSession::new(Some("old".to_string()), (80, 24));
        let old_activity = session.last_activity;
        // Sem a pausa, new() e rename() podem cair no mesmo tique do relógio.
        std::thread::sleep(std::time::Duration::from_millis(2));

        session.rename("new".to_string());
        assert_eq!(session.name, "new");
        assert!(session.last_activity > old_activity);
    }
}
