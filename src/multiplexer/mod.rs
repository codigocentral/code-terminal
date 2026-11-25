//! Session Server for Multiplexer Mode
//!
//! Provides tmux-like functionality with persistent sessions
//! that can be attached and detached.

#[cfg(unix)]
pub mod app_client;
#[cfg(unix)]
pub mod app_session;
pub mod attach;
pub mod client;
pub mod client_session;
pub mod commands;
#[cfg(unix)]
pub mod daemon;
pub mod groups;
pub mod monitor;
pub mod pane;
pub mod persistence;
pub mod protocol;
pub mod screen;
pub mod server;
pub mod session;
pub mod session_list;
pub mod shared;
pub mod socket;
pub mod socket_api;
pub mod ssh_compat;
#[cfg(unix)]
pub mod stream;
#[cfg(unix)]
pub mod tab_store;
pub mod transport;
pub mod window;

// Re-exportação de tipos comuns
pub use crate::types::PaneId;

#[cfg(test)]
pub mod integration_tests;

pub use client::{ClientConnection, ClientId};
pub use client_session::SessionClient;
pub use commands::{
    default_keybindings, CommandParser, Layout, MuxCommand, PaneDirection, ResizeDirection,
    WindowTarget,
};
pub use groups::{GroupId, GroupManager, GroupStats, SessionGroup};
pub use monitor::{GlobalStats, HealthReport, SessionMonitor, SessionStats};
pub use pane::{
    CursorMode, Pane, PaneContent, PaneFlags, PaneManager, PaneState, ScrollbackBuffer, Selection,
    SelectionType,
};
pub use persistence::{
    PersistedSessionData, PersistenceConfig, PersistenceStats, SessionPersistence,
};
pub use protocol::{ErrorCode, Message, MessageType, SessionInfo};
pub use server::SessionServer;
pub use session::{MultiplexerSession, SessionFlags, SessionId, SessionState};
pub use session_list::{
    ListStats, SessionFilter, SessionList, SessionListItem, SessionSwitcher, SortOrder,
};
pub use shared::{
    ClientPermissions, InputData, SharedClient, SharedInputEvent, SharedSession,
    SharedSessionConfig, SharedSessionManager, SyncMode,
};
pub use socket::{is_server_running, runtime_dir, server_socket_path};
pub use socket_api::{
    ApiCapabilities, ApiError, ApiResult, ApiServer, ApiServerConfig, Direction,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, KeyEvent, KeyboardApi, KeyboardShortcut,
    Modifier, NotificationEvent, PaneApi, PaneInfo, PaneLayout, SplitDirection, StateApi,
    StateQueryType, TabApi, TabInfo, TerminalState, WorkspaceApi, WorkspaceInfo, API_VERSION,
};
pub use ssh_compat::{
    AuthMethod, PortForwardConfig, SshCompatConfig, SshCompatManager, SshCompatibleSession,
    SshSessionInfo, SshTransport,
};
pub use window::{Window, WindowFlags, WindowId, WindowLayout, WindowManager, WindowState};

use thiserror::Error;

/// Erros do multiplexador
#[derive(Error, Debug)]
pub enum MultiplexerError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Client not found: {0}")]
    ClientNotFound(String),

    #[error("Session locked: {0}")]
    SessionLocked(String),

    #[error("Session not attachable: {0}")]
    SessionNotAttachable(String),

    #[error("Already attached to a session")]
    AlreadyAttached,

    #[error("Not attached to any session")]
    NotAttached,

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Resultado do multiplexador
pub type MultiplexerResult<T> = Result<T, MultiplexerError>;
