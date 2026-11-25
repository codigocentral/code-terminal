//! Testes de Integração do Sistema de Plugins
//!
//! Estes testes verificam o funcionamento integrado de todos os
//! componentes do sistema de plugins.

use code_terminal::plugin::manifest::Permission;
use code_terminal::plugin::{
    check_compatibility, Plugin, PluginApi, PluginApiVersion, PluginContext, PluginError,
    PluginInfo, PluginLifecycle, PluginManager, PluginManagerConfig, PluginManifest, PluginResult,
    PluginState, Sandbox, SandboxConfig, SandboxConfigBuilder, SandboxOperation,
};

use std::path::PathBuf;
use std::time::Duration;

fn create_test_manager() -> PluginManager {
    let temp_dir = std::env::temp_dir();
    let config = PluginManagerConfig {
        plugins_dir: temp_dir.join("test-plugins-integration"),
        data_dir: temp_dir.join("test-plugin-data-integration"),
        config_dir: temp_dir.join("test-plugin-config-integration"),
        ..Default::default()
    };
    PluginManager::new(config)
}

fn create_test_manifest(name: &str) -> PluginManifest {
    let mut manifest = PluginManifest::new(name, "1.0.0", "plugin.wasm");
    manifest.plugin.description = format!("Plugin de teste: {}", name);
    manifest.plugin.author = "Test Suite".to_string();
    manifest
}

#[derive(Debug)]
struct IntegrationTestPlugin {
    info: PluginInfo,
    init_called: bool,
    shutdown_called: bool,
    update_count: u32,
    input_received: Option<String>,
}

impl IntegrationTestPlugin {
    fn new(name: &str) -> Self {
        Self {
            info: PluginInfo {
                name: name.to_string(),
                version: "1.0.0".to_string(),
                author: "Test Suite".to_string(),
                description: "Plugin de teste de integração".to_string(),
                homepage: None,
                repository: None,
            },
            init_called: false,
            shutdown_called: false,
            update_count: 0,
            input_received: None,
        }
    }
}

impl Plugin for IntegrationTestPlugin {
    fn info(&self) -> PluginInfo {
        self.info.clone()
    }

    fn init(&mut self, _ctx: &mut PluginContext) -> PluginResult<()> {
        self.init_called = true;
        Ok(())
    }

    fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {
        self.update_count += 1;
    }

    fn shutdown(&mut self, _ctx: &mut PluginContext) {
        self.shutdown_called = true;
    }

    fn on_input(&mut self, _ctx: &mut PluginContext, input: &str) -> bool {
        self.input_received = Some(input.to_string());
        input.starts_with("/test")
    }
}

mod manager_integration {
    use super::*;

    #[test]
    fn test_full_plugin_lifecycle() {
        let manager = create_test_manager();
        let manifest = create_test_manifest("lifecycle-test");
        let plugin = Box::new(IntegrationTestPlugin::new("lifecycle-test"));

        let lifecycle = manager.load(manifest, plugin).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Running);
        assert!(manager.is_loaded("lifecycle-test"));

        let state = manager.get_state("lifecycle-test");
        assert_eq!(state, Some(PluginState::Running));

        manager.update(100);

        let result = manager.unload("lifecycle-test", Default::default());
        assert!(result.is_success());
        assert!(!manager.is_loaded("lifecycle-test"));
    }

    #[test]
    fn test_multiple_plugins_management() {
        let manager = create_test_manager();

        for i in 0..5 {
            let name = format!("multi-plugin-{}", i);
            let manifest = create_test_manifest(&name);
            let plugin = Box::new(IntegrationTestPlugin::new(&name));
            manager.load(manifest, plugin).unwrap();
        }

        assert_eq!(manager.count(), 5);

        let names = manager.list_names();
        assert_eq!(names.len(), 5);

        manager.update(50);

        let results = manager.unload_all(Default::default());
        assert_eq!(results.len(), 5);
        assert!(results.iter().all(|r| r.is_success()));
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_plugin_reload_cycle() {
        let manager = create_test_manager();
        let name = "reload-test";
        let manifest = create_test_manifest(name);
        let plugin = Box::new(IntegrationTestPlugin::new(name));

        manager.load(manifest, plugin).unwrap();
        assert!(manager.is_loaded(name));

        manager.unload(name, Default::default());
        assert!(!manager.is_loaded(name));

        let manifest2 = create_test_manifest(name);
        let plugin2 = Box::new(IntegrationTestPlugin::new(name));
        manager.load(manifest2, plugin2).unwrap();
        assert!(manager.is_loaded(name));

        manager.unload(name, Default::default());
    }

    #[test]
    fn test_dependency_order_unload() {
        let manager = create_test_manager();

        let manifest_a = create_test_manifest("plugin-a");
        let mut manifest_b = create_test_manifest("plugin-b");

        manifest_b
            .dependencies
            .push(code_terminal::plugin::manifest::PluginDependency::required(
                "plugin-a", "1.0.0",
            ));

        let plugin_a = Box::new(IntegrationTestPlugin::new("plugin-a"));
        let plugin_b = Box::new(IntegrationTestPlugin::new("plugin-b"));

        manager.load(manifest_a, plugin_a).unwrap();
        manager.load(manifest_b, plugin_b).unwrap();

        let dependents = manager.get_dependents("plugin-a");
        assert!(dependents.contains(&"plugin-b".to_string()));

        manager.unload_all(Default::default());
    }
}

