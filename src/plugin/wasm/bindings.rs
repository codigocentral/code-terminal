//! Host Functions Bindings para WASM
//!
//! Define as funções que o host expõe para plugins WASM, conectadas
//! às APIs reais do terminal.

use crate::plugin::manifest::Permission;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::*;

/// Estado do host compartilhado entre instâncias
#[derive(Debug)]
pub struct HostState {
    /// ID do plugin
    pub plugin_id: String,
    /// Nome do plugin
    pub plugin_name: String,
    /// Permissões concedidas
    pub permissions: HashSet<Permission>,
    /// Logs acumulados
    pub logs: Vec<String>,
    /// Contador de operações
    pub operation_count: u64,
    /// Resource limiter
    pub limiter: ResourceLimiter,
    /// Flag de inicialização
    pub initialized: bool,
    /// Callback para enviar input ao terminal
    pub terminal_input_tx: Option<tokio::sync::mpsc::Sender<String>>,
    /// Callback para obter informações do terminal
    pub terminal_info_rx: Option<tokio::sync::mpsc::Receiver<TerminalInfoResponse>>,
    /// Armazenamento de dados do plugin (key-value)
    pub storage: std::collections::HashMap<String, String>,
}

/// Resposta de informações do terminal
#[derive(Debug, Clone)]
pub struct TerminalInfoResponse {
    pub cols: u16,
    pub rows: u16,
    pub active_tab: usize,
    pub tab_count: usize,
}

impl HostState {
    /// Cria um novo estado do host
    pub fn new(plugin_id: impl Into<String>, plugin_name: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            plugin_name: plugin_name.into(),
            permissions: HashSet::new(),
            logs: Vec::new(),
            operation_count: 0,
            limiter: ResourceLimiter::default(),
            initialized: false,
            terminal_input_tx: None,
            terminal_info_rx: None,
            storage: std::collections::HashMap::new(),
        }
    }

    /// Configura o canal de input do terminal
    pub fn set_terminal_input_tx(&mut self, tx: tokio::sync::mpsc::Sender<String>) {
        self.terminal_input_tx = Some(tx);
    }

    /// Adiciona uma permissão
    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions.insert(permission);
    }

    /// Verifica se tem uma permissão
    pub fn has_permission(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }

    /// Adiciona um log
    pub fn log(&mut self, level: &str, message: &str) {
        self.logs.push(format!("[{}] {}", level, message));
    }

    /// Incrementa o contador de operações
    pub fn increment_operations(&mut self) {
        self.operation_count += 1;
    }

    /// Marca como inicializado
    pub fn set_initialized(&mut self) {
        self.initialized = true;
    }
}

/// Resource limiter para WASM
#[derive(Debug, Clone)]
pub struct ResourceLimiter {
    max_memory: usize,
    max_table_elements: usize,
}

impl Default for ResourceLimiter {
    fn default() -> Self {
        Self {
            max_memory: 16 * 1024 * 1024, // 16MB
            max_table_elements: 1000,
        }
    }
}

impl ResourceLimiter {
    pub fn new(max_memory: usize, max_table_elements: usize) -> Self {
        Self {
            max_memory,
            max_table_elements,
        }
    }
}

unsafe impl Send for ResourceLimiter {}
unsafe impl Sync for ResourceLimiter {}

impl wasmtime::ResourceLimiter for ResourceLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> Result<bool> {
        Ok(desired <= self.max_memory)
    }

    fn table_growing(&mut self, _current: u32, desired: u32, _maximum: Option<u32>) -> Result<bool> {
        Ok(desired as usize <= self.max_table_elements)
    }
}

/// Coleção de host functions
pub struct WasmHostFunctions;

impl WasmHostFunctions {
    /// Registra todas as host functions em um linker
    pub fn register(linker: &mut Linker<HostState>) -> Result<()> {
        Self::register_logging(linker)?;
        Self::register_terminal(linker)?;
        Self::register_config(linker)?;
        Self::register_system(linker)?;
        Self::register_storage(linker)?;
        Self::register_ui(linker)?;
        Ok(())
    }

    fn register_logging(linker: &mut Linker<HostState>) -> Result<()> {
        linker.func_wrap(
            "host",
            "log_info",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let state = caller.data_mut();
                if let Some(message) = get_string_from_memory(&caller, ptr, len) {
                    state.log("INFO", &message);
                    tracing::info!("[plugin:{}] {}", state.plugin_name, message);
                }
                0
            },
        )?;

