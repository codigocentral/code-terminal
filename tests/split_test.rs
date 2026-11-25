//! Testes de Integração - Sistema de Splits
//!
//! Este módulo testa o sistema de divisão de tela em múltiplos panes,
//! incluindo criação de splits horizontal/vertical, navegação,
//! redimensionamento e fechamento.
//!
//! # Execução
//!
//! ```bash
//! cargo test --test split_test
//! ```

use code_terminal::ui::layout::{LayoutManager, Rect, ResizeDirection, SplitDirection};
use code_terminal::ui::splits::{
    action_to_split_op, generate_split_render_info, NavigationDirection, SplitManager,
    SplitOperation, SplitTree,
};
use code_terminal::ui::tabs::TabManager;

/// Setup: Cria um TabManager para testes
fn setup_tab_manager() -> TabManager {
    TabManager::new()
}

/// Teste 1: Criação básica do SplitManager
///
/// Verifica que o SplitManager é criado corretamente e inicia sem panes.
#[test]
fn test_split_manager_creation() {
    let manager = SplitManager::new(800, 600);

    assert_eq!(
        manager.pane_count(),
        0,
        "SplitManager deve iniciar sem panes"
    );
    assert!(
        manager.active_pane().is_none(),
        "Não deve haver pane ativo inicialmente"
    );
}

/// Teste 2: Criação do pane raiz
///
/// Verifica que o primeiro pane é criado corretamente como raiz.
#[test]
fn test_create_root_pane() {
    let mut manager = SplitManager::new(800, 600);
    let tab_manager = setup_tab_manager();

    let root_id = manager.create_root_pane(tab_manager);

    assert_eq!(manager.pane_count(), 1, "Deve haver exatamente 1 pane");
    assert_eq!(
        manager.active_pane().unwrap().id,
        root_id,
        "Pane raiz deve estar ativo"
    );
}

/// Teste 3: Split horizontal
///
/// Verifica que um split horizontal divide a altura do pane.
#[test]
fn test_split_horizontal() {
    let mut manager = SplitManager::new(800, 600);
    let _root_id = manager.create_root_pane(setup_tab_manager());

    let new_id = manager.split_horizontal(setup_tab_manager());

    assert!(new_id.is_some(), "Split horizontal deve criar novo pane");
    assert_eq!(manager.pane_count(), 2, "Deve haver 2 panes após split");

    // Verifica geometria
    let panes = manager.panes();
    let pane1 = panes.get(&1).unwrap();
    let pane2 = panes.get(&new_id.unwrap()).unwrap();

    // Ambos devem ter mesma largura
    assert_eq!(
        pane1.rect.width, pane2.rect.width,
        "Panes devem ter mesma largura"
    );

    // Alturas devem somar altura total
    assert_eq!(
        pane1.rect.height + pane2.rect.height,
        600,
        "Alturas devem somar altura total"
    );
}

/// Teste 4: Split vertical
///
/// Verifica que um split vertical divide a largura do pane.
#[test]
fn test_split_vertical() {
    let mut manager = SplitManager::new(800, 600);
    let _root_id = manager.create_root_pane(setup_tab_manager());

    let new_id = manager.split_vertical(setup_tab_manager());

    assert!(new_id.is_some(), "Split vertical deve criar novo pane");
    assert_eq!(manager.pane_count(), 2, "Deve haver 2 panes após split");

    // Verifica geometria
    let panes = manager.panes();
    let pane1 = panes.get(&1).unwrap();
    let pane2 = panes.get(&new_id.unwrap()).unwrap();

    // Ambos devem ter mesma altura
    assert_eq!(
        pane1.rect.height, pane2.rect.height,
        "Panes devem ter mesma altura"
    );

    // Larguras devem somar largura total
    assert_eq!(
        pane1.rect.width + pane2.rect.width,
        800,
        "Larguras devem somar largura total"
    );
}

