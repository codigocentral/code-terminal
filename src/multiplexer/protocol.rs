//! Protocol - Protocolo de comunicação do servidor de sessões

use serde::{Deserialize, Serialize};

/// Tipo de mensagem do protocolo
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    // Comandos do cliente para o servidor
    /// Criar uma nova sessão
    CreateSession {
        name: Option<String>,
        size: (u16, u16),
    },
    /// Anexar a uma sessão existente
    Attach { session_id: String, read_only: bool },
    /// Desanexar da sessão atual
    Detach,
    /// Listar sessões
    ListSessions,
    /// Renomear uma sessão
    RenameSession {
        session_id: String,
        new_name: String,
    },
    /// Destruir uma sessão
    DestroySession { session_id: String },
    /// Enviar dados para o PTY
    Input { data: Vec<u8> },
    /// Redimensionar o terminal
    Resize { cols: u16, rows: u16 },
    /// Solicitar informações
    GetInfo { target: String },
    /// Ping (keep-alive)
    Ping,
    /// Desconectar
    Quit,
    /// Abre o fluxo de uma tab persistente (cria a sessão com `create` se
    /// não existir). Depois da resposta a conexão passa a quadros binários
    /// (ver `multiplexer::stream`).
    OpenStream {
        session: String,
        cols: u16,
        rows: u16,
        create: Option<StreamCreate>,
        /// Reenviar o histórico (falso ao adotar: a janela já tem a tela).
        replay: bool,
    },
    /// Grava os dados da tab (JSON do app) e as marcas da sessão.
    SetSessionMeta {
        session_id: String,
        meta: String,
        hidden: bool,
        kill_on_detach: bool,
        /// Retomar o programa sozinho depois de reiniciar a máquina.
        #[serde(default)]
        auto_resume: bool,
    },
    /// Hook de um agente (Claude Code) rodando no processo `pid`: o daemon
    /// acha a aba pelo terminal do processo e guarda a conversa (para
    /// retomar depois do reboot). `active: false` = a conversa terminou.
    ReportAgent {
        pid: u32,
        kind: String,
        session_id: Option<String>,
        cwd: Option<String>,
        active: bool,
    },

    // Respostas do servidor para o cliente
    /// Sessão criada
    SessionCreated { session_id: String, name: String },
    /// Anexado a uma sessão
    Attached { session_id: String, name: String },
    /// Desanexado
    Detached { session_id: String },
    /// Lista de sessões
    SessionList { sessions: Vec<SessionInfo> },
    /// Sessão renomeada
    SessionRenamed {
        session_id: String,
        new_name: String,
    },
    /// Sessão destruída
    SessionDestroyed { session_id: String },
    /// Aba persistente em que o agente roda (resposta ao `ReportAgent`).
    AgentReported { session: Option<String> },
    /// Fluxo aberto (resposta ao `OpenStream`).
    StreamOpened {
        session_id: String,
        name: String,
        pid: Option<u32>,
        created: bool,
    },
    /// Dados do PTY
    Output { data: Vec<u8> },
    /// Informações
    Info { data: InfoData },
    /// Pong (resposta ao ping)
    Pong,
    /// Erro
    Error { code: ErrorCode, message: String },

    // Eventos assíncronos
    /// Sessão destruída (notificação)
    SessionDestroyedNotification { session_id: String },
    /// Cliente conectado a sessão
    ClientAttached {
        session_id: String,
        client_id: String,
    },
    /// Cliente desconectado da sessão
    ClientDetached {
        session_id: String,
        client_id: String,
    },
    /// Sessão redimensionada
    SessionResized {
        session_id: String,
        cols: u16,
        rows: u16,
    },
}

/// Informação de uma sessão
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    /// ID da sessão
    pub id: String,
    /// Nome da sessão
    pub name: String,
    /// Estado
    pub state: String,
    /// Número de clientes conectados
    pub clients: usize,
    /// Tamanho do terminal
    pub size: (u16, u16),
    /// Diretório de trabalho
    pub working_dir: String,
    /// Quando foi criada
    pub created_at: String,
    /// Tempo de inatividade (segundos)
    pub idle_seconds: u64,
    /// Flags
    pub flags: SessionFlagsInfo,
    /// Só nas sessões de tabs persistentes do app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<AppSessionInfo>,
}

/// Dados de uma sessão de tab persistente.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSessionInfo {
    /// Processo principal (o shell).
    pub pid: Option<u32>,
    /// Dados da tab gravados pelo app (JSON).
    pub meta: String,
    /// "Só esconder": não volta sozinha ao abrir a janela.
    pub hidden: bool,
    /// Deixou de ser persistente: morre quando a janela desanexa.
    pub kill_on_detach: bool,
    /// Retomar o programa sozinho depois de reiniciar a máquina.
    #[serde(default)]
    pub auto_resume: bool,
}