mod sandbox_integration {
    use super::*;

    #[test]
    fn test_sandbox_memory_limits() {
        let config = SandboxConfigBuilder::new().max_memory(1024 * 1024).build();

        let mut sandbox = Sandbox::new(config);

        assert!(sandbox.allocate_memory(512 * 1024).is_ok());
        assert!(sandbox.allocate_memory(512 * 1024).is_ok());
        assert!(sandbox.allocate_memory(1024).is_err());
    }

    #[test]
    fn test_sandbox_file_descriptors() {
        let config = SandboxConfigBuilder::new().max_open_files(3).build();

        let mut sandbox = Sandbox::new(config);

        assert!(sandbox.open_file().is_ok());
        assert!(sandbox.open_file().is_ok());
        assert!(sandbox.open_file().is_ok());
        assert!(sandbox.open_file().is_err());

        sandbox.close_file();
        assert!(sandbox.open_file().is_ok());
    }

    #[test]
    fn test_sandbox_fuel_consumption() {
        let config = SandboxConfigBuilder::new().max_fuel(1000).build();

        let mut sandbox = Sandbox::new(config);

        assert!(sandbox.consume_fuel(500).is_ok());
        assert_eq!(sandbox.remaining_fuel(), 500);

        assert!(sandbox.consume_fuel(400).is_ok());
        assert_eq!(sandbox.remaining_fuel(), 100);

        assert!(sandbox.consume_fuel(200).is_err());
        assert_eq!(sandbox.remaining_fuel(), 100);
    }

    #[test]
    fn test_sandbox_network_permissions() {
        let config = SandboxConfigBuilder::new()
            .allow(Permission::Network)
            .allow_host("api.example.com")
            .block_host("localhost")
            .build();

        assert!(config.check_network_access(Some("api.example.com")).is_ok());
        assert!(config.check_network_access(Some("localhost")).is_err());
        assert!(config.check_network_access(Some("other.com")).is_err());
    }

    #[test]
    fn test_sandbox_operations() {
        let config = SandboxConfigBuilder::new()
            .allow(Permission::Filesystem)
            .base_dir(std::env::temp_dir())
            .build();

        let sandbox = Sandbox::new(config);

        let allowed_path = std::env::temp_dir().join("test.txt");
        assert!(sandbox
            .check(SandboxOperation::ReadFile(&allowed_path))
            .is_ok());

        let blocked_path = PathBuf::from("/etc/passwd");
        assert!(sandbox
            .check(SandboxOperation::ReadFile(&blocked_path))
            .is_err());
    }

    #[test]
    fn test_sandbox_reset_and_shutdown() {
        let config = SandboxConfigBuilder::new()
            .max_memory(1024 * 1024)
            .max_fuel(1000)
            .build();

        let mut sandbox = Sandbox::new(config);

        sandbox.allocate_memory(512 * 1024).unwrap();
        sandbox.consume_fuel(500).unwrap();

        assert_eq!(sandbox.resource_usage().memory_used, 512 * 1024);
        assert_eq!(sandbox.resource_usage().fuel_consumed, 500);

        sandbox.reset();

        assert_eq!(sandbox.resource_usage().memory_used, 0);
        assert_eq!(sandbox.resource_usage().fuel_consumed, 0);

        sandbox.allocate_memory(256 * 1024).unwrap();
        sandbox.shutdown();

        assert_eq!(sandbox.resource_usage().memory_used, 0);
    }
}