/// Teste 5: Navegação entre panes com Alt+Arrow
///
/// Verifica navegação nas quatro direções (up, down, left, right).
#[test]
fn test_navigate_between_panes() {
    let mut manager = SplitManager::new(800, 600);
    let id1 = manager.create_root_pane(setup_tab_manager());
    let id2 = manager.split_vertical(setup_tab_manager()).unwrap();

    // Pane 2 deve estar à direita do pane 1
    manager.activate_pane(id1);
    assert_eq!(manager.active_pane().unwrap().id, id1);

    // Navega para direita
    manager.navigate(NavigationDirection::Right);
    assert_eq!(
        manager.active_pane().unwrap().id,
        id2,
        "Deve navegar para pane à direita"
    );

    // Navega para esquerda
    manager.navigate(NavigationDirection::Left);
    assert_eq!(
        manager.active_pane().unwrap().id,
        id1,
        "Deve navegar para pane à esquerda"
    );
}

/// Teste 6: Navegação vertical entre panes
///
/// Verifica navegação para cima/baixo após split horizontal.
#[test]
fn test_navigate_vertical() {
    let mut manager = SplitManager::new(800, 600);
    let id1 = manager.create_root_pane(setup_tab_manager());
    let id2 = manager.split_horizontal(setup_tab_manager()).unwrap();

    // Pane 2 deve estar abaixo do pane 1
    manager.activate_pane(id1);
    assert_eq!(manager.active_pane().unwrap().id, id1);

    // Navega para baixo
    manager.navigate(NavigationDirection::Down);
    assert_eq!(
        manager.active_pane().unwrap().id,
        id2,
        "Deve navegar para pane abaixo"
    );

    // Navega para cima
    manager.navigate(NavigationDirection::Up);
    assert_eq!(
        manager.active_pane().unwrap().id,
        id1,
        "Deve navegar para pane acima"
    );
}

/// Teste 7: Redimensionamento de pane
///
/// Verifica aumento e diminuição do tamanho do pane ativo.
#[test]
fn test_resize_pane() {
    let mut manager = SplitManager::new(800, 600);
    let id1 = manager.create_root_pane(setup_tab_manager());
    let _id2 = manager.split_vertical(setup_tab_manager()).unwrap();
    manager.activate_pane(id1);

    // Guarda tamanho inicial
    let initial_width = manager.panes().get(&id1).unwrap().rect.width;

    // Aumenta tamanho
    manager.increase_pane_size();
    let increased_width = manager.panes().get(&id1).unwrap().rect.width;
    assert!(
        increased_width > initial_width,
        "Largura deve aumentar após increase_pane_size"
    );

    // Diminui tamanho
    manager.decrease_pane_size();
    let decreased_width = manager.panes().get(&id1).unwrap().rect.width;
    assert!(
        decreased_width < increased_width,
        "Largura deve diminuir após decrease_pane_size"
    );
}

/// Teste 8: Redimensionamento com limites
///
/// Verifica que o redimensionamento respeita limites mínimos e máximos.
#[test]
fn test_resize_limits() {
    let mut manager = SplitManager::new(800, 600);
    let id1 = manager.create_root_pane(setup_tab_manager());
    let _id2 = manager.split_vertical(setup_tab_manager()).unwrap();

    // Tenta diminuir muito (deve parar no limite mínimo ~10%)
    for _ in 0..20 {
        manager.decrease_pane_size();
    }

    let pane1 = manager.panes().get(&id1).unwrap();
    // Largura mínima deve ser pelo menos 10% de 800 = 80
    assert!(
        pane1.rect.width >= 70,
        "Pane deve respeitar limite mínimo de tamanho: got {}",
        pane1.rect.width
    );

    // Tenta aumentar muito (deve parar no limite máximo ~90%)
    for _ in 0..20 {
        manager.increase_pane_size();
    }

    let pane1 = manager.panes().get(&id1).unwrap();
    // Largura máxima deve ser no máximo 90% de 800 = 720
    assert!(
        pane1.rect.width <= 730,
        "Pane deve respeitar limite máximo de tamanho: got {}",
        pane1.rect.width
    );
}

/// Teste 9: Fechamento de pane
///
/// Verifica que panes podem ser fechados e o layout é ajustado.
#[test]
fn test_close_pane() {
    let mut manager = SplitManager::new(800, 600);
    let _id1 = manager.create_root_pane(setup_tab_manager());
    let id2 = manager.split_vertical(setup_tab_manager()).unwrap();

    assert_eq!(manager.pane_count(), 2);

    // Fecha o segundo pane
    assert!(manager.close_pane(id2), "Deve conseguir fechar pane");
    assert_eq!(manager.pane_count(), 1, "Deve restar 1 pane");

    // Não pode fechar o último pane
    assert!(
        !manager.close_current_pane(),
        "Não deve conseguir fechar o último pane"
    );
    assert_eq!(manager.pane_count(), 1, "Pane count deve permanecer 1");
}

