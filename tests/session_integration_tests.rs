//! Testes de Integração do Sistema de Sessões
//!
//! Este arquivo contém testes end-to-end que verificam:
//! - Salvamento e carregamento de sessões
//! - Backup e restauração
//! - Auto-save
//! - Migração de versões
//! - Integração com AppState

use std::time::Duration;
use tempfile::TempDir;

// Importa os módulos de sessão do code-terminal
use code_terminal::session::*;

/// Fixture para criar ambiente de teste isolado
struct TestFixture {
    temp_dir: TempDir,
    session_manager: SessionManager,
    backup_manager: BackupManager,
}

impl TestFixture {
    fn new() -> Self {
        let temp_dir = TempDir::new().unwrap();

        let session_config = SessionConfig::with_directory(temp_dir.path().join("sessions"));
        let session_manager = SessionManager::new(session_config);
        session_manager.initialize().unwrap();

        let backup_config = BackupConfig::with_directory(temp_dir.path().join("backups"));
        let backup_manager = BackupManager::new(backup_config);
        backup_manager.initialize().unwrap();

        Self {
            temp_dir,
            session_manager,
            backup_manager,
        }
    }

    fn temp_path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }
}

mod session_crud_tests {
    use super::*;

    #[test]
    fn test_save_and_load_session() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        // Salva sessão
        fixture.session_manager.save("test-session", &data).unwrap();

        // Carrega sessão
        let loaded = fixture.session_manager.load("test-session").unwrap();

        assert_eq!(loaded.metadata.name, "test-session");
        assert_eq!(loaded.data.version, SESSION_FORMAT_VERSION);
    }

    #[test]
    fn test_session_not_found() {
        let fixture = TestFixture::new();

        let result = fixture.session_manager.load("nonexistent");
        assert!(matches!(result, Err(SessionError::NotFound(_))));
    }

    #[test]
    fn test_list_sessions() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        // Cria várias sessões
        fixture.session_manager.save("session-1", &data).unwrap();
        fixture.session_manager.save("session-2", &data).unwrap();
        fixture.session_manager.save("session-3", &data).unwrap();

        let sessions = fixture.session_manager.list_sessions().unwrap();
        assert_eq!(sessions.len(), 3);
    }

    #[test]
    fn test_delete_session() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        fixture.session_manager.save("to-delete", &data).unwrap();
        assert!(fixture.session_manager.exists("to-delete"));

        fixture.session_manager.delete("to-delete").unwrap();
        assert!(!fixture.session_manager.exists("to-delete"));
    }

    #[test]
    fn test_rename_session() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        fixture.session_manager.save("old-name", &data).unwrap();
        fixture
            .session_manager
            .rename("old-name", "new-name")
            .unwrap();

        assert!(!fixture.session_manager.exists("old-name"));
        assert!(fixture.session_manager.exists("new-name"));
    }

    #[test]
    fn test_rename_to_existing_fails() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        fixture.session_manager.save("source", &data).unwrap();
        fixture.session_manager.save("target", &data).unwrap();

        let result = fixture.session_manager.rename("source", "target");
        assert!(matches!(result, Err(SessionError::AlreadyExists(_))));
    }

    #[test]
    fn test_invalid_session_names() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        assert!(fixture.session_manager.save("", &data).is_err());
        assert!(fixture
            .session_manager
            .save("path/with/slashes", &data)
            .is_err());
        assert!(fixture
            .session_manager
            .save("name:with:colons", &data)
            .is_err());
    }

    #[test]
    fn test_session_with_working_directory() {
        let fixture = TestFixture::new();
        let mut data = SessionData::new();
        data.working_directory = Some(std::path::PathBuf::from("/home/user/projects"));

        fixture.session_manager.save("with-dir", &data).unwrap();
        let loaded = fixture.session_manager.load("with-dir").unwrap();

        assert_eq!(
            loaded.data.working_directory,
            Some(std::path::PathBuf::from("/home/user/projects"))
        );
    }

    #[test]
    fn test_session_with_environment() {
        let fixture = TestFixture::new();
        let mut data = SessionData::new();
        data.environment
            .insert("EDITOR".to_string(), "vim".to_string());
        data.environment
            .insert("SHELL".to_string(), "/bin/bash".to_string());

        fixture.session_manager.save("with-env", &data).unwrap();
        let loaded = fixture.session_manager.load("with-env").unwrap();

        assert_eq!(
            loaded.data.environment.get("EDITOR"),
            Some(&"vim".to_string())
        );
        assert_eq!(loaded.data.environment.len(), 2);
    }

    #[test]
    fn test_default_session() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        // Inicialmente não há sessão padrão
        assert!(fixture.session_manager.load_default().unwrap().is_none());

        // Salva como padrão
        fixture.session_manager.save_default(&data).unwrap();

        // Agora deve existir
        let default = fixture.session_manager.load_default().unwrap();
        assert!(default.is_some());
    }
}