        linker.func_wrap(
            "host",
            "log_warn",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let state = caller.data_mut();
                if let Some(message) = get_string_from_memory(&caller, ptr, len) {
                    state.log("WARN", &message);
                    tracing::warn!("[plugin:{}] {}", state.plugin_name, message);
                }
                0
            },
        )?;

        linker.func_wrap(
            "host",
            "log_error",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let state = caller.data_mut();
                if let Some(message) = get_string_from_memory(&caller, ptr, len) {
                    state.log("ERROR", &message);
                    tracing::error!("[plugin:{}] {}", state.plugin_name, message);
                }
                0
            },
        )?;

        linker.func_wrap(
            "host",
            "log_debug",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
                let state = caller.data_mut();
                if let Some(message) = get_string_from_memory(&caller, ptr, len) {
                    state.log("DEBUG", &message);
                    tracing::debug!("[plugin:{}] {}", state.plugin_name, message);
                }
                0
            },
        )?;

        Ok(())
    }

    fn register_terminal(linker: &mut Linker<HostState>) -> Result<()> {
        // Escreve no terminal ativo
        linker.func_wrap(
            "host",
            "terminal_write",
            |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| -> i32 {
                let state = caller.data_mut();

                if !state.has_permission(Permission::Pty) {
                    return -1;
                }

                if let Some(data) = get_string_from_memory(&caller, ptr, len) {
                    state.increment_operations();
                    
                    // Envia para o terminal através do canal
                    if let Some(ref tx) = state.terminal_input_tx {
                        let _ = tx.try_send(data);
                        return 0;
                    }
                }
                -2
            },
        )?;

        // Obtém tamanho do terminal
        linker.func_wrap(
            "host",
            "terminal_get_size",
            |caller: Caller<'_, HostState>| -> i64 {
                let state = caller.data();
                if !state.has_permission(Permission::Pty) {
                    return -1;
                }
                // Retorna cols << 16 | rows
                80 << 16 | 24
            },
        )?;

        // Define tamanho do terminal (se permitido)
        linker.func_wrap(
            "host",
            "terminal_set_size",
            |mut caller: Caller<'_, HostState>, cols: i32, rows: i32| -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::Pty) {
                    return -1;
                }
                tracing::debug!("[plugin:{}] Requested terminal resize to {}x{}", 
                    state.plugin_name, cols, rows);
                // Em implementação real, enviaria comando ao terminal
                0
            },
        )?;

        // Obtém número de tabs
        linker.func_wrap(
            "host",
            "terminal_get_tab_count",
            |caller: Caller<'_, HostState>| -> i32 {
                let state = caller.data();
                if !state.has_permission(Permission::Pty) {
                    return -1;
                }
                // Placeholder - retornaria número real de tabs
                1
            },
        )?;

        // Obtém tab ativa
        linker.func_wrap(
            "host",
            "terminal_get_active_tab",
            |caller: Caller<'_, HostState>| -> i32 {
                let state = caller.data();
                if !state.has_permission(Permission::Pty) {
                    return -1;
                }
                // Placeholder - retornaria índice da tab ativa
                0
            },
        )?;

        // Muda para tab específica
        linker.func_wrap(
            "host",
            "terminal_goto_tab",
            |mut caller: Caller<'_, HostState>, index: i32| -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::Pty) {
                    return -1;
                }
                tracing::debug!("[plugin:{}] Requested goto tab {}", 
                    state.plugin_name, index);
                // Em implementação real, enviaria comando ao terminal
                0
            },
        )?;

        Ok(())
    }

    fn register_config(linker: &mut Linker<HostState>) -> Result<()> {
        // Obtém valor de configuração
        linker.func_wrap(
            "host",
            "config_get",
            |mut caller: Caller<'_, HostState>,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> i32 {
                let state = caller.data_mut();

                if !state.has_permission(Permission::ReadConfig) {
                    return -1;
                }

                if let Some(key) = get_string_from_memory(&caller, key_ptr, key_len) {
                    state.increment_operations();
                    
                    // Tenta obter do storage do plugin primeiro
                    let value = state.storage.get(&key).cloned()
                        .unwrap_or_else(|| "default_value".to_string());
                    
                    return write_string_to_memory(&mut caller, val_ptr, val_len, &value);
                }
                -2
            },
        )?;

        // Define valor de configuração
        linker.func_wrap(
            "host",
            "config_set",
            |mut caller: Caller<'_, HostState>,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> i32 {
                let state = caller.data_mut();

                if !state.has_permission(Permission::WriteConfig) {
                    return -1;
                }

                if let (Some(key), Some(value)) = (
                    get_string_from_memory(&caller, key_ptr, key_len),
                    get_string_from_memory(&caller, val_ptr, val_len),
                ) {
                    state.increment_operations();
                    state.storage.insert(key, value);
                    return 0;
                }
                -2
            },
        )?;

        // Lista todas as configurações
        linker.func_wrap(
            "host",
            "config_list",
            |mut caller: Caller<'_, HostState>, buf_ptr: i32, buf_len: i32| -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::ReadConfig) {
                    return -1;
                }

                let keys: Vec<String> = state.storage.keys().cloned().collect();
                let json = serde_json::to_string(&keys).unwrap_or_default();
                
                write_string_to_memory(&mut caller, buf_ptr, buf_len, &json)
            },
        )?;

        Ok(())
    }

    fn register_storage(linker: &mut Linker<HostState>) -> Result<()> {
        // Armazena dados persistentes
        linker.func_wrap(
            "host",
            "storage_set",
            |mut caller: Caller<'_, HostState>,
             key_ptr: i32,
             key_len: i32,
             val_ptr: i32,
             val_len: i32|
             -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::Filesystem) {
                    return -1;
                }

                if let (Some(key), Some(value)) = (
                    get_string_from_memory(&caller, key_ptr, key_len),
                    get_string_from_memory(&caller, val_ptr, val_len),
                ) {
                    state.storage.insert(key, value);
                    return 0;
                }
                -2
            },
        )?;

        // Recupera dados persistentes
        linker.func_wrap(
            "host",
            "storage_get",
            |mut caller: Caller<'_, HostState>,
             key_ptr: i32,
             key_len: i32,
             buf_ptr: i32,
             buf_len: i32|
             -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::Filesystem) {
                    return -1;
                }

                if let Some(key) = get_string_from_memory(&caller, key_ptr, key_len) {
                    let value = state.storage.get(&key).cloned().unwrap_or_default();
                    return write_string_to_memory(&mut caller, buf_ptr, buf_len, &value);
                }
                -2
            },
        )?;

        // Remove dados
        linker.func_wrap(
            "host",
            "storage_remove",
            |mut caller: Caller<'_, HostState>, key_ptr: i32, key_len: i32| -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::Filesystem) {
                    return -1;
                }

                if let Some(key) = get_string_from_memory(&caller, key_ptr, key_len) {
                    state.storage.remove(&key);
                    return 0;
                }
                -2
            },
        )?;

        // Limpa todos os dados
        linker.func_wrap(
            "host",
            "storage_clear",
            |mut caller: Caller<'_, HostState>| -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::Filesystem) {
                    return -1;
                }
                state.storage.clear();
                0
            },
        )?;

        Ok(())
    }

    fn register_ui(linker: &mut Linker<HostState>) -> Result<()> {
        // Mostra notificação
        linker.func_wrap(
            "host",
            "ui_notify",
            |mut caller: Caller<'_, HostState>,
             title_ptr: i32,
             title_len: i32,
             msg_ptr: i32,
             msg_len: i32|
             -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::Notifications) {
                    return -1;
                }

                if let (Some(title), Some(message)) = (
                    get_string_from_memory(&caller, title_ptr, title_len),
                    get_string_from_memory(&caller, msg_ptr, msg_len),
                ) {
                    tracing::info!("[plugin:{}] Notification: {} - {}", 
                        state.plugin_name, title, message);
                    // Em implementação real, mostraria notificação nativa
                    return 0;
                }
                -2
            },
        )?;

        // Mostra mensagem de status
        linker.func_wrap(
            "host",
            "ui_set_status",
            |mut caller: Caller<'_, HostState>, msg_ptr: i32, msg_len: i32| -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::CustomUi) {
                    return -1;
                }

                if let Some(message) = get_string_from_memory(&caller, msg_ptr, msg_len) {
                    tracing::info!("[plugin:{}] Status: {}", state.plugin_name, message);
                    return 0;
                }
                -2
            },
        )?;

        // Registra keybinding
        linker.func_wrap(
            "host",
            "ui_register_keybinding",
            |mut caller: Caller<'_, HostState>,
             keys_ptr: i32,
             keys_len: i32,
             action_ptr: i32,
             action_len: i32|
             -> i32 {
                let state = caller.data_mut();
                if !state.has_permission(Permission::CustomKeybindings) {
                    return -1;
                }

                if let (Some(keys), Some(action)) = (
                    get_string_from_memory(&caller, keys_ptr, keys_len),
                    get_string_from_memory(&caller, action_ptr, action_len),
                ) {
                    tracing::info!("[plugin:{}] Register keybinding: {} -> {}", 
                        state.plugin_name, keys, action);
                    // Em implementação real, registraria o keybinding
                    return 0;
                }
                -2
            },
        )?;

        Ok(())
    }

    fn register_system(linker: &mut Linker<HostState>) -> Result<()> {
        // Obtém timestamp atual em ms
        linker.func_wrap("host", "system_get_time_ms", || -> i64 {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        })?;

        // Obtém informações do plugin
        linker.func_wrap(
            "host",
            "system_get_plugin_info",
            |mut caller: Caller<'_, HostState>, buf_ptr: i32, buf_len: i32| -> i32 {
                let state = caller.data();
                let info = serde_json::json!({
                    "id": state.plugin_id,
                    "name": state.plugin_name,
                    "initialized": state.initialized,
                    "operation_count": state.operation_count,
                });

                let json = info.to_string();
                write_string_to_memory(&mut caller, buf_ptr, buf_len, &json)
            },
        )?;

        // Verifica permissão
        linker.func_wrap(
            "host",
            "system_check_permission",
            |caller: Caller<'_, HostState>, perm: i32| -> i32 {
                let state = caller.data();
                let permission = match perm {
                    0 => Permission::Filesystem,
                    1 => Permission::Network,
                    2 => Permission::Shell,
                    3 => Permission::Clipboard,
                    4 => Permission::ReadConfig,
                    5 => Permission::WriteConfig,
                    6 => Permission::Notifications,
                    7 => Permission::Pty,
                    8 => Permission::CustomUi,
                    _ => return 0,
                };
                if state.has_permission(permission) {
                    1
                } else {
                    0
                }
            },
        )?;

        // Obtém versão do host
        linker.func_wrap(
            "host",
            "system_get_host_version",
            |mut caller: Caller<'_, HostState>, buf_ptr: i32, buf_len: i32| -> i32 {
                let version = env!("CARGO_PKG_VERSION");
                write_string_to_memory(&mut caller, buf_ptr, buf_len, version)
            },
        )?;

        // Pausa execução (yield)
        linker.func_wrap(
            "host",
            "system_yield",
            |_caller: Caller<'_, HostState>| {
                // Permite que o runtime processe outros eventos
                std::thread::yield_now();
            },
        )?;

        Ok(())
    }
}

