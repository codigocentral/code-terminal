//! UI Manager - Integração de SplitManager com TabManager
//!
//! Este módulo fornece uma interface unificada para gerenciar:
//! - Splits (divisão de tela)
//! - Tabs (abas em cada pane)
//! - Input routing
//! - TerminalPane content

use crate::config::keybindings::{Action, KeyBindings};
use crate::pane::browser_pane::{BrowserPane, BrowserPaneBuilder};
use crate::pane::input::{InputEvent, InputRouter};
use crate::pane::terminal_pane::TerminalPane;
use crate::ui::layout::{PaneId, Rect};
use crate::ui::splits::{NavigationDirection, SplitManager};
use crate::ui::tabs::{TabId, TabManager};
use anyhow::Result;

/// Gerenciador UI unificado
///
/// Integra SplitManager (layouts) com TabManager (abas) e TerminalPane (conteúdo)
#[derive(Debug)]
pub struct UIManager {
    /// Gerenciador de splits (contém os TabManagers nos panes)
    split_manager: SplitManager,
    /// ID do pane ativo
    active_pane: Option<PaneId>,
    /// Keybindings
    keybindings: KeyBindings,
    /// Tamanho da janela
    window_size: (u32, u32),
    /// Roteador de input para TerminalPanes
    input_router: InputRouter,
    /// Mapeamento de PaneId para TerminalPane
    terminal_panes: std::collections::HashMap<PaneId, TerminalPane>,
}

impl UIManager {
    /// Cria um novo UIManager
    pub fn new(window_width: u32, window_height: u32) -> Self {
        Self {
            split_manager: SplitManager::new(window_width, window_height),
            active_pane: None,
            keybindings: KeyBindings::default(),
            window_size: (window_width, window_height),
            input_router: InputRouter::new(),
            terminal_panes: std::collections::HashMap::new(),
        }
    }

    /// Retorna o número de panes terminais acompanhados pelo UIManager.
    pub fn terminal_pane_count(&self) -> usize {
        self.terminal_panes.len()
    }

    /// Cria um novo TerminalPane e o registra no input router
    ///
    /// # Arguments
    /// * `cols` - Número de colunas do terminal
    /// * `rows` - Número de linhas do terminal
    /// * `pane_id` - ID do pane onde o terminal será criado
    ///
    /// # Returns
    /// * `Ok(())` se o pane foi criado com sucesso
    /// * `Err` se houve erro na criação
    pub fn create_terminal_pane(&mut self, cols: u16, rows: u16, _pane_id: PaneId) -> Result<()> {
        let terminal_pane = TerminalPane::new(cols, rows)?;

        // Registra no input router
        self.input_router.register_pane(Box::new(terminal_pane));

        // Armazena referência (nota: precisamos acessar de volta do router)
        // Na prática, o InputRouter mantém a ownership

        Ok(())
    }

    /// Cria um TerminalPane no pane ativo
    pub fn create_terminal_in_active_pane(
        &mut self,
        cols: u16,
        rows: u16,
    ) -> Result<Option<PaneId>> {
        let pane_id = self
            .active_pane
            .ok_or_else(|| anyhow::anyhow!("Nenhum pane ativo"))?;
        self.create_terminal_pane(cols, rows, pane_id)?;
        Ok(Some(pane_id))
    }

    /// Processa input e envia para o TerminalPane apropriado
    ///
    /// # Returns
    /// * `true` se o input foi consumido
    /// * `false` se não foi processado
    pub fn process_input(&mut self, event: InputEvent) -> bool {
        match self.input_router.process_input(event) {
            Ok(consumed) => consumed,
            Err(e) => {
                tracing::warn!(error = %e, "Erro ao processar input");
                false
            }
        }
    }

    /// Atualiza todos os TerminalPanes (processa output do PTY)
    pub fn update_terminal_panes(&mut self) -> Result<()> {
        self.input_router.update_all()?;
        Ok(())
    }

    // =========================================================================
    // BrowserPane Methods
    // =========================================================================

    /// Cria um novo BrowserPane e o registra no input router
    ///
    /// # Arguments
    /// * `url` - URL inicial opcional
    /// * `pane_id` - ID do pane onde o browser será criado
    pub fn create_browser_pane(&mut self, url: Option<&str>, _pane_id: PaneId) -> Result<()> {
        let browser_pane = match url {
            Some(u) => BrowserPaneBuilder::new()
                .with_url(u)
                .with_title("Browser")
                .build()?,
            None => BrowserPane::new(None)?,
        };

        // Registra no input router
        self.input_router.register_pane(Box::new(browser_pane));

        Ok(())
    }

