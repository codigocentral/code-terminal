//! Testes de integração para o módulo Sidebar
//!
//! Este arquivo contém testes para todas as funcionalidades da Sprint 21:
//! - TASK-162: Sidebar UI
//! - TASK-163: Vertical tabs layout
//! - TASK-164: Git branch detection
//! - TASK-165: Working directory display
//! - TASK-166: Port detection
//! - TASK-167: Sidebar toggle
//! - TASK-168: Sidebar config
//! - TASK-169: Sidebar tests (este arquivo)

use code_terminal::ui::layout::Rect;
use code_terminal::ui::sidebar::{
    config::*,
    directory::{format_short_path, substitute_home, ProjectType},
    git::GitBranchDetector,
    ports::{is_port_available, PortDetector},
    toggle::*,
    vertical_tabs::VerticalTabs,
    Sidebar, SidebarContent, SidebarTabInfo, SidebarVisibility,
};
use code_terminal::ui::tabs::TabManager;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

// ============================================================================
// TASK-162: Sidebar UI Tests
// ============================================================================

#[test]
fn test_sidebar_ui_basic() {
    let config = SidebarConfig::default();
    let sidebar = Sidebar::new(config);

    assert!(sidebar.is_visible());
    assert!(!sidebar.is_compact());
    assert_eq!(sidebar.effective_width(), 250);
}

#[test]
fn test_sidebar_visibility_toggle() {
    let config = SidebarConfig::default();
    let mut sidebar = Sidebar::new(config);

    // Inicia visível
    assert!(sidebar.is_visible());

    // Toggle para oculto
    sidebar.toggle_visibility();
    assert!(!sidebar.is_visible());
    assert_eq!(sidebar.effective_width(), 0);

    // Toggle de volta para visível
    sidebar.toggle_visibility();
    assert!(sidebar.is_visible());
}

#[test]
fn test_sidebar_compact_mode() {
    let config = SidebarConfig::default();
    let mut sidebar = Sidebar::new(config);

    // Alterna para modo compacto
    sidebar.toggle_compact();
    assert!(sidebar.is_compact());
    assert_eq!(sidebar.effective_width(), 50); // Largura compacta padrão

    // Volta para modo normal
    sidebar.toggle_compact();
    assert!(!sidebar.is_compact());
    assert_eq!(sidebar.effective_width(), 250);
}

#[test]
fn test_sidebar_show_hide() {
    let config = SidebarConfig::default();
    let mut sidebar = Sidebar::new(config);

    sidebar.hide();
    assert!(!sidebar.is_visible());

    sidebar.show();
    assert!(sidebar.is_visible());
}

#[test]
fn test_sidebar_rect() {
    let config = SidebarConfig::default();
    let mut sidebar = Sidebar::new(config);

    let rect = Rect::new(0, 0, 250, 600);
    sidebar.set_rect(rect);

    assert_eq!(sidebar.rect().x, 0);
    assert_eq!(sidebar.rect().width, 250);
    assert_eq!(sidebar.rect().height, 600);
}

#[test]
fn test_sidebar_content_offset() {
    let config = SidebarConfig::default();
    let mut sidebar = Sidebar::new(config);

    // Quando visível na esquerda, offset é a largura
    assert_eq!(sidebar.content_offset(), 250);

    // Quando oculto, offset é 0
    sidebar.hide();
    assert_eq!(sidebar.content_offset(), 0);
}

#[test]
fn test_sidebar_contains_point() {
    let config = SidebarConfig::default();
    let mut sidebar = Sidebar::new(config);

    sidebar.set_rect(Rect::new(0, 0, 250, 600));

    // Dentro da sidebar
    assert!(sidebar.contains_point(100, 100));

    // Fora da sidebar
    assert!(!sidebar.contains_point(300, 100));

    // Quando oculta
    sidebar.hide();
    assert!(!sidebar.contains_point(100, 100));
}

// ============================================================================
// TASK-163: Vertical Tabs Layout Tests
// ============================================================================

#[test]
fn test_vertical_tabs_creation() {
    let tabs = VerticalTabs::default();
    assert!(tabs.is_empty());
    assert_eq!(tabs.count(), 0);
}

#[test]
fn test_vertical_tabs_selection() {
    let mut tabs = VerticalTabs::default();
    let mut tab_manager = TabManager::new();

    // Cria algumas tabs
    let _id1 = tab_manager.create_tab("bash");
    let id2 = tab_manager.create_tab("bash");
    let _id3 = tab_manager.create_tab("bash");
    tabs.update_tabs(&tab_info_from_manager(&tab_manager));

    // Inicialmente nenhuma selecionada
    assert_eq!(tabs.selected_index(), None);

    // Seleciona por índice
    let selected = tabs.select_index(0);
    assert!(selected.is_some());

    // Seleciona por ID
    assert!(tabs.select_tab(id2));
    assert_eq!(tabs.selected_tab().map(|t| t.tab_id), Some(id2));
}

