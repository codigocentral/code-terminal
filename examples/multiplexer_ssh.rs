//! Exemplo de integração SSH com multiplexador
//!
//! Demonstra como criar sessões SSH remotas com funcionalidades
//! de multiplexador.

use code_terminal::multiplexer::ssh_compat::PortForwardType;
use code_terminal::multiplexer::{
    AuthMethod, ClientPermissions, PortForwardConfig, SessionId, SharedSessionConfig,
    SshCompatConfig, SshCompatManager, SshCompatibleSession, SshTransport,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔐 Exemplo de Integração SSH\n");

    // ========================================
    // 1. Criar sessão SSH básica
    // ========================================
    println!("1. Criando sessão SSH básica...");

    let session_id = SessionId::new();
    let config = SshCompatConfig::default();

    let session = SshCompatibleSession::new(session_id, "server.example.com", "user", config);

    println!("   ✓ Sessão criada: {}", session_id);
    println!("   ✓ Host: {}", session.ssh_info.host);
    println!("   ✓ User: {}", session.ssh_info.user);
    println!("   ✓ Port: {}", session.ssh_info.port);
    println!("   ✓ Conectado: {}\n", session.ssh_info.is_connected);

    // ========================================
    // 2. Configurar autenticação
    // ========================================
    println!("2. Configurando autenticação...");

    // Com agente SSH
    let session2 = SshCompatibleSession::new(
        SessionId::new(),
        "server2.example.com",
        "user",
        SshCompatConfig::default(),
    )
    .with_auth_method(AuthMethod::Agent);

    println!("   ✓ Método: {}", session2.ssh_info.auth_method);

    // Com chave pública
    let session3 = SshCompatibleSession::new(
        SessionId::new(),
        "server3.example.com",
        "user",
        SshCompatConfig::default(),
    )
    .with_auth_method(AuthMethod::PublicKey {
        key_path: std::path::PathBuf::from("~/.ssh/id_ed25519"),
    });

    println!("   ✓ Método: {}\n", session3.ssh_info.auth_method);

    // ========================================
    // 3. Configurar transporte
    // ========================================
    println!("3. Configurando transporte...");

    // Conexão direta
    let direct = SshCompatibleSession::new(
        SessionId::new(),
        "server.example.com",
        "user",
        SshCompatConfig::default(),
    )
    .with_transport(SshTransport::Direct);
    println!("   ✓ Transporte: {}", direct.ssh_info.transport);

    // Via jump host
    let jump = SshCompatibleSession::new(
        SessionId::new(),
        "internal.server.com",
        "user",
        SshCompatConfig::default(),
    )
    .with_transport(SshTransport::ProxyJump)
    .with_port(22);
    println!("   ✓ Transporte: {}", jump.ssh_info.transport);

    // Via SOCKS
    let socks = SshCompatibleSession::new(
        SessionId::new(),
        "remote.server.com",
        "user",
        SshCompatConfig::default(),
    )
    .with_transport(SshTransport::SocksProxy);
    println!("   ✓ Transporte: {}\n", socks.ssh_info.transport);

    // ========================================
    // 4. Gerenciador SSH
    // ========================================
    println!("4. Usando SshCompatManager...");

    let mut manager = SshCompatManager::new();

    // Criar múltiplas sessões
    let prod_id = SessionId::new();
    let staging_id = SessionId::new();
    let dev_id = SessionId::new();

    manager.create_session(prod_id, "prod.example.com", "deploy");
    manager.create_session(staging_id, "staging.example.com", "deploy");
    manager.create_session(dev_id, "dev.example.com", "developer");

    println!("   ✓ Criadas 3 sessões SSH");
    println!("   ✓ Total de sessões: {}\n", manager.list_sessions().len());

    // ========================================
    // 5. Conectar e desconectar
    // ========================================
    println!("5. Conectando sessões...");

    manager.connect(prod_id)?;
    println!("   ✓ Produção conectada");

    manager.connect(staging_id)?;
    println!("   ✓ Staging conectada");

    println!("   ✓ Conectadas: {}", manager.list_connected().len());

    manager.disconnect(prod_id)?;
    println!("   ✓ Produção desconectada");
    println!("   ✓ Conectadas: {}\n", manager.list_connected().len());

    // ========================================
    // 6. Estatísticas
    // ========================================
    println!("6. Estatísticas...");

    let stats = manager.stats();
    println!("   ✓ Total de sessões: {}", stats.total_sessions);
    println!("   ✓ Sessões conectadas: {}", stats.connected_sessions);
    println!("   ✓ Bytes enviados: {}", stats.total_bytes_sent);
    println!("   ✓ Bytes recebidos: {}\n", stats.total_bytes_received);

    // ========================================
    // 7. Port Forwarding
    // ========================================
    println!("7. Configurando port forwarding...");

    if let Some(sess) = manager.get_session_mut(staging_id) {
        // Forward local
        let local_forward = PortForwardConfig {
            forward_type: PortForwardType::Local,
            local_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 80,
        };
        sess.setup_port_forward(local_forward)?;
        println!("   ✓ Forward local: 8080 -> localhost:80");

        // SOCKS proxy
        sess.create_socks_proxy(1080)?;
        println!("   ✓ SOCKS proxy na porta 1080\n");
    }

    // ========================================
    // 8. Sessão compartilhada via SSH
    // ========================================
    println!("8. Criando sessão compartilhada via SSH...");

    let shared_ssh_id = SessionId::new();
    let shared_config = SharedSessionConfig::default()
        .with_max_clients(3)
        .with_default_permissions(ClientPermissions::read_only())
        .with_password("team-secret");

    manager.create_shared_ssh_session(
        shared_ssh_id,
        "shared.example.com",
        "team",
        shared_config,
    )?;

    println!("   ✓ Sessão SSH compartilhada criada");
    println!("   ✓ Máximo de clientes: 3");
    println!("   ✓ Protegida por senha: true\n");

    // ========================================
    // 9. Configuração personalizada
    // ========================================
    println!("9. Configuração personalizada...");

    let custom_config = SshCompatConfig::default();

    // Simulando configuração
    println!("   ✓ Usar ~/.ssh/config: {}", custom_config.use_ssh_config);
    println!("   ✓ Usar SSH agent: {}", custom_config.use_ssh_agent);
    println!("   ✓ Timeout: {}s", custom_config.connection_timeout);
    println!("   ✓ Keepalive: {}s", custom_config.keepalive_interval);
    println!(
        "   ✓ Strict host key checking: {}\n",
        custom_config.strict_host_key_checking
    );

    // ========================================
    // 10. Resolver host de ~/.ssh/config
    // ========================================
    println!("10. Resolvendo hosts...");

    // Em produção, isso buscaria no ~/.ssh/config
    println!("   ✓ 'myserver' -> Resolvido de ~/.ssh/config (simulado)");
    println!("   ✓ 'production' -> Resolvido de ~/.ssh/config (simulado)");
    println!("   ✓ '192.168.1.100' -> Usado diretamente\n");

    // ========================================
    // 11. Limpeza
    // ========================================
    println!("11. Limpando sessões desconectadas...");

    let removed = manager.cleanup_disconnected();
    println!("   ✓ Removidas {} sessões desconectadas", removed);
    println!(
        "   ✓ Sessões restantes: {}\n",
        manager.list_sessions().len()
    );

    println!("✅ Exemplo concluído com sucesso!");

    Ok(())
}
