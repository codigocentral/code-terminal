//! Exemplo de sessões compartilhadas
//!
//! Demonstra como criar e gerenciar sessões compartilhadas
//! com múltiplos clientes.

use code_terminal::multiplexer::{
    ClientId, ClientPermissions, InputData, SessionId, SharedSession, SharedSessionConfig,
    SharedSessionManager, SyncMode,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("👥 Exemplo de Sessões Compartilhadas\n");

    // ========================================
    // 1. Criar sessão compartilhada
    // ========================================
    println!("1. Criando sessão compartilhada...");

    let session_id = SessionId::new();
    let config = SharedSessionConfig::default()
        .with_max_clients(5)
        .with_sync_mode(SyncMode::Independent);

    let session = SharedSession::new(session_id, config);
    println!("   ✓ Sessão criada: {}", session_id);
    println!("   ✓ Capacidade: {} clientes", session.config.max_clients);
    println!("   ✓ Modo: {}\n", session.config.sync_mode);

    // ========================================
    // 2. Clientes entrando na sessão
    // ========================================
    println!("2. Clientes entrando na sessão...");

    let client1 = ClientId::new();
    let client2 = ClientId::new();
    let client3 = ClientId::new();

    let perms1 = session.join(client1, None)?;
    println!("   ✓ Cliente 1 entrou: {}", client1);
    println!(
        "     - Write: {}, Read-Only: {}",
        perms1.write, perms1.read_only
    );

    let _perms2 = session.join(client2, None)?;
    println!("   ✓ Cliente 2 entrou: {}", client2);

    let _perms3 = session.join(client3, None)?;
    println!("   ✓ Cliente 3 entrou: {}", client3);
    println!("   ✓ Total de clientes: {}\n", session.client_count());

    // ========================================
    // 3. Enviar input
    // ========================================
    println!("3. Enviando input...");

    let input = InputData::Keyboard {
        bytes: vec![0x48, 0x65, 0x6C, 0x6C, 0x6F], // "Hello"
    };
    session.send_input(client1, input)?;
    println!("   ✓ Cliente 1 enviou input");

    let events = session.drain_input();
    println!("   ✓ Input recebido: {} eventos\n", events.len());

    // ========================================
    // 4. Gerenciamento de permissões
    // ========================================
    println!("4. Gerenciando permissões...");

    // Promover cliente 2 para admin
    session.update_permissions(client2, ClientPermissions::admin())?;
    println!("   ✓ Cliente 2 promovido a admin");

    // Colocar cliente 3 em modo read-only
    session.update_permissions(client3, ClientPermissions::read_only())?;
    println!("   ✓ Cliente 3 definido como read-only");

    // Verificar permissões
    println!(
        "   ✓ Cliente 1 pode escrever: {}",
        session.has_permission(client1, |p| p.write)
    );
    println!(
        "   ✓ Cliente 3 pode escrever: {}\n",
        session.has_permission(client3, |p| p.write)
    );

    // ========================================
    // 5. Cliente 3 tenta escrever (vai falhar)
    // ========================================
    println!("5. Testando permissões...");

    let input = InputData::Keyboard { bytes: vec![0x41] };
    match session.send_input(client3, input) {
        Ok(_) => println!("   ✗ Cliente 3 conseguiu escrever (inesperado)"),
        Err(_) => println!("   ✓ Cliente 3 não conseguiu escrever (esperado)\n"),
    }

    // ========================================
    // 6. Sessão com senha
    // ========================================
    println!("6. Criando sessão com senha...");

    let secure_session_id = SessionId::new();
    let secure_config = SharedSessionConfig::default().with_password("secret123");

    let secure_session = SharedSession::new(secure_session_id, secure_config);
    println!("   ✓ Sessão segura criada");
    println!("   ✓ Protegida por senha: true");

    let client4 = ClientId::new();

    // Tentar sem senha (deve falhar)
    match secure_session.join(client4, None) {
        Ok(_) => println!("   ✗ Entrou sem senha (inesperado)"),
        Err(_) => println!("   ✓ Rejeitado sem senha (esperado)"),
    }

    // Tentar com senha errada (deve falhar)
    match secure_session.join(client4, Some("wrong")) {
        Ok(_) => println!("   ✗ Entrou com senha errada (inesperado)"),
        Err(_) => println!("   ✓ Rejeitado com senha errada (esperado)"),
    }

    // Tentar com senha correta (deve funcionar)
    match secure_session.join(client4, Some("secret123")) {
        Ok(_) => println!("   ✓ Entrou com senha correta (esperado)\n"),
        Err(_) => println!("   ✗ Falhou com senha correta (inesperado)\n"),
    }

    // ========================================
    // 7. Gerenciador de sessões compartilhadas
    // ========================================
    println!("7. Usando SharedSessionManager...");

    let mut manager = SharedSessionManager::new();

    // Criar múltiplas sessões
    let session_a = SessionId::new();
    let session_b = SessionId::new();

    manager.create_shared_session(session_a, SharedSessionConfig::default());
    manager.create_shared_session(session_b, SharedSessionConfig::default());
    println!("   ✓ Criadas 2 sessões no gerenciador");

    // Conectar clientes
    let user1 = ClientId::new();
    let user2 = ClientId::new();

    manager.connect_client(user1, session_a, None)?;
    manager.connect_client(user2, session_b, None)?;
    println!("   ✓ User1 conectado à sessão A");
    println!("   ✓ User2 conectado à sessão B");

    // Verificar conexões
    println!(
        "   ✓ User1 conectado: {}",
        manager.is_client_connected(user1)
    );
    println!(
        "   ✓ User2 conectado: {}",
        manager.is_client_connected(user2)
    );
    println!("   ✓ Total de sessões: {}\n", manager.session_count());

    // ========================================
    // 8. Informações da sessão
    // ========================================
    println!("8. Informações da sessão...");

    let info = session.info();
    println!("   ✓ ID: {}", info.session_id);
    println!("   ✓ Clientes: {}/{}", info.client_count, info.max_clients);
    println!("   ✓ Modo: {}", info.sync_mode);
    println!("   ✓ Criada em: {}", info.created_at);
    println!("   ✓ Última atividade: {}\n", info.last_activity);

    // ========================================
    // 9. Clientes saindo
    // ========================================
    println!("9. Clientes saindo...");

    session.leave(client1)?;
    println!("   ✓ Cliente 1 saiu");

    session.leave(client2)?;
    println!("   ✓ Cliente 2 saiu");

    session.leave(client3)?;
    println!("   ✓ Cliente 3 saiu");

    println!("   ✓ Clientes restantes: {}\n", session.client_count());

    // ========================================
    // 10. Listar todas as sessões
    // ========================================
    println!("10. Listando todas as sessões...");

    let sessions = manager.list_sessions();
    for (i, info) in sessions.iter().enumerate() {
        println!(
            "   ✓ Sessão {}: {} clientes conectados",
            i + 1,
            info.client_count
        );
    }

    println!("\n✅ Exemplo concluído com sucesso!");

    Ok(())
}
