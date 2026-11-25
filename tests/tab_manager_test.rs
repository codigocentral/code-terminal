//! Testes de Integração - TabManager
//!
//! Este módulo testa o gerenciamento de tabs do code-terminal,
//! incluindo criação, fechamento, navegação e atualização de títulos.
//!
//! # Execução
//!
//! ```bash
//! cargo test --test tab_manager_test
//! ```

use code_terminal::terminal::buffer::TerminalBuffer;
use code_terminal::ui::tabs::{Tab, TabCloseResult, TabId, TabManager};

/// Setup: Cria um TabManager para testes
fn setup_manager() -> TabManager {
    TabManager::new()
}

/// Teste 1: Criação básica do TabManager
///
/// Verifica que o TabManager é criado corretamente e inicia sem tabs.
#[test]
fn test_tab_manager_creation() {
    let manager = setup_manager();

    assert_eq!(manager.tab_count(), 0, "TabManager deve iniciar sem tabs");
    assert!(
        manager.get_active_tab().is_none(),
        "Não deve haver tab ativa inicialmente"
    );
}

/// Teste 2: Criação de tabs
///
/// Verifica que tabs podem ser criadas e recebem IDs únicos.
#[test]
fn test_create_tabs() {
    let mut manager = setup_manager();

    let id1 = manager.create_tab("bash");
    assert_eq!(manager.tab_count(), 1, "Deve haver 1 tab");
    assert_eq!(
        manager.get_active_tab().unwrap().id,
        id1,
        "Primeira tab deve estar ativa"
    );

    let id2 = manager.create_tab("zsh");
    assert_eq!(manager.tab_count(), 2, "Deve haver 2 tabs");

    // IDs devem ser únicos
    assert_ne!(id1, id2, "IDs devem ser únicos");

    // Tab ativa não deve mudar automaticamente
    assert_eq!(
        manager.get_active_tab().unwrap().id,
        id1,
        "Tab ativa deve permanecer a mesma"
    );
}

/// Teste 3: Troca de tab ativa
///
/// Verifica que podemos mudar a tab ativa corretamente.
#[test]
fn test_switch_tab() {
    let mut manager = setup_manager();
    let id1 = manager.create_tab("bash");
    let _id2 = manager.create_tab("zsh");
    let id3 = manager.create_tab("fish");

    // Inicialmente a primeira está ativa
    assert_eq!(manager.active_tab_index(), 0);

    // Troca para tab 2 (índice 1)
    manager.switch_tab(1);
    assert_eq!(manager.active_tab_index(), 1);
    assert!(manager.get_active_tab().unwrap().is_active());

    // Troca para tab 3 (índice 2)
    manager.switch_tab(2);
    assert_eq!(manager.active_tab_index(), 2);
    assert_eq!(manager.get_active_tab().unwrap().id, id3);

    // Troca de volta para tab 1
    manager.switch_tab(0);
    assert_eq!(manager.active_tab_index(), 0);
    assert_eq!(manager.get_active_tab().unwrap().id, id1);
}

/// Teste 4: Navegação entre tabs (next/previous)
///
/// Verifica navegação circular entre tabs.
#[test]
fn test_next_previous_tab() {
    let mut manager = setup_manager();
    manager.create_tab("bash");
    manager.create_tab("zsh");
    manager.create_tab("fish");

    assert_eq!(manager.active_tab_index(), 0);

    // next_tab avança
    manager.next_tab();
    assert_eq!(manager.active_tab_index(), 1);

    manager.next_tab();
    assert_eq!(manager.active_tab_index(), 2);

    // Circular: deve voltar para 0
    manager.next_tab();
    assert_eq!(manager.active_tab_index(), 0);

    // previous_tab volta
    manager.previous_tab();
    assert_eq!(manager.active_tab_index(), 2); // Circular

    manager.previous_tab();
    assert_eq!(manager.active_tab_index(), 1);
}

