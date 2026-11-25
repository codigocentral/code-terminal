//! UI Splits - Sistema de divisão de tela em múltiplos terminais
//!
//! Este módulo implementa:
//! - SplitTree (árvore binária) para gerenciar layout de panes
//! - Splits horizontal/vertical
//! - Navegação Alt+Arrow entre panes
//! - Atalhos Ctrl+Shift+E (horizontal) e Ctrl+Shift+O (vertical)

use crate::config::keybindings::{Action, NativeSplitShortcuts};
use crate::ui::layout::{LayoutManager, PaneId, Rect, ResizeDirection, SplitDirection};
use crate::ui::tabs::TabManager;
use std::collections::HashMap;

/// Representa uma direção de navegação
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavigationDirection {
    Up,
    Down,
    Left,
    Right,
}

impl NavigationDirection {
    /// Converte para SplitDirection quando apropriado
    pub fn to_split_direction(&self) -> Option<SplitDirection> {
        match self {
            NavigationDirection::Up | NavigationDirection::Down => Some(SplitDirection::Horizontal),
            NavigationDirection::Left | NavigationDirection::Right => {
                Some(SplitDirection::Vertical)
            }
        }
    }

    /// Converte de string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

impl From<Action> for Option<NavigationDirection> {
    fn from(action: Action) -> Self {
        match action {
            Action::NavigateUp => Some(NavigationDirection::Up),
            Action::NavigateDown => Some(NavigationDirection::Down),
            Action::NavigateLeft => Some(NavigationDirection::Left),
            Action::NavigateRight => Some(NavigationDirection::Right),
            _ => None,
        }
    }
}

/// Estado de um pane na UI
#[derive(Debug)]
pub struct SplitPane {
    pub id: PaneId,
    pub tab_manager: TabManager,
    pub rect: Rect,
    pub is_active: bool,
}

/// Gerenciador de splits com integração de input
pub struct SplitManager {
    layout: LayoutManager,
    /// Histórico de panes ativos (para navegação circular)
    pane_history: Vec<PaneId>,
    /// Configurações de teclas
    shortcuts: NativeSplitShortcuts,
    /// Callback quando um pane é ativado
    on_pane_activated: Option<Box<dyn Fn(PaneId) + Send + Sync>>,
    /// Callback quando um pane é criado
    on_pane_created: Option<Box<dyn Fn(PaneId) + Send + Sync>>,
    /// Callback quando um pane é fechado
    on_pane_closed: Option<Box<dyn Fn(PaneId) + Send + Sync>>,
}

impl std::fmt::Debug for SplitManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SplitManager")
            .field("layout", &self.layout)
            .field("pane_history", &self.pane_history)
            .field("shortcuts", &self.shortcuts)
            .field("has_callbacks", &self.on_pane_activated.is_some())
            .finish()
    }
}

impl SplitManager {
    /// Cria um novo SplitManager
    pub fn new(window_width: u32, window_height: u32) -> Self {
        Self {
            layout: LayoutManager::new(window_width, window_height),
            pane_history: Vec::new(),
            shortcuts: NativeSplitShortcuts::default(),
            on_pane_activated: None,
            on_pane_created: None,
            on_pane_closed: None,
        }
    }

    /// Cria o pane raiz inicial
    pub fn create_root_pane(&mut self, tab_manager: TabManager) -> PaneId {
        let id = self.layout.create_root_pane(tab_manager);
        self.pane_history.push(id);

        if let Some(ref callback) = self.on_pane_created {
            callback(id);
        }
        if let Some(ref callback) = self.on_pane_activated {
            callback(id);
        }

        id
    }

    /// Cria um split horizontal (empilha verticalmente)
    /// Retorna o ID do novo pane
    pub fn split_horizontal(&mut self, tab_manager: TabManager) -> Option<PaneId> {
        let current_pane = self.layout.active_pane()?;
        let new_id =
            self.layout
                .split_pane(current_pane.id, SplitDirection::Horizontal, tab_manager)?;

        self.pane_history.push(new_id);

        if let Some(ref callback) = self.on_pane_created {
            callback(new_id);
        }
        if let Some(ref callback) = self.on_pane_activated {
            callback(new_id);
        }

        Some(new_id)
    }

