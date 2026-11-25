use super::config::VerticalTabsConfig;
use super::SidebarTabInfo;
use crate::ui::tabs::TabId;
use std::collections::HashMap;

/// Componente de Abas Verticais
#[derive(Debug)]
pub struct VerticalTabs {
    config: VerticalTabsConfig,
    tabs: Vec<VerticalTabItem>,
    selected_index: Option<usize>,
    hovered_index: Option<usize>,
    scroll_offset: u32,
}

/// Item de aba vertical
#[derive(Debug, Clone)]
pub struct VerticalTabItem {
    pub tab_id: TabId,
    pub title: String,
    pub is_active: bool,
    pub working_dir: Option<String>,
    pub git_branch: Option<String>,
    pub ports: Vec<u16>,
    pub has_activity: bool,
    pub is_dirty: bool,
}

impl VerticalTabs {
    /// Cria um novo componente de abas verticais
    pub fn new(config: VerticalTabsConfig) -> Self {
        Self {
            config,
            tabs: Vec::new(),
            selected_index: None,
            hovered_index: None,
            scroll_offset: 0,
        }
    }

    /// Cria com configuração padrão
    pub fn default() -> Self {
        Self::new(VerticalTabsConfig::default())
    }

    /// Atualiza a configuração
    pub fn update_config(&mut self, config: VerticalTabsConfig) {
        self.config = config;
    }

    /// Atualiza as tabs a partir das informações
    pub fn update_tabs(&mut self, tab_info: &HashMap<TabId, SidebarTabInfo>) {
        self.tabs.clear();

        // Ordena por ID para manter ordem consistente
        let mut sorted: Vec<_> = tab_info.iter().collect();
        sorted.sort_by_key(|(id, _)| **id);

        for (_, info) in sorted {
            let item = VerticalTabItem {
                tab_id: info.id,
                title: if self.config.truncate_titles {
                    truncate_string(&info.title, self.config.max_title_length)
                } else {
                    info.title.clone()
                },
                is_active: info.is_active,
                working_dir: info.working_dir.as_ref().map(|p| {
                    if self.config.show_working_dir {
                        format_working_dir(p, &self.config)
                    } else {
                        String::new()
                    }
                }),
                git_branch: info.git_branch.clone(),
                ports: info.ports.clone(),
                has_activity: info.has_activity,
                is_dirty: false, // TODO: detect dirty state
            };
            self.tabs.push(item);
        }

        // Mantém seleção válida
        if let Some(selected) = self.selected_index {
            if selected >= self.tabs.len() {
                self.selected_index = if self.tabs.is_empty() {
                    None
                } else {
                    Some(self.tabs.len() - 1)
                };
            }
        }
    }

    /// Retorna o número de tabs
    pub fn count(&self) -> usize {
        self.tabs.len()
    }

    /// Verifica se está vazio
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Retorna todas as tabs
    pub fn tabs(&self) -> &[VerticalTabItem] {
        &self.tabs
    }

    /// Retorna a tab ativa
    pub fn active_tab(&self) -> Option<&VerticalTabItem> {
        self.tabs.iter().find(|t| t.is_active)
    }

    /// Retorna o ID da tab ativa
    pub fn active_tab_id(&self) -> Option<TabId> {
        self.active_tab().map(|t| t.tab_id)
    }

    /// Seleciona uma tab pelo índice
    pub fn select_index(&mut self, index: usize) -> Option<TabId> {
        if index < self.tabs.len() {
            self.selected_index = Some(index);
            Some(self.tabs[index].tab_id)
        } else {
            None
        }
    }

    /// Seleciona uma tab pelo ID
    pub fn select_tab(&mut self, tab_id: TabId) -> bool {
        if let Some(index) = self.tabs.iter().position(|t| t.tab_id == tab_id) {
            self.selected_index = Some(index);
            true
        } else {
            false
        }
    }

    /// Seleciona a próxima tab
    pub fn select_next(&mut self) -> Option<TabId> {
        if self.tabs.is_empty() {
            return None;
        }

        let new_index = match self.selected_index {
            Some(i) if i + 1 < self.tabs.len() => i + 1,
            _ => 0,
        };

        self.selected_index = Some(new_index);
        Some(self.tabs[new_index].tab_id)
    }

