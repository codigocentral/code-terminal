//! Configuração do Engine WASM
//!
//! Este módulo configura o engine wasmtime com otimizações e limites de segurança.

use crate::plugin::error::{PluginError, PluginResult};
use std::sync::Arc;
use wasmtime::{
    Config, Engine, InstanceAllocationStrategy, MemoryLimit, ModuleVersionStrategy,
    WasmBacktraceDetails,
};

/// Configuração padrão de memória (64MB)
pub const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024;

/// Configuração máxima de memória (256MB)
pub const MAX_MEMORY_LIMIT: usize = 256 * 1024 * 1024;

/// Configuração do engine WASM
#[derive(Debug, Clone)]
pub struct WasmEngineConfig {
    /// Limite de memória em bytes
    pub memory_limit: usize,
    /// Habilitar otimizações Cranelift
    pub cranelift_opt: bool,
    /// Habilitar debug info
    pub debug_info: bool,
    /// Habilitar fuel metering
    pub fuel_metering: bool,
    /// Fuel inicial (se habilitado)
    pub initial_fuel: u64,
    /// Habilitar epoch interrupção
    pub epoch_interruption: bool,
    /// Habilitar WASI
    pub wasi_support: bool,
    /// Validar versão do módulo
    pub version_validation: bool,
}

impl WasmEngineConfig {
    /// Cria configuração padrão (segura e otimizada)
    pub fn new() -> Self {
        Self {
            memory_limit: DEFAULT_MEMORY_LIMIT,
            cranelift_opt: true,
            debug_info: cfg!(debug_assertions),
            fuel_metering: true,
            initial_fuel: 1_000_000,
            epoch_interruption: true,
            wasi_support: true,
            version_validation: true,
        }
    }

    /// Define limite de memória
    pub fn with_memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = limit.min(MAX_MEMORY_LIMIT);
        self
    }

    /// Habilita/desabilita debug info
    pub fn with_debug_info(mut self, enabled: bool) -> Self {
        self.debug_info = enabled;
        self
    }

    /// Habilita/desabilita fuel metering
    pub fn with_fuel_metering(mut self, enabled: bool, initial: u64) -> Self {
        self.fuel_metering = enabled;
        self.initial_fuel = initial;
        self
    }

    /// Habilita/desabilita epoch interruption
    pub fn with_epoch_interruption(mut self, enabled: bool) -> Self {
        self.epoch_interruption = enabled;
        self
    }

    /// Habilita/desabilita WASI
    pub fn with_wasi(mut self, enabled: bool) -> Self {
        self.wasi_support = enabled;
        self
    }

    /// Converte para Config do wasmtime
    pub fn to_wasmtime_config(&self) -> PluginResult<Config> {
        let mut config = Config::new();

        config
            .wasm_backtrace_details(WasmBacktraceDetails::Enable)
            .wasm_multi_memory(true)
            .wasm_memory64(false)
            .wasm_component_model(false);

        if self.cranelift_opt {
            config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        }

        if self.debug_info {
            config.debug_info(true);
        }

        if self.fuel_metering {
            config.consume_fuel(true);
        }

        if self.epoch_interruption {
            config.epoch_interruption(true);
        }

        config.memory_limit(MemoryLimit {
            maximum: Some(self.memory_limit as u64),
        });

        config.allocation_strategy(InstanceAllocationStrategy::pooling());

        if self.version_validation {
            config.module_version(ModuleVersionStrategy::Wasmtime);
        }

        Ok(config)
    }
}

impl Default for WasmEngineConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Engine WASM compartilhado
#[derive(Clone)]
pub struct WasmEngine {
    engine: Arc<Engine>,
    config: WasmEngineConfig,
}

impl WasmEngine {
    /// Cria um novo engine com configuração padrão
    pub fn new() -> PluginResult<Self> {
        Self::with_config(WasmEngineConfig::new())
    }

    /// Cria um novo engine com configuração customizada
    pub fn with_config(config: WasmEngineConfig) -> PluginResult<Self> {
        let wasmtime_config = config.to_wasmtime_config()?;
        let engine = Engine::new(&wasmtime_config)
            .map_err(|e| PluginError::init_failed("wasm_engine", e.to_string()))?;

        Ok(Self {
            engine: Arc::new(engine),
            config,
        }
    }

    /// Retorna referência ao engine wasmtime
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Retorna clone do engine (Arc)
    pub fn engine_clone(&self) -> Arc<Engine> {
        Arc::clone(&self.engine)
    }

    /// Retorna a configuração
    pub fn config(&self) -> &WasmEngineConfig {
        &self.config
    }

    /// Verifica se um módulo WASM é válido
    pub fn validate(&self, wasm_bytes: &[u8]) -> PluginResult<()> {
        self.engine
            .validate(wasm_bytes)
            .map_err(|e| PluginError::Compilation {
                reason: e.to_string(),
            })
    }

    /// Compila um módulo WASM (sem instanciar)
    pub fn compile(&self, wasm_bytes: &[u8]) -> PluginResult<wasmtime::Module> {
        wasmtime::Module::new(&self.engine, wasm_bytes).map_err(|e| PluginError::Compilation {
            reason: e.to_string(),
        })
    }

    /// Cria um novo store configurado
    pub fn create_store(&self, data: impl wasmtime::AsContextMut) -> wasmtime::Store<()> {
        let mut store = wasmtime::Store::new(&self.engine, ());

        if self.config.fuel_metering {
            store.set_fuel(Some(self.config.initial_fuel));
        }

        store
    }
}

impl std::fmt::Debug for WasmEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmEngine")
            .field("config", &self.config)
            .finish()
    }
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create default WasmEngine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_config_default() {
        let config = WasmEngineConfig::new();
        assert_eq!(config.memory_limit, DEFAULT_MEMORY_LIMIT);
        assert!(config.cranelift_opt);
        assert!(config.fuel_metering);
        assert!(config.epoch_interruption);
    }

    #[test]
    fn test_engine_config_builder() {
        let config = WasmEngineConfig::new()
            .with_memory_limit(128 * 1024 * 1024)
            .with_debug_info(true)
            .with_fuel_metering(true, 500_000);

        assert_eq!(config.memory_limit, 128 * 1024 * 1024);
        assert!(config.debug_info);
        assert_eq!(config.initial_fuel, 500_000);
    }

    #[test]
    fn test_engine_config_memory_limit_cap() {
        let config = WasmEngineConfig::new()
            .with_memory_limit(512 * 1024 * 1024);

        assert_eq!(config.memory_limit, MAX_MEMORY_LIMIT);
    }

    #[test]
    fn test_engine_creation() {
        let engine = WasmEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_engine_validate_invalid_wasm() {
        let engine = WasmEngine::new().unwrap();
        let invalid_wasm = b"invalid wasm bytes";
        let result = engine.validate(invalid_wasm);
        assert!(result.is_err());
    }

    #[test]
    fn test_engine_to_wasmtime_config() {
        let config = WasmEngineConfig::new();
        let wasmtime_config = config.to_wasmtime_config();
        assert!(wasmtime_config.is_ok());
    }

    #[test]
    fn test_engine_clone() {
        let engine1 = WasmEngine::new().unwrap();
        let engine2 = engine1.clone();
        
        assert!(Arc::ptr_eq(&engine1.engine, &engine2.engine));
    }
}
