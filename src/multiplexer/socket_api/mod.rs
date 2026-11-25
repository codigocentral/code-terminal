//! Socket API - API completa de controle via socket
//!
//! Este módulo implementa uma API JSON-RPC completa para controle
//! externo do terminal via socket local (Unix socket ou TCP).
//!
//! # Exemplos de uso
//!
//! ```bash
//! # Listar tabs
//! echo '{"jsonrpc":"2.0","id":1,"method":"tab.list","params":{}}' | nc -U /tmp/codeterm.sock
//!
//! # Criar nova tab
//! echo '{"jsonrpc":"2.0","id":2,"method":"tab.create","params":{"title":"novo","command":"bash"}}' | nc -U /tmp/codeterm.sock
//!
//! # Enviar keystrokes
//! echo '{"jsonrpc":"2.0","id":3,"method":"keyboard.send","params":{"keys":"ls -la\n"}}' | nc -U /tmp/codeterm.sock
//! ```

pub mod app_integration;
pub mod keyboard;
pub mod pane;
pub mod server;
pub mod session_renderer;
pub mod session_renderer_protocol;
pub mod state;
pub mod tab;
pub mod types;
pub mod workspace;

pub use app_integration::{
    AppCommand, AppCommandProcessor, AppCommandResult, SocketApiIntegration,
};
pub use keyboard::KeyboardApi;
pub use pane::PaneApi;
pub use server::{clear_app_bridge, set_app_bridge, ApiServer, ApiServerConfig, AppBridge};
pub use session_renderer::SessionRendererApi;
pub use session_renderer_protocol::{
    PanePosition, PaneSize, RenderConfigInfo, RenderConfigUpdate, RenderQuality, RenderSessionInfo,
    RenderState, RenderStats, RenderSyncMode, RenderedPaneInfo, RenderedSessionInfo,
    SessionRendererCommand, SessionRendererHandler, SessionRendererResponse, VisualStateInfo,
};
pub use state::StateApi;
pub use tab::TabApi;
pub use types::*;
pub use workspace::WorkspaceApi;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Erros da API
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum ApiError {
    #[error("Method not found: {0}")]
    MethodNotFound(String),

    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Timeout")]
    Timeout,

    #[error("Not implemented: {0}")]
    NotImplemented(String),
}

impl ApiError {
    /// Código de erro JSON-RPC
    pub fn code(&self) -> i32 {
        match self {
            ApiError::MethodNotFound(_) => -32601,
            ApiError::InvalidParams(_) => -32602,
            ApiError::InternalError(_) => -32603,
            ApiError::NotFound(_) => -32001,
            ApiError::PermissionDenied(_) => -32002,
            ApiError::InvalidState(_) => -32003,
            ApiError::Timeout => -32004,
            ApiError::NotImplemented(_) => -32005,
        }
    }

    /// Mensagem de erro
    pub fn message(&self) -> String {
        self.to_string()
    }
}

/// Resultado de uma operação da API
pub type ApiResult<T> = Result<T, ApiError>;

/// Requisição JSON-RPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Resposta JSON-RPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Cria uma resposta de sucesso
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Cria uma resposta de erro
    pub fn error(id: Option<Value>, error: ApiError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: error.code(),
                message: error.message(),
                data: None,
            }),
        }
    }
}

/// Erro JSON-RPC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Notificação JSON-RPC (sem ID)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Versão da API
pub const API_VERSION: &str = "1.0.0";

/// Capacidades da API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCapabilities {
    pub version: String,
    pub methods: Vec<String>,
    pub features: Vec<String>,
}

impl ApiCapabilities {
    /// Retorna as capacidades da API
    pub fn current() -> Self {
        Self {
            version: API_VERSION.to_string(),
            methods: vec![
                // Workspace
                "workspace.list".to_string(),
                "workspace.current".to_string(),
                "workspace.switch".to_string(),
                "workspace.create".to_string(),
                "workspace.rename".to_string(),
                "workspace.close".to_string(),
                // Tab
                "tab.list".to_string(),
                "tab.get".to_string(),
                "tab.create".to_string(),
                "tab.close".to_string(),
                "tab.switch".to_string(),
                "tab.next".to_string(),
                "tab.previous".to_string(),
                "tab.move".to_string(),
                "tab.rename".to_string(),
                "tab.duplicate".to_string(),
                // Pane
                "pane.list".to_string(),
                "pane.get".to_string(),
                "pane.split".to_string(),
                "pane.close".to_string(),
                "pane.focus".to_string(),
                "pane.resize".to_string(),
                "pane.swap".to_string(),
                "pane.layout".to_string(),
                // Keyboard
                "keyboard.send".to_string(),
                "keyboard.key".to_string(),
                "keyboard.text".to_string(),
                "keyboard.chord".to_string(),
                "keyboard.shortcut".to_string(),
                // State
                "state.get".to_string(),
                "state.query".to_string(),
                "state.subscribe".to_string(),
                "state.unsubscribe".to_string(),
                "state.dump".to_string(),
                // Session Renderer
                "session_renderer.list".to_string(),
                "session_renderer.get_info".to_string(),
                "session_renderer.start".to_string(),
                "session_renderer.stop".to_string(),
                "session_renderer.force_rerender".to_string(),
                "session_renderer.get_visual_state".to_string(),
                "session_renderer.update_config".to_string(),
            ],
            features: vec![
                "workspace".to_string(),
                "tabs".to_string(),
                "panes".to_string(),
                "keyboard".to_string(),
                "notifications".to_string(),
                "state-query".to_string(),
                "session-renderer".to_string(),
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_codes() {
        assert_eq!(ApiError::MethodNotFound("test".to_string()).code(), -32601);
        assert_eq!(ApiError::InvalidParams("test".to_string()).code(), -32602);
        assert_eq!(ApiError::InternalError("test".to_string()).code(), -32603);
    }

    #[test]
    fn test_json_rpc_response() {
        let response = JsonRpcResponse::success(
            Some(Value::Number(1.into())),
            Value::String("ok".to_string()),
        );

        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.result, Some(Value::String("ok".to_string())));
        assert!(response.error.is_none());
    }

    #[test]
    fn test_api_capabilities() {
        let caps = ApiCapabilities::current();
        assert_eq!(caps.version, API_VERSION);
        assert!(!caps.methods.is_empty());
        assert!(!caps.features.is_empty());
    }
}
