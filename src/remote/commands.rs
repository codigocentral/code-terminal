//! Comandos remotos - Implementação dos métodos JSON-RPC
//!
//! Conecta os handlers JSON-RPC às funcionalidades reais do terminal.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::remote::handler::RpcHandler;
use crate::remote::protocol::RpcError;

/// Registra todos os comandos remotos no handler
pub async fn register_remote_commands(handler: &RpcHandler, state: Arc<RemoteState>) {
    // Info commands
    register_info_commands(handler, state.clone()).await;

    // Tab commands
    register_tab_commands(handler, state.clone()).await;

    // Terminal commands
    register_terminal_commands(handler, state.clone()).await;

    // Config commands
    register_config_commands(handler, state.clone()).await;

    // Session commands
    register_session_commands(handler, state.clone()).await;

    // Workspace commands
    register_workspace_commands(handler, state.clone()).await;

    // System commands
    register_system_commands(handler, state.clone()).await;
}

/// Estado compartilhado para comandos remotos
#[derive(Debug)]
pub struct RemoteState {
    /// Versão do terminal
    pub version: String,
    /// Informações sobre tabs
    pub tabs: RwLock<Vec<TabInfo>>,
    /// Configuração atual
    pub config: RwLock<std::collections::HashMap<String, String>>,
    /// Histórico de comandos
    pub command_history: RwLock<Vec<String>>,
    /// Contador de comandos executados
    pub command_count: RwLock<u64>,
    /// Workspaces
    pub workspaces: RwLock<Vec<WorkspaceInfo>>,
    /// Workspace ativo
    pub active_workspace: RwLock<usize>,
    /// Sessions
    pub sessions: RwLock<Vec<SessionInfo>>,
}

impl Default for RemoteState {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteState {
    /// Criar novo estado
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            tabs: RwLock::new(Vec::new()),
            config: RwLock::new(std::collections::HashMap::new()),
            command_history: RwLock::new(Vec::new()),
            command_count: RwLock::new(0),
            workspaces: RwLock::new(vec![WorkspaceInfo::default()]),
            active_workspace: RwLock::new(0),
            sessions: RwLock::new(Vec::new()),
        }
    }

    /// Adicionar tab de teste
    pub async fn add_test_tab(&self, title: impl Into<String>) -> usize {
        let mut tabs = self.tabs.write().await;
        let index = tabs.len();
        let is_first = tabs.is_empty();
        let id = next_tab_id(&tabs);
        tabs.push(TabInfo {
            id,
            index,
            title: title.into(),
            is_active: is_first,
            pid: None,
            cwd: Some(
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
        });
        index
    }

    /// Adicionar comando ao histórico
    pub async fn add_command_history(&self, command: impl Into<String>) {
        let mut history = self.command_history.write().await;
        let cmd = command.into();
        if !cmd.is_empty() {
            history.push(cmd);
            // Limita histórico a 1000 comandos
            if history.len() > 1000 {
                history.remove(0);
            }
        }

        let mut count = self.command_count.write().await;
        *count += 1;
    }

    /// Incrementar contador de comandos
    pub async fn increment_command_count(&self) {
        let mut count = self.command_count.write().await;
        *count += 1;
    }
}

/// Próximo ID de tab livre.
///
/// O ID não pode ser derivado do tamanho da lista: depois de um `tab.close`
/// isso repetiria o ID de uma tab que ainda existe.
fn next_tab_id(tabs: &[TabInfo]) -> u64 {
    tabs.iter().map(|tab| tab.id + 1).max().unwrap_or(0)
}

/// Próximo ID de sessão livre (`session-N`), sem repetir um ID existente.
fn next_session_id(sessions: &[SessionInfo]) -> String {
    let mut n = sessions.len();
    loop {
        let id = format!("session-{}", n);
        if !sessions.iter().any(|session| session.id == id) {
            return id;
        }
        n += 1;
    }
}

/// Informações sobre uma tab
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TabInfo {
    /// ID único da tab
    pub id: u64,
    /// Índice da tab
    pub index: usize,
    /// Título da tab
    pub title: String,
    /// Se é a tab ativa
    pub is_active: bool,
    /// PID do processo (se houver)
    pub pid: Option<u32>,
    /// Diretório atual
    pub cwd: Option<String>,
}

/// Informações sobre o terminal
#[derive(Debug, Clone, Serialize)]
pub struct TerminalInfo {
    pub version: String,
    pub tab_count: usize,
    pub active_tab: usize,
    pub size: TerminalSize,
}

/// Tamanho do terminal
#[derive(Debug, Clone, Serialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
}

/// Informações sobre workspace
#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInfo {
    pub id: usize,
    pub name: String,
    pub tab_count: usize,
}

impl Default for WorkspaceInfo {
    fn default() -> Self {
        Self {
            id: 0,
            name: "Default".to_string(),
            tab_count: 0,
        }
    }
}

/// Informações sobre sessão
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub tab_count: usize,
}