    /// Cria um BrowserPane no pane ativo
    pub fn create_browser_in_active_pane(&mut self, url: Option<&str>) -> Result<Option<PaneId>> {
        let pane_id = self
            .active_pane
            .ok_or_else(|| anyhow::anyhow!("Nenhum pane ativo"))?;
        self.create_browser_pane(url, pane_id)?;
        Ok(Some(pane_id))
    }

    /// Cria um split com terminal e browser lado a lado
    pub fn create_split_terminal_browser(
        &mut self,
        terminal_cols: u16,
        terminal_rows: u16,
        browser_url: Option<&str>,
    ) -> Result<(PaneId, PaneId)> {
        let terminal_pane_id = self
            .active_pane
            .ok_or_else(|| anyhow::anyhow!("Nenhum pane ativo"))?;

        // Cria terminal no pane atual
        self.create_terminal_pane(terminal_cols, terminal_rows, terminal_pane_id)?;

        // Cria split vertical
        let browser_pane_id = self
            .split_vertical()
            .ok_or_else(|| anyhow::anyhow!("Falha ao criar split"))?;

        // Cria browser no novo pane
        self.create_browser_pane(browser_url, browser_pane_id)?;

        Ok((terminal_pane_id, browser_pane_id))
    }

    /// Retorna o input router (para acesso avançado)
    pub fn input_router(&self) -> &InputRouter {
        &self.input_router
    }

    /// Retorna o input router mutável
    pub fn input_router_mut(&mut self) -> &mut InputRouter {
        &mut self.input_router
    }

    /// Envia input para um pane específico (bypass do roteamento normal)
    pub fn send_input_to_pane(&mut self, pane_id: PaneId, event: InputEvent) -> Result<bool> {
        Ok(self.input_router.send_to_pane(pane_id, event)?)
    }

    /// Cria o pane raiz inicial
    pub fn create_root_pane(&mut self) -> PaneId {
        let tab_manager = TabManager::new();
        let pane_id = self.split_manager.create_root_pane(tab_manager);

        self.active_pane = Some(pane_id);

        pane_id
    }

    /// Cria um split horizontal no pane atual
    pub fn split_horizontal(&mut self) -> Option<PaneId> {
        let tab_manager = TabManager::new();
        let new_id = self.split_manager.split_horizontal(tab_manager)?;

        self.active_pane = Some(new_id);

        Some(new_id)
    }

    /// Cria um split vertical no pane atual
    pub fn split_vertical(&mut self) -> Option<PaneId> {
        let tab_manager = TabManager::new();
        let new_id = self.split_manager.split_vertical(tab_manager)?;

        self.active_pane = Some(new_id);

        Some(new_id)
    }

    /// Fecha o pane atual
    pub fn close_current_pane(&mut self) -> bool {
        if !self.split_manager.close_current_pane() {
            return false;
        }

        // Atualiza o pane ativo
        self.active_pane = self.split_manager.active_pane().map(|p| p.id);

        true
    }

    /// Navega para o pane na direção especificada
    pub fn navigate(&mut self, direction: NavigationDirection) {
        self.split_manager.navigate(direction);
        self.active_pane = self.split_manager.active_pane().map(|p| p.id);
    }

    /// Cria uma nova tab no pane ativo
    pub fn create_tab(&mut self, title: &str) -> Option<TabId> {
        let pane = self.split_manager.active_pane_mut()?;
        let tab_id = pane.tab_manager.create_tab(title);
        Some(tab_id)
    }

    /// Fecha a tab atual no pane ativo
    pub fn close_current_tab(&mut self) -> bool {
        let Some(pane) = self.split_manager.active_pane_mut() else {
            return false;
        };

        pane.tab_manager.close_active_tab()
    }

    /// Navega para a próxima tab no pane ativo
    pub fn next_tab(&mut self) {
        if let Some(pane) = self.split_manager.active_pane_mut() {
            pane.tab_manager.next_tab();
        }
    }

    /// Navega para a tab anterior no pane ativo
    pub fn previous_tab(&mut self) {
        if let Some(pane) = self.split_manager.active_pane_mut() {
            pane.tab_manager.previous_tab();
        }
    }