#[test]
fn test_vertical_tabs_navigation() {
    let mut tabs = VerticalTabs::default();
    let mut tab_manager = TabManager::new();

    // Cria tabs
    tab_manager.create_tab("bash");
    tab_manager.create_tab("bash");
    tab_manager.create_tab("bash");
    tabs.update_tabs(&tab_info_from_manager(&tab_manager));

    tabs.select_index(0);

    // Next
    let next = tabs.select_next();
    assert!(next.is_some());
    assert_eq!(tabs.selected_index(), Some(1));

    // Previous
    let prev = tabs.select_previous();
    assert!(prev.is_some());
    assert_eq!(tabs.selected_index(), Some(0));

    // Wrap around
    tabs.select_previous();
    assert_eq!(tabs.selected_index(), Some(2)); // Última tab
}

#[test]
fn test_vertical_tabs_active_tab() {
    let tabs = VerticalTabs::default();
    let mut tab_manager = TabManager::new();

    let _id1 = tab_manager.create_tab("bash");
    let _id2 = tab_manager.create_tab("bash");

    // Primeira tab está ativa por padrão
    assert_eq!(tabs.active_tab_id(), None); // Ainda não atualizado
}

// ============================================================================
// TASK-164: Git Branch Detection Tests
// ============================================================================

#[test]
fn test_git_detector_creation() {
    let detector = GitBranchDetector::new();
    assert!(detector.is_enabled());
}

#[test]
fn test_git_detector_disabled() {
    let mut detector = GitBranchDetector::new();
    detector.set_enabled(false);
    assert!(!detector.is_enabled());

    let path = PathBuf::from("/tmp");
    assert_eq!(detector.detect_branch(&path), None);
}

#[test]
fn test_git_detector_cache() {
    let detector = GitBranchDetector::new();

    // Limpa cache inicial
    detector.clear_cache();
    let stats = detector.cache_stats();
    assert_eq!(stats.size, 0);

    // Cache duration padrão
    let detector_with_duration = GitBranchDetector::with_cache_duration(Duration::from_secs(10));
    assert!(detector_with_duration.is_enabled());
}

#[test]
fn test_git_repository_detection() {
    let detector = GitBranchDetector::new();

    // Diretório sem .git
    let temp_dir = std::env::temp_dir();
    let is_git = detector.is_git_repository(&temp_dir);

    // Resultado depende do ambiente
    println!("Is git repo: {}", is_git);
}

// ============================================================================
// TASK-165: Working Directory Display Tests
// ============================================================================

#[test]
fn test_directory_display_creation() {
    use code_terminal::ui::sidebar::directory::WorkingDirectoryDisplay;

    let display = WorkingDirectoryDisplay::new();
    let path = PathBuf::from("/home/user/project");

    let formatted = display.format(&path);
    // Deve retornar algo
    assert!(!formatted.is_empty());
}

#[test]
fn test_directory_substitute_home() {
    let home = if cfg!(target_os = "windows") {
        std::env::var("USERPROFILE").unwrap_or_default()
    } else {
        std::env::var("HOME").unwrap_or_default()
    };
    let path = PathBuf::from(&home).join("projects").join("test");

    let result = substitute_home(&path);

    if !home.is_empty() {
        assert!(
            result.starts_with("~"),
            "Result should start with ~: {}",
            result
        );
    }
}

fn tab_info_from_manager(tab_manager: &TabManager) -> HashMap<u32, SidebarTabInfo> {
    tab_manager
        .tabs()
        .iter()
        .map(|tab| {
            (
                tab.id,
                SidebarTabInfo {
                    id: tab.id,
                    title: tab.title(),
                    is_active: tab.is_active(),
                    working_dir: tab.working_dir().map(PathBuf::from),
                    git_branch: None,
                    ports: Vec::new(),
                    has_activity: false,
                },
            )
        })
        .collect()
}

#[test]
fn test_format_short_path() {
    let path = "/very/long/path/to/project/src/components";
    let formatted = format_short_path(path, 3);

    assert!(
        formatted.contains("project/src/components") || formatted.starts_with(".../"),
        "Formatted path: {}",
        formatted
    );
}

#[test]
fn test_project_type_detection() {
    // Testa tipos conhecidos pelos seus ícones
    assert_eq!(ProjectType::Rust.icon(), "🦀");
    assert_eq!(ProjectType::Node.icon(), "⬢");
    assert_eq!(ProjectType::Python.icon(), "🐍");
    assert_eq!(ProjectType::Go.icon(), "🐹");

    // Testa nomes
    assert_eq!(ProjectType::Rust.name(), "Rust");
    assert_eq!(ProjectType::Unknown.name(), "Unknown");
}

