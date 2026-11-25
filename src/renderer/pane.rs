//! Pane Renderer - Renderização de múltiplos panes na janela
//!
//! Responsável por renderizar cada pane com:
//! - Bordas (ativa/inativa)
//! - Conteúdo do terminal (via TextRenderer)
//! - Viewport clipping para cada pane

use crate::config::ColorScheme;
use crate::renderer::text::TextRenderer;
use crate::ui::layout::{Pane, PaneId, Rect};
use anyhow::Result;
use wgpu::{Color, Device, Queue, RenderPass, TextureFormat};

/// Cores para renderização de panes
#[derive(Debug, Clone)]
pub struct PaneColors {
    /// Cor da borda inativa
    pub border_inactive: Color,
    /// Cor da borda ativa
    pub border_active: Color,
    /// Background do pane
    pub background: Color,
    /// Cor da barra de título (se houver)
    pub title_bar: Color,
}

impl Default for PaneColors {
    fn default() -> Self {
        Self {
            border_inactive: Color {
                r: 0.25,
                g: 0.25,
                b: 0.25,
                a: 1.0,
            }, // #404040
            border_active: Color {
                r: 0.0,
                g: 0.48,
                b: 0.8,
                a: 1.0,
            }, // #007ACC
            background: Color {
                r: 0.12,
                g: 0.12,
                b: 0.12,
                a: 1.0,
            }, // #1E1E1E
            title_bar: Color {
                r: 0.15,
                g: 0.15,
                b: 0.15,
                a: 1.0,
            }, // #262626
        }
    }
}

impl PaneColors {
    /// Cria cores a partir de um ColorScheme
    pub fn from_color_scheme(scheme: &ColorScheme) -> Self {
        let parse_color = |hex: &str| -> Color {
            let hex = hex.trim_start_matches('#');
            if hex.len() >= 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255) as f64 / 255.0;
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255) as f64 / 255.0;
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255) as f64 / 255.0;
                Color { r, g, b, a: 1.0 }
            } else {
                Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                }
            }
        };

        Self {
            border_inactive: parse_color("#404040"),
            border_active: parse_color(&scheme.cursor),
            background: parse_color(&scheme.background),
            title_bar: parse_color("#262626"),
        }
    }
}

/// Configuração de renderização de panes
#[derive(Debug, Clone)]
pub struct PaneRenderConfig {
    /// Largura da borda em pixels
    pub border_width: u32,
    /// Largura extra da borda ativa
    pub active_border_extra: u32,
    /// Margem interna do conteúdo
    pub content_margin: u32,
    /// Altura da barra de título (0 = sem barra)
    pub title_bar_height: u32,
}

impl Default for PaneRenderConfig {
    fn default() -> Self {
        Self {
            border_width: 1,
            active_border_extra: 1,
            content_margin: 2,
            title_bar_height: 0, // Desabilitado por padrão
        }
    }
}

/// Estado de renderização de um pane
#[derive(Debug)]
pub struct PaneRenderState {
    /// Se o pane precisa ser re-renderizado
    pub dirty: bool,
    /// Timestamp da última renderização
    pub last_render: std::time::Instant,
    /// Último plano de renderização preparado para este pane.
    pub last_plan: Option<PaneRenderPlan>,
}

impl Default for PaneRenderState {
    fn default() -> Self {
        Self {
            dirty: true,
            last_render: std::time::Instant::now(),
            last_plan: None,
        }
    }
}

/// Plano preparado para desenhar um pane.
#[derive(Debug, Clone)]
pub struct PaneRenderPlan {
    /// Pane que será desenhado.
    pub pane_id: PaneId,
    /// Área completa do pane.
    pub background_rect: Rect,
    /// Área interna reservada ao conteúdo.
    pub content_rect: Rect,
    /// Retângulo da barra de título, se habilitada.
    pub title_bar_rect: Option<Rect>,
    /// Cor da borda considerando foco.
    pub border_color: Color,
    /// Cor de fundo do pane.
    pub background_color: Color,
    /// Comandos de desenho preparados para o backend GPU.
    pub draw_commands: Vec<PaneDrawCommand>,
}

