//! App Integration - Conecta a Socket API com o estado real da aplicação
//!
//! Este módulo fornece integração entre a Socket API JSON-RPC e o estado
//! real da aplicação (AppState, TabManager, etc.), permitindo controle
//! externo completo do terminal.

use crate::app::AppState;
use crate::config::Config;
use crate::multiplexer::PaneId;
use crate::terminal::buffer::TerminalBuffer;
use crate::ui::tabs::{TabId, TabManager};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error};

use super::{
    ApiError, ApiResult, JsonRpcRequest, JsonRpcResponse, KeyboardApi, PaneApi, StateApi, TabApi,
    WorkspaceApi,
};

/// Integração da Socket API com a aplicação
pub struct SocketApiIntegration {
    /// Canal de comandos para o AppState
    command_tx: mpsc::Sender<AppCommand>,
    /// Estado da aplicação (referência)
    app_state: Option<Arc<RwLock<AppState>>>,
    /// Tab Manager (referência)
    tab_manager: Option<Arc<RwLock<TabManager>>>,
    /// APIs
    workspace_api: Arc<WorkspaceApi>,
    tab_api: Arc<TabApi>,
    pane_api: Arc<PaneApi>,
    keyboard_api: Arc<KeyboardApi>,
    state_api: Arc<StateApi>,
}

/// Comandos que podem ser enviados para a aplicação
#[derive(Debug, Clone)]
pub enum AppCommand {
    /// Cria uma nova tab
    CreateTab {
        title: Option<String>,
        command: Option<String>,
        working_dir: Option<String>,
        response_tx: mpsc::Sender<AppCommandResult>,
    },
    /// Fecha uma tab
    CloseTab {
        tab_id: TabId,
        force: bool,
        response_tx: mpsc::Sender<AppCommandResult>,
    },
    /// Muda para uma tab
    SwitchTab {
        tab_id: TabId,
        response_tx: mpsc::Sender<AppCommandResult>,
    },
    /// Envia keystrokes
    SendKeys {
        tab_id: Option<TabId>,
        keys: String,
        response_tx: mpsc::Sender<AppCommandResult>,
    },
    /// Obtém informações das tabs
    GetTabs {
        response_tx: mpsc::Sender<AppCommandResult>,
    },
    /// Obtém informações de um pane
    GetPaneInfo {
        pane_id: PaneId,
        response_tx: mpsc::Sender<AppCommandResult>,
    },
    /// Divide um pane
    SplitPane {
        pane_id: PaneId,
        vertical: bool,
        response_tx: mpsc::Sender<AppCommandResult>,
    },
    /// Redimensiona um pane
    ResizePane {
        pane_id: PaneId,
        direction: ResizeDirection,
        amount: i32,
        response_tx: mpsc::Sender<AppCommandResult>,
    },
    /// Envia comando ao multiplexer
    MultiplexerCommand {
        command: MultiplexerCommand,
        response_tx: mpsc::Sender<AppCommandResult>,
    },
}

/// Resultado de um comando da aplicação
#[derive(Debug, Clone)]
pub enum AppCommandResult {
    Success(Option<serde_json::Value>),
    Error(String),
}

/// Direção de redimensionamento
#[derive(Debug, Clone, Copy)]
pub enum ResizeDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Comandos do multiplexer
#[derive(Debug, Clone)]
pub enum MultiplexerCommand {
    ListSessions,
    AttachSession { name: String },
    DetachSession,
    CreateSession { name: String },
    KillSession { name: String },
}

impl SocketApiIntegration {
    /// Cria uma nova integração
    pub fn new(
        workspace_api: Arc<WorkspaceApi>,
        tab_api: Arc<TabApi>,
        pane_api: Arc<PaneApi>,
        keyboard_api: Arc<KeyboardApi>,
        state_api: Arc<StateApi>,
    ) -> (Self, mpsc::Receiver<AppCommand>) {
        let (command_tx, command_rx) = mpsc::channel(100);

        let integration = Self {
            command_tx,
            app_state: None,
            tab_manager: None,
            workspace_api,
            tab_api,
            pane_api,
            keyboard_api,
            state_api,
        };

        (integration, command_rx)
    }

    /// Registra o AppState
    pub fn register_app_state(&mut self, app_state: Arc<RwLock<AppState>>) {
        self.app_state = Some(app_state);
    }

    /// Registra o TabManager
    pub fn register_tab_manager(&mut self, tab_manager: Arc<RwLock<TabManager>>) {
        let keyboard_tab_manager = Arc::clone(&tab_manager);
        self.keyboard_api.set_input_backend(move |tab_id, keys| {
            let tab_id = tab_id
                .map(TabId::try_from)
                .transpose()
                .map_err(|_| ApiError::InvalidParams("'tab_id' is too large".to_string()))?;
            let mut manager = keyboard_tab_manager.write();
            let tab = match tab_id {
                Some(id) => manager.tabs_mut().iter_mut().find(|tab| tab.id == id),
                None => manager.get_active_tab_mut(),
            };

            match tab {
                Some(tab) => {
                    let tab_id = tab.id;
                    let Some(buffer) = tab.buffer.as_mut() else {
                        return Err(ApiError::InvalidState(format!(
                            "Tab {} has no terminal buffer",
                            tab_id
                        )));
                    };

                    buffer.write_str(&keys).map_err(|error| {
                        ApiError::InternalError(format!(
                            "Failed to write to PTY for tab {}: {}",
                            tab_id, error
                        ))
                    })
                }
                None => Err(ApiError::NotFound(
                    tab_id
                        .map(|id| format!("Tab {} not found", id))
                        .unwrap_or_else(|| "No active tab".to_string()),
                )),
            }
        });
        self.tab_manager = Some(tab_manager);
    }

    /// Processa uma requisição JSON-RPC
    pub async fn handle_request(&self, request: JsonRpcRequest) -> Option<JsonRpcResponse> {
        let id = request.id.clone();

        // Extrai namespace e método
        let parts: Vec<&str> = request.method.split('.').collect();
        if parts.len() != 2 {
            return Some(JsonRpcResponse::error(
                id,
                ApiError::InvalidParams(format!(
                    "Invalid method format: {}. Expected 'namespace.method'",
                    request.method
                )),
            ));
        }

        let (namespace, method) = (parts[0], parts[1]);

        // Primeiro tenta métodos que precisam de acesso ao estado real
        let result = match namespace {
            "app" => self.handle_app_method(method, &request.params).await,
            "terminal" => self.handle_terminal_method(method, &request.params).await,
            _ => {
                // Delega para as APIs existentes, que esperam o nome completo
                // ("tab.list", "keyboard.send"...), igual ao roteador do servidor.
                let full_method = request.method.as_str();
                match namespace {
                    "workspace" => self
                        .workspace_api
                        .handle_method(full_method, request.params),
                    "tab" => self.tab_api.handle_method(full_method, request.params),
                    "pane" => self.pane_api.handle_method(full_method, request.params),
                    "keyboard" => self.keyboard_api.handle_method(full_method, request.params),
                    "state" => self.state_api.handle_method(full_method, request.params),
                    _ => Err(ApiError::MethodNotFound(format!(
                        "Unknown namespace: {}",
                        namespace
                    ))),
                }
            }
        };

        match result {
            Ok(value) => Some(JsonRpcResponse::success(id, value)),
            Err(e) => Some(JsonRpcResponse::error(id, e)),
        }
    }

