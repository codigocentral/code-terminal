//! Testes de Integração - Sistema de Plugins
//!
//! Testes que cobrem múltiplos componentes do sistema de plugins trabalhando juntos.

use code_terminal::plugin::api::{ApiCapability, PluginApi, PluginApiVersion};
use code_terminal::plugin::error::PluginError;
use code_terminal::plugin::lifecycle::{PluginLifecycle, PluginState};
use code_terminal::plugin::manifest::{
    ApiRequirement, Permission, PluginDependency, PluginManifest, PluginMetadata,
};
use code_terminal::plugin::sandbox::{Sandbox, SandboxConfigBuilder, SandboxOperation};
use code_terminal::plugin::{check_compatibility, Plugin, PluginContext, PluginInfo};
use code_terminal::plugin::{WasmPluginInstance, WasmRuntime};
use std::collections::HashMap;
use std::sync::Arc;

mod integration {
    use super::*;

    #[derive(Debug)]
    struct MockPlugin {
        name: String,
        version: String,
        state: PluginState,
    }

    impl MockPlugin {
        fn new(name: &str, version: &str) -> Self {
            Self {
                name: name.to_string(),
                version: version.to_string(),
                state: PluginState::Created,
            }
        }
    }

    impl Plugin for MockPlugin {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: self.name.clone(),
                version: self.version.clone(),
                author: "Test Author".to_string(),
                description: "Mock plugin for testing".to_string(),
                homepage: None,
                repository: None,
            }
        }

        fn init(
            &mut self,
            _ctx: &mut PluginContext,
        ) -> code_terminal::plugin::error::PluginResult<()> {
            self.state = PluginState::Running;
            Ok(())
        }

        fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {}

        fn shutdown(&mut self, _ctx: &mut PluginContext) {
            self.state = PluginState::Stopped;
        }

        fn state(&self) -> PluginState {
            self.state
        }
    }

    #[test]
    fn test_plugin_full_lifecycle() {
        let plugin = MockPlugin::new("test-plugin", "1.0.0");
        let lifecycle = PluginLifecycle::new();

        assert_eq!(plugin.state(), PluginState::Created);
        assert_eq!(lifecycle.get(), PluginState::Created);

        lifecycle.transition(PluginState::Initializing).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Initializing);

        lifecycle.transition(PluginState::Running).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Running);

        lifecycle.transition(PluginState::Stopping).unwrap();
        lifecycle.transition(PluginState::Stopped).unwrap();
        assert_eq!(lifecycle.get(), PluginState::Stopped);
    }

    #[test]
    fn test_manifest_validation_and_api_compatibility() {
        let manifest = PluginManifest::new("test", "1.0.0", "plugin.wasm");
        assert!(manifest.validate().is_ok());

        let api_version = PluginApiVersion::CURRENT;
        let manifest_version = PluginApiVersion::new(1, 0, 0);
        assert!(api_version.is_compatible_with(&manifest_version));

        let result = check_compatibility(&api_version, &manifest_version);
        assert!(result.is_compatible());
    }

    #[test]
    fn test_sandbox_permission_integration() {
        let config = SandboxConfigBuilder::new()
            .allow(Permission::Filesystem)
            .allow(Permission::Notifications)
            .build();

        assert!(config.has_permission(Permission::Filesystem));
        assert!(config.has_permission(Permission::Notifications));
        assert!(!config.has_permission(Permission::Network));
        assert!(!config.has_permission(Permission::Shell));
    }

    #[test]
    fn test_sandbox_operations_with_permissions() {
        let config = SandboxConfigBuilder::new()
            .allow(Permission::Network)
            .build();

        let sandbox = Sandbox::new(config);

        let network_op = SandboxOperation::NetworkRequest {
            host: Some("example.com"),
        };
        assert!(sandbox.check(network_op).is_ok());

        let shell_op = SandboxOperation::ShellCommand;
        assert!(sandbox.check(shell_op).is_err());
    }

    #[test]
    fn test_api_capabilities_integration() {
        let api = PluginApi::current();

        assert!(api.has_capability(ApiCapability::HotReload));
        assert!(api.has_capability(ApiCapability::CustomKeybindings));
        assert!(!api.has_capability(ApiCapability::Network));

        let minimal = PluginApi::minimal();
        assert!(!minimal.has_capability(ApiCapability::HotReload));
        assert!(minimal.has_capability(ApiCapability::CustomKeybindings));
    }

    #[test]
    fn test_plugin_dependencies_resolution() {
        let dep1 = PluginDependency::required("base-plugin", "^1.0.0");
        let dep2 = PluginDependency::optional("extra-plugin", ">=2.0.0");

        assert!(!dep1.optional);
        assert!(dep2.optional);

        let manifest = PluginManifest {
            plugin: PluginMetadata {
                name: "main-plugin".to_string(),
                version: "1.0.0".to_string(),
                description: String::new(),
                author: String::new(),
                homepage: None,
                repository: None,
                license: None,
                keywords: Vec::new(),
            },
            api: ApiRequirement::default(),
            dependencies: vec![dep1, dep2],
            config: std::collections::HashMap::new(),
            permissions: vec![Permission::Notifications, Permission::CustomUi],
            scripts: Default::default(),
            entry: "plugin.wasm".to_string(),
        };

        assert_eq!(manifest.dependencies.len(), 2);
        assert_eq!(manifest.permissions.len(), 2);
    }

    #[test]
    fn test_error_handling_integration() {
        let err = PluginError::not_found("missing-plugin");
        assert!(err.is_fatal());
        assert!(!err.is_recoverable());

        let err = PluginError::sandbox_violation("network", "not allowed");
        assert!(!err.is_fatal());
        assert!(err.is_recoverable());

        let err = PluginError::Timeout {
            operation: "init".to_string(),
            duration_ms: 5000,
        };
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_lifecycle_state_transitions() {
        // Testa transição válida: Created -> Initializing -> Running
        let lifecycle = PluginLifecycle::new();
        assert_eq!(lifecycle.get(), PluginState::Created);

        assert!(lifecycle.transition(PluginState::Initializing).is_ok());
        assert_eq!(lifecycle.get(), PluginState::Initializing);

        assert!(lifecycle.transition(PluginState::Running).is_ok());
        assert_eq!(lifecycle.get(), PluginState::Running);

        // Testa transição válida: Running -> Paused
        assert!(lifecycle.transition(PluginState::Paused).is_ok());
        assert_eq!(lifecycle.get(), PluginState::Paused);

        // Testa transição válida: Paused -> Running
        assert!(lifecycle.transition(PluginState::Running).is_ok());
        assert_eq!(lifecycle.get(), PluginState::Running);

        // Testa transição válida: Running -> Stopping -> Stopped
        assert!(lifecycle.transition(PluginState::Stopping).is_ok());
        assert_eq!(lifecycle.get(), PluginState::Stopping);

        assert!(lifecycle.transition(PluginState::Stopped).is_ok());
        assert_eq!(lifecycle.get(), PluginState::Stopped);

        // Testa transições inválidas
        let lifecycle2 = PluginLifecycle::new();
        // Created -> Running é inválido
        assert!(lifecycle2.transition(PluginState::Running).is_err());
    }

    #[test]
    fn test_version_parsing_and_comparison() {
        // (host_version, plugin_required_version, expected_compatible)
        let versions = vec![
            // Mesma versão - compatível
            ("1.0.0", "1.0.0", true),
            // Host mais novo que plugin - compatível
            ("1.1.0", "1.0.0", true),
            ("1.5.0", "1.3.0", true),
            // Host mais antigo que plugin - incompatível
            ("1.0.0", "1.1.0", false),
            // Major diferente - incompatível
            ("1.0.0", "2.0.0", false),
            ("2.0.0", "1.0.0", false),
        ];

        for (host_ver, plugin_ver, compatible) in versions {
            let host = PluginApiVersion::parse(host_ver).unwrap();
            let plugin = PluginApiVersion::parse(plugin_ver).unwrap();
            assert_eq!(
                host.is_compatible_with(&plugin),
                compatible,
                "Host {} should {} be compatible with plugin {}",
                host_ver,
                if compatible { "" } else { "not" },
                plugin_ver
            );
        }
    }

    #[test]
    fn test_permission_danger_level() {
        let dangerous = vec![
            Permission::Shell,
            Permission::Filesystem,
            Permission::Network,
        ];
        let safe = vec![
            Permission::Clipboard,
            Permission::ReadConfig,
            Permission::Notifications,
            Permission::CustomUi,
        ];

        for perm in dangerous {
            assert!(perm.is_dangerous(), "{:?} should be dangerous", perm);
        }

        for perm in safe {
            assert!(!perm.is_dangerous(), "{:?} should be safe", perm);
        }
    }

    #[test]
    fn test_sandbox_resource_tracking() {
        let config = SandboxConfigBuilder::new().max_memory(1024 * 1024).build();

        let mut sandbox = Sandbox::new(config);

        assert!(sandbox.allocate_memory(512 * 1024).is_ok());
        assert!(sandbox.allocate_memory(512 * 1024).is_ok());
        assert!(sandbox.allocate_memory(1).is_err());

        sandbox.free_memory(512 * 1024);
        assert!(sandbox.allocate_memory(1).is_ok());
    }
}