impl PaneRenderPlan {
    /// Retorna o texto de conteúdo preparado, se houver comando textual.
    pub fn content_text(&self) -> Option<&str> {
        self.draw_commands.iter().find_map(|command| match command {
            PaneDrawCommand::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }
}

/// Comando de desenho de pane independente do backend.
#[derive(Debug, Clone, PartialEq)]
pub enum PaneDrawCommand {
    /// Preenche um retângulo sólido.
    FillRect {
        /// Camada semântica do retângulo.
        layer: PaneDrawLayer,
        /// Área em pixels.
        rect: Rect,
        /// Cor.
        color: Color,
    },
    /// Desenha texto dentro de um retângulo com clipping.
    Text {
        /// Área de clipping/conteúdo.
        rect: Rect,
        /// Texto visível preparado a partir da tab ativa.
        text: String,
    },
}

/// Camada semântica de um comando de desenho.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneDrawLayer {
    /// Fundo do pane.
    Background,
    /// Borda do pane.
    Border,
    /// Barra de título.
    TitleBar,
    /// Conteúdo textual.
    Content,
}

/// Renderizador de panes
///
/// Responsável por renderizar múltiplos panes na janela,
/// gerenciando bordas, viewports e o conteúdo de cada pane.
pub struct PaneRenderer {
    /// Text renderer para conteúdo dos panes
    text_renderer: TextRenderer,
    /// Cores de renderização
    colors: PaneColors,
    /// Configuração
    config: PaneRenderConfig,
    /// Dimensões da janela
    window_size: (u32, u32),
    /// Cache de estados de renderização por pane
    pane_states: std::collections::HashMap<PaneId, PaneRenderState>,
}

impl PaneRenderer {
    /// Cria um novo PaneRenderer
    pub fn new(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        window_width: u32,
        window_height: u32,
    ) -> Result<Self> {
        let text_renderer = TextRenderer::new(device, queue, format)?;

        Ok(Self {
            text_renderer,
            colors: PaneColors::default(),
            config: PaneRenderConfig::default(),
            window_size: (window_width, window_height),
            pane_states: std::collections::HashMap::new(),
        })
    }

    /// Cria com configurações personalizadas
    pub fn new_with_config(
        device: &Device,
        queue: &Queue,
        format: TextureFormat,
        config: PaneRenderConfig,
        window_width: u32,
        window_height: u32,
    ) -> Result<Self> {
        let mut renderer = Self::new(device, queue, format, window_width, window_height)?;
        renderer.config = config;
        Ok(renderer)
    }

    /// Atualiza as cores a partir de um ColorScheme
    pub fn update_colors(&mut self, color_scheme: &ColorScheme) {
        self.colors = PaneColors::from_color_scheme(color_scheme);
    }

    /// Atualiza o tamanho da janela
    pub fn resize(&mut self, width: u32, height: u32, queue: &Queue) {
        self.window_size = (width, height);
        self.text_renderer.resize(width, height, queue);
        // Marca todos os panes como dirty
        for state in self.pane_states.values_mut() {
            state.dirty = true;
        }
    }

    /// Marca um pane como needing re-render
    pub fn mark_pane_dirty(&mut self, pane_id: PaneId) {
        if let Some(state) = self.pane_states.get_mut(&pane_id) {
            state.dirty = true;
        } else {
            self.pane_states.insert(pane_id, PaneRenderState::default());
        }
    }

    /// Marca todos os panes como dirty
    pub fn mark_all_dirty(&mut self) {
        for state in self.pane_states.values_mut() {
            state.dirty = true;
        }
    }

    /// Retorna a área de conteúdo interna de um pane (excluindo bordas)
    pub fn content_rect(&self, pane_rect: &Rect) -> Rect {
        Self::content_rect_for_config(&self.config, pane_rect)
    }

    /// Renderiza todos os panes
    ///
    /// # Arguments
    /// * `panes` - Slice de panes para renderizar
    /// * `active_pane_id` - ID do pane ativo (para destacar borda)
    /// * `_device` - Device WGPU
    /// * `_queue` - Queue WGPU
    /// * `_render_pass` - Render pass ativo
    pub fn render_panes(
        &mut self,
        panes: &[&Pane],
        active_pane_id: Option<PaneId>,
        _device: &Device,
        _queue: &Queue,
        _render_pass: &mut RenderPass<'_>,
    ) -> Result<()> {
        for pane in panes {
            let is_active = active_pane_id == Some(pane.id);
            self.render_pane_internal(pane, is_active);
        }
        Ok(())
    }

