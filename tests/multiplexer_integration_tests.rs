//! Testes de Integração do Multiplexer

use code_terminal::multiplexer::commands::SplitDirection as CommandSplitDirection;
use code_terminal::multiplexer::{
    CommandParser, GroupManager, MultiplexerError, MuxCommand, PersistenceConfig, SessionId,
    SessionList, SessionMonitor, SessionPersistence, SessionServer, SessionSwitcher, SortOrder,
};

fn cleanup_test_dir(path: &std::path::Path) {
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn test_session_server_create_and_list() {
    let server = SessionServer::new();

    let _id1 = server
        .create_session(Some("session-1".to_string()), (80, 24))
        .unwrap();
    let _id2 = server
        .create_session(Some("session-2".to_string()), (80, 24))
        .unwrap();

    let sessions = server.list_sessions();
    assert_eq!(sessions.len(), 2);

    let names: Vec<_> = sessions.iter().map(|s| s.name.clone()).collect();
    assert!(names.contains(&"session-1".to_string()));
    assert!(names.contains(&"session-2".to_string()));
}

#[test]
fn test_session_server_get_and_remove() {
    let server = SessionServer::new();

    let id = server
        .create_session(Some("test-session".to_string()), (80, 24))
        .unwrap();

    let session = server.get_session(id).unwrap();
    assert_eq!(session.name, "test-session");

    server.remove_session(id).unwrap();

    assert!(server.get_session(id).is_err());
}

#[test]
fn test_session_server_rename() {
    let server = SessionServer::new();

    let id = server
        .create_session(Some("old-name".to_string()), (80, 24))
        .unwrap();
    server.rename_session(id, "new-name".to_string()).unwrap();

    let session = server.get_session(id).unwrap();
    assert_eq!(session.name, "new-name");
}

#[test]
fn test_persistence_basic() {
    let test_dir = std::env::temp_dir().join("codeterm_test_mux_1");
    cleanup_test_dir(&test_dir);

    let config = PersistenceConfig {
        persistence_dir: test_dir.clone(),
        auto_save_interval: 60,
        max_saved_sessions: 100,
        persist_output_history: false,
        inactive_session_ttl_days: 30,
    };

    let persistence = SessionPersistence::with_config(config);

    let mut session = code_terminal::multiplexer::MultiplexerSession::new(
        Some("persistent-session".to_string()),
        (80, 24),
    );
    session.working_dir = "/home/user".to_string();

    persistence.save_session(&session).unwrap();

    let loaded = persistence.load_session(session.id).unwrap();
    assert_eq!(loaded.name, "persistent-session");
    assert_eq!(loaded.working_dir, "/home/user");

    cleanup_test_dir(&test_dir);
}

#[test]
fn test_persistence_list() {
    let test_dir = std::env::temp_dir().join("codeterm_test_mux_2");
    cleanup_test_dir(&test_dir);

    let config = PersistenceConfig {
        persistence_dir: test_dir.clone(),
        auto_save_interval: 60,
        max_saved_sessions: 100,
        persist_output_history: false,
        inactive_session_ttl_days: 30,
    };

    let persistence = SessionPersistence::with_config(config);

    for i in 0..3 {
        let session = code_terminal::multiplexer::MultiplexerSession::new(
            Some(format!("session-{}", i)),
            (80, 24),
        );
        persistence.save_session(&session).unwrap();
    }

    let sessions = persistence.list_saved_sessions().unwrap();
    assert_eq!(sessions.len(), 3);

    cleanup_test_dir(&test_dir);
}

#[test]
fn test_session_monitor_io() {
    let mut monitor = SessionMonitor::new();

    let session_id = SessionId::new();

    monitor.record_io(session_id, 100, 50);
    monitor.record_attach(session_id);

    // Verifica que monitor funcionou sem panic
    let _global = monitor.global_stats();
}

#[test]
fn test_group_manager_basic() {
    let mut manager = GroupManager::new();

    let group_id = manager.create_group("test-group".to_string());

    let session_id = SessionId::new();
    manager.add_to_group(session_id, group_id).unwrap();

    let groups = manager.get_session_groups(session_id);
    assert!(!groups.is_empty());
}

#[test]
fn test_group_manager_multiple() {
    let mut manager = GroupManager::new();

    let group1 = manager.create_group("group-1".to_string());
    let group2 = manager.create_group("group-2".to_string());

    let session1 = SessionId::new();
    let session2 = SessionId::new();

    manager.add_to_group(session1, group1).unwrap();
    manager.add_to_group(session2, group2).unwrap();

    let s1_groups = manager.get_session_groups(session1);
    let s2_groups = manager.get_session_groups(session2);

    assert_eq!(s1_groups.len(), 1);
    assert_eq!(s2_groups.len(), 1);
}

#[test]
fn test_session_list_navigation() {
    let mut list = SessionList::new();

    for i in 0..5 {
        let session = code_terminal::multiplexer::MultiplexerSession::new(
            Some(format!("session-{}", i)),
            (80, 24),
        );
        list.add_session(session);
    }

    assert_eq!(list.sessions().len(), 5);

    list.select_next();
    list.select_next();
    list.select_index(3);

    let first_id = list.sessions()[0].id;
    assert!(list.select_by_id(first_id));
}

#[test]
fn test_session_list_sorting() {
    let mut list = SessionList::new();

    list.add_session(code_terminal::multiplexer::MultiplexerSession::new(
        Some("charlie".to_string()),
        (80, 24),
    ));
    list.add_session(code_terminal::multiplexer::MultiplexerSession::new(
        Some("alpha".to_string()),
        (80, 24),
    ));
    list.add_session(code_terminal::multiplexer::MultiplexerSession::new(
        Some("bravo".to_string()),
        (80, 24),
    ));

    list.set_sort_order(SortOrder::Name);

    let sessions = list.sessions();
    assert_eq!(sessions[0].name, "alpha");
    assert_eq!(sessions[1].name, "bravo");
    assert_eq!(sessions[2].name, "charlie");
}

#[test]
fn test_session_switcher_navigation() {
    let mut switcher = SessionSwitcher::new();

    let sessions: Vec<_> = (0..3)
        .map(|i| {
            code_terminal::multiplexer::MultiplexerSession::new(
                Some(format!("session-{}", i)),
                (80, 24),
            )
        })
        .collect();

    let ids: Vec<_> = sessions.iter().map(|s| s.id).collect();
    switcher.set_sessions(sessions);

    // Navegação básica
    assert_eq!(switcher.current_id(), Some(ids[0]));

    // switch_next
    switcher.switch_next();
    assert_eq!(switcher.current_id(), Some(ids[1]));

    switcher.switch_next();
    assert_eq!(switcher.current_id(), Some(ids[2]));

    // wrap around
    switcher.switch_next();
    assert_eq!(switcher.current_id(), Some(ids[0]));

    // switch_previous
    switcher.switch_previous();
    assert_eq!(switcher.current_id(), Some(ids[2]));
}

#[test]
fn test_session_switcher_last() {
    let mut switcher = SessionSwitcher::new();

    let sessions: Vec<_> = (0..2)
        .map(|i| {
            code_terminal::multiplexer::MultiplexerSession::new(
                Some(format!("session-{}", i)),
                (80, 24),
            )
        })
        .collect();

    let ids: Vec<_> = sessions.iter().map(|s| s.id).collect();
    switcher.set_sessions(sessions);

    switcher.switch_next();

    let result = switcher.switch_last();
    assert!(result.is_some());
    assert_eq!(switcher.current_id(), Some(ids[0]));
}

#[test]
fn test_session_switcher_by_name() {
    let mut switcher = SessionSwitcher::new();

    let sessions: Vec<_> = (0..3)
        .map(|i| {
            code_terminal::multiplexer::MultiplexerSession::new(
                Some(format!("session-{}", i)),
                (80, 24),
            )
        })
        .collect();

    switcher.set_sessions(sessions);

    let result = switcher.switch_to_name("session-1");
    assert!(result);

    let result = switcher.switch_to_name("nonexistent");
    assert!(!result);
}

#[test]
fn test_command_parser_new_session() {
    let cmd = CommandParser::parse("new-session -s mysession").unwrap();
    match cmd {
        MuxCommand::NewSession { name } => {
            assert_eq!(name, Some("mysession".to_string()));
        }
        _ => panic!("Expected NewSession command"),
    }
}

#[test]
fn test_command_parser_split() {
    let cmd = CommandParser::parse("split-window -h").unwrap();
    match cmd {
        MuxCommand::SplitPane { direction, .. } => {
            assert_eq!(direction, CommandSplitDirection::Horizontal);
        }
        _ => panic!("Expected SplitPane command"),
    }

    let cmd = CommandParser::parse("split-window -v").unwrap();
    match cmd {
        MuxCommand::SplitPane { direction, .. } => {
            assert_eq!(direction, CommandSplitDirection::Vertical);
        }
        _ => panic!("Expected SplitPane command"),
    }
}

#[test]
fn test_command_parser_attach() {
    let cmd = CommandParser::parse("attach-session mysession").unwrap();
    match cmd {
        MuxCommand::AttachSession { target } => {
            assert_eq!(target, "mysession");
        }
        _ => panic!("Expected AttachSession command"),
    }
}

#[test]
fn test_command_parser_aliases() {
    let cmd1 = CommandParser::parse("new -s test").unwrap();
    let cmd2 = CommandParser::parse("new-session -s test").unwrap();

    match (cmd1, cmd2) {
        (MuxCommand::NewSession { name: n1 }, MuxCommand::NewSession { name: n2 }) => {
            assert_eq!(n1, n2);
        }
        _ => panic!("Commands should be equivalent"),
    }
}

#[test]
fn test_error_session_not_found() {
    let server = SessionServer::new();

    let fake_id = SessionId::new();
    let result = server.get_session(fake_id);

    assert!(result.is_err());
    match result {
        Err(MultiplexerError::SessionNotFound(_)) => {}
        _ => panic!("Expected SessionNotFound error"),
    }
}

#[test]
fn test_multiplexer_end_to_end() {
    let test_dir = std::env::temp_dir().join("codeterm_test_mux_e2e");
    cleanup_test_dir(&test_dir);

    let server = SessionServer::new();

    let session_id = server
        .create_session(Some("e2e-test".to_string()), (80, 24))
        .unwrap();

    let session = server.get_session(session_id).unwrap();

    let config = PersistenceConfig {
        persistence_dir: test_dir.clone(),
        auto_save_interval: 60,
        max_saved_sessions: 100,
        persist_output_history: false,
        inactive_session_ttl_days: 30,
    };
    let persistence = SessionPersistence::with_config(config);
    persistence.save_session(&session).unwrap();

    let loaded = persistence.load_session(session_id).unwrap();
    assert_eq!(loaded.name, "e2e-test");

    let mut monitor = SessionMonitor::new();
    monitor.record_attach(session_id);

    let mut groups = GroupManager::new();
    let group_id = groups.create_group("test-group".to_string());
    groups.add_to_group(session_id, group_id).unwrap();

    let mut list = SessionList::new();
    list.add_session(session.clone());
    assert_eq!(list.sessions().len(), 1);

    let mut switcher = SessionSwitcher::new();
    switcher.set_sessions(vec![session.clone()]);
    assert!(switcher.current().is_some());

    cleanup_test_dir(&test_dir);
}