mod context_integration {
    use super::*;

    fn create_test_context() -> PluginContext {
        PluginContext::new(
            PluginApi::current(),
            1,
            "test-plugin",
            std::path::PathBuf::from("/tmp/data"),
            std::path::PathBuf::from("/tmp/config"),
        )
    }

    #[test]
    fn test_context_capability_enforcement() {
        let mut ctx = create_test_context();

        // Por padrão, nenhuma capability está habilitada
        assert!(!ctx.has_capability(ApiCapability::HotReload));
        assert!(!ctx.has_capability(ApiCapability::Network));

        // Habilita uma capability
        ctx.enable_capability(ApiCapability::HotReload);
        assert!(ctx.has_capability(ApiCapability::HotReload));
    }

    #[test]
    fn test_context_config_management() {
        let mut ctx = create_test_context();

        ctx.set_config("setting1", "value1");
        ctx.set_config("setting2", "value2");

        assert_eq!(ctx.get_config("setting1"), Some(&"value1".to_string()));
        assert_eq!(ctx.get_config("setting2"), Some(&"value2".to_string()));
        assert_eq!(ctx.get_config("nonexistent"), None);
    }

    #[test]
    fn test_context_sandbox_restrictions() {
        let ctx = create_test_context();

        let result = ctx.read_file("/etc/passwd");
        assert!(result.is_err());

        let result = ctx.notify("Test", "Message");
        assert!(result.is_err());
    }
}