/// Como criar a sessão de uma tab persistente.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamCreate {
    /// Diretório inicial.
    pub cwd: Option<String>,
    /// Programa e argumentos; `None` roda o shell (`SHELL` de `env`).
    pub command: Option<Vec<String>>,
    /// Ambiente do processo (o do app: o daemon roda fora da sessão gráfica).
    pub env: Vec<(String, String)>,
    /// Adotar o PTY que veio com a mensagem (`SCM_RIGHTS`), deste processo.
    pub adopt_pid: Option<u32>,
    /// Bytes da tela atual da tab adotada, mandados logo depois da linha.
    pub seed_len: usize,
    /// Dados iniciais da tab (JSON do app).
    pub meta: String,
}

/// Flags de uma sessão (para serialização)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionFlagsInfo {
    pub destroy_on_exit: bool,
    pub attachable: bool,
    pub shared: bool,
    pub locked: bool,
}

/// Dados de informação
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InfoData {
    ServerInfo {
        version: String,
        sessions_count: usize,
        clients_count: usize,
        uptime_seconds: u64,
    },
    SessionInfo {
        id: String,
        name: String,
        state: String,
        clients: Vec<ClientInfo>,
    },
    ClientInfo {
        id: String,
        address: Option<String>,
        terminal: String,
        attached_to: Option<String>,
    },
}

/// Informação de um cliente
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    /// ID do cliente
    pub id: String,
    /// Endereço
    pub address: Option<String>,
    /// Nome do terminal
    pub terminal: String,
    /// Tamanho do terminal
    pub terminal_size: (u16, u16),
    /// Anexado à sessão
    pub attached_to: Option<String>,
    /// É controlador
    pub is_controller: bool,
    /// Tempo de conexão (segundos)
    pub connected_seconds: u64,
}

/// Código de erro
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    SessionNotFound,
    SessionLocked,
    SessionNotAttachable,
    SessionAlreadyExists,
    ClientNotFound,
    InvalidSessionId,
    InvalidMessage,
    ServerError,
    PermissionDenied,
    NotAttached,
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorCode::SessionNotFound => write!(f, "session not found"),
            ErrorCode::SessionLocked => write!(f, "session locked"),
            ErrorCode::SessionNotAttachable => write!(f, "session not attachable"),
            ErrorCode::SessionAlreadyExists => write!(f, "session already exists"),
            ErrorCode::ClientNotFound => write!(f, "client not found"),
            ErrorCode::InvalidSessionId => write!(f, "invalid session id"),
            ErrorCode::InvalidMessage => write!(f, "invalid message"),
            ErrorCode::ServerError => write!(f, "server error"),
            ErrorCode::PermissionDenied => write!(f, "permission denied"),
            ErrorCode::NotAttached => write!(f, "not attached to any session"),
        }
    }
}

/// Mensagem completa do protocolo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// ID da mensagem
    pub id: u64,
    /// Tipo de mensagem
    pub msg_type: MessageType,
}

impl Message {
    /// Cria uma nova mensagem
    pub fn new(id: u64, msg_type: MessageType) -> Self {
        Self { id, msg_type }
    }

    /// Cria uma mensagem de erro
    pub fn error(id: u64, code: ErrorCode, message: impl Into<String>) -> Self {
        Self::new(
            id,
            MessageType::Error {
                code,
                message: message.into(),
            },
        )
    }

    /// Codifica a mensagem em JSON
    pub fn encode(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Decodifica uma mensagem de JSON
    pub fn decode(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_encode_decode() {
        let msg = Message::new(
            1,
            MessageType::CreateSession {
                name: Some("test".to_string()),
                size: (80, 24),
            },
        );

        let encoded = msg.encode().unwrap();
        let decoded = Message::decode(&encoded).unwrap();

        assert_eq!(decoded.id, 1);
        assert_eq!(decoded.msg_type, msg.msg_type);
    }

    #[test]
    fn test_message_error() {
        let msg = Message::error(1, ErrorCode::SessionNotFound, "test session not found");

        if let MessageType::Error { code, message } = msg.msg_type {
            assert_eq!(code, ErrorCode::SessionNotFound);
            assert_eq!(message, "test session not found");
        } else {
            panic!("Expected Error message type");
        }
    }

    #[test]
    fn test_session_info_serialization() {
        let info = SessionInfo {
            id: "123".to_string(),
            name: "test".to_string(),
            state: "active".to_string(),
            clients: 1,
            size: (80, 24),
            working_dir: "/home/user".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            idle_seconds: 0,
            flags: SessionFlagsInfo {
                destroy_on_exit: false,
                attachable: true,
                shared: false,
                locked: false,
            },
            app: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        let decoded: SessionInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.id, "123");
        assert_eq!(decoded.name, "test");
        assert!(decoded.flags.attachable);
    }
}