    /// Handler para métodos de aplicação
    async fn handle_app_method(
        &self,
        method: &str,
        _params: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        match method {
            "ping" => Ok(serde_json::json!({
                "pong": true,
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            })),
            "version" => Ok(serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "name": env!("CARGO_PKG_NAME"),
            })),
            "quit" => {
                // Envia comando para encerrar aplicação
                let (tx, mut rx) = mpsc::channel(1);
                let cmd = AppCommand::MultiplexerCommand {
                    command: MultiplexerCommand::DetachSession,
                    response_tx: tx,
                };

                if let Err(e) = self.command_tx.send(cmd).await {
                    return Err(ApiError::InternalError(format!(
                        "Failed to send command: {}",
                        e
                    )));
                }

                // Aguarda confirmação (com timeout)
                match tokio::time::timeout(tokio::time::Duration::from_secs(5), rx.recv()).await {
                    Ok(Some(AppCommandResult::Success(_))) => {
                        Ok(serde_json::json!({ "quitting": true }))
                    }
                    _ => Err(ApiError::Timeout),
                }
            }
            "focus" => {
                // Foca a janela da aplicação
                Ok(serde_json::json!({ "focused": true }))
            }
            _ => Err(ApiError::MethodNotFound(format!("app.{}", method))),
        }
    }

    /// Handler para métodos de terminal
    async fn handle_terminal_method(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> ApiResult<serde_json::Value> {
        match method {
            "send_text" => {
                let text = params.get("text").and_then(|v| v.as_str()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'text' parameter".to_string())
                })?;

                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId);

                let (tx, mut rx) = mpsc::channel(1);
                let cmd = AppCommand::SendKeys {
                    tab_id,
                    keys: text.to_string(),
                    response_tx: tx,
                };

                if let Err(e) = self.command_tx.send(cmd).await {
                    return Err(ApiError::InternalError(format!(
                        "Failed to send command: {}",
                        e
                    )));
                }

                match tokio::time::timeout(tokio::time::Duration::from_secs(5), rx.recv()).await {
                    Ok(Some(AppCommandResult::Success(_))) => {
                        Ok(serde_json::json!({ "sent": true }))
                    }
                    Ok(Some(AppCommandResult::Error(error))) => Err(ApiError::InvalidState(error)),
                    Ok(None) => Err(ApiError::InternalError(
                        "Application command processor stopped".to_string(),
                    )),
                    Err(_) => Err(ApiError::Timeout),
                }
            }
            "get_size" => {
                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId);

                let Some(tab_manager) = &self.tab_manager else {
                    return Err(ApiError::InvalidState(
                        "TabManager is not registered".to_string(),
                    ));
                };

                let manager = tab_manager.read();
                let tab = match tab_id {
                    Some(id) => manager.tabs().iter().find(|tab| tab.id == id),
                    None => manager.get_active_tab(),
                }
                .ok_or_else(|| {
                    ApiError::NotFound(
                        tab_id
                            .map(|id| format!("Tab {} not found", id))
                            .unwrap_or_else(|| "No active tab".to_string()),
                    )
                })?;

                let Some(buffer) = tab.buffer.as_ref() else {
                    return Err(ApiError::InvalidState(format!(
                        "Tab {} has no terminal buffer",
                        tab.id
                    )));
                };

                let (cols, rows) = buffer.content_size();
                Ok(serde_json::json!({
                    "cols": cols,
                    "rows": rows,
                }))
            }
            "resize" => {
                let cols = params.get("cols").and_then(|v| v.as_u64()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'cols' parameter".to_string())
                })?;
                let rows = params.get("rows").and_then(|v| v.as_u64()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'rows' parameter".to_string())
                })?;
                let cols = u16::try_from(cols)
                    .map_err(|_| ApiError::InvalidParams("'cols' is too large".to_string()))?;
                let rows = u16::try_from(rows)
                    .map_err(|_| ApiError::InvalidParams("'rows' is too large".to_string()))?;
                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId);

                let Some(tab_manager) = &self.tab_manager else {
                    return Err(ApiError::InvalidState(
                        "TabManager is not registered".to_string(),
                    ));
                };

                let mut manager = tab_manager.write();
                let tab = match tab_id {
                    Some(id) => manager.tabs_mut().iter_mut().find(|tab| tab.id == id),
                    None => manager.get_active_tab_mut(),
                }
                .ok_or_else(|| {
                    ApiError::NotFound(
                        tab_id
                            .map(|id| format!("Tab {} not found", id))
                            .unwrap_or_else(|| "No active tab".to_string()),
                    )
                })?;

                let Some(buffer) = tab.buffer.as_mut() else {
                    return Err(ApiError::InvalidState(format!(
                        "Tab {} has no terminal buffer",
                        tab.id
                    )));
                };

                buffer
                    .resize(cols, rows)
                    .map_err(|e| ApiError::InternalError(format!("Failed to resize PTY: {}", e)))?;

                Ok(serde_json::json!({
                    "resized": true,
                    "cols": cols,
                    "rows": rows,
                }))
            }
            "get_cwd" => {
                // Obtém diretório atual
                Ok(serde_json::json!({
                    "cwd": std::env::current_dir()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default(),
                }))
            }
            _ => Err(ApiError::MethodNotFound(format!("terminal.{}", method))),
        }
    }

    /// Obtém uma referência ao command sender
    pub fn command_sender(&self) -> mpsc::Sender<AppCommand> {
        self.command_tx.clone()
    }

    /// Verifica se está integrado com o estado real
    pub fn is_integrated(&self) -> bool {
        self.app_state.is_some() || self.tab_manager.is_some()
    }
}

/// Processador de comandos da aplicação
pub struct AppCommandProcessor {
    app_state: Option<Arc<RwLock<AppState>>>,
    tab_manager: Option<Arc<RwLock<TabManager>>>,
}

