use super::config::SidebarConfig;
use super::{SidebarItem, SidebarVisibility};
use crate::ui::layout::Rect;

/// Componente de UI da Sidebar
#[derive(Debug)]
pub struct SidebarUI {
    config: SidebarConfig,
    rect: Rect,
    visibility: SidebarVisibility,
    scroll_offset: u32,
    max_scroll: u32,
    item_rects: Vec<Rect>,
    toggle_button_rect: Option<Rect>,
    resize_handle_rect: Option<Rect>,
}

impl SidebarUI {
    /// Cria um novo componente de UI
    pub fn new(config: SidebarConfig) -> Self {
        let rect = Rect::new(0, 0, config.width, 600);

        Self {
            config,
            rect,
            visibility: SidebarVisibility::Visible,
            scroll_offset: 0,
            max_scroll: 0,
            item_rects: Vec::new(),
            toggle_button_rect: None,
            resize_handle_rect: None,
        }
    }

    /// Define o retângulo da sidebar
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
        self.update_layout();
    }

    /// Retorna o retângulo
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Atualiza a visibilidade
    pub fn update_visibility(&mut self, visibility: SidebarVisibility) {
        self.visibility = visibility;
    }

    /// Atualiza a configuração
    pub fn update_config(&mut self, config: SidebarConfig) {
        self.config = config;
        self.update_layout();
    }

    /// Calcula o retângulo do conteúdo (excluindo padding)
    pub fn content_rect(&self) -> Rect {
        let padding = self.config.padding;
        Rect::new(
            self.rect.x + padding,
            self.rect.y + padding,
            self.rect.width.saturating_sub(padding * 2),
            self.rect.height.saturating_sub(padding * 2),
        )
    }

    /// Calcula os retângulos dos itens
    pub fn calculate_item_layout(&mut self, items: &[SidebarItem]) {
        self.item_rects.clear();

        if self.visibility != SidebarVisibility::Visible {
            return;
        }

        let content = self.content_rect();
        let spacing = self.config.item_spacing;
        let mut y = content.y;

        for item in items {
            let height = self.item_height(item);

            // Verifica se cabe no viewport visível
            let visible_y = y.saturating_sub(self.scroll_offset);
            if visible_y < content.y + content.height {
                let rect = Rect::new(content.x, visible_y, content.width, height);
                self.item_rects.push(rect);
            } else {
                // Item fora da tela, mas ainda adiciona para manter índices
                self.item_rects.push(Rect::new(0, 0, 0, 0));
            }

            y += height + spacing;
        }

        // Calcula scroll máximo
        let total_height = y - content.y;
        self.max_scroll = total_height.saturating_sub(content.height);
    }

    /// Retorna a altura de um item
    fn item_height(&self, item: &SidebarItem) -> u32 {
        match item {
            SidebarItem::Header(_) => 30,
            SidebarItem::Tab(_) => self.config.vertical_tabs.item_height,
            SidebarItem::GitInfo(_) => 50,
            SidebarItem::Directory(_) => 35,
            SidebarItem::Port(_) => 30,
            SidebarItem::Separator => 10,
        }
    }

    /// Retorna os retângulos dos itens
    pub fn item_rects(&self) -> &[Rect] {
        &self.item_rects
    }

    /// Retorna o retângulo de um item específico
    pub fn get_item_rect(&self, index: usize) -> Option<&Rect> {
        self.item_rects.get(index)
    }

    /// Verifica se um ponto está sobre um item
    pub fn hit_test(&self, x: u32, y: u32) -> Option<usize> {
        if !self.rect.contains(x, y) {
            return None;
        }

        for (index, rect) in self.item_rects.iter().enumerate() {
            if rect.contains(x, y) {
                return Some(index);
            }
        }
        None
    }

    /// Verifica se o ponto está sobre o botão de toggle
    pub fn hit_test_toggle(&self, x: u32, y: u32) -> bool {
        self.toggle_button_rect
            .map(|rect| rect.contains(x, y))
            .unwrap_or(false)
    }

    /// Verifica se o ponto está sobre o handle de resize
    pub fn hit_test_resize(&self, x: u32, y: u32) -> bool {
        self.resize_handle_rect
            .map(|rect| rect.contains(x, y))
            .unwrap_or(false)
    }

    /// Scroll para cima
    pub fn scroll_up(&mut self, amount: u32) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll para baixo
    pub fn scroll_down(&mut self, amount: u32) {
        self.scroll_offset = (self.scroll_offset + amount).min(self.max_scroll);
    }

    /// Scroll para um item específico
    pub fn scroll_to_item(&mut self, index: usize) {
        if index >= self.item_rects.len() {
            return;
        }

        let content = self.content_rect();
        let item_rect = &self.item_rects[index];

        // Item acima do viewport
        if item_rect.y < content.y {
            self.scroll_offset = self.scroll_offset.saturating_sub(content.y - item_rect.y);
        }
        // Item abaixo do viewport
        else if item_rect.y + item_rect.height > content.y + content.height {
            let overflow = (item_rect.y + item_rect.height) - (content.y + content.height);
            self.scroll_offset = (self.scroll_offset + overflow).min(self.max_scroll);
        }
    }

    /// Retorna o offset de scroll atual
    pub fn scroll_offset(&self) -> u32 {
        self.scroll_offset
    }

    /// Retorna o scroll máximo
    pub fn max_scroll(&self) -> u32 {
        self.max_scroll
    }

    /// Verifica se precisa de scroll
    pub fn needs_scroll(&self) -> bool {
        self.max_scroll > 0
    }

    /// Calcula a área da scrollbar
    pub fn scrollbar_rect(&self) -> Option<Rect> {
        if !self.needs_scroll() {
            return None;
        }

        let scrollbar_width = 8;
        let x = if self.config.position == super::config::SidebarPosition::Right {
            self.rect.x
        } else {
            self.rect.x + self.rect.width - scrollbar_width
        };

        Some(Rect::new(x, self.rect.y, scrollbar_width, self.rect.height))
    }

    /// Calcula a área do thumb da scrollbar
    pub fn scrollbar_thumb_rect(&self) -> Option<Rect> {
        let scrollbar = self.scrollbar_rect()?;

        let content_height = self.rect.height as f32;
        let viewport_ratio = content_height / (content_height + self.max_scroll as f32);
        let thumb_height = (content_height * viewport_ratio) as u32;

        let scroll_ratio = if self.max_scroll > 0 {
            self.scroll_offset as f32 / self.max_scroll as f32
        } else {
            0.0
        };
        let thumb_y =
            scrollbar.y + ((self.rect.height - thumb_height) as f32 * scroll_ratio) as u32;

        Some(Rect::new(
            scrollbar.x,
            thumb_y,
            scrollbar.width,
            thumb_height,
        ))
    }

    /// Retorna as cores para um item
    pub fn item_colors(&self, is_selected: bool, is_active: bool) -> ItemColors {
        let background = if is_active {
            &self.config.active_color
        } else if is_selected {
            &self.config.selected_color
        } else {
            &self.config.background_color
        };

        ItemColors {
            background: background.clone(),
            foreground: self.config.foreground_color.clone(),
            accent: self.config.accent_color.clone(),
        }
    }

    /// Retorna as dimensões para modo compacto
    pub fn compact_dimensions(&self) -> CompactDimensions {
        CompactDimensions {
            width: self.config.compact_width,
            icon_size: 24,
            item_height: 40,
        }
    }

    /// Atualiza o layout interno
    fn update_layout(&mut self) {
        // Recalcula posição do botão de toggle
        let toggle_size = 24;
        self.toggle_button_rect = Some(Rect::new(
            if self.config.position == super::config::SidebarPosition::Left {
                self.rect.x + self.rect.width - toggle_size - 4
            } else {
                self.rect.x + 4
            },
            self.rect.y + 4,
            toggle_size,
            toggle_size,
        ));

        // Recalcula posição do handle de resize
        let handle_width = 4;
        self.resize_handle_rect = Some(Rect::new(
            if self.config.position == super::config::SidebarPosition::Left {
                self.rect.x + self.rect.width - handle_width
            } else {
                self.rect.x
            },
            self.rect.y,
            handle_width,
            self.rect.height,
        ));
    }

    /// Retorna o retângulo do botão de toggle
    pub fn toggle_button_rect(&self) -> Option<Rect> {
        self.toggle_button_rect
    }

    /// Retorna o retângulo do handle de resize
    pub fn resize_handle_rect(&self) -> Option<Rect> {
        self.resize_handle_rect
    }

    /// Calcula a largura preferida baseada no conteúdo
    pub fn preferred_width(&self, items: &[SidebarItem]) -> u32 {
        let mut max_width = self.config.min_width;

        for item in items {
            let item_width = self.estimate_item_width(item);
            max_width = max_width.max(item_width);
        }

        max_width.min(self.config.max_width)
    }

    /// Estima a largura necessária para um item
    fn estimate_item_width(&self, item: &SidebarItem) -> u32 {
        let base_padding = self.config.padding * 2;
        let content_width = match item {
            SidebarItem::Header(text) => (text.len() as u32) * 8,
            SidebarItem::Tab(info) => {
                let mut w = (info.title.len() as u32) * 8;
                if let Some(ref dir) = info.working_dir {
                    w += dir.to_string_lossy().len() as u32 * 6;
                }
                w
            }
            SidebarItem::GitInfo(info) => (info.branch_name.len() as u32) * 8 + 40,
            SidebarItem::Directory(path) => (path.to_string_lossy().len() as u32) * 6,
            SidebarItem::Port(info) => (info.port.to_string().len() as u32) * 8 + 20,
            SidebarItem::Separator => 0,
        };

        base_padding + content_width + 20 // Margem extra
    }
}

