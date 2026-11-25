//! Testes de Integração - Sistema Unificado de Pane
//!
//! Testes que validam a integração entre:
//! - Pane unificado (UnifiedPane)
//! - Trait PaneContent
//! - Input routing
//! - Session integration

use code_terminal::pane::content::{PaneContent, PaneContentType, PaneState};
use code_terminal::pane::input::{InputEvent, InputRouter};
use code_terminal::pane::unified::{UnifiedPane, UnifiedPaneManager};
use code_terminal::pane::*;

/// Testa criação básica de pane unificado
#[test]
fn test_unified_pane_basic() {
    let mut pane = UnifiedPane::new(80, 24).unwrap();

    // Estado inicial
    assert_eq!(pane.content_size(), (80, 24));
    assert_eq!(pane.state(), PaneState::Background);
    assert!(pane.is_dirty());

    // Ativa
    pane.activate();
    assert_eq!(pane.state(), PaneState::Active);
    assert!(pane.render_flags.highlight_active);

    // Desativa
    pane.deactivate();
    assert_eq!(pane.state(), PaneState::Background);
    assert!(!pane.render_flags.highlight_active);
}

/// Testa trait PaneContent para UnifiedPane
#[test]
fn test_pane_content_trait() {
    let mut pane = UnifiedPane::new(80, 24).unwrap();

    // Testa métodos do trait
    assert_eq!(pane.content_type(), PaneContentType::Shell);

    pane.set_state(PaneState::Active);
    assert_eq!(pane.state(), PaneState::Active);

    pane.mark_clean();
    assert!(!pane.is_dirty());

    pane.mark_dirty();
    assert!(pane.is_dirty());
}

/// Testa render info
#[test]
fn test_render_info() {
    let mut pane = UnifiedPane::new(80, 24).unwrap();

    // Configura posição e tamanho
    pane.move_to(100, 200);
    pane.set_size(800, 600);

    let info = pane.render_info();
    assert_eq!(info.position, (100, 200));
    assert_eq!(info.size, (800, 600));
    assert!(!info.is_active); // Background por padrão
}

/// Testa scroll methods
#[test]
fn test_scroll_operations() {
    let mut pane = UnifiedPane::new(80, 24).unwrap();

    // Scroll up
    pane.scroll_up(5);
    // Scroll down
    pane.scroll_down(3);
    // Scroll to top
    pane.scroll_to_top();
    // Scroll to bottom
    pane.scroll_to_bottom();
}

/// Testa copy mode
#[test]
fn test_copy_mode() {
    let mut pane = UnifiedPane::new(80, 24).unwrap();

    assert!(!pane.is_in_copy_mode());

    pane.enter_copy_mode();
    assert!(pane.is_in_copy_mode());

    pane.exit_copy_mode();
    assert!(!pane.is_in_copy_mode());
}

/// Testa UnifiedPaneManager
#[test]
fn test_pane_manager() {
    let mut manager = UnifiedPaneManager::new();

    // Cria panes
    let id1 = manager.create_pane(80, 24).unwrap();
    let id2 = manager.create_pane(100, 30).unwrap();

    assert_eq!(manager.pane_count(), 2);

    // O primeiro deve ser ativo automaticamente
    assert_eq!(manager.active_pane().unwrap().id, id1);

    // Muda ativo
    manager.set_active_pane(id2).unwrap();
    assert_eq!(manager.active_pane().unwrap().id, id2);

    // id1 deve estar em background
    assert_eq!(manager.get_pane(id1).unwrap().state, PaneState::Background);
    // id2 deve estar ativo
    assert_eq!(manager.get_pane(id2).unwrap().state, PaneState::Active);
}

/// Testa foco cíclico
#[test]
fn test_pane_focus_cycling() {
    let mut manager = UnifiedPaneManager::new();

    let id1 = manager.create_pane(80, 24).unwrap();
    let id2 = manager.create_pane(80, 24).unwrap();
    let id3 = manager.create_pane(80, 24).unwrap();

    // Ciclo: 1 -> 2 -> 3 -> 1
    assert_eq!(manager.active_pane().unwrap().id, id1);

    // Não temos método next/previous no manager, mas testamos set_active
    manager.set_active_pane(id2).unwrap();
    assert_eq!(manager.active_pane().unwrap().id, id2);

    manager.set_active_pane(id3).unwrap();
    assert_eq!(manager.active_pane().unwrap().id, id3);

    manager.set_active_pane(id1).unwrap();
    assert_eq!(manager.active_pane().unwrap().id, id1);
}

