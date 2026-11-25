//! Testes de Integração - WindowManager
//!
//! Este módulo testa o gerenciamento de múltiplas janelas do code-terminal,
//! incluindo criação, fechamento, troca de janela ativa e mapeamento de IDs.
//!
//! # Execução
//!
//! ```bash
//! cargo test --test window_manager_test
//! ```

use code_terminal::config::Config;

// Importações necessárias dos módulos internos
// Nota: Como window.rs é um módulo privado, testamos via API pública quando possível
// ou usamos os tipos exportados em lib.rs

/// Testa a criação básica do WindowManager
///
/// Verifica que o WindowManager é criado corretamente com estado inicial vazio.
#[test]
fn test_window_manager_creation() {
    let config = Config::default();
    let manager = code_terminal::ui::window::WindowManager::new(config);

    assert_eq!(
        manager.window_count(),
        0,
        "WindowManager deve iniciar sem janelas"
    );
    assert!(
        !manager.has_windows(),
        "has_windows() deve retornar false quando vazio"
    );
    assert!(
        manager.active_window_id().is_none(),
        "Não deve haver janela ativa inicialmente"
    );
}

/// Testa a criação de WindowId únicos
///
/// Cada chamada para WindowId::default() deve retornar um ID único sequencial.
#[test]
fn test_window_id_generation() {
    use code_terminal::ui::window::WindowId;

    let id1 = WindowId::default();
    let id2 = WindowId::default();
    let id3 = WindowId::default();

    // IDs devem ser únicos
    assert_ne!(id1, id2, "IDs devem ser únicos");
    assert_ne!(id2, id3, "IDs devem ser únicos");
    assert_ne!(id1, id3, "IDs devem ser únicos");

    // IDs devem ser sequenciais (começando de 1)
    assert!(id2.value() > id1.value(), "IDs devem ser sequenciais");
    assert!(id3.value() > id2.value(), "IDs devem ser sequenciais");
}

/// Testa os estados de uma janela
///
/// Verifica transições entre estados: Active, Inactive, Minimized, Maximized, Closing.
#[test]
fn test_window_state_transitions() {
    use code_terminal::ui::window::WindowState;

    // Teste de estados individuais
    assert!(
        WindowState::Active.is_active(),
        "Active deve retornar true para is_active()"
    );
    assert!(
        !WindowState::Inactive.is_active(),
        "Inactive deve retornar false para is_active()"
    );
    assert!(
        !WindowState::Minimized.is_active(),
        "Minimized deve retornar false para is_active()"
    );
    assert!(
        !WindowState::Maximized.is_active(),
        "Maximized deve retornar false para is_active()"
    );
    assert!(
        !WindowState::Closing.is_active(),
        "Closing deve retornar false para is_active()"
    );

    // Verifica igualdade
    assert_eq!(WindowState::Active, WindowState::Active);
    assert_ne!(WindowState::Active, WindowState::Inactive);
}

/// Testa WindowCreateInfo padrão
///
/// Verifica que as configurações padrão de criação de janela estão corretas.
#[test]
fn test_window_create_info_default() {
    use code_terminal::ui::window::WindowCreateInfo;

    let info = WindowCreateInfo::default();

    assert_eq!(
        info.title, "Code Terminal",
        "Título padrão deve ser 'Code Terminal'"
    );
    assert_eq!(info.size, (800, 600), "Tamanho padrão deve ser 800x600");
    assert!(info.position.is_none(), "Posição padrão deve ser None");
}

/// Testa eventos de comunicação entre janelas
///
/// Verifica que os eventos de janela são criados corretamente.
#[test]
fn test_window_event_msg_variants() {
    use code_terminal::ui::window::{WindowEventMsg, WindowId};

    // Testa criação de eventos
    let create_event = WindowEventMsg::CreateWindow {
        position: Some((100, 100)),
        size: (1024, 768),
    };
    match create_event {
        WindowEventMsg::CreateWindow { position, size } => {
            assert_eq!(position, Some((100, 100)));
            assert_eq!(size, (1024, 768));
        }
        _ => panic!("Tipo de evento incorreto"),
    }

    // Testa fechamento
    let window_id = WindowId::default();
    let close_event = WindowEventMsg::CloseWindow(window_id);
    match close_event {
        WindowEventMsg::CloseWindow(id) => {
            assert_eq!(id, window_id);
        }
        _ => panic!("Tipo de evento incorreto"),
    }

    // Testa foco
    let focus_event = WindowEventMsg::FocusWindow(window_id);
    match focus_event {
        WindowEventMsg::FocusWindow(id) => {
            assert_eq!(id, window_id);
        }
        _ => panic!("Tipo de evento incorreto"),
    }

    // Testa movimento
    let move_event = WindowEventMsg::MoveWindow(window_id, 200, 300);
    match move_event {
        WindowEventMsg::MoveWindow(id, x, y) => {
            assert_eq!(id, window_id);
            assert_eq!(x, 200);
            assert_eq!(y, 300);
        }
        _ => panic!("Tipo de evento incorreto"),
    }

    // Testa resize
    let resize_event = WindowEventMsg::ResizeWindow(window_id, 1920, 1080);
    match resize_event {
        WindowEventMsg::ResizeWindow(id, w, h) => {
            assert_eq!(id, window_id);
            assert_eq!(w, 1920);
            assert_eq!(h, 1080);
        }
        _ => panic!("Tipo de evento incorreto"),
    }

    // Testa broadcast
    let broadcast_event = WindowEventMsg::Broadcast("test message".to_string());
    match broadcast_event {
        WindowEventMsg::Broadcast(msg) => {
            assert_eq!(msg, "test message");
        }
        _ => panic!("Tipo de evento incorreto"),
    }
}