// ============================================================================
// TASK-166: Port Detection Tests
// ============================================================================

#[test]
fn test_port_detector_creation() {
    let detector = PortDetector::new();
    assert!(detector.is_enabled());
}

#[test]
fn test_port_detector_disabled() {
    let mut detector = PortDetector::new();
    detector.set_enabled(false);
    assert!(!detector.is_enabled());

    let ports = detector.detect_system_ports();
    assert!(ports.is_empty());
}

#[test]
fn test_common_ports_scan() {
    let detector = PortDetector::new();
    let ports = detector.scan_common_ports();

    // Resultado depende do sistema
    println!("Common ports in use: {:?}", ports);
}

#[test]
fn test_is_port_available() {
    // Porta 0 é inválida
    let port_0 = is_port_available(0);
    println!("Port 0 available: {}", port_0);

    // Porta 1 geralmente está disponível
    let port_1 = is_port_available(1);
    println!("Port 1 available: {}", port_1);
}

#[test]
fn test_port_guess_service() {
    let detector = PortDetector::new();

    assert_eq!(detector.guess_service(80), "HTTP");
    assert_eq!(detector.guess_service(443), "HTTPS");
    assert_eq!(detector.guess_service(22), "SSH");
    assert_eq!(detector.guess_service(3000), "React/Node");
    assert_eq!(detector.guess_service(8080), "HTTP-Alt");
    assert_eq!(detector.guess_service(65000), "Unknown");
}

// ============================================================================
// TASK-167: Sidebar Toggle Tests
// ============================================================================

#[test]
fn test_toggle_creation() {
    let toggle = SidebarToggle::new();
    assert!(toggle.show_button());
    assert_eq!(toggle.animation_duration_ms(), 200);
}

#[test]
fn test_toggle_visibility_states() {
    let toggle = SidebarToggle::new();

    // Visible -> Hidden
    assert_eq!(
        toggle.toggle(SidebarVisibility::Visible),
        SidebarVisibility::Hidden
    );

    // Hidden -> Visible
    assert_eq!(
        toggle.toggle(SidebarVisibility::Hidden),
        SidebarVisibility::Visible
    );

    // Compact -> Visible
    assert_eq!(
        toggle.toggle(SidebarVisibility::Compact),
        SidebarVisibility::Visible
    );
}

#[test]
fn test_toggle_compact_states() {
    let toggle = SidebarToggle::new();

    assert_eq!(
        toggle.toggle_compact(SidebarVisibility::Visible),
        SidebarVisibility::Compact
    );
    assert_eq!(
        toggle.toggle_compact(SidebarVisibility::Compact),
        SidebarVisibility::Visible
    );
}

#[test]
fn test_toggle_direct_states() {
    let toggle = SidebarToggle::new();

    assert_eq!(toggle.show(), SidebarVisibility::Visible);
    assert_eq!(toggle.hide(), SidebarVisibility::Hidden);
    assert_eq!(toggle.compact(), SidebarVisibility::Compact);
}

#[test]
fn test_toggle_cycle_states() {
    let toggle = SidebarToggle::new();

    let s1 = toggle.cycle_states(SidebarVisibility::Visible);
    assert_eq!(s1, SidebarVisibility::Compact);

    let s2 = toggle.cycle_states(s1);
    assert_eq!(s2, SidebarVisibility::Hidden);

    let s3 = toggle.cycle_states(s2);
    assert_eq!(s3, SidebarVisibility::Visible);
}

#[test]
fn test_toggle_icons() {
    assert_eq!(
        SidebarToggle::icon_for_state(SidebarVisibility::Visible),
        "◀"
    );
    assert_eq!(
        SidebarToggle::icon_for_state(SidebarVisibility::Hidden),
        "▶"
    );
}

#[test]
fn test_toggle_tooltips() {
    assert_eq!(
        SidebarToggle::tooltip_for_state(SidebarVisibility::Visible),
        "Hide Sidebar"
    );
    assert_eq!(
        SidebarToggle::tooltip_for_state(SidebarVisibility::Hidden),
        "Show Sidebar"
    );
}

#[test]
fn test_toggle_target_width() {
    let toggle = SidebarToggle::new();
    let config = SidebarConfig::default();

    assert_eq!(
        toggle.target_width(SidebarVisibility::Visible, &config),
        250
    );
    assert_eq!(toggle.target_width(SidebarVisibility::Hidden, &config), 0);
    assert_eq!(toggle.target_width(SidebarVisibility::Compact, &config), 50);
}