mod api_versioning {
    use super::*;

    #[test]
    fn test_version_compatibility_matrix() {
        let test_cases = vec![
            ((1, 0, 0), (1, 0, 0), true),
            ((1, 5, 0), (1, 0, 0), true),
            ((1, 0, 0), (1, 5, 0), false),
            ((2, 0, 0), (1, 0, 0), false),
            ((1, 0, 0), (2, 0, 0), false),
            ((1, 2, 3), (1, 2, 3), true),
        ];

        for ((h_major, h_minor, h_patch), (p_major, p_minor, p_patch), expected) in test_cases {
            let host = PluginApiVersion::new(h_major, h_minor, h_patch);
            let plugin = PluginApiVersion::new(p_major, p_minor, p_patch);
            assert_eq!(
                host.is_compatible_with(&plugin),
                expected,
                "Host {} should {} be compatible with plugin {}",
                host,
                if expected { "" } else { "not" },
                plugin
            );
        }
    }

    #[test]
    fn test_api_capabilities() {
        let api = PluginApi::current();

        assert!(api.has_capability(code_terminal::plugin::api::ApiCapability::HotReload));
        assert!(api.has_capability(code_terminal::plugin::api::ApiCapability::Async));
        assert!(api.has_capability(code_terminal::plugin::api::ApiCapability::CustomKeybindings));
    }

    #[test]
    fn test_check_compatibility_function() {
        let host = PluginApiVersion::new(1, 5, 0);
        let plugin = PluginApiVersion::new(1, 3, 0);

        let result = check_compatibility(&host, &plugin);
        assert!(result.is_compatible());

        let host_old = PluginApiVersion::new(1, 0, 0);
        let plugin_new = PluginApiVersion::new(1, 5, 0);

        let result = check_compatibility(&host_old, &plugin_new);
        assert!(!result.is_compatible());
    }
}

mod manifest_integration {
    use super::*;

    #[test]
    fn test_manifest_roundtrip() {
        let original = PluginManifest::new("test-plugin", "1.2.3", "plugin.wasm");

        let toml_str = original.to_string_pretty().unwrap();
        let parsed = PluginManifest::from_str(&toml_str).unwrap();

        assert_eq!(original.plugin.name, parsed.plugin.name);
        assert_eq!(original.plugin.version, parsed.plugin.version);
        assert_eq!(original.entry, parsed.entry);
    }

    #[test]
    fn test_manifest_with_permissions() {
        let mut manifest = PluginManifest::new("secure-plugin", "1.0.0", "plugin.wasm");
        manifest.permissions = vec![
            Permission::Filesystem,
            Permission::Network,
            Permission::Notifications,
        ];

        let toml_str = manifest.to_string_pretty().unwrap();
        assert!(toml_str.contains("filesystem"));
        assert!(toml_str.contains("network"));
        assert!(toml_str.contains("notifications"));

        let parsed = PluginManifest::from_str(&toml_str).unwrap();
        assert_eq!(parsed.permissions.len(), 3);
    }

    #[test]
    fn test_manifest_with_dependencies() {
        let mut manifest = PluginManifest::new("dependent-plugin", "1.0.0", "plugin.wasm");
        manifest
            .dependencies
            .push(code_terminal::plugin::manifest::PluginDependency::required(
                "base-plugin",
                "^2.0.0",
            ));
        manifest
            .dependencies
            .push(code_terminal::plugin::manifest::PluginDependency::optional(
                "optional-plugin",
                ">=1.0.0",
            ));

        manifest.validate().unwrap();
        assert_eq!(manifest.dependencies.len(), 2);
    }
}

mod lifecycle_integration {
    use super::*;

    #[test]
    fn test_lifecycle_state_machine() {
        let lifecycle = PluginLifecycle::new();

        assert_eq!(lifecycle.get(), PluginState::Created);

        lifecycle.transition(PluginState::Initializing).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Initializing);