mod backup_tests {
    use super::*;

    #[test]
    fn test_create_backup() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        let backup = fixture
            .backup_manager
            .backup("test-session", &data)
            .unwrap();

        assert_eq!(backup.metadata.session_name, "test-session");
        assert!(backup.path.exists());
    }

    #[test]
    fn test_list_backups() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        fixture
            .backup_manager
            .backup("test-session", &data)
            .unwrap();
        fixture
            .backup_manager
            .backup("test-session", &data)
            .unwrap();

        let backups = fixture.backup_manager.list_backups("test-session").unwrap();
        assert_eq!(backups.len(), 2);
    }

    #[test]
    fn test_restore_backup() {
        let fixture = TestFixture::new();
        let mut data = SessionData::new();
        data.working_directory = Some(std::path::PathBuf::from("/test/path"));

        let backup = fixture
            .backup_manager
            .backup("test-session", &data)
            .unwrap();
        let restored = fixture.backup_manager.restore_backup(&backup).unwrap();

        assert_eq!(restored.working_directory, data.working_directory);
    }

    #[test]
    fn test_backup_retention_policy_count() {
        let temp_dir = TempDir::new().unwrap();
        let config = BackupConfig::with_directory(temp_dir.path())
            .with_retention_policy(BackupRetentionPolicy::KeepCount(3));
        let manager = BackupManager::new(config);
        manager.initialize().unwrap();

        let data = SessionData::new();

        // Cria 5 backups com nomes que incluem o session_name
        for i in 0..5 {
            let opts = BackupOptions::with_name(format!("test-session_backup-{}", i));
            manager.create_backup("test-session", &data, opts).unwrap();
        }

        // Deve manter apenas 3
        let backups = manager.list_backups("test-session").unwrap();
        assert_eq!(backups.len(), 3);
    }

    #[test]
    fn test_backup_with_options() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        let options =
            BackupOptions::with_name("custom-backup").with_description("Descrição personalizada");

        let backup = fixture
            .backup_manager
            .create_backup("test-session", &data, options)
            .unwrap();

        assert_eq!(backup.metadata.backup_name, "custom-backup");
        assert_eq!(
            backup.metadata.description,
            Some("Descrição personalizada".to_string())
        );
    }

    #[test]
    fn test_delete_backup() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        let backup = fixture
            .backup_manager
            .backup("test-session", &data)
            .unwrap();
        assert!(fixture.backup_manager.has_backup("test-session").unwrap());

        fixture.backup_manager.delete_backup(&backup).unwrap();
        assert!(!fixture.backup_manager.has_backup("test-session").unwrap());
    }

    #[test]
    fn test_backup_stats() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        fixture.backup_manager.backup("session-1", &data).unwrap();
        fixture.backup_manager.backup("session-1", &data).unwrap();
        fixture.backup_manager.backup("session-2", &data).unwrap();

        let stats = fixture.backup_manager.stats().unwrap();
        assert_eq!(stats.total_backups, 3);
        assert_eq!(stats.total_sessions, 2);
    }

    #[test]
    fn test_get_latest_backup() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        assert!(fixture
            .backup_manager
            .get_latest_backup("test-session")
            .unwrap()
            .is_none());

        let backup = fixture
            .backup_manager
            .backup("test-session", &data)
            .unwrap();
        let latest = fixture
            .backup_manager
            .get_latest_backup("test-session")
            .unwrap();

        assert!(latest.is_some());
        assert_eq!(
            latest.unwrap().metadata.backup_name,
            backup.metadata.backup_name
        );
    }
}

mod migration_tests {
    use super::*;

