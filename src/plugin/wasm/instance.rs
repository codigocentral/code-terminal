//! Instância de Plugin WASM
//!
//! Gerencia uma instância específica de plugin WASM carregado.

use super::bindings::{HostState, TerminalInfoResponse, WasmHostFunctions};
use crate::plugin::error::{PluginError, PluginResult};
use crate::plugin::manifest::Permission;
use std::sync::Arc;
use tokio::sync::mpsc;
use wasmtime::*;

/// Exports esperados de um plugin WASM
#[derive(Debug, Clone)]
pub struct WasmPluginExports {
    /// Função de inicialização
    pub init: Option<Extern>,
    /// Função de atualização (chamada periodicamente)
    pub update: Option<Extern>,
    /// Função de desligamento
    pub shutdown: Option<Extern>,
    /// Função chamada quando há input
    pub on_input: Option<Extern>,
    /// Função chamada quando o tema muda
    pub on_theme_changed: Option<Extern>,
    /// Função chamada quando uma tab é criada
    pub on_tab_created: Option<Extern>,
    /// Função chamada quando uma tab é fechada
    pub on_tab_closed: Option<Extern>,
}

impl WasmPluginExports {
    /// Extrai exports de uma instância WASM
    pub fn from_instance(instance: &Instance, store: &impl AsContext) -> Self {
        Self {
            init: instance.get_export(store, "init"),
            update: instance.get_export(store, "update"),
            shutdown: instance.get_export(store, "shutdown"),
            on_input: instance.get_export(store, "on_input"),
            on_theme_changed: instance.get_export(store, "on_theme_changed"),
            on_tab_created: instance.get_export(store, "on_tab_created"),
            on_tab_closed: instance.get_export(store, "on_tab_closed"),
        }
    }

    /// Verifica se o plugin tem a função init
    pub fn has_init(&self) -> bool {
        self.init.is_some()
    }

    /// Verifica se o plugin tem a função update
    pub fn has_update(&self) -> bool {
        self.update.is_some()
    }

    /// Verifica se o plugin tem a função shutdown
    pub fn has_shutdown(&self) -> bool {
        self.shutdown.is_some()
    }
}

/// Instância de plugin WASM em execução
pub struct WasmPluginInstance {
    /// Store do wasmtime
    store: Store<HostState>,
    /// Instância WASM
    instance: Instance,
    /// Exports do plugin
    exports: WasmPluginExports,
    /// Canal para enviar input ao terminal
    terminal_input_tx: mpsc::Sender<String>,
    /// Receptor para input do terminal
    terminal_input_rx: mpsc::Receiver<String>,
    /// ID da instância
    instance_id: String,
    /// Nome do plugin
    plugin_name: String,
}

impl WasmPluginInstance {
    /// Cria uma nova instância de plugin
    pub async fn new(
        engine: &Engine,
        module: &Module,
        plugin_name: impl Into<String>,
        permissions: Vec<Permission>,
    ) -> PluginResult<Self> {
        let plugin_name = plugin_name.into();
        let instance_id = format!("{}-{}", plugin_name, uuid::Uuid::new_v4());

        // Cria canais de comunicação
        let (input_tx, input_rx) = mpsc::channel(100);

        // Cria estado do host
        let mut host_state = HostState::new(&instance_id, &plugin_name);
        host_state.set_terminal_input_tx(input_tx.clone());
        
        // Adiciona permissões
        for perm in permissions {
            host_state.add_permission(perm);
        }

        // Cria store
        let mut store = Store::new(engine, host_state);

        // Configura limites de memória
        store.limiter(|state| &mut state.limiter);

        // Cria linker e registra host functions
        let mut linker = Linker::new(engine);
        WasmHostFunctions::register(&mut linker)
            .map_err(|e| PluginError::wasm_init(format!("Failed to register host functions: {}", e)))?;

        // Instancia o módulo
        let instance = linker
            .instantiate(&mut store, module)
            .map_err(|e| PluginError::wasm_init(format!("Failed to instantiate module: {}", e)))?;

        // Extrai exports
        let exports = WasmPluginExports::from_instance(&instance, &store);

        Ok(Self {
            store,
            instance,
            exports,
            terminal_input_tx: input_tx,
            terminal_input_rx: input_rx,
            instance_id,
            plugin_name,
        })
    }

    /// Inicializa o plugin
    pub async fn init(&mut self) -> PluginResult<()> {
        if let Some(ref init_export) = self.exports.init {
            let init_func: TypedFunc<(), ()> = init_export
                .clone()
                .into_func()
                .ok_or_else(|| PluginError::wasm_init("init is not a function".to_string()))?
                .typed(&self.store)
                .map_err(|e| PluginError::wasm_init(format!("Failed to get init function: {}", e)))?;

            init_func.call(&mut self.store, ()).map_err(|e| {
                PluginError::wasm_init(format!("Init function failed: {}", e))
            })?;

            // Marca como inicializado
            self.store.data_mut().set_initialized();
            tracing::info!("Plugin {} initialized successfully", self.plugin_name);
        }

        Ok(())
    }