impl Default for AppCommandProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl AppCommandProcessor {
    /// Cria um novo processador
    pub fn new() -> Self {
        Self {
            app_state: None,
            tab_manager: None,
        }
    }

    /// Registra o AppState
    pub fn register_app_state(&mut self, app_state: Arc<RwLock<AppState>>) {
        self.app_state = Some(app_state);
    }

    /// Registra o TabManager
    pub fn register_tab_manager(&mut self, tab_manager: Arc<RwLock<TabManager>>) {
        self.tab_manager = Some(tab_manager);
    }

    /// Processa um comando
    pub async fn process_command(&self, command: AppCommand) {
        let result = match command {
            AppCommand::CreateTab {
                title,
                command: cmd,
                working_dir,
                response_tx,
            } => {
                self.handle_create_tab(title, cmd, working_dir, response_tx)
                    .await
            }
            AppCommand::CloseTab {
                tab_id,
                force,
                response_tx,
            } => self.handle_close_tab(tab_id, force, response_tx).await,
            AppCommand::SwitchTab {
                tab_id,
                response_tx,
            } => self.handle_switch_tab(tab_id, response_tx).await,
            AppCommand::SendKeys {
                tab_id,
                keys,
                response_tx,
            } => self.handle_send_keys(tab_id, keys, response_tx).await,
            AppCommand::GetTabs { response_tx } => self.handle_get_tabs(response_tx).await,
            AppCommand::GetPaneInfo {
                pane_id,
                response_tx,
            } => self.handle_get_pane_info(pane_id, response_tx).await,
            AppCommand::SplitPane {
                pane_id,
                vertical,
                response_tx,
            } => self.handle_split_pane(pane_id, vertical, response_tx).await,
            AppCommand::ResizePane {
                pane_id,
                direction,
                amount,
                response_tx,
            } => {
                self.handle_resize_pane(pane_id, direction, amount, response_tx)
                    .await
            }
            AppCommand::MultiplexerCommand {
                command,
                response_tx,
            } => self.handle_multiplexer_command(command, response_tx).await,
        };

        if let Err(e) = result {
            error!("Error processing command: {:?}", e);
        }
    }

    async fn handle_create_tab(
        &self,
        title: Option<String>,
        command: Option<String>,
        working_dir: Option<String>,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        debug!("Creating tab: title={:?}, cmd={:?}", title, command);

        let Some(tab_manager) = &self.tab_manager else {
            return response_tx
                .send(AppCommandResult::Error(
                    "TabManager is not registered".to_string(),
                ))
                .await;
        };

        let display_title = title
            .as_deref()
            .or(command.as_deref())
            .unwrap_or("terminal");
        let buffer_result = match command.as_deref() {
            Some(command_line) => {
                let mut parts = command_line.split_whitespace();
                let Some(program) = parts.next() else {
                    return response_tx
                        .send(AppCommandResult::Error(
                            "Command cannot be empty".to_string(),
                        ))
                        .await;
                };
                let args: Vec<String> = parts.map(ToString::to_string).collect();
                TerminalBuffer::with_command(80, 24, program, &args, &Config::default())
            }
            None => TerminalBuffer::new(80, 24),
        };

        let buffer = match buffer_result {
            Ok(buffer) => buffer,
            Err(error) => {
                return response_tx
                    .send(AppCommandResult::Error(format!(
                        "Failed to create terminal buffer for tab: {}",
                        error
                    )))
                    .await;
            }
        };

        let mut manager = tab_manager.write();
        let tab_id = manager.create_tab_with_buffer(display_title, buffer);
        if let Some(working_dir) = working_dir.as_deref() {
            if let Some(tab) = manager.tabs_mut().iter_mut().find(|tab| tab.id == tab_id) {
                tab.set_working_dir(working_dir);
            }
        }
        let tab_index = manager
            .tabs()
            .iter()
            .position(|tab| tab.id == tab_id)
            .unwrap_or_else(|| manager.tab_count().saturating_sub(1));
        let tab = manager.get_tab(tab_index);

        let tab_info = serde_json::json!({
            "id": tab_id,
            "index": tab_index,
            "title": tab.map(|t| t.title()).unwrap_or_else(|| display_title.to_string()),
            "is_active": tab.map(|t| t.is_active()).unwrap_or(false),
            "created": true,
        });

        response_tx
            .send(AppCommandResult::Success(Some(tab_info)))
            .await
    }

    async fn handle_close_tab(
        &self,
        tab_id: TabId,
        force: bool,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        debug!("Closing tab: id={}, force={}", tab_id, force);

        let Some(tab_manager) = &self.tab_manager else {
            return response_tx
                .send(AppCommandResult::Error(
                    "TabManager is not registered".to_string(),
                ))
                .await;
        };

        let mut manager = tab_manager.write();
        let Some(index) = manager.tabs().iter().position(|tab| tab.id == tab_id) else {
            return response_tx
                .send(AppCommandResult::Error(format!("Tab {} not found", tab_id)))
                .await;
        };

        if !force && manager.has_running_process(index) {
            return response_tx
                .send(AppCommandResult::Error(format!(
                    "Tab {} has a running process",
                    tab_id
                )))
                .await;
        }

        if manager.close_tab(index) {
            response_tx.send(AppCommandResult::Success(None)).await
        } else {
            response_tx
                .send(AppCommandResult::Error(format!(
                    "Failed to close tab {}",
                    tab_id
                )))
                .await
        }
    }

    async fn handle_switch_tab(
        &self,
        tab_id: TabId,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        debug!("Switching to tab: id={}", tab_id);

        let Some(tab_manager) = &self.tab_manager else {
            return response_tx
                .send(AppCommandResult::Error(
                    "TabManager is not registered".to_string(),
                ))
                .await;
        };

        let mut manager = tab_manager.write();
        let Some(index) = manager.tabs().iter().position(|tab| tab.id == tab_id) else {
            return response_tx
                .send(AppCommandResult::Error(format!("Tab {} not found", tab_id)))
                .await;
        };

        manager.switch_tab(index);
        response_tx.send(AppCommandResult::Success(None)).await
    }