    /// Navega para uma tab específica no pane ativo
    pub fn go_to_tab(&mut self, index: usize) {
        if let Some(pane) = self.split_manager.active_pane_mut() {
            pane.tab_manager.switch_tab(index);
        }
    }

    /// Processa uma Action
    /// Retorna true se a ação foi consumida
    pub fn handle_action(&mut self, action: Action) -> bool {
        // Primeiro tenta ações de split
        if self.split_manager.handle_action(action) {
            self.active_pane = self.split_manager.active_pane().map(|p| p.id);
            return true;
        }

        // Depois tenta ações de tab
        match action {
            Action::NewTab => {
                self.create_tab("New Tab");
                true
            }
            Action::CloseTab => {
                self.close_current_tab();
                true
            }
            Action::NextTab => {
                self.next_tab();
                true
            }
            Action::PreviousTab => {
                self.previous_tab();
                true
            }
            Action::GoToTab(index) => {
                self.go_to_tab(index);
                true
            }
            _ => false,
        }
    }

    /// Processa input de teclado
    /// Retorna true se o input foi consumido
    ///
    /// Nota: A conversão de key -> action deve ser feita pelo caller
    /// usando os atalhos configurados em keybindings
    pub fn handle_key_input(&mut self, _ctrl: bool, _shift: bool, _alt: bool, _key: &str) -> bool {
        // TODO: Implementar conversão de teclas para actions
        // baseado nas configurações de keybindings
        false
    }

    /// Retorna o pane ativo
    pub fn active_pane(&self) -> Option<PaneId> {
        self.active_pane
    }

    /// Retorna referência ao TabManager do pane ativo
    pub fn active_tab_manager(&self) -> Option<&TabManager> {
        self.split_manager.active_pane().map(|p| &p.tab_manager)
    }

    /// Retorna referência mutável ao TabManager do pane ativo
    pub fn active_tab_manager_mut(&mut self) -> Option<&mut TabManager> {
        self.split_manager
            .active_pane_mut()
            .map(|p| &mut p.tab_manager)
    }

    /// Retorna o TabManager para um pane específico
    pub fn get_tab_manager(&self, pane_id: PaneId) -> Option<&TabManager> {
        self.split_manager
            .panes()
            .get(&pane_id)
            .map(|p| &p.tab_manager)
    }

    /// Retorna o TabManager mutável para um pane específico
    pub fn get_tab_manager_mut(&mut self, pane_id: PaneId) -> Option<&mut TabManager> {
        self.split_manager
            .panes_mut()
            .get_mut(&pane_id)
            .map(|p| &mut p.tab_manager)
    }

    /// Retorna o número de panes
    pub fn pane_count(&self) -> usize {
        self.split_manager.pane_count()
    }

    /// Retorna o número total de tabs (em todos os panes)
    pub fn total_tab_count(&self) -> usize {
        self.split_manager
            .panes()
            .values()
            .map(|p| p.tab_manager.tab_count())
            .sum()
    }

    /// Atualiza o tamanho da janela
    pub fn set_window_size(&mut self, width: u32, height: u32) {
        self.window_size = (width, height);
        self.split_manager.set_window_size(width, height);
    }

    /// Retorna o tamanho da janela
    pub fn window_size(&self) -> (u32, u32) {
        self.window_size
    }

    /// Retorna o retângulo de um pane
    pub fn get_pane_rect(&self, pane_id: PaneId) -> Option<Rect> {
        self.split_manager.panes().get(&pane_id).map(|p| p.rect)
    }

    /// Retorna o retângulo do pane ativo
    pub fn active_pane_rect(&self) -> Option<Rect> {
        self.active_pane.and_then(|id| self.get_pane_rect(id))
    }

    /// Retorna uma referência ao SplitManager
    pub fn split_manager(&self) -> &SplitManager {
        &self.split_manager
    }

    /// Retorna uma referência mutável ao SplitManager
    pub fn split_manager_mut(&mut self) -> &mut SplitManager {
        &mut self.split_manager
    }

    /// Define keybindings personalizados
    pub fn set_keybindings(&mut self, keybindings: KeyBindings) {
        self.keybindings = keybindings;
    }

    /// Retorna referência aos keybindings
    pub fn keybindings(&self) -> &KeyBindings {
        &self.keybindings
    }

