//! Plugin Manager
//!
//! Gerencia o ciclo de vida completo dos plugins: carregamento, execução e descarregamento.

use crate::plugin::api::PluginApi;
use crate::plugin::context::PluginContext;
use crate::plugin::error::{PluginError, PluginResult};
use crate::plugin::lifecycle::{LifecycleEvent, LifecycleManager, PluginLifecycle, PluginState};
use crate::plugin::manifest::{Permission, PluginManifest};
use crate::plugin::sandbox::{ResourceUsage, Sandbox, SandboxConfig};
use crate::plugin::{BoxedPlugin, PluginInfo};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

static NEXT_PLUGIN_ID: AtomicU64 = AtomicU64::new(1);

fn generate_plugin_id() -> u64 {
    NEXT_PLUGIN_ID.fetch_add(1, Ordering::SeqCst)
}

pub struct LoadedPlugin {
    pub id: u64,
    pub name: String,
    pub manifest: PluginManifest,
    pub plugin: BoxedPlugin,
    pub context: PluginContext,
    pub lifecycle: Arc<PluginLifecycle>,
    pub sandbox: Sandbox,
    pub dependencies: Vec<String>,
    pub dependents: Vec<String>,
    pub loaded_at: std::time::Instant,
    pub wasm_instance: Option<WasmInstanceHandle>,
}

impl LoadedPlugin {
    pub fn state(&self) -> PluginState {
        self.lifecycle.get()
    }

    pub fn is_running(&self) -> bool {
        self.state() == PluginState::Running
    }

    pub fn resource_usage(&self) -> &ResourceUsage {
        self.sandbox.resource_usage()
    }

    pub fn uptime(&self) -> Duration {
        self.loaded_at.elapsed()
    }
}

impl std::fmt::Debug for LoadedPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedPlugin")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("state", &self.state())
            .field("dependencies", &self.dependencies)
            .field("dependents", &self.dependents)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct WasmInstanceHandle {
    pub module_hash: [u8; 32],
    pub memory_pages: u32,
}

#[derive(Debug, Clone)]
pub struct UnloadOptions {
    pub force: bool,
    pub timeout_ms: u64,
    pub cleanup_data: bool,
    pub notify_dependents: bool,
}

impl Default for UnloadOptions {
    fn default() -> Self {
        Self {
            force: false,
            timeout_ms: 5000,
            cleanup_data: false,
            notify_dependents: true,
        }
    }
}

impl UnloadOptions {
    pub fn force() -> Self {
        Self {
            force: true,
            timeout_ms: 1000,
            cleanup_data: false,
            notify_dependents: false,
        }
    }

    pub fn graceful() -> Self {
        Self {
            force: false,
            timeout_ms: 10000,
            cleanup_data: false,
            notify_dependents: true,
        }
    }

    pub fn with_cleanup(mut self) -> Self {
        self.cleanup_data = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnloadResult {
    Unloaded {
        name: String,
        id: u64,
    },
    NotFound {
        name: String,
    },
    HasDependents {
        name: String,
        dependents: Vec<String>,
    },
    Timeout {
        name: String,
    },
    Error {
        name: String,
        reason: String,
    },
}

impl UnloadResult {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Unloaded { .. })
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Unloaded { name, .. } => name,
            Self::NotFound { name } => name,
            Self::HasDependents { name, .. } => name,
            Self::Timeout { name } => name,
            Self::Error { name, .. } => name,
        }
    }
}

pub struct PluginManagerConfig {
    pub plugins_dir: PathBuf,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub default_sandbox_config: SandboxConfig,
    pub auto_enable_permissions: bool,
    pub max_plugins: usize,
    pub unload_timeout_ms: u64,
}

impl Default for PluginManagerConfig {
    fn default() -> Self {
        Self {
            plugins_dir: PathBuf::from("plugins"),
            data_dir: std::env::temp_dir()
                .join("code-terminal")
                .join("plugin-data"),
            config_dir: std::env::temp_dir()
                .join("code-terminal")
                .join("plugin-config"),
            default_sandbox_config: SandboxConfig::moderate(),
            auto_enable_permissions: false,
            max_plugins: 100,
            unload_timeout_ms: 5000,
        }
    }
}

