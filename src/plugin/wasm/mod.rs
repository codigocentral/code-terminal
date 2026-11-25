//! WASM Runtime para Plugins
//!
//! Este módulo implementa o runtime WASM usando wasmtime para carregar
//! e executar plugins de forma segura e isolada.
//!
//! # Arquitetura
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              Plugin Manager                  │
//! └────────────────────┬────────────────────────┘
//!                      │
//!                      ▼
//! ┌─────────────────────────────────────────────┐
//! │              WasmRuntime                     │
//! │  ┌─────────────┐  ┌─────────────────────┐   │
//! │  │   Engine    │  │   Module Cache      │   │
//! │  └─────────────┘  └─────────────────────┘   │
//! └────────────────────┬────────────────────────┘
//!                      │
//!                      ▼
//! ┌─────────────────────────────────────────────┐
//! │            WasmPluginInstance               │
//! │  ┌─────────────┐  ┌─────────────────────┐   │
//! │  │   Store     │  │   Instance          │   │
//! │  │   Memory    │  │   Exports           │   │
//! │  └─────────────┘  └─────────────────────┘   │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! # Exemplo de uso
//!
//! ```rust,ignore
//! use code_terminal::plugin::wasm::{WasmRuntime, WasmPluginLoader, WasmRuntimeConfig};
//!
//! // Criar runtime
//! let runtime = WasmRuntime::new(WasmRuntimeConfig::default())?;
//! let loader = WasmPluginLoader::new(runtime.engine().clone());
//!
//! // Carregar plugin
//! let plugin = loader.load_from_file("meu_plugin.wasm", None).await?;
//!
//! // Usar plugin
//! plugin.init().await?;
//! plugin.update(16).await?;
//! plugin.shutdown().await?;
//! ```

pub mod bindings;
pub mod instance;
pub mod loader;

pub use bindings::{HostState, TerminalInfoResponse, WasmHostFunctions, ResourceLimiter};
pub use instance::{WasmPluginInstance, WasmPluginExports};
pub use loader::{WasmPluginLoader, WasmLoaderConfig, LoadedPlugin, ModuleCache, CacheStats};

use crate::plugin::error::{PluginError, PluginResult};
use std::path::Path;
use std::sync::Arc;
use wasmtime::*;

/// Configuração do runtime WASM
#[derive(Debug, Clone)]
pub struct WasmRuntimeConfig {
    /// Limite de memória (em páginas de 64KB)
    pub max_memory_pages: u32,
    /// Habilitar fuel (consumo de código)
    pub enable_fuel: bool,
    /// Fuel inicial
    pub initial_fuel: u64,
    /// Habilitar módulos WASI
    pub enable_wasi: bool,
    /// Habilitar threads
    pub enable_threads: bool,
    /// Timeout de execução (ms)
    pub execution_timeout_ms: u64,
}

impl WasmRuntimeConfig {
    /// Configuração padrão (segura)
    pub fn safe() -> Self {
        Self {
            max_memory_pages: 256, // 16MB
            enable_fuel: true,
            initial_fuel: 1_000_000,
            enable_wasi: false,
            enable_threads: false,
            execution_timeout_ms: 1000,
        }
    }

    /// Configuração para desenvolvimento
    pub fn development() -> Self {
        Self {
            max_memory_pages: 1024, // 64MB
            enable_fuel: true,
            initial_fuel: 10_000_000,
            enable_wasi: true,
            enable_threads: false,
            execution_timeout_ms: 5000,
        }
    }

    /// Configuração permissiva
    pub fn permissive() -> Self {
        Self {
            max_memory_pages: 4096, // 256MB
            enable_fuel: false,
            initial_fuel: 0,
            enable_wasi: true,
            enable_threads: true,
            execution_timeout_ms: 30000,
        }
    }
}

impl Default for WasmRuntimeConfig {
    fn default() -> Self {
        Self::safe()
    }
}

/// Runtime WASM para execução de plugins
pub struct WasmRuntime {
    engine: Engine,
    config: WasmRuntimeConfig,
}

impl WasmRuntime {
    /// Cria um novo runtime WASM
    pub fn new(config: WasmRuntimeConfig) -> PluginResult<Self> {
        let mut wasm_config = Config::new();

        wasm_config
            .max_wasm_stack(1024 * 1024)
            .cranelift_opt_level(OptLevel::Speed)
            .memory_init_cow(true);

        if config.enable_fuel {
            wasm_config.consume_fuel(true);
        }

        if config.enable_wasi {
            wasm_config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        }

        wasm_config.wasm_multi_memory(false);
        wasm_config.wasm_memory64(false);

        if config.enable_threads {
            wasm_config.wasm_threads(true);
        }

        let engine = Engine::new(&wasm_config)
            .map_err(|e| PluginError::wasm_init(format!("Failed to create engine: {}", e)))?;

        Ok(Self { engine, config })
    }