/// Teste 5: Fechamento de tab
///
/// Verifica que tabs podem ser fechadas e a navegação é ajustada.
#[test]
fn test_close_tab() {
    let mut manager = setup_manager();
    let _id1 = manager.create_tab("bash");
    let _id2 = manager.create_tab("zsh");
    let _id3 = manager.create_tab("fish");

    assert_eq!(manager.tab_count(), 3);

    // Fecha a tab do meio (índice 1)
    assert!(manager.close_tab(1), "Deve conseguir fechar tab");
    assert_eq!(manager.tab_count(), 2, "Deve restar 2 tabs");

    // Fecha a primeira tab (índice 0)
    assert!(manager.close_tab(0));
    assert_eq!(manager.tab_count(), 1);

    // Índice inválido deve retornar false
    assert!(!manager.close_tab(5), "Índice inválido deve retornar false");
}

/// Teste 6: Fechamento da tab ativa
///
/// Verifica comportamento ao fechar a tab atualmente ativa.
#[test]
fn test_close_active_tab() {
    let mut manager = setup_manager();
    let _id1 = manager.create_tab("bash");
    let _id2 = manager.create_tab("zsh");
    let _id3 = manager.create_tab("fish");

    // Ativa a última tab
    manager.switch_tab(2);
    assert_eq!(manager.active_tab_index(), 2);

    // Fecha a tab ativa
    assert!(manager.close_active_tab());

    // Deve ter ajustado para uma tab válida
    assert!(manager.active_tab_index() < manager.tab_count());
    assert_eq!(manager.tab_count(), 2);
}

/// Teste 7: Fechamento de todas as tabs
///
/// Verifica que close_tab retorna true quando a última tab é fechada.
#[test]
fn test_close_all_tabs() {
    let mut manager = setup_manager();
    manager.create_tab("bash");
    manager.create_tab("zsh");

    // Fecha todas
    assert!(manager.close_tab(0));
    assert!(manager.close_tab(0));

    // Não há mais tabs
    assert_eq!(manager.tab_count(), 0);
    assert!(manager.get_active_tab().is_none());

    // Tentar fechar quando vazio
    assert!(!manager.close_active_tab());
}

/// Teste 8: Atualização de título da tab
///
/// Verifica que o título pode ser atualizado com base no processo.
#[test]
fn test_update_tab_title() {
    let mut manager = setup_manager();
    let _id1 = manager.create_tab("bash");

    // Título inicial deve ser o shell
    let initial_title = manager.get_active_tab().unwrap().title();
    assert!(
        initial_title.contains("bash"),
        "Título deve conter o shell inicial"
    );

    // Atualiza título
    manager.update_title(0, "vim");
    let updated_title = manager.get_tab(0).unwrap().title();
    assert!(
        updated_title.contains("vim"),
        "Título deve refletir o processo atual"
    );
}

/// Teste 9: Atualização de diretório de trabalho
///
/// Verifica que o diretório de trabalho é atualizado corretamente.
#[test]
fn test_update_working_dir() {
    let mut manager = setup_manager();
    manager.create_tab("bash");

    // Atualiza diretório
    manager.update_working_dir(0, "/home/user/projects");

    let tab = manager.get_tab(0).unwrap();
    assert_eq!(tab.working_dir(), Some("/home/user/projects"));
}

/// Teste 10: Estado da tab
///
/// Verifica transições de estado da tab (Active/Inactive/Closing).
#[test]
fn test_tab_state() {
    let mut manager = setup_manager();
    manager.create_tab("bash");
    manager.create_tab("zsh");

    // Primeira tab ativa, segunda inativa
    assert!(manager.get_tab(0).unwrap().is_active());
    assert!(!manager.get_tab(1).unwrap().is_active());

    // Troca
    manager.switch_tab(1);
    assert!(!manager.get_tab(0).unwrap().is_active());
    assert!(manager.get_tab(1).unwrap().is_active());
}

/// Teste 11: Verificação de processo rodando
///
/// Verifica detecção de processos em execução.
#[test]
fn test_has_running_process() {
    let mut manager = setup_manager();
    manager.create_tab("bash");

    // Sem buffer, não deve ter processo rodando
    assert!(!manager.active_tab_has_running_process());
    assert!(!manager.has_running_process(0));

    let buffer = TerminalBuffer::new(80, 24).expect("test buffer");
    manager.create_tab_with_buffer("bash", buffer);

    assert!(manager.has_running_process(1));

    manager.switch_tab(1);
    assert!(manager.active_tab_has_running_process());
}