pub struct PluginManager {
    plugins: RwLock<HashMap<String, LoadedPlugin>>,
    config: PluginManagerConfig,
    lifecycle_manager: RwLock<LifecycleManager>,
    api: PluginApi,
}

impl PluginManager {
    pub fn new(config: PluginManagerConfig) -> Self {
        Self {
            plugins: RwLock::new(HashMap::new()),
            config,
            lifecycle_manager: RwLock::new(LifecycleManager::new()),
            api: PluginApi::current(),
        }
    }

    pub fn load(
        &self,
        manifest: PluginManifest,
        mut plugin: BoxedPlugin,
    ) -> PluginResult<Arc<PluginLifecycle>> {
        let name = manifest.plugin.name.clone();

        {
            let plugins = self.plugins.read();
            if plugins.contains_key(&name) {
                return Err(PluginError::already_loaded(&name));
            }
        }

        if self.plugins.read().len() >= self.config.max_plugins {
            return Err(PluginError::Other {
                message: format!(
                    "Maximum number of plugins ({}) reached",
                    self.config.max_plugins
                ),
            });
        }

        let id = generate_plugin_id();
        let lifecycle = Arc::new(PluginLifecycle::new());

        let data_dir = self.config.data_dir.join(&name);
        let config_dir = self.config.config_dir.join(&name);

        std::fs::create_dir_all(&data_dir)
            .map_err(|_e| PluginError::io("create_data_dir", data_dir.display().to_string()))?;
        std::fs::create_dir_all(&config_dir)
            .map_err(|_e| PluginError::io("create_config_dir", config_dir.display().to_string()))?;

        let mut context = PluginContext::new(self.api, id, &name, data_dir, config_dir);

        let sandbox_config = self.build_sandbox_config(&manifest);
        let sandbox = Sandbox::new(sandbox_config);

        let _ = lifecycle.transition(PluginState::Initializing);
        self.lifecycle_manager
            .write()
            .notify(&name, LifecycleEvent::Initializing);

        if let Err(e) = plugin.init(&mut context) {
            lifecycle.set_error();
            self.lifecycle_manager.write().notify(
                &name,
                LifecycleEvent::Error {
                    reason: e.to_string(),
                },
            );
            return Err(e);
        }

        let _ = lifecycle.transition(PluginState::Running);
        self.lifecycle_manager
            .write()
            .notify(&name, LifecycleEvent::Started);

        let dependencies: Vec<String> = manifest
            .dependencies
            .iter()
            .map(|d| d.name.clone())
            .collect();

        let loaded = LoadedPlugin {
            id,
            name: name.clone(),
            manifest,
            plugin,
            context,
            lifecycle: lifecycle.clone(),
            sandbox,
            dependencies,
            dependents: Vec::new(),
            loaded_at: std::time::Instant::now(),
            wasm_instance: None,
        };

        {
            let mut plugins = self.plugins.write();
            plugins.insert(name.clone(), loaded);
        }

        self.update_dependency_graph(&name);

        tracing::info!("Plugin '{}' (id={}) loaded successfully", name, id);
        Ok(lifecycle)
    }

