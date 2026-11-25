//! Sistema de Plugins
//!
//! Este módulo fornece a arquitetura para extensão do terminal via plugins.
//! Os plugins podem ser carregados dinamicamente e executados em sandbox WASM.
//!
//! # Exemplo
//!
//! ```rust,ignore
//! use code_terminal::plugin::{Plugin, PluginContext, PluginInfo, PluginError};
//!
//! struct MeuPlugin;
//!
//! impl Plugin for MeuPlugin {
//!     fn info(&self) -> PluginInfo {
//!         PluginInfo {
//!             name: "MeuPlugin".to_string(),
//!             version: "1.0.0".to_string(),
//!             author: "Autor".to_string(),
//!             description: "Descrição do plugin".to_string(),
//!             homepage: None,
//!             repository: None,
//!         }
//!     }
//!
//!     fn init(&mut self, _ctx: &mut PluginContext) -> Result<(), PluginError> {
//!         // Inicialização do plugin
//!         Ok(())
//!     }
//!
//!     fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {
//!         // Atualização periódica
//!     }
//!
//!     fn shutdown(&mut self, _ctx: &mut PluginContext) {
//!         // Cleanup
//!     }
//! }
//! ```

pub mod api;
pub mod bridge;
pub mod context;
pub mod error;
pub mod host;
pub mod hot_reload;
pub mod lifecycle;
pub mod loader;
pub mod manager;
pub mod manifest;
pub mod sandbox;
pub mod wasm_error;
pub mod wasm_memory;
pub mod wasm_plugin;
pub mod wasm_runtime;

pub use api::{
    check_compatibility, ApiCapability, CompatibilityResult, PluginApi, PluginApiVersion,
};
pub use context::PluginContext;
pub use error::{PluginError, PluginResult};
pub use host::{
    ClearMode,
    ExecutableHostFunctions,
    // Funções executáveis
    ExecutionContext,
    GetMousePos,
    GetSearchResults,
    HostFunction,
    HostFunctionCategory,
    HostFunctionId,
    HostFunctionPermission,
    HostFunctionRegistry,
    HostFunctionSignature,
    HostFunctions,
    // Host Functions de mouse
    MouseButton,
    NotificationLevel,
    ProcessGetOutput,
    ProcessGetStatus,
    ProcessHandle,
    ProcessKill,
    ProcessList,
    // Host Functions de processos
    ProcessManager,
    ProcessSpawn,
    ProcessStatus,
    ProcessWait,
    SearchClear,
    // Host Functions de busca
    SearchContext,
    SearchFind,
    SearchNext,
    SearchPrev,
    SearchResult,
    SetMousePos,
    SimulateMouseClick,
    SimulateMouseScroll,
    SystemExec,
    TerminalClear,
    TerminalCommand,
    TerminalRead,
    TerminalWrite,
    UiShowNotification,
    HOST_MODULE_NAME,
};
pub use hot_reload::{
    can_hot_reload, HotReloadCallback, HotReloadConfig, HotReloadEvent, HotReloadManager,
    HotReloadStats, HotReloadWatcher, ReloadContext, ReloadHandler, DEFAULT_DEBOUNCE_MS,
    MAX_RELOAD_ATTEMPTS,
};
pub use lifecycle::{PluginLifecycle, PluginState};
pub use loader::{LoadedPlugin, ModuleCache, WasmPluginLoader};
pub use manager::{PluginManager, PluginManagerConfig, UnloadOptions, UnloadResult};
pub use manifest::Permission;
pub use manifest::{PluginDependency, PluginManifest};
pub use sandbox::{
    RateLimiter, ResourceUsage, Sandbox, SandboxConfig, SandboxConfigBuilder, SandboxOperation,
};
pub use wasm_error::{
    is_recoverable_error, log_wasm_error, WasmErrorInfo, WasmErrorType, WasmResultExt,
};
pub use wasm_memory::{
    validate_read_bounds, validate_write_bounds, BufferPool, MemoryStats, SharedBuffer,
    WasmAllocation, WasmMemoryManager,
};
pub use wasm_plugin::{PluginStoreData, WasmPluginBuilder, WasmPluginInstance};
pub use wasm_runtime::{RuntimeStats, WasmRuntime, WasmRuntimeBuilder};