    /// Atualiza o plugin (chamado periodicamente)
    pub async fn update(&mut self, delta_ms: u64) -> PluginResult<()> {
        if let Some(ref update_export) = self.exports.update {
            let update_func: TypedFunc<(i64,), ()> = update_export
                .clone()
                .into_func()
                .ok_or_else(|| PluginError::wasm_init("update is not a function".to_string()))?
                .typed(&self.store)
                .map_err(|e| PluginError::wasm_init(format!("Failed to get update function: {}", e)))?;

            update_func.call(&mut self.store, (delta_ms as i64,)).map_err(|e| {
                PluginError::Execution {
                    reason: format!("Update function failed: {}", e),
                }
            })?;
        }

        // Processa mensagens pendentes no canal
        while let Ok(msg) = self.terminal_input_rx.try_recv() {
            tracing::debug!("Plugin {} received terminal input: {}", self.plugin_name, msg);
        }

        Ok(())
    }

    /// Desliga o plugin
    pub async fn shutdown(&mut self) -> PluginResult<()> {
        if let Some(ref shutdown_export) = self.exports.shutdown {
            let shutdown_func: TypedFunc<(), ()> = shutdown_export
                .clone()
                .into_func()
                .ok_or_else(|| PluginError::wasm_init("shutdown is not a function".to_string()))?
                .typed(&self.store)
                .map_err(|e| PluginError::wasm_init(format!("Failed to get shutdown function: {}", e)))?;

            shutdown_func.call(&mut self.store, ()).map_err(|e| {
                PluginError::wasm_init(format!("Shutdown function failed: {}", e))
            })?;

            tracing::info!("Plugin {} shutdown successfully", self.plugin_name);
        }

        Ok(())
    }

    /// Envia input para o plugin (se tiver handler)
    pub async fn on_input(&mut self, input: &str) -> PluginResult<bool> {
        if let Some(ref input_export) = self.exports.on_input {
            // Aloca memória para a string de input
            let input_bytes = input.as_bytes();
            let ptr = self.alloc(input_bytes.len() as i32).await?;

            // Escreve o input na memória
            self.write_memory(ptr, input_bytes).await?;

            // Chama a função on_input
            let input_func: TypedFunc<(i32, i32), i32> = input_export
                .clone()
                .into_func()
                .ok_or_else(|| PluginError::wasm_init("on_input is not a function".to_string()))?
                .typed(&self.store)
                .map_err(|e| PluginError::wasm_init(format!("Failed to get on_input function: {}", e)))?;

            let consumed = input_func.call(&mut self.store, (ptr, input_bytes.len() as i32)).map_err(|e| {
                PluginError::Execution {
                    reason: format!("on_input function failed: {}", e),
                }
            })?;

            // Libera a memória alocada
            self.free(ptr).await?;

            return Ok(consumed != 0);
        }

        Ok(false)
    }

    /// Notifica o plugin sobre mudança de tema
    pub async fn on_theme_changed(&mut self, theme_name: &str) -> PluginResult<()> {
        if let Some(ref theme_export) = self.exports.on_theme_changed {
            let theme_bytes = theme_name.as_bytes();
            let ptr = self.alloc(theme_bytes.len() as i32).await?;
            self.write_memory(ptr, theme_bytes).await?;

            let theme_func: TypedFunc<(i32, i32), ()> = theme_export
                .clone()
                .into_func()
                .ok_or_else(|| PluginError::wasm_init("on_theme_changed is not a function".to_string()))?
                .typed(&self.store)
                .map_err(|e| PluginError::wasm_init(format!("Failed to get on_theme_changed function: {}", e)))?;

            theme_func.call(&mut self.store, (ptr, theme_bytes.len() as i32)).map_err(|e| {
                PluginError::Execution {
                    reason: format!("on_theme_changed function failed: {}", e),
                }
            })?;

            self.free(ptr).await?;
        }

        Ok(())
    }

    /// Notifica o plugin sobre criação de tab
    pub async fn on_tab_created(&mut self, tab_id: u64) -> PluginResult<()> {
        if let Some(ref tab_export) = self.exports.on_tab_created {
            let tab_func: TypedFunc<(i64,), ()> = tab_export
                .clone()
                .into_func()
                .ok_or_else(|| PluginError::wasm_init("on_tab_created is not a function".to_string()))?
                .typed(&self.store)
                .map_err(|e| PluginError::wasm_init(format!("Failed to get on_tab_created function: {}", e)))?;

            tab_func.call(&mut self.store, (tab_id as i64,)).map_err(|e| {
                PluginError::Execution {
                    reason: format!("on_tab_created function failed: {}", e),
                }
            })?;
        }

        Ok(())
    }