#[test]
fn test_animation() {
    let mut anim = ToggleAnimation::new(100);

    assert!(!anim.is_animating());

    anim.start(0, 100, AnimationState::Expanding);
    assert!(anim.is_animating());
    assert_eq!(anim.state(), AnimationState::Expanding);
}

// ============================================================================
// TASK-168: Sidebar Config Tests
// ============================================================================

#[test]
fn test_default_config() {
    let config = SidebarConfig::default();

    assert!(config.visible);
    assert_eq!(config.width, 250);
    assert_eq!(config.compact_width, 50);
    assert_eq!(config.min_width, 150);
    assert_eq!(config.max_width, 500);
}

#[test]
fn test_light_theme_config() {
    let config = SidebarConfig::light_theme();

    assert_eq!(config.background_color, "#f5f5f5");
    assert_eq!(config.foreground_color, "#333333");
}

#[test]
fn test_compact_config() {
    let config = SidebarConfig::compact();

    assert_eq!(config.width, 180);
    assert_eq!(config.compact_width, 40);
}

#[test]
fn test_config_validation() {
    let mut config = SidebarConfig::default();

    // Config válida
    assert!(config.validate().is_ok());

    // Largura muito pequena
    config.width = 100;
    assert!(config.validate().is_err());

    // Restaura e testa largura compacta inválida
    config.width = 250;
    config.compact_width = 300;
    assert!(config.validate().is_err());
}

#[test]
fn test_config_clamp_width() {
    let config = SidebarConfig::default();

    assert_eq!(config.clamp_width(100), 150); // Min
    assert_eq!(config.clamp_width(300), 300); // OK
    assert_eq!(config.clamp_width(600), 500); // Max
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_sidebar_full_workflow() {
    let config = SidebarConfig::default();
    let mut sidebar = Sidebar::new(config);
    let mut tab_manager = TabManager::new();

    // Cria tabs
    let _id1 = tab_manager.create_tab("bash");
    let _id2 = tab_manager.create_tab("bash");

    // Atualiza informações
    sidebar.update_tabs(&tab_manager);

    // Verifica que há itens
    assert!(!sidebar.items().is_empty());

    // Seleciona item
    sidebar.select_item(0);
    assert!(sidebar.selected_index().is_some());

    // Navega
    sidebar.select_next();
    // Seleção muda
}

#[test]
fn test_sidebar_content_types() {
    let config = SidebarConfig::default();
    let mut sidebar = Sidebar::new(config);

    // Testa diferentes tipos de conteúdo
    sidebar.set_content(SidebarContent::VerticalTabs);
    sidebar.set_content(SidebarContent::GitInfo);
    sidebar.set_content(SidebarContent::WorkingDirectory);
    sidebar.set_content(SidebarContent::PortInfo);
    sidebar.set_content(SidebarContent::Combined);

    // Todos devem funcionar sem panic
}

#[test]
fn test_sidebar_position_config() {
    use code_terminal::ui::sidebar::config::SidebarPosition;

    let mut config = SidebarConfig::default();
    assert_eq!(config.position, SidebarPosition::Left);

    // Muda para direita
    config.position = SidebarPosition::Right;
    assert_eq!(config.position, SidebarPosition::Right);
}

#[test]
fn test_vertical_tabs_config() {
    let mut config = VerticalTabsConfig::default();

    assert!(config.show_status_icon);
    assert!(config.show_working_dir);
    assert!(config.show_git_branch);
    assert!(config.truncate_titles);
    assert_eq!(config.max_title_length, 25);

    // Altera configuração
    config.show_git_branch = false;
    config.max_title_length = 15;

    assert!(!config.show_git_branch);
    assert_eq!(config.max_title_length, 15);
}

#[test]
fn test_git_detection_config() {
    let mut config = GitDetectionConfig::default();

    assert!(config.enabled);
    assert!(config.show_dirty_indicator);
    assert!(config.show_ahead_behind);
    assert_eq!(config.cache_duration_secs, 5);

    // Altera
    config.enabled = false;
    assert!(!config.enabled);
}

#[test]
fn test_directory_display_config() {
    let mut config = DirectoryDisplayConfig::default();

    assert!(config.enabled);
    assert!(!config.show_full_path);
    assert_eq!(config.max_path_components, 3);
    assert!(config.substitute_home);

    // Altera
    config.show_full_path = true;
    assert!(config.show_full_path);
}

#[test]
fn test_port_detection_config() {
    let mut config = PortDetectionConfig::default();

    assert!(config.enabled);
    assert!(!config.common_ports.is_empty());
    assert_eq!(config.scan_range, (3000, 9000));

    // Altera range
    config.scan_range = (8000, 9000);
    assert_eq!(config.scan_range, (8000, 9000));
}