/// Registra comandos de informação
async fn register_info_commands(handler: &RpcHandler, state: Arc<RemoteState>) {
    // ping
    handler
        .register(
            "ping",
            |_params| async move { Ok(serde_json::json!("pong")) },
        )
        .await;

    // server.info
    let state_clone = state.clone();
    handler
        .register("server.info", move |_params| {
            let state = state_clone.clone();
            async move {
                let tabs = state.tabs.read().await;
                let active_tab = tabs.iter().position(|t| t.is_active).unwrap_or(0);
                let workspaces = state.workspaces.read().await;
                let command_count = *state.command_count.read().await;

                Ok(serde_json::json!({
                    "name": "Code Terminal",
                    "version": state.version,
                    "protocol": "jsonrpc-2.0",
                    "tab_count": tabs.len(),
                    "active_tab": active_tab,
                    "workspace_count": workspaces.len(),
                    "command_count": command_count,
                }))
            }
        })
        .await;

    // rpc.listMethods
    handler
        .register("rpc.listMethods", |_params| async move {
            Ok(serde_json::json!([
                // Info
                "ping",
                "server.info",
                "server.stats",
                "rpc.listMethods",
                // Tabs
                "tab.list",
                "tab.get",
                "tab.new",
                "tab.close",
                "tab.goto",
                "tab.rename",
                "tab.duplicate",
                // Terminal
                "terminal.size",
                "terminal.resize",
                "terminal.input",
                "terminal.sendKeys",
                "terminal.info",
                "terminal.clear",
                "terminal.notify",
                // Config
                "config.get",
                "config.set",
                "config.list",
                "config.reset",
                // Session
                "session.save",
                "session.load",
                "session.list",
                "session.close",
                // Workspace
                "workspace.list",
                "workspace.switch",
                "workspace.create",
                "workspace.close",
                // System
                "system.getClipboard",
                "system.setClipboard",
            ]))
        })
        .await;

    // server.stats
    let state_clone = state.clone();
    handler
        .register("server.stats", move |_params| {
            let state = state_clone.clone();
            async move {
                let tabs = state.tabs.read().await;
                let history = state.command_history.read().await;
                let command_count = *state.command_count.read().await;

                Ok(serde_json::json!({
                    "tabs": tabs.len(),
                    "command_history_size": history.len(),
                    "commands_executed": command_count,
                }))
            }
        })
        .await;
}

/// Registra comandos de tabs
async fn register_tab_commands(handler: &RpcHandler, state: Arc<RemoteState>) {
    // tab.list
    let state_clone = state.clone();
    handler
        .register("tab.list", move |_params| {
            let state = state_clone.clone();
            async move {
                let tabs = state.tabs.read().await;
                let tabs_json: Vec<Value> = tabs
                    .iter()
                    .map(|t| serde_json::to_value(t).unwrap())
                    .collect();
                Ok(serde_json::json!(tabs_json))
            }
        })
        .await;

    // tab.get - obtém informações de uma tab específica
    let state_clone = state.clone();
    handler
        .register("tab.get", move |params| {
            let state = state_clone.clone();
            async move {
                let index = params
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| {
                        RpcError::invalid_params("Missing or invalid 'index' parameter")
                    })? as usize;

                let tabs = state.tabs.read().await;
                if index >= tabs.len() {
                    return Err(RpcError::invalid_params(format!(
                        "Invalid tab index: {}",
                        index
                    )));
                }

                Ok(serde_json::to_value(&tabs[index]).unwrap())
            }
        })
        .await;

    // tab.new
    let state_clone = state.clone();
    handler
        .register("tab.new", move |params| {
            let state = state_clone.clone();
            async move {
                let title = params
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("New Tab");
                let cwd = params.get("cwd").and_then(|v| v.as_str());

                let mut tabs = state.tabs.write().await;
                let index = tabs.len();
                let is_active = index == 0;
                let id = next_tab_id(&tabs);
                tabs.push(TabInfo {
                    id,
                    index,
                    title: title.to_string(),
                    is_active,
                    pid: None,
                    cwd: cwd.map(|s| s.to_string()),
                });

                Ok(serde_json::json!(index))
            }
        })
        .await;

    // tab.close
    let state_clone = state.clone();
    handler
        .register("tab.close", move |params| {
            let state = state_clone.clone();
            async move {
                let index = params
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'index' parameter"))?
                    as usize;

                let mut tabs = state.tabs.write().await;
                if index >= tabs.len() {
                    return Err(RpcError::invalid_params(format!(
                        "Invalid tab index: {}",
                        index
                    )));
                }
                tabs.remove(index);

                // Reindex remaining tabs
                for (i, tab) in tabs.iter_mut().enumerate() {
                    tab.index = i;
                }

                // Se não há tabs ativas, ativa a primeira
                if !tabs.is_empty() && !tabs.iter().any(|t| t.is_active) {
                    tabs[0].is_active = true;
                }

                Ok(serde_json::json!(null))
            }
        })
        .await;

    // tab.goto
    let state_clone = state.clone();
    handler
        .register("tab.goto", move |params| {
            let state = state_clone.clone();
            async move {
                let index = params
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'index' parameter"))?
                    as usize;

                let mut tabs = state.tabs.write().await;
                if index >= tabs.len() {
                    return Err(RpcError::invalid_params(format!(
                        "Invalid tab index: {}",
                        index
                    )));
                }

                // Deactivate all, activate target
                for (i, tab) in tabs.iter_mut().enumerate() {
                    tab.is_active = i == index;
                }

                Ok(serde_json::json!(null))
            }
        })
        .await;

    // tab.rename
    let state_clone = state.clone();
    handler
        .register("tab.rename", move |params| {
            let state = state_clone.clone();
            async move {
                let index = params
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'index' parameter"))?
                    as usize;
                let title = params
                    .get("title")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'title' parameter"))?;

                let mut tabs = state.tabs.write().await;
                if index >= tabs.len() {
                    return Err(RpcError::invalid_params(format!(
                        "Invalid tab index: {}",
                        index
                    )));
                }

                tabs[index].title = title.to_string();
                Ok(serde_json::json!(null))
            }
        })
        .await;

    // tab.duplicate
    let state_clone = state.clone();
    handler
        .register("tab.duplicate", move |params| {
            let state = state_clone.clone();
            async move {
                let index = params
                    .get("index")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'index' parameter"))?
                    as usize;

                let mut tabs = state.tabs.write().await;
                if index >= tabs.len() {
                    return Err(RpcError::invalid_params(format!(
                        "Invalid tab index: {}",
                        index
                    )));
                }

                let new_index = tabs.len();
                let original_title = tabs[index].title.clone();
                let original_cwd = tabs[index].cwd.clone();
                let id = next_tab_id(&tabs);
                tabs.push(TabInfo {
                    id,
                    index: new_index,
                    title: format!("{} (copy)", original_title),
                    is_active: false,
                    pid: None,
                    cwd: original_cwd,
                });

                Ok(serde_json::json!(new_index))
            }
        })
        .await;
}

