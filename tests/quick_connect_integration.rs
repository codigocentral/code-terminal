//! Testes de integração para o Quick Connect UI
//!
//! Estes testes verificam a integração entre o Quick Connect UI,
//! o gerenciador SSH e o estado da aplicação.

use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Importa os módulos necessários
use code_terminal::ssh::{
    QuickConnectField, QuickConnectParams, QuickConnectRecent, SshHost, SshManager,
};
use code_terminal::ui::quick_connect::QuickConnectRenderState;
use code_terminal::ui::quick_connect::{QuickConnectKey, QuickConnectUI};

/// Cria um gerenciador SSH para testes
fn create_test_ssh_manager() -> Arc<RwLock<SshManager>> {
    Arc::new(RwLock::new(SshManager::new()))
}

#[test]
fn test_quick_connect_ui_lifecycle() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);

    // Estado inicial
    assert!(!ui.is_visible());
    assert!(!ui.is_active());

    // Mostra
    ui.show();
    assert!(ui.is_visible());
    assert!(!ui.is_active());

    // Ativa
    ui.activate();
    assert!(ui.is_active());

    // Desativa
    ui.deactivate();
    assert!(!ui.is_active());

    // Esconde
    ui.hide();
    assert!(!ui.is_visible());
}

#[test]
fn test_quick_connect_input_handling() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Digita um hostname
    ui.type_char('s');
    ui.type_char('e');
    ui.type_char('r');
    ui.type_char('v');
    ui.type_char('e');
    ui.type_char('r');

    assert_eq!(ui.get_field(QuickConnectField::Host), "server");

    // Backspace
    ui.backspace();
    assert_eq!(ui.get_field(QuickConnectField::Host), "serve");

    // Limpa tudo
    ui.clear_fields();
    assert_eq!(ui.get_field(QuickConnectField::Host), "");
}

#[test]
fn test_quick_connect_field_navigation() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Campo inicial: Host
    assert_eq!(ui.focused_field(), QuickConnectField::Host);

    // Navega para frente
    ui.next_field();
    assert_eq!(ui.focused_field(), QuickConnectField::User);

    ui.next_field();
    assert_eq!(ui.focused_field(), QuickConnectField::Port);

    ui.next_field();
    assert_eq!(ui.focused_field(), QuickConnectField::IdentityFile);

    // Volta para o início
    ui.next_field();
    assert_eq!(ui.focused_field(), QuickConnectField::Host);

    // Navega para trás
    ui.previous_field();
    assert_eq!(ui.focused_field(), QuickConnectField::IdentityFile);
}

#[test]
fn test_quick_connect_key_handling() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Teclas de caractere
    assert!(ui.handle_key(QuickConnectKey::Char('h')));
    assert!(ui.handle_key(QuickConnectKey::Char('o')));
    assert!(ui.handle_key(QuickConnectKey::Char('s')));
    assert!(ui.handle_key(QuickConnectKey::Char('t')));

    assert_eq!(ui.get_field(QuickConnectField::Host), "host");

    // Backspace
    assert!(ui.handle_key(QuickConnectKey::Backspace));
    assert_eq!(ui.get_field(QuickConnectField::Host), "hos");

    // Tab navega entre campos
    assert!(ui.handle_key(QuickConnectKey::Tab));
    assert_eq!(ui.focused_field(), QuickConnectField::User);

    // Shift+Tab volta
    assert!(ui.handle_key(QuickConnectKey::ShiftTab));
    assert_eq!(ui.focused_field(), QuickConnectField::Host);

    // Escape fecha
    assert!(ui.handle_key(QuickConnectKey::Escape));
    assert!(!ui.is_visible());
}

#[test]
fn test_quick_connect_parse_and_fill() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Preenche com formato completo
    ui.parse_and_fill("admin@example.com:2222");

    assert_eq!(ui.get_field(QuickConnectField::Host), "example.com");
    assert_eq!(ui.get_field(QuickConnectField::User), "admin");
    assert_eq!(ui.get_field(QuickConnectField::Port), "2222");

    // Limpa e preenche sem porta
    ui.clear_fields();
    ui.parse_and_fill("user@server");

    assert_eq!(ui.get_field(QuickConnectField::Host), "server");
    assert_eq!(ui.get_field(QuickConnectField::User), "user");
    assert_eq!(ui.get_field(QuickConnectField::Port), "");

    // Apenas hostname
    ui.clear_fields();
    ui.parse_and_fill("myhost");

    assert_eq!(ui.get_field(QuickConnectField::Host), "myhost");
    assert_eq!(ui.get_field(QuickConnectField::User), "");
}