    /// Cria runtime com configuração padrão
    pub fn default_runtime() -> PluginResult<Self> {
        Self::new(WasmRuntimeConfig::default())
    }

    /// Retorna referência à engine
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Retorna engine como Arc para compartilhamento
    pub fn engine_arc(&self) -> Arc<Engine> {
        Arc::new(self.engine.clone())
    }

    /// Retorna a configuração
    pub fn config(&self) -> &WasmRuntimeConfig {
        &self.config
    }

    /// Compila um módulo WASM
    pub fn compile(&self, bytes: &[u8]) -> PluginResult<Module> {
        Module::new(&self.engine, bytes)
            .map_err(|e| PluginError::wasm_compile(format!("Failed to compile module: {}", e)))
    }

    /// Compila um módulo WASM de um arquivo
    pub fn compile_from_file(&self, path: impl AsRef<Path>) -> PluginResult<Module> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|e| PluginError::io("read wasm", path.display().to_string()))?;
        self.compile(&bytes)
    }

    /// Valida um módulo WASM sem compilar
    pub fn validate(&self, bytes: &[u8]) -> PluginResult<()> {
        wasmtime::Module::validate(&self.engine, bytes)
            .map_err(|e| PluginError::wasm_validate(format!("Invalid WASM: {}", e)))?;
        Ok(())
    }

    /// Cria um novo store com configurações do runtime
    pub fn create_store(&self, host_state: HostState) -> Store<HostState> {
        let mut store = Store::new(&self.engine, host_state);

        if self.config.enable_fuel {
            store.set_fuel(self.config.initial_fuel).ok();
        }

        store.limiter(|state| &mut state.limiter);

        store
    }

    /// Cria um loader configurado
    pub fn create_loader(&self) -> WasmPluginLoader {
        WasmPluginLoader::new(Arc::new(self.engine.clone()))
    }
}

/// Cache de módulos WASM compilados
pub struct WasmModuleCache {
    modules: std::collections::HashMap<String, Module>,
}

impl WasmModuleCache {
    /// Cria um novo cache
    pub fn new() -> Self {
        Self {
            modules: std::collections::HashMap::new(),
        }
    }

    /// Adiciona um módulo ao cache
    pub fn insert(&mut self, name: impl Into<String>, module: Module) {
        self.modules.insert(name.into(), module);
    }

    /// Obtém um módulo do cache
    pub fn get(&self, name: &str) -> Option<&Module> {
        self.modules.get(name)
    }

    /// Remove um módulo do cache
    pub fn remove(&mut self, name: &str) -> Option<Module> {
        self.modules.remove(name)
    }

    /// Limpa o cache
    pub fn clear(&mut self) {
        self.modules.clear();
    }

    /// Retorna o número de módulos no cache
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Retorna true se o cache está vazio
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

impl Default for WasmModuleCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_config_safe() {
        let config = WasmRuntimeConfig::safe();
        assert_eq!(config.max_memory_pages, 256);
        assert!(config.enable_fuel);
        assert!(!config.enable_wasi);
    }

    #[test]
    fn test_runtime_config_development() {
        let config = WasmRuntimeConfig::development();
        assert_eq!(config.max_memory_pages, 1024);
        assert!(config.enable_wasi);
    }

    #[test]
    fn test_runtime_config_permissive() {
        let config = WasmRuntimeConfig::permissive();
        assert_eq!(config.max_memory_pages, 4096);
        assert!(!config.enable_fuel);
    }

    #[test]
    fn test_runtime_creation() {
        let config = WasmRuntimeConfig::safe();
        let runtime = WasmRuntime::new(config);
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_runtime_default() {
        let runtime = WasmRuntime::default_runtime();
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_module_cache() {
        let mut cache = WasmModuleCache::new();
        assert!(cache.is_empty());

        // Não podemos criar um Module sem engine, então apenas testamos a estrutura
        assert_eq!(cache.len(), 0);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_validate_empty_wasm() {
        let runtime = WasmRuntime::default_runtime().unwrap();
        let result = runtime.validate(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_host_state_terminal_response() {
        let response = TerminalInfoResponse {
            cols: 80,
            rows: 24,
            active_tab: 0,
            tab_count: 1,
        };
        
        assert_eq!(response.cols, 80);
        assert_eq!(response.rows, 24);
        assert_eq!(response.tab_count, 1);
    }
}