/// Cores para renderização de um item
#[derive(Debug, Clone)]
pub struct ItemColors {
    pub background: String,
    pub foreground: String,
    pub accent: String,
}

/// Dimensões para modo compacto
#[derive(Debug, Clone, Copy)]
pub struct CompactDimensions {
    pub width: u32,
    pub icon_size: u32,
    pub item_height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::sidebar::SidebarTabInfo;

    fn create_test_tab_info(id: u32, title: &str) -> SidebarTabInfo {
        SidebarTabInfo {
            id,
            title: title.to_string(),
            is_active: false,
            working_dir: None,
            git_branch: None,
            ports: Vec::new(),
            has_activity: false,
        }
    }

    #[test]
    fn test_ui_creation() {
        let config = SidebarConfig::default();
        let ui = SidebarUI::new(config);
        assert_eq!(ui.rect.width, 250);
    }

    #[test]
    fn test_content_rect() {
        let config = SidebarConfig::default();
        let ui = SidebarUI::new(config);
        let content = ui.content_rect();
        assert_eq!(content.x, 8); // padding
        assert_eq!(content.width, 250 - 16); // width - 2*padding
    }

    #[test]
    fn test_scroll_management() {
        let config = SidebarConfig::default();
        let mut ui = SidebarUI::new(config);

        assert_eq!(ui.scroll_offset(), 0);

        ui.scroll_down(100);
        assert_eq!(ui.scroll_offset(), 0); // Não scrolla se max_scroll é 0

        // Simula items que precisam de scroll
        ui.max_scroll = 500;
        ui.scroll_down(100);
        assert_eq!(ui.scroll_offset(), 100);

        ui.scroll_up(50);
        assert_eq!(ui.scroll_offset(), 50);

        // Não passa do máximo
        ui.scroll_down(1000);
        assert_eq!(ui.scroll_offset(), 500);

        // Não fica negativo
        ui.scroll_up(1000);
        assert_eq!(ui.scroll_offset(), 0);
    }