    /// Cria um split vertical (lado a lado)
    /// Retorna o ID do novo pane
    pub fn split_vertical(&mut self, tab_manager: TabManager) -> Option<PaneId> {
        let current_pane = self.layout.active_pane()?;
        let new_id =
            self.layout
                .split_pane(current_pane.id, SplitDirection::Vertical, tab_manager)?;

        self.pane_history.push(new_id);

        if let Some(ref callback) = self.on_pane_created {
            callback(new_id);
        }
        if let Some(ref callback) = self.on_pane_activated {
            callback(new_id);
        }

        Some(new_id)
    }

    /// Divide o pane atual na direção especificada
    pub fn split_current(
        &mut self,
        direction: SplitDirection,
        tab_manager: TabManager,
    ) -> Option<PaneId> {
        match direction {
            SplitDirection::Horizontal => self.split_horizontal(tab_manager),
            SplitDirection::Vertical => self.split_vertical(tab_manager),
        }
    }

    /// Fecha o pane atual
    /// Retorna false se não puder fechar (é o último pane)
    pub fn close_current_pane(&mut self) -> bool {
        let closed_id = self.layout.active_pane().map(|p| p.id);

        if !self.layout.close_active_pane() {
            return false;
        }

        // Remove do histórico
        if let Some(id) = closed_id {
            self.pane_history.retain(|&pane_id| pane_id != id);

            if let Some(ref callback) = self.on_pane_closed {
                callback(id);
            }
        }

        true
    }

    /// Fecha um pane específico
    pub fn close_pane(&mut self, pane_id: PaneId) -> bool {
        if !self.layout.close_pane(pane_id) {
            return false;
        }

        self.pane_history.retain(|&id| id != pane_id);

        if let Some(ref callback) = self.on_pane_closed {
            callback(pane_id);
        }

        true
    }

    /// Navega para o pane na direção especificada (Alt+Arrow)
    pub fn navigate(&mut self, direction: NavigationDirection) {
        let split_direction = match direction {
            NavigationDirection::Up | NavigationDirection::Down => SplitDirection::Horizontal,
            NavigationDirection::Left | NavigationDirection::Right => SplitDirection::Vertical,
        };

        if let Some(new_pane_id) = self.layout.find_pane_in_direction(split_direction) {
            // Verifica se a direção faz sentido (para cima deve ter y menor, etc)
            let current = self.layout.active_pane().expect("Should have active pane");
            let new_pane = self
                .layout
                .panes()
                .get(&new_pane_id)
                .expect("Pane should exist");

            let should_navigate = match direction {
                NavigationDirection::Up => new_pane.rect.y < current.rect.y,
                NavigationDirection::Down => new_pane.rect.y > current.rect.y,
                NavigationDirection::Left => new_pane.rect.x < current.rect.x,
                NavigationDirection::Right => new_pane.rect.x > current.rect.x,
            };

            if should_navigate {
                self.activate_pane(new_pane_id);
            }
        }
    }

    /// Ativa um pane específico
    pub fn activate_pane(&mut self, pane_id: PaneId) {
        let old_pane = self.layout.active_pane().map(|p| p.id);

        self.layout.set_active_pane(pane_id);

        // Adiciona ao histórico se mudou
        if old_pane != Some(pane_id) {
            self.pane_history.push(pane_id);

            if let Some(ref callback) = self.on_pane_activated {
                callback(pane_id);
            }
        }
    }

    /// Navega para o próximo pane (circular)
    pub fn next_pane(&mut self) {
        self.layout.next_pane();

        if let Some(pane) = self.layout.active_pane() {
            self.pane_history.push(pane.id);

            if let Some(ref callback) = self.on_pane_activated {
                callback(pane.id);
            }
        }
    }

    /// Navega para o pane anterior (circular)
    pub fn previous_pane(&mut self) {
        self.layout.previous_pane();

        if let Some(pane) = self.layout.active_pane() {
            self.pane_history.push(pane.id);

            if let Some(ref callback) = self.on_pane_activated {
                callback(pane.id);
            }
        }
    }

    /// Aumenta o tamanho do pane ativo
    pub fn increase_pane_size(&mut self) {
        self.layout.resize_active_pane(ResizeDirection::Increase);
    }

    /// Diminui o tamanho do pane ativo
    pub fn decrease_pane_size(&mut self) {
        self.layout.resize_active_pane(ResizeDirection::Decrease);
    }

