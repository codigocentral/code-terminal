//! Exemplo básico do modo multiplexador
//!
//! Demonstra criação de sessões, janelas e navegação básica.

use code_terminal::multiplexer::{CommandParser, MultiplexerSession, WindowLayout, WindowManager};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Exemplo Básico do Multiplexer\n");

    // ========================================
    // 1. Criar uma sessão
    // ========================================
    println!("1. Criando sessão 'development'...");
    let session = MultiplexerSession::new(Some("development".to_string()), (80, 24));

    println!("   ✓ Sessão criada: {} ({})", session.name, session.id);
    println!("   ✓ Tamanho: {}x{}", session.size.0, session.size.1);
    println!("   ✓ Shell: {}\n", session.shell);

    // ========================================
    // 2. Criar janelas
    // ========================================
    println!("2. Criando janelas...");
    let mut windows = WindowManager::new();

    // Janela 1: Editor
    let editor_window = windows.create_window(Some("editor".to_string()), (80, 24));
    println!("   ✓ Janela 'editor' criada: {}", editor_window);

    // Janela 2: Terminal
    let terminal_window = windows.create_window(Some("terminal".to_string()), (80, 24));
    println!("   ✓ Janela 'terminal' criada: {}", terminal_window);

    // Janela 3: Logs
    let logs_window = windows.create_window(Some("logs".to_string()), (80, 24));
    println!("   ✓ Janela 'logs' criada: {}\n", logs_window);

    // ========================================
    // 3. Navegar entre janelas
    // ========================================
    println!("3. Navegando entre janelas...");

    // Seleciona janela 2
    windows.select_window(terminal_window)?;
    println!(
        "   ✓ Janela ativa: {}",
        windows
            .active_window()
            .map(|w| &w.name)
            .unwrap_or(&"?".to_string())
    );

    // Próxima janela
    windows.select_next();
    println!(
        "   ✓ Próxima janela: {}",
        windows
            .active_window()
            .map(|w| &w.name)
            .unwrap_or(&"?".to_string())
    );

    // Janela anterior
    windows.select_previous();
    println!(
        "   ✓ Janela anterior: {}\n",
        windows
            .active_window()
            .map(|w| &w.name)
            .unwrap_or(&"?".to_string())
    );

    // ========================================
    // 4. Criar painéis
    // ========================================
    println!("4. Criando painéis na janela ativa...");

    if let Some(window) = windows.active_window_mut() {
        // Divide painel verticalmente (30% / 70%)
        let pane1 = window.split_active_pane(true, Some(30))?;
        println!("   ✓ Painel dividido (30%): {}", pane1);

        // Divide novamente horizontalmente
        let pane2 = window.split_active_pane(false, Some(50))?;
        println!("   ✓ Painel dividido (50%): {}", pane2);

        println!("   ✓ Total de painéis: {}\n", window.pane_count());
    }

    // ========================================
    // 5. Aplicar layouts
    // ========================================
    println!("5. Aplicando layouts...");

    let layouts = vec![
        WindowLayout::EvenHorizontal,
        WindowLayout::EvenVertical,
        WindowLayout::Tiled,
        WindowLayout::MainHorizontal,
        WindowLayout::MainVertical,
    ];

    for layout in layouts {
        if let Some(window) = windows.active_window_mut() {
            window.set_layout(layout);
            println!("   ✓ Layout aplicado: {}", layout);
        }
    }
    println!();

    // ========================================
    // 6. Comandos
    // ========================================
    println!("6. Parse de comandos...");

    let commands = vec![
        "new-session -s mysession",
        "new-window -n editor vim",
        "split-window -v -p 30",
        "select-pane -U",
        "rename-window coding",
        "list-panes",
    ];

    for cmd_str in commands {
        match CommandParser::parse(cmd_str) {
            Ok(cmd) => println!("   ✓ '{}' -> {:?}", cmd_str, std::mem::discriminant(&cmd)),
            Err(e) => println!("   ✗ '{}' -> Error: {}", cmd_str, e),
        }
    }
    println!();

    // ========================================
    // 7. Estatísticas
    // ========================================
    println!("7. Estatísticas...");
    println!("   ✓ Total de janelas: {}", windows.window_count());
    println!("   ✓ Sessão: {}", session.name);
    println!("   ✓ Criada em: {}", session.created_at);
    println!("   ✓ Idade: {:?}", session.age());

    println!("\n✅ Exemplo concluído com sucesso!");

    Ok(())
}
