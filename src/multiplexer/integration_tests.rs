//! Integration Tests - Testes de integração para o modo multiplexer

#[cfg(test)]
mod tests {
    use parking_lot::RwLock;
    use std::sync::Arc;
    use std::time::Duration;

    /// Orçamento de tempo dos testes de desempenho.
    ///
    /// Os limites valem para build otimizado. `cargo test` e a cobertura rodam
    /// em debug (instrumentado, com milhares de testes em paralelo no mesmo
    /// runner), onde 50 gravações já passaram de 500 ms sem regressão nenhuma.
    /// Em debug a folga é 10×: o teste continua pegando regressão de ordem de
    /// grandeza (ex.: algo que ficou quadrático); números finos ficam em benches/.
    fn budget(ms: u64) -> Duration {
        let slack = if cfg!(debug_assertions) { 10 } else { 1 };
        Duration::from_millis(ms * slack)
    }

    use crate::multiplexer::*;

    // =========================================================================
    // Testes de Integração do Servidor
    // =========================================================================

    #[tokio::test]
    async fn test_server_create_multiple_sessions() {
        let server = Arc::new(RwLock::new(SessionServer::new()));

        // Cria múltiplas sessões
        let _id1 = server
            .write()
            .create_session(Some("session1".to_string()), (80, 24))
            .unwrap();
        let _id2 = server
            .write()
            .create_session(Some("session2".to_string()), (120, 40))
            .unwrap();
        let _id3 = server.write().create_session(None, (80, 24)).unwrap();

        // Verifica lista
        let sessions = server.read().list_sessions();
        assert_eq!(sessions.len(), 3);

        // Verifica nomes
        let names: Vec<String> = sessions.iter().map(|s| s.name.clone()).collect();
        assert!(names.contains(&"session1".to_string()));
        assert!(names.contains(&"session2".to_string()));
    }

    #[tokio::test]
    async fn test_server_attach_multiple_clients() {
        let server = Arc::new(RwLock::new(SessionServer::new()));

        // Cria sessão
        let session_id = server
            .write()
            .create_session(Some("shared".to_string()), (80, 24))
            .unwrap();

        // Cria múltiplos clientes
        let client1 = client::ClientConnection::new((80, 24));
        let client2 = client::ClientConnection::new((80, 24));
        let client3 = client::ClientConnection::new((80, 24));

        let client_id1 = server.write().add_client(client1).unwrap();
        let client_id2 = server.write().add_client(client2).unwrap();
        let client_id3 = server.write().add_client(client3).unwrap();

        // Anexa todos à mesma sessão
        server
            .write()
            .attach_client(client_id1, session_id)
            .unwrap();
        server
            .write()
            .attach_client(client_id2, session_id)
            .unwrap();
        server
            .write()
            .attach_client(client_id3, session_id)
            .unwrap();

        // Verifica contagem
        let session = server.read().get_session(session_id).unwrap();
        assert_eq!(session.client_count, 3);
        assert_eq!(session.state, SessionState::Active);
    }

    #[tokio::test]
    async fn test_server_client_lifecycle() {
        let server = Arc::new(RwLock::new(SessionServer::new()));

        // Cria sessão e cliente
        let session_id = server
            .write()
            .create_session(Some("test".to_string()), (80, 24))
            .unwrap();
        let client = client::ClientConnection::new((80, 24));
        let client_id = server.write().add_client(client).unwrap();

        // Anexa
        server.write().attach_client(client_id, session_id).unwrap();

        // Verifica estado
        let session = server.read().get_session(session_id).unwrap();
        assert_eq!(session.client_count, 1);
        assert_eq!(session.state, SessionState::Active);

        // Desanexa
        server.write().detach_client(client_id).unwrap();

        // Verifica estado
        let session = server.read().get_session(session_id).unwrap();
        assert_eq!(session.client_count, 0);
        assert_eq!(session.state, SessionState::Background);

        // Remove cliente
        server.write().remove_client(client_id).unwrap();

        // Verifica que cliente foi removido
        assert!(server.read().get_client(client_id).is_err());
    }

    // =========================================================================
    // Testes de Integração de Persistência
    // =========================================================================