/// Registra comandos de terminal
async fn register_terminal_commands(handler: &RpcHandler, state: Arc<RemoteState>) {
    // terminal.size
    handler
        .register("terminal.size", |_params| async move {
            Ok(serde_json::json!({
                "cols": 80,
                "rows": 24
            }))
        })
        .await;

    // terminal.resize
    handler
        .register("terminal.resize", |params| async move {
            let cols = params
                .get("cols")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("Missing 'cols' parameter"))?
                as u16;
            let rows = params
                .get("rows")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| RpcError::invalid_params("Missing 'rows' parameter"))?
                as u16;

            tracing::info!("Terminal resize requested: {}x{}", cols, rows);
            Ok(serde_json::json!({ "cols": cols, "rows": rows }))
        })
        .await;

    // terminal.input
    let state_clone = state.clone();
    handler
        .register("terminal.input", move |params| {
            let state = state_clone.clone();
            async move {
                let text = params
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'text' parameter"))?;

                state.add_command_history(text).await;
                tracing::info!("Terminal input received: {}", text);
                Ok(serde_json::json!(null))
            }
        })
        .await;

    // terminal.sendKeys
    handler
        .register("terminal.sendKeys", |params| async move {
            let keys = params
                .get("keys")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::invalid_params("Missing 'keys' parameter"))?;

            tracing::info!("Terminal keys received: {}", keys);
            Ok(serde_json::json!(null))
        })
        .await;

    // terminal.info
    let state_clone = state.clone();
    handler
        .register("terminal.info", move |_params| {
            let state = state_clone.clone();
            async move {
                let tabs = state.tabs.read().await;
                let active_tab = tabs.iter().position(|t| t.is_active).unwrap_or(0);

                Ok(serde_json::json!({
                    "version": state.version,
                    "tab_count": tabs.len(),
                    "active_tab": active_tab,
                    "size": {
                        "cols": 80,
                        "rows": 24
                    }
                }))
            }
        })
        .await;

    // terminal.clear
    handler
        .register("terminal.clear", |_params| async move {
            tracing::info!("Terminal clear requested");
            Ok(serde_json::json!(null))
        })
        .await;

    // terminal.notify
    handler
        .register("terminal.notify", |params| async move {
            let title = params
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Code Terminal");
            let message = params
                .get("message")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::invalid_params("Missing 'message' parameter"))?;

            tracing::info!("Notification: {} - {}", title, message);
            Ok(serde_json::json!(null))
        })
        .await;
}

/// Registra comandos de configuração
async fn register_config_commands(handler: &RpcHandler, state: Arc<RemoteState>) {
    // config.get
    let state_clone = state.clone();
    handler
        .register("config.get", move |params| {
            let state = state_clone.clone();
            async move {
                let key = params
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'key' parameter"))?;

                let config = state.config.read().await;
                let value = config.get(key).cloned().unwrap_or_default();
                Ok(serde_json::json!(value))
            }
        })
        .await;

    // config.set
    let state_clone = state.clone();
    handler
        .register("config.set", move |params| {
            let state = state_clone.clone();
            async move {
                let key = params
                    .get("key")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'key' parameter"))?;
                let value = params
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'value' parameter"))?;

                let mut config = state.config.write().await;
                config.insert(key.to_string(), value.to_string());
                Ok(serde_json::json!(null))
            }
        })
        .await;

    // config.list
    let state_clone = state.clone();
    handler
        .register("config.list", move |_params| {
            let state = state_clone.clone();
            async move {
                let config = state.config.read().await;
                let keys: Vec<String> = config.keys().cloned().collect();
                Ok(serde_json::json!(keys))
            }
        })
        .await;

    // config.reset
    let state_clone = state.clone();
    handler
        .register("config.reset", move |_params| {
            let state = state_clone.clone();
            async move {
                let mut config = state.config.write().await;
                config.clear();
                Ok(serde_json::json!(null))
            }
        })
        .await;
}