    /// Aumenta o tamanho do pane ativo
    pub fn increase_pane_size(&mut self) {
        self.split_manager.increase_pane_size();
    }

    /// Diminui o tamanho do pane ativo
    pub fn decrease_pane_size(&mut self) {
        self.split_manager.decrease_pane_size();
    }

    /// Navega para o próximo pane
    pub fn next_pane(&mut self) {
        self.split_manager.next_pane();
        self.active_pane = self.split_manager.active_pane().map(|p| p.id);
    }

    /// Navega para o pane anterior
    pub fn previous_pane(&mut self) {
        self.split_manager.previous_pane();
        self.active_pane = self.split_manager.active_pane().map(|p| p.id);
    }
}

impl Default for UIManager {
    fn default() -> Self {
        Self::new(800, 600)
    }
}

/// Dados para criar um terminal em um pane
#[derive(Debug, Clone)]
pub struct TerminalPaneConfig {
    /// Número de colunas
    pub cols: u16,
    /// Número de linhas
    pub rows: u16,
    /// Título opcional
    pub title: Option<String>,
    /// Diretório de trabalho inicial
    pub cwd: Option<String>,
    /// Comando a executar (se None, usa shell padrão)
    pub command: Option<String>,
}

impl Default for TerminalPaneConfig {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            title: None,
            cwd: None,
            command: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_manager_creation() {
        let manager = UIManager::new(800, 600);
        assert_eq!(manager.pane_count(), 0);
        assert_eq!(manager.window_size(), (800, 600));
    }

    #[test]
    fn test_create_root_pane() {
        let mut manager = UIManager::new(800, 600);
        let pane_id = manager.create_root_pane();

        assert_eq!(manager.pane_count(), 1);
        assert_eq!(manager.active_pane(), Some(pane_id));
    }

    #[test]
    fn test_split_and_tabs() {
        let mut manager = UIManager::new(800, 600);
        let _root_id = manager.create_root_pane();

        // Cria uma tab no root
        let tab1 = manager.create_tab("Tab 1");
        assert!(tab1.is_some());

        // Split horizontal
        let new_pane = manager.split_horizontal();
        assert!(new_pane.is_some());
        assert_eq!(manager.pane_count(), 2);

        // Cria tab no novo pane
        let tab2 = manager.create_tab("Tab 2");
        assert!(tab2.is_some());

        // Total de tabs deve ser 2
        assert_eq!(manager.total_tab_count(), 2);
    }

    #[test]
    fn test_tab_navigation() {
        let mut manager = UIManager::new(800, 600);
        manager.create_root_pane();

        // Cria várias tabs
        manager.create_tab("Tab 1");
        manager.create_tab("Tab 2");
        manager.create_tab("Tab 3");

        let tm = manager.active_tab_manager().unwrap();
        assert_eq!(tm.tab_count(), 3);

        // Navega entre tabs
        manager.next_tab();
        manager.previous_tab();

        // Vai para tab específica
        manager.go_to_tab(0);
    }

    #[test]
    fn test_close_pane_removes_tabs() {
        let mut manager = UIManager::new(800, 600);
        let _root_id = manager.create_root_pane();
        manager.create_tab("Tab 1");

        let new_pane = manager.split_vertical().unwrap();
        manager.create_tab("Tab 2");

        assert_eq!(manager.pane_count(), 2);
        assert_eq!(manager.total_tab_count(), 2);

        // Fecha o novo pane
        manager.split_manager.activate_pane(new_pane);
        assert!(manager.close_current_pane());

        assert_eq!(manager.pane_count(), 1);
        assert_eq!(manager.total_tab_count(), 1);
    }

    #[test]
    fn test_cannot_close_last_pane() {
        let mut manager = UIManager::new(800, 600);
        manager.create_root_pane();

        // Não pode fechar o último pane
        assert!(!manager.close_current_pane());
        assert_eq!(manager.pane_count(), 1);
    }

    #[test]
    fn test_handle_action() {
        let mut manager = UIManager::new(800, 600);
        manager.create_root_pane();

        // Split horizontal
        assert!(manager.handle_action(Action::SplitHorizontal));
        assert_eq!(manager.pane_count(), 2);

        // New tab
        assert!(manager.handle_action(Action::NewTab));
        assert_eq!(manager.total_tab_count(), 1);

        // Navigate
        assert!(manager.handle_action(Action::NextPane));
        assert!(manager.handle_action(Action::PreviousPane));
    }