    /// Processa uma Action de split
    /// Retorna true se a ação foi consumida
    pub fn handle_action(&mut self, action: Action) -> bool {
        match action {
            Action::SplitHorizontal => {
                let new_tab_manager = TabManager::new();
                self.split_horizontal(new_tab_manager);
                true
            }
            Action::SplitVertical => {
                let new_tab_manager = TabManager::new();
                self.split_vertical(new_tab_manager);
                true
            }
            Action::ClosePane => self.close_current_pane(),
            Action::NavigateUp => {
                self.navigate(NavigationDirection::Up);
                true
            }
            Action::NavigateDown => {
                self.navigate(NavigationDirection::Down);
                true
            }
            Action::NavigateLeft => {
                self.navigate(NavigationDirection::Left);
                true
            }
            Action::NavigateRight => {
                self.navigate(NavigationDirection::Right);
                true
            }
            Action::NextPane => {
                self.next_pane();
                true
            }
            Action::PreviousPane => {
                self.previous_pane();
                true
            }
            Action::IncreasePaneSize => {
                self.increase_pane_size();
                true
            }
            Action::DecreasePaneSize => {
                self.decrease_pane_size();
                true
            }
            _ => false,
        }
    }

    /// Processa input de teclado para navegação de splits
    /// Retorna true se o input foi consumido
    pub fn handle_key_input(&mut self, ctrl: bool, shift: bool, alt: bool, key: &str) -> bool {
        // Tenta converter para action
        if let Some(action) = self.shortcuts.try_get_action(ctrl, shift, alt, key) {
            self.handle_action(action)
        } else {
            false
        }
    }

    /// Atualiza o tamanho da janela
    pub fn set_window_size(&mut self, width: u32, height: u32) {
        self.layout.set_window_size(width, height);
    }

    /// Retorna o pane ativo
    pub fn active_pane(&self) -> Option<&crate::ui::layout::Pane> {
        self.layout.active_pane()
    }

    /// Retorna o pane ativo (mutável)
    pub fn active_pane_mut(&mut self) -> Option<&mut crate::ui::layout::Pane> {
        self.layout.active_pane_mut()
    }

    /// Retorna todos os panes
    pub fn panes(&self) -> &HashMap<PaneId, crate::ui::layout::Pane> {
        self.layout.panes()
    }

    /// Retorna todos os panes (mutável)
    pub fn panes_mut(&mut self) -> &mut HashMap<PaneId, crate::ui::layout::Pane> {
        self.layout.panes_mut()
    }

    /// Retorna o número de panes
    pub fn pane_count(&self) -> usize {
        self.layout.pane_count()
    }

    /// Define callback para quando um pane é ativado
    pub fn on_pane_activated<F>(&mut self, callback: F)
    where
        F: Fn(PaneId) + Send + Sync + 'static,
    {
        self.on_pane_activated = Some(Box::new(callback));
    }

    /// Define callback para quando um pane é criado
    pub fn on_pane_created<F>(&mut self, callback: F)
    where
        F: Fn(PaneId) + Send + Sync + 'static,
    {
        self.on_pane_created = Some(Box::new(callback));
    }

    /// Define callback para quando um pane é fechado
    pub fn on_pane_closed<F>(&mut self, callback: F)
    where
        F: Fn(PaneId) + Send + Sync + 'static,
    {
        self.on_pane_closed = Some(Box::new(callback));
    }

    /// Retorna shortcuts
    pub fn shortcuts(&self) -> &NativeSplitShortcuts {
        &self.shortcuts
    }

    /// Define shortcuts personalizados
    pub fn set_shortcuts(&mut self, shortcuts: NativeSplitShortcuts) {
        self.shortcuts = shortcuts;
    }

    /// Retorna referência ao LayoutManager interno
    pub fn layout_manager(&self) -> &LayoutManager {
        &self.layout
    }

    /// Retorna referência mutável ao LayoutManager interno
    pub fn layout_manager_mut(&mut self) -> &mut LayoutManager {
        &mut self.layout
    }
}

/// SplitTree - Interface compatível com o que foi solicitado
/// Esta é uma camada de compatibilidade sobre o LayoutManager
pub struct SplitTree {
    manager: SplitManager,
}