    pub fn unload(&self, name: &str, options: UnloadOptions) -> UnloadResult {
        let loaded_plugin = {
            let plugins = self.plugins.read();
            match plugins.get(name) {
                Some(p) => (
                    p.id,
                    p.name.clone(),
                    p.lifecycle.clone(),
                    p.dependents.clone(),
                ),
                None => {
                    return UnloadResult::NotFound {
                        name: name.to_string(),
                    }
                }
            }
        };

        let (id, plugin_name, lifecycle, dependents) = loaded_plugin;

        if !options.force && !dependents.is_empty() {
            return UnloadResult::HasDependents {
                name: plugin_name,
                dependents,
            };
        }

        if options.notify_dependents {
            for dep_name in &dependents {
                self.notify_dependent_unloaded(dep_name, &plugin_name);
            }
        }

        let transition_result = lifecycle.transition(PluginState::Stopping);
        if transition_result.is_err() && !options.force {
            return UnloadResult::Error {
                name: plugin_name.clone(),
                reason: "Plugin cannot be stopped in current state".to_string(),
            };
        }

        self.lifecycle_manager
            .write()
            .notify(&plugin_name, LifecycleEvent::Stopping);

        let shutdown_result = self.perform_shutdown(name, options.timeout_ms);

        if shutdown_result.is_err() && !options.force {
            lifecycle.set_error();
            return UnloadResult::Timeout { name: plugin_name };
        }

        let _ = lifecycle.transition(PluginState::Stopped);
        self.lifecycle_manager
            .write()
            .notify(&plugin_name, LifecycleEvent::Stopped);

        let removed = {
            let mut plugins = self.plugins.write();
            plugins.remove(name)
        };

        if let Some(mut removed_plugin) = removed {
            removed_plugin.sandbox.shutdown();

            if options.cleanup_data {
                self.cleanup_plugin_data(&removed_plugin);
            }

            self.remove_from_dependency_graph(&plugin_name);

            tracing::info!("Plugin '{}' (id={}) unloaded successfully", plugin_name, id);
        }

        UnloadResult::Unloaded {
            name: plugin_name,
            id,
        }
    }

    pub fn unload_all(&self, options: UnloadOptions) -> Vec<UnloadResult> {
        let names: Vec<String> = {
            let plugins = self.plugins.read();
            plugins.keys().cloned().collect()
        };

        let mut results = Vec::new();
        let mut unloaded = Vec::new();

        let sorted = self.sort_by_dependency_order(&names);

        for name in sorted.iter().rev() {
            let result = self.unload(name, options.clone());
            if result.is_success() {
                unloaded.push(name.clone());
            }
            results.push(result);
        }

        results
    }

    pub fn unload_by_prefix(&self, prefix: &str, options: UnloadOptions) -> Vec<UnloadResult> {
        let names: Vec<String> = {
            let plugins = self.plugins.read();
            plugins
                .keys()
                .filter(|n| n.starts_with(prefix))
                .cloned()
                .collect()
        };

        names
            .iter()
            .map(|name| self.unload(name, options.clone()))
            .collect()
    }

    pub fn reload(&self, name: &str) -> PluginResult<Arc<PluginLifecycle>> {
        let _manifest = {
            let plugins = self.plugins.read();
            plugins
                .get(name)
                .ok_or_else(|| PluginError::not_found(name))?
                .manifest
                .clone()
        };

        let result = self.unload(name, UnloadOptions::graceful());
        if !result.is_success() {
            return Err(PluginError::Other {
                message: format!("Failed to unload plugin for reload: {:?}", result),
            });
        }

        Err(PluginError::Other {
            message: "Reload requires re-instantiating the plugin".to_string(),
        })
    }

    pub fn get(&self, name: &str) -> Option<PluginInfo> {
        let plugins = self.plugins.read();
        plugins.get(name).map(|p| p.plugin.info())
    }

    pub fn get_state(&self, name: &str) -> Option<PluginState> {
        let plugins = self.plugins.read();
        plugins.get(name).map(|p| p.state())
    }

    pub fn get_resource_usage(&self, name: &str) -> Option<ResourceUsage> {
        let plugins = self.plugins.read();
        plugins.get(name).map(|p| p.resource_usage().clone())
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        let plugins = self.plugins.read();
        plugins.values().map(|p| p.plugin.info()).collect()
    }

    pub fn list_names(&self) -> Vec<String> {
        let plugins = self.plugins.read();
        plugins.keys().cloned().collect()
    }

    pub fn count(&self) -> usize {
        self.plugins.read().len()
    }

    pub fn is_loaded(&self, name: &str) -> bool {
        self.plugins.read().contains_key(name)
    }

    pub fn get_dependents(&self, name: &str) -> Vec<String> {
        let plugins = self.plugins.read();
        plugins
            .get(name)
            .map(|p| p.dependents.clone())
            .unwrap_or_default()
    }

    pub fn get_dependencies(&self, name: &str) -> Vec<String> {
        let plugins = self.plugins.read();
        plugins
            .get(name)
            .map(|p| p.dependencies.clone())
            .unwrap_or_default()
    }