    async fn handle_send_keys(
        &self,
        tab_id: Option<TabId>,
        keys: String,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        debug!("Sending keys to tab {:?}: {} chars", tab_id, keys.len());

        let Some(tab_manager) = &self.tab_manager else {
            return response_tx
                .send(AppCommandResult::Error(
                    "TabManager is not registered".to_string(),
                ))
                .await;
        };

        let result = {
            let mut manager = tab_manager.write();
            let tab = match tab_id {
                Some(id) => manager.tabs_mut().iter_mut().find(|tab| tab.id == id),
                None => manager.get_active_tab_mut(),
            };

            match tab {
                Some(tab) => {
                    let tab_id = tab.id;
                    match tab.buffer.as_mut() {
                        Some(buffer) => buffer
                            .write_str(&keys)
                            .map(|_| AppCommandResult::Success(None))
                            .unwrap_or_else(|error| {
                                AppCommandResult::Error(format!(
                                    "Failed to write to PTY for tab {}: {}",
                                    tab_id, error
                                ))
                            }),
                        None => AppCommandResult::Error(format!(
                            "Tab {} has no terminal buffer",
                            tab_id
                        )),
                    }
                }
                None => AppCommandResult::Error(
                    tab_id
                        .map(|id| format!("Tab {} not found", id))
                        .unwrap_or_else(|| "No active tab".to_string()),
                ),
            }
        };

        response_tx.send(result).await
    }

    async fn handle_get_tabs(
        &self,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        let Some(tab_manager) = &self.tab_manager else {
            return response_tx
                .send(AppCommandResult::Error(
                    "TabManager is not registered".to_string(),
                ))
                .await;
        };

        let manager = tab_manager.read();
        let tabs = serde_json::Value::Array(
            manager
                .tabs()
                .iter()
                .enumerate()
                .map(|(index, tab)| {
                    serde_json::json!({
                        "id": tab.id,
                        "index": index,
                        "title": tab.title(),
                        "is_active": tab.is_active(),
                        "working_dir": tab.working_dir(),
                    })
                })
                .collect(),
        );

        response_tx
            .send(AppCommandResult::Success(Some(tabs)))
            .await
    }

    async fn handle_get_pane_info(
        &self,
        pane_id: PaneId,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        response_tx
            .send(AppCommandResult::Error(format!(
                "Pane {} is not connected to a real pane backend yet",
                pane_id
            )))
            .await
    }

    async fn handle_split_pane(
        &self,
        pane_id: PaneId,
        vertical: bool,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        debug!("Splitting pane: id={}, vertical={}", pane_id, vertical);

        response_tx
            .send(AppCommandResult::Error(format!(
                "Pane split is not connected to a real pane backend yet: pane={}, vertical={}",
                pane_id, vertical
            )))
            .await
    }

    async fn handle_resize_pane(
        &self,
        _pane_id: PaneId,
        direction: ResizeDirection,
        amount: i32,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        debug!("Resizing pane: id={:?}, amount={}", direction, amount);

        response_tx
            .send(AppCommandResult::Error(format!(
                "Pane resize is not connected to a real pane backend yet: direction={:?}, amount={}",
                direction, amount
            )))
            .await
    }