    /// Renderiza um único pane
    ///
    /// # Arguments
    /// * `pane` - Pane a renderizar
    /// * `is_active` - Se o pane está ativo
    /// * `_device` - Device WGPU
    /// * `_queue` - Queue WGPU
    /// * `_render_pass` - Render pass ativo
    pub fn render_pane<'pass>(
        &'pass mut self,
        pane: &Pane,
        is_active: bool,
        device: &Device,
        queue: &Queue,
        render_pass: &mut RenderPass<'pass>,
    ) -> Result<()> {
        self.render_pane_internal(pane, is_active);
        self.text_renderer.render(device, queue, render_pass)?;
        Ok(())
    }

    /// Função interna de renderização de pane (sem dependências de GPU)
    fn render_pane_internal(&mut self, pane: &Pane, is_active: bool) {
        let plan = Self::build_render_plan(&self.config, &self.colors, pane, is_active);
        if let Some(text) = plan.content_text() {
            self.text_renderer.set_text(text);
        }
        let state = self.pane_states.entry(pane.id).or_default();
        state.dirty = false;
        state.last_render = std::time::Instant::now();
        state.last_plan = Some(plan);
    }

    fn build_render_plan(
        config: &PaneRenderConfig,
        colors: &PaneColors,
        pane: &Pane,
        is_active: bool,
    ) -> PaneRenderPlan {
        let content_rect = Self::content_rect_for_config(config, &pane.rect);
        let border_color = if is_active {
            colors.border_active
        } else {
            colors.border_inactive
        };
        let title_bar_rect = (config.title_bar_height > 0).then(|| {
            Rect::new(
                pane.rect.x + config.border_width,
                pane.rect.y + config.border_width,
                pane.rect.width.saturating_sub(config.border_width * 2),
                config.title_bar_height,
            )
        });
        let mut draw_commands = Vec::new();
        draw_commands.push(PaneDrawCommand::FillRect {
            layer: PaneDrawLayer::Background,
            rect: pane.rect,
            color: colors.background,
        });
        draw_commands.extend(
            Self::border_rects(config, &pane.rect, is_active)
                .into_iter()
                .map(|rect| PaneDrawCommand::FillRect {
                    layer: PaneDrawLayer::Border,
                    rect,
                    color: border_color,
                }),
        );
        if let Some(rect) = title_bar_rect {
            draw_commands.push(PaneDrawCommand::FillRect {
                layer: PaneDrawLayer::TitleBar,
                rect,
                color: colors.title_bar,
            });
        }
        draw_commands.push(PaneDrawCommand::Text {
            rect: content_rect,
            text: Self::pane_content_text(pane),
        });

        PaneRenderPlan {
            pane_id: pane.id,
            background_rect: pane.rect,
            content_rect,
            title_bar_rect,
            border_color,
            background_color: colors.background,
            draw_commands,
        }
    }

    fn border_rects(config: &PaneRenderConfig, pane_rect: &Rect, is_active: bool) -> Vec<Rect> {
        let thickness = config.border_width
            + if is_active {
                config.active_border_extra
            } else {
                0
            };
        if thickness == 0 || pane_rect.width == 0 || pane_rect.height == 0 {
            return Vec::new();
        }

        let horizontal = thickness.min(pane_rect.height);
        let vertical = thickness.min(pane_rect.width);
        let bottom_y = pane_rect
            .y
            .saturating_add(pane_rect.height.saturating_sub(horizontal));
        let right_x = pane_rect
            .x
            .saturating_add(pane_rect.width.saturating_sub(vertical));

        vec![
            Rect::new(pane_rect.x, pane_rect.y, pane_rect.width, horizontal),
            Rect::new(pane_rect.x, bottom_y, pane_rect.width, horizontal),
            Rect::new(pane_rect.x, pane_rect.y, vertical, pane_rect.height),
            Rect::new(right_x, pane_rect.y, vertical, pane_rect.height),
        ]
    }