/// Testa remoção de pane
#[test]
fn test_pane_removal() {
    let mut manager = UnifiedPaneManager::new();

    let id1 = manager.create_pane(80, 24).unwrap();
    let id2 = manager.create_pane(80, 24).unwrap();

    manager.set_active_pane(id2).unwrap();

    // Remove o ativo - deve voltar para id1
    manager.remove_pane(id2).unwrap();

    assert_eq!(manager.pane_count(), 1);
    assert_eq!(manager.active_pane().unwrap().id, id1);
}

/// Testa pane_at_position
#[test]
fn test_pane_at_position() {
    let mut manager = UnifiedPaneManager::new();

    let id = manager.create_pane(80, 24).unwrap();
    {
        let pane = manager.get_pane_mut(id).unwrap();
        pane.move_to(0, 0);
        pane.set_size(100, 100);
    }

    // Dentro do pane
    let found = manager.pane_at_position(50, 50);
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, id);

    // Fora do pane
    let not_found = manager.pane_at_position(200, 200);
    assert!(not_found.is_none());
}

/// Testa InputRouter
#[test]
fn test_input_router() {
    let mut router = InputRouter::new();

    // Registra pane
    let pane = TerminalPane::new(80, 24).unwrap();
    let id = pane.pane_id();
    router.register_pane(Box::new(pane));

    assert_eq!(router.pane_count(), 1);
    assert_eq!(router.focused_pane(), Some(id));
}

/// Testa input routing para foco
#[test]
fn test_input_router_focus() {
    let mut router = InputRouter::new();

    let pane1 = TerminalPane::new(80, 24).unwrap();
    let pane2 = TerminalPane::new(80, 24).unwrap();

    let id1 = pane1.pane_id();
    let id2 = pane2.pane_id();

    router.register_pane(Box::new(pane1));
    router.register_pane(Box::new(pane2));

    // Primeiro registrado deve ser focado
    assert_eq!(router.focused_pane(), Some(id1));

    // Muda foco
    router.focus_pane(id2).unwrap();
    assert_eq!(router.focused_pane(), Some(id2));

    // Pane focado deve estar ativo
    let focused = router.get_pane(id2).unwrap();
    assert_eq!(focused.state(), PaneState::Active);

    // Pane anterior deve estar em background
    let prev = router.get_pane(id1).unwrap();
    assert_eq!(prev.state(), PaneState::Background);
}

/// Testa foco cíclico no router
#[test]
fn test_router_focus_cycling() {
    let mut router = InputRouter::new();

    let pane1 = TerminalPane::new(80, 24).unwrap();
    let pane2 = TerminalPane::new(80, 24).unwrap();
    let pane3 = TerminalPane::new(80, 24).unwrap();

    let id1 = pane1.pane_id();
    let id2 = pane2.pane_id();
    let _id3 = pane3.pane_id();

    router.register_pane(Box::new(pane1));
    router.register_pane(Box::new(pane2));
    router.register_pane(Box::new(pane3));

    assert_eq!(router.focused_pane(), Some(id1));

    router.focus_next();
    assert_eq!(router.focused_pane(), Some(id2));

    router.focus_previous();
    assert_eq!(router.focused_pane(), Some(id1));
}

/// Testa broadcast mode
#[test]
fn test_broadcast_mode() {
    let mut router = InputRouter::new();

    assert!(!router.is_broadcast_mode());

    router.toggle_broadcast();
    assert!(router.is_broadcast_mode());

    router.set_broadcast_mode(false);
    assert!(!router.is_broadcast_mode());
}

/// Testa processamento de input de resize
#[test]
fn test_input_resize() {
    let mut router = InputRouter::new();

    let pane = TerminalPane::new(80, 24).unwrap();
    let id = pane.pane_id();
    router.register_pane(Box::new(pane));

    // Evento de resize
    let resize_event = InputEvent::Resize {
        cols: 100,
        rows: 30,
    };

    // O resize deve ser processado
    let result = router.process_input(resize_event);
    assert!(result.is_ok());

    // Pane deve ter sido redimensionado
    let pane = router.get_pane(id).unwrap();
    assert_eq!(pane.content_size(), (100, 30));
}

