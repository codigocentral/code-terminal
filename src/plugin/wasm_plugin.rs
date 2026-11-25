//! WASM Plugin Instance
//!
//! Implementa o trait Plugin para plugins WASM carregados.
//! Faz a ponte entre o runtime wasmtime e o sistema de plugins.

use crate::plugin::context::PluginContext;
use crate::plugin::error::{PluginError, PluginResult};
use crate::plugin::host::HostFunctionRegistry;
use crate::plugin::lifecycle::PluginState;
use crate::plugin::manifest::PluginManifest;
use crate::plugin::wasm_runtime::WasmRuntime;
use crate::plugin::{Plugin, PluginInfo};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::warn;
use wasmtime::{Instance, Linker, Module, Store, TypedFunc};

/// Instância de um plugin WASM
///
/// Representa um plugin carregado e pronto para execução.
/// Implementa o trait Plugin para integração com o sistema.
pub struct WasmPluginInstance {
    /// Nome do plugin
    name: String,
    /// Informações do plugin
    info: PluginInfo,
    /// Manifesto do plugin
    manifest: PluginManifest,
    /// Módulo WASM compilado
    module: Arc<Module>,
    /// Instância do WASM
    instance: Option<Instance>,
    /// Store do WASM
    store: Option<Store<PluginStoreData>>,
    /// Estado atual
    state: PluginState,
    /// Caminho do arquivo (se carregado de arquivo)
    path: Option<PathBuf>,
}

impl std::fmt::Debug for WasmPluginInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmPluginInstance")
            .field("name", &self.name)
            .field("info", &self.info)
            .field("state", &self.state)
            .field("path", &self.path)
            .field("instantiated", &self.instance.is_some())
            .finish_non_exhaustive()
    }
}

/// Dados armazenados no store do plugin
#[derive(Debug, Default)]
pub struct PluginStoreData {
    /// Dados customizados do plugin
    custom_data: HashMap<String, Vec<u8>>,
    /// Nome do plugin (identifica os pedidos na ponte com o app)
    plugin_name: String,
    /// Permissões declaradas no manifesto
    permissions: Vec<crate::plugin::manifest::Permission>,
}

impl PluginStoreData {
    /// Cria novos dados de store
    pub fn new() -> Self {
        Self::default()
    }

    /// Dados de store de um plugin com nome e permissões do manifesto.
    pub fn for_plugin(
        name: impl Into<String>,
        permissions: Vec<crate::plugin::manifest::Permission>,
    ) -> Self {
        Self {
            plugin_name: name.into(),
            permissions,
            ..Self::default()
        }
    }

    /// Nome do plugin dono do store.
    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    /// O manifesto declarou a permissão?
    pub fn has_permission(&self, permission: crate::plugin::manifest::Permission) -> bool {
        self.permissions.contains(&permission)
    }

    /// Armazena dados customizados
    pub fn store_data(&mut self, key: impl Into<String>, data: Vec<u8>) {
        self.custom_data.insert(key.into(), data);
    }

    /// Recupera dados customizados
    pub fn get_data(&self, key: &str) -> Option<&Vec<u8>> {
        self.custom_data.get(key)
    }
}

impl WasmPluginInstance {
    /// Cria uma nova instância de plugin WASM a partir de um módulo
    pub fn new(
        name: impl Into<String>,
        module: Arc<Module>,
        manifest: PluginManifest,
        path: Option<PathBuf>,
    ) -> Self {
        let name = name.into();
        let info = PluginInfo {
            name: name.clone(),
            version: manifest.plugin.version.clone(),
            author: manifest.plugin.author.clone(),
            description: manifest.plugin.description.clone(),
            homepage: manifest.plugin.homepage.clone(),
            repository: manifest.plugin.repository.clone(),
        };

        Self {
            name,
            info,
            manifest,
            module,
            instance: None,
            store: None,
            state: PluginState::Created,
            path,
        }
    }

    /// Instancia o plugin
    ///
    /// Cria a store e instancia o módulo WASM com host functions.
    pub fn instantiate(&mut self, runtime: &WasmRuntime) -> PluginResult<()> {
        self.instantiate_with_linker(runtime)
    }