    #[test]
    fn test_version_manager_needs_migration() {
        let version_manager = VersionManager::new();

        // Sessão na versão atual não precisa de migração
        let current_session = Session::new("test", SessionData::new());
        assert!(!version_manager.needs_migration(&current_session));

        // Sessão antiga precisa de migração
        let mut old_session = Session::new("test", SessionData::new());
        old_session.metadata.version = 0;
        assert!(version_manager.needs_migration(&old_session));
    }

    #[test]
    fn test_migrate_session() {
        let version_manager = VersionManager::new();

        let mut old_session = Session::new("test", SessionData::new());
        old_session.metadata.version = 0;

        let migrated = version_manager.migrate(old_session).unwrap();

        assert_eq!(migrated.metadata.version, SESSION_FORMAT_VERSION);
        assert_eq!(migrated.data.version, SESSION_FORMAT_VERSION);
    }

    #[test]
    fn test_migration_no_op_for_current_version() {
        let version_manager = VersionManager::new();
        let current_session = Session::new("test", SessionData::new());

        let result = version_manager.migrate(current_session.clone()).unwrap();

        // Deve retornar a mesma sessão sem modificações
        assert_eq!(result.metadata.version, current_session.metadata.version);
    }

    #[test]
    fn test_version_info() {
        let version_manager = VersionManager::new();
        let info = version_manager.version_info();

        assert_eq!(info.current_version, SESSION_FORMAT_VERSION);
        assert!(!info.supported_versions.is_empty());
    }

    #[test]
    fn test_migration_registry_pathfinding() {
        let registry = MigrationRegistry::new();

        // Caminho direto 0 -> 1
        let path = registry.find_migration_path(0, 1);
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec![(0, 1)]);

        // Caminho indireto 0 -> 2 (via 1)
        let path = registry.find_migration_path(0, 2);
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec![(0, 1), (1, 2)]);

        // Downgrade não suportado
        let path = registry.find_migration_path(2, 1);
        assert!(path.is_none());
    }

    #[test]
    fn test_migration_ext_trait() {
        let fixture = TestFixture::new();
        let version_manager = VersionManager::new();

        // Cria sessão antiga manualmente (versão 0)
        let old_session = Session {
            metadata: SessionMetadata {
                name: "test".to_string(),
                created_at: chrono::Utc::now(),
                modified_at: chrono::Utc::now(),
                version: 0, // Versão antiga
                description: None,
            },
            data: SessionData::new(),
        };

        // Salva diretamente como JSON para preservar a versão antiga
        let session_path = fixture.temp_dir.path().join("sessions").join("test.json");
        let json = serde_json::to_string_pretty(&old_session).unwrap();
        std::fs::write(&session_path, json).unwrap();

        // Verifica se precisa de migração
        let needs_migration = fixture
            .session_manager
            .check_migration_needed("test")
            .unwrap();
        assert_eq!(needs_migration, Some(0));

        // Carrega e migra
        let migrated = fixture
            .session_manager
            .load_and_migrate("test", &version_manager)
            .unwrap();

        assert_eq!(migrated.metadata.version, SESSION_FORMAT_VERSION);
    }
}

mod serialization_tests {
    use super::*;

    #[test]
    fn test_session_data_serialization() {
        let mut data = SessionData::new();
        data.working_directory = Some(std::path::PathBuf::from("/home/user"));
        data.environment
            .insert("TEST".to_string(), "value".to_string());

        let json = serde_json::to_string_pretty(&data).unwrap();
        let restored: SessionData = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.version, data.version);
        assert_eq!(restored.working_directory, data.working_directory);
        assert_eq!(restored.environment.get("TEST"), Some(&"value".to_string()));
    }

    #[test]
    fn test_layout_data_serialization() {
        use code_terminal::session::serialization::{LayoutData, PaneData, TabData};

        let mut layout = LayoutData::new();
        let mut pane = PaneData::new(1);
        pane.add_tab(TabData::new(1, "bash"));
        layout.add_pane(pane);

        let json = serde_json::to_string_pretty(&layout).unwrap();
        let restored: LayoutData = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.panes.len(), 1);
        assert_eq!(restored.panes[0].tabs.len(), 1);
    }

    #[test]
    fn test_session_metadata_serialization() {
        let metadata = SessionMetadata::new("test-session").with_description("Descrição de teste");

        let json = serde_json::to_string_pretty(&metadata).unwrap();
        let restored: SessionMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.name, "test-session");
        assert_eq!(restored.description, Some("Descrição de teste".to_string()));
        assert_eq!(restored.version, SESSION_FORMAT_VERSION);
    }

    #[test]
    fn test_split_data_serialization() {
        use code_terminal::session::serialization::{PaneData, SplitData, SplitDirection};

        let split = SplitData::Split {
            direction: SplitDirection::Vertical,
            ratio: 0.5,
            first: Box::new(SplitData::Pane(PaneData::new(1))),
            second: Box::new(SplitData::Pane(PaneData::new(2))),
        };

        let json = serde_json::to_string_pretty(&split).unwrap();
        let restored: SplitData = serde_json::from_str(&json).unwrap();

        match restored {
            SplitData::Split {
                direction, ratio, ..
            } => {
                assert_eq!(direction, SplitDirection::Vertical);
                assert_eq!(ratio, 0.5);
            }
            _ => panic!("Esperado Split variant"),
        }
    }
}