    /// Seleciona a tab anterior
    pub fn select_previous(&mut self) -> Option<TabId> {
        if self.tabs.is_empty() {
            return None;
        }

        let new_index = match self.selected_index {
            Some(0) => self.tabs.len() - 1,
            Some(i) => i - 1,
            None => self.tabs.len() - 1,
        };

        self.selected_index = Some(new_index);
        Some(self.tabs[new_index].tab_id)
    }

    /// Define o índice em hover
    pub fn set_hovered(&mut self, index: Option<usize>) {
        self.hovered_index = index;
    }

    /// Retorna o índice em hover
    pub fn hovered_index(&self) -> Option<usize> {
        self.hovered_index
    }

    /// Retorna o índice selecionado
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Retorna a tab selecionada
    pub fn selected_tab(&self) -> Option<&VerticalTabItem> {
        self.selected_index.and_then(|i| self.tabs.get(i))
    }

    /// Marca uma tab como tendo atividade
    pub fn set_activity(&mut self, tab_id: TabId, has_activity: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.tab_id == tab_id) {
            tab.has_activity = has_activity;
        }
    }

    /// Remove uma tab
    pub fn remove_tab(&mut self, tab_id: TabId) -> bool {
        if let Some(index) = self.tabs.iter().position(|t| t.tab_id == tab_id) {
            self.tabs.remove(index);

            // Ajusta seleção
            if let Some(selected) = self.selected_index {
                if selected >= index && selected > 0 {
                    self.selected_index = Some(selected - 1);
                } else if self.tabs.is_empty() {
                    self.selected_index = None;
                }
            } else if !self.tabs.is_empty() && self.selected_index.is_none() {
                self.selected_index = Some(0);
            }
            true
        } else {
            false
        }
    }

    /// Move uma tab para cima
    pub fn move_tab_up(&mut self, tab_id: TabId) -> bool {
        if let Some(index) = self.tabs.iter().position(|t| t.tab_id == tab_id) {
            if index > 0 {
                self.tabs.swap(index, index - 1);

                // Ajusta seleção se necessário
                if self.selected_index == Some(index) {
                    self.selected_index = Some(index - 1);
                } else if self.selected_index == Some(index - 1) {
                    self.selected_index = Some(index);
                }
                return true;
            }
        }
        false
    }

    /// Move uma tab para baixo
    pub fn move_tab_down(&mut self, tab_id: TabId) -> bool {
        if let Some(index) = self.tabs.iter().position(|t| t.tab_id == tab_id) {
            if index + 1 < self.tabs.len() {
                self.tabs.swap(index, index + 1);

                // Ajusta seleção se necessário
                if self.selected_index == Some(index) {
                    self.selected_index = Some(index + 1);
                } else if self.selected_index == Some(index + 1) {
                    self.selected_index = Some(index);
                }
                return true;
            }
        }
        false
    }

    /// Scroll para cima
    pub fn scroll_up(&mut self, amount: u32) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll para baixo
    pub fn scroll_down(&mut self, amount: u32) {
        let max_scroll = self.calculate_max_scroll();
        self.scroll_offset = (self.scroll_offset + amount).min(max_scroll);
    }

    /// Calcula o scroll máximo
    fn calculate_max_scroll(&self) -> u32 {
        let total_height = self.tabs.len() as u32 * self.config.item_height;
        let viewport_height = self.config.item_height * 10; // Assumimos 10 items visíveis

        total_height.saturating_sub(viewport_height)
    }

    /// Retorna o offset de scroll
    pub fn scroll_offset(&self) -> u32 {
        self.scroll_offset
    }

    /// Formata o título para display
    pub fn format_title(&self, item: &VerticalTabItem) -> String {
        let mut result = item.title.clone();

        // Adiciona indicadores
        if item.is_dirty {
            result.push_str(" ●");
        }
        if item.has_activity {
            result = format!("● {}", result);
        }

        result
    }

    /// Formata o subtítulo (working dir + git)
    pub fn format_subtitle(&self, item: &VerticalTabItem) -> String {
        let mut parts = Vec::new();

        if self.config.show_working_dir {
            if let Some(ref dir) = item.working_dir {
                if !dir.is_empty() {
                    parts.push(dir.clone());
                }
            }
        }

        if self.config.show_git_branch {
            if let Some(ref branch) = item.git_branch {
                parts.push(format!("({})", branch));
            }
        }

        if self.config.show_port_indicator && !item.ports.is_empty() {
            let port_str: Vec<_> = item.ports.iter().map(|p| p.to_string()).collect();
            parts.push(format!("[{}]", port_str.join(", ")));
        }

        parts.join(" ")
    }

    /// Retorna ícone para status
    pub fn status_icon(&self, item: &VerticalTabItem) -> &'static str {
        if item.is_active {
            "▶"
        } else if item.has_activity {
            "●"
        } else {
            "○"
        }
    }

    /// Retorna a altura total necessária
    pub fn total_height(&self) -> u32 {
        self.tabs.len() as u32 * self.config.item_height
    }

    /// Retorna índice da tab em uma posição Y
    pub fn tab_index_at_y(&self, y: u32) -> Option<usize> {
        let index = (y + self.scroll_offset) / self.config.item_height;
        if (index as usize) < self.tabs.len() {
            Some(index as usize)
        } else {
            None
        }
    }
}