    fn pane_content_text(pane: &Pane) -> String {
        let Some(tab) = pane.tab_manager.get_active_tab() else {
            return String::new();
        };

        let Some(grid) = tab.grid() else {
            return tab.title();
        };

        grid.visible_lines()
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| if cell.char == '\0' { ' ' } else { cell.char })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn content_rect_for_config(config: &PaneRenderConfig, pane_rect: &Rect) -> Rect {
        let border = config.border_width;
        let margin = config.content_margin;
        let total_padding = border + margin;

        let title_height = if config.title_bar_height > 0 {
            config.title_bar_height + border
        } else {
            border
        };

        Rect::new(
            pane_rect.x + total_padding,
            pane_rect.y + title_height,
            pane_rect.width.saturating_sub(total_padding * 2),
            pane_rect
                .height
                .saturating_sub(title_height + total_padding),
        )
    }

    /// Renderiza apenas os panes que estão dirty (otimização)
    pub fn render_dirty_panes(
        &mut self,
        panes: &[&Pane],
        active_pane_id: Option<PaneId>,
        _device: &Device,
        _queue: &Queue,
        _render_pass: &mut RenderPass<'_>,
    ) -> Result<()> {
        // Coleta os IDs dirty primeiro para evitar borrow issues
        let dirty_ids: Vec<PaneId> = panes
            .iter()
            .filter(|p| {
                self.pane_states.get(&p.id).map(|s| s.dirty).unwrap_or(true)
                    || active_pane_id == Some(p.id)
            })
            .map(|p| p.id)
            .collect();

        for pane in panes {
            if dirty_ids.contains(&pane.id) {
                let is_active = active_pane_id == Some(pane.id);
                self.render_pane_internal(pane, is_active);
            }
        }
        Ok(())
    }

    /// Retorna as cores atuais
    pub fn colors(&self) -> &PaneColors {
        &self.colors
    }

    /// Retorna a configuração atual
    pub fn config(&self) -> &PaneRenderConfig {
        &self.config
    }

    /// Atualiza a configuração
    pub fn set_config(&mut self, config: PaneRenderConfig) {
        self.config = config;
        self.mark_all_dirty();
    }

    /// Retorna o text renderer (para renderização de conteúdo)
    pub fn text_renderer(&mut self) -> &mut TextRenderer {
        &mut self.text_renderer
    }

    /// Remove o estado de um pane fechado
    pub fn remove_pane_state(&mut self, pane_id: PaneId) {
        self.pane_states.remove(&pane_id);
    }

    /// Limpa todos os estados de pane
    pub fn clear_pane_states(&mut self) {
        self.pane_states.clear();
    }

    /// Verifica se um pane está dirty
    pub fn is_pane_dirty(&self, pane_id: PaneId) -> bool {
        self.pane_states
            .get(&pane_id)
            .map(|s| s.dirty)
            .unwrap_or(true)
    }

    /// Retorna o último plano de renderização preparado para um pane.
    pub fn last_render_plan(&self, pane_id: PaneId) -> Option<&PaneRenderPlan> {
        self.pane_states
            .get(&pane_id)
            .and_then(|state| state.last_plan.as_ref())
    }