    #[test]
    fn test_pane_navigation() {
        let mut manager = UIManager::new(800, 600);
        let root = manager.create_root_pane();
        let pane2 = manager.split_vertical().unwrap();

        // Navegação direcional
        manager.navigate(NavigationDirection::Left);
        assert_eq!(manager.active_pane(), Some(root));

        manager.navigate(NavigationDirection::Right);
        assert_eq!(manager.active_pane(), Some(pane2));
    }

    use crate::pane::input::{KeyModifiers, MouseEventKind};

    fn key(data: &[u8]) -> InputEvent {
        InputEvent::Key {
            data: data.to_vec(),
            key_code: None,
            modifiers: KeyModifiers::default(),
        }
    }

    #[test]
    fn test_content_panes_need_an_active_pane() {
        let mut m = UIManager::new(800, 600);
        assert!(m.create_terminal_in_active_pane(20, 5).is_err());
        assert!(m.create_browser_in_active_pane(None).is_err());
        assert!(m.create_split_terminal_browser(20, 5, None).is_err());
        assert!(!m.process_input(key(b"x")));
        assert!(m.update_terminal_panes().is_ok());
    }

    #[test]
    fn test_terminal_and_browser_panes_are_routed() {
        let mut m = UIManager::new(800, 600);
        let root = m.create_root_pane();
        assert_eq!(m.create_terminal_in_active_pane(20, 5).unwrap(), Some(root));
        assert_eq!(m.input_router().pane_count(), 1);
        // o primeiro pane registrado recebe o foco e as teclas
        assert!(m.process_input(key(b"true\r")));
        m.update_terminal_panes().unwrap();

        assert_eq!(
            m.create_browser_in_active_pane(Some("https://example.com"))
                .unwrap(),
            Some(root)
        );
        assert_eq!(m.input_router().pane_count(), 2);

        let (term, browser) = m
            .create_split_terminal_browser(20, 5, Some("https://example.org"))
            .unwrap();
        assert_eq!(term, root);
        assert_ne!(browser, root);
        assert_eq!(m.active_pane(), Some(browser));
        assert_eq!(m.pane_count(), 2);
        assert_eq!(m.input_router_mut().pane_count(), 4);

        let focused = m.input_router().focused_pane().unwrap();
        assert!(m.send_input_to_pane(focused, key(b"\r")).is_ok());
        assert!(m.send_input_to_pane(9_999_999, key(b"\r")).is_err());

        // o router sem erro devolve false para eventos não consumidos
        let mouse = InputEvent::Mouse {
            kind: MouseEventKind::Move,
            position: (5000, 5000),
            modifiers: KeyModifiers::default(),
        };
        assert!(!m.process_input(mouse));
    }

    #[test]
    fn test_tab_actions_and_accessors() {
        let mut m = UIManager::new(800, 600);
        assert!(m.create_tab("x").is_none());
        assert!(!m.close_current_tab());
        m.next_tab();
        m.previous_tab();
        m.go_to_tab(0);

        let root = m.create_root_pane();
        for action in [
            Action::NewTab,
            Action::NewTab,
            Action::NextTab,
            Action::PreviousTab,
        ] {
            assert!(m.handle_action(action));
        }
        assert_eq!(m.get_tab_manager(root).unwrap().tab_count(), 2);
        assert!(m.handle_action(Action::GoToTab(1)));
        assert!(m.handle_action(Action::CloseTab));
        assert_eq!(m.active_tab_manager().unwrap().tab_count(), 1);
        m.active_tab_manager_mut().unwrap().create_tab("y");
        m.get_tab_manager_mut(root).unwrap().create_tab("z");
        assert_eq!(m.total_tab_count(), 3);
        assert!(m.get_tab_manager(9_999).is_none());
        assert!(!m.handle_key_input(true, false, false, "t"));

        m.set_window_size(1024, 768);
        assert_eq!(m.window_size(), (1024, 768));
        assert_eq!(m.active_pane_rect(), m.get_pane_rect(root));
        assert!(m.get_pane_rect(9_999).is_none());
        assert_eq!(m.split_manager().pane_count(), 1);
        assert_eq!(m.split_manager_mut().pane_count(), 1);
        let kb = KeyBindings::default();
        m.set_keybindings(kb.clone());
        let _ = m.keybindings();
        assert_eq!(m.terminal_pane_count(), m.terminal_panes.len());
    }
}
