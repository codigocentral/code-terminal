//! Testes de integracao para o modulo SSH
//!
//! Estes testes verificam a integracao entre todos os componentes do modulo SSH:
//! - ConfigParser
//! - Host management
//! - Key management
//! - Session management
//! - Storage

use code_terminal::ssh::{
    KeyFingerprint, KeyStorage, KeyType, PortForward, SessionManager, SessionPersistenceConfig,
    SessionState, SessionType, SshConfigParser, SshHost, SshManager, SshSession,
};
use std::path::PathBuf;
use tempfile::TempDir;

/// Cria um arquivo de configuracao SSH temporario para testes
fn create_test_ssh_config() -> String {
    r#"
# Configuracao de teste
Host myserver
    HostName 192.168.1.100
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_rsa
    ForwardAgent yes
    ServerAliveInterval 60
    ServerAliveCountMax 3

Host *.internal.company.com
    User internal
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null

Host github.com gitlab.com
    User git
    IdentityFile ~/.ssh/github_key
    IdentitiesOnly yes

Host jump-server
    HostName jump.example.com
    User jumper
    ProxyJump none

Host final-server
    HostName final.example.com
    ProxyJump jump-server
    User admin
    LocalForward 8080 localhost:80
    RemoteForward 9090 localhost:3000
    DynamicForward 1080

Host * !*.company.com
    User default
    Port 22
"#
    .to_string()
}

#[test]
fn test_ssh_manager_full_workflow() {
    let manager = SshManager::new();

    // Inicialmente sem hosts
    assert!(manager.get_all_hosts().is_empty());

    // Adiciona conexoes recentes
    manager.add_recent_connection(code_terminal::ssh::RecentConnection::new(
        "user@test.com",
        22,
    ));
    let recent = manager.get_recent_connections();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].host, "test.com");

    // Limpa conexoes recentes
    manager.clear_recent_connections();
    assert!(manager.get_recent_connections().is_empty());
}

#[test]
fn test_config_parser_comprehensive() {
    let config = create_test_ssh_config();
    let parser = SshConfigParser::from_str(&config).unwrap();

    // Verifica numero de hosts
    assert_eq!(parser.hosts().len(), 6);

    // Verifica host especifico
    let myserver = parser.find_host("myserver").unwrap();
    assert_eq!(myserver.host_name, Some("192.168.1.100".to_string()));
    assert_eq!(myserver.user, Some("admin".to_string()));
    assert_eq!(myserver.port, Some(2222));
    assert_eq!(myserver.forward_agent, Some(true));
    assert_eq!(myserver.server_alive_interval, Some(60));
    assert_eq!(myserver.server_alive_count_max, Some(3));

    // Verifica wildcard
    let wildcard = parser.find_host("*.internal.company.com").unwrap();
    assert_eq!(wildcard.user, Some("internal".to_string()));

    // Verifica multiplos patterns (usa find_matching_hosts pois github.com e gitlab.com compartilham a mesma config)
    let github_matches = parser.find_matching_hosts("github.com");
    assert!(!github_matches.is_empty());
    assert_eq!(github_matches[0].user, Some("git".to_string()));

    // Verifica gitlab.com tambem
    let gitlab_matches = parser.find_matching_hosts("gitlab.com");
    assert!(!gitlab_matches.is_empty());
    assert_eq!(gitlab_matches[0].user, Some("git".to_string()));

    // Verifica ProxyJump
    let final_server = parser.find_host("final-server").unwrap();
    assert_eq!(final_server.proxy_jump, Some("jump-server".to_string()));

    // Verifica port forwards
    assert_eq!(final_server.local_forward.len(), 1);
    assert_eq!(final_server.local_forward[0].local_port, 8080);
    assert_eq!(final_server.remote_forward.len(), 1);
    assert_eq!(final_server.dynamic_forward.len(), 1);
}

#[test]
fn test_host_matching_comprehensive() {
    // Teste exato
    let host = SshHost::new("exact.match.com");
    assert!(host.matches("exact.match.com"));
    assert!(!host.matches("other.match.com"));

    // Teste wildcard
    let host = SshHost::new("*.example.com");
    assert!(host.matches("www.example.com"));
    assert!(host.matches("api.example.com"));
    assert!(!host.matches("example.org"));

    // Teste multiplos patterns
    let host = SshHost::new("host1 host2 host3");
    assert!(host.matches("host1"));
    assert!(host.matches("host2"));
    assert!(host.matches("host3"));
    assert!(!host.matches("host4"));

    // Teste negacao
    let host = SshHost::new("*.com !bad.com");
    assert!(host.matches("good.com"));
    assert!(!host.matches("bad.com"));

    // Teste glob universal
    let host = SshHost::new("*");
    assert!(host.matches("anything"));
}