/// Teste 10: Fechamento do pane ativo
///
/// Verifica comportamento ao fechar o pane atualmente ativo.
#[test]
fn test_close_active_pane() {
    let mut manager = SplitManager::new(800, 600);
    let id1 = manager.create_root_pane(setup_tab_manager());
    let _id2 = manager.split_vertical(setup_tab_manager()).unwrap();

    // Pane ativo deve ser o último criado (id2)
    assert_eq!(manager.active_pane().unwrap().id, 2);

    // Fecha pane ativo
    assert!(manager.close_current_pane());

    // Deve ter voltado para o pane 1
    assert_eq!(manager.active_pane().unwrap().id, id1);
    assert_eq!(manager.pane_count(), 1);
}

/// Teste 11: Navegação circular entre panes
///
/// Verifica next_pane e previous_pane.
#[test]
fn test_circular_navigation() {
    let mut manager = SplitManager::new(800, 600);
    let _id1 = manager.create_root_pane(setup_tab_manager());
    let _id2 = manager.split_vertical(setup_tab_manager()).unwrap();
    let _id3 = manager.split_horizontal(setup_tab_manager()).unwrap();

    assert_eq!(manager.pane_count(), 3);

    // next_pane circular
    let start_id = manager.active_pane().unwrap().id;
    manager.next_pane();
    assert_ne!(manager.active_pane().unwrap().id, start_id);
    manager.next_pane();
    manager.next_pane();
    assert_eq!(
        manager.active_pane().unwrap().id,
        start_id,
        "Deve voltar ao início"
    );

    // previous_pane circular
    manager.previous_pane();
    manager.previous_pane();
    manager.previous_pane();
    assert_eq!(
        manager.active_pane().unwrap().id,
        start_id,
        "Deve voltar ao início"
    );
}

/// Teste 12: SplitTree interface
///
/// Verifica a interface de compatibilidade SplitTree.
#[test]
fn test_split_tree_interface() {
    let mut tree = SplitTree::new(800, 600);

    let root = tree.create_root(setup_tab_manager());
    assert_eq!(tree.manager().pane_count(), 1);

    // Split horizontal
    let split1 = tree.split_horizontal(root, setup_tab_manager());
    assert!(split1.is_some());
    assert_eq!(tree.manager().pane_count(), 2);

    // Split vertical no novo pane
    let split2 = tree.split_vertical(split1.unwrap(), setup_tab_manager());
    assert!(split2.is_some());
    assert_eq!(tree.manager().pane_count(), 3);

    // Navegação via tree
    tree.navigate(NavigationDirection::Left);
}

/// Teste 13: Conversão de NavigationDirection
///
/// Verifica conversões entre NavigationDirection e SplitDirection.
#[test]
fn test_navigation_direction_conversion() {
    // Up/Down -> Horizontal
    assert_eq!(
        NavigationDirection::Up.to_split_direction(),
        Some(SplitDirection::Horizontal)
    );
    assert_eq!(
        NavigationDirection::Down.to_split_direction(),
        Some(SplitDirection::Horizontal)
    );

    // Left/Right -> Vertical
    assert_eq!(
        NavigationDirection::Left.to_split_direction(),
        Some(SplitDirection::Vertical)
    );
    assert_eq!(
        NavigationDirection::Right.to_split_direction(),
        Some(SplitDirection::Vertical)
    );
}

/// Teste 14: NavigationDirection from_str
///
/// Verifica parsing de string para NavigationDirection.
#[test]
fn test_navigation_direction_from_str() {
    assert_eq!(
        NavigationDirection::from_str("up"),
        Some(NavigationDirection::Up)
    );
    assert_eq!(
        NavigationDirection::from_str("DOWN"),
        Some(NavigationDirection::Down)
    );
    assert_eq!(
        NavigationDirection::from_str("Left"),
        Some(NavigationDirection::Left)
    );
    assert_eq!(
        NavigationDirection::from_str("RIGHT"),
        Some(NavigationDirection::Right)
    );
    assert_eq!(NavigationDirection::from_str("invalid"), None);
}