mod manifest_integration {
    use super::*;

    #[test]
    fn test_manifest_toml_roundtrip() {
        let original = PluginManifest {
            plugin: PluginMetadata {
                name: "test-plugin".to_string(),
                version: "1.2.3".to_string(),
                description: "A test plugin".to_string(),
                author: "Test Author".to_string(),
                homepage: Some("https://example.com".to_string()),
                repository: Some("https://github.com/example/plugin".to_string()),
                license: Some("MIT".to_string()),
                keywords: vec!["test".to_string(), "plugin".to_string()],
            },
            api: ApiRequirement::default(),
            dependencies: vec![PluginDependency::required("dep1", "^1.0.0")],
            config: std::collections::HashMap::new(),
            permissions: vec![Permission::Notifications],
            scripts: Default::default(),
            entry: "plugin.wasm".to_string(),
        };

        let toml_str = original.to_string_pretty().unwrap();
        let parsed = PluginManifest::from_str(&toml_str).unwrap();

        assert_eq!(original.plugin.name, parsed.plugin.name);
        assert_eq!(original.plugin.version, parsed.plugin.version);
        assert_eq!(original.entry, parsed.entry);
    }

    #[test]
    fn test_manifest_validation_errors() {
        let manifest = PluginManifest::new("", "1.0.0", "plugin.wasm");
        assert!(manifest.validate().is_err());

        let manifest = PluginManifest::new("test", "invalid", "plugin.wasm");
        assert!(manifest.validate().is_err());

        let manifest = PluginManifest::new("test", "1.0.0", "");
        assert!(manifest.validate().is_err());
    }
}