    pub fn update(&self, delta_ms: u64) {
        let names: Vec<String> = {
            let plugins = self.plugins.read();
            plugins
                .values()
                .filter(|p| p.is_running())
                .map(|p| p.name.clone())
                .collect()
        };

        for name in names {
            let mut plugins = self.plugins.write();
            if let Some(loaded) = plugins.get_mut(&name) {
                loaded.plugin.update(&mut loaded.context, delta_ms);
            }
        }
    }

    pub fn on_input(&self, input: &str) -> bool {
        let names: Vec<String> = {
            let plugins = self.plugins.read();
            plugins
                .values()
                .filter(|p| p.is_running())
                .map(|p| p.name.clone())
                .collect()
        };

        for name in names {
            let mut plugins = self.plugins.write();
            if let Some(loaded) = plugins.get_mut(&name) {
                if loaded.plugin.on_input(&mut loaded.context, input) {
                    return true;
                }
            }
        }

        false
    }

    fn perform_shutdown(&self, name: &str, _timeout_ms: u64) -> PluginResult<()> {
        let mut plugins = self.plugins.write();

        if let Some(loaded) = plugins.get_mut(name) {
            loaded.plugin.shutdown(&mut loaded.context);
            Ok(())
        } else {
            Err(PluginError::not_found(name))
        }
    }

    fn build_sandbox_config(&self, manifest: &PluginManifest) -> SandboxConfig {
        let mut config = self.config.default_sandbox_config.clone();

        if self.config.auto_enable_permissions {
            for perm in &manifest.permissions {
                match perm {
                    Permission::Filesystem => {
                        config.permissions.insert(Permission::Filesystem);
                    }
                    Permission::Network => {
                        config.permissions.insert(Permission::Network);
                    }
                    Permission::Shell => {
                        config.permissions.insert(Permission::Shell);
                    }
                    _ => {}
                }
            }
        }

        config
    }

    fn update_dependency_graph(&self, name: &str) {
        let mut plugins = self.plugins.write();

        let Some(deps) = plugins.get(name).map(|loaded| loaded.dependencies.clone()) else {
            return;
        };

        // As dependências do novo plugin passam a listá-lo como dependente
        for dep_name in deps {
            if let Some(dep) = plugins.get_mut(&dep_name) {
                if !dep.dependents.iter().any(|d| d == name) {
                    dep.dependents.push(name.to_string());
                }
            }
        }

        // Plugins carregados antes que declaram depender deste viram seus
        // dependentes (antes só a ordem dependência -> dependente funcionava)
        let dependents: Vec<String> = plugins
            .values()
            .filter(|p| p.name != name && p.manifest.dependencies.iter().any(|d| d.name == name))
            .map(|p| p.name.clone())
            .collect();
        if let Some(loaded) = plugins.get_mut(name) {
            for dependent in dependents {
                if !loaded.dependents.contains(&dependent) {
                    loaded.dependents.push(dependent);
                }
            }
        }
    }

    fn remove_from_dependency_graph(&self, name: &str) {
        let mut plugins = self.plugins.write();

        for loaded in plugins.values_mut() {
            loaded.dependents.retain(|d| d != name);
            loaded.dependencies.retain(|d| d != name);
        }
    }

    fn notify_dependent_unloaded(&self, dependent_name: &str, unloaded_name: &str) {
        let plugins = self.plugins.read();
        if let Some(_loaded) = plugins.get(dependent_name) {
            tracing::warn!(
                "Plugin '{}' lost dependency '{}'",
                dependent_name,
                unloaded_name
            );
        }
    }

    fn cleanup_plugin_data(&self, plugin: &LoadedPlugin) {
        let data_dir = plugin.context.data_dir();
        let config_dir = plugin.context.config_dir();

        if let Err(e) = std::fs::remove_dir_all(data_dir) {
            tracing::warn!("Failed to cleanup data dir for '{}': {}", plugin.name, e);
        }

        if let Err(e) = std::fs::remove_dir_all(config_dir) {
            tracing::warn!("Failed to cleanup config dir for '{}': {}", plugin.name, e);
        }
    }