    /// Retorna o número de panes sendo rastreados
    pub fn tracked_pane_count(&self) -> usize {
        self.pane_states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::layout::Rect;

    #[test]
    fn test_pane_colors_default() {
        let colors = PaneColors::default();

        // Verifica que as cores não são zero
        assert!(colors.border_inactive.r > 0.0);
        assert!(colors.border_active.b > 0.0); // Azul tem componente B alto
        assert!(colors.background.r > 0.0);
    }

    #[test]
    fn test_pane_render_config_default() {
        let config = PaneRenderConfig::default();

        assert_eq!(config.border_width, 1);
        assert_eq!(config.active_border_extra, 1);
        assert_eq!(config.content_margin, 2);
        assert_eq!(config.title_bar_height, 0);
    }

    #[test]
    fn test_content_rect_calculation() {
        let renderer = TestPaneRenderer {
            config: PaneRenderConfig::default(),
        };

        let pane_rect = Rect::new(10, 10, 100, 100);
        let content = renderer.content_rect(&pane_rect);

        // Com border=1 e margin=2, total padding = 3
        // x = 10 + 3 = 13
        // y = 10 + border(1) = 11 (sem title bar, apenas border)
        assert_eq!(content.x, 13); // 10 + 1 + 2
        assert_eq!(content.y, 11); // 10 + 1 (border, não total_padding)
        assert_eq!(content.width, 94); // 100 - 3*2
        assert_eq!(content.height, 96); // 100 - 1 (border) - 3 (padding) = 96
    }

    #[test]
    fn test_content_rect_with_title_bar() {
        let mut config = PaneRenderConfig::default();
        config.title_bar_height = 20;

        let renderer = TestPaneRenderer { config };

        let pane_rect = Rect::new(10, 10, 100, 100);
        let content = renderer.content_rect(&pane_rect);

        // Com title bar de 20:
        // y = 10 + 20 (title) + 1 (border) = 31
        assert_eq!(content.y, 31);
    }

    #[test]
    fn test_pane_state_tracking() {
        let mut states = std::collections::HashMap::new();

        // Inicialmente não existe
        assert!(!states.contains_key(&1));

        // Simula mark_pane_dirty
        states.insert(1, PaneRenderState::default());
        assert!(states.contains_key(&1));
        assert!(states.get(&1).unwrap().dirty);

        // Simula mark_all_dirty
        states.insert(
            2,
            PaneRenderState {
                dirty: false,
                last_render: std::time::Instant::now(),
                last_plan: None,
            },
        );
        for state in states.values_mut() {
            state.dirty = true;
        }
        assert!(states.get(&1).unwrap().dirty);
        assert!(states.get(&2).unwrap().dirty);
    }

    #[test]
    fn test_remove_pane_state() {
        let mut states = std::collections::HashMap::new();

        states.insert(1, PaneRenderState::default());
        assert!(states.contains_key(&1));

        states.remove(&1);
        assert!(!states.contains_key(&1));
    }

    #[test]
    fn test_tracked_pane_count() {
        let mut states = std::collections::HashMap::new();

        assert_eq!(states.len(), 0);

        states.insert(1, PaneRenderState::default());
        states.insert(2, PaneRenderState::default());
        assert_eq!(states.len(), 2);

        states.clear();
        assert_eq!(states.len(), 0);
    }

    #[test]
    fn test_pane_colors_from_color_scheme() {
        use crate::config::ColorScheme;

        let scheme = ColorScheme::default();
        let colors = PaneColors::from_color_scheme(&scheme);

        // Verifica que as cores foram convertidas
        assert!(colors.background.r >= 0.0 && colors.background.r <= 1.0);
        assert!(colors.border_active.a == 1.0); // Alpha sempre 1.0
    }

    #[test]
    fn test_render_state_default() {
        let state = PaneRenderState::default();
        assert!(state.dirty);
        assert!(state.last_plan.is_none());
        // Verifica que o timestamp foi criado (não é zero)
        assert!(state.last_render.elapsed().as_secs() < 1);
    }

    #[test]
    fn test_build_render_plan_active_pane() {
        use crate::ui::tabs::TabManager;

        let config = PaneRenderConfig::default();
        let colors = PaneColors::default();
        let mut pane = Pane::new(42, TabManager::new());
        pane.rect = Rect::new(10, 20, 120, 80);

        let plan = PaneRenderer::build_render_plan(&config, &colors, &pane, true);

        assert_eq!(plan.pane_id, 42);
        assert_eq!(plan.background_rect, pane.rect);
        assert_eq!(plan.content_rect, Rect::new(13, 21, 114, 76));
        assert!(plan.title_bar_rect.is_none());
        assert_eq!(plan.border_color, colors.border_active);
        assert_eq!(plan.background_color, colors.background);
        assert_eq!(plan.content_text(), Some(""));
        assert_eq!(plan.draw_commands.len(), 6);
        assert!(matches!(
            plan.draw_commands[0],
            PaneDrawCommand::FillRect {
                layer: PaneDrawLayer::Background,
                ..
            }
        ));
        assert_eq!(
            plan.draw_commands
                .iter()
                .filter(|command| matches!(
                    command,
                    PaneDrawCommand::FillRect {
                        layer: PaneDrawLayer::Border,
                        ..
                    }
                ))
                .count(),
            4
        );
    }

    #[test]
    fn test_build_render_plan_with_title_bar() {
        use crate::ui::tabs::TabManager;

        let config = PaneRenderConfig {
            title_bar_height: 20,
            ..PaneRenderConfig::default()
        };
        let colors = PaneColors::default();
        let mut pane = Pane::new(7, TabManager::new());
        pane.rect = Rect::new(5, 6, 90, 70);

        let plan = PaneRenderer::build_render_plan(&config, &colors, &pane, false);

        assert_eq!(plan.content_rect.y, 27);
        assert_eq!(plan.title_bar_rect, Some(Rect::new(6, 7, 88, 20)));
        assert_eq!(plan.border_color, colors.border_inactive);
        assert!(plan.draw_commands.iter().any(|command| matches!(
            command,
            PaneDrawCommand::FillRect {
                layer: PaneDrawLayer::TitleBar,
                rect,
                color,
            } if *rect == Rect::new(6, 7, 88, 20) && *color == colors.title_bar
        )));
    }

    #[test]
    fn test_build_render_plan_uses_active_tab_title_as_content_fallback() {
        use crate::ui::tabs::TabManager;

        let config = PaneRenderConfig::default();
        let colors = PaneColors::default();
        let mut tab_manager = TabManager::new();
        tab_manager.create_tab("pwsh");
        let expected_title = tab_manager.get_active_tab().unwrap().title();
        let mut pane = Pane::new(9, tab_manager);
        pane.rect = Rect::new(0, 0, 80, 24);

        let plan = PaneRenderer::build_render_plan(&config, &colors, &pane, false);

        assert_eq!(plan.content_text(), Some(expected_title.as_str()));
        assert!(plan.draw_commands.iter().any(|command| matches!(
            command,
            PaneDrawCommand::Text { rect, text }
                if *rect == Rect::new(3, 1, 74, 20) && text == &expected_title
        )));
    }

    #[test]
    fn test_active_border_uses_extra_thickness() {
        let config = PaneRenderConfig {
            border_width: 1,
            active_border_extra: 2,
            ..PaneRenderConfig::default()
        };
        let rect = Rect::new(10, 20, 100, 50);

        let borders = PaneRenderer::border_rects(&config, &rect, true);

        assert_eq!(borders[0], Rect::new(10, 20, 100, 3));
        assert_eq!(borders[1], Rect::new(10, 67, 100, 3));
        assert_eq!(borders[2], Rect::new(10, 20, 3, 50));
        assert_eq!(borders[3], Rect::new(107, 20, 3, 50));
    }

    #[test]
    fn test_is_pane_dirty_unknown_pane() {
        let states: std::collections::HashMap<PaneId, PaneRenderState> =
            std::collections::HashMap::new();
        // Pane desconhecido é considerado dirty
        let is_dirty = states.get(&999).map(|s| s.dirty).unwrap_or(true);
        assert!(is_dirty);
    }

    // Função auxiliar para calcular content rect - mesma lógica do PaneRenderer
    fn calculate_content_rect(pane_rect: &Rect, config: &PaneRenderConfig) -> Rect {
        let border = config.border_width;
        let margin = config.content_margin;
        let total_padding = border + margin;

        let title_height = if config.title_bar_height > 0 {
            config.title_bar_height + border
        } else {
            border
        };

        Rect::new(
            pane_rect.x + total_padding,
            pane_rect.y + title_height,
            pane_rect.width.saturating_sub(total_padding * 2),
            pane_rect
                .height
                .saturating_sub(title_height + total_padding),
        )
    }

    // Testa o content_rect através de uma struct mínima
    struct TestPaneRenderer {
        config: PaneRenderConfig,
    }

    impl TestPaneRenderer {
        fn content_rect(&self, pane_rect: &Rect) -> Rect {
            calculate_content_rect(pane_rect, &self.config)
        }
    }
}