/// Teste 15: Conversão de Action para SplitOperation
///
/// Verifica que ações são mapeadas corretamente para operações.
#[test]
fn test_action_to_split_operation() {
    use code_terminal::config::keybindings::Action;

    assert_eq!(
        action_to_split_op(&Action::SplitHorizontal),
        Some(SplitOperation::Split(SplitDirection::Horizontal))
    );
    assert_eq!(
        action_to_split_op(&Action::SplitVertical),
        Some(SplitOperation::Split(SplitDirection::Vertical))
    );
    assert_eq!(
        action_to_split_op(&Action::ClosePane),
        Some(SplitOperation::CloseCurrent)
    );
    assert_eq!(
        action_to_split_op(&Action::NavigateUp),
        Some(SplitOperation::Navigate(NavigationDirection::Up))
    );
    assert_eq!(
        action_to_split_op(&Action::NavigateDown),
        Some(SplitOperation::Navigate(NavigationDirection::Down))
    );
    assert_eq!(
        action_to_split_op(&Action::NavigateLeft),
        Some(SplitOperation::Navigate(NavigationDirection::Left))
    );
    assert_eq!(
        action_to_split_op(&Action::NavigateRight),
        Some(SplitOperation::Navigate(NavigationDirection::Right))
    );
    assert_eq!(
        action_to_split_op(&Action::NextPane),
        Some(SplitOperation::NextPane)
    );
    assert_eq!(
        action_to_split_op(&Action::PreviousPane),
        Some(SplitOperation::PreviousPane)
    );
    assert_eq!(
        action_to_split_op(&Action::IncreasePaneSize),
        Some(SplitOperation::IncreaseSize)
    );
    assert_eq!(
        action_to_split_op(&Action::DecreasePaneSize),
        Some(SplitOperation::DecreaseSize)
    );
    assert_eq!(action_to_split_op(&Action::NewTab), None);
}

/// Teste 16: Geração de render info
///
/// Verifica que as informações de renderização são geradas corretamente.
#[test]
fn test_generate_split_render_info() {
    let mut manager = SplitManager::new(800, 600);
    manager.create_root_pane(setup_tab_manager());

    let active_color = [1.0, 0.0, 0.0, 1.0];
    let inactive_color = [0.0, 0.0, 0.0, 1.0];

    let render_info = generate_split_render_info(&manager, active_color, inactive_color, 2);

    assert_eq!(render_info.len(), 1);
    assert!(
        render_info[0].is_active,
        "Pane ativo deve ter is_active=true"
    );
    assert_eq!(render_info[0].border_color, active_color);
    assert_eq!(render_info[0].border_thickness, 2);
}

/// Teste 17: Layouts aninhados
///
/// Verifica layouts complexos com múltiplos níveis de splits.
#[test]
fn test_nested_layouts() {
    let mut manager = SplitManager::new(800, 600);

    // Cria: P1 | (P2 / P3)
    // Split vertical, depois horizontal no lado direito
    let id1 = manager.create_root_pane(setup_tab_manager());
    let id2 = manager.split_vertical(setup_tab_manager()).unwrap();
    let id3 = manager.split_horizontal(setup_tab_manager()).unwrap();

    assert_eq!(manager.pane_count(), 3);

    // Verifica posições
    let panes = manager.panes();
    let pane1 = panes.get(&id1).unwrap();
    let pane2 = panes.get(&id2).unwrap();
    let pane3 = panes.get(&id3).unwrap();

    // P1 deve estar à esquerda
    assert!(pane1.rect.x < pane2.rect.x, "P1 deve estar à esquerda");
    assert!(
        pane1.rect.x < pane3.rect.x,
        "P1 deve estar à esquerda de P3"
    );

    // P2 deve estar acima de P3
    assert!(pane2.rect.y < pane3.rect.y, "P2 deve estar acima de P3");

    // P2 e P3 devem ter mesmo X
    assert_eq!(
        pane2.rect.x, pane3.rect.x,
        "P2 e P3 devem ter mesma posição X"
    );
}

