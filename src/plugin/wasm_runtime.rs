//! WASM Runtime
//!
//! Runtime wasmtime para execução de plugins WASM.
//! Gerencia engine, store e instâncias de plugins.

use crate::plugin::error::{PluginError, PluginResult};
use std::path::Path;
use std::sync::Arc;
use wasmtime::{Config, Engine, Module, Store, StoreLimits};

/// Configuração padrão do runtime WASM
pub const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024; // 64 MB
pub const DEFAULT_TABLE_ELEMENTS: u32 = 10000;

/// Runtime WASM para plugins
///
/// Gerencia o ciclo de vida do runtime wasmtime, incluindo:
/// - Engine (compilador JIT)
/// - Configuração de limites
/// - Módulos compilados
pub struct WasmRuntime {
    /// Engine wasmtime (compartilhado entre todas as instâncias)
    engine: Arc<Engine>,
    /// Configuração de limites
    limits: StoreLimits,
    /// Contador de instâncias
    instance_count: std::sync::atomic::AtomicUsize,
}

impl WasmRuntime {
    /// Cria um novo runtime WASM com configuração padrão
    pub fn new() -> PluginResult<Self> {
        let config = Self::create_config()?;
        let engine = Engine::new(&config).map_err(|e| {
            PluginError::init_failed("wasm_runtime", format!("Failed to create engine: {}", e))
        })?;

        Ok(Self {
            engine: Arc::new(engine),
            limits: StoreLimits::default(),
            instance_count: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Cria um novo runtime com limites customizados
    pub fn with_limits(_memory_limit: usize, _table_elements: u32) -> PluginResult<Self> {
        let mut config = Self::create_config()?;

        // Configurar limites de memória
        config.max_wasm_stack(256 * 1024); // 256KB stack

        let engine = Engine::new(&config).map_err(|e| {
            PluginError::init_failed("wasm_runtime", format!("Failed to create engine: {}", e))
        })?;

        Ok(Self {
            engine: Arc::new(engine),
            limits: StoreLimits::default(),
            instance_count: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Cria configuração otimizada do engine
    fn create_config() -> PluginResult<Config> {
        let mut config = Config::new();

        // O Hardened Runtime exige MAP_JIT para código nativo gerado em
        // execução. O interpretador mantém o suporte a WASM nessa edição.
        #[cfg(all(target_os = "macos", feature = "macos-notarized"))]
        config.target("pulley64").map_err(|error| {
            PluginError::init_failed("wasm_runtime", format!("Pulley: {error}"))
        })?;

        // Otimizações
        config.cranelift_opt_level(wasmtime::OptLevel::Speed);
        config.parallel_compilation(true);

        // Segurança (threads ficam de fora: o wasmtime é compilado sem a
        // feature `threads`)
        config.wasm_simd(false); // Desabilitar SIMD por segurança
        config.wasm_relaxed_simd(false); // Desabilitar relaxed SIMD também
        config.wasm_multi_memory(false);
        config.wasm_memory64(false);

        // Features suportadas
        config.wasm_bulk_memory(true);
        config.wasm_reference_types(true);

        // Debug
        #[cfg(debug_assertions)]
        {
            // Wasmtime 30 só implementa símbolos nativos para os backends JIT.
            config.debug_info(!cfg!(all(target_os = "macos", feature = "macos-notarized")));
            config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
        }

        Ok(config)
    }

    /// Compila um arquivo WASM para módulo
    pub fn compile_file(&self, path: impl AsRef<Path>) -> PluginResult<Arc<Module>> {
        let path = path.as_ref();
        let module =
            Module::from_file(&self.engine, path).map_err(|e| PluginError::Compilation {
                reason: format!("Failed to compile '{}': {}", path.display(), e),
            })?;
        Ok(Arc::new(module))
    }

    /// Compila bytes WASM para módulo
    pub fn compile_bytes(&self, bytes: &[u8]) -> PluginResult<Arc<Module>> {
        let module =
            Module::from_binary(&self.engine, bytes).map_err(|e| PluginError::Compilation {
                reason: format!("Failed to compile WASM bytes: {}", e),
            })?;
        Ok(Arc::new(module))
    }

    /// Cria uma nova store para uma instância
    pub fn create_store<T: Default>(&self, data: T) -> Store<T> {
        Store::new(&self.engine, data)
    }

    /// Retorna uma referência ao engine
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    /// Retorna os limites configurados
    pub fn limits(&self) -> &StoreLimits {
        &self.limits
    }

    /// Incrementa contador de instâncias
    pub fn increment_instance_count(&self) -> usize {
        self.instance_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Decrementa contador de instâncias
    pub fn decrement_instance_count(&self) -> usize {
        self.instance_count
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Retorna o número de instâncias ativas
    pub fn instance_count(&self) -> usize {
        self.instance_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Retorna estatísticas do runtime
    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            instance_count: self.instance_count(),
            engine_clone_count: Arc::strong_count(&self.engine),
        }
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create default WASM runtime")
    }
}

impl std::fmt::Debug for WasmRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmRuntime")
            .field("instance_count", &self.instance_count())
            .field("engine_refs", &Arc::strong_count(&self.engine))
            .finish()
    }
}

/// Estatísticas do runtime
#[derive(Debug, Clone, Copy)]
pub struct RuntimeStats {
    /// Número de instâncias ativas
    pub instance_count: usize,
    /// Referências ao engine
    pub engine_clone_count: usize,
}

/// Builder para criar runtime com configurações customizadas
pub struct WasmRuntimeBuilder {
    memory_limit: usize,
    table_elements: u32,
    opt_level: wasmtime::OptLevel,
    debug: bool,
}

impl WasmRuntimeBuilder {
    /// Cria novo builder com defaults
    pub fn new() -> Self {
        Self {
            memory_limit: DEFAULT_MEMORY_LIMIT,
            table_elements: DEFAULT_TABLE_ELEMENTS,
            opt_level: wasmtime::OptLevel::Speed,
            debug: cfg!(debug_assertions),
        }
    }

    /// Define limite de memória
    pub fn memory_limit(mut self, limit: usize) -> Self {
        self.memory_limit = limit;
        self
    }

    /// Define elementos de tabela
    pub fn table_elements(mut self, elements: u32) -> Self {
        self.table_elements = elements;
        self
    }

    /// Define nível de otimização
    pub fn opt_level(mut self, level: wasmtime::OptLevel) -> Self {
        self.opt_level = level;
        self
    }

    /// Habilita/desabilita debug
    pub fn debug(mut self, enable: bool) -> Self {
        self.debug = enable;
        self
    }

    /// Constrói o runtime
    pub fn build(self) -> PluginResult<WasmRuntime> {
        WasmRuntime::with_limits(self.memory_limit, self.table_elements)
    }
}

impl Default for WasmRuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_creation() {
        let runtime = WasmRuntime::new();
        if let Err(ref e) = runtime {
            eprintln!("Runtime creation failed: {:?}", e);
        }
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_runtime_with_limits() {
        let runtime = WasmRuntime::with_limits(32 * 1024 * 1024, 5000);
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_runtime_builder() {
        let runtime = WasmRuntimeBuilder::new()
            .memory_limit(128 * 1024 * 1024)
            .table_elements(20000)
            .build();
        assert!(runtime.is_ok());
    }

    #[test]
    fn test_instance_count() {
        let runtime = WasmRuntime::new().unwrap();

        assert_eq!(runtime.instance_count(), 0);

        runtime.increment_instance_count();
        assert_eq!(runtime.instance_count(), 1);

        runtime.increment_instance_count();
        assert_eq!(runtime.instance_count(), 2);

        runtime.decrement_instance_count();
        assert_eq!(runtime.instance_count(), 1);
    }

    #[test]
    fn test_runtime_stats() {
        let runtime = WasmRuntime::new().unwrap();
        let stats = runtime.stats();

        assert_eq!(stats.instance_count, 0);
        assert!(stats.engine_clone_count >= 1);
    }

    #[test]
    fn test_compile_invalid_bytes() {
        let runtime = WasmRuntime::new().unwrap();
        let invalid_wasm = b"not wasm";

        let result = runtime.compile_bytes(invalid_wasm);
        assert!(result.is_err());
    }

    #[test]
    fn test_compile_valid_wasm() {
        let runtime = WasmRuntime::new().unwrap();

        // WASM mínimo válido (módulo vazio)
        let minimal_wasm = wat::parse_str("(module)").unwrap();

        let result = runtime.compile_bytes(&minimal_wasm);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_file_and_invalid_inputs() {
        let runtime = WasmRuntime::default();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.wasm");
        std::fs::write(
            &path,
            wat::parse_str("(module (func (export \"f\")))").unwrap(),
        )
        .unwrap();
        let module = runtime.compile_file(&path).unwrap();
        assert!(module.exports().any(|e| e.name() == "f"));

        let bad = dir.path().join("bad.wasm");
        std::fs::write(&bad, b"lixo").unwrap();
        let err = runtime.compile_file(&bad).unwrap_err();
        assert!(err.to_string().contains("bad.wasm"), "{err}");
        assert!(runtime.compile_file(dir.path().join("nada.wasm")).is_err());

        let err = runtime.compile_bytes(b"\0asm\x02").unwrap_err();
        assert!(
            err.to_string().contains("Failed to compile WASM bytes"),
            "{err}"
        );
    }

    #[test]
    fn test_instance_counter_stats_and_debug() {
        let runtime = WasmRuntime::new().unwrap();
        assert_eq!(runtime.increment_instance_count(), 0);
        assert_eq!(runtime.increment_instance_count(), 1);
        assert_eq!(runtime.decrement_instance_count(), 2);
        assert_eq!(runtime.instance_count(), 1);

        let engine = runtime.engine().clone();
        let stats = runtime.stats();
        assert_eq!(stats.instance_count, 1);
        assert_eq!(stats.engine_clone_count, 2);
        drop(engine);

        let debug = format!("{:?}", runtime);
        assert!(debug.contains("instance_count: 1"), "{debug}");
        let _ = runtime.limits();
        let store = runtime.create_store(42u32);
        assert_eq!(*store.data(), 42);
    }

    #[test]
    fn test_builder_and_with_limits_create_working_engines() {
        let runtime = WasmRuntimeBuilder::default()
            .memory_limit(1024 * 1024)
            .table_elements(100)
            .opt_level(wasmtime::OptLevel::None)
            .debug(false)
            .build()
            .unwrap();
        assert!(runtime
            .compile_bytes(&wat::parse_str("(module)").unwrap())
            .is_ok());
        assert!(WasmRuntime::with_limits(1024, 10).is_ok());
    }
}