/// Registra comandos de sessão
async fn register_session_commands(handler: &RpcHandler, state: Arc<RemoteState>) {
    // session.save
    let state_clone = state.clone();
    handler
        .register("session.save", move |params| {
            let state = state_clone.clone();
            async move {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unnamed");

                let mut sessions = state.sessions.write().await;
                let id = next_session_id(&sessions);
                sessions.push(SessionInfo {
                    id: id.clone(),
                    name: name.to_string(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    tab_count: state.tabs.read().await.len(),
                });

                Ok(serde_json::json!({ "session_id": id }))
            }
        })
        .await;

    // session.load
    let state_clone = state.clone();
    handler
        .register("session.load", move |params| {
            let state = state_clone.clone();
            async move {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'id' parameter"))?;

                let sessions = state.sessions.read().await;
                if let Some(session) = sessions.iter().find(|s| s.id == id) {
                    Ok(serde_json::json!({
                        "loaded": true,
                        "session": session
                    }))
                } else {
                    Err(RpcError::invalid_params(format!(
                        "Session '{}' not found",
                        id
                    )))
                }
            }
        })
        .await;

    // session.list
    let state_clone = state.clone();
    handler
        .register("session.list", move |_params| {
            let state = state_clone.clone();
            async move {
                let sessions = state.sessions.read().await;
                let sessions_json: Vec<Value> = sessions
                    .iter()
                    .map(|s| serde_json::to_value(s).unwrap())
                    .collect();
                Ok(serde_json::json!(sessions_json))
            }
        })
        .await;

    // session.close
    let state_clone = state.clone();
    handler
        .register("session.close", move |params| {
            let state = state_clone.clone();
            async move {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'id' parameter"))?;

                let mut sessions = state.sessions.write().await;
                let initial_len = sessions.len();
                sessions.retain(|s| s.id != id);

                if sessions.len() < initial_len {
                    Ok(serde_json::json!({ "closed": true }))
                } else {
                    Err(RpcError::invalid_params(format!(
                        "Session '{}' not found",
                        id
                    )))
                }
            }
        })
        .await;
}

/// Registra comandos de workspace
async fn register_workspace_commands(handler: &RpcHandler, state: Arc<RemoteState>) {
    // workspace.list
    let state_clone = state.clone();
    handler
        .register("workspace.list", move |_params| {
            let state = state_clone.clone();
            async move {
                let workspaces = state.workspaces.read().await;
                let active = *state.active_workspace.read().await;

                let workspaces_json: Vec<Value> = workspaces
                    .iter()
                    .map(|w| {
                        let mut json = serde_json::to_value(w).unwrap();
                        if let Some(obj) = json.as_object_mut() {
                            obj.insert("is_active".to_string(), serde_json::json!(w.id == active));
                        }
                        json
                    })
                    .collect();
                Ok(serde_json::json!(workspaces_json))
            }
        })
        .await;

    // workspace.switch
    let state_clone = state.clone();
    handler
        .register("workspace.switch", move |params| {
            let state = state_clone.clone();
            async move {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'id' parameter"))?
                    as usize;

                let workspaces = state.workspaces.read().await;
                if id >= workspaces.len() {
                    return Err(RpcError::invalid_params(format!(
                        "Invalid workspace id: {}",
                        id
                    )));
                }
                drop(workspaces);

                let mut active = state.active_workspace.write().await;
                *active = id;

                Ok(serde_json::json!({ "active_workspace": id }))
            }
        })
        .await;

    // workspace.create
    let state_clone = state.clone();
    handler
        .register("workspace.create", move |params| {
            let state = state_clone.clone();
            async move {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("New Workspace");

                let mut workspaces = state.workspaces.write().await;
                let id = workspaces.len();
                workspaces.push(WorkspaceInfo {
                    id,
                    name: name.to_string(),
                    tab_count: 0,
                });

                Ok(serde_json::json!({ "workspace_id": id }))
            }
        })
        .await;

    // workspace.close
    let state_clone = state.clone();
    handler
        .register("workspace.close", move |params| {
            let state = state_clone.clone();
            async move {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| RpcError::invalid_params("Missing 'id' parameter"))?
                    as usize;

                let mut workspaces = state.workspaces.write().await;
                if id >= workspaces.len() {
                    return Err(RpcError::invalid_params(format!(
                        "Invalid workspace id: {}",
                        id
                    )));
                }

                if workspaces.len() <= 1 {
                    return Err(RpcError::invalid_params("Cannot close the last workspace"));
                }

                workspaces.retain(|w| w.id != id);

                // Reindex
                for (i, w) in workspaces.iter_mut().enumerate() {
                    w.id = i;
                }

                // Ajusta workspace ativo se necessário
                drop(workspaces);
                let mut active = state.active_workspace.write().await;
                if *active >= id && *active > 0 {
                    *active -= 1;
                }

                Ok(serde_json::json!(null))
            }
        })
        .await;
}