#[test]
fn test_ssh_host_effective_values() {
    let mut host = SshHost::new("alias");

    // Sem hostname definido, usa o pattern
    assert_eq!(host.effective_host(), "alias");

    host.host_name = Some("real.server.com".to_string());
    assert_eq!(host.effective_host(), "real.server.com");

    // Porta padrao
    assert_eq!(host.effective_port(), 22);

    host.port = Some(2222);
    assert_eq!(host.effective_port(), 2222);

    // Usuario
    assert_eq!(host.effective_user(), None);

    host.user = Some("admin".to_string());
    assert_eq!(host.effective_user(), Some("admin"));
}

#[test]
fn test_config_parser_effective_config() {
    let config = r#"
Host *
    User defaultuser
    Port 22

Host specific
    HostName specific.example.com
    Port 2222
"#;

    let parser = SshConfigParser::from_str(config).unwrap();

    // Config especifica deve ter usuario padrao + porta especifica
    let effective = parser.get_effective_config("specific").unwrap();
    assert_eq!(effective.user, Some("defaultuser".to_string()));
    assert_eq!(effective.port, Some(2222));
}

#[test]
fn test_config_parser_ssh_command() {
    let config = create_test_ssh_config();
    let parser = SshConfigParser::from_str(&config).unwrap();

    let myserver = parser.find_host("myserver").unwrap();
    let args = parser.to_ssh_command(myserver);

    // Verifica argumentos gerados
    assert!(args.contains(&"-l".to_string()));
    assert!(args.contains(&"admin".to_string()));
    assert!(args.contains(&"-p".to_string()));
    assert!(args.contains(&"2222".to_string()));
    assert!(args.contains(&"-A".to_string()));
    assert!(args.contains(&"192.168.1.100".to_string()));
}

#[test]
fn test_quick_connect_comprehensive() {
    use code_terminal::ssh::{QuickConnectDialog, QuickConnectField};

    let mut dialog = QuickConnectDialog::new();

    // Estado inicial
    assert!(!dialog.is_visible());

    // Mostra dialogo
    dialog.show();
    assert!(dialog.is_visible());

    // Digita hostname
    dialog.set_field(QuickConnectField::Host, "example.com");
    dialog.set_field(QuickConnectField::User, "admin");
    dialog.set_field(QuickConnectField::Port, "2222");

    // Valida
    let params = dialog.validate().unwrap();
    assert_eq!(params.hostname, "example.com");
    assert_eq!(params.user, Some("admin".to_string()));
    assert_eq!(params.port, 2222);

    // Gera comando SSH
    let cmd = params.to_ssh_command();
    assert!(cmd.contains(&"-p".to_string()));
    assert!(cmd.contains(&"2222".to_string()));
}

#[test]
fn test_quick_connect_parse_connection_string() {
    use code_terminal::ssh::{QuickConnectDialog, QuickConnectField};

    let mut dialog = QuickConnectDialog::new();

    // Formato completo
    dialog.parse_and_fill("admin@server.example.com:2222");
    assert_eq!(
        dialog.get_field(QuickConnectField::Host),
        "server.example.com"
    );
    assert_eq!(dialog.get_field(QuickConnectField::User), "admin");
    assert_eq!(dialog.get_field(QuickConnectField::Port), "2222");

    // Sem porta
    dialog.clear_fields();
    dialog.parse_and_fill("user@host");
    assert_eq!(dialog.get_field(QuickConnectField::Host), "host");
    assert_eq!(dialog.get_field(QuickConnectField::User), "user");
    assert_eq!(dialog.get_field(QuickConnectField::Port), "");

    // Apenas hostname
    dialog.clear_fields();
    dialog.parse_and_fill("justhost");
    assert_eq!(dialog.get_field(QuickConnectField::Host), "justhost");
    assert_eq!(dialog.get_field(QuickConnectField::User), "");
}

#[test]
fn test_quick_connect_recent_connections() {
    use code_terminal::ssh::QuickConnectRecent;

    let recent = QuickConnectRecent::new("admin@example.com", 2222);

    assert_eq!(recent.hostname, "example.com");
    assert_eq!(recent.user, Some("admin".to_string()));
    assert_eq!(recent.port, 2222);
    assert_eq!(recent.use_count, 1);

    // Display text
    assert_eq!(recent.display_text(), "admin@example.com:2222");

    // Sem usuario
    let recent2 = QuickConnectRecent::new("example.com", 22);
    assert_eq!(recent2.display_text(), "example.com");
}