/// Testa TerminalPane
#[test]
fn test_terminal_pane() {
    let pane = TerminalPane::new(80, 24).unwrap();

    assert_eq!(pane.content_type(), PaneContentType::Shell);
    assert!(pane.grid().is_some());
}

/// Testa serialization básica
#[test]
fn test_session_serialization() {
    let mut manager = UnifiedPaneManager::new();
    let id = manager.create_pane(80, 24).unwrap();

    {
        let pane = manager.get_pane_mut(id).unwrap();
        pane.metadata.title = "Test Pane".to_string();
    }

    let json = manager.serialize_session();

    assert!(json.get("panes").is_some());
    assert!(json.get("active_pane").is_some());
}

/// Testa RenderFlags
#[test]
fn test_render_flags() {
    let flags = RenderFlags {
        show_border: true,
        highlight_active: false,
        show_title_bar: true,
        show_scrollbar: false,
        reverse_video: false,
    };

    assert!(flags.show_border);
    assert!(!flags.highlight_active);
    assert!(flags.show_title_bar);
}

/// Testa contém ponto
#[test]
fn test_contains_point() {
    let mut pane = UnifiedPane::new(80, 24).unwrap();
    pane.move_to(10, 10);
    pane.set_size(100, 100);

    // Dentro
    assert!(pane.contains_point(50, 50));
    assert!(pane.contains_point(10, 10)); // Borda superior-esquerda
    assert!(pane.contains_point(109, 109)); // Borda inferior-direita (exclusiva)

    // Fora
    assert!(!pane.contains_point(5, 5));
    assert!(!pane.contains_point(200, 200));
}

/// Testa idle time
#[test]
fn test_idle_time() {
    let pane = UnifiedPane::new(80, 24).unwrap();

    // Recém criado, deve ter idle time próximo de zero
    let idle = pane.idle_time();
    assert!(idle.as_secs() < 1);
}

/// Testa área em células
#[test]
fn test_area_cells() {
    let pane = UnifiedPane::new(80, 24).unwrap();

    // 80 * 24 = 1920
    assert_eq!(pane.area_cells(), 1920);
}

/// Testa toggle zoom
#[test]
fn test_toggle_zoom() {
    let mut pane = UnifiedPane::new(80, 24).unwrap();

    assert_eq!(pane.state(), PaneState::Background);

    pane.toggle_zoom();
    assert_eq!(pane.state(), PaneState::Zoomed);

    pane.toggle_zoom();
    assert_eq!(pane.state(), PaneState::Active);
}

/// Testa metadata
#[test]
fn test_pane_metadata() {
    let mut pane = UnifiedPane::new(80, 24).unwrap();

    let metadata = pane.metadata().clone();
    assert!(metadata.title.is_empty());

    pane.metadata.title = "My Terminal".to_string();
    pane.metadata.cwd = Some("/home/user".to_string());

    assert_eq!(pane.metadata().title, "My Terminal");
    assert_eq!(pane.metadata().cwd, Some("/home/user".to_string()));
}

/// Testa pane com título
#[test]
fn test_pane_with_title() {
    let pane = UnifiedPane::new(80, 24).unwrap().with_title("Custom Title");

    assert_eq!(pane.metadata.title, "Custom Title");
}

/// Testa clear no manager
#[test]
fn test_manager_clear() {
    let mut manager = UnifiedPaneManager::new();

    manager.create_pane(80, 24).unwrap();
    manager.create_pane(80, 24).unwrap();

    assert_eq!(manager.pane_count(), 2);

    manager.clear();

    assert_eq!(manager.pane_count(), 0);
    assert!(manager.active_pane().is_none());
}

/// Testa atualização de todos os panes
#[test]
fn test_update_all() {
    let mut manager = UnifiedPaneManager::new();

    manager.create_pane(80, 24).unwrap();
    manager.create_pane(80, 24).unwrap();

    // Não deve panickar
    manager.update_all();
}

/// Testa lista de panes
#[test]
fn test_list_panes() {
    let mut manager = UnifiedPaneManager::new();

    let id1 = manager.create_pane(80, 24).unwrap();
    let id2 = manager.create_pane(80, 24).unwrap();

    let panes = manager.list_panes();
    assert_eq!(panes.len(), 2);

    // Verifica que os IDs estão na lista
    let ids: Vec<_> = panes.iter().map(|p| p.id).collect();
    assert!(ids.contains(&id1));
    assert!(ids.contains(&id2));
}