/// Registra comandos de sistema
async fn register_system_commands(handler: &RpcHandler, _state: Arc<RemoteState>) {
    // system.getClipboard
    handler
        .register("system.getClipboard", |_params| async {
            // Em implementação real, leria do clipboard
            Ok(serde_json::json!(""))
        })
        .await;

    // system.setClipboard
    handler
        .register("system.setClipboard", |params| async move {
            let text = params
                .get("text")
                .and_then(|v| v.as_str())
                .ok_or_else(|| RpcError::invalid_params("Missing 'text' parameter"))?;

            tracing::info!("Clipboard set: {} bytes", text.len());
            Ok(serde_json::json!(null))
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::protocol::error_codes;

    #[tokio::test]
    async fn test_remote_state_new() {
        let state = RemoteState::new();
        let tabs = state.tabs.read().await;
        assert!(tabs.is_empty());
    }

    #[tokio::test]
    async fn test_remote_state_add_tab() {
        let state = RemoteState::new();
        let index = state.add_test_tab("Test Tab").await;
        assert_eq!(index, 0);

        let tabs = state.tabs.read().await;
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "Test Tab");
        assert!(tabs[0].is_active);
    }

    #[tokio::test]
    async fn test_remote_state_add_command_history() {
        let state = RemoteState::new();
        state.add_command_history("ls -la").await;
        state.add_command_history("pwd").await;

        let history = state.command_history.read().await;
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], "ls -la");

        let count = state.command_count.read().await;
        assert_eq!(*count, 2);
    }

    #[tokio::test]
    async fn test_register_commands() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());

        register_remote_commands(&handler, state).await;

        // Verificar se métodos foram registrados
        assert!(handler.has_method("ping").await);
        assert!(handler.has_method("server.info").await);
        assert!(handler.has_method("tab.list").await);
        assert!(handler.has_method("tab.new").await);
        assert!(handler.has_method("config.get").await);
        assert!(handler.has_method("session.save").await);
        assert!(handler.has_method("workspace.list").await);
    }

    #[tokio::test]
    async fn test_ping_command() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state).await;

        let result = handler.execute_method("ping", None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!("pong"));
    }

    #[tokio::test]
    async fn test_tab_new_and_list() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state.clone()).await;

        // Create tab
        let result = handler
            .execute_method("tab.new", Some(serde_json::json!({"title": "Test"})))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!(0));

        // List tabs
        let result = handler.execute_method("tab.list", None).await;
        assert!(result.is_ok());

        let tabs: Vec<TabInfo> = serde_json::from_value(result.unwrap()).unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "Test");
    }

    #[tokio::test]
    async fn test_tab_close() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state.clone()).await;

        // Add two tabs
        state.add_test_tab("Tab 1").await;
        state.add_test_tab("Tab 2").await;

        // Close first tab
        let result = handler
            .execute_method("tab.close", Some(serde_json::json!({"index": 0})))
            .await;
        assert!(result.is_ok());

        let tabs = state.tabs.read().await;
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].title, "Tab 2");
        assert_eq!(tabs[0].index, 0); // Reindexed
    }

    #[tokio::test]
    async fn test_config_commands() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state.clone()).await;

        // Set config
        let result = handler
            .execute_method(
                "config.set",
                Some(serde_json::json!({"key": "font.size", "value": "14"})),
            )
            .await;
        assert!(result.is_ok());

        // Get config
        let result = handler
            .execute_method("config.get", Some(serde_json::json!({"key": "font.size"})))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!("14"));

        // Reset config
        let result = handler.execute_method("config.reset", None).await;
        assert!(result.is_ok());

        let result = handler
            .execute_method("config.get", Some(serde_json::json!({"key": "font.size"})))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!(""));
    }

    #[tokio::test]
    async fn test_tab_rename() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state.clone()).await;

        // Create tab
        state.add_test_tab("Old Name").await;

        // Rename tab
        let result = handler
            .execute_method(
                "tab.rename",
                Some(serde_json::json!({"index": 0, "title": "New Name"})),
            )
            .await;
        assert!(result.is_ok());

        let tabs = state.tabs.read().await;
        assert_eq!(tabs[0].title, "New Name");
    }

    #[tokio::test]
    async fn test_tab_duplicate() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state.clone()).await;

        // Create tab
        state.add_test_tab("Original").await;

        // Duplicate tab
        let result = handler
            .execute_method("tab.duplicate", Some(serde_json::json!({"index": 0})))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!(1));

        let tabs = state.tabs.read().await;
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[1].title, "Original (copy)");
    }

    #[tokio::test]
    async fn test_session_commands() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state.clone()).await;

        // Save session
        let result = handler
            .execute_method(
                "session.save",
                Some(serde_json::json!({"name": "My Session"})),
            )
            .await;
        assert!(result.is_ok());

        let save_result = result.unwrap();
        let session_id = save_result.get("session_id").unwrap().as_str().unwrap();

        // List sessions
        let result = handler.execute_method("session.list", None).await;
        assert!(result.is_ok());

        let sessions: Vec<SessionInfo> = serde_json::from_value(result.unwrap()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "My Session");

        // Load session
        let result = handler
            .execute_method("session.load", Some(serde_json::json!({"id": session_id})))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_workspace_commands() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state.clone()).await;

        // List workspaces
        let result = handler.execute_method("workspace.list", None).await;
        assert!(result.is_ok());

        // Create workspace
        let result = handler
            .execute_method("workspace.create", Some(serde_json::json!({"name": "Dev"})))
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), serde_json::json!({"workspace_id": 1}));

        // Switch workspace
        let result = handler
            .execute_method("workspace.switch", Some(serde_json::json!({"id": 1})))
            .await;
        assert!(result.is_ok());

        let active = state.active_workspace.read().await;
        assert_eq!(*active, 1);
    }

    #[tokio::test]
    async fn test_invalid_params() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state).await;

        // Missing required parameter
        let result = handler
            .execute_method("tab.close", Some(serde_json::json!({})))
            .await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.code(), error_codes::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_server_stats() {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::new());
        register_remote_commands(&handler, state.clone()).await;

        // Add some data
        state.add_test_tab("Tab 1").await;
        state.add_command_history("ls").await;

        // Get stats
        let result = handler.execute_method("server.stats", None).await;
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert_eq!(stats.get("tabs").unwrap(), 1);
        assert_eq!(stats.get("commands_executed").unwrap(), 1);
    }

    async fn setup() -> (RpcHandler, Arc<RemoteState>) {
        let handler = RpcHandler::new();
        let state = Arc::new(RemoteState::default());
        register_remote_commands(&handler, state.clone()).await;
        (handler, state)
    }

    async fn call(handler: &RpcHandler, method: &str, params: Value) -> Result<Value, RpcError> {
        handler.execute_method(method, Some(params)).await
    }

    /// Garante que o erro é InvalidParams com a mensagem esperada.
    fn assert_invalid(result: Result<Value, RpcError>, message: &str) {
        let error = result.expect_err("esperava erro");
        assert_eq!(error.code(), error_codes::INVALID_PARAMS);
        assert_eq!(error.message(), message);
    }

    #[tokio::test]
    async fn test_history_is_capped_and_empty_commands_only_count() {
        let state = RemoteState::new();
        for i in 0..1001 {
            state.add_command_history(format!("cmd-{}", i)).await;
        }
        state.add_command_history("").await;
        state.increment_command_count().await;

        let history = state.command_history.read().await;
        assert_eq!(history.len(), 1000);
        assert_eq!(history[0], "cmd-1");
        assert_eq!(history[999], "cmd-1000");
        assert_eq!(*state.command_count.read().await, 1003);
    }

    #[tokio::test]
    async fn test_server_info_reports_state() {
        let (handler, state) = setup().await;
        state.add_test_tab("a").await;
        state.add_test_tab("b").await;
        state.tabs.write().await[0].is_active = false;
        state.tabs.write().await[1].is_active = true;
        state.add_command_history("ls").await;

        let info = call(&handler, "server.info", Value::Null).await.unwrap();
        assert_eq!(info["name"], "Code Terminal");
        assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(info["protocol"], "jsonrpc-2.0");
        assert_eq!(info["tab_count"], 2);
        assert_eq!(info["active_tab"], 1);
        assert_eq!(info["workspace_count"], 1);
        assert_eq!(info["command_count"], 1);
    }

    /// Regressão: server.stats e terminal.notify existiam mas não apareciam
    /// em rpc.listMethods.
    #[tokio::test]
    async fn test_list_methods_matches_registered_methods() {
        let (handler, _state) = setup().await;
        let listed = call(&handler, "rpc.listMethods", Value::Null)
            .await
            .unwrap();
        let mut listed: Vec<String> = serde_json::from_value(listed).unwrap();
        listed.sort();
        let mut registered = handler.list_methods().await;
        registered.sort();
        assert_eq!(listed, registered);
    }

    #[tokio::test]
    async fn test_tab_get_and_validation() {
        let (handler, state) = setup().await;
        state.add_test_tab("primeira").await;

        let tab = call(&handler, "tab.get", serde_json::json!({"index": 0}))
            .await
            .unwrap();
        assert_eq!(tab["title"], "primeira");
        assert_eq!(tab["is_active"], true);

        assert_invalid(
            call(&handler, "tab.get", serde_json::json!({})).await,
            "Missing or invalid 'index' parameter",
        );
        assert_invalid(
            call(&handler, "tab.get", serde_json::json!({"index": 5})).await,
            "Invalid tab index: 5",
        );
    }

    #[tokio::test]
    async fn test_tab_new_defaults_and_cwd() {
        let (handler, state) = setup().await;
        assert_eq!(
            call(&handler, "tab.new", serde_json::json!({}))
                .await
                .unwrap(),
            serde_json::json!(0)
        );
        assert_eq!(
            call(&handler, "tab.new", serde_json::json!({"cwd": "/srv"}))
                .await
                .unwrap(),
            serde_json::json!(1)
        );
        let tabs = state.tabs.read().await;
        assert_eq!(tabs[0].title, "New Tab");
        assert!(tabs[0].is_active);
        assert!(tabs[0].cwd.is_none());
        assert!(!tabs[1].is_active);
        assert_eq!(tabs[1].cwd.as_deref(), Some("/srv"));
    }

    /// Regressão: o ID vinha do tamanho da lista e repetia depois de fechar uma tab.
    #[tokio::test]
    async fn test_tab_ids_stay_unique_after_close() {
        let (handler, state) = setup().await;
        for _ in 0..3 {
            call(&handler, "tab.new", serde_json::json!({}))
                .await
                .unwrap();
        }
        call(&handler, "tab.close", serde_json::json!({"index": 0}))
            .await
            .unwrap();
        call(&handler, "tab.new", serde_json::json!({}))
            .await
            .unwrap();
        call(&handler, "tab.duplicate", serde_json::json!({"index": 0}))
            .await
            .unwrap();
        state.add_test_tab("teste").await;

        let tabs = state.tabs.read().await;
        let mut ids: Vec<u64> = tabs.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), tabs.len(), "IDs repetidos: {:?}", tabs);
        let indexes: Vec<usize> = tabs.iter().map(|t| t.index).collect();
        assert_eq!(indexes, vec![0, 1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_tab_close_reactivates_first_and_validates() {
        let (handler, state) = setup().await;
        state.add_test_tab("a").await;
        state.add_test_tab("b").await;
        // Fecha a ativa: a próxima vira ativa
        call(&handler, "tab.close", serde_json::json!({"index": 0}))
            .await
            .unwrap();
        {
            let tabs = state.tabs.read().await;
            assert_eq!(tabs.len(), 1);
            assert_eq!(tabs[0].title, "b");
            assert!(tabs[0].is_active);
            assert_eq!(tabs[0].index, 0);
        }
        assert_invalid(
            call(&handler, "tab.close", serde_json::json!({"index": 3})).await,
            "Invalid tab index: 3",
        );
        call(&handler, "tab.close", serde_json::json!({"index": 0}))
            .await
            .unwrap();
        assert!(state.tabs.read().await.is_empty());
    }

    #[tokio::test]
    async fn test_tab_goto_switches_active_tab() {
        let (handler, state) = setup().await;
        state.add_test_tab("a").await;
        state.add_test_tab("b").await;

        assert_eq!(
            call(&handler, "tab.goto", serde_json::json!({"index": 1}))
                .await
                .unwrap(),
            Value::Null
        );
        let active: Vec<bool> = state
            .tabs
            .read()
            .await
            .iter()
            .map(|t| t.is_active)
            .collect();
        assert_eq!(active, vec![false, true]);

        assert_invalid(
            call(&handler, "tab.goto", serde_json::json!({})).await,
            "Missing 'index' parameter",
        );
        assert_invalid(
            call(&handler, "tab.goto", serde_json::json!({"index": 2})).await,
            "Invalid tab index: 2",
        );
    }

    #[tokio::test]
    async fn test_tab_rename_and_duplicate_validation() {
        let (handler, state) = setup().await;
        state.add_test_tab("a").await;
        assert_invalid(
            call(&handler, "tab.rename", serde_json::json!({"title": "x"})).await,
            "Missing 'index' parameter",
        );
        assert_invalid(
            call(&handler, "tab.rename", serde_json::json!({"index": 0})).await,
            "Missing 'title' parameter",
        );
        assert_invalid(
            call(
                &handler,
                "tab.rename",
                serde_json::json!({"index": 9, "title": "x"}),
            )
            .await,
            "Invalid tab index: 9",
        );
        assert_invalid(
            call(&handler, "tab.duplicate", serde_json::json!({})).await,
            "Missing 'index' parameter",
        );
        assert_invalid(
            call(&handler, "tab.duplicate", serde_json::json!({"index": 4})).await,
            "Invalid tab index: 4",
        );
    }

    #[tokio::test]
    async fn test_terminal_commands() {
        let (handler, state) = setup().await;
        assert_eq!(
            call(&handler, "terminal.size", Value::Null).await.unwrap(),
            serde_json::json!({"cols": 80, "rows": 24})
        );

        assert_eq!(
            call(
                &handler,
                "terminal.resize",
                serde_json::json!({"cols": 120, "rows": 40})
            )
            .await
            .unwrap(),
            serde_json::json!({"cols": 120, "rows": 40})
        );
        assert_invalid(
            call(&handler, "terminal.resize", serde_json::json!({"rows": 1})).await,
            "Missing 'cols' parameter",
        );
        assert_invalid(
            call(&handler, "terminal.resize", serde_json::json!({"cols": 1})).await,
            "Missing 'rows' parameter",
        );

        call(
            &handler,
            "terminal.input",
            serde_json::json!({"text": "ls -la"}),
        )
        .await
        .unwrap();
        assert_eq!(
            state.command_history.read().await.as_slice(),
            ["ls -la".to_string()]
        );
        assert_invalid(
            call(&handler, "terminal.input", serde_json::json!({})).await,
            "Missing 'text' parameter",
        );

        call(
            &handler,
            "terminal.sendKeys",
            serde_json::json!({"keys": "C-c"}),
        )
        .await
        .unwrap();
        assert_invalid(
            call(&handler, "terminal.sendKeys", serde_json::json!({})).await,
            "Missing 'keys' parameter",
        );

        state.add_test_tab("a").await;
        let info = call(&handler, "terminal.info", Value::Null).await.unwrap();
        assert_eq!(info["tab_count"], 1);
        assert_eq!(info["active_tab"], 0);
        assert_eq!(info["size"], serde_json::json!({"cols": 80, "rows": 24}));
        assert_eq!(info["version"], env!("CARGO_PKG_VERSION"));

        assert_eq!(
            call(&handler, "terminal.clear", Value::Null).await.unwrap(),
            Value::Null
        );

        call(
            &handler,
            "terminal.notify",
            serde_json::json!({"message": "oi"}),
        )
        .await
        .unwrap();
        call(
            &handler,
            "terminal.notify",
            serde_json::json!({"title": "t", "message": "oi"}),
        )
        .await
        .unwrap();
        assert_invalid(
            call(
                &handler,
                "terminal.notify",
                serde_json::json!({"title": "t"}),
            )
            .await,
            "Missing 'message' parameter",
        );
    }

    #[tokio::test]
    async fn test_config_set_list_and_validation() {
        let (handler, _state) = setup().await;
        call(
            &handler,
            "config.set",
            serde_json::json!({"key": "theme", "value": "dark"}),
        )
        .await
        .unwrap();
        assert_eq!(
            call(&handler, "config.list", Value::Null).await.unwrap(),
            serde_json::json!(["theme"])
        );
        assert_eq!(
            call(&handler, "config.get", serde_json::json!({"key": "nada"}))
                .await
                .unwrap(),
            serde_json::json!("")
        );
        assert_invalid(
            call(&handler, "config.get", serde_json::json!({})).await,
            "Missing 'key' parameter",
        );
        assert_invalid(
            call(&handler, "config.set", serde_json::json!({"value": "x"})).await,
            "Missing 'key' parameter",
        );
        assert_invalid(
            call(&handler, "config.set", serde_json::json!({"key": "x"})).await,
            "Missing 'value' parameter",
        );
    }

    #[tokio::test]
    async fn test_session_close_and_unique_ids() {
        let (handler, state) = setup().await;
        let first = call(&handler, "session.save", serde_json::json!({}))
            .await
            .unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let second = call(&handler, "session.save", serde_json::json!({"name": "b"}))
            .await
            .unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(state.sessions.read().await[0].name, "unnamed");

        assert_eq!(
            call(&handler, "session.close", serde_json::json!({"id": first}))
                .await
                .unwrap(),
            serde_json::json!({"closed": true})
        );
        // Regressão: o novo ID não pode repetir o da sessão que sobrou
        let third = call(&handler, "session.save", serde_json::json!({}))
            .await
            .unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(third, second);

        assert_invalid(
            call(&handler, "session.close", serde_json::json!({"id": first})).await,
            &format!("Session '{}' not found", first),
        );
        assert_invalid(
            call(&handler, "session.close", serde_json::json!({})).await,
            "Missing 'id' parameter",
        );
        assert_invalid(
            call(&handler, "session.load", serde_json::json!({"id": "x"})).await,
            "Session 'x' not found",
        );
        assert_invalid(
            call(&handler, "session.load", serde_json::json!({})).await,
            "Missing 'id' parameter",
        );
    }

    #[tokio::test]
    async fn test_workspace_switch_and_close() {
        let (handler, state) = setup().await;
        let list = call(&handler, "workspace.list", Value::Null).await.unwrap();
        assert_eq!(list[0]["name"], "Default");
        assert_eq!(list[0]["is_active"], true);

        assert_invalid(
            call(&handler, "workspace.switch", serde_json::json!({"id": 3})).await,
            "Invalid workspace id: 3",
        );
        assert_invalid(
            call(&handler, "workspace.switch", serde_json::json!({})).await,
            "Missing 'id' parameter",
        );
        assert_invalid(
            call(&handler, "workspace.close", serde_json::json!({"id": 0})).await,
            "Cannot close the last workspace",
        );

        call(&handler, "workspace.create", serde_json::json!({}))
            .await
            .unwrap();
        call(
            &handler,
            "workspace.create",
            serde_json::json!({"name": "C"}),
        )
        .await
        .unwrap();
        call(&handler, "workspace.switch", serde_json::json!({"id": 2}))
            .await
            .unwrap();

        // Fecha um workspace antes do ativo: ids são reindexados e o ativo acompanha
        call(&handler, "workspace.close", serde_json::json!({"id": 1}))
            .await
            .unwrap();
        {
            let workspaces = state.workspaces.read().await;
            let names: Vec<&str> = workspaces.iter().map(|w| w.name.as_str()).collect();
            assert_eq!(names, vec!["Default", "C"]);
            let ids: Vec<usize> = workspaces.iter().map(|w| w.id).collect();
            assert_eq!(ids, vec![0, 1]);
        }
        assert_eq!(*state.active_workspace.read().await, 1);
        let list = call(&handler, "workspace.list", Value::Null).await.unwrap();
        assert_eq!(list[1]["is_active"], true);

        // Fecha um workspace depois do ativo: ativo não muda
        call(&handler, "workspace.switch", serde_json::json!({"id": 0}))
            .await
            .unwrap();
        call(&handler, "workspace.close", serde_json::json!({"id": 1}))
            .await
            .unwrap();
        assert_eq!(*state.active_workspace.read().await, 0);

        assert_invalid(
            call(&handler, "workspace.close", serde_json::json!({"id": 7})).await,
            "Invalid workspace id: 7",
        );
        assert_invalid(
            call(&handler, "workspace.close", serde_json::json!({})).await,
            "Missing 'id' parameter",
        );
    }

    #[tokio::test]
    async fn test_system_clipboard_commands() {
        let (handler, _state) = setup().await;
        assert_eq!(
            call(&handler, "system.getClipboard", Value::Null)
                .await
                .unwrap(),
            serde_json::json!("")
        );
        assert_eq!(
            call(
                &handler,
                "system.setClipboard",
                serde_json::json!({"text": "x"})
            )
            .await
            .unwrap(),
            Value::Null
        );
        assert_invalid(
            call(&handler, "system.setClipboard", serde_json::json!({})).await,
            "Missing 'text' parameter",
        );
    }
}