    fn sort_by_dependency_order(&self, names: &[String]) -> Vec<String> {
        let plugins = self.plugins.read();
        let mut sorted = Vec::new();
        let mut visited = std::collections::HashSet::new();

        fn visit(
            name: &str,
            plugins: &HashMap<String, LoadedPlugin>,
            visited: &mut std::collections::HashSet<String>,
            sorted: &mut Vec<String>,
        ) {
            if visited.contains(name) {
                return;
            }

            visited.insert(name.to_string());

            if let Some(plugin) = plugins.get(name) {
                for dep in &plugin.dependencies {
                    visit(dep, plugins, visited, sorted);
                }
            }

            sorted.push(name.to_string());
        }

        for name in names {
            visit(name, &plugins, &mut visited, &mut sorted);
        }

        sorted
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new(PluginManagerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Plugin, PluginInfo, WasmPluginLoader, WasmRuntime};

    #[derive(Debug)]
    struct MockPlugin {
        info: PluginInfo,
        init_called: bool,
        shutdown_called: bool,
    }

    impl MockPlugin {
        fn new(name: &str) -> Self {
            Self {
                info: PluginInfo {
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    author: "test".to_string(),
                    description: "test plugin".to_string(),
                    homepage: None,
                    repository: None,
                },
                init_called: false,
                shutdown_called: false,
            }
        }
    }

    impl Plugin for MockPlugin {
        fn info(&self) -> PluginInfo {
            self.info.clone()
        }

        fn init(&mut self, _ctx: &mut PluginContext) -> PluginResult<()> {
            self.init_called = true;
            Ok(())
        }

        fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {}

        fn shutdown(&mut self, _ctx: &mut PluginContext) {
            self.shutdown_called = true;
        }
    }

    fn create_test_manager() -> PluginManager {
        let config = PluginManagerConfig {
            plugins_dir: std::env::temp_dir().join("test-plugins"),
            data_dir: std::env::temp_dir().join("test-plugin-data"),
            config_dir: std::env::temp_dir().join("test-plugin-config"),
            ..Default::default()
        };
        PluginManager::new(config)
    }

    fn create_test_manifest(name: &str) -> PluginManifest {
        PluginManifest::new(name, "1.0.0", "plugin.wasm")
    }

    fn create_input_consuming_wasm() -> Vec<u8> {
        wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (global $heap (mut i32) (i32.const 1024))

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

              (func (export "init") (param $ptr i32) (param $len i32))

              (func (export "update") (param $delta i64))

              (func (export "shutdown"))

              (func (export "on_input") (param $ptr i32) (param $len i32) (result i32)
                local.get $len
                i32.const 4
                i32.eq
                local.get $ptr
                i32.load8_u
                i32.const 101
                i32.eq
                i32.and))
            "#,
        )
        .unwrap()
    }

    #[test]
    fn test_manager_new() {
        let manager = create_test_manager();
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_load_plugin() {
        let manager = create_test_manager();
        let manifest = create_test_manifest("test-plugin");
        let plugin = Box::new(MockPlugin::new("test-plugin"));

        let result = manager.load(manifest, plugin);
        assert!(result.is_ok());
        assert_eq!(manager.count(), 1);
    }

    #[test]
    fn test_load_duplicate_plugin() {
        let manager = create_test_manager();

        let manifest1 = create_test_manifest("test-plugin");
        let plugin1 = Box::new(MockPlugin::new("test-plugin"));
        manager.load(manifest1, plugin1).unwrap();

        let manifest2 = create_test_manifest("test-plugin");
        let plugin2 = Box::new(MockPlugin::new("test-plugin"));
        let result = manager.load(manifest2, plugin2);

        assert!(result.is_err());
    }

    #[test]
    fn test_unload_plugin() {
        let manager = create_test_manager();
        let manifest = create_test_manifest("test-plugin");
        let plugin = Box::new(MockPlugin::new("test-plugin"));
        manager.load(manifest, plugin).unwrap();

        let result = manager.unload("test-plugin", UnloadOptions::default());

        assert!(result.is_success());
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_unload_not_found() {
        let manager = create_test_manager();
        let result = manager.unload("nonexistent", UnloadOptions::default());

        assert!(matches!(result, UnloadResult::NotFound { .. }));
    }

    #[test]
    fn test_unload_all() {
        let manager = create_test_manager();

        for name in &["plugin-a", "plugin-b", "plugin-c"] {
            let manifest = create_test_manifest(name);
            let plugin = Box::new(MockPlugin::new(name));
            manager.load(manifest, plugin).unwrap();
        }

        assert_eq!(manager.count(), 3);

        let results = manager.unload_all(UnloadOptions::default());

        assert_eq!(results.len(), 3);
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_unload_by_prefix() {
        let manager = create_test_manager();

        for name in &["foo-a", "foo-b", "bar-c"] {
            let manifest = create_test_manifest(name);
            let plugin = Box::new(MockPlugin::new(name));
            manager.load(manifest, plugin).unwrap();
        }

        let results = manager.unload_by_prefix("foo-", UnloadOptions::default());

        assert_eq!(results.len(), 2);
        assert_eq!(manager.count(), 1);
        assert!(manager.is_loaded("bar-c"));
    }

    #[test]
    fn test_is_loaded() {
        let manager = create_test_manager();

        assert!(!manager.is_loaded("test-plugin"));

        let manifest = create_test_manifest("test-plugin");
        let plugin = Box::new(MockPlugin::new("test-plugin"));
        manager.load(manifest, plugin).unwrap();

        assert!(manager.is_loaded("test-plugin"));
    }

    #[test]
    fn test_get_state() {
        let manager = create_test_manager();
        let manifest = create_test_manifest("test-plugin");
        let plugin = Box::new(MockPlugin::new("test-plugin"));
        manager.load(manifest, plugin).unwrap();

        let state = manager.get_state("test-plugin");
        assert!(state.is_some());
        assert_eq!(state.unwrap(), PluginState::Running);
    }

    #[test]
    fn test_list_names() {
        let manager = create_test_manager();

        for name in &["plugin-a", "plugin-b"] {
            let manifest = create_test_manifest(name);
            let plugin = Box::new(MockPlugin::new(name));
            manager.load(manifest, plugin).unwrap();
        }

        let names = manager.list_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"plugin-a".to_string()));
        assert!(names.contains(&"plugin-b".to_string()));
    }

    #[test]
    fn test_unload_options_default() {
        let options = UnloadOptions::default();
        assert!(!options.force);
        assert_eq!(options.timeout_ms, 5000);
        assert!(!options.cleanup_data);
    }

    #[test]
    fn test_unload_options_force() {
        let options = UnloadOptions::force();
        assert!(options.force);
        assert_eq!(options.timeout_ms, 1000);
    }

    #[test]
    fn test_unload_result_is_success() {
        let success = UnloadResult::Unloaded {
            name: "test".to_string(),
            id: 1,
        };
        assert!(success.is_success());

        let failure = UnloadResult::NotFound {
            name: "test".to_string(),
        };
        assert!(!failure.is_success());
    }

    #[test]
    fn test_generate_plugin_id() {
        let id1 = generate_plugin_id();
        let id2 = generate_plugin_id();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_manager_dispatches_input_to_loaded_wasm_plugin() {
        let manager = create_test_manager();
        let runtime = Arc::new(WasmRuntime::new().unwrap());
        let mut loader = WasmPluginLoader::new(runtime, std::env::temp_dir());
        let wasm = create_input_consuming_wasm();
        let loaded = loader.load_from_bytes("wasm-input", &wasm).unwrap();
        let manifest = create_test_manifest("wasm-input");
        let instance = loader.create_instance(&loaded, manifest.clone()).unwrap();

        manager.load(manifest, Box::new(instance)).unwrap();

        assert!(manager.on_input("echo"));
        assert!(!manager.on_input("noop"));
    }

    use crate::plugin::manifest::PluginDependency;
    use std::sync::atomic::AtomicUsize;

    /// Plugin configurável para os testes do manager
    #[derive(Debug, Default)]
    struct Probe {
        name: String,
        fail_init: bool,
        consume_input: bool,
        updates: Arc<AtomicUsize>,
        shutdowns: Arc<AtomicUsize>,
    }

    impl Probe {
        fn named(name: &str) -> Self {
            Self {
                name: name.to_string(),
                ..Default::default()
            }
        }
    }

    impl Plugin for Probe {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: self.name.clone(),
                version: "1.0.0".into(),
                author: "t".into(),
                description: "probe".into(),
                homepage: None,
                repository: None,
            }
        }
        fn init(&mut self, _ctx: &mut PluginContext) -> PluginResult<()> {
            if self.fail_init {
                Err(PluginError::init_failed(&self.name, "falhou de propósito"))
            } else {
                Ok(())
            }
        }
        fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {
            self.updates.fetch_add(1, Ordering::SeqCst);
        }
        fn shutdown(&mut self, _ctx: &mut PluginContext) {
            self.shutdowns.fetch_add(1, Ordering::SeqCst);
        }
        fn on_input(&mut self, _ctx: &mut PluginContext, _input: &str) -> bool {
            self.consume_input
        }
    }

    fn manager_in(dir: &std::path::Path) -> PluginManager {
        PluginManager::new(PluginManagerConfig {
            plugins_dir: dir.join("plugins"),
            data_dir: dir.join("data"),
            config_dir: dir.join("config"),
            ..Default::default()
        })
    }

    fn manifest_with_deps(name: &str, deps: &[&str]) -> PluginManifest {
        let mut manifest = create_test_manifest(name);
        manifest.dependencies = deps
            .iter()
            .map(|d| PluginDependency::required(*d, "1.0.0"))
            .collect();
        manifest
    }

    #[test]
    fn test_dependency_graph_links_dependents() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        manager
            .load(create_test_manifest("base"), Box::new(Probe::named("base")))
            .unwrap();
        manager
            .load(
                manifest_with_deps("child", &["base"]),
                Box::new(Probe::named("child")),
            )
            .unwrap();

        assert_eq!(manager.get_dependents("base"), vec!["child"]);
        assert_eq!(manager.get_dependencies("child"), vec!["base"]);
        assert!(manager.get_dependents("nada").is_empty());
        assert!(manager.get_dependencies("nada").is_empty());

        // Com dependente carregado, unload normal é recusado
        let result = manager.unload("base", UnloadOptions::default());
        assert_eq!(
            result,
            UnloadResult::HasDependents {
                name: "base".into(),
                dependents: vec!["child".into()]
            }
        );
        assert_eq!(result.name(), "base");

        // Forçado: sai e o grafo é limpo
        assert!(manager.unload("base", UnloadOptions::force()).is_success());
        assert!(manager.get_dependencies("child").is_empty());
    }

    /// Regressão: carregar o dependente antes da dependência não registrava a
    /// relação, e a dependência podia ser descarregada sem aviso.
    #[test]
    fn test_dependency_graph_independent_of_load_order() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        manager
            .load(
                manifest_with_deps("child", &["base"]),
                Box::new(Probe::named("child")),
            )
            .unwrap();
        manager
            .load(create_test_manifest("base"), Box::new(Probe::named("base")))
            .unwrap();

        assert_eq!(manager.get_dependents("base"), vec!["child"]);
        assert!(matches!(
            manager.unload("base", UnloadOptions::default()),
            UnloadResult::HasDependents { .. }
        ));

        // unload_all respeita a ordem: dependente sai antes da dependência
        let results = manager.unload_all(UnloadOptions::default());
        assert!(results.iter().all(UnloadResult::is_success), "{results:?}");
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_failed_init_is_not_registered() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        let probe = Probe {
            fail_init: true,
            ..Probe::named("ruim")
        };
        let err = manager
            .load(create_test_manifest("ruim"), Box::new(probe))
            .unwrap_err();
        assert!(err.to_string().contains("falhou de propósito"), "{err}");
        assert!(!manager.is_loaded("ruim"));
    }

    #[test]
    fn test_max_plugins_limit() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager_in(dir.path());
        manager.config.max_plugins = 1;
        manager
            .load(create_test_manifest("a"), Box::new(Probe::named("a")))
            .unwrap();
        let err = manager
            .load(create_test_manifest("b"), Box::new(Probe::named("b")))
            .unwrap_err();
        assert!(
            err.to_string().contains("Maximum number of plugins"),
            "{err}"
        );
    }

    #[test]
    fn test_update_input_info_and_usage() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        let updates = Arc::new(AtomicUsize::new(0));
        let probe = Probe {
            updates: updates.clone(),
            ..Probe::named("quieto")
        };
        manager
            .load(create_test_manifest("quieto"), Box::new(probe))
            .unwrap();

        manager.update(16);
        manager.update(16);
        assert_eq!(updates.load(Ordering::SeqCst), 2);
        assert!(!manager.on_input("ls"));

        manager
            .load(
                create_test_manifest("guloso"),
                Box::new(Probe {
                    consume_input: true,
                    ..Probe::named("guloso")
                }),
            )
            .unwrap();
        assert!(manager.on_input("x"));

        assert_eq!(manager.get("quieto").unwrap().description, "probe");
        assert!(manager.get("nada").is_none());
        assert_eq!(manager.list().len(), 2);
        assert!(manager.get_resource_usage("quieto").is_some());
        assert!(manager.get_resource_usage("nada").is_none());
        assert!(manager.get_state("nada").is_none());
    }

    #[test]
    fn test_unload_cleanup_shutdown_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        let shutdowns = Arc::new(AtomicUsize::new(0));
        manager
            .load(
                create_test_manifest("p"),
                Box::new(Probe {
                    shutdowns: shutdowns.clone(),
                    ..Probe::named("p")
                }),
            )
            .unwrap();
        let data_dir = dir.path().join("data").join("p");
        assert!(data_dir.exists());

        let result = manager.unload("p", UnloadOptions::graceful().with_cleanup());
        assert!(result.is_success());
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
        assert!(!data_dir.exists());
        assert!(!dir.path().join("config").join("p").exists());

        // reload de desconhecido falha; de conhecido descarrega e pede nova instância
        assert!(manager.reload("p").is_err());
        manager
            .load(create_test_manifest("q"), Box::new(Probe::named("q")))
            .unwrap();
        let err = manager.reload("q").unwrap_err();
        assert!(err.to_string().contains("re-instantiating"), "{err}");
        assert!(!manager.is_loaded("q"));
    }

    #[test]
    fn test_unload_stopped_plugin_without_force_errors() {
        let dir = tempfile::tempdir().unwrap();
        let manager = manager_in(dir.path());
        let lifecycle = manager
            .load(create_test_manifest("p"), Box::new(Probe::named("p")))
            .unwrap();
        lifecycle.set_error();
        let result = manager.unload("p", UnloadOptions::default());
        assert!(matches!(result, UnloadResult::Error { .. }), "{result:?}");
        assert_eq!(result.name(), "p");
        // Forçando, sai mesmo assim
        assert!(manager.unload("p", UnloadOptions::force()).is_success());
    }

    #[test]
    fn test_loaded_plugin_accessors_and_sandbox_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = manager_in(dir.path());
        manager.config.auto_enable_permissions = true;
        let mut manifest = create_test_manifest("perm");
        manifest.permissions = vec![
            Permission::Filesystem,
            Permission::Network,
            Permission::Shell,
            Permission::Clipboard,
        ];
        let sandbox = manager.build_sandbox_config(&manifest);
        assert!(sandbox.permissions.contains(&Permission::Shell));
        assert!(sandbox.permissions.contains(&Permission::Network));

        manager
            .load(manifest, Box::new(Probe::named("perm")))
            .unwrap();
        let plugins = manager.plugins.read();
        let loaded = plugins.get("perm").unwrap();
        assert!(loaded.is_running());
        assert!(loaded.uptime() < Duration::from_secs(60));
        let debug = format!("{:?}", loaded);
        assert!(debug.contains("LoadedPlugin") && debug.contains("perm"));

        let timeout = UnloadResult::Timeout { name: "t".into() };
        assert!(!timeout.is_success());
        assert_eq!(timeout.name(), "t");
        let not_found = UnloadResult::NotFound { name: "n".into() };
        assert_eq!(not_found.name(), "n");
    }
}