use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// Informações sobre um plugin
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PluginInfo {
    /// Nome do plugin
    pub name: String,
    /// Versão do plugin (semver)
    pub version: String,
    /// Autor do plugin
    pub author: String,
    /// Descrição do plugin
    pub description: String,
    /// Homepage do plugin
    pub homepage: Option<String>,
    /// Repositório do plugin
    pub repository: Option<String>,
}

impl Default for PluginInfo {
    fn default() -> Self {
        Self {
            name: "unnamed".to_string(),
            version: "0.0.0".to_string(),
            author: "".to_string(),
            description: "".to_string(),
            homepage: None,
            repository: None,
        }
    }
}

/// Trait principal para todos os plugins
///
/// Este trait define a interface que todo plugin deve implementar.
/// Os plugins são carregados dinamicamente e gerenciados pelo PluginManager.
pub trait Plugin: Send + Sync + Any + Debug {
    /// Retorna informações sobre o plugin
    fn info(&self) -> PluginInfo;

    /// Inicializa o plugin
    ///
    /// Chamado quando o plugin é carregado. O plugin deve configurar
    /// seus recursos e se registrar para eventos.
    ///
    /// # Arguments
    /// * `ctx` - Contexto do plugin com acesso às APIs do host
    ///
    /// # Returns
    /// * `Ok(())` - Plugin inicializado com sucesso
    /// * `Err(PluginError)` - Falha na inicialização
    fn init(&mut self, ctx: &mut PluginContext) -> PluginResult<()>;

    /// Atualiza o estado do plugin
    ///
    /// Chamado periodicamente para permitir que o plugin execute tarefas.
    ///
    /// # Arguments
    /// * `ctx` - Contexto do plugin
    /// * `delta_ms` - Tempo desde a última atualização em milissegundos
    fn update(&mut self, ctx: &mut PluginContext, delta_ms: u64);

    /// Desliga o plugin
    ///
    /// Chamado quando o plugin está sendo descarregado. O plugin deve
    /// liberar todos os recursos e cancelar registros.
    ///
    /// # Arguments
    /// * `ctx` - Contexto do plugin
    fn shutdown(&mut self, ctx: &mut PluginContext);

    /// Retorna o estado atual do plugin
    fn state(&self) -> PluginState {
        PluginState::Running
    }

    /// Chamado quando o terminal recebe input do usuário
    ///
    /// # Arguments
    /// * `ctx` - Contexto do plugin
    /// * `input` - Input recebido
    ///
    /// # Returns
    /// * `true` - Input foi consumido pelo plugin
    /// * `false` - Input deve ser processado normalmente
    fn on_input(&mut self, _ctx: &mut PluginContext, _input: &str) -> bool {
        false
    }

    /// Chamado quando o conteúdo do terminal muda
    ///
    /// # Arguments
    /// * `ctx` - Contexto do plugin
    /// * `grid_id` - ID do grid que mudou
    fn on_grid_changed(&mut self, _ctx: &mut PluginContext, _grid_id: u64) {}

    /// Chamado quando uma nova aba é criada
    ///
    /// # Arguments
    /// * `ctx` - Contexto do plugin
    /// * `tab_id` - ID da nova aba
    fn on_tab_created(&mut self, _ctx: &mut PluginContext, _tab_id: u64) {}

    /// Chamado quando uma aba é fechada
    ///
    /// # Arguments
    /// * `ctx` - Contexto do plugin
    /// * `tab_id` - ID da aba fechada
    fn on_tab_closed(&mut self, _ctx: &mut PluginContext, _tab_id: u64) {}

    /// Chamado quando o tema muda
    ///
    /// # Arguments
    /// * `ctx` - Contexto do plugin
    /// * `theme_name` - Nome do novo tema
    fn on_theme_changed(&mut self, _ctx: &mut PluginContext, _theme_name: &str) {}

    /// Retorna configuração padrão do plugin
    ///
    /// # Returns
    /// Mapa de configurações padrão
    fn default_config(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Aplica configuração ao plugin
    ///
    /// # Arguments
    /// * `config` - Mapa de configurações
    fn apply_config(&mut self, _config: &HashMap<String, String>) {}
}

/// Tipo dinâmico para plugins
pub type BoxedPlugin = Box<dyn Plugin>;

/// Referência compartilhada para um plugin
pub type SharedPlugin = Arc<dyn Plugin>;

/// Factory function para criar plugins
pub type PluginFactory = fn() -> BoxedPlugin;

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestPlugin {
        state: PluginState,
        initialized: bool,
    }