/// Teste 12: Try close com confirmação
///
/// Verifica que tentar fechar com processo rodando retorna NeedsConfirmation.
#[cfg(unix)]
#[test]
fn test_try_close_needs_confirmation() {
    let mut manager = setup_manager();
    manager.create_tab("bash");

    // Sem processo rodando, deve fechar normalmente
    let result = manager.try_close_active_tab();
    assert_eq!(result, TabCloseResult::AllTabsClosed);

    // Shell parado não pede confirmação: só fecha.
    let idle = TerminalBuffer::with_command(
        80,
        24,
        "/bin/bash",
        &["--norc".to_string(), "-i".to_string()],
        &Default::default(),
    )
    .expect("test buffer");
    manager.create_tab_with_buffer("bash", idle);
    // Espera o bash terminar de subir (até lá ele ainda roda coisas em primeiro plano).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while manager.get_tab(0).unwrap().busy_process().is_some()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        manager.try_close_active_tab(),
        TabCloseResult::AllTabsClosed
    );

    // Com um programa em primeiro plano, pede.
    let mut busy = TerminalBuffer::with_command(
        80,
        24,
        "/bin/bash",
        &["--norc".to_string(), "-i".to_string()],
        &Default::default(),
    )
    .expect("test buffer");
    busy.write_str("sleep 30\n").unwrap();
    manager.create_tab_with_buffer("bash", busy);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while manager.get_tab(0).unwrap().busy_process().is_none()
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let result = manager.try_close_active_tab();
    assert_eq!(result, TabCloseResult::NeedsConfirmation(0));
}

/// Teste 13: Criação de tab com buffer
///
/// Verifica criação de tab com TerminalBuffer.
#[test]
fn test_create_tab_with_buffer() {
    let mut manager = setup_manager();

    // Cria um buffer com PTY real para teste
    let buffer = TerminalBuffer::new(80, 24).expect("test buffer");

    let id = manager.create_tab_with_buffer("bash", buffer);
    assert_eq!(manager.tab_count(), 1);

    let tab = manager.get_tab(0).unwrap();
    assert!(tab.buffer.is_some(), "Tab deve ter buffer");
    assert_eq!(tab.id, id);
}

/// Teste 14: Estados de fechamento de tab
///
/// Verifica variantes do TabCloseResult.
#[test]
fn test_tab_close_result_variants() {
    // Testa igualdade
    assert_eq!(TabCloseResult::Closed, TabCloseResult::Closed);
    assert_eq!(TabCloseResult::AllTabsClosed, TabCloseResult::AllTabsClosed);
    assert_eq!(TabCloseResult::NoTabs, TabCloseResult::NoTabs);
    assert_eq!(
        TabCloseResult::NeedsConfirmation(0),
        TabCloseResult::NeedsConfirmation(0)
    );

    // Testa desigualdade
    assert_ne!(TabCloseResult::Closed, TabCloseResult::NoTabs);
    assert_ne!(
        TabCloseResult::NeedsConfirmation(0),
        TabCloseResult::NeedsConfirmation(1)
    );

    // Testa Debug
    assert_eq!(format!("{:?}", TabCloseResult::Closed), "Closed");
    assert_eq!(
        format!("{:?}", TabCloseResult::AllTabsClosed),
        "AllTabsClosed"
    );
    assert_eq!(format!("{:?}", TabCloseResult::NoTabs), "NoTabs");
    assert_eq!(
        format!("{:?}", TabCloseResult::NeedsConfirmation(5)),
        "NeedsConfirmation(5)"
    );
}

/// Teste 15: Tab individual
///
/// Verifica métodos da struct Tab.
#[test]
fn test_tab_methods() {
    let mut tab = Tab::new("bash");

    // Título inicial
    let title = tab.title();
    assert!(title.contains("bash"));

    // Processo
    tab.set_process("vim");
    assert!(tab.title().contains("vim"));

    // Limpa processo
    tab.clear_process();
    assert!(!tab.title().contains("vim"));

    // Diretório
    tab.set_working_dir("/tmp");
    assert_eq!(tab.working_dir(), Some("/tmp"));

    // Estado
    tab.set_active(true);
    assert!(tab.is_active());
    tab.set_active(false);
    assert!(!tab.is_active());
}