impl SplitTree {
    /// Cria uma nova árvore de splits
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            manager: SplitManager::new(width, height),
        }
    }

    /// Cria o pane raiz
    pub fn create_root(&mut self, tab_manager: TabManager) -> PaneId {
        self.manager.create_root_pane(tab_manager)
    }

    /// Split horizontal (divide altura)
    pub fn split_horizontal(&mut self, pane_id: PaneId, tab_manager: TabManager) -> Option<PaneId> {
        self.manager.layout.set_active_pane(pane_id);
        self.manager.split_horizontal(tab_manager)
    }

    /// Split vertical (divide largura)
    pub fn split_vertical(&mut self, pane_id: PaneId, tab_manager: TabManager) -> Option<PaneId> {
        self.manager.layout.set_active_pane(pane_id);
        self.manager.split_vertical(tab_manager)
    }

    /// Fecha um pane
    pub fn close_pane(&mut self, pane_id: PaneId) -> bool {
        self.manager.close_pane(pane_id)
    }

    /// Ativa um pane
    pub fn activate_pane(&mut self, pane_id: PaneId) {
        self.manager.activate_pane(pane_id);
    }

    /// Navega na direção especificada
    pub fn navigate(&mut self, direction: NavigationDirection) {
        self.manager.navigate(direction);
    }

    /// Retorna o manager interno
    pub fn manager(&self) -> &SplitManager {
        &self.manager
    }

    /// Retorna o manager interno (mutável)
    pub fn manager_mut(&mut self) -> &mut SplitManager {
        &mut self.manager
    }
}

/// Converte Action do sistema de configuração para operação de split
pub fn action_to_split_op(action: &Action) -> Option<SplitOperation> {
    match action {
        Action::SplitHorizontal => Some(SplitOperation::Split(SplitDirection::Horizontal)),
        Action::SplitVertical => Some(SplitOperation::Split(SplitDirection::Vertical)),
        Action::ClosePane => Some(SplitOperation::CloseCurrent),
        Action::NavigateUp => Some(SplitOperation::Navigate(NavigationDirection::Up)),
        Action::NavigateDown => Some(SplitOperation::Navigate(NavigationDirection::Down)),
        Action::NavigateLeft => Some(SplitOperation::Navigate(NavigationDirection::Left)),
        Action::NavigateRight => Some(SplitOperation::Navigate(NavigationDirection::Right)),
        Action::NextPane => Some(SplitOperation::NextPane),
        Action::PreviousPane => Some(SplitOperation::PreviousPane),
        Action::IncreasePaneSize => Some(SplitOperation::IncreaseSize),
        Action::DecreasePaneSize => Some(SplitOperation::DecreaseSize),
        _ => None,
    }
}

/// Operações que podem ser executadas em splits
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitOperation {
    Split(SplitDirection),
    CloseCurrent,
    Navigate(NavigationDirection),
    NextPane,
    PreviousPane,
    IncreaseSize,
    DecreaseSize,
}

/// Render info para splits
#[derive(Debug, Clone)]
pub struct SplitRenderInfo {
    pub pane_id: PaneId,
    pub rect: Rect,
    pub is_active: bool,
    pub border_thickness: u32,
    pub border_color: [f32; 4],
}