    #[test]
    fn test_persistence_session_lifecycle() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = persistence::PersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save_interval: 300,
            max_saved_sessions: 50,
            persist_output_history: true,
            inactive_session_ttl_days: 30,
        };

        let persistence = persistence::SessionPersistence::with_config(config);

        // Cria e salva sessão
        let mut session = MultiplexerSession::new(Some("persistent".to_string()), (100, 30));
        session.working_dir = "/home/user/projects".to_string();
        session.shell = "/bin/zsh".to_string();
        session
            .metadata
            .insert("project".to_string(), "codeterm".to_string());

        persistence.save_session(&session).unwrap();

        // Verifica que existe
        assert!(persistence.session_exists(session.id));

        // Carrega
        let loaded = persistence.load_session(session.id).unwrap();
        assert_eq!(loaded.name, "persistent");
        assert_eq!(loaded.working_dir, "/home/user/projects");
        assert_eq!(loaded.shell, "/bin/zsh");
        assert_eq!(
            loaded.metadata.get("project"),
            Some(&"codeterm".to_string())
        );

        // Remove
        persistence.remove_session(session.id).unwrap();
        assert!(!persistence.session_exists(session.id));
    }

    #[test]
    fn test_persistence_cleanup() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = persistence::PersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save_interval: 300,
            max_saved_sessions: 50,
            persist_output_history: true,
            inactive_session_ttl_days: 1, // 1 dia para teste
        };

        let persistence = persistence::SessionPersistence::with_config(config);

        // Cria sessões
        let mut old_session = MultiplexerSession::new(Some("old".to_string()), (80, 24));
        old_session.last_activity = chrono::Utc::now() - chrono::Duration::days(2);

        let mut recent_session = MultiplexerSession::new(Some("recent".to_string()), (80, 24));
        recent_session.last_activity = chrono::Utc::now();

        persistence.save_session(&old_session).unwrap();
        persistence.save_session(&recent_session).unwrap();

        // Limpa sessões antigas
        let removed = persistence.cleanup_old_sessions(1).unwrap();
        assert_eq!(removed, 1);

        // Verifica que apenas a recente permanece
        let saved = persistence.list_saved_sessions().unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].name, "recent");
    }

    // =========================================================================
    // Testes de Integração de Grupos
    // =========================================================================

    #[test]
    fn test_groups_session_organization() {
        let mut manager = groups::GroupManager::new();

        // Cria grupos
        let work_group = manager.create_group("Work");
        let personal_group = manager.create_group("Personal");

        // Cria sessões
        let session1 = SessionId::new();
        let session2 = SessionId::new();
        let session3 = SessionId::new();

        // Organiza em grupos
        manager.add_to_group(session1, work_group).unwrap();
        manager.add_to_group(session2, work_group).unwrap();
        manager.add_to_group(session3, personal_group).unwrap();

        // Verifica grupos das sessões
        let work_sessions = manager.get_group_sessions(work_group).unwrap();
        assert_eq!(work_sessions.len(), 2);
        assert!(work_sessions.contains(&session1));
        assert!(work_sessions.contains(&session2));

        let personal_sessions = manager.get_group_sessions(personal_group).unwrap();
        assert_eq!(personal_sessions.len(), 1);
        assert!(personal_sessions.contains(&session3));

        // Move sessão entre grupos
        manager
            .move_session(session1, work_group, personal_group)
            .unwrap();

        // Verifica movimentação
        let work_sessions = manager.get_group_sessions(work_group).unwrap();
        assert_eq!(work_sessions.len(), 1);
        assert!(!work_sessions.contains(&session1));

        let personal_sessions = manager.get_group_sessions(personal_group).unwrap();
        assert_eq!(personal_sessions.len(), 2);
        assert!(personal_sessions.contains(&session1));
    }

    #[test]
    fn test_groups_default_group() {
        let mut manager = groups::GroupManager::new().with_default_group("Default");

        // Cria sessões
        let session1 = SessionId::new();
        let session2 = SessionId::new();

        // Adiciona ao grupo padrão
        manager.add_to_default(session1).unwrap();
        manager.add_to_default(session2).unwrap();

        // Verifica
        let default_id = manager.default_group().unwrap();
        let sessions = manager.get_group_sessions(default_id).unwrap();
        assert_eq!(sessions.len(), 2);
    }

    // =========================================================================
    // Testes de Integração de Monitor
    // =========================================================================

    #[test]
    fn test_monitor_session_tracking() {
        let mut monitor = monitor::SessionMonitor::new();

        // Registra sessões
        let id1 = SessionId::new();
        let id2 = SessionId::new();

        monitor.register_session(id1);
        monitor.register_session(id2);

        // Simula atividade
        monitor.record_io(id1, 1000, 500);
        monitor.record_io(id2, 2000, 1000);
        monitor.record_command(id1);
        monitor.record_command(id1);
        monitor.record_command(id2);

        // Verifica estatísticas
        let stats1 = monitor.get_session_stats(id1).unwrap();
        assert_eq!(stats1.bytes_received, 1000);
        assert_eq!(stats1.bytes_sent, 500);
        assert_eq!(stats1.commands_executed, 2);

        let stats2 = monitor.get_session_stats(id2).unwrap();
        assert_eq!(stats2.bytes_received, 2000);
        assert_eq!(stats2.bytes_sent, 1000);
        assert_eq!(stats2.commands_executed, 1);

        // Verifica globais
        let global = monitor.global_stats();
        assert_eq!(
            global
                .total_sessions_created
                .load(std::sync::atomic::Ordering::SeqCst),
            2
        );
    }

    #[test]
    fn test_monitor_state_changes() {
        let mut monitor = monitor::SessionMonitor::new();
        let id = SessionId::new();

        monitor.register_session(id);

        // Registra mudanças de estado
        monitor.record_state_change(id, SessionState::Creating, SessionState::Active);
        monitor.record_state_change(id, SessionState::Active, SessionState::Background);
        monitor.record_state_change(id, SessionState::Background, SessionState::Active);

        // Verifica histórico
        let history = monitor.state_history();
        assert_eq!(history.len(), 3);

        // Verifica último estado
        let last = history.last().unwrap();
        assert_eq!(last.new_state, SessionState::Active);
    }

    #[test]
    fn test_monitor_sessions_by_activity() {
        let mut monitor = monitor::SessionMonitor::new();

        let id1 = SessionId::new();
        let id2 = SessionId::new();
        let id3 = SessionId::new();

        monitor.register_session(id1);
        monitor.register_session(id2);
        monitor.register_session(id3);

        // Diferentes níveis de atividade
        monitor.record_io(id1, 100, 50); // Total: 150
        monitor.record_io(id2, 500, 300); // Total: 800
        monitor.record_io(id3, 200, 100); // Total: 300

        // Ordena por atividade
        let sorted = monitor.sessions_by_activity();

        // Mais ativa primeiro
        assert_eq!(sorted[0].0, id2);
        assert_eq!(sorted[1].0, id3);
        assert_eq!(sorted[2].0, id1);
    }

    // =========================================================================
    // Testes de Integração de Session List
    // =========================================================================

    #[test]
    fn test_session_list_full_workflow() {
        let mut list = session_list::SessionList::new();

        // Cria sessões
        let sessions = vec![
            create_test_session("alpha", 1),
            create_test_session("beta", 2),
            create_test_session("gamma", 3),
        ];

        list.set_sessions(sessions);

        // Verifica ordenação padrão
        assert_eq!(list.sessions()[0].name, "alpha");

        // Ordena por nome
        list.set_sort_order(session_list::SortOrder::Name);
        assert_eq!(list.sessions()[0].name, "alpha");
        assert_eq!(list.sessions()[1].name, "beta");
        assert_eq!(list.sessions()[2].name, "gamma");

        // Navega
        list.select_next();
        assert_eq!(list.selected().unwrap().name, "beta");

        list.select_next();
        assert_eq!(list.selected().unwrap().name, "gamma");

        // Busca
        assert!(list.find_and_select("beta"));
        assert_eq!(list.selected().unwrap().name, "beta");

        // Estatísticas
        let stats = list.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.selected, 1);
    }

    #[test]
    fn test_session_switcher_navigation() {
        let mut switcher = session_list::SessionSwitcher::new();

        let sessions = vec![
            create_test_session("a", 1),
            create_test_session("b", 2),
            create_test_session("c", 3),
        ];

        switcher.set_sessions(sessions);

        // Estado inicial: primeira sessão selecionada
        assert_eq!(switcher.current().unwrap().name, "a");

        // Primeiro switch: a -> b, registra b no histórico
        switcher.switch_next();
        assert_eq!(switcher.current().unwrap().name, "b");
        // Histórico: [b], index=0 - não pode voltar (apenas 1 elemento)
        assert!(!switcher.can_go_back());

        // Segundo switch: b -> c, histórico trunca e adiciona c
        switcher.switch_next();
        assert_eq!(switcher.current().unwrap().name, "c");

        // Troca para a usando switch_to_name, histórico: [b, c, a]
        assert!(switcher.switch_to_name("a"));
        assert_eq!(switcher.current().unwrap().name, "a");
        // Agora pode voltar porque temos histórico com múltiplos elementos
        assert!(switcher.can_go_back());

        // Volta: a -> c
        switcher.back();
        assert_eq!(switcher.current().unwrap().name, "c");
        assert!(switcher.can_go_back()); // pode voltar para b
        assert!(switcher.can_go_forward()); // pode avançar para a

        // Volta novamente: c -> b
        switcher.back();
        assert_eq!(switcher.current().unwrap().name, "b");
        assert!(!switcher.can_go_back()); // início do histórico

        // Avança: b -> c
        switcher.forward();
        assert_eq!(switcher.current().unwrap().name, "c");
    }

    // =========================================================================
    // Testes de Performance
    // =========================================================================

    #[test]
    fn test_server_performance_many_sessions() {
        let server = SessionServer::new();

        // Cria 100 sessões
        let start = std::time::Instant::now();
        for i in 0..100 {
            server
                .create_session(Some(format!("session_{}", i)), (80, 24))
                .unwrap();
        }
        let elapsed = start.elapsed();

        // Deve ser rápido (< 100ms)
        assert!(
            elapsed < budget(100),
            "Creating 100 sessions took too long: {:?}",
            elapsed
        );

        // Lista deve ser rápida
        let start = std::time::Instant::now();
        let sessions = server.list_sessions();
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(10),
            "Listing sessions took too long: {:?}",
            elapsed
        );
        assert_eq!(sessions.len(), 100);
    }

    #[test]
    fn test_session_list_performance_large_list() {
        let mut list = session_list::SessionList::new();

        // Cria 1000 sessões
        let sessions: Vec<_> = (0..1000)
            .map(|i| create_test_session(&format!("session_{}", i), (i % 256) as u8))
            .collect();

        let start = std::time::Instant::now();
        list.set_sessions(sessions);
        let elapsed = start.elapsed();

        // Deve ser rápido (< 50ms)
        assert!(
            elapsed < budget(50),
            "Setting 1000 sessions took too long: {:?}",
            elapsed
        );

        // Ordenação deve ser rápida
        let start = std::time::Instant::now();
        list.set_sort_order(session_list::SortOrder::Name);
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(50),
            "Sorting 1000 sessions took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_persistence_performance() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = persistence::PersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save_interval: 300,
            max_saved_sessions: 1000,
            persist_output_history: true,
            inactive_session_ttl_days: 30,
        };

        let persistence = persistence::SessionPersistence::with_config(config);

        // Salva 50 sessões
        let start = std::time::Instant::now();
        for i in 0..50 {
            let session = MultiplexerSession::new(Some(format!("session_{}", i)), (80, 24));
            persistence.save_session(&session).unwrap();
        }
        let elapsed = start.elapsed();

        // Deve ser rápido (< 500ms)
        assert!(
            elapsed < budget(500),
            "Saving 50 sessions took too long: {:?}",
            elapsed
        );

        // Listagem deve ser rápida
        let start = std::time::Instant::now();
        let sessions = persistence.list_saved_sessions().unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(100),
            "Listing saved sessions took too long: {:?}",
            elapsed
        );
        assert_eq!(sessions.len(), 50);
    }

    // =========================================================================
    // Testes de Comandos
    // =========================================================================

    #[test]
    fn test_command_parser_comprehensive() {
        // Testa parse de vários comandos
        let test_cases = vec![
            ("new-session -s mysession", "NewSession"),
            ("attach mysession", "AttachSession"),
            ("detach", "DetachClient"),
            ("split-window -h", "SplitPane"),
            ("split-window -v", "SplitPane"),
            ("select-pane -U", "SelectPane"),
            ("select-pane -D", "SelectPane"),
            ("new-window -n mywindow vim", "NewWindow"),
            ("kill-session", "KillSession"),
            ("rename-session newname", "RenameSession"),
            ("list-sessions", "ListSessions"),
            ("next-window", "NextWindow"),
            ("previous-window", "PreviousWindow"),
            ("copy-mode", "CopySelection"),
            ("paste-buffer", "PasteBuffer"),
            ("set-option -g status on", "SetOption"),
            ("display-message hello", "DisplayMessage"),
            ("clock-mode", "ClockMode"),
            ("command-prompt", "CommandPrompt"),
            ("exit", "Exit"),
        ];

        for (input, expected_type) in test_cases {
            let result = commands::CommandParser::parse(input);
            assert!(result.is_ok(), "Failed to parse: {}", input);

            let cmd = result.unwrap();
            let actual_type = format!("{:?}", cmd).split('(').next().unwrap().to_string();
            assert!(
                actual_type.contains(expected_type),
                "Expected {} for input '{}', got {}",
                expected_type,
                input,
                actual_type
            );
        }
    }

    #[test]
    fn test_default_keybindings_coverage() {
        let bindings = commands::default_keybindings();

        // Verifica que keybindings essenciais existem
        let essential = vec![
            "C-b c",  // new-window
            "C-b d",  // detach
            "C-b %",  // split vertical
            "C-b \"", // split horizontal
            "C-b n",  // next-window
            "C-b p",  // previous-window
            "C-b [",  // copy-mode
            "C-b ]",  // paste-buffer
            "C-b t",  // clock-mode
            "C-b :",  // command-prompt
            "C-b ?",  // help
            "C-b r",  // reload-config
        ];

        for key in essential {
            assert!(bindings.contains_key(key), "Missing keybinding: {}", key);
        }

        // Verifica total
        assert!(
            bindings.len() >= 20,
            "Expected at least 20 keybindings, got {}",
            bindings.len()
        );
    }

    // =========================================================================
    // Testes de Integração de Windows (Sprint 20)
    // =========================================================================

    #[test]
    fn test_window_manager_full_workflow() {
        let mut manager = WindowManager::new();

        // Cria janelas
        let w1 = manager.create_window(Some("editor".to_string()), (80, 24));
        let w2 = manager.create_window(Some("terminal".to_string()), (80, 24));
        let w3 = manager.create_window(Some("logs".to_string()), (80, 24));

        assert_eq!(manager.window_count(), 3);
        assert_eq!(manager.active_window().unwrap().id, w1);

        // Navegação
        manager.select_window(w2).unwrap();
        assert_eq!(manager.active_window().unwrap().id, w2);

        manager.select_next();
        assert_eq!(manager.active_window().unwrap().id, w3);

        manager.select_previous();
        assert_eq!(manager.active_window().unwrap().id, w2);

        // Renomear
        manager.rename_window(w2, "shell".to_string()).unwrap();
        assert_eq!(manager.get_window(w2).unwrap().name, "shell");

        // Remover
        manager.remove_window(w1).unwrap();
        assert_eq!(manager.window_count(), 2);
    }

    #[test]
    fn test_window_layouts() {
        let mut window = Window::new(Some("test".to_string()), 0, (100, 40));

        // Adiciona painéis
        let pane1 = Pane::new((50, 20), (0, 0));
        let pane2 = Pane::new((50, 20), (50, 0));
        let pane3 = Pane::new((100, 20), (0, 20));

        window.add_pane(pane1);
        window.add_pane(pane2);
        window.add_pane(pane3);

        // Testa layouts
        let layouts = vec![
            WindowLayout::EvenHorizontal,
            WindowLayout::EvenVertical,
            WindowLayout::Tiled,
            WindowLayout::MainHorizontal,
            WindowLayout::MainVertical,
        ];

        for layout in layouts {
            window.set_layout(layout);
            assert_eq!(window.layout, layout);
            assert_eq!(window.pane_count(), 3);
        }
    }

    #[test]
    fn test_window_split_panes() {
        let mut window = Window::new(Some("test".to_string()), 0, (100, 40));

        // Cria primeiro painel
        let pane1 = Pane::new((100, 40), (0, 0));
        window.add_pane(pane1);

        // Divide verticalmente
        let pane2 = window.split_active_pane(true, Some(30)).unwrap();
        assert_eq!(window.pane_count(), 2);

        // Divide horizontalmente
        let pane3 = window.split_active_pane(false, Some(50)).unwrap();
        assert_eq!(window.pane_count(), 3);

        // Troca painéis
        window.swap_panes(pane2, pane3).unwrap();
    }

    // =========================================================================
    // Testes de Integração de Panes (Sprint 20)
    // =========================================================================

    #[test]
    fn test_pane_manager_navigation() {
        let mut manager = PaneManager::new();

        // Cria painéis
        let p1 = manager.create_pane((40, 20), (0, 0));
        let p2 = manager.create_pane((40, 20), (40, 0));
        let p3 = manager.create_pane((80, 20), (0, 20));

        assert_eq!(manager.pane_count(), 3);
        assert_eq!(manager.active_pane().unwrap().id, p1);

        // Seleciona
        manager.select_pane(p2).unwrap();
        assert_eq!(manager.active_pane().unwrap().id, p2);

        manager.select_pane(p3).unwrap();
        assert_eq!(manager.active_pane().unwrap().id, p3);
    }

    #[test]
    fn test_pane_scrollback() {
        let mut pane = Pane::new((80, 24), (0, 0));

        // Adiciona linhas ao scrollback
        for _i in 0..100 {
            let mut line = pane::Line::default();
            for j in 0..80 {
                line.cells.push(pane::Cell {
                    ch: (b'a' + (j % 26) as u8) as char,
                    attributes: pane::CellAttributes::default(),
                });
            }
            pane.scrollback.push_line(line);
        }

        assert_eq!(pane.scrollback.len(), 100);

        // Scroll
        pane.scroll_up(10);
        assert_eq!(pane.scroll_position, 10);

        pane.scroll_down(5);
        assert_eq!(pane.scroll_position, 5);

        pane.scroll_to_top();
        assert_eq!(pane.scroll_position, 76); // 100 - 24

        pane.scroll_to_bottom();
        assert_eq!(pane.scroll_position, 0);
    }

    #[test]
    fn test_pane_copy_mode() {
        let mut pane = Pane::new((80, 24), (0, 0));

        // Preenche scrollback
        for _i in 0..50 {
            let mut line = pane::Line::default();
            for j in 0..80 {
                line.cells.push(pane::Cell {
                    ch: (b'a' + (j % 26) as u8) as char,
                    attributes: pane::CellAttributes::default(),
                });
            }
            pane.scrollback.push_line(line);
        }

        // Entra em modo de cópia
        pane.enter_copy_mode();
        assert!(pane.copy_mode);
        assert!(pane.selection.is_some());

        // Move cursor
        pane.move_cursor(5, 3);
        assert_eq!(pane.cursor, (5, 3));

        // Sai do modo de cópia
        pane.exit_copy_mode();
        assert!(!pane.copy_mode);
        assert!(pane.selection.is_none());
    }

    #[test]
    fn test_pane_contains_point() {
        let pane = Pane::new((40, 12), (10, 5));

        // Dentro
        assert!(pane.contains_point(10, 5));
        assert!(pane.contains_point(49, 16));

        // Fora
        assert!(!pane.contains_point(9, 5));
        assert!(!pane.contains_point(10, 4));
        assert!(!pane.contains_point(50, 16));
        assert!(!pane.contains_point(49, 17));
    }

    // =========================================================================
    // Testes de Integração de Shared Sessions (Sprint 20)
    // =========================================================================

    #[test]
    fn test_shared_session_multiple_clients() {
        let session_id = SessionId::new();
        let session = SharedSession::new(session_id, SharedSessionConfig::default());

        // Clientes entram
        let client1 = ClientId::new();
        let client2 = ClientId::new();
        let client3 = ClientId::new();

        session.join(client1, None).unwrap();
        session.join(client2, None).unwrap();
        session.join(client3, None).unwrap();

        assert_eq!(session.client_count(), 3);

        // Lista clientes
        let clients = session.list_clients();
        assert_eq!(clients.len(), 3);

        // Um cliente sai
        session.leave(client2).unwrap();
        assert_eq!(session.client_count(), 2);

        // Verifica que ainda está vazio
        assert!(!session.is_empty());

        // Todos saem
        session.leave(client1).unwrap();
        session.leave(client3).unwrap();
        assert!(session.is_empty());
    }

    #[test]
    fn test_shared_session_permissions() {
        let session_id = SessionId::new();
        let config =
            SharedSessionConfig::default().with_default_permissions(ClientPermissions::read_only());

        let session = SharedSession::new(session_id, config);

        let client1 = ClientId::new();
        let perms = session.join(client1, None).unwrap();

        // Deve ser read-only
        assert!(!perms.write);
        assert!(perms.read_only);

        // Promove para admin
        session
            .update_permissions(client1, ClientPermissions::admin())
            .unwrap();

        // Agora deve ter todas as permissões
        assert!(session.has_permission(client1, |p| p.write));
        assert!(session.has_permission(client1, |p| p.manage_clients));
    }

    #[test]
    fn test_shared_session_password() {
        let session_id = SessionId::new();
        let config = SharedSessionConfig::default().with_password("secret123");

        let session = SharedSession::new(session_id, config);
        let client = ClientId::new();

        // Sem senha deve falhar
        assert!(session.join(client, None).is_err());

        // Senha errada deve falhar
        assert!(session.join(client, Some("wrong")).is_err());

        // Senha correta deve funcionar
        assert!(session.join(client, Some("secret123")).is_ok());
    }

    #[test]
    fn test_shared_session_input_buffer() {
        let session_id = SessionId::new();
        let session = SharedSession::new(session_id, SharedSessionConfig::default());

        let client = ClientId::new();
        session.join(client, None).unwrap();

        // Envia input
        let input1 = InputData::Keyboard { bytes: vec![0x41] };
        let input2 = InputData::Mouse {
            x: 10,
            y: 20,
            button: 1,
        };

        session.send_input(client, input1).unwrap();
        session.send_input(client, input2).unwrap();

        // Verifica buffer
        let events = session.drain_input();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_shared_session_manager() {
        let mut manager = SharedSessionManager::new();

        // Cria sessões
        let s1 = SessionId::new();
        let s2 = SessionId::new();

        manager.create_shared_session(s1, SharedSessionConfig::default());
        manager.create_shared_session(s2, SharedSessionConfig::default());

        assert_eq!(manager.session_count(), 2);

        // Conecta clientes
        let c1 = ClientId::new();
        let c2 = ClientId::new();

        manager.connect_client(c1, s1, None).unwrap();
        manager.connect_client(c2, s2, None).unwrap();

        assert!(manager.is_client_connected(c1));
        assert!(manager.is_client_connected(c2));

        // Lista sessões
        let sessions = manager.list_sessions();
        assert_eq!(sessions.len(), 2);

        // Desconecta
        manager.disconnect_client(c1).unwrap();
        assert!(!manager.is_client_connected(c1));
    }

    #[test]
    fn test_shared_session_max_clients() {
        let session_id = SessionId::new();
        let config = SharedSessionConfig::default().with_max_clients(3);

        let session = SharedSession::new(session_id, config);

        // Preenche até o máximo
        for _ in 0..3 {
            let client = ClientId::new();
            assert!(session.join(client, None).is_ok());
        }

        // Quarto cliente deve falhar
        let client4 = ClientId::new();
        assert!(session.join(client4, None).is_err());
    }

    // =========================================================================
    // Testes de Integração de SSH Compatibility (Sprint 20)
    // =========================================================================

    #[test]
    fn test_ssh_compat_session_creation() {
        let mut session = SshCompatibleSession::new(
            SessionId::new(),
            "server.example.com",
            "user",
            SshCompatConfig::default(),
        );

        assert_eq!(session.ssh_info.host, "server.example.com");
        assert_eq!(session.ssh_info.user, "user");
        assert_eq!(session.ssh_info.port, 22);
        assert!(!session.ssh_info.is_connected);

        // Configura
        session.ssh_info.port = 2222;
        session.ssh_info.auth_method = AuthMethod::Agent;

        assert_eq!(session.ssh_info.port, 2222);
        assert!(matches!(session.ssh_info.auth_method, AuthMethod::Agent));
    }

    #[test]
    fn test_ssh_compat_connection() {
        let mut manager = SshCompatManager::new();
        let session_id = SessionId::new();

        // Cria sessão
        manager.create_session(session_id, "server.example.com", "user");
        assert_eq!(manager.list_sessions().len(), 1);

        // Conecta
        manager.connect(session_id).unwrap();
        let info = manager.get_session(session_id).unwrap().info();
        assert!(info.is_connected);

        // Desconecta
        manager.disconnect(session_id).unwrap();
        let info = manager.get_session(session_id).unwrap().info();
        assert!(!info.is_connected);
    }

    #[test]
    fn test_ssh_compat_stats() {
        let mut manager = SshCompatManager::new();

        // Cria sessões
        let s1 = SessionId::new();
        let s2 = SessionId::new();
        let s3 = SessionId::new();

        manager.create_session(s1, "host1.com", "user");
        manager.create_session(s2, "host2.com", "user");
        manager.create_session(s3, "host3.com", "user");

        // Conecta algumas
        manager.connect(s1).unwrap();
        manager.connect(s3).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.total_sessions, 3);
        assert_eq!(stats.connected_sessions, 2);
    }

    #[test]
    fn test_ssh_compat_cleanup() {
        let mut manager = SshCompatManager::new();

        let s1 = SessionId::new();
        let s2 = SessionId::new();

        manager.create_session(s1, "host1.com", "user");
        manager.create_session(s2, "host2.com", "user");

        // Conecta apenas uma
        manager.connect(s1).unwrap();

        // Limpa desconectadas
        let removed = manager.cleanup_disconnected();
        assert_eq!(removed, 1);
        assert_eq!(manager.list_sessions().len(), 1);
    }

    #[test]
    fn test_ssh_compat_transport_types() {
        let transports = vec![
            SshTransport::Direct,
            SshTransport::ProxyJump,
            SshTransport::SocksProxy,
            SshTransport::Tunnel,
        ];

        for transport in transports {
            let session = SshCompatibleSession::new(
                SessionId::new(),
                "host.com",
                "user",
                SshCompatConfig::default(),
            )
            .with_transport(transport);

            assert_eq!(session.ssh_info.transport, transport);
        }
    }

    #[test]
    fn test_ssh_compat_auth_methods() {
        let methods = vec![
            AuthMethod::Password,
            AuthMethod::Agent,
            AuthMethod::Kerberos,
            AuthMethod::KeyboardInteractive,
            AuthMethod::PublicKey {
                key_path: std::path::PathBuf::from("~/.ssh/id_rsa"),
            },
        ];

        for method in methods {
            let session = SshCompatibleSession::new(
                SessionId::new(),
                "host.com",
                "user",
                SshCompatConfig::default(),
            )
            .with_auth_method(method.clone());

            assert_eq!(
                format!("{:?}", session.ssh_info.auth_method),
                format!("{:?}", method)
            );
        }
    }

    // =========================================================================
    // Testes de Performance dos Novos Módulos (Sprint 20)
    // =========================================================================

    #[test]
    fn test_window_manager_performance() {
        let mut manager = WindowManager::new();

        // Cria 50 janelas
        let start = std::time::Instant::now();
        for i in 0..50 {
            manager.create_window(Some(format!("window_{}", i)), (80, 24));
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(50),
            "Creating 50 windows took too long: {:?}",
            elapsed
        );
        assert_eq!(manager.window_count(), 50);

        // Navegação deve ser rápida
        let start = std::time::Instant::now();
        for _ in 0..50 {
            manager.select_next();
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(10),
            "Navigation took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_shared_session_performance() {
        let session_id = SessionId::new();
        let session = SharedSession::new(
            session_id,
            SharedSessionConfig::default().with_max_clients(20),
        );

        // Adiciona 20 clientes
        let start = std::time::Instant::now();
        for _ in 0..20 {
            let client = ClientId::new();
            session.join(client, None).unwrap();
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(50),
            "Adding 20 clients took too long: {:?}",
            elapsed
        );
        assert_eq!(session.client_count(), 20);

        // Input deve ser rápido
        let start = std::time::Instant::now();
        for i in 0..100 {
            let client = session.list_clients()[0].client_id;
            let input = InputData::Keyboard {
                bytes: vec![i as u8],
            };
            session.send_input(client, input).unwrap();
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(50),
            "Sending 100 inputs took too long: {:?}",
            elapsed
        );
    }

    #[test]
    fn test_pane_manager_performance() {
        let mut manager = PaneManager::new();

        // Cria 30 painéis
        let start = std::time::Instant::now();
        for i in 0..30 {
            manager.create_pane((40, 12), ((i * 40) as u16 % 800, 0));
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(50),
            "Creating 30 panes took too long: {:?}",
            elapsed
        );
        assert_eq!(manager.pane_count(), 30);

        // Busca por posição deve ser rápida
        let start = std::time::Instant::now();
        for _ in 0..100 {
            manager.find_by_position(100, 5);
        }
        let elapsed = start.elapsed();

        assert!(
            elapsed < budget(10),
            "Finding panes took too long: {:?}",
            elapsed
        );
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn create_test_session(name: &str, id_byte: u8) -> MultiplexerSession {
        let mut session = MultiplexerSession::new(Some(name.to_string()), (80, 24));
        session.id = SessionId(uuid::Uuid::from_bytes([
            id_byte, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]));
        session
    }
}