/// Teste 16: Acesso a tabs por índice
///
/// Verifica get_tab e get_tab_mut.
#[test]
fn test_tab_access_by_index() {
    let mut manager = setup_manager();
    let id1 = manager.create_tab("bash");
    let id2 = manager.create_tab("zsh");

    // Acesso imutável
    assert_eq!(manager.get_tab(0).unwrap().id, id1);
    assert_eq!(manager.get_tab(1).unwrap().id, id2);
    assert!(manager.get_tab(5).is_none());

    // Acesso mutável
    manager.get_tab_mut(0).unwrap().set_process("vim");
    assert!(manager.get_tab(0).unwrap().title().contains("vim"));
}

/// Teste 17: Iteração sobre tabs
///
/// Verifica tabs() e tabs_mut.
#[test]
fn test_tabs_iteration() {
    let mut manager = setup_manager();
    manager.create_tab("bash");
    manager.create_tab("zsh");
    manager.create_tab("fish");

    // Iteração imutável
    let titles: Vec<String> = manager.tabs().iter().map(|t| t.title()).collect();
    assert_eq!(titles.len(), 3);

    // Iteração mutável
    for tab in manager.tabs_mut() {
        tab.set_process("test");
    }

    for tab in manager.tabs() {
        assert!(tab.title().contains("test"));
    }
}

/// Teste 18: TabManager Default
///
/// Verifica implementação de Default.
#[test]
fn test_tab_manager_default() {
    let manager: TabManager = Default::default();
    assert_eq!(manager.tab_count(), 0);
}

/// Teste 19: IDs sequenciais de tabs
///
/// Verifica que IDs são atribuídos sequencialmente.
#[test]
fn test_tab_id_sequence() {
    let mut manager = setup_manager();

    let ids: Vec<TabId> = (0..10).map(|_| manager.create_tab("bash")).collect();

    // IDs devem ser únicos
    let unique_count = ids.iter().collect::<std::collections::HashSet<_>>().len();
    assert_eq!(unique_count, 10);

    // IDs devem ser crescentes
    for i in 1..ids.len() {
        assert!(ids[i] > ids[i - 1], "IDs devem ser crescentes");
    }
}

/// Teste 20: Comportamento após fechar tab do meio
///
/// Verifica que índices são ajustados corretamente.
#[test]
fn test_close_middle_tab() {
    let mut manager = setup_manager();
    let id1 = manager.create_tab("bash");
    let _id2 = manager.create_tab("zsh");
    let id3 = manager.create_tab("fish");

    // Fecha a do meio
    manager.close_tab(1);

    // Agora temos 2 tabs: id1 e id3
    assert_eq!(manager.tab_count(), 2);
    assert_eq!(manager.get_tab(0).unwrap().id, id1);
    assert_eq!(manager.get_tab(1).unwrap().id, id3);
}

/// Teste 21: Grid access via tab
///
/// Verifica acesso ao grid através da tab.
#[test]
fn test_tab_grid_access() {
    let mut manager = setup_manager();

    // Tab sem buffer
    let _id1 = manager.create_tab("bash");
    assert!(manager.get_tab(0).unwrap().grid().is_none());

    // Tab com buffer
    let buffer = TerminalBuffer::new(80, 24).expect("test buffer");
    let _id2 = manager.create_tab_with_buffer("zsh", buffer);
    assert!(manager.get_tab(1).unwrap().grid().is_some());
}

/// Teste 22: Debug formatting
///
/// Verifica saída de Debug para tabs.
#[test]
fn test_tab_debug_format() {
    let mut manager = setup_manager();
    manager.create_tab("bash");

    let debug_str = format!("{:?}", manager.get_tab(0).unwrap());
    assert!(debug_str.contains("Tab"));
    assert!(debug_str.contains("id"));
    assert!(debug_str.contains("title"));
    assert!(debug_str.contains("state"));
}