    #[test]
    fn test_hit_test() {
        let config = SidebarConfig::default();
        let mut ui = SidebarUI::new(config);
        ui.set_rect(Rect::new(0, 0, 250, 600));

        let items = vec![
            SidebarItem::Header("Test".to_string()),
            SidebarItem::Tab(create_test_tab_info(1, "Tab 1")),
        ];

        ui.calculate_item_layout(&items);

        // Dentro da área
        assert!(ui.hit_test(10, 10).is_some());

        // Fora da área
        assert!(ui.hit_test(300, 10).is_none());
    }

    #[test]
    fn test_item_heights() {
        let config = SidebarConfig::default();
        let ui = SidebarUI::new(config);

        assert_eq!(ui.item_height(&SidebarItem::Header("Test".to_string())), 30);
        assert_eq!(
            ui.item_height(&SidebarItem::Tab(create_test_tab_info(1, "Test"))),
            60
        );
        assert_eq!(ui.item_height(&SidebarItem::Separator), 10);
    }

    #[test]
    fn test_preferred_width() {
        let config = SidebarConfig::default();
        let min_width = config.min_width;
        let max_width = config.max_width;
        let ui = SidebarUI::new(config);

        let items = vec![
            SidebarItem::Header("Short".to_string()),
            SidebarItem::Header("This is a very long header text".to_string()),
        ];

        let width = ui.preferred_width(&items);
        assert!(width >= min_width);
        assert!(width <= max_width);
    }
}