    /// Obtém uma função exportada tipada
    pub fn get_typed_func<Params, Results>(
        &mut self,
        name: &str,
    ) -> Result<TypedFunc<Params, Results>, PluginError>
    where
        Params: wasmtime::WasmParams,
        Results: wasmtime::WasmResults,
    {
        let instance = self
            .instance
            .as_ref()
            .ok_or_else(|| PluginError::init_failed(&self.name, "Plugin not instantiated"))?;

        let store = self
            .store
            .as_mut()
            .ok_or_else(|| PluginError::init_failed(&self.name, "Store not available"))?;

        let func = instance
            .get_typed_func::<Params, Results>(store, name)
            .map_err(|_| PluginError::not_found(format!("function '{}'", name)))?;

        Ok(func)
    }

    /// Chama uma função exportada
    pub fn call<Params, Results>(&mut self, name: &str, params: Params) -> PluginResult<Results>
    where
        Params: wasmtime::WasmParams,
        Results: wasmtime::WasmResults,
    {
        let func = self.get_typed_func::<Params, Results>(name)?;
        let store = self.store.as_mut().unwrap();

        let result = func
            .call(store, params)
            .map_err(|e| PluginError::Execution {
                reason: format!("Function '{}' failed: {}", name, e),
            })?;

        Ok(result)
    }

    fn call_func<Params, Results>(
        &mut self,
        name: &str,
        func: TypedFunc<Params, Results>,
        params: Params,
    ) -> PluginResult<Results>
    where
        Params: wasmtime::WasmParams,
        Results: wasmtime::WasmResults,
    {
        let store = self
            .store
            .as_mut()
            .ok_or_else(|| PluginError::init_failed(&self.name, "Store not available"))?;

        func.call(store, params)
            .map_err(|e| PluginError::Execution {
                reason: format!("Function '{}' failed: {}", name, e),
            })
    }

    /// Verifica se o plugin exporta uma função específica
    pub fn has_function(&mut self, name: &str) -> bool {
        let Some(ref instance) = self.instance else {
            return false;
        };
        let Some(ref mut store) = self.store else {
            return false;
        };

        instance.get_func(store, name).is_some()
    }

    /// Retorna a memória exportada (se houver)
    pub fn get_memory(&mut self) -> Option<wasmtime::Memory> {
        let instance = self.instance.as_ref()?;
        let store = self.store.as_mut()?;
        instance.get_memory(store, "memory")
    }

    /// Lê dados da memória do plugin
    pub fn read_memory(&mut self, offset: usize, len: usize) -> PluginResult<Vec<u8>> {
        let memory = self.get_memory().ok_or_else(|| PluginError::Execution {
            reason: "Plugin has no memory export".to_string(),
        })?;

        let store = self.store.as_ref().unwrap();
        let data = memory.data(store);

        // checked_add: offset/len podem vir do guest e estourar a soma
        if offset.checked_add(len).is_none_or(|end| end > data.len()) {
            return Err(PluginError::Execution {
                reason: format!(
                    "Memory out of bounds: offset={}, len={}, size={}",
                    offset,
                    len,
                    data.len()
                ),
            });
        }

        Ok(data[offset..offset + len].to_vec())
    }