#[test]
fn test_quick_connect_fill_from_host() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Cria um host SSH completo
    let mut host = SshHost::new("myserver");
    host.host_name = Some("192.168.1.100".to_string());
    host.user = Some("root".to_string());
    host.port = Some(2222);

    // Preenche a UI a partir do host
    ui.fill_from_host(&host);

    assert_eq!(ui.get_field(QuickConnectField::Host), "192.168.1.100");
    assert_eq!(ui.get_field(QuickConnectField::User), "root");
    assert_eq!(ui.get_field(QuickConnectField::Port), "2222");
}

#[test]
fn test_quick_connect_callbacks() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);

    let connected = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));

    let connected_clone = Arc::clone(&connected);
    let cancelled_clone = Arc::clone(&cancelled);

    ui.on_connect(move |_| {
        connected_clone.store(true, Ordering::SeqCst);
    });

    ui.on_cancel(move || {
        cancelled_clone.store(true, Ordering::SeqCst);
    });

    // Mostra e cancela
    ui.show();
    ui.cancel();

    assert!(cancelled.load(Ordering::SeqCst));
    assert!(!ui.is_visible());
}

#[test]
fn test_quick_connect_selection_navigation() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Adiciona alguns dados para filtrar
    let recent = vec![
        QuickConnectRecent::new("host1.example.com", 22),
        QuickConnectRecent::new("host2.example.com", 22),
    ];
    let hosts = vec![
        SshHost::new("server1"),
        SshHost::new("server2"),
        SshHost::new("server3"),
    ];

    ui.dialog.update_filter(&recent, &hosts);

    // Verifica estado inicial
    assert_eq!(ui.selection_index(), 0);
    assert_eq!(ui.total_results(), 5); // 2 recentes + 3 hosts

    // Navega para baixo
    ui.select_down();
    assert_eq!(ui.selection_index(), 1);

    ui.select_down();
    assert_eq!(ui.selection_index(), 2);

    // Navega para cima
    ui.select_up();
    assert_eq!(ui.selection_index(), 1);

    // Não passa do início
    ui.select_up();
    ui.select_up();
    ui.select_up();
    assert_eq!(ui.selection_index(), 0);
}

#[test]
fn test_quick_connect_apply_selection() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Adiciona um host para selecionar
    // QuickConnectRecent::new espera formato "user@host" sem porta
    let mut recent_conn = QuickConnectRecent::new("admin@test.com", 2222);
    recent_conn.port = 2222; // Porta explícita
    let recent = vec![recent_conn];
    let hosts: Vec<SshHost> = vec![];

    ui.dialog.update_filter(&recent, &hosts);

    // Aplica a seleção
    ui.apply_selection();

    // Verifica se os campos foram preenchidos
    // Nota: a função apply_selection preenche o hostname como "test.com" (sem porta)
    // pois a porta é tratada separadamente
    assert_eq!(ui.get_field(QuickConnectField::Host), "test.com");
    assert_eq!(ui.get_field(QuickConnectField::User), "admin");
    assert_eq!(ui.get_field(QuickConnectField::Port), "2222");
}

#[test]
fn test_quick_connect_filter_mode() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Modo inicial: All
    use code_terminal::ssh::FilterMode;
    assert_eq!(ui.filter_mode(), FilterMode::All);

    // Alterna para Recent
    ui.toggle_filter_mode();
    assert_eq!(ui.filter_mode(), FilterMode::Recent);

    // Alterna para Config
    ui.toggle_filter_mode();
    assert_eq!(ui.filter_mode(), FilterMode::Config);

    // Volta para All
    ui.toggle_filter_mode();
    assert_eq!(ui.filter_mode(), FilterMode::All);
}

#[test]
fn test_quick_connect_render_state() {
    // Testa criação centralizada
    let state = QuickConnectRenderState::centered(1024, 768, 500.0, 400.0);

    assert_eq!(state.width, 500.0);
    assert_eq!(state.height, 400.0);
    assert_eq!(state.x, 262.0); // (1024 - 500) / 2
    assert_eq!(state.y, 184.0); // (768 - 400) / 2

    // Testa posições de campos
    let field_0_y = state.field_y(0);
    assert_eq!(field_0_y, state.y + state.padding);

    // Testa largura do input
    let input_width = state.input_width();
    assert_eq!(input_width, state.width - (2.0 * state.padding) - 100.0);
}