mod wasm_integration {
    use super::*;

    fn create_real_wasm_plugin() -> Vec<u8> {
        wat::parse_str(
            r#"
            (module
              (memory (export "memory") 1)
              (global $heap (mut i32) (i32.const 2048))
              (global $init_len (mut i32) (i32.const 0))
              (global $input_len (mut i32) (i32.const 0))
              (global $theme_len (mut i32) (i32.const 0))
              (global $config_len (mut i32) (i32.const 0))
              (global $updates (mut i32) (i32.const 0))

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

              (func (export "update") (param $delta i64)
                global.get $updates
                i32.const 1
                i32.add
                global.set $updates)

              (func (export "shutdown"))

              (func (export "on_input") (param $ptr i32) (param $len i32) (result i32)
                local.get $len
                global.set $input_len
                local.get $len
                i32.const 5
                i32.eq
                local.get $ptr
                i32.load8_u
                i32.const 47
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

              (func (export "get_input_len") (result i32)
                global.get $input_len)

              (func (export "get_theme_len") (result i32)
                global.get $theme_len)

              (func (export "get_config_len") (result i32)
                global.get $config_len)

              (func (export "get_updates") (result i32)
                global.get $updates))
            "#,
        )
        .unwrap()
    }

    fn create_context() -> PluginContext {
        PluginContext::new(
            PluginApi::current(),
            42,
            "real-wasm-plugin",
            std::env::temp_dir().join("real-wasm-plugin-data"),
            std::env::temp_dir().join("real-wasm-plugin-config"),
        )
    }

    #[test]
    fn test_real_wasm_plugin_lifecycle_and_payloads() {
        let runtime = Arc::new(WasmRuntime::new().unwrap());
        let wasm = create_real_wasm_plugin();
        let module = runtime.compile_bytes(&wasm).unwrap();
        let manifest = PluginManifest::new("real-wasm-plugin", "1.0.0", "plugin.wasm");
        let mut plugin = WasmPluginInstance::new("real-wasm-plugin", module, manifest, None);
        let mut context = create_context();

        plugin.instantiate(&runtime).unwrap();
        plugin.init(&mut context).unwrap();
        plugin.update(&mut context, 16);
        assert!(plugin.on_input(&mut context, "/test"));
        plugin.on_theme_changed(&mut context, "dark");

        let mut config = HashMap::new();
        config.insert("enabled".to_string(), "true".to_string());
        plugin.apply_config(&config);

        assert_eq!(plugin.state(), PluginState::Running);
        assert!(plugin.call::<(), i32>("get_init_len", ()).unwrap() > 0);
        assert_eq!(plugin.call::<(), i32>("get_input_len", ()).unwrap(), 5);
        assert_eq!(plugin.call::<(), i32>("get_theme_len", ()).unwrap(), 4);
        assert!(plugin.call::<(), i32>("get_config_len", ()).unwrap() > 0);
        assert_eq!(plugin.call::<(), i32>("get_updates", ()).unwrap(), 1);

        Plugin::shutdown(&mut plugin, &mut context);
        assert_eq!(plugin.state(), PluginState::Stopped);
    }
}