    /// Escreve dados na memória do plugin
    pub fn write_memory(&mut self, offset: usize, data: &[u8]) -> PluginResult<()> {
        let memory = self.get_memory().ok_or_else(|| PluginError::Execution {
            reason: "Plugin has no memory export".to_string(),
        })?;

        let store = self.store.as_mut().unwrap();
        let mem_data = memory.data_mut(store);

        if offset
            .checked_add(data.len())
            .is_none_or(|end| end > mem_data.len())
        {
            return Err(PluginError::Execution {
                reason: format!(
                    "Memory out of bounds: offset={}, len={}, size={}",
                    offset,
                    data.len(),
                    mem_data.len()
                ),
            });
        }

        mem_data[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn guest_malloc(&mut self, len: usize) -> PluginResult<i32> {
        let len = i32::try_from(len).map_err(|_| PluginError::Execution {
            reason: format!("Guest allocation too large: {} bytes", len),
        })?;
        let malloc = self.get_typed_func::<i32, i32>("malloc")?;
        let ptr = self.call_func("malloc", malloc, len)?;
        if ptr < 0 {
            return Err(PluginError::Execution {
                reason: format!("malloc returned invalid pointer: {}", ptr),
            });
        }
        Ok(ptr)
    }

    fn guest_free(&mut self, ptr: i32) -> PluginResult<()> {
        match self.get_typed_func::<i32, ()>("free") {
            Ok(free) => self.call_func("free", free, ptr),
            Err(PluginError::NotFound { .. }) => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn with_guest_bytes<R>(
        &mut self,
        data: &[u8],
        f: impl FnOnce(&mut Self, i32, i32) -> PluginResult<R>,
    ) -> PluginResult<R> {
        let ptr = self.guest_malloc(data.len())?;
        let len = i32::try_from(data.len()).map_err(|_| PluginError::Execution {
            reason: format!("Guest buffer too large: {} bytes", data.len()),
        })?;

        let result = self
            .write_memory(ptr as usize, data)
            .and_then(|_| f(self, ptr, len));

        if let Err(error) = self.guest_free(ptr) {
            warn!(plugin = %self.name, error = %error, "Failed to free guest WASM buffer");
        }

        result
    }

    fn call_with_guest_bytes(&mut self, name: &str, data: &[u8]) -> PluginResult<()> {
        let func = self.get_typed_func::<(i32, i32), ()>(name)?;
        self.with_guest_bytes(data, |plugin, ptr, len| {
            plugin.call_func(name, func, (ptr, len))
        })
    }

    fn call_with_guest_bytes_ret_i32(&mut self, name: &str, data: &[u8]) -> PluginResult<i32> {
        let func = self.get_typed_func::<(i32, i32), i32>(name)?;
        self.with_guest_bytes(data, |plugin, ptr, len| {
            plugin.call_func(name, func, (ptr, len))
        })
    }

    fn context_payload(&self, ctx: &PluginContext) -> PluginResult<Vec<u8>> {
        serde_json::to_vec(&serde_json::json!({
            "plugin_id": ctx.plugin_id(),
            "plugin_name": ctx.plugin_name(),
            "api_version": ctx.api_version(),
            "data_dir": ctx.data_dir().to_string_lossy(),
            "config_dir": ctx.config_dir().to_string_lossy(),
            "config": ctx.config(),
        }))
        .map_err(|e| PluginError::Execution {
            reason: format!("Failed to serialize plugin context: {}", e),
        })
    }

    fn config_payload(config: &HashMap<String, String>) -> PluginResult<Vec<u8>> {
        serde_json::to_vec(config).map_err(|e| PluginError::Execution {
            reason: format!("Failed to serialize plugin config: {}", e),
        })
    }

    /// Instancia o plugin usando o linker com host functions
    fn instantiate_with_linker(&mut self, runtime: &WasmRuntime) -> PluginResult<()> {
        if self.instance.is_some() {
            return Err(PluginError::already_loaded(&self.name));
        }

        // Criar store com dados do plugin
        let data = PluginStoreData::for_plugin(
            &self.manifest.plugin.name,
            self.manifest.permissions.clone(),
        );
        let mut store = runtime.create_store(data);

        // Criar linker e registrar host functions
        let registry = HostFunctionRegistry::new();
        let mut linker = Linker::new(store.engine());
        registry.register_all(&mut linker).map_err(|e| {
            PluginError::init_failed(
                &self.name,
                format!("Failed to register host functions: {}", e),
            )
        })?;

        // Instanciar módulo com o linker
        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            PluginError::init_failed(&self.name, format!("Failed to instantiate: {}", e))
        })?;

        self.store = Some(store);
        self.instance = Some(instance);
        self.state = PluginState::Initializing;

        // Chamar função _start se existir
        if let Ok(start) = self.get_typed_func::<(), ()>("_start") {
            let store = self.store.as_mut().unwrap();
            start.call(store, ()).map_err(|e| {
                PluginError::init_failed(&self.name, format!("_start failed: {}", e))
            })?;
        }

        Ok(())
    }

    /// Retorna o manifesto do plugin
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    /// Retorna o caminho do arquivo (se disponível)
    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    /// Libera recursos da instância
    pub fn shutdown(&mut self) {
        // Chamar shutdown do plugin se existir
        if self.has_function("shutdown") {
            let _ = self.call::<(), ()>("shutdown", ());
        }

        self.instance = None;
        self.store = None;
        self.state = PluginState::Stopped;
    }
}

impl Drop for WasmPluginInstance {
    fn drop(&mut self) {
        if self.instance.is_some() {
            self.shutdown();
        }
    }
}

impl Plugin for WasmPluginInstance {
    fn info(&self) -> PluginInfo {
        self.info.clone()
    }

    fn init(&mut self, ctx: &mut PluginContext) -> PluginResult<()> {
        if self.state != PluginState::Initializing && self.state != PluginState::Created {
            return Err(PluginError::init_failed(
                &self.name,
                "Invalid state for init",
            ));
        }

        // Chamar init do WASM se existir
        if self.has_function("init") {
            match self.get_typed_func::<(i32, i32), ()>("init") {
                Ok(init) => {
                    let payload = self.context_payload(ctx)?;
                    self.with_guest_bytes(&payload, |plugin, ptr, len| {
                        plugin.call_func("init", init, (ptr, len))
                    })?;
                }
                Err(_) => {
                    self.call::<(), ()>("init", ())?;
                }
            }
        }

        self.state = PluginState::Running;
        Ok(())
    }

    fn update(&mut self, _ctx: &mut PluginContext, delta_ms: u64) {
        if self.state != PluginState::Running {
            return;
        }

        // Chamar update do WASM se existir
        if self.has_function("update") {
            let _ = self.call::<u64, ()>("update", delta_ms);
        }
    }

    fn shutdown(&mut self, _ctx: &mut PluginContext) {
        WasmPluginInstance::shutdown(self);
    }

    fn state(&self) -> PluginState {
        self.state
    }

    fn on_input(&mut self, _ctx: &mut PluginContext, input: &str) -> bool {
        if self.state != PluginState::Running {
            return false;
        }

        if self.has_function("on_input") {
            match self.call_with_guest_bytes_ret_i32("on_input", input.as_bytes()) {
                Ok(consumed) => consumed != 0,
                Err(error) => {
                    warn!(plugin = %self.name, error = %error, "WASM on_input failed");
                    false
                }
            }
        } else {
            false
        }
    }

    fn on_grid_changed(&mut self, _ctx: &mut PluginContext, grid_id: u64) {
        if self.state != PluginState::Running {
            return;
        }

        if self.has_function("on_grid_changed") {
            let _ = self.call::<u64, ()>("on_grid_changed", grid_id);
        }
    }

    fn on_tab_created(&mut self, _ctx: &mut PluginContext, tab_id: u64) {
        if self.state != PluginState::Running {
            return;
        }

        if self.has_function("on_tab_created") {
            let _ = self.call::<u64, ()>("on_tab_created", tab_id);
        }
    }

    fn on_tab_closed(&mut self, _ctx: &mut PluginContext, tab_id: u64) {
        if self.state != PluginState::Running {
            return;
        }

        if self.has_function("on_tab_closed") {
            let _ = self.call::<u64, ()>("on_tab_closed", tab_id);
        }
    }

    fn on_theme_changed(&mut self, _ctx: &mut PluginContext, theme_name: &str) {
        if self.state != PluginState::Running {
            return;
        }

        if self.has_function("on_theme_changed") {
            if let Err(error) =
                self.call_with_guest_bytes("on_theme_changed", theme_name.as_bytes())
            {
                warn!(plugin = %self.name, error = %error, "WASM on_theme_changed failed");
            }
        }
    }

    fn default_config(&self) -> HashMap<String, String> {
        self.manifest.config.clone()
    }

    fn apply_config(&mut self, config: &HashMap<String, String>) {
        if self.has_function("apply_config") {
            let payload = match Self::config_payload(config) {
                Ok(payload) => payload,
                Err(error) => {
                    warn!(plugin = %self.name, error = %error, "Failed to serialize WASM config");
                    return;
                }
            };

            if let Err(error) = self.call_with_guest_bytes("apply_config", &payload) {
                warn!(plugin = %self.name, error = %error, "WASM apply_config failed");
            }
        }
    }
}

/// Builder para criar instâncias de plugins WASM
pub struct WasmPluginBuilder {
    name: String,
    module: Option<Arc<Module>>,
    manifest: Option<PluginManifest>,
    path: Option<PathBuf>,
}

impl WasmPluginBuilder {
    /// Cria novo builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            module: None,
            manifest: None,
            path: None,
        }
    }