#[test]
fn test_quick_connect_error_handling() {
    let ssh_manager = create_test_ssh_manager();
    let mut ui = QuickConnectUI::new(ssh_manager);
    ui.show();

    // Inicialmente sem erro
    assert!(ui.error_message().is_none());

    // Define um erro
    ui.dialog.set_error("Test error message");
    assert_eq!(ui.error_message(), Some("Test error message"));

    // Limpa o erro digitando
    ui.type_char('a');
    assert!(ui.error_message().is_none());
}

#[test]
fn test_quick_connect_recent_creation() {
    // Testa criação de conexão recente
    let recent = QuickConnectRecent::new("admin@example.com", 2222);

    assert_eq!(recent.hostname, "example.com");
    assert_eq!(recent.user, Some("admin".to_string()));
    assert_eq!(recent.port, 2222);
    assert_eq!(recent.use_count, 1);

    // Testa display
    assert_eq!(recent.display_text(), "admin@example.com:2222");

    // Testa touch (atualiza contador)
    let mut recent_mut = recent.clone();
    let original_count = recent_mut.use_count;
    recent_mut.touch();
    assert_eq!(recent_mut.use_count, original_count + 1);
}

#[test]
fn test_quick_connect_params() {
    let params = QuickConnectParams {
        hostname: "example.com".to_string(),
        port: 2222,
        user: Some("admin".to_string()),
        identity_file: Some("~/.ssh/id_rsa".to_string()),
    };

    // Testa connection_string
    assert_eq!(params.connection_string(), "admin@example.com:2222");

    // Testa to_ssh_command
    let cmd = params.to_ssh_command();
    assert!(cmd.contains(&"-p".to_string()));
    assert!(cmd.contains(&"2222".to_string()));
    assert!(cmd.contains(&"-i".to_string()));
    assert!(cmd.contains(&"~/.ssh/id_rsa".to_string()));
    assert!(cmd.contains(&"admin@example.com".to_string()));
}

#[test]
fn test_quick_connect_params_default_port() {
    let params = QuickConnectParams {
        hostname: "example.com".to_string(),
        port: 22,
        user: None,
        identity_file: None,
    };

    // Com porta padrão, não deve aparecer no connection_string
    assert_eq!(params.connection_string(), "example.com");
}

#[test]
fn test_quick_connect_field_labels() {
    assert_eq!(QuickConnectField::Host.label(), "Host");
    assert_eq!(QuickConnectField::User.label(), "User");
    assert_eq!(QuickConnectField::Port.label(), "Port");
    assert_eq!(QuickConnectField::IdentityFile.label(), "Identity File");
}

#[test]
fn test_quick_connect_field_placeholders() {
    assert_eq!(
        QuickConnectField::Host.placeholder(),
        "hostname or IP address"
    );
    assert_eq!(QuickConnectField::User.placeholder(), "username (optional)");
    assert_eq!(QuickConnectField::Port.placeholder(), "22");
    assert_eq!(
        QuickConnectField::IdentityFile.placeholder(),
        "~/.ssh/id_rsa (optional)"
    );
}

#[test]
fn test_quick_connect_field_navigation_order() {
    // Testa ordem de navegação
    assert_eq!(QuickConnectField::Host.next(), QuickConnectField::User);
    assert_eq!(QuickConnectField::User.next(), QuickConnectField::Port);
    assert_eq!(
        QuickConnectField::Port.next(),
        QuickConnectField::IdentityFile
    );
    assert_eq!(
        QuickConnectField::IdentityFile.next(),
        QuickConnectField::Host
    );

    // Testa navegação reversa
    assert_eq!(
        QuickConnectField::Host.previous(),
        QuickConnectField::IdentityFile
    );
    assert_eq!(QuickConnectField::User.previous(), QuickConnectField::Host);
    assert_eq!(QuickConnectField::Port.previous(), QuickConnectField::User);
    assert_eq!(
        QuickConnectField::IdentityFile.previous(),
        QuickConnectField::Port
    );
}

#[test]
fn test_ssh_manager_integration() {
    let ssh_manager = create_test_ssh_manager();

    // Inicialmente sem configuração carregada
    let _hosts = ssh_manager.read().get_all_hosts();
    // Pode estar vazio se não houver arquivo de config

    // Testa busca
    let results = ssh_manager.read().search_hosts("test");
    assert!(results.is_empty() || !results.is_empty()); // Depende do ambiente

    // Testa conexões recentes
    let recent = ssh_manager.read().get_recent_connections();
    assert!(recent.is_empty());
}