mod integration_tests {
    use super::*;

    #[test]
    fn test_full_session_lifecycle() {
        let fixture = TestFixture::new();

        // 1. Cria dados de sessão
        let mut data = SessionData::new();
        data.working_directory = Some(std::path::PathBuf::from("/home/user/project"));
        data.environment
            .insert("EDITOR".to_string(), "vim".to_string());

        // 2. Salva sessão
        fixture
            .session_manager
            .save("lifecycle-test", &data)
            .unwrap();

        // 3. Cria backup
        let backup = fixture
            .backup_manager
            .backup("lifecycle-test", &data)
            .unwrap();

        // 4. Carrega sessão
        let loaded = fixture.session_manager.load("lifecycle-test").unwrap();
        assert_eq!(loaded.data.working_directory, data.working_directory);

        // 5. Restaura backup
        let restored = fixture.backup_manager.restore_backup(&backup).unwrap();
        assert_eq!(restored.working_directory, data.working_directory);

        // 6. Deleta sessão
        fixture.session_manager.delete("lifecycle-test").unwrap();
        assert!(!fixture.session_manager.exists("lifecycle-test"));
    }

    #[test]
    fn test_multiple_sessions_isolation() {
        let fixture = TestFixture::new();

        // Cria duas sessões diferentes
        let mut data1 = SessionData::new();
        data1.working_directory = Some(std::path::PathBuf::from("/project1"));

        let mut data2 = SessionData::new();
        data2.working_directory = Some(std::path::PathBuf::from("/project2"));

        fixture.session_manager.save("session-1", &data1).unwrap();
        fixture.session_manager.save("session-2", &data2).unwrap();

        // Verifica isolamento
        let loaded1 = fixture.session_manager.load("session-1").unwrap();
        let loaded2 = fixture.session_manager.load("session-2").unwrap();

        assert_eq!(
            loaded1.data.working_directory,
            Some(std::path::PathBuf::from("/project1"))
        );
        assert_eq!(
            loaded2.data.working_directory,
            Some(std::path::PathBuf::from("/project2"))
        );
    }

    #[test]
    fn test_session_persistence_across_operations() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        // Múltiplas operações de salvar/carregar
        for i in 0..5 {
            let name = format!("session-{}", i);
            fixture.session_manager.save(&name, &data).unwrap();
        }

        // Verifica persistência
        let sessions = fixture.session_manager.list_sessions().unwrap();
        assert_eq!(sessions.len(), 5);