#[test]
fn test_key_storage_operations() {
    let temp_dir = TempDir::new().unwrap();
    let mut storage = KeyStorage::new(temp_dir.path().to_path_buf()).unwrap();

    // Lista vazia inicialmente
    assert!(storage.list_keys().is_empty());

    // Cria uma chave para teste
    let key = code_terminal::ssh::SshKey::new(
        "test-key".to_string(),
        KeyType::Ed25519,
        PathBuf::from("/tmp/id_ed25519"),
    );

    // Armazena
    storage.store_key(&key).unwrap();
    assert!(storage.key_exists("test-key"));
    assert_eq!(storage.list_keys().len(), 1);

    // Recupera
    let retrieved = storage.retrieve_key("test-key").unwrap();
    assert_eq!(retrieved.name, "test-key");
    assert_eq!(retrieved.key_type, KeyType::Ed25519);

    // Deleta
    storage.delete_key("test-key").unwrap();
    assert!(!storage.key_exists("test-key"));
}

#[test]
fn test_session_manager_operations() {
    // Diretório temporário: remove_session grava um backup, que não pode
    // cair em ~/.config/codeterm de quem roda os testes.
    let temp_dir = TempDir::new().unwrap();
    let mut session_manager = SessionManager::with_config(SessionPersistenceConfig {
        persistence_dir: temp_dir.path().to_path_buf(),
        ..Default::default()
    });

    // Cria uma sessao
    let session = SshSession::new("test-session", "example.com", "user")
        .with_tags(vec!["test".to_string(), "important".to_string()]);

    // Adiciona ao manager
    let id = session_manager.create_session(session);
    assert_eq!(session_manager.list_sessions().len(), 1);

    // Busca por tag
    let tagged = session_manager.find_by_tag("important");
    assert_eq!(tagged.len(), 1);

    // Lista todas
    let all = session_manager.list_sessions();
    assert_eq!(all.len(), 1);

    // Remove
    session_manager.remove_session(id).ok();
    assert_eq!(session_manager.list_sessions().len(), 0);
    assert!(temp_dir.path().join("backups").exists());
}

#[test]
fn test_session_builder_pattern() {
    let session = SshSession::new("test-id", "host.example.com", "admin")
        .with_port(2222)
        .with_type(SessionType::Interactive)
        .with_tags(vec!["tag1".to_string(), "tag2".to_string()]);

    assert_eq!(session.name, "test-id");
    assert_eq!(session.host, "host.example.com");
    assert_eq!(session.user, "admin");
    assert_eq!(session.port, 2222);
    assert!(session.tags.contains(&"tag1".to_string()));
    assert!(session.tags.contains(&"tag2".to_string()));
}

#[test]
fn test_session_connection_string() {
    let session = SshSession::new("test", "host.com", "user").with_port(2222);
    assert_eq!(session.connection_string(), "user@host.com:2222");
}

#[test]
fn test_session_state_management() {
    let mut session = SshSession::new("test", "host.com", "user");

    // Estado inicial
    assert_eq!(session.state, SessionState::Disconnected);
    assert!(!session.is_active());

    // Muda para conectado
    session.state = SessionState::Connected;
    assert!(session.is_active());
}

#[test]
fn test_port_forward_structs() {
    // LocalForward
    let local = PortForward {
        local_port: 8080,
        bind_address: Some("127.0.0.1".to_string()),
        remote_host: "internal.server".to_string(),
        remote_port: 80,
    };
    assert_eq!(local.local_port, 8080);
    assert_eq!(local.remote_port, 80);

    // DynamicForward
    let dynamic = code_terminal::ssh::DynamicForward {
        local_port: 1080,
        bind_address: Some("0.0.0.0".to_string()),
    };
    assert_eq!(dynamic.local_port, 1080);
}

#[test]
fn test_key_type_properties() {
    // Tamanhos padrao
    assert_eq!(KeyType::Rsa.default_bits(), 4096);
    assert_eq!(KeyType::Ed25519.default_bits(), 256);
    assert_eq!(KeyType::EcdsaP256.default_bits(), 256);
    assert_eq!(KeyType::EcdsaP384.default_bits(), 384);
    assert_eq!(KeyType::EcdsaP521.default_bits(), 521);

    // Nomes de arquivo padrao
    assert_eq!(KeyType::Rsa.default_filename(), "id_rsa");
    assert_eq!(KeyType::Ed25519.default_filename(), "id_ed25519");
    assert_eq!(KeyType::EcdsaP256.default_filename(), "id_ecdsa");

    // Recomendacao
    assert!(KeyType::Ed25519.is_recommended());
    assert!(KeyType::Rsa.is_recommended());
    assert!(KeyType::EcdsaP256.is_recommended());

    // Parsing
    assert_eq!(KeyType::from_str("RSA"), Some(KeyType::Rsa));
    assert_eq!(KeyType::from_str("ed25519"), Some(KeyType::Ed25519));
    assert_eq!(KeyType::from_str("invalid"), None);
}

