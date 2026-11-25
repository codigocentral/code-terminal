pub mod config;
pub mod directory;
pub mod git;
pub mod ports;
pub mod toggle;
pub mod ui;
pub mod vertical_tabs;

use crate::ui::layout::Rect;
use crate::ui::tabs::{TabId, TabManager};
use std::collections::HashMap;
use std::path::PathBuf;

/// Identificador único de um item na sidebar
pub type SidebarItemId = u32;

/// Estado de visibilidade da sidebar
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SidebarVisibility {
    /// Sidebar visível
    Visible,
    /// Sidebar oculta
    Hidden,
    /// Sidebar em modo compacto (apenas ícones)
    Compact,
}

/// Tipo de conteúdo exibido na sidebar
#[derive(Debug, Clone, PartialEq)]
pub enum SidebarContent {
    /// Abas verticais
    VerticalTabs,
    /// Informações do Git
    GitInfo,
    /// Diretório de trabalho
    WorkingDirectory,
    /// Portas em uso
    PortInfo,
    /// Visão combinada (todos)
    Combined,
}

/// Informações de uma aba para exibição na sidebar
#[derive(Debug, Clone)]
pub struct SidebarTabInfo {
    pub id: TabId,
    pub title: String,
    pub is_active: bool,
    pub working_dir: Option<PathBuf>,
    pub git_branch: Option<String>,
    pub ports: Vec<u16>,
    pub has_activity: bool,
}

/// Informações de branch Git
#[derive(Debug, Clone)]
pub struct GitBranchInfo {
    pub branch_name: String,
    pub is_dirty: bool,
    pub ahead: usize,
    pub behind: usize,
    pub last_commit_msg: Option<String>,
}

/// Informações de porta em uso
#[derive(Debug, Clone)]
pub struct PortInfo {
    pub port: u16,
    pub protocol: String,
    pub process_name: Option<String>,
    pub is_listening: bool,
}

/// Item genérico na sidebar
#[derive(Debug, Clone)]
pub enum SidebarItem {
    Tab(SidebarTabInfo),
    GitInfo(GitBranchInfo),
    Directory(PathBuf),
    Port(PortInfo),
    Separator,
    Header(String),
}

/// Estrutura principal da Sidebar
#[derive(Debug)]
pub struct Sidebar {
    /// Configuração da sidebar
    pub config: config::SidebarConfig,
    /// Estado de visibilidade
    pub visibility: SidebarVisibility,
    /// Conteúdo atual sendo exibido
    pub content: SidebarContent,
    /// Lista de itens
    items: Vec<SidebarItem>,
    /// Mapeamento de tabs para informações
    tab_info: HashMap<TabId, SidebarTabInfo>,
    /// Componente de UI
    ui: ui::SidebarUI,
    /// Componente de abas verticais
    vertical_tabs: vertical_tabs::VerticalTabs,
    /// Detector de Git
    git_detector: git::GitBranchDetector,
    /// Display de diretório
    dir_display: directory::WorkingDirectoryDisplay,
    /// Detector de portas
    port_detector: ports::PortDetector,
    /// Componente de toggle
    toggle: toggle::SidebarToggle,
    /// Retângulo da sidebar
    rect: Rect,
    /// ID do próximo item
    next_item_id: SidebarItemId,
    /// Índice do item selecionado
    selected_index: Option<usize>,
}

impl Sidebar {
    /// Cria uma nova sidebar
    pub fn new(config: config::SidebarConfig) -> Self {
        let rect = Rect::new(0, 0, config.width, 600);

        Self {
            config: config.clone(),
            visibility: if config.visible {
                SidebarVisibility::Visible
            } else {
                SidebarVisibility::Hidden
            },
            content: SidebarContent::Combined,
            items: Vec::new(),
            tab_info: HashMap::new(),
            ui: ui::SidebarUI::new(config.clone()),
            vertical_tabs: vertical_tabs::VerticalTabs::new(config.vertical_tabs),
            git_detector: git::GitBranchDetector::new(),
            dir_display: directory::WorkingDirectoryDisplay::new(),
            port_detector: ports::PortDetector::new(),
            toggle: toggle::SidebarToggle::new(),
            rect,
            next_item_id: 1,
            selected_index: None,
        }
    }

    /// Cria uma sidebar com configuração padrão
    pub fn default() -> Self {
        Self::new(config::SidebarConfig::default())
    }