/// Gera informações de renderização para os splits
pub fn generate_split_render_info(
    manager: &SplitManager,
    active_border_color: [f32; 4],
    inactive_border_color: [f32; 4],
    border_thickness: u32,
) -> Vec<SplitRenderInfo> {
    manager
        .panes()
        .values()
        .map(|pane| SplitRenderInfo {
            pane_id: pane.id,
            rect: pane.rect,
            is_active: manager.active_pane().map(|p| p.id) == Some(pane.id),
            border_thickness,
            border_color: if manager.active_pane().map(|p| p.id) == Some(pane.id) {
                active_border_color
            } else {
                inactive_border_color
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_manager_creation() {
        let manager = SplitManager::new(800, 600);
        assert_eq!(manager.pane_count(), 0);
    }

    #[test]
    fn test_create_root_and_split() {
        let mut manager = SplitManager::new(800, 600);
        let root_id = manager.create_root_pane(TabManager::new());

        assert_eq!(manager.pane_count(), 1);
        assert_eq!(manager.active_pane().unwrap().id, root_id);

        // Split horizontal
        let new_id = manager.split_horizontal(TabManager::new());
        assert!(new_id.is_some());
        assert_eq!(manager.pane_count(), 2);
    }

    #[test]
    fn test_navigation_direction() {
        let mut manager = SplitManager::new(800, 600);
        let id1 = manager.create_root_pane(TabManager::new());
        let id2 = manager.split_vertical(TabManager::new()).unwrap();

        // Pane 2 deve estar à direita do pane 1
        manager.activate_pane(id1);
        manager.navigate(NavigationDirection::Right);
        assert_eq!(manager.active_pane().unwrap().id, id2);

        manager.navigate(NavigationDirection::Left);
        assert_eq!(manager.active_pane().unwrap().id, id1);
    }

    #[test]
    fn test_close_pane() {
        let mut manager = SplitManager::new(800, 600);
        let _id1 = manager.create_root_pane(TabManager::new());
        let id2 = manager.split_horizontal(TabManager::new()).unwrap();

        assert_eq!(manager.pane_count(), 2);
        assert!(manager.close_pane(id2));
        assert_eq!(manager.pane_count(), 1);

        // Não pode fechar o último
        assert!(!manager.close_current_pane());
        assert_eq!(manager.pane_count(), 1);
    }

    #[test]
    fn test_handle_action() {
        let mut manager = SplitManager::new(800, 600);
        let _id1 = manager.create_root_pane(TabManager::new());

        // Split horizontal
        assert!(manager.handle_action(Action::SplitHorizontal));
        assert_eq!(manager.pane_count(), 2);

        // Split vertical
        assert!(manager.handle_action(Action::SplitVertical));
        assert_eq!(manager.pane_count(), 3);

        // Navegação
        assert!(manager.handle_action(Action::NextPane));
        assert!(manager.handle_action(Action::PreviousPane));

        // Fechar pane
        assert!(manager.handle_action(Action::ClosePane));
        assert_eq!(manager.pane_count(), 2);

        // Fechar outro
        assert!(manager.handle_action(Action::ClosePane));
        assert_eq!(manager.pane_count(), 1);

        // Não pode fechar o último
        assert!(!manager.handle_action(Action::ClosePane));
    }

    #[test]
    fn test_split_tree() {
        let mut tree = SplitTree::new(800, 600);
        let root = tree.create_root(TabManager::new());

        let split1 = tree.split_horizontal(root, TabManager::new());
        assert!(split1.is_some());

        let split2 = tree.split_vertical(split1.unwrap(), TabManager::new());
        assert!(split2.is_some());

        assert_eq!(tree.manager().pane_count(), 3);
    }

    #[test]
    fn test_navigation_direction_conversion() {
        assert_eq!(
            NavigationDirection::Up.to_split_direction(),
            Some(SplitDirection::Horizontal)
        );
        assert_eq!(
            NavigationDirection::Left.to_split_direction(),
            Some(SplitDirection::Vertical)
        );
    }

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

    #[test]
    fn test_action_to_split_op() {
        assert_eq!(
            action_to_split_op(&Action::SplitHorizontal),
            Some(SplitOperation::Split(SplitDirection::Horizontal))
        );
        assert_eq!(
            action_to_split_op(&Action::NavigateUp),
            Some(SplitOperation::Navigate(NavigationDirection::Up))
        );
        assert_eq!(
            action_to_split_op(&Action::ClosePane),
            Some(SplitOperation::CloseCurrent)
        );
        assert_eq!(action_to_split_op(&Action::NewTab), None);
    }

    #[test]
    fn test_generate_split_render_info() {
        let mut manager = SplitManager::new(800, 600);
        manager.create_root_pane(TabManager::new());

        let active_color = [1.0, 0.0, 0.0, 1.0];
        let inactive_color = [0.0, 0.0, 0.0, 1.0];

        let render_info = generate_split_render_info(&manager, active_color, inactive_color, 2);

        assert_eq!(render_info.len(), 1);
        assert!(render_info[0].is_active);
        assert_eq!(render_info[0].border_color, active_color);
    }

    #[test]
    fn test_action_into_navigation_direction() {
        let nav: Option<NavigationDirection> = Action::NavigateUp.into();
        assert_eq!(nav, Some(NavigationDirection::Up));

        let nav: Option<NavigationDirection> = Action::NavigateRight.into();
        assert_eq!(nav, Some(NavigationDirection::Right));

        let nav: Option<NavigationDirection> = Action::NewTab.into();
        assert_eq!(nav, None);
    }
}