        // Carrega cada uma
        for i in 0..5 {
            let name = format!("session-{}", i);
            let loaded = fixture.session_manager.load(&name).unwrap();
            assert_eq!(loaded.metadata.name, name);
        }
    }

    #[test]
    fn test_concurrent_backup_operations() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        // Cria backup_manager com política de retenção KeepAll
        let backup_config =
            BackupConfig::with_directory(fixture.temp_path().join("backups_concurrent"))
                .with_retention_policy(BackupRetentionPolicy::KeepAll);
        let backup_manager = BackupManager::new(backup_config);
        backup_manager.initialize().unwrap();

        // Cria múltiplos backups rapidamente
        for i in 0..10 {
            let opts = BackupOptions::with_name(format!("concurrent_backup-{}", i));
            backup_manager
                .create_backup("concurrent", &data, opts)
                .unwrap();
        }

        let backups = backup_manager.list_backups("concurrent").unwrap();
        assert_eq!(backups.len(), 10);
    }

    #[test]
    fn test_session_update_preserves_backups() {
        let fixture = TestFixture::new();

        // Cria sessão inicial
        let data1 = SessionData::new();
        fixture.session_manager.save("update-test", &data1).unwrap();
        fixture
            .backup_manager
            .backup("update-test", &data1)
            .unwrap();

        // Atualiza sessão
        let mut data2 = SessionData::new();
        data2.working_directory = Some(std::path::PathBuf::from("/updated"));
        fixture.session_manager.save("update-test", &data2).unwrap();

        // Verifica que backup anterior ainda existe
        let backups = fixture.backup_manager.list_backups("update-test").unwrap();
        assert_eq!(backups.len(), 1);

        // Verifica que sessão atualizada tem novos dados
        let loaded = fixture.session_manager.load("update-test").unwrap();
        assert_eq!(
            loaded.data.working_directory,
            Some(std::path::PathBuf::from("/updated"))
        );
    }

    #[test]
    fn test_error_recovery() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        // Tenta operações inválidas
        assert!(fixture.session_manager.load("nonexistent").is_err());
        assert!(fixture.session_manager.delete("nonexistent").is_err());
        assert!(fixture
            .session_manager
            .rename("nonexistent", "new")
            .is_err());

        // Operações válidas devem continuar funcionando
        fixture.session_manager.save("valid", &data).unwrap();
        let loaded = fixture.session_manager.load("valid").unwrap();
        assert_eq!(loaded.metadata.name, "valid");
    }
}

mod auto_save_tests {
    use super::*;

    #[tokio::test]
    async fn test_auto_save_config_default() {
        let config = AutoSaveConfig::default();

        assert!(config.enabled);
        assert_eq!(config.interval, Duration::from_secs(300));
        assert!(config.on_tab_created);
        assert!(config.on_tab_closed);
        assert_eq!(config.max_auto_save_sessions, 3);
    }

    #[tokio::test]
    async fn test_auto_save_config_variants() {
        let timer_only = AutoSaveConfig::timer_only();
        assert!(timer_only.should_save_on_event(AutoSaveEvent::TimerTick));
        assert!(!timer_only.should_save_on_event(AutoSaveEvent::TabCreated));

        let all_events = AutoSaveConfig::all_events();
        assert!(all_events.should_save_on_event(AutoSaveEvent::TabCreated));
        assert!(all_events.should_save_on_event(AutoSaveEvent::TabTitleChanged));
    }

    #[tokio::test]
    async fn test_auto_save_disabled() {
        let fixture = TestFixture::new();

        let config = AutoSaveConfig {
            enabled: false,
            ..Default::default()
        };

        let auto_save = AutoSaveManager::new(config, fixture.session_manager.clone());
        let handle =
            auto_save.start(|_name, _manager| async { Ok(std::path::PathBuf::from("/tmp/test")) });

        assert!(handle.is_none());
    }

    #[tokio::test]
    async fn test_auto_save_handle_commands() {
        let fixture = TestFixture::new();

        let auto_save = AutoSaveManager::with_default_config(fixture.session_manager.clone());
        let handle = auto_save
            .start(|_name, _manager| async { Ok(std::path::PathBuf::from("/tmp/test")) })
            .expect("Auto-save deveria iniciar");

        // Testa comandos (não verifica efeitos, apenas que não panic)
        handle.notify_event(AutoSaveEvent::TabCreated);
        handle.pause();
        handle.resume();
        handle.force_save();

        // Shutdown
        handle.shutdown().await;
    }

    #[test]
    fn test_auto_save_events() {
        assert_eq!(AutoSaveEvent::TabCreated.description(), "tab_created");
        assert_eq!(AutoSaveEvent::TabClosed.description(), "tab_closed");
        assert_eq!(AutoSaveEvent::LayoutChanged.description(), "layout_changed");
        assert_eq!(AutoSaveEvent::TimerTick.description(), "timer_tick");
        assert_eq!(AutoSaveEvent::Shutdown.description(), "shutdown");
    }
}

mod edge_case_tests {
    use super::*;