    /// Define o tamanho da sidebar
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
        self.ui.set_rect(rect);
    }

    /// Retorna o retângulo da sidebar
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Retorna o display de diretório de trabalho usado pela sidebar.
    pub fn directory_display(&self) -> &directory::WorkingDirectoryDisplay {
        &self.dir_display
    }

    /// Retorna o próximo ID reservado para item da sidebar.
    pub fn next_item_id(&self) -> SidebarItemId {
        self.next_item_id
    }

    /// Retorna a largura efetiva da sidebar (0 se oculta)
    pub fn effective_width(&self) -> u32 {
        match self.visibility {
            SidebarVisibility::Visible => self.config.width,
            SidebarVisibility::Compact => self.config.compact_width,
            SidebarVisibility::Hidden => 0,
        }
    }

    /// Verifica se a sidebar está visível
    pub fn is_visible(&self) -> bool {
        self.visibility != SidebarVisibility::Hidden
    }

    /// Verifica se está em modo compacto
    pub fn is_compact(&self) -> bool {
        self.visibility == SidebarVisibility::Compact
    }

    /// Alterna a visibilidade da sidebar
    pub fn toggle_visibility(&mut self) {
        self.visibility = self.toggle.toggle(self.visibility);
        self.ui.update_visibility(self.visibility);
    }

    /// Mostra a sidebar
    pub fn show(&mut self) {
        self.visibility = SidebarVisibility::Visible;
        self.ui.update_visibility(self.visibility);
    }

    /// Esconde a sidebar
    pub fn hide(&mut self) {
        self.visibility = SidebarVisibility::Hidden;
        self.ui.update_visibility(self.visibility);
    }

    /// Alterna para modo compacto
    pub fn toggle_compact(&mut self) {
        self.visibility = if self.is_compact() {
            SidebarVisibility::Visible
        } else {
            SidebarVisibility::Compact
        };
        self.ui.update_visibility(self.visibility);
    }

    /// Atualiza as informações das tabs
    pub fn update_tabs(&mut self, tab_manager: &TabManager) {
        self.tab_info.clear();

        for tab in tab_manager.tabs() {
            let working_dir = tab.working_dir().map(PathBuf::from);

            // Detecta branch git
            let git_branch = working_dir
                .as_ref()
                .and_then(|dir| self.git_detector.detect_branch(dir));

            // Detecta portas
            let ports = working_dir
                .as_ref()
                .map(|dir| self.port_detector.detect_ports_in_dir(dir))
                .unwrap_or_default();

            let info = SidebarTabInfo {
                id: tab.id,
                title: tab.title(),
                is_active: tab.is_active(),
                working_dir,
                git_branch,
                ports,
                has_activity: false,
            };

            self.tab_info.insert(tab.id, info);
        }

        self.rebuild_items();
        self.vertical_tabs.update_tabs(&self.tab_info);
    }

    /// Reconstrói a lista de itens da sidebar
    fn rebuild_items(&mut self) {
        self.items.clear();

        match self.content {
            SidebarContent::VerticalTabs => {
                self.items.push(SidebarItem::Header("Tabs".to_string()));
                for info in self.tab_info.values() {
                    self.items.push(SidebarItem::Tab(info.clone()));
                }
            }
            SidebarContent::GitInfo => {
                self.items.push(SidebarItem::Header("Git".to_string()));
                for info in self.tab_info.values() {
                    if let Some(ref branch) = info.git_branch {
                        let git_info = GitBranchInfo {
                            branch_name: branch.clone(),
                            is_dirty: false,
                            ahead: 0,
                            behind: 0,
                            last_commit_msg: None,
                        };
                        self.items.push(SidebarItem::GitInfo(git_info));
                    }
                }
            }
            SidebarContent::WorkingDirectory => {
                self.items
                    .push(SidebarItem::Header("Directories".to_string()));
                for info in self.tab_info.values() {
                    if let Some(ref dir) = info.working_dir {
                        self.items.push(SidebarItem::Directory(dir.clone()));
                    }
                }
            }
            SidebarContent::PortInfo => {
                self.items.push(SidebarItem::Header("Ports".to_string()));
                for info in self.tab_info.values() {
                    for port in &info.ports {
                        let port_info = PortInfo {
                            port: *port,
                            protocol: "TCP".to_string(),
                            process_name: None,
                            is_listening: true,
                        };
                        self.items.push(SidebarItem::Port(port_info));
                    }
                }
            }
            SidebarContent::Combined => {
                // Abas verticais
                self.items.push(SidebarItem::Header("Tabs".to_string()));
                for info in self.tab_info.values() {
                    self.items.push(SidebarItem::Tab(info.clone()));
                }

                self.items.push(SidebarItem::Separator);

                // Informações extras da tab ativa
                if let Some(active_info) = self.tab_info.values().find(|i| i.is_active) {
                    if let Some(ref branch) = active_info.git_branch {
                        self.items
                            .push(SidebarItem::Header("Git Branch".to_string()));
                        let git_info = GitBranchInfo {
                            branch_name: branch.clone(),
                            is_dirty: false,
                            ahead: 0,
                            behind: 0,
                            last_commit_msg: None,
                        };
                        self.items.push(SidebarItem::GitInfo(git_info));
                    }

                    if let Some(ref dir) = active_info.working_dir {
                        self.items
                            .push(SidebarItem::Header("Directory".to_string()));
                        self.items.push(SidebarItem::Directory(dir.clone()));
                    }

                    if !active_info.ports.is_empty() {
                        self.items
                            .push(SidebarItem::Header("Active Ports".to_string()));
                        for port in &active_info.ports {
                            let port_info = PortInfo {
                                port: *port,
                                protocol: "TCP".to_string(),
                                process_name: None,
                                is_listening: true,
                            };
                            self.items.push(SidebarItem::Port(port_info));
                        }
                    }
                }
            }
        }
    }

    /// Seleciona um item pelo índice
    pub fn select_item(&mut self, index: usize) {
        if index < self.items.len() {
            self.selected_index = Some(index);
        }
    }

    /// Seleciona o próximo item
    pub fn select_next(&mut self) {
        if self.items.is_empty() {
            return;
        }

        let new_index = match self.selected_index {
            Some(i) if i + 1 < self.items.len() => i + 1,
            _ => 0,
        };
        self.selected_index = Some(new_index);
    }

    /// Seleciona o item anterior
    pub fn select_previous(&mut self) {
        if self.items.is_empty() {
            return;
        }

        let new_index = match self.selected_index {
            Some(0) => self.items.len() - 1,
            Some(i) => i - 1,
            None => self.items.len() - 1,
        };
        self.selected_index = Some(new_index);
    }

    /// Retorna o ID da tab selecionada (se houver)
    pub fn selected_tab_id(&self) -> Option<TabId> {
        self.selected_index.and_then(|idx| {
            self.items.get(idx).and_then(|item| {
                if let SidebarItem::Tab(info) = item {
                    Some(info.id)
                } else {
                    None
                }
            })
        })
    }

    /// Retorna os itens da sidebar
    pub fn items(&self) -> &[SidebarItem] {
        &self.items
    }

    /// Retorna o índice selecionado
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Atualiza o conteúdo exibido
    pub fn set_content(&mut self, content: SidebarContent) {
        self.content = content;
        self.rebuild_items();
    }

    /// Atualiza configuração
    pub fn update_config(&mut self, config: config::SidebarConfig) {
        self.config = config.clone();
        self.ui.update_config(config.clone());
        self.vertical_tabs.update_config(config.vertical_tabs);
    }

    /// Força atualização das informações de Git
    pub fn refresh_git_info(&mut self, tab_manager: &TabManager) {
        self.git_detector.clear_cache();
        self.update_tabs(tab_manager);
    }

    /// Força atualização das informações de portas
    pub fn refresh_port_info(&mut self, tab_manager: &TabManager) {
        self.port_detector.clear_cache();
        self.update_tabs(tab_manager);
    }

    /// Retorna informações de uma tab específica
    pub fn get_tab_info(&self, tab_id: TabId) -> Option<&SidebarTabInfo> {
        self.tab_info.get(&tab_id)
    }

    /// Verifica se um ponto está dentro da sidebar
    pub fn contains_point(&self, x: u32, y: u32) -> bool {
        self.is_visible() && self.rect.contains(x, y)
    }

    /// Calcula a posição do conteúdo do terminal (considerando a sidebar)
    pub fn content_offset(&self) -> u32 {
        if self.is_visible() && self.config.position == config::SidebarPosition::Left {
            self.effective_width()
        } else {
            0
        }
    }
}

impl Default for Sidebar {
    fn default() -> Self {
        Self::new(config::SidebarConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidebar_creation() {
        let sidebar = Sidebar::default();
        assert!(sidebar.is_visible());
        assert_eq!(sidebar.effective_width(), 250); // Largura padrão
    }

    #[test]
    fn test_sidebar_toggle() {
        let mut sidebar = Sidebar::default();
        assert!(sidebar.is_visible());

        sidebar.toggle_visibility();
        assert!(!sidebar.is_visible());
        assert_eq!(sidebar.effective_width(), 0);

        sidebar.toggle_visibility();
        assert!(sidebar.is_visible());
    }

    #[test]
    fn test_sidebar_compact() {
        let mut sidebar = Sidebar::default();

        sidebar.toggle_compact();
        assert!(sidebar.is_compact());
        assert_eq!(sidebar.effective_width(), 50); // Largura compacta

        sidebar.toggle_compact();
        assert!(!sidebar.is_compact());
    }

    #[test]
    fn test_content_offset() {
        let mut sidebar = Sidebar::default();
        assert_eq!(sidebar.content_offset(), 250);

        sidebar.hide();
        assert_eq!(sidebar.content_offset(), 0);
    }
}