/// Teste 18: Redimensionamento de janela
///
/// Verifica que os panes são recalculados quando a janela muda de tamanho.
#[test]
fn test_window_resize() {
    let mut manager = SplitManager::new(800, 600);
    let _id1 = manager.create_root_pane(setup_tab_manager());

    // Verifica tamanho inicial
    assert_eq!(manager.active_pane().unwrap().rect.width, 800);
    assert_eq!(manager.active_pane().unwrap().rect.height, 600);

    // Redimensiona
    manager.set_window_size(1024, 768);

    // Verifica novo tamanho
    assert_eq!(manager.active_pane().unwrap().rect.width, 1024);
    assert_eq!(manager.active_pane().unwrap().rect.height, 768);
}

/// Teste 19: Callbacks de pane
///
/// Verifica que os callbacks são chamados corretamente.
#[test]
fn test_pane_callbacks() {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    let mut manager = SplitManager::new(800, 600);

    let created_count = Arc::new(AtomicU64::new(0));
    let activated_count = Arc::new(AtomicU64::new(0));

    // Configura callbacks
    let created_count_cb = Arc::clone(&created_count);
    manager.on_pane_created(move |_| {
        created_count_cb.fetch_add(1, Ordering::SeqCst);
    });
    let activated_count_cb = Arc::clone(&activated_count);
    manager.on_pane_activated(move |_| {
        activated_count_cb.fetch_add(1, Ordering::SeqCst);
    });

    // Cria pane raiz
    manager.create_root_pane(setup_tab_manager());
    assert_eq!(created_count.load(Ordering::SeqCst), 1);
    assert_eq!(activated_count.load(Ordering::SeqCst), 1);

    // Split - deve criar e ativar
    manager.split_vertical(setup_tab_manager());
    assert_eq!(created_count.load(Ordering::SeqCst), 2);
    assert_eq!(activated_count.load(Ordering::SeqCst), 2);
}

/// Teste 20: LayoutManager integration
///
/// Verifica integração direta com LayoutManager.
#[test]
fn test_layout_manager_integration() {
    let mut layout = LayoutManager::new(800, 600);

    let id1 = layout.create_root_pane(setup_tab_manager());
    assert_eq!(layout.pane_count(), 1);

    let id2 = layout
        .split_pane(id1, SplitDirection::Vertical, setup_tab_manager())
        .unwrap();
    assert_eq!(layout.pane_count(), 2);

    // Testa navegação
    layout.set_active_pane(id1);
    assert_eq!(layout.active_pane().unwrap().id, id1);

    layout.next_pane();
    assert_eq!(layout.active_pane().unwrap().id, id2);

    // Testa redimensionamento
    layout.resize_active_pane(ResizeDirection::Increase);

    // Testa fechamento
    assert!(layout.close_pane(id1));
    assert_eq!(layout.pane_count(), 1);
}

/// Teste 21: Rect operations
///
/// Verifica operações do Rect.
#[test]
fn test_rect_operations() {
    let rect = Rect::new(10, 20, 100, 50);

    // Contains
    assert!(rect.contains(10, 20), "Canto superior esquerdo");
    assert!(rect.contains(50, 40), "Centro");
    assert!(rect.contains(109, 69), "Canto inferior direito (dentro)");

    assert!(!rect.contains(9, 20), "Fora à esquerda");
    assert!(!rect.contains(10, 19), "Fora acima");
    assert!(!rect.contains(110, 20), "Fora à direita");
    assert!(!rect.contains(10, 70), "Fora abaixo");

    // Center
    let center = rect.center();
    assert_eq!(center, (60, 45), "Centro deve ser (x + w/2, y + h/2)");
}

/// Teste 22: Handle de ações de split
///
/// Verifica que handle_action processa corretamente as ações.
#[test]
fn test_handle_action() {
    use code_terminal::config::keybindings::Action;

    let mut manager = SplitManager::new(800, 600);
    manager.create_root_pane(setup_tab_manager());

    // Split horizontal via ação
    assert!(manager.handle_action(Action::SplitHorizontal));
    assert_eq!(manager.pane_count(), 2);

    // Split vertical via ação
    assert!(manager.handle_action(Action::SplitVertical));
    assert_eq!(manager.pane_count(), 3);

    // Navegação via ação
    assert!(manager.handle_action(Action::NextPane));
    assert!(manager.handle_action(Action::PreviousPane));

    // Fechamento via ação
    assert!(manager.handle_action(Action::ClosePane));
    assert_eq!(manager.pane_count(), 2);

    // Não pode fechar o último
    assert!(manager.handle_action(Action::ClosePane));
    assert!(!manager.handle_action(Action::ClosePane));
}