/// Testa o mapeamento entre WindowId e valores u64
///
/// Verifica que WindowId pode ser convertido para/from u64 corretamente.
#[test]
fn test_window_id_value_mapping() {
    use code_terminal::ui::window::WindowId;

    let id = WindowId::default();
    let value = id.value();

    // Valor deve ser um u64 válido
    assert!(value > 0, "WindowId deve ser positivo");

    // IDs diferentes devem ter valores diferentes
    let id2 = WindowId::default();
    assert_ne!(id.value(), id2.value(), "Valores devem ser únicos");
}

/// Testa o Default de WindowManager
///
/// Verifica que WindowManager implementa Default corretamente.
#[test]
fn test_window_manager_default() {
    use code_terminal::ui::window::WindowManager;

    let manager: WindowManager = Default::default();

    assert_eq!(manager.window_count(), 0);
    assert!(!manager.has_windows());
}

/// Testa RenderHandle
///
/// Verifica que o RenderHandle é criado corretamente com um WindowId.
#[test]
fn test_render_handle() {
    use code_terminal::ui::window::{RenderHandle, WindowId};

    let window_id = WindowId::default();
    let handle = RenderHandle::new(window_id);

    // O handle deve permitir resize (mesmo que seja um no-op)
    let result = handle.resize(800, 600);
    assert!(result.is_ok(), "RenderHandle::resize deve retornar Ok");
}

/// Testa características de Debug e Clone para WindowId
///
/// Verifica que WindowId implementa os traits necessários.
#[test]
fn test_window_id_traits() {
    use code_terminal::ui::window::WindowId;

    let id = WindowId::default();

    // Debug
    let debug_str = format!("{:?}", id);
    assert!(
        debug_str.contains("WindowId"),
        "Debug deve mostrar WindowId"
    );

    // Clone
    let id_clone = id;
    assert_eq!(id, id_clone, "Clone deve criar valor igual");

    // Copy (deve compilar se for Copy)
    let id_copy = id;
    assert_eq!(id, id_copy, "Copy deve preservar valor");

    // Hash e Eq
    use std::collections::HashMap;
    let mut map: HashMap<WindowId, String> = HashMap::new();
    map.insert(id, "test".to_string());
    assert_eq!(map.get(&id), Some(&"test".to_string()));
}

/// Testa características de Debug para WindowState
///
/// Verifica que WindowState implementa Debug.
#[test]
fn test_window_state_debug() {
    use code_terminal::ui::window::WindowState;

    let active = WindowState::Active;
    let debug_str = format!("{:?}", active);
    assert_eq!(debug_str, "Active");

    let inactive = WindowState::Inactive;
    assert_eq!(format!("{:?}", inactive), "Inactive");

    let minimized = WindowState::Minimized;
    assert_eq!(format!("{:?}", minimized), "Minimized");

    let maximized = WindowState::Maximized;
    assert_eq!(format!("{:?}", maximized), "Maximized");

    let closing = WindowState::Closing;
    assert_eq!(format!("{:?}", closing), "Closing");
}

/// Teste de estresse para geração de WindowIds
///
/// Cria muitos IDs para garantir que não haja colisões.
#[test]
fn test_window_id_stress() {
    use code_terminal::ui::window::WindowId;
    use std::collections::HashSet;

    const COUNT: usize = 10000;
    let mut ids = HashSet::with_capacity(COUNT);

    for _ in 0..COUNT {
        let id = WindowId::default();
        // Insert retorna false se o valor já existia
        assert!(ids.insert(id), "ID duplicado detectado: {:?}", id);
    }

    assert_eq!(ids.len(), COUNT, "Todos os IDs devem ser únicos");
}

/// Testa a estrutura de Config da janela
///
/// Verifica que WindowConfig pode ser criada com valores padrão.
#[test]
fn test_window_config() {
    use code_terminal::config::WindowConfig;

    let config = WindowConfig::default();

    // Verifica que a configuração foi criada
    // (os valores específicos dependem da implementação em config)
    let _ = format!("{:?}", config);
}

/// Testa compatibilidade de tipos WindowId
///
/// Verifica que WindowId do ui::window é consistente com outros módulos.
#[test]
fn test_window_id_compatibility() {
    use code_terminal::types::WindowId as TypesWindowId;
    use code_terminal::ui::window::WindowId as UiWindowId;

    // Ambos devem ser baseados em u64
    let ui_id = UiWindowId::default();
    let types_id: TypesWindowId = ui_id.value();

    // Valores devem ser compatíveis
    assert_eq!(
        ui_id.value(),
        types_id,
        "WindowId values devem ser compatíveis"
    );
}