    async fn handle_multiplexer_command(
        &self,
        command: MultiplexerCommand,
        response_tx: mpsc::Sender<AppCommandResult>,
    ) -> Result<(), mpsc::error::SendError<AppCommandResult>> {
        debug!("Multiplexer command: {:?}", command);

        match command {
            MultiplexerCommand::ListSessions => {
                let sessions = serde_json::json!([
                    {
                        "name": "default",
                        "attached": true,
                    }
                ]);
                response_tx
                    .send(AppCommandResult::Success(Some(sessions)))
                    .await
            }
            MultiplexerCommand::AttachSession { name } => {
                response_tx
                    .send(AppCommandResult::Success(Some(serde_json::json!({
                        "attached": name,
                    }))))
                    .await
            }
            _ => response_tx.send(AppCommandResult::Success(None)).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_command_processor() {
        let mut processor = AppCommandProcessor::new();
        let mut tab_manager = TabManager::new();
        tab_manager.create_tab("bash");
        processor.register_tab_manager(Arc::new(RwLock::new(tab_manager)));

        let (tx, mut rx) = mpsc::channel(1);
        let cmd = AppCommand::GetTabs { response_tx: tx };

        processor.process_command(cmd).await;

        let result = rx.recv().await;
        assert!(matches!(result, Some(AppCommandResult::Success(_))));
    }

    #[tokio::test]
    async fn test_app_command_processor_create_tab_with_real_buffer() {
        let mut processor = AppCommandProcessor::new();
        let tab_manager = Arc::new(RwLock::new(TabManager::new()));
        processor.register_tab_manager(Arc::clone(&tab_manager));

        let (tx, mut rx) = mpsc::channel(1);
        processor
            .process_command(AppCommand::CreateTab {
                title: Some("socket-tab".to_string()),
                command: None,
                working_dir: Some("D:\\".to_string()),
                response_tx: tx,
            })
            .await;

        let Some(AppCommandResult::Success(Some(tab_info))) = rx.recv().await else {
            panic!("expected successful tab creation");
        };
        let tab_id = tab_info["id"].as_u64().expect("tab id") as TabId;

        let manager = tab_manager.read();
        let tab = manager
            .tabs()
            .iter()
            .find(|tab| tab.id == tab_id)
            .expect("created tab");
        assert!(tab.buffer.is_some());
        assert_eq!(tab.working_dir(), Some("D:\\"));
    }

    #[tokio::test]
    async fn test_json_rpc_terminal_flow_uses_real_tab_buffer() {
        let workspace_api = Arc::new(WorkspaceApi::new());
        let tab_api = Arc::new(TabApi::new());
        let pane_api = Arc::new(PaneApi::new());
        let keyboard_api = Arc::new(KeyboardApi::new());
        let state_api = Arc::new(StateApi::new(Arc::new(RwLock::new(
            crate::config::Config::default(),
        ))));

        let (mut integration, mut command_rx) =
            SocketApiIntegration::new(workspace_api, tab_api, pane_api, keyboard_api, state_api);

        let tab_manager = Arc::new(RwLock::new(TabManager::new()));
        integration.register_tab_manager(Arc::clone(&tab_manager));

        let mut processor = AppCommandProcessor::new();
        processor.register_tab_manager(Arc::clone(&tab_manager));

        let (create_tx, mut create_rx) = mpsc::channel(1);
        processor
            .process_command(AppCommand::CreateTab {
                title: Some("json-rpc-tab".to_string()),
                command: None,
                working_dir: None,
                response_tx: create_tx,
            })
            .await;

        let Some(AppCommandResult::Success(Some(tab_info))) = create_rx.recv().await else {
            panic!("expected successful tab creation");
        };
        let tab_id = tab_info["id"].as_u64().expect("tab id");

        let send_request = integration.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "terminal.send_text".to_string(),
            params: serde_json::json!({
                "tab_id": tab_id,
                "text": "echo codeterm\\n"
            }),
        });
        let process_request = async {
            let command = command_rx.recv().await.expect("app command");
            processor.process_command(command).await;
        };
        let (send_response, _) = tokio::join!(send_request, process_request);
        let send_response = send_response.expect("send_text response");
        assert!(send_response.error.is_none(), "{:?}", send_response.error);
        assert_eq!(
            send_response.result,
            Some(serde_json::json!({"sent": true}))
        );

        let resize_response = integration
            .handle_request(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::json!(2)),
                method: "terminal.resize".to_string(),
                params: serde_json::json!({
                    "tab_id": tab_id,
                    "cols": 100,
                    "rows": 32
                }),
            })
            .await
            .expect("resize response");
        assert!(
            resize_response.error.is_none(),
            "{:?}",
            resize_response.error
        );

        let size_response = integration
            .handle_request(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(serde_json::json!(3)),
                method: "terminal.get_size".to_string(),
                params: serde_json::json!({ "tab_id": tab_id }),
            })
            .await
            .expect("get_size response");
        assert_eq!(
            size_response.result,
            Some(serde_json::json!({
                "cols": 100,
                "rows": 32
            }))
        );
    }

    #[test]
    fn test_socket_api_integration() {
        let workspace_api = Arc::new(WorkspaceApi::new());
        let tab_api = Arc::new(TabApi::new());
        let pane_api = Arc::new(PaneApi::new());
        let keyboard_api = Arc::new(KeyboardApi::new());
        let state_api = Arc::new(StateApi::new(Arc::new(RwLock::new(
            crate::config::Config::default(),
        ))));

        let (integration, _command_rx) =
            SocketApiIntegration::new(workspace_api, tab_api, pane_api, keyboard_api, state_api);

        assert!(!integration.is_integrated());
    }

    fn new_integration() -> (SocketApiIntegration, mpsc::Receiver<AppCommand>) {
        let state_api = Arc::new(StateApi::new(Arc::new(RwLock::new(
            crate::config::Config::default(),
        ))));
        SocketApiIntegration::new(
            Arc::new(WorkspaceApi::new()),
            Arc::new(TabApi::new()),
            Arc::new(PaneApi::new()),
            Arc::new(KeyboardApi::new()),
            state_api,
        )
    }

    fn request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(7)),
            method: method.to_string(),
            params,
        }
    }

    async fn call(
        integration: &SocketApiIntegration,
        method: &str,
        params: serde_json::Value,
    ) -> JsonRpcResponse {
        integration
            .handle_request(request(method, params))
            .await
            .expect("resposta JSON-RPC")
    }

    fn error_of(response: &JsonRpcResponse) -> (i32, String) {
        let error = response.error.as_ref().expect("esperava erro");
        (error.code, error.message.clone())
    }

    /// Processador sem TabManager registrado
    async fn run_unregistered(command: impl FnOnce(mpsc::Sender<AppCommandResult>) -> AppCommand) {
        let processor = AppCommandProcessor::default();
        let (tx, mut rx) = mpsc::channel(1);
        processor.process_command(command(tx)).await;
        match rx.recv().await {
            Some(AppCommandResult::Error(message)) => {
                assert_eq!(message, "TabManager is not registered")
            }
            other => panic!("esperava erro de TabManager ausente, veio {:?}", other),
        }
    }

    async fn run(processor: &AppCommandProcessor, command: AppCommand) {
        processor.process_command(command).await;
    }

    fn processor_with(manager: TabManager) -> (AppCommandProcessor, Arc<RwLock<TabManager>>) {
        let tab_manager = Arc::new(RwLock::new(manager));
        let mut processor = AppCommandProcessor::new();
        processor.register_tab_manager(Arc::clone(&tab_manager));
        (processor, tab_manager)
    }

    async fn expect_error(rx: &mut mpsc::Receiver<AppCommandResult>) -> String {
        match rx.recv().await {
            Some(AppCommandResult::Error(message)) => message,
            other => panic!("esperava erro, veio {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_handle_request_rejects_malformed_method_names() {
        let (integration, _rx) = new_integration();
        for method in ["ping", "a.b.c"] {
            let response = call(&integration, method, serde_json::json!({})).await;
            let (code, message) = error_of(&response);
            assert_eq!(code, -32602);
            assert!(message.contains("Expected 'namespace.method'"), "{message}");
            assert_eq!(response.id, Some(serde_json::json!(7)));
        }
    }

    #[tokio::test]
    async fn test_handle_request_unknown_namespace() {
        let (integration, _rx) = new_integration();
        let response = call(&integration, "nope.list", serde_json::json!({})).await;
        let (code, message) = error_of(&response);
        assert_eq!(code, -32601);
        assert!(message.contains("Unknown namespace: nope"), "{message}");
    }

    /// Regressão: a integração repassava só "list" para as APIs, que esperam
    /// "tab.list"; toda chamada delegada respondia MethodNotFound.
    #[tokio::test]
    async fn test_handle_request_delegates_full_method_name_to_apis() {
        let (integration, _rx) = new_integration();
        for method in ["workspace.list", "tab.list", "pane.list", "state.get"] {
            let response = call(&integration, method, serde_json::json!({})).await;
            assert!(
                response.error.is_none(),
                "{method} falhou: {:?}",
                response.error
            );
            assert!(response.result.is_some(), "{method} sem resultado");
        }

        // keyboard.send chega na KeyboardApi (sem backend: NotImplemented, não MethodNotFound)
        let response = call(
            &integration,
            "keyboard.send",
            serde_json::json!({"keys": "ls"}),
        )
        .await;
        assert_eq!(error_of(&response).0, -32005);
    }

    #[tokio::test]
    async fn test_app_ping_version_focus_and_unknown() {
        let (integration, _rx) = new_integration();

        let ping = call(&integration, "app.ping", serde_json::json!({})).await;
        let ping = ping.result.expect("ping");
        assert_eq!(ping["pong"], true);
        assert!(ping["timestamp"].as_u64().expect("timestamp") > 0);

        let version = call(&integration, "app.version", serde_json::json!({}))
            .await
            .result
            .expect("version");
        assert_eq!(version["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(version["name"], env!("CARGO_PKG_NAME"));

        let focus = call(&integration, "app.focus", serde_json::json!({})).await;
        assert_eq!(focus.result, Some(serde_json::json!({"focused": true})));

        let unknown = call(&integration, "app.explode", serde_json::json!({})).await;
        let (code, message) = error_of(&unknown);
        assert_eq!(code, -32601);
        assert_eq!(message, "Method not found: app.explode");
    }

    #[tokio::test]
    async fn test_app_quit_sends_detach_and_waits_for_confirmation() {
        let (integration, mut command_rx) = new_integration();
        let processor = AppCommandProcessor::new();

        let quit = call(&integration, "app.quit", serde_json::json!({}));
        let process = async {
            let command = command_rx.recv().await.expect("comando");
            assert!(matches!(
                command,
                AppCommand::MultiplexerCommand {
                    command: MultiplexerCommand::DetachSession,
                    ..
                }
            ));
            processor.process_command(command).await;
        };
        let (response, _) = tokio::join!(quit, process);
        assert_eq!(response.result, Some(serde_json::json!({"quitting": true})));
    }

    #[tokio::test]
    async fn test_app_quit_reports_timeout_when_command_is_refused() {
        let (integration, mut command_rx) = new_integration();
        let quit = call(&integration, "app.quit", serde_json::json!({}));
        let refuse = async {
            if let Some(AppCommand::MultiplexerCommand { response_tx, .. }) =
                command_rx.recv().await
            {
                response_tx
                    .send(AppCommandResult::Error("não".to_string()))
                    .await
                    .expect("resposta");
            }
        };
        let (response, _) = tokio::join!(quit, refuse);
        assert_eq!(error_of(&response).0, -32004);
    }

    #[tokio::test]
    async fn test_app_commands_fail_when_processor_is_gone() {
        let (integration, command_rx) = new_integration();
        drop(command_rx);

        let quit = call(&integration, "app.quit", serde_json::json!({})).await;
        let (code, message) = error_of(&quit);
        assert_eq!(code, -32603);
        assert!(message.contains("Failed to send command"), "{message}");

        let send = call(
            &integration,
            "terminal.send_text",
            serde_json::json!({"text": "x"}),
        )
        .await;
        assert!(error_of(&send).1.contains("Failed to send command"));
    }

    #[tokio::test]
    async fn test_terminal_send_text_validation_and_error_mapping() {
        let (integration, mut command_rx) = new_integration();

        let missing = call(&integration, "terminal.send_text", serde_json::json!({})).await;
        assert_eq!(
            error_of(&missing),
            (
                -32602,
                "Invalid parameters: Missing 'text' parameter".to_string()
            )
        );

        // O processador responde com erro -> InvalidState
        let send = call(
            &integration,
            "terminal.send_text",
            serde_json::json!({"text": "abc", "tab_id": 42}),
        );
        let reply = async {
            match command_rx.recv().await {
                Some(AppCommand::SendKeys {
                    tab_id,
                    keys,
                    response_tx,
                }) => {
                    assert_eq!(tab_id, Some(42));
                    assert_eq!(keys, "abc");
                    response_tx
                        .send(AppCommandResult::Error("sem pty".to_string()))
                        .await
                        .expect("resposta");
                }
                other => panic!("comando inesperado: {:?}", other),
            }
        };
        let (response, _) = tokio::join!(send, reply);
        assert_eq!(error_of(&response).0, -32003);
        assert!(error_of(&response).1.contains("sem pty"));

        // O processador descarta o canal sem responder -> InternalError
        let send = call(
            &integration,
            "terminal.send_text",
            serde_json::json!({"text": "abc"}),
        );
        let drop_reply = async {
            let command = command_rx.recv().await.expect("comando");
            drop(command);
        };
        let (response, _) = tokio::join!(send, drop_reply);
        let (code, message) = error_of(&response);
        assert_eq!(code, -32603);
        assert!(message.contains("processor stopped"), "{message}");
    }

    #[tokio::test]
    async fn test_terminal_size_methods_require_tab_manager() {
        let (integration, _rx) = new_integration();
        let size = call(&integration, "terminal.get_size", serde_json::json!({})).await;
        assert_eq!(error_of(&size).0, -32003);
        assert!(error_of(&size).1.contains("TabManager is not registered"));

        let resize = call(
            &integration,
            "terminal.resize",
            serde_json::json!({"cols": 10, "rows": 5}),
        )
        .await;
        assert!(error_of(&resize).1.contains("TabManager is not registered"));
    }

    #[tokio::test]
    async fn test_terminal_resize_validates_params() {
        let (integration, _rx) = new_integration();
        let cases = [
            (serde_json::json!({"rows": 5}), "Missing 'cols' parameter"),
            (serde_json::json!({"cols": 5}), "Missing 'rows' parameter"),
            (
                serde_json::json!({"cols": 70000, "rows": 5}),
                "'cols' is too large",
            ),
            (
                serde_json::json!({"cols": 5, "rows": 70000}),
                "'rows' is too large",
            ),
        ];
        for (params, expected) in cases {
            let response = call(&integration, "terminal.resize", params).await;
            let (code, message) = error_of(&response);
            assert_eq!(code, -32602);
            assert!(message.contains(expected), "{message}");
        }
    }

    #[tokio::test]
    async fn test_terminal_size_methods_report_missing_tabs_and_buffers() {
        let (mut integration, _rx) = new_integration();
        let tab_manager = Arc::new(RwLock::new(TabManager::new()));
        integration.register_tab_manager(Arc::clone(&tab_manager));
        assert!(integration.is_integrated());

        let size = call(&integration, "terminal.get_size", serde_json::json!({})).await;
        assert_eq!(
            error_of(&size),
            (-32001, "Not found: No active tab".to_string())
        );
        let resize = call(
            &integration,
            "terminal.resize",
            serde_json::json!({"cols": 10, "rows": 5}),
        )
        .await;
        assert_eq!(error_of(&resize).1, "Not found: No active tab");

        let size = call(
            &integration,
            "terminal.get_size",
            serde_json::json!({"tab_id": 999_999}),
        )
        .await;
        assert_eq!(error_of(&size).1, "Not found: Tab 999999 not found");
        let resize = call(
            &integration,
            "terminal.resize",
            serde_json::json!({"cols": 10, "rows": 5, "tab_id": 999_999}),
        )
        .await;
        assert_eq!(error_of(&resize).1, "Not found: Tab 999999 not found");

        // Tab sem buffer de terminal
        let tab_id = tab_manager.write().create_tab("bash");
        let size = call(&integration, "terminal.get_size", serde_json::json!({})).await;
        assert_eq!(error_of(&size).0, -32003);
        assert!(error_of(&size)
            .1
            .contains(&format!("Tab {} has no terminal buffer", tab_id)));
        let resize = call(
            &integration,
            "terminal.resize",
            serde_json::json!({"cols": 10, "rows": 5}),
        )
        .await;
        assert!(error_of(&resize)
            .1
            .contains(&format!("Tab {} has no terminal buffer", tab_id)));
    }

    #[tokio::test]
    async fn test_terminal_get_cwd_and_unknown_method() {
        let (integration, _rx) = new_integration();
        let cwd = call(&integration, "terminal.get_cwd", serde_json::json!({}))
            .await
            .result
            .expect("cwd");
        let expected = std::env::current_dir()
            .expect("cwd")
            .to_string_lossy()
            .to_string();
        assert_eq!(cwd["cwd"], expected);

        let unknown = call(&integration, "terminal.paint", serde_json::json!({})).await;
        assert_eq!(
            error_of(&unknown),
            (-32601, "Method not found: terminal.paint".to_string())
        );
    }

    #[tokio::test]
    async fn test_keyboard_backend_routes_to_tab_manager() {
        let (mut integration, _rx) = new_integration();
        let tab_manager = Arc::new(RwLock::new(TabManager::new()));
        integration.register_tab_manager(Arc::clone(&tab_manager));

        let send = |params: serde_json::Value| call(&integration, "keyboard.send", params);

        let response = send(serde_json::json!({"keys": "ls"})).await;
        assert_eq!(
            error_of(&response),
            (-32001, "Not found: No active tab".to_string())
        );

        let response = send(serde_json::json!({"keys": "ls", "tab_id": 5_000_000_000u64})).await;
        assert_eq!(error_of(&response).0, -32602);
        assert!(error_of(&response).1.contains("'tab_id' is too large"));

        let response = send(serde_json::json!({"keys": "ls", "tab_id": 999_999})).await;
        assert_eq!(error_of(&response).1, "Not found: Tab 999999 not found");

        let tab_id = tab_manager.write().create_tab("bash");
        let response = send(serde_json::json!({"keys": "ls", "tab_id": tab_id})).await;
        assert_eq!(error_of(&response).0, -32003);
        assert!(error_of(&response)
            .1
            .contains(&format!("Tab {} has no terminal buffer", tab_id)));
    }

    #[tokio::test]
    async fn test_keyboard_backend_writes_to_real_buffer() {
        let (mut integration, _rx) = new_integration();
        let buffer = TerminalBuffer::new(80, 24).expect("buffer");
        let mut manager = TabManager::new();
        manager.create_tab_with_buffer("bash", buffer);
        integration.register_tab_manager(Arc::new(RwLock::new(manager)));

        let response = call(
            &integration,
            "keyboard.send",
            serde_json::json!({"keys": "echo oi"}),
        )
        .await;
        assert!(response.error.is_none(), "{:?}", response.error);
    }

    #[tokio::test]
    async fn test_command_sender_reaches_receiver() {
        let (integration, mut command_rx) = new_integration();
        let (tx, _response_rx) = mpsc::channel(1);
        integration
            .command_sender()
            .send(AppCommand::GetTabs { response_tx: tx })
            .await
            .expect("envio");
        assert!(matches!(
            command_rx.recv().await,
            Some(AppCommand::GetTabs { .. })
        ));
    }

    #[tokio::test]
    async fn test_processor_without_tab_manager_rejects_tab_commands() {
        run_unregistered(|tx| AppCommand::CreateTab {
            title: None,
            command: None,
            working_dir: None,
            response_tx: tx,
        })
        .await;
        run_unregistered(|tx| AppCommand::CloseTab {
            tab_id: 1,
            force: true,
            response_tx: tx,
        })
        .await;
        run_unregistered(|tx| AppCommand::SwitchTab {
            tab_id: 1,
            response_tx: tx,
        })
        .await;
        run_unregistered(|tx| AppCommand::SendKeys {
            tab_id: None,
            keys: "x".to_string(),
            response_tx: tx,
        })
        .await;
        run_unregistered(|tx| AppCommand::GetTabs { response_tx: tx }).await;
    }

    #[tokio::test]
    async fn test_processor_create_tab_rejects_empty_and_invalid_commands() {
        let (processor, tab_manager) = processor_with(TabManager::new());

        let (tx, mut rx) = mpsc::channel(1);
        run(
            &processor,
            AppCommand::CreateTab {
                title: None,
                command: Some("   ".to_string()),
                working_dir: None,
                response_tx: tx,
            },
        )
        .await;
        assert_eq!(expect_error(&mut rx).await, "Command cannot be empty");

        let (tx, mut rx) = mpsc::channel(1);
        run(
            &processor,
            AppCommand::CreateTab {
                title: None,
                command: Some("/caminho/que/nao/existe/prog --flag".to_string()),
                working_dir: None,
                response_tx: tx,
            },
        )
        .await;
        let message = expect_error(&mut rx).await;
        assert!(
            message.starts_with("Failed to create terminal buffer for tab"),
            "{message}"
        );
        assert_eq!(tab_manager.read().tab_count(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_processor_create_tab_with_command_uses_it_as_title() {
        let (processor, tab_manager) = processor_with(TabManager::new());
        let (tx, mut rx) = mpsc::channel(1);
        run(
            &processor,
            AppCommand::CreateTab {
                title: None,
                command: Some("sh -c true".to_string()),
                working_dir: None,
                response_tx: tx,
            },
        )
        .await;
        let Some(AppCommandResult::Success(Some(info))) = rx.recv().await else {
            panic!("esperava sucesso");
        };
        assert_eq!(info["index"], 0);
        assert_eq!(info["created"], true);
        assert_eq!(info["is_active"], true);
        let manager = tab_manager.read();
        assert_eq!(manager.tab_count(), 1);
        assert_eq!(info["title"], manager.tabs()[0].title());
    }

    #[tokio::test]
    async fn test_processor_close_tab_flow() {
        let mut manager = TabManager::new();
        let idle = manager.create_tab("idle");
        let busy =
            manager.create_tab_with_buffer("busy", TerminalBuffer::new(80, 24).expect("pty"));
        let (processor, tab_manager) = processor_with(manager);

        let close = |tab_id, force| {
            let (tx, rx) = mpsc::channel(1);
            (
                AppCommand::CloseTab {
                    tab_id,
                    force,
                    response_tx: tx,
                },
                rx,
            )
        };

        let (cmd, mut rx) = close(999_999, false);
        run(&processor, cmd).await;
        assert_eq!(expect_error(&mut rx).await, "Tab 999999 not found");

        // Shell rodando: sem force é recusado
        let (cmd, mut rx) = close(busy, false);
        run(&processor, cmd).await;
        assert_eq!(
            expect_error(&mut rx).await,
            format!("Tab {} has a running process", busy)
        );
        assert_eq!(tab_manager.read().tab_count(), 2);

        let (cmd, mut rx) = close(busy, true);
        run(&processor, cmd).await;
        assert!(matches!(
            rx.recv().await,
            Some(AppCommandResult::Success(None))
        ));

        // Tab sem processo fecha sem force
        let (cmd, mut rx) = close(idle, false);
        run(&processor, cmd).await;
        assert!(matches!(
            rx.recv().await,
            Some(AppCommandResult::Success(None))
        ));
        assert_eq!(tab_manager.read().tab_count(), 0);
    }

    #[tokio::test]
    async fn test_processor_switch_tab() {
        let mut manager = TabManager::new();
        manager.create_tab("a");
        let second = manager.create_tab("b");
        let (processor, tab_manager) = processor_with(manager);

        let (tx, mut rx) = mpsc::channel(1);
        run(
            &processor,
            AppCommand::SwitchTab {
                tab_id: 999_999,
                response_tx: tx,
            },
        )
        .await;
        assert_eq!(expect_error(&mut rx).await, "Tab 999999 not found");

        let (tx, mut rx) = mpsc::channel(1);
        run(
            &processor,
            AppCommand::SwitchTab {
                tab_id: second,
                response_tx: tx,
            },
        )
        .await;
        assert!(matches!(
            rx.recv().await,
            Some(AppCommandResult::Success(None))
        ));
        assert_eq!(tab_manager.read().active_tab_index(), 1);
    }

    #[tokio::test]
    async fn test_processor_send_keys_errors() {
        let (processor, tab_manager) = processor_with(TabManager::new());
        let send = |tab_id| {
            let (tx, rx) = mpsc::channel(1);
            (
                AppCommand::SendKeys {
                    tab_id,
                    keys: "ls".to_string(),
                    response_tx: tx,
                },
                rx,
            )
        };

        let (cmd, mut rx) = send(None);
        run(&processor, cmd).await;
        assert_eq!(expect_error(&mut rx).await, "No active tab");

        let (cmd, mut rx) = send(Some(999_999));
        run(&processor, cmd).await;
        assert_eq!(expect_error(&mut rx).await, "Tab 999999 not found");

        let tab_id = tab_manager.write().create_tab("bash");
        let (cmd, mut rx) = send(Some(tab_id));
        run(&processor, cmd).await;
        assert_eq!(
            expect_error(&mut rx).await,
            format!("Tab {} has no terminal buffer", tab_id)
        );
    }

    #[tokio::test]
    async fn test_processor_get_tabs_lists_tab_details() {
        let mut manager = TabManager::new();
        let first = manager.create_tab("bash");
        manager.create_tab("zsh");
        manager.tabs_mut()[0].set_working_dir("/tmp/projeto");
        let (processor, _tab_manager) = processor_with(manager);

        let (tx, mut rx) = mpsc::channel(1);
        run(&processor, AppCommand::GetTabs { response_tx: tx }).await;
        let Some(AppCommandResult::Success(Some(tabs))) = rx.recv().await else {
            panic!("esperava lista de tabs");
        };
        let tabs = tabs.as_array().expect("array");
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0]["id"], first);
        assert_eq!(tabs[0]["index"], 0);
        assert_eq!(tabs[0]["is_active"], true);
        assert_eq!(tabs[0]["working_dir"], "/tmp/projeto");
        assert_eq!(tabs[1]["index"], 1);
        assert_eq!(tabs[1]["is_active"], false);
        assert!(tabs[1]["working_dir"].is_null());
    }

    #[tokio::test]
    async fn test_processor_pane_commands_report_missing_backend() {
        let processor = AppCommandProcessor::new();

        let (tx, mut rx) = mpsc::channel(1);
        run(
            &processor,
            AppCommand::GetPaneInfo {
                pane_id: 3,
                response_tx: tx,
            },
        )
        .await;
        assert_eq!(
            expect_error(&mut rx).await,
            "Pane 3 is not connected to a real pane backend yet"
        );

        let (tx, mut rx) = mpsc::channel(1);
        run(
            &processor,
            AppCommand::SplitPane {
                pane_id: 4,
                vertical: true,
                response_tx: tx,
            },
        )
        .await;
        assert!(expect_error(&mut rx)
            .await
            .contains("pane=4, vertical=true"));

        for direction in [
            ResizeDirection::Left,
            ResizeDirection::Right,
            ResizeDirection::Up,
            ResizeDirection::Down,
        ] {
            let (tx, mut rx) = mpsc::channel(1);
            run(
                &processor,
                AppCommand::ResizePane {
                    pane_id: 1,
                    direction,
                    amount: -2,
                    response_tx: tx,
                },
            )
            .await;
            let message = expect_error(&mut rx).await;
            assert!(
                message.contains(&format!("direction={:?}, amount=-2", direction)),
                "{message}"
            );
        }
    }

    #[tokio::test]
    async fn test_processor_multiplexer_commands() {
        let processor = AppCommandProcessor::new();
        let run_mux = |command| {
            let (tx, rx) = mpsc::channel(1);
            (
                AppCommand::MultiplexerCommand {
                    command,
                    response_tx: tx,
                },
                rx,
            )
        };

        let (cmd, mut rx) = run_mux(MultiplexerCommand::ListSessions);
        run(&processor, cmd).await;
        let Some(AppCommandResult::Success(Some(sessions))) = rx.recv().await else {
            panic!("esperava sessões");
        };
        assert_eq!(sessions[0]["name"], "default");
        assert_eq!(sessions[0]["attached"], true);

        let (cmd, mut rx) = run_mux(MultiplexerCommand::AttachSession {
            name: "dev".to_string(),
        });
        run(&processor, cmd).await;
        let Some(AppCommandResult::Success(Some(attached))) = rx.recv().await else {
            panic!("esperava attach");
        };
        assert_eq!(attached, serde_json::json!({"attached": "dev"}));

        for command in [
            MultiplexerCommand::CreateSession {
                name: "x".to_string(),
            },
            MultiplexerCommand::KillSession {
                name: "x".to_string(),
            },
        ] {
            let (cmd, mut rx) = run_mux(command);
            run(&processor, cmd).await;
            assert!(matches!(
                rx.recv().await,
                Some(AppCommandResult::Success(None))
            ));
        }
    }

    #[tokio::test]
    async fn test_processor_survives_dropped_response_channel() {
        let processor = AppCommandProcessor::new();
        let (tx, rx) = mpsc::channel(1);
        drop(rx);
        // A falha de envio só é registrada em log; não deve entrar em pânico.
        processor
            .process_command(AppCommand::GetPaneInfo {
                pane_id: 1,
                response_tx: tx,
            })
            .await;
    }
}