    /// Notifica o plugin sobre fechamento de tab
    pub async fn on_tab_closed(&mut self, tab_id: u64) -> PluginResult<()> {
        if let Some(ref tab_export) = self.exports.on_tab_closed {
            let tab_func: TypedFunc<(i64,), ()> = tab_export
                .clone()
                .into_func()
                .ok_or_else(|| PluginError::wasm_init("on_tab_closed is not a function".to_string()))?
                .typed(&self.store)
                .map_err(|e| PluginError::wasm_init(format!("Failed to get on_tab_closed function: {}", e)))?;

            tab_func.call(&mut self.store, (tab_id as i64,)).map_err(|e| {
                PluginError::Execution {
                    reason: format!("on_tab_closed function failed: {}", e),
                }
            })?;
        }

        Ok(())
    }

    /// Aloca memória no WASM
    async fn alloc(&mut self, size: i32) -> PluginResult<i32> {
        // Tenta obter a função malloc do plugin
        if let Some(alloc_export) = self.instance.get_export(&mut self.store, "malloc") {
            if let Some(alloc_func) = alloc_export.into_func() {
                let typed: TypedFunc<(i32,), i32> = alloc_func
                    .typed(&self.store)
                    .map_err(|e| PluginError::wasm_init(format!("Failed to get malloc function: {}", e)))?;

                let ptr = typed.call(&mut self.store, (size,)).map_err(|e| {
                    PluginError::Execution {
                        reason: format!("malloc failed: {}", e),
                    }
                })?;

                return Ok(ptr);
            }
        }

        Err(PluginError::wasm_init("malloc function not found".to_string()))
    }

    /// Libera memória no WASM
    async fn free(&mut self, ptr: i32) -> PluginResult<()> {
        if let Some(free_export) = self.instance.get_export(&mut self.store, "free") {
            if let Some(free_func) = free_export.into_func() {
                let typed: TypedFunc<(i32,), ()> = free_func
                    .typed(&self.store)
                    .map_err(|e| PluginError::wasm_init(format!("Failed to get free function: {}", e)))?;

                typed.call(&mut self.store, (ptr,)).map_err(|e| {
                    PluginError::Execution {
                        reason: format!("free failed: {}", e),
                    }
                })?;
            }
        }

        Ok(())
    }

    /// Escreve dados na memória do WASM
    async fn write_memory(&mut self, ptr: i32, data: &[u8]) -> PluginResult<()> {
        let memory = self.get_memory()?;
        let offset = ptr as usize;
        let end = offset + data.len();

        memory.write(&mut self.store, offset, data).map_err(|e| {
            PluginError::Execution {
                reason: format!("Failed to write memory: {}", e),
            }
        })?;

        Ok(())
    }

    /// Obtém referência à memória do WASM
    fn get_memory(&self) -> PluginResult<Memory> {
        self.instance
            .get_export(&self.store, "memory")
            .and_then(|e| e.into_memory())
            .ok_or_else(|| PluginError::wasm_init("Memory export not found".to_string()))
    }

    /// Retorna o ID da instância
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    /// Retorna o nome do plugin
    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    /// Verifica se está inicializado
    pub fn is_initialized(&self) -> bool {
        self.store.data().initialized
    }

    /// Retorna estatísticas de operações
    pub fn operation_count(&self) -> u64 {
        self.store.data().operation_count
    }

    /// Obtém o canal de input do terminal
    pub fn terminal_input_tx(&self) -> mpsc::Sender<String> {
        self.terminal_input_tx.clone()
    }
}

impl std::fmt::Debug for WasmPluginInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmPluginInstance")
            .field("instance_id", &self.instance_id)
            .field("plugin_name", &self.plugin_name)
            .field("initialized", &self.is_initialized())
            .field("operation_count", &self.operation_count())
            .field("has_init", &self.exports.has_init())
            .field("has_update", &self.exports.has_update())
            .field("has_shutdown", &self.exports.has_shutdown())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Testes de estrutura básica (sem engine real)
    #[test]
    fn test_wasm_plugin_exports_empty() {
        let exports = WasmPluginExports {
            init: None,
            update: None,
            shutdown: None,
            on_input: None,
            on_theme_changed: None,
            on_tab_created: None,
            on_tab_closed: None,
        };

        assert!(!exports.has_init());
        assert!(!exports.has_update());
        assert!(!exports.has_shutdown());
    }

    #[test]
    fn test_host_state_with_terminal_tx() {
        let (tx, _rx) = mpsc::channel(10);
        let mut state = HostState::new("test-id", "test-plugin");
        state.set_terminal_input_tx(tx);
        
        assert!(state.terminal_input_tx.is_some());
    }

    #[test]
    fn test_terminal_info_response() {
        let info = TerminalInfoResponse {
            cols: 80,
            rows: 24,
            active_tab: 0,
            tab_count: 1,
        };
        
        assert_eq!(info.cols, 80);
        assert_eq!(info.rows, 24);
    }
}