    /// Define o módulo
    pub fn module(mut self, module: Arc<Module>) -> Self {
        self.module = Some(module);
        self
    }

    /// Define o manifesto
    pub fn manifest(mut self, manifest: PluginManifest) -> Self {
        self.manifest = Some(manifest);
        self
    }

    /// Define o caminho
    pub fn path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Constrói a instância
    pub fn build(self) -> PluginResult<WasmPluginInstance> {
        let module = self
            .module
            .ok_or_else(|| PluginError::invalid_manifest("Module is required"))?;

        let manifest = self
            .manifest
            .unwrap_or_else(|| PluginManifest::new(&self.name, "0.0.0", "plugin.wasm"));

        Ok(WasmPluginInstance::new(
            self.name, module, manifest, self.path,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::api::PluginApi;
    use crate::plugin::wasm_runtime::WasmRuntime;
    use crate::plugin::Plugin;

    fn create_test_runtime() -> Arc<WasmRuntime> {
        Arc::new(WasmRuntime::new().unwrap())
    }

    fn create_minimal_wasm() -> Vec<u8> {
        wat::parse_str("(module)").unwrap()
    }

    fn create_context_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (global $heap (mut i32) (i32.const 1024))
              (global $init_len (mut i32) (i32.const 0))
              (global $theme_len (mut i32) (i32.const 0))
              (global $config_len (mut i32) (i32.const 0))

              (func (export "malloc") (param $len i32) (result i32)
                (local $ptr i32)
                global.get $heap
                local.set $ptr
                global.get $heap
                local.get $len
                i32.add
                global.set $heap
                local.get $ptr)

              (func (export "free") (param $ptr i32))

              (func (export "init") (param $ptr i32) (param $len i32)
                local.get $len
                global.set $init_len)

              (func (export "on_input") (param $ptr i32) (param $len i32) (result i32)
                local.get $len
                i32.const 4
                i32.eq
                local.get $ptr
                i32.load8_u
                i32.const 101
                i32.eq
                i32.and)

              (func (export "on_theme_changed") (param $ptr i32) (param $len i32)
                local.get $len
                global.set $theme_len)

              (func (export "apply_config") (param $ptr i32) (param $len i32)
                local.get $len
                global.set $config_len)

              (func (export "get_init_len") (result i32)
                global.get $init_len)

              (func (export "get_theme_len") (result i32)
                global.get $theme_len)

              (func (export "get_config_len") (result i32)
                global.get $config_len))
            "#,
        )
        .unwrap()
    }