        lifecycle.transition(PluginState::Running).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Running);

        lifecycle.transition(PluginState::Paused).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Paused);

        lifecycle.transition(PluginState::Running).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Running);

        lifecycle.transition(PluginState::Stopping).unwrap();
        lifecycle.transition(PluginState::Stopped).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Stopped);
    }

    #[test]
    fn test_lifecycle_error_recovery() {
        let lifecycle = PluginLifecycle::new();

        lifecycle.transition(PluginState::Initializing).unwrap();
        lifecycle.transition(PluginState::Running).unwrap();
        lifecycle.set_error();

        assert_eq!(lifecycle.get(), PluginState::Error);

        lifecycle.transition(PluginState::Stopped).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Stopped);
    }

    #[test]
    fn test_lifecycle_invalid_transitions() {
        let lifecycle = PluginLifecycle::new();

        assert!(lifecycle.transition(PluginState::Running).is_err());
        assert!(lifecycle.transition(PluginState::Paused).is_err());
        assert!(lifecycle.transition(PluginState::Stopped).is_err());

        lifecycle.transition(PluginState::Initializing).unwrap();
        assert!(lifecycle.transition(PluginState::Paused).is_err());
    }
}

mod error_handling {
    use super::*;

    #[test]
    fn test_error_constructors() {
        let err = PluginError::not_found("test");
        assert!(err.plugin_name().is_some());

        let err = PluginError::already_loaded("test");
        assert!(err.plugin_name().is_some());

        let err = PluginError::sandbox_violation("op", "reason");
        assert!(err.is_recoverable());

        let err = PluginError::Timeout {
            operation: "init".to_string(),
            duration_ms: 5000,
        };
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_error_display() {
        let err = PluginError::not_found("my-plugin");
        let msg = err.to_string();
        assert!(msg.contains("my-plugin"));

        let err = PluginError::sandbox_violation("file_read", "access denied");
        let msg = err.to_string();
        assert!(msg.contains("sandbox"));
        assert!(msg.contains("file_read"));
    }
}

mod context_integration {
    use super::*;

    fn create_test_context() -> PluginContext {
        let api = PluginApi::current();
        PluginContext::new(
            api,
            1,
            "test-plugin",
            PathBuf::from("/tmp/data"),
            PathBuf::from("/tmp/config"),
        )
    }

    #[test]
    fn test_context_capabilities() {
        let mut ctx = create_test_context();

        assert!(!ctx.has_capability(code_terminal::plugin::api::ApiCapability::Notifications));

        ctx.enable_capability(code_terminal::plugin::api::ApiCapability::Notifications);
        assert!(ctx.has_capability(code_terminal::plugin::api::ApiCapability::Notifications));

        ctx.disable_capability(code_terminal::plugin::api::ApiCapability::Notifications);
        assert!(!ctx.has_capability(code_terminal::plugin::api::ApiCapability::Notifications));
    }

    #[test]
    fn test_context_config() {
        let mut ctx = create_test_context();

        ctx.set_config("key1", "value1");
        ctx.set_config("key2", "value2");

        assert_eq!(ctx.get_config("key1"), Some(&"value1".to_string()));
        assert_eq!(ctx.get_config("key2"), Some(&"value2".to_string()));
        assert_eq!(ctx.get_config("key3"), None);
    }

    #[test]
    fn test_context_sandbox_enforcement() {
        let ctx = create_test_context();

        let result = ctx.read_file("/etc/passwd");
        assert!(result.is_err());

        let result = ctx.write_file("/tmp/test.txt", b"data");
        assert!(result.is_err());
    }
}

mod performance {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_plugin_load_performance() {
        let manager = create_test_manager();
        let start = Instant::now();

        for i in 0..10 {
            let name = format!("perf-plugin-{}", i);
            let manifest = create_test_manifest(&name);
            let plugin = Box::new(IntegrationTestPlugin::new(&name));
            manager.load(manifest, plugin).unwrap();
        }

        let elapsed = start.elapsed();
        println!("Load 10 plugins: {:?}", elapsed);

        assert!(elapsed < Duration::from_millis(1000));
    }

    #[test]
    fn test_sandbox_check_performance() {
        let config = SandboxConfig::moderate();
        let sandbox = Sandbox::new(config);

        let start = Instant::now();

        for _ in 0..1000 {
            let _ = sandbox.check(SandboxOperation::MemoryAllocation(1024));
        }

        let elapsed = start.elapsed();
        println!("1000 sandbox checks: {:?}", elapsed);

        assert!(elapsed < Duration::from_millis(100));
    }
}