fn get_memory(caller: &impl AsContext) -> Option<Memory> {
    let memory = caller.as_context().get_export("memory")?.into_memory()?;
    Some(memory)
}

fn get_string_from_memory(caller: &impl AsContext, ptr: i32, len: i32) -> Option<String> {
    if len <= 0 || ptr < 0 {
        return None;
    }

    let memory = get_memory(caller)?;
    let data = memory.data(caller);
    let start = ptr as usize;
    let end = start + len as usize;

    if end > data.len() {
        return None;
    }

    std::str::from_utf8(&data[start..end])
        .ok()
        .map(|s| s.to_string())
}

fn write_string_to_memory(
    caller: &mut Caller<'_, HostState>,
    ptr: i32,
    len: i32,
    value: &str,
) -> i32 {
    if ptr < 0 || len <= 0 {
        return -1;
    }

    let memory = match get_memory(caller) {
        Some(m) => m,
        None => return -1,
    };

    let data = memory.data_mut(caller);
    let start = ptr as usize;
    let end = start + len as usize;

    if end > data.len() {
        return -1;
    }

    let bytes = value.as_bytes();
    let to_write = bytes.len().min(len as usize);
    
    data[start..start + to_write].copy_from_slice(&bytes[..to_write]);
    
    to_write as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_state_new() {
        let state = HostState::new("test-id", "test-plugin");
        assert_eq!(state.plugin_id, "test-id");
        assert_eq!(state.plugin_name, "test-plugin");
        assert!(state.permissions.is_empty());
        assert!(!state.initialized);
    }

    #[test]
    fn test_host_state_permissions() {
        let mut state = HostState::new("test-id", "test-plugin");
        assert!(!state.has_permission(Permission::Filesystem));

        state.add_permission(Permission::Filesystem);
        assert!(state.has_permission(Permission::Filesystem));
    }

    #[test]
    fn test_host_state_logging() {
        let mut state = HostState::new("test-id", "test-plugin");
        state.log("INFO", "test message");
        assert_eq!(state.logs.len(), 1);
        assert!(state.logs[0].contains("test message"));
    }

    #[test]
    fn test_resource_limiter_default() {
        let limiter = ResourceLimiter::default();
        assert_eq!(limiter.max_memory, 16 * 1024 * 1024);
    }

    #[test]
    fn test_resource_limiter_custom() {
        let limiter = ResourceLimiter::new(32 * 1024 * 1024, 2000);
        assert_eq!(limiter.max_memory, 32 * 1024 * 1024);
        assert_eq!(limiter.max_table_elements, 2000);
    }

    #[test]
    fn test_host_state_storage() {
        let mut state = HostState::new("test-id", "test-plugin");
        state.storage.insert("key".to_string(), "value".to_string());
        
        assert_eq!(state.storage.get("key"), Some(&"value".to_string()));
    }
}