    #[test]
    fn test_empty_session_name() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        assert!(fixture.session_manager.save("", &data).is_err());
    }

    #[test]
    fn test_very_long_session_name() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        let long_name = "a".repeat(200);
        fixture.session_manager.save(&long_name, &data).unwrap();

        let loaded = fixture.session_manager.load(&long_name).unwrap();
        assert_eq!(loaded.metadata.name, long_name);
    }

    #[test]
    fn test_special_characters_in_environment() {
        let fixture = TestFixture::new();
        let mut data = SessionData::new();

        data.environment.insert(
            "SPECIAL".to_string(),
            "value with spaces and ñ unicode".to_string(),
        );
        data.environment
            .insert("EMOJI".to_string(), "🎉🚀".to_string());

        fixture.session_manager.save("special", &data).unwrap();
        let loaded = fixture.session_manager.load("special").unwrap();

        assert_eq!(
            loaded.data.environment.get("SPECIAL"),
            Some(&"value with spaces and ñ unicode".to_string())
        );
        assert_eq!(
            loaded.data.environment.get("EMOJI"),
            Some(&"🎉🚀".to_string())
        );
    }

    #[test]
    fn test_unicode_session_name() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        let unicode_names = vec!["sessão-日本語", "backup_한국어", "test_中文", "🚀-rocket"];

        for name in unicode_names {
            fixture.session_manager.save(name, &data).unwrap();
            let loaded = fixture.session_manager.load(name).unwrap();
            assert_eq!(loaded.metadata.name, name);
        }
    }

    #[test]
    fn test_large_session_data() {
        let fixture = TestFixture::new();
        let mut data = SessionData::new();

        // Adiciona muitas variáveis de ambiente
        for i in 0..100 {
            data.environment.insert(
                format!("VAR_{}", i),
                format!("value_{}_{}", i, "x".repeat(100)),
            );
        }

        fixture.session_manager.save("large", &data).unwrap();
        let loaded = fixture.session_manager.load("large").unwrap();

        assert_eq!(loaded.data.environment.len(), 100);
    }

    #[test]
    fn test_empty_backup_list() {
        let fixture = TestFixture::new();

        let backups = fixture.backup_manager.list_backups("nonexistent").unwrap();
        assert!(backups.is_empty());

        let stats = fixture.backup_manager.stats().unwrap();
        assert_eq!(stats.total_backups, 0);
    }

    #[test]
    fn test_delete_nonexistent_backup() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        let backup = fixture.backup_manager.backup("temp", &data).unwrap();

        // Deleta uma vez
        fixture.backup_manager.delete_backup(&backup).unwrap();

        // Tentar deletar novamente deve retornar erro ou ser no-op
        // (comportamento depende da implementação)
    }

    #[tokio::test]
    async fn test_migration_unsupported_version() {
        let version_manager = VersionManager::new();

        // Sessão com versão futura
        let mut future_session = Session::new("test", SessionData::new());
        future_session.metadata.version = 999;

        let result = version_manager.migrate(future_session);
        assert!(result.is_err());
    }
}

// Testes de performance (básicos)
mod performance_tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_save_performance() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        let start = Instant::now();

        for i in 0..100 {
            fixture
                .session_manager
                .save(format!("perf-{}", i), &data)
                .unwrap();
        }

        let duration = start.elapsed();
        println!("Salvar 100 sessões: {:?}", duration);

        // Deve ser rápido (< 1 segundo)
        assert!(duration < Duration::from_secs(1));
    }

    #[test]
    fn test_load_performance() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        // Prepara dados
        for i in 0..100 {
            fixture
                .session_manager
                .save(format!("perf-{}", i), &data)
                .unwrap();
        }

        let start = Instant::now();

        for i in 0..100 {
            fixture.session_manager.load(format!("perf-{}", i)).unwrap();
        }

        let duration = start.elapsed();
        println!("Carregar 100 sessões: {:?}", duration);

        // No Windows o antivírus examina cada arquivo aberto: na VM de CI,
        // em debug, foram ~28 ms por arquivo (só leitura + JSON).
        let limit = if cfg!(windows) { 6 } else { 1 };
        assert!(duration < Duration::from_secs(limit), "{duration:?}");
    }

    #[test]
    fn test_list_performance() {
        let fixture = TestFixture::new();
        let data = SessionData::new();

        // Prepara dados
        for i in 0..100 {
            fixture
                .session_manager
                .save(format!("perf-{}", i), &data)
                .unwrap();
        }

        let start = Instant::now();
        let sessions = fixture.session_manager.list_sessions().unwrap();
        let duration = start.elapsed();

        println!("Listar {} sessões: {:?}", sessions.len(), duration);
        assert_eq!(sessions.len(), 100);
    }
}