    fn create_system_import_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"
            (module
              (import "host" "get_platform" (func $get_platform (result i32)))
              (import "host" "get_host_version" (func $get_host_version (param i32 i32) (result i32)))
              (memory (export "memory") 1)

              (func (export "platform") (result i32)
                call $get_platform)

              (func (export "version_len") (result i32)
                i32.const 0
                i32.const 32
                call $get_host_version))
            "#,
        )
        .unwrap()
    }

    fn create_test_context() -> PluginContext {
        PluginContext::new(
            PluginApi::current(),
            7,
            "test",
            PathBuf::from("/tmp/test-plugin-data"),
            PathBuf::from("/tmp/test-plugin-config"),
        )
    }

    #[test]
    fn test_plugin_instance_creation() {
        let wasm = create_minimal_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();

        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let instance = WasmPluginInstance::new("test", module, manifest, None);

        assert_eq!(instance.info().name, "test");
        assert_eq!(instance.state(), PluginState::Created);
    }

    #[test]
    fn test_plugin_instance_instantiate() {
        let wasm = create_minimal_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();

        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let mut instance = WasmPluginInstance::new("test", module, manifest, None);

        let result = instance.instantiate(&runtime);
        assert!(result.is_ok());
        assert!(instance.instance.is_some());
    }

    #[test]
    fn test_plugin_instance_double_instantiate() {
        let wasm = create_minimal_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();

        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let mut instance = WasmPluginInstance::new("test", module, manifest, None);

        instance.instantiate(&runtime).unwrap();
        let result = instance.instantiate(&runtime);
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_builder() {
        let wasm = create_minimal_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();

        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let result = WasmPluginBuilder::new("test")
            .module(module)
            .manifest(manifest)
            .path("/test/plugin.wasm")
            .build();

        assert!(result.is_ok());
        let instance = result.unwrap();
        assert_eq!(instance.path(), Some(&PathBuf::from("/test/plugin.wasm")));
    }

    #[test]
    fn test_plugin_builder_without_module() {
        let result = WasmPluginBuilder::new("test").build();
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_trait_impl() {
        let wasm = create_minimal_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();

        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let instance = WasmPluginInstance::new("test", module, manifest, None);

        // Testar trait Plugin
        assert_eq!(instance.info().name, "test");
        assert_eq!(instance.info().version, "1.0.0");
        assert_eq!(instance.state(), PluginState::Created);
    }

    #[test]
    fn test_store_data() {
        let mut data = PluginStoreData::new();

        data.store_data("key1", vec![1, 2, 3]);
        assert_eq!(data.get_data("key1"), Some(&vec![1, 2, 3]));
        assert_eq!(data.get_data("key2"), None);
    }

    #[test]
    fn test_wasm_init_receives_context_payload() {
        let wasm = create_context_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();
        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let mut instance = WasmPluginInstance::new("test", module, manifest, None);
        let mut ctx = create_test_context();

        instance.instantiate(&runtime).unwrap();
        instance.init(&mut ctx).unwrap();

        let len = instance.call::<(), i32>("get_init_len", ()).unwrap();
        assert!(len > 0);
    }

    #[test]
    fn test_wasm_on_input_receives_input_payload() {
        let wasm = create_context_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();
        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let mut instance = WasmPluginInstance::new("test", module, manifest, None);
        let mut ctx = create_test_context();

        instance.instantiate(&runtime).unwrap();
        instance.init(&mut ctx).unwrap();

        assert!(instance.on_input(&mut ctx, "echo"));
        assert!(!instance.on_input(&mut ctx, "noop"));
    }

    #[test]
    fn test_wasm_theme_and_config_receive_payloads() {
        let wasm = create_context_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();
        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let mut instance = WasmPluginInstance::new("test", module, manifest, None);
        let mut ctx = create_test_context();

        instance.instantiate(&runtime).unwrap();
        instance.init(&mut ctx).unwrap();
        instance.on_theme_changed(&mut ctx, "solarized");

        let mut config = HashMap::new();
        config.insert("enabled".to_string(), "true".to_string());
        instance.apply_config(&config);

        let theme_len = instance.call::<(), i32>("get_theme_len", ()).unwrap();
        let config_len = instance.call::<(), i32>("get_config_len", ()).unwrap();
        assert_eq!(theme_len, "solarized".len() as i32);
        assert!(config_len > 0);
    }

    #[test]
    fn test_wasm_can_call_system_host_imports() {
        let wasm = create_system_import_wasm();
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&wasm).unwrap();
        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let mut instance = WasmPluginInstance::new("test", module, manifest, None);

        instance.instantiate(&runtime).unwrap();

        let platform = instance.call::<(), i32>("platform", ()).unwrap();
        let version_len = instance.call::<(), i32>("version_len", ()).unwrap();

        assert!(matches!(platform, 0 | 1 | 2 | 3 | 99));
        assert!(version_len > 0);
    }

    /// Módulo que registra em globais cada callback recebido
    fn create_recording_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (global $started (mut i32) (i32.const 0))
              (global $inits (mut i32) (i32.const 0))
              (global $updates (mut i64) (i64.const 0))
              (global $grid (mut i64) (i64.const 0))
              (global $tab_created (mut i64) (i64.const 0))
              (global $tab_closed (mut i64) (i64.const 0))
              (global $shutdowns (mut i32) (i32.const 0))
              (func (export "_start") (global.set $started (i32.const 1)))
              (func (export "init") (global.set $inits (i32.add (global.get $inits) (i32.const 1))))
              (func (export "update") (param i64) (global.set $updates (i64.add (global.get $updates) (local.get 0))))
              (func (export "on_grid_changed") (param i64) (global.set $grid (local.get 0)))
              (func (export "on_tab_created") (param i64) (global.set $tab_created (local.get 0)))
              (func (export "on_tab_closed") (param i64) (global.set $tab_closed (local.get 0)))
              (func (export "shutdown") (global.set $shutdowns (i32.add (global.get $shutdowns) (i32.const 1))))
              (func (export "started") (result i32) global.get $started)
              (func (export "inits") (result i32) global.get $inits)
              (func (export "updates") (result i64) global.get $updates)
              (func (export "grid") (result i64) global.get $grid)
              (func (export "tab_created") (result i64) global.get $tab_created)
              (func (export "tab_closed") (result i64) global.get $tab_closed)
              (func (export "shutdowns") (result i32) global.get $shutdowns))
            "#,
        )
        .unwrap()
    }

    fn instance_from(wat_bytes: Vec<u8>, runtime: &WasmRuntime) -> WasmPluginInstance {
        let module = runtime.compile_bytes(&wat_bytes).unwrap();
        let mut manifest = PluginManifest::new("rec", "2.0.0", "rec.wasm");
        manifest.config.insert("cor".into(), "azul".into());
        WasmPluginInstance::new("rec", module, manifest, None)
    }

    #[test]
    fn test_callbacks_reach_guest_only_while_running() {
        let runtime = create_test_runtime();
        let mut instance = instance_from(create_recording_wasm(), &runtime);
        let mut ctx = create_test_context();
        instance.instantiate(&runtime).unwrap();
        assert_eq!(instance.call::<(), i32>("started", ()).unwrap(), 1);
        assert!(format!("{:?}", instance).contains("instantiated: true"));

        // Antes do init (Initializing) os callbacks são ignorados
        instance.update(&mut ctx, 5);
        assert_eq!(instance.call::<(), i64>("updates", ()).unwrap(), 0);

        // init sem parâmetros (fallback)
        instance.init(&mut ctx).unwrap();
        assert_eq!(instance.call::<(), i32>("inits", ()).unwrap(), 1);
        assert_eq!(instance.state(), PluginState::Running);
        // init de novo é inválido
        assert!(instance.init(&mut ctx).is_err());

        instance.update(&mut ctx, 7);
        instance.update(&mut ctx, 3);
        instance.on_grid_changed(&mut ctx, 11);
        instance.on_tab_created(&mut ctx, 12);
        instance.on_tab_closed(&mut ctx, 13);
        assert_eq!(instance.call::<(), i64>("updates", ()).unwrap(), 10);
        assert_eq!(instance.call::<(), i64>("grid", ()).unwrap(), 11);
        assert_eq!(instance.call::<(), i64>("tab_created", ()).unwrap(), 12);
        assert_eq!(instance.call::<(), i64>("tab_closed", ()).unwrap(), 13);

        // Sem on_input/on_theme_changed/apply_config exportados: no-op
        assert!(!instance.on_input(&mut ctx, "x"));
        instance.on_theme_changed(&mut ctx, "escuro");
        instance.apply_config(&HashMap::new());
        assert_eq!(
            instance.default_config().get("cor").map(String::as_str),
            Some("azul")
        );
        assert_eq!(instance.manifest().plugin.version, "2.0.0");

        Plugin::shutdown(&mut instance, &mut ctx);
        assert_eq!(instance.state(), PluginState::Stopped);
        assert!(!instance.has_function("shutdown"));
        // Parado: callbacks ignorados e sem panic
        instance.update(&mut ctx, 1);
        instance.on_grid_changed(&mut ctx, 1);
        instance.on_tab_created(&mut ctx, 1);
        instance.on_tab_closed(&mut ctx, 1);
        instance.on_theme_changed(&mut ctx, "x");
        assert!(!instance.on_input(&mut ctx, "x"));
    }

    #[test]
    fn test_uninstantiated_instance_reports_errors() {
        let runtime = create_test_runtime();
        let mut instance = instance_from(create_minimal_wasm(), &runtime);
        assert!(!instance.has_function("init"));
        assert!(instance.get_memory().is_none());
        assert!(instance
            .call::<(), ()>("x", ())
            .unwrap_err()
            .to_string()
            .contains("not instantiated"));
        assert!(instance.read_memory(0, 1).is_err());
        assert!(instance.write_memory(0, b"x").is_err());
        // init sem instância: nada a chamar, vira Running
        let mut ctx = create_test_context();
        instance.init(&mut ctx).unwrap();
        assert_eq!(instance.state(), PluginState::Running);
    }

    #[test]
    fn test_memory_roundtrip_and_bounds() {
        let runtime = create_test_runtime();
        let mut instance = instance_from(create_context_wasm(), &runtime);
        instance.instantiate(&runtime).unwrap();
        instance.write_memory(100, b"ola").unwrap();
        assert_eq!(instance.read_memory(100, 3).unwrap(), b"ola");

        let page = 65536;
        assert!(instance.read_memory(page - 1, 2).is_err());
        assert!(instance.write_memory(page, b"x").is_err());
        // Regressão: offset + len estourava usize e dava pânico
        let err = instance.read_memory(usize::MAX, 2).unwrap_err();
        assert!(err.to_string().contains("out of bounds"), "{err}");
        assert!(instance.write_memory(usize::MAX, b"xy").is_err());

        // Módulo sem memória
        let mut bare = instance_from(create_minimal_wasm(), &runtime);
        bare.instantiate(&runtime).unwrap();
        let err = bare.read_memory(0, 1).unwrap_err();
        assert!(err.to_string().contains("no memory export"), "{err}");
    }

    #[test]
    fn test_instantiation_failures() {
        let runtime = create_test_runtime();
        // Import que o host não fornece
        let unknown = wat::parse_str(r#"(module (import "host" "nao_existe" (func)))"#).unwrap();
        let mut instance = instance_from(unknown, &runtime);
        let err = instance.instantiate(&runtime).unwrap_err();
        assert!(err.to_string().contains("Failed to instantiate"), "{err}");

        // _start que falha
        let trap = wat::parse_str(r#"(module (func (export "_start") unreachable))"#).unwrap();
        let mut instance = instance_from(trap, &runtime);
        let err = instance.instantiate(&runtime).unwrap_err();
        assert!(err.to_string().contains("_start failed"), "{err}");
    }

    #[test]
    fn test_guest_allocation_failures_are_handled() {
        let runtime = create_test_runtime();
        // malloc devolve ponteiro negativo; free com assinatura errada
        let bad_alloc = wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "malloc") (param i32) (result i32) i32.const -1)
              (func (export "on_input") (param i32 i32) (result i32) i32.const 1)
              (func (export "on_theme_changed") (param i32 i32))
              (func (export "apply_config") (param i32 i32)))
            "#,
        )
        .unwrap();
        let mut instance = instance_from(bad_alloc, &runtime);
        let mut ctx = create_test_context();
        instance.instantiate(&runtime).unwrap();
        instance.init(&mut ctx).unwrap();
        assert!(!instance.on_input(&mut ctx, "x"));
        instance.on_theme_changed(&mut ctx, "tema");
        instance.apply_config(&HashMap::new());

        // Sem malloc exportado: falha ao alocar
        let no_malloc = wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "on_input") (param i32 i32) (result i32) i32.const 1))
            "#,
        )
        .unwrap();
        let mut instance = instance_from(no_malloc, &runtime);
        instance.instantiate(&runtime).unwrap();
        instance.init(&mut ctx).unwrap();
        assert!(!instance.on_input(&mut ctx, "x"));

        // free com assinatura errada: on_input ainda funciona (só avisa)
        let bad_free = wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (func (export "malloc") (param i32) (result i32) i32.const 64)
              (func (export "free") (param i64))
              (func (export "on_input") (param i32 i32) (result i32) i32.const 1))
            "#,
        )
        .unwrap();
        let mut instance = instance_from(bad_free, &runtime);
        instance.instantiate(&runtime).unwrap();
        instance.init(&mut ctx).unwrap();
        assert!(instance.on_input(&mut ctx, "x"));
    }

    #[test]
    fn test_builder_default_manifest_and_drop_shutdown() {
        let runtime = create_test_runtime();
        let module = runtime.compile_bytes(&create_recording_wasm()).unwrap();
        let mut instance = WasmPluginBuilder::new("anonimo")
            .module(module)
            .build()
            .unwrap();
        assert_eq!(instance.manifest().plugin.version, "0.0.0");
        assert!(instance.path().is_none());
        instance.instantiate(&runtime).unwrap();
        assert!(instance.get_memory().is_some());
        // Drop com instância ativa chama shutdown sem panic
        drop(instance);
    }
}