/// Trunca uma string
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Formata o diretório de trabalho para display
fn format_working_dir(path: &std::path::Path, _config: &VerticalTabsConfig) -> String {
    // Sempre substitui home e trunca para 3 componentes
    let path_str = substitute_home_dir(path);

    // Mostra apenas os últimos componentes
    let components: Vec<_> = path_str.split(std::path::MAIN_SEPARATOR).collect();
    let max_components = 3;
    let start = components.len().saturating_sub(max_components);
    components[start..].join(std::path::MAIN_SEPARATOR_STR)
}

/// Substitui home directory por ~
fn substitute_home_dir(path: &std::path::Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = std::path::Path::new(&home);
        if let Ok(stripped) = path.strip_prefix(home_path) {
            if stripped.as_os_str().is_empty() {
                return "~".to_string();
            } else {
                return format!("~{}", stripped.to_string_lossy());
            }
        }
    }
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_tab_info(id: u32, title: &str) -> SidebarTabInfo {
        SidebarTabInfo {
            id,
            title: title.to_string(),
            is_active: false,
            working_dir: Some(PathBuf::from("/home/user/project")),
            git_branch: Some("main".to_string()),
            ports: vec![3000],
            has_activity: false,
        }
    }

    #[test]
    fn test_vertical_tabs_creation() {
        let tabs = VerticalTabs::default();
        assert!(tabs.is_empty());
        assert_eq!(tabs.count(), 0);
    }

    #[test]
    fn test_update_tabs() {
        let mut tabs = VerticalTabs::default();
        let mut info = HashMap::new();

        info.insert(1, create_test_tab_info(1, "Tab 1"));
        info.insert(2, create_test_tab_info(2, "Tab 2"));

        tabs.update_tabs(&info);

        assert_eq!(tabs.count(), 2);
        assert_eq!(tabs.tabs()[0].tab_id, 1);
        assert_eq!(tabs.tabs()[1].tab_id, 2);
    }

    #[test]
    fn test_select_tab() {
        let mut tabs = VerticalTabs::default();
        let mut info = HashMap::new();

        info.insert(1, create_test_tab_info(1, "Tab 1"));
        info.insert(2, create_test_tab_info(2, "Tab 2"));

        tabs.update_tabs(&info);

        assert!(tabs.select_tab(1));
        assert_eq!(tabs.selected_index(), Some(0));

        assert!(tabs.select_tab(2));
        assert_eq!(tabs.selected_index(), Some(1));

        assert!(!tabs.select_tab(999)); // Tab inexistente
    }

    #[test]
    fn test_select_next_previous() {
        let mut tabs = VerticalTabs::default();
        let mut info = HashMap::new();

        info.insert(1, create_test_tab_info(1, "Tab 1"));
        info.insert(2, create_test_tab_info(2, "Tab 2"));
        info.insert(3, create_test_tab_info(3, "Tab 3"));

        tabs.update_tabs(&info);
        tabs.select_index(0);

        assert_eq!(tabs.select_next(), Some(2)); // Vai para tab 2
        assert_eq!(tabs.select_next(), Some(3)); // Vai para tab 3
        assert_eq!(tabs.select_next(), Some(1)); // Volta para tab 1 (wrap)

        assert_eq!(tabs.select_previous(), Some(3)); // Volta para tab 3
    }

    #[test]
    fn test_active_tab() {
        let mut tabs = VerticalTabs::default();
        let mut info = HashMap::new();

        let mut tab1 = create_test_tab_info(1, "Tab 1");
        tab1.is_active = true;

        info.insert(1, tab1);
        info.insert(2, create_test_tab_info(2, "Tab 2"));

        tabs.update_tabs(&info);

        assert_eq!(tabs.active_tab_id(), Some(1));
        assert!(tabs.active_tab().unwrap().is_active);
    }

    #[test]
    fn test_remove_tab() {
        let mut tabs = VerticalTabs::default();
        let mut info = HashMap::new();

        info.insert(1, create_test_tab_info(1, "Tab 1"));
        info.insert(2, create_test_tab_info(2, "Tab 2"));
        info.insert(3, create_test_tab_info(3, "Tab 3"));

        tabs.update_tabs(&info);
        tabs.select_index(1); // Seleciona tab 2

        assert!(tabs.remove_tab(2));
        assert_eq!(tabs.count(), 2);
        assert!(!tabs.select_tab(2)); // Tab 2 não existe mais
    }

    #[test]
    fn test_move_tab() {
        let mut tabs = VerticalTabs::default();
        let mut info = HashMap::new();

        info.insert(1, create_test_tab_info(1, "Tab 1"));
        info.insert(2, create_test_tab_info(2, "Tab 2"));
        info.insert(3, create_test_tab_info(3, "Tab 3"));

        tabs.update_tabs(&info);

        // Move tab 2 para cima
        assert!(tabs.move_tab_up(2));
        assert_eq!(tabs.tabs()[0].tab_id, 2);
        assert_eq!(tabs.tabs()[1].tab_id, 1);

        // Move tab 2 para baixo (volta ao original)
        assert!(tabs.move_tab_down(2));
        assert_eq!(tabs.tabs()[0].tab_id, 1);
        assert_eq!(tabs.tabs()[1].tab_id, 2);

        // Não pode mover primeira tab para cima
        assert!(!tabs.move_tab_up(1));

        // Não pode mover última tab para baixo
        assert!(!tabs.move_tab_down(3));
    }

    #[test]
    fn test_format_title() {
        let config = VerticalTabsConfig::default();
        let mut tabs = VerticalTabs::new(config);

        let mut info = HashMap::new();
        info.insert(1, create_test_tab_info(1, "Test Tab"));
        tabs.update_tabs(&info);

        let item = tabs.tabs()[0].clone();
        let formatted = tabs.format_title(&item);
        assert_eq!(formatted, "Test Tab");

        // Com activity
        tabs.set_activity(1, true);
        let item = tabs.tabs()[0].clone();
        let formatted = tabs.format_title(&item);
        assert!(formatted.starts_with("● "));
    }

    #[test]
    fn test_format_subtitle() {
        let mut config = VerticalTabsConfig::default();
        config.show_working_dir = true;
        config.show_git_branch = true;
        config.show_port_indicator = true;

        let mut tabs = VerticalTabs::new(config);

        let mut info = HashMap::new();
        info.insert(1, create_test_tab_info(1, "Test"));
        tabs.update_tabs(&info);

        let item = tabs.tabs()[0].clone();
        let subtitle = tabs.format_subtitle(&item);

        assert!(subtitle.contains("project") || subtitle.contains("~"));
        assert!(subtitle.contains("(main)"));
        assert!(subtitle.contains("[3000]"));
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("short", 10), "short");
        assert_eq!(
            truncate_string("this is a very long string", 10),
            "this is..."
        );
    }

    #[test]
    fn test_substitute_home_dir() {
        // Este teste depende do ambiente, então testamos o comportamento básico
        let path = PathBuf::from("/some/random/path");
        let result = substitute_home_dir(&path);
        assert!(result.contains("/some/random/path"));
    }

    #[test]
    fn test_scroll() {
        let mut tabs = VerticalTabs::default();

        // Cria muitas tabs
        let mut info = HashMap::new();
        for i in 0..20 {
            info.insert(i, create_test_tab_info(i, &format!("Tab {}", i)));
        }
        tabs.update_tabs(&info);

        assert_eq!(tabs.scroll_offset(), 0);

        tabs.scroll_down(100);
        assert!(tabs.scroll_offset() > 0);

        let offset = tabs.scroll_offset();
        tabs.scroll_up(50);
        assert!(tabs.scroll_offset() < offset);
    }

    #[test]
    fn test_tab_index_at_y() {
        let tabs = VerticalTabs::default();

        assert_eq!(tabs.tab_index_at_y(0), None); // Sem tabs

        // Com tabs, o cálculo seria baseado no offset
    }
}