    impl TestPlugin {
        fn new() -> Self {
            Self {
                state: PluginState::Created,
                initialized: false,
            }
        }
    }

    impl Plugin for TestPlugin {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "TestPlugin".to_string(),
                version: "1.0.0".to_string(),
                author: "Test".to_string(),
                description: "Plugin de teste".to_string(),
                homepage: None,
                repository: None,
            }
        }

        fn init(&mut self, _ctx: &mut PluginContext) -> PluginResult<()> {
            self.initialized = true;
            self.state = PluginState::Running;
            Ok(())
        }

        fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {
            // Nada a fazer
        }

        fn shutdown(&mut self, _ctx: &mut PluginContext) {
            self.state = PluginState::Stopped;
        }

        fn state(&self) -> PluginState {
            self.state
        }
    }

    #[test]
    fn test_plugin_info() {
        let plugin = TestPlugin::new();
        let info = plugin.info();

        assert_eq!(info.name, "TestPlugin");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.author, "Test");
    }

    #[test]
    fn test_plugin_default_config() {
        let plugin = TestPlugin::new();
        let config = plugin.default_config();
        assert!(config.is_empty());
    }

    #[test]
    fn test_plugin_state_lifecycle() {
        let plugin = TestPlugin::new();

        // Estado inicial
        assert_eq!(plugin.state(), PluginState::Created);
        assert!(!plugin.initialized);

        // Inicialização (sem contexto real para teste)
        // Na prática precisaríamos de um mock
        // plugin.init(&mut ctx).unwrap();
        // assert!(plugin.initialized);
        // assert_eq!(plugin.state(), PluginState::Running);

        // Shutdown
        // plugin.shutdown(&mut ctx);
        // assert_eq!(plugin.state(), PluginState::Stopped);
    }

    #[test]
    fn test_plugin_info_default() {
        let info = PluginInfo::default();
        assert_eq!(info.name, "unnamed");
        assert_eq!(info.version, "0.0.0");
    }

    fn test_ctx() -> PluginContext {
        PluginContext::new(
            api::PluginApi::current(),
            1,
            "t",
            std::path::PathBuf::from("/tmp/d"),
            std::path::PathBuf::from("/tmp/c"),
        )
    }

    /// Plugin que só implementa o obrigatório: exercita os padrões do trait
    #[derive(Debug)]
    struct Bare;
    impl Plugin for Bare {
        fn info(&self) -> PluginInfo {
            PluginInfo::default()
        }
        fn init(&mut self, _ctx: &mut PluginContext) -> PluginResult<()> {
            Ok(())
        }
        fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {}
        fn shutdown(&mut self, _ctx: &mut PluginContext) {}
    }

    #[test]
    fn test_trait_defaults_are_no_ops() {
        let mut ctx = test_ctx();
        let mut plugin: BoxedPlugin = Box::new(Bare);
        plugin.init(&mut ctx).unwrap();
        assert_eq!(plugin.state(), PluginState::Running);
        assert!(!plugin.on_input(&mut ctx, "x"));
        plugin.on_grid_changed(&mut ctx, 1);
        plugin.on_tab_created(&mut ctx, 1);
        plugin.on_tab_closed(&mut ctx, 1);
        plugin.on_theme_changed(&mut ctx, "dark");
        plugin.apply_config(&HashMap::new());
        assert!(plugin.default_config().is_empty());
        plugin.update(&mut ctx, 1);
        plugin.shutdown(&mut ctx);
    }

    #[test]
    fn test_test_plugin_init_and_shutdown() {
        let mut ctx = test_ctx();
        let mut plugin = TestPlugin::new();
        plugin.init(&mut ctx).unwrap();
        assert!(plugin.initialized);
        assert_eq!(plugin.state(), PluginState::Running);
        plugin.update(&mut ctx, 16);
        plugin.shutdown(&mut ctx);
        assert_eq!(plugin.state(), PluginState::Stopped);
    }
}