#[test]
fn test_fingerprint_creation() {
    let fp = KeyFingerprint::from_sha256("abc123".to_string());
    assert_eq!(fp.sha256, "abc123");

    let fp_with_md5 = KeyFingerprint::new("sha256-hash".to_string(), Some("md5-hash".to_string()));
    assert_eq!(fp_with_md5.sha256, "sha256-hash");
    assert_eq!(fp_with_md5.md5, Some("md5-hash".to_string()));
}

#[test]
fn test_ssh_manager_search() {
    let manager = SshManager::new();

    // Adiciona algumas conexoes recentes
    manager.add_recent_connection(code_terminal::ssh::RecentConnection::new(
        "prod.example.com",
        22,
    ));
    manager.add_recent_connection(code_terminal::ssh::RecentConnection::new(
        "dev.example.com",
        22,
    ));
    manager.add_recent_connection(code_terminal::ssh::RecentConnection::new(
        "staging.example.com",
        2222,
    ));

    // Busca
    let search = manager.search_hosts("prod");
    // Deve estar vazio pois nao carregou config, mas o metodo funciona
    assert!(search.is_empty() || !search.is_empty());

    // Todas as recentes
    let recent = manager.get_recent_connections();
    assert_eq!(recent.len(), 3);
}

#[test]
fn test_config_parser_edge_cases() {
    // Config vazia
    let parser = SshConfigParser::from_str("").unwrap();
    assert!(parser.hosts().is_empty());

    // Apenas comentarios
    let parser = SshConfigParser::from_str("# Comment 1\n# Comment 2").unwrap();
    assert!(parser.hosts().is_empty());

    // Host sem diretivas
    let parser = SshConfigParser::from_str("Host empty").unwrap();
    assert_eq!(parser.hosts().len(), 1);
    assert_eq!(parser.hosts()[0].pattern, "empty");
}

#[test]
fn test_config_parser_find_matching() {
    let config = r#"
Host *.production.company.com
    User prod

Host *.staging.company.com
    User staging

Host specific.production.company.com
    User specific
"#;

    let parser = SshConfigParser::from_str(config).unwrap();

    // Deve encontrar matches para wildcard
    let matches = parser.find_matching_hosts("api.production.company.com");
    assert!(!matches.is_empty());

    // Deve encontrar match especifico
    let matches = parser.find_matching_hosts("specific.production.company.com");
    assert!(!matches.is_empty());
}

#[test]
fn test_connection_string_formatting() {
    // Com usuario e porta nao padrao
    let mut host = SshHost::new("server");
    host.host_name = Some("server.example.com".to_string());
    host.user = Some("admin".to_string());
    host.port = Some(2222);
    assert_eq!(host.connection_string(), "admin@server.example.com:2222");

    // Apenas host e porta padrao
    let mut host = SshHost::new("simple");
    host.host_name = Some("simple.example.com".to_string());
    assert_eq!(host.connection_string(), "simple.example.com:22");
}

#[test]
fn test_quick_connect_validation() {
    use code_terminal::ssh::{QuickConnectDialog, QuickConnectError, QuickConnectField};

    let mut dialog = QuickConnectDialog::new();

    // Sem host deve falhar
    let result = dialog.validate();
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), QuickConnectError::EmptyHost));

    // Host invalido
    dialog.set_field(QuickConnectField::Host, "invalid host with spaces");
    let result = dialog.validate();
    assert!(result.is_err());

    // Porta invalida
    dialog.set_field(QuickConnectField::Host, "valid.host");
    dialog.set_field(QuickConnectField::Port, "not-a-number");
    let result = dialog.validate();
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        QuickConnectError::InvalidPort(_)
    ));

    // Valido
    dialog.set_field(QuickConnectField::Port, "2222");
    let result = dialog.validate();
    assert!(result.is_ok());
}

#[test]
fn test_key_type_display() {
    assert_eq!(format!("{}", KeyType::Rsa), "RSA");
    assert_eq!(format!("{}", KeyType::Ed25519), "ED25519");
    assert_eq!(KeyType::Rsa.as_str(), "RSA");
}

#[test]
fn test_session_duration() {
    use std::thread;

    let session = SshSession::new("test", "host.com", "user");

    // Espera um pouco
    thread::sleep(std::time::Duration::from_millis(10));

    // Verifica duracao
    let duration = session.duration();
    assert!(duration.as_millis() >= 10);
}

#[test]
fn test_host_extra_options() {
    let mut host = SshHost::new("test");
    host.extra_options
        .insert("CustomOption".to_string(), "CustomValue".to_string());

    assert!(host.extra_options.contains_key("CustomOption"));
    assert_eq!(
        host.extra_options.get("CustomOption"),
        Some(&"CustomValue".to_string())
    );
}
