//! Window Manager - Suporte a múltiplas janelas
//!
//! Este módulo implementa o gerenciamento de múltiplas janelas,
//! onde cada janela tem suas próprias tabs e splits independentes.
//!
//! # Arquitetura
//!
//! ```text
//! Application
//!     |
//!     v
//! WindowManager
//!     |
//!     ├── Window 1 (ativa)
//!     |       ├── TabManager
//!     |       ├── LayoutManager (splits)
//!     |       └── winit Window
//!     |
//!     ├── Window 2
//!     |       ├── TabManager
//!     |       ├── LayoutManager
//!     |       └── winit Window
//!     |
//!     └── Window N
//! ```

use crate::app::AppState;
use crate::config::{Config, WindowConfig};
use crate::renderer::cells::{build_line, DecorationKind};
use crate::renderer::gpu::GpuState;
use crate::renderer::quad::QuadBatch;
use crate::renderer::text::RenderColors;
use crate::renderer::{GridLayout, TAB_BAR_BUTTON_WIDTH, TAB_BAR_HEIGHT, TAB_WIDTH};
use crate::ui::command_palette::CommandPalette;
use crate::ui::hover::{
    card_button_at, card_button_rect, footer_button_at, footer_button_rect, footer_rect,
    pane_close_rect, CardButton, FooterButton, Hover, CARD_BUTTONS_WIDTH, FOOTER_HEIGHT,
};
use crate::ui::layout::LayoutManager;
use crate::ui::overlay_hits::{follow_selection, ListGeometry, OverlayHits, OverlayKind, Rect};
use crate::ui::pane_layout::{CellRect, PaneId, SplitDirection};
use crate::ui::settings::SettingsScreen;
use crate::ui::sidebar_rows::{DropHint, SidebarRow};
use crate::ui::tabs::DropTarget;
use crate::ui::tabs::TabManager;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::EventLoopProxy;
use winit::window::{Window as WinitWindow, WindowId as WinitWindowId};

/// Mistura duas cores e retorna um `glyphon::Color`.
fn blend_color(bg: &glyphon::Color, fg: &glyphon::Color, t: f32) -> glyphon::Color {
    let inv = 1.0 - t;
    glyphon::Color::rgb(
        (bg.r() as f32 * inv + fg.r() as f32 * t) as u8,
        (bg.g() as f32 * inv + fg.g() as f32 * t) as u8,
        (bg.b() as f32 * inv + fg.b() as f32 * t) as u8,
    )
}

/// Identificador único de uma janela
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(u64);

impl WindowId {
    /// Cria um novo ID único
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Retorna o valor interno
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl Default for WindowId {
    fn default() -> Self {
        Self::new()
    }
}

/// Estado de uma janela individual
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// Janela está ativa (focada)
    Active,
    /// Janela está inativa
    Inactive,
    /// Janela está minimizada
    Minimized,
    /// Janela está maximizada
    Maximized,
    /// Janela está fechando
    Closing,
}

impl WindowState {
    /// Verifica se a janela está ativa
    pub fn is_active(&self) -> bool {
        matches!(self, WindowState::Active)
    }
}

/// Eventos de comunicação entre janelas
#[derive(Debug, Clone)]
pub enum WindowEventMsg {
    /// Criar nova janela
    CreateWindow {
        position: Option<(i32, i32)>,
        size: (u32, u32),
    },
    /// Fechar janela
    CloseWindow(WindowId),
    /// Focar janela
    FocusWindow(WindowId),
    /// Mover janela para posição
    MoveWindow(WindowId, i32, i32),
    /// Redimensionar janela
    ResizeWindow(WindowId, u32, u32),
    /// Broadcast de evento para todas as janelas
    Broadcast(String),
    /// Algum PTY produziu saída (ou terminou): é hora de redesenhar.
    PtyOutput,
    /// Chamada da Socket API para aplicar no estado da janela ativa.
    Api {
        method: String,
        params: serde_json::Value,
        reply: std::sync::mpsc::Sender<
            Option<Result<serde_json::Value, crate::multiplexer::socket_api::ApiError>>,
        >,
    },
    /// Janela pedida ao modo servidor. A resposta (ms até o primeiro frame,
    /// ou o erro) vai pelo `reply` depois que a janela desenha.
    ServerOpen {
        cwd: Option<std::path::PathBuf>,
        close_after_first_frame: bool,
        reply: std::sync::mpsc::Sender<Result<u64, String>>,
    },
}

/// Dados necessários para criar uma janela
pub struct WindowCreateInfo {
    /// Título da janela
    pub title: String,
    /// Tamanho inicial (width, height)
    pub size: (u32, u32),
    /// Posição inicial (opcional)
    pub position: Option<(i32, i32)>,
    /// Configuração da janela
    pub config: WindowConfig,
    /// Diretório onde o shell da primeira tab começa (`None`: o do processo).
    pub cwd: Option<std::path::PathBuf>,
}

impl Default for WindowCreateInfo {
    fn default() -> Self {
        Self {
            title: "Code Terminal".to_string(),
            size: (800, 600),
            position: None,
            config: WindowConfig::default(),
            cwd: None,
        }
    }
}

/// Handle para a thread de renderização (placeholder)
pub struct RenderHandle {
    window_id: WindowId,
}

impl RenderHandle {
    pub fn new(window_id: WindowId) -> Self {
        Self { window_id }
    }

    pub fn resize(&self, width: u32, height: u32) -> Result<()> {
        debug!(window_id = ?self.window_id, width, height, "Render resize requested");
        Ok(())
    }
}

/// Representa uma janela do terminal
/// Cada janela tem seu próprio TabManager e LayoutManager
pub struct Window {
    /// ID único da janela
    pub id: WindowId,
    /// Referência à winit window
    pub winit_window: Arc<WinitWindow>,
    /// Gerenciador de tabs desta janela
    pub tab_manager: TabManager,
    /// Gerenciador de layout/splits desta janela
    pub layout_manager: LayoutManager,
    /// Estado atual da janela
    pub state: WindowState,
    /// Configuração da janela
    pub config: WindowConfig,
    /// Handle para renderização
    render_handle: Option<RenderHandle>,
    /// Estado da aplicação específico desta janela
    pub app_state: Option<AppState>,
    /// GPU state para renderização
    pub gpu_state: Option<GpuState>,
    /// Painéis modais do último frame, para o mouse (ver `overlay_hits`).
    pub overlay_hits: OverlayHits,
}

impl Window {
    /// Cria uma nova janela
    pub fn new(id: WindowId, winit_window: Arc<WinitWindow>, config: WindowConfig) -> Result<Self> {
        let tab_manager = TabManager::new();
        let layout_manager = LayoutManager::new(800, 600);

        Ok(Self {
            id,
            winit_window,
            tab_manager,
            layout_manager,
            state: WindowState::Active,
            config,
            render_handle: None,
            app_state: None,
            gpu_state: None,
            overlay_hits: OverlayHits::new(),
        })
    }

    /// Define o estado da janela
    pub fn set_state(&mut self, state: WindowState) {
        self.state = state;
        debug!(window_id = ?self.id, state = ?state, "Window state changed");
    }

    /// Verifica se a janela está ativa
    pub fn is_active(&self) -> bool {
        self.state == WindowState::Active
    }

    /// Retorna o tamanho da janela
    pub fn size(&self) -> PhysicalSize<u32> {
        self.winit_window.inner_size()
    }

    /// Retorna a posição da janela
    pub fn position(&self) -> Option<PhysicalPosition<i32>> {
        self.winit_window.outer_position().ok()
    }

    /// Solicita atenção do usuário (flash/bounce)
    pub fn request_attention(&self) {
        self.winit_window.request_user_attention(None);
    }

    /// Define o título da janela
    pub fn set_title(&self, title: &str) {
        self.winit_window.set_title(title);
    }

    /// Atualiza o título baseado na tab ativa
    pub fn update_title(&self) {
        if let Some(tab) = self.tab_manager.get_active_tab() {
            let title = format!("{} - Code Terminal", tab.title());
            self.set_title(&title);
        } else {
            self.set_title("Code Terminal");
        }
    }

    /// Processa eventos de janela do winit
    pub fn handle_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::Resized(size) => {
                debug!(window_id = ?self.id, size = ?size, "Window resized");
                // Atualiza layout_manager quando o método resize for implementado
                // self.layout_manager.resize(size.width, size.height);

                // Notifica render thread
                if let Some(ref handle) = self.render_handle {
                    let _ = handle.resize(size.width, size.height);
                }

                // O terminal é ajustado em `fit_terminal_to_window`, depois que
                // o GPU mede a fonte; aqui não sabemos o tamanho real da célula.
            }
            WindowEvent::Moved(position) => {
                debug!(window_id = ?self.id, position = ?position, "Window moved");
            }
            WindowEvent::Focused(focused) => {
                self.set_state(if *focused {
                    WindowState::Active
                } else {
                    WindowState::Inactive
                });
            }
            WindowEvent::CloseRequested => {
                self.set_state(WindowState::Closing);
            }
            _ => {}
        }
    }

    /// Inicializa o estado da aplicação para esta janela
    pub fn initialize_app_state(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.initialize_app_state_in(cols, rows, None)
    }

    /// Como [`Window::initialize_app_state`], com o shell começando em `cwd`.
    pub fn initialize_app_state_in(
        &mut self,
        cols: u16,
        rows: u16,
        cwd: Option<&std::path::Path>,
    ) -> Result<()> {
        self.app_state = Some(AppState::new_in(cols, rows, cwd)?);
        Ok(())
    }

    /// Retorna referência ao estado da aplicação
    pub fn app_state(&self) -> Option<&AppState> {
        self.app_state.as_ref()
    }

    /// Retorna referência mutável ao estado da aplicação
    pub fn app_state_mut(&mut self) -> Option<&mut AppState> {
        self.app_state.as_mut()
    }

    /// Inicializa o GPU state com a configuração fornecida.
    pub async fn initialize_gpu(
        &mut self,
        app_config: Option<(&crate::config::FontConfig, &crate::config::ColorScheme)>,
    ) -> Result<()> {
        let gpu = match app_config {
            Some((font, colors)) => {
                GpuState::new_with_config(self.winit_window.clone(), font, colors).await?
            }
            None => GpuState::new(self.winit_window.clone()).await?,
        };
        self.gpu_state = Some(gpu);
        self.render_handle = Some(RenderHandle::new(self.id));
        Ok(())
    }

    /// Geometria do grid nesta janela. Depende da fonte medida no GPU.
    pub fn grid_layout(&self) -> Option<GridLayout> {
        let inset = self
            .app_state
            .as_ref()
            .map_or(0.0, |app| app.sidebar_width());
        self.gpu_state.as_ref().map(|gpu| {
            let (cell_width, cell_height) = gpu.cell_size();
            GridLayout::below_tab_bar(cell_width, cell_height).with_left_inset(inset)
        })
    }

    /// Ajusta colunas/linhas do terminal ao tamanho atual da janela.
    pub fn fit_terminal_to_window(&mut self) {
        let Some(layout) = self.grid_layout() else {
            return;
        };
        let size = self.winit_window.inner_size();
        let (cols, rows) = layout.grid_size(size.width, size.height);
        if let Some(ref mut app) = self.app_state {
            if app.terminal_size != (cols, rows) {
                if let Err(e) = app.resize(cols, rows) {
                    warn!(error = %e, cols, rows, "Failed to fit terminal to window");
                }
            }
        }
    }

    /// Renderiza o conteúdo da janela
    pub fn render(&mut self) -> Result<()> {
        let mut refit = false;
        if let (Some(gpu), Some(app)) = (self.gpu_state.as_mut(), self.app_state.as_mut()) {
            if app.config_dirty {
                gpu.update_font(&app.config.font);
                gpu.update_colors(&app.config.colors);
                app.refresh_theme_colors();
                app.config_dirty = false;
                refit = true;
            }
        }
        // Sempre confere: a sidebar ou a fonte podem ter mudado a área útil.
        let _ = refit;
        self.fit_terminal_to_window();

        let cell = self.gpu_state.as_ref().map(|gpu| gpu.cell_size());
        if let Some(app) = self.app_state.as_mut() {
            app.mark_visible_seen();
            app.refresh_sidebar(false);
            app.refresh_pane_programs();
            if let Some((cw, ch)) = cell {
                app.set_cell_pixel_size(cw, ch);
            }
        }
        let window_size = self.winit_window.inner_size();
        let is_focused = self.state.is_active();
        let (Some(gpu), Some(app)) = (self.gpu_state.as_mut(), self.app_state.as_ref()) else {
            return Ok(());
        };

        let (under, over, hits) = compose_frame_with_hits(
            app,
            gpu,
            window_size.width as f32,
            window_size.height as f32,
            is_focused,
        );
        self.overlay_hits = hits;

        let result = gpu.render_layers(Some(&under), Some(&over));
        if let Some(app) = self.app_state.as_mut() {
            // A rolagem que o desenho usou vira a base do próximo frame.
            app.remember_overlay_scroll(&self.overlay_hits);
        }
        let Some(gpu) = self.gpu_state.as_mut() else {
            return Ok(());
        };
        match result {
            Ok(()) => {}
            Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                gpu.resize(window_size);
                self.winit_window.request_redraw();
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                error!("OutOfMemory in GPU rendering");
                return Err(anyhow::anyhow!("GPU OutOfMemory"));
            }
            Err(wgpu::SurfaceError::Timeout) => {
                // A janela ainda não estava pronta para apresentar (comum no
                // primeiro frame): tenta no próximo, em vez de ficar sem
                // desenhar até o próximo evento.
                warn!("Surface timeout, retrying on the next frame");
                self.winit_window.request_redraw();
            }
            Err(e) => {
                warn!("Surface error: {:?}", e);
            }
        }
        Ok(())
    }

    /// Processa output da tab ativa
    pub fn process_output(&mut self) {
        if let Some(ref mut app) = self.app_state {
            if let Some(ref mut tab) = app.active_tab_mut() {
                if let Some(ref mut buffer) = tab.buffer {
                    buffer.process_output();
                }
            }
        }
    }

    /// Processa cliques do mouse na UI (tabs, botões).
    /// Retorna true se o clique foi consumido e não deve ir para o terminal.
    pub fn handle_mouse_click(&mut self, x: f64, y: f64) -> bool {
        let window_height = self.winit_window.inner_size().height as f32;
        if let Some(app) = self.app_state.as_mut() {
            let (fx, fy) = (x as f32, y as f32);
            if app.is_sidebar_visible() {
                if let Some(button) = footer_button_at(app.sidebar_width(), window_height, fx, fy) {
                    app.footer_action(button);
                    return true;
                }
                if footer_rect(app.sidebar_width(), window_height).contains(fx, fy) {
                    return true;
                }
            }
            if let Some(row) = sidebar_row_at(app, fx, fy) {
                match card_button_at(row.y, app.sidebar_width(), fx, fy) {
                    Some(CardButton::Close) => {
                        app.close_tab_from_mouse(row.tab);
                        return true;
                    }
                    Some(CardButton::More) => {
                        app.open_tab_menu(row.tab, fx, fy);
                        return true;
                    }
                    None => {}
                }
                if row.toggle_contains(fx, fy) {
                    app.toggle_tab_group(row.tab);
                } else if app.sidebar_press(row.tab, fx, fy) {
                    // Duplo clique no cartão: renomeia a tab.
                    app.start_tab_rename(row.tab);
                }
                return true;
            }
            if app.is_sidebar_visible() && (x as f32) < app.sidebar_width() {
                return true;
            }
        }
        let y = y as f32;
        if y > TAB_BAR_HEIGHT as f32 {
            // × do painel (tab dividida, mouse em cima do painel).
            let layout = self.grid_layout();
            if let (Some(app), Some(layout)) = (self.app_state.as_mut(), layout) {
                if let Hover::Pane { pane, close: true } =
                    hover_at(app, &layout, window_height, x as f32, y)
                {
                    app.request_close_pane(pane);
                    return true;
                }
                // Clique na barra de título dá foco ao painel.
                if let Some(pane) = pane_title_at(app, &layout, x as f32, y) {
                    app.focus_pane(pane);
                    return true;
                }
            }
            return false;
        }

        if let Some(ref mut app) = self.app_state {
            let width = self.winit_window.inner_size().width as f32;
            let button_width = TAB_BAR_BUTTON_WIDTH as f32;

            // Botão de menu no canto direito.
            let menu_x = width - button_width;
            if x as f32 >= menu_x {
                app.toggle_command_palette();
                return true;
            }

            // Botão de nova tab ao lado do menu.
            let new_tab_x = width - button_width * 2.0;
            if x as f32 >= new_tab_x {
                let _ = app.create_new_tab();
                return true;
            }

            // Clique em uma tab existente.
            let tab_index = (x as f32 / TAB_WIDTH as f32) as usize;
            if !app.is_sidebar_visible() && tab_index < app.tab_count() {
                app.go_to_tab(tab_index);
                return true;
            }
        }

        false
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        info!(window_id = ?self.id, "Window dropped");
    }
}

/// O que está sob o mouse em (x, y): cartão da sidebar (e botão dele) ou
/// painel de uma tab dividida (e o × dele). Desenho e clique usam isto.
pub fn hover_at(app: &AppState, layout: &GridLayout, window_height: f32, x: f32, y: f32) -> Hover {
    if app.is_sidebar_visible() {
        if let Some(button) = footer_button_at(app.sidebar_width(), window_height, x, y) {
            return Hover::Footer(button);
        }
        if footer_rect(app.sidebar_width(), window_height).contains(x, y) {
            return Hover::None;
        }
    }
    if let Some(row) = sidebar_row_at(app, x, y) {
        let Some(tab) = app.tab_manager.tabs().get(row.tab) else {
            return Hover::None;
        };
        return Hover::Card {
            tab: tab.id,
            button: card_button_at(row.y, app.sidebar_width(), x, y),
        };
    }
    let Some(tab) = app.active_tab() else {
        return Hover::None;
    };
    if tab.pane_count() < 2 || y < layout.origin_y || x < layout.origin_x {
        return Hover::None;
    }
    for (id, rect) in tab.layout().rects(app.pane_area()) {
        let (px, py, w, h) = pane_pixels(layout, rect);
        if Rect::new(px, py, w, h).contains(x, y) {
            return Hover::Pane {
                pane: id,
                close: pane_close_rect(px, py, w, layout.cell_height).contains(x, y),
            };
        }
    }
    Hover::None
}

/// Painel cuja barra de título (tab dividida, #32) está em (x, y).
pub fn pane_title_at(app: &AppState, layout: &GridLayout, x: f32, y: f32) -> Option<PaneId> {
    let tab = app.active_tab().filter(|tab| tab.pane_count() > 1)?;
    tab.layout()
        .rects(app.pane_area())
        .into_iter()
        .find(|(_, rect)| {
            let (px, py, w, _) = pane_pixels(layout, *rect);
            let bar = f32::from(crate::ui::pane_layout::PANE_TITLE_ROWS) * layout.cell_height;
            rect.rows > crate::ui::pane_layout::PANE_TITLE_ROWS
                && Rect::new(px, py, w, bar).contains(x, y)
        })
        .map(|(id, _)| id)
}

/// Retângulo do painel em pixels da janela: (x, y, largura, altura).
fn pane_pixels(layout: &GridLayout, rect: CellRect) -> (f32, f32, f32, f32) {
    let (cw, ch) = (layout.cell_width, layout.cell_height);
    (
        layout.origin_x + rect.col as f32 * cw,
        layout.origin_y + rect.row as f32 * ch,
        rect.cols as f32 * cw,
        rect.rows as f32 * ch,
    )
}

/// Botão pequeno com um glifo no meio; destacado com o mouse em cima.
fn draw_small_button(
    batch: &mut QuadBatch,
    gpu: &mut GpuState,
    rect: Rect,
    glyph: &str,
    hot: bool,
    colors: &RenderColors,
) {
    let bg = if hot {
        blend_color(&colors.background, &colors.foreground, 0.28)
    } else {
        blend_color(&colors.background, &colors.foreground, 0.16)
    };
    batch.add_color(rect.x, rect.y, rect.width, rect.height, bg, 1.0);
    let size = 13.0;
    gpu.add_text_overlay(
        rect.x + (rect.width - size * 0.6) / 2.0,
        rect.y + (rect.height - size * 1.25) / 2.0,
        glyph,
        colors.foreground,
        size,
    );
}

/// Monta um frame: barra de tabs, grid e painéis abertos.
///
/// Devolve os quads de baixo (fundos) e de cima (decorações, painéis); os
/// textos vão direto para `gpu`. Usado pela janela e pelo `--snapshot`.
pub fn compose_frame(
    app: &AppState,
    gpu: &mut GpuState,
    width: f32,
    height: f32,
    is_focused: bool,
) -> (QuadBatch, QuadBatch) {
    let (under, over, _) = compose_frame_with_hits(app, gpu, width, height, is_focused);
    (under, over)
}

/// Como [`compose_frame`], devolvendo também onde cada painel modal foi
/// desenhado: o mouse usa isso para saber o que está sob o cursor.
pub fn compose_frame_with_hits(
    app: &AppState,
    gpu: &mut GpuState,
    width: f32,
    height: f32,
    is_focused: bool,
) -> (QuadBatch, QuadBatch, OverlayHits) {
    let mut hits = OverlayHits::new();
    let mut under = QuadBatch::new();
    let mut over = QuadBatch::new();
    gpu.clear_text_overlays();
    let colors = gpu.text_renderer.get_colors().clone();
    let (cell_width, cell_height) = gpu.cell_size();
    let layout =
        GridLayout::below_tab_bar(cell_width, cell_height).with_left_inset(app.sidebar_width());
    gpu.set_grid_origin(layout.origin_x, layout.origin_y);

    // Textos da barra de abas, da sidebar e das barras de título dos painéis:
    // ficam numa camada acima dos painéis. No fim, recortamos a parte coberta.
    let chrome_text_start = gpu.text_overlay_count();
    render_tab_bar(app, width, &colors, gpu, &mut under);
    if app.is_sidebar_visible() {
        render_sidebar(app, height, &colors, gpu, &mut under);
    }

    gpu.begin_grids();
    if let Some(tab) = app.active_tab() {
        render_panes(
            tab, app, &layout, &colors, is_focused, gpu, &mut under, &mut over,
        );
        gpu.set_images(pane_images(tab, app, &layout));
    } else {
        gpu.set_images(Vec::new());
    }
    gpu.end_grids();
    let chrome_text_end = gpu.text_overlay_count();

    if app.is_search_visible() {
        render_search_bar(app, width, height, &colors, gpu, &mut over);
    }
    if let Some(ui) = app.quick_connect_ui().filter(|ui| ui.is_visible()) {
        let (panel, list) = render_quick_connect(ui, width, height, &colors, gpu, &mut over);
        hits.push(OverlayKind::QuickConnect, panel, list);
    }
    if app.is_command_palette_visible() {
        let (panel, list) = render_command_palette(
            app.command_palette(),
            width,
            height,
            &colors,
            gpu,
            &mut over,
        );
        hits.push(OverlayKind::CommandPalette, panel, list);
    }
    if let Some(screen) = app.settings_screen() {
        let (panel, list) = render_settings_screen(screen, width, height, &colors, gpu, &mut over);
        hits.push(OverlayKind::Settings, panel, Some(list));
    }
    if app.is_notifications_panel_visible() {
        let panel = render_notifications_panel(app, width, height, &colors, gpu, &mut over);
        hits.push(OverlayKind::Notifications, panel, None);
    }
    if app.is_inspector_visible() {
        render_inspector(app, width, height, &colors, gpu, &mut over);
    }
    if let Some(view) = app.assistant_view() {
        let panel = render_assistant(view, width, height, &colors, gpu, &mut over);
        hits.push(OverlayKind::Assistant, panel, None);
    }
    if app.has_pending_close_tab() {
        let panel = render_close_confirmation(app, width, height, &colors, gpu, &mut over);
        hits.push(OverlayKind::CloseConfirmation, panel, None);
    }
    if app.is_onboarding_visible() {
        let panel = render_onboarding(
            &app.config.keybindings,
            width,
            height,
            &colors,
            gpu,
            &mut over,
        );
        hits.push(OverlayKind::Onboarding, panel, None);
    }
    if let Some(menu) = app.tab_menu() {
        let (panel, list) = render_tab_menu(menu, width, height, &colors, gpu, &mut over);
        hits.push(OverlayKind::TabMenu, panel, Some(list));
    }
    let panels: Vec<_> = hits
        .iter()
        .map(|hit| {
            let panel = hit.panel;
            glyphon::TextBounds {
                left: panel.x.round() as i32,
                top: panel.y.round() as i32,
                right: (panel.x + panel.width).round() as i32,
                bottom: (panel.y + panel.height).round() as i32,
            }
        })
        .collect();
    gpu.clip_text_overlays_under(chrome_text_start..chrome_text_end, &panels);
    // O aviso ("Copiado") não é modal: fica fora do mapa de cliques.
    if let Some(toast) = app.toast() {
        let lift = if app.is_search_visible() { 40.0 } else { 0.0 };
        render_toast(toast, width, height - lift, &colors, gpu, &mut over);
    }
    (under, over, hits)
}

/// Desenha a barra de tabs e os botões (+ e menu).
fn render_tab_bar(
    app: &AppState,
    window_width: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    under: &mut QuadBatch,
) {
    let tab_bar_height = TAB_BAR_HEIGHT as f32;
    let active_index = app.active_tab_index();

    // Fundo da barra inteira, um pouco mais claro que o terminal.
    under.add_color(
        0.0,
        0.0,
        window_width,
        tab_bar_height,
        blend_color(&colors.background, &colors.foreground, 0.08),
        1.0,
    );

    // Com a sidebar, as tabs ficam na vertical; a barra mostra só o título.
    if app.is_sidebar_visible() {
        if let Some(tab) = app.active_tab() {
            let title = truncate_title(&tab.title(), 60);
            gpu.add_text_overlay(
                app.sidebar_width() + 10.0,
                6.0,
                title,
                colors.foreground,
                13.0,
            );
        }
    }
    if !app.is_sidebar_visible() {
        for (i, tab) in app.tab_manager.tabs().iter().enumerate() {
            let glow = tab_glow(app, tab.id, colors);
            let title = display_title(app, &tab.title());
            render_tab_button(tab, &title, i, i == active_index, glow, colors, gpu, under);
        }
    }

    let button_width = TAB_BAR_BUTTON_WIDTH as f32;
    let button_bg = blend_color(&colors.background, &colors.foreground, 0.16);
    let button_border = blend_color(&colors.background, &colors.foreground, 0.3);

    let new_tab_x = window_width - button_width * 2.0;
    under.add_color(new_tab_x, 0.0, button_width, tab_bar_height, button_bg, 1.0);
    under.add_color(new_tab_x, 0.0, 1.0, tab_bar_height, button_border, 1.0);
    gpu.add_text_overlay(new_tab_x + 10.0, 3.0, "+", colors.foreground, 18.0);

    let menu_x = window_width - button_width;
    under.add_color(menu_x, 0.0, button_width, tab_bar_height, button_bg, 1.0);
    under.add_color(menu_x, 0.0, 1.0, tab_bar_height, button_border, 1.0);
    gpu.add_text_overlay(menu_x + 9.0, 5.0, "\u{2630}", colors.foreground, 15.0);
}

pub use crate::ui::sidebar_rows::SIDEBAR_ENTRY_HEIGHT;

/// Linhas visíveis da sidebar (vazio se ela estiver escondida).
pub fn sidebar_rows(app: &AppState) -> Vec<SidebarRow> {
    if !app.is_sidebar_visible() {
        return Vec::new();
    }
    crate::ui::sidebar_rows::sidebar_rows(&app.tab_manager, TAB_BAR_HEIGHT as f32)
}

fn inside_sidebar(app: &AppState, x: f32, y: f32) -> bool {
    app.is_sidebar_visible() && x >= 0.0 && x < app.sidebar_width() && y >= TAB_BAR_HEIGHT as f32
}

/// Linha da sidebar sob (x, y), se houver.
pub fn sidebar_row_at(app: &AppState, x: f32, y: f32) -> Option<SidebarRow> {
    if !inside_sidebar(app, x, y) {
        return None;
    }
    crate::ui::sidebar_rows::row_at(&sidebar_rows(app), y).copied()
}

/// Índice da tab no cartão sob (x, y), se houver.
pub fn sidebar_entry_at(app: &AppState, x: f32, y: f32) -> Option<usize> {
    sidebar_row_at(app, x, y).map(|row| row.tab)
}

/// Onde soltar uma tab arrastada até (x, y); `None` fora da sidebar.
pub fn sidebar_drop_target(app: &AppState, x: f32, y: f32) -> Option<DropTarget> {
    if !app.is_sidebar_visible() || x < 0.0 || x >= app.sidebar_width() {
        return None;
    }
    let rows = sidebar_rows(app);
    Some(crate::ui::sidebar_rows::drop_target_in(
        &rows,
        app.tab_count(),
        y,
    ))
}

/// O ponto (x, y) cai no campo do título em edição.
pub fn is_on_rename_field(app: &AppState, x: f32, y: f32) -> bool {
    let Some(rename) = app.tab_rename() else {
        return false;
    };
    sidebar_row_at(app, x, y)
        .is_some_and(|row| app.tab_manager.tabs().get(row.tab).map(|t| t.id) == Some(rename.tab))
}

/// Linha de detalhes de um cartão: branch, portas e painéis.
fn sidebar_details(info: &crate::ui::sidebar_info::TabSidebarInfo) -> String {
    let mut parts = Vec::new();
    if let Some(branch) = &info.branch {
        let status = info.git.map(|git| git.suffix()).unwrap_or_default();
        parts.push(format!("\u{e0a0} {branch}{status}"));
    }
    if !info.ports.is_empty() {
        let ports: Vec<String> = info.ports.iter().take(4).map(u16::to_string).collect();
        parts.push(format!(":{}", ports.join(" :")));
    }
    if info.panes > 1 {
        parts.push(crate::ui::i18n::panes_count(info.panes));
    }
    parts.join("  ")
}

/// Contorno de `thickness` pixels.
fn outline(batch: &mut QuadBatch, rect: Rect, thickness: f32, color: glyphon::Color) {
    let Rect {
        x,
        y,
        width,
        height,
    } = rect;
    batch.add_color(x, y, width, thickness, color, 1.0);
    batch.add_color(x, y + height - thickness, width, thickness, color, 1.0);
    batch.add_color(x, y, thickness, height, color, 1.0);
    batch.add_color(x + width - thickness, y, thickness, height, color, 1.0);
}

/// Campo de edição do título no próprio cartão (renomear tab).
fn render_rename_field(
    rename: &crate::app::state::TabRename,
    x: f32,
    y: f32,
    sidebar_width: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    under: &mut QuadBatch,
) {
    const CHAR_WIDTH: f32 = 13.0 * 0.6;
    let field = Rect::new(x - 5.0, y + 4.0, sidebar_width - x - 4.0, 22.0);
    under.add_color(
        field.x,
        field.y,
        field.width,
        field.height,
        blend_color(&colors.background, &colors.foreground, 0.2),
        1.0,
    );
    outline(under, field, 1.0, colors.ansi[4]);
    let fit = ((field.width - 10.0) / CHAR_WIDTH).max(1.0) as usize;
    let (text, cursor) = rename.edit.visible(fit);
    if rename.edit.is_all_selected() && !text.is_empty() {
        under.add_color(
            x,
            y + 7.0,
            text.chars().count() as f32 * CHAR_WIDTH,
            16.0,
            colors.selection_background,
            1.0,
        );
    }
    gpu.add_text_overlay(x, y + 8.0, text, colors.foreground, 13.0);
    under.add_color(
        x + cursor as f32 * CHAR_WIDTH,
        y + 7.0,
        1.5,
        16.0,
        colors.foreground,
        1.0,
    );
}

/// Arraste de tab na sidebar: destaca o grupo em que ela vai entrar ou marca
/// o espaço onde vai ficar.
fn render_sidebar_drag(
    app: &AppState,
    rows: &[SidebarRow],
    width: f32,
    colors: &RenderColors,
    under: &mut QuadBatch,
) {
    let Some(drag) = app.sidebar_drag().filter(|drag| drag.dragging) else {
        return;
    };
    let Some(dragged) = app.tab_manager.tab_index_of(drag.tab) else {
        return;
    };
    let (x, y) = drag.pos;
    let hint = sidebar_drop_target(app, x, y).and_then(|target| {
        crate::ui::sidebar_rows::drop_hint(&app.tab_manager, rows, dragged, target)
    });
    let accent = colors.ansi[4];
    match hint {
        Some(DropHint::Into(tab)) => {
            if let Some(row) = rows.iter().find(|row| row.tab == tab) {
                let rect = Rect::new(2.0, row.y + 1.0, width - 5.0, SIDEBAR_ENTRY_HEIGHT - 2.0);
                outline(under, rect, 2.0, accent);
            }
        }
        Some(DropHint::Line(n)) => {
            let line_y = TAB_BAR_HEIGHT as f32 + n as f32 * SIDEBAR_ENTRY_HEIGHT;
            under.add_color(6.0, line_y - 1.0, width - 12.0, 3.0, accent, 1.0);
        }
        None => {}
    }
}

/// Altura de cada item do menu de contexto da tab.
const TAB_MENU_ITEM_HEIGHT: f32 = 26.0;

const TAB_MENU_PAD: f32 = 4.0;
const TAB_MENU_TEXT_SIZE: f32 = 13.0;

/// Onde o menu de contexto da tab fica: no ponto do clique, dentro da janela.
fn tab_menu_rect(menu: &crate::app::state::TabMenu, window_width: f32, window_height: f32) -> Rect {
    let visible = menu.entries.len().min(crate::app::state::TAB_MENU_ROWS);
    let longest = menu
        .entries
        .iter()
        .map(|entry| entry.label.chars().count())
        .max()
        .unwrap_or(0);
    let width = (longest as f32 * TAB_MENU_TEXT_SIZE * 0.6 + 28.0)
        .clamp(180.0, 360.0)
        .min(window_width - 8.0);
    let height = visible as f32 * TAB_MENU_ITEM_HEIGHT + 2.0 * TAB_MENU_PAD;
    let x = menu.x.min(window_width - width - 4.0).max(4.0).round();
    let y = menu.y.min(window_height - height - 4.0).max(4.0).round();
    Rect::new(x, y, width, height)
}

/// Os dois retângulos se cruzam.
fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Menu de contexto de uma tab (botão direito na sidebar), aberto no ponto
/// do clique e mantido dentro da janela.
fn render_tab_menu(
    menu: &crate::app::state::TabMenu,
    window_width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) -> (Rect, ListGeometry) {
    const PAD: f32 = TAB_MENU_PAD;
    const TEXT_SIZE: f32 = TAB_MENU_TEXT_SIZE;
    let visible = menu.entries.len().min(crate::app::state::TAB_MENU_ROWS);
    let panel = tab_menu_rect(menu, window_width, window_height);
    let Rect {
        x: panel_x,
        y: panel_y,
        width: panel_width,
        height: panel_height,
    } = panel;

    over.add_color(
        panel_x + 3.0,
        panel_y + 3.0,
        panel_width,
        panel_height,
        blend_color(&colors.background, &glyphon::Color::rgb(0, 0, 0), 0.5),
        0.6,
    );
    over.add_color(
        panel_x,
        panel_y,
        panel_width,
        panel_height,
        blend_color(&colors.background, &colors.foreground, 0.08),
        1.0,
    );
    outline(
        over,
        panel,
        1.0,
        blend_color(&colors.background, &colors.foreground, 0.35),
    );

    let list = ListGeometry {
        x: panel_x,
        y: panel_y + PAD,
        width: panel_width,
        row_height: TAB_MENU_ITEM_HEIGHT,
        first: menu.first,
        visible,
        total: menu.entries.len(),
    };
    for (row, (index, entry)) in menu
        .entries
        .iter()
        .enumerate()
        .skip(menu.first)
        .take(visible)
        .enumerate()
    {
        let y = list.y + row as f32 * TAB_MENU_ITEM_HEIGHT;
        if index == menu.selected {
            over.add_color(
                panel_x + PAD,
                y,
                panel_width - 2.0 * PAD,
                TAB_MENU_ITEM_HEIGHT,
                colors.selection_background,
                1.0,
            );
        }
        let color = match entry.action {
            crate::app::state::TabMenuAction::Close => colors.ansi[1],
            _ => colors.foreground,
        };
        // Cor pronta: quadradinho com a cor antes do nome.
        let mut text_x = panel_x + 12.0;
        if let crate::app::state::TabMenuAction::SetColor(_, Some([r, g, b])) = entry.action {
            let swatch = Rect::new(text_x, y + 7.0, 12.0, 12.0);
            over.add_color(
                swatch.x,
                swatch.y,
                swatch.width,
                swatch.height,
                glyphon::Color::rgb(r, g, b),
                1.0,
            );
            outline(
                over,
                swatch,
                1.0,
                blend_color(&colors.background, &colors.foreground, 0.4),
            );
            text_x += 20.0;
        }
        let max_chars =
            ((panel_width - 24.0 - (text_x - panel_x - 12.0)) / (TEXT_SIZE * 0.6)) as usize;
        gpu.add_text_overlay(
            text_x,
            y + 5.0,
            truncate_title(&entry.label, max_chars),
            color,
            TEXT_SIZE,
        );
    }
    if menu.entries.len() > visible {
        // Há mais itens: setinhas indicam para onde rola.
        let dim = blend_color(&colors.background, &colors.foreground, 0.5);
        if menu.first > 0 {
            gpu.add_text_overlay(
                panel_x + panel_width - 16.0,
                panel_y + 2.0,
                "\u{25b4}",
                dim,
                11.0,
            );
        }
        if menu.first + visible < menu.entries.len() {
            gpu.add_text_overlay(
                panel_x + panel_width - 16.0,
                panel_y + panel_height - 15.0,
                "\u{25be}",
                dim,
                11.0,
            );
        }
    }
    (panel, list)
}

/// Sidebar de abas verticais: um cartão por tab com título, diretório,
/// branch, portas e avisos não lidos.
fn render_sidebar(
    app: &AppState,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    under: &mut QuadBatch,
) {
    let width = app.sidebar_width();
    let top = TAB_BAR_HEIGHT as f32;
    under.add_color(
        0.0,
        top,
        width,
        window_height - top,
        blend_color(&colors.background, &colors.foreground, 0.05),
        1.0,
    );
    under.add_color(
        width - 1.0,
        top,
        1.0,
        window_height - top,
        blend_color(&colors.background, &colors.foreground, 0.2),
        1.0,
    );

    let dim = blend_color(&colors.background, &colors.foreground, 0.6);
    let active = app.active_tab_index();
    let tabs = app.tab_manager.tabs();
    let infos = app.sidebar_info();
    let rows = sidebar_rows(app);
    let footer = footer_rect(width, window_height);
    let text = |gpu: &mut GpuState, x: f32, y: f32, s: String, color: glyphon::Color, size: f32| {
        let span = Rect::new(x, y, s.chars().count() as f32 * size * 0.6, size * 1.4);
        if !rects_overlap(footer, span) {
            gpu.add_text_overlay(x, y, s, color, size);
        }
    };
    for row in &rows {
        let (i, y) = (row.tab, row.y);
        if y > window_height {
            break;
        }
        let Some(info) = infos.get(i) else {
            continue;
        };
        // Grupo recolhido: o cartão de cima mostra os avisos das de dentro.
        let hidden: Vec<usize> = if row.collapsed {
            (0..tabs.len())
                .filter(|&j| app.tab_manager.parent_index(j) == Some(i))
                .collect()
        } else {
            Vec::new()
        };
        let unread = info.unread
            + hidden
                .iter()
                .filter_map(|&j| infos.get(j))
                .map(|child| child.unread)
                .sum::<usize>();
        let glow_tab = std::iter::once(i)
            .chain(hidden.iter().copied())
            .find(|&j| infos.get(j).is_some_and(|child| child.unread > 0));
        let glow = glow_tab
            .and_then(|j| tabs.get(j))
            .and_then(|tab| tab_glow(app, tab.id, colors));
        // Cor escolhida pela pessoa (as de dentro herdam a do grupo).
        let tab_color = crate::ui::tab_colors::effective_tab_color(&app.tab_manager, i)
            .map(|[r, g, b]| glyphon::Color::rgb(r, g, b));
        let accent = tab_color.unwrap_or(colors.ansi[4]);
        if let Some(color) = tab_color {
            // Cartão levemente tingido e faixa na cor.
            under.add_color(
                0.0,
                y,
                width - 1.0,
                SIDEBAR_ENTRY_HEIGHT,
                blend_color(&colors.background, &color, 0.10),
                1.0,
            );
            under.add_color(0.0, y, 3.0, SIDEBAR_ENTRY_HEIGHT, color, 1.0);
        }
        if i == active {
            under.add_color(
                0.0,
                y,
                width - 1.0,
                SIDEBAR_ENTRY_HEIGHT,
                blend_color(&colors.background, &accent, 0.16),
                1.0,
            );
            under.add_color(0.0, y, 3.0, SIDEBAR_ENTRY_HEIGHT, accent, 1.0);
        } else if row.children > 0 && tab_color.is_none() {
            // Cartão de cima de um grupo: fundo um pouco diferente.
            under.add_color(
                0.0,
                y,
                width - 1.0,
                SIDEBAR_ENTRY_HEIGHT,
                blend_color(&colors.background, &colors.foreground, 0.08),
                1.0,
            );
        }
        // Aviso: faixa na cor (animada conforme o estilo escolhido, #26).
        let motion = app.motion();
        let notice_age = glow_tab
            .and_then(|j| tabs.get(j))
            .and_then(|tab| app.notifications().tab_unread_age(tab.id));
        if let (Some(glow), Some(age)) = (glow, notice_age) {
            use crate::ui::tab_animation::TabAnimation;
            match motion.style {
                TabAnimation::Wave => {
                    // A cor desce pela faixa em pedaços adiantados.
                    const SEGMENTS: usize = 6;
                    let step = SIDEBAR_ENTRY_HEIGHT / SEGMENTS as f32;
                    for k in 0..SEGMENTS {
                        let color = crate::ui::glow::notice_color(
                            &glow_accents(colors),
                            age,
                            motion,
                            1.0 - k as f32 / SEGMENTS as f32,
                        );
                        under.add_color(0.0, y + k as f32 * step, 4.0, step + 0.5, color, 1.0);
                    }
                }
                TabAnimation::Dot => {}
                _ => under.add_color(0.0, y, 4.0, SIDEBAR_ENTRY_HEIGHT, glow, 1.0),
            }
        }
        let x0 = row.text_x();
        // Árvore do grupo: ├─ / └─ ligando a de cima a cada uma de dentro.
        let tree_color = blend_color(&colors.background, &accent, 0.75);
        for line in crate::ui::sidebar_rows::tree_lines(row) {
            under.add_color(line.x, line.y, line.width, line.height, tree_color, 1.0);
        }
        if let Some(dot) = crate::ui::sidebar_rows::tree_dot(row) {
            // Ponto de status: cor do aviso, ou a da árvore.
            let dot_color = if info.unread > 0 {
                glow.unwrap_or(colors.ansi[3])
            } else if i == active {
                accent
            } else {
                tree_color
            };
            under.add_color(dot.x, dot.y, dot.width, dot.height, dot_color, 1.0);
        }
        under.add_color(
            x0 - 6.0,
            y + SIDEBAR_ENTRY_HEIGHT - 1.0,
            width - x0 - 2.0,
            1.0,
            blend_color(&colors.background, &colors.foreground, 0.1),
            1.0,
        );
        if row.children > 0 {
            let arrow = if row.collapsed {
                "\u{25b6}"
            } else {
                "\u{25bc}"
            };
            text(
                gpu,
                5.0,
                y + 9.0,
                arrow.to_string(),
                colors.foreground,
                12.0,
            );
        }

        // Mouse em cima do cartão: aparecem × e ⋯ no canto (o título encolhe).
        let hovered_button = match app.hover() {
            Hover::Card { tab, button }
                if tabs.get(i).map(|t| t.id) == Some(tab)
                    && app.sidebar_drag().is_none()
                    && app.tab_rename().map(|r| r.tab) != Some(tab) =>
            {
                Some(button)
            }
            _ => None,
        };
        let buttons_room = if hovered_button.is_some() {
            CARD_BUTTONS_WIDTH
        } else {
            0.0
        };
        let max_chars = ((width - x0 - 14.0) / (12.0 * 0.6)).max(4.0) as usize;
        let title_chars_max = ((width - x0 - 14.0 - buttons_room) / (12.0 * 0.6)).max(4.0) as usize;
        let title_color = if i == active { colors.foreground } else { dim };
        // Alfinete da tab persistente, à direita do título (antes do badge).
        let pin_space = if info.persistent {
            // Com o mouse em cima, × e ⋯ ocupam o canto: o alfinete vai para a esquerda.
            let px = width - if unread > 0 { 52.0 } else { 26.0 } - buttons_room;
            draw_pin(under, px, y + 9.0, colors.ansi[4]);
            3
        } else {
            0
        };
        let max_chars = max_chars.saturating_sub(pin_space);
        let title_chars_max = title_chars_max.saturating_sub(pin_space);
        match app
            .tab_rename()
            .filter(|r| tabs.get(i).map(|t| t.id) == Some(r.tab))
        {
            Some(rename) => render_rename_field(rename, x0, y, width, colors, gpu, under),
            None => {
                let title = truncate_title(
                    &display_title(app, &info.title),
                    title_chars_max.saturating_sub(4),
                );
                let title_chars = title.chars().count();
                if row.children > 0 {
                    // Cartão de cima em negrito (o texto desenhado duas vezes,
                    // meio pixel ao lado) e a contagem num selo.
                    text(
                        gpu,
                        x0 + 0.6,
                        y + 8.0,
                        title.clone(),
                        colors.foreground,
                        13.0,
                    );
                    text(gpu, x0, y + 8.0, title, colors.foreground, 13.0);
                    let count = format!("{}", row.children);
                    let badge_x = x0 + title_chars as f32 * 13.0 * 0.6 + 8.0;
                    let badge_w = mono_text_width(&count, 11.0) + 10.0;
                    under.add_color(
                        badge_x,
                        y + 8.0,
                        badge_w,
                        17.0,
                        blend_color(&colors.background, &colors.ansi[4], 0.45),
                        1.0,
                    );
                    text(gpu, badge_x + 5.0, y + 9.0, count, colors.foreground, 11.0);
                } else {
                    text(gpu, x0, y + 8.0, title, title_color, 13.0);
                }
            }
        }
        // Memória da tab, discreta, à direita da linha do diretório.
        let memory = info.memory_kb.map(crate::ui::sidebar_info::format_memory);
        let memory_chars = memory.as_ref().map_or(0, |m| m.chars().count() + 2);
        if let Some(memory) = &memory {
            let x = width - 14.0 - memory.chars().count() as f32 * 12.0 * 0.6;
            text(gpu, x, y + 29.0, memory.clone(), dim, 12.0);
        }
        if let Some(cwd) = &info.cwd {
            text(
                gpu,
                x0,
                y + 29.0,
                truncate_title(cwd, max_chars.saturating_sub(memory_chars)),
                dim,
                12.0,
            );
        }
        // Grupo recolhido: resumo do que está dentro no lugar dos detalhes.
        let details = if row.collapsed && row.children > 0 {
            crate::ui::i18n::group_summary(row.children, unread)
        } else {
            sidebar_details(info)
        };
        if !details.is_empty() {
            text(
                gpu,
                x0,
                y + 48.0,
                truncate_title(&details, max_chars),
                colors.ansi[6],
                12.0,
            );
        }
        if unread > 0 {
            // Badge de avisos não lidos.
            let badge = format!("{}", unread.min(99));
            let badge_bg = glow.unwrap_or(colors.ansi[3]);
            let badge_x = width - 34.0 - buttons_room;
            under.add_color(badge_x, y + 8.0, 22.0, 18.0, badge_bg, 1.0);
            text(gpu, badge_x + 5.0, y + 9.0, badge, colors.background, 12.0);
            // Estilos com marca ao lado do contador: giratório ou ponto.
            use crate::ui::tab_animation::{spinner_frame, TabAnimation};
            let mark = match motion.style {
                TabAnimation::Spinner
                    if notice_age
                        .is_some_and(|age| crate::ui::glow::is_notice_animating(age, motion)) =>
                {
                    Some(spinner_frame(spinner_clock(), motion.speed))
                }
                TabAnimation::Dot => Some("\u{25cf}"),
                _ => None,
            };
            if let Some(mark) = mark {
                text(
                    gpu,
                    badge_x - 16.0,
                    y + 8.0,
                    mark.to_string(),
                    badge_bg,
                    13.0,
                );
            }
        }
        if let Some(button) = hovered_button {
            for (kind, glyph) in [
                (CardButton::More, "\u{22ef}"),
                (CardButton::Close, "\u{00d7}"),
            ] {
                let rect = card_button_rect(kind, y, width);
                draw_small_button(under, gpu, rect, glyph, button == Some(kind), colors);
            }
        }
    }
    render_sidebar_drag(app, &rows, width, colors, under);
    render_plugin_statuses(
        app,
        width,
        window_height - FOOTER_HEIGHT,
        colors,
        gpu,
        under,
    );
    render_sidebar_footer(app, width, window_height, colors, gpu, under);
}

/// Rodapé da sidebar: um botão por atalho (temas, SSH, navegador, avisos,
/// IA, configurações) e a dica do botão sob o mouse.
fn render_sidebar_footer(
    app: &AppState,
    width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    under: &mut QuadBatch,
) {
    let bar = footer_rect(width, window_height);
    under.add_color(
        bar.x,
        bar.y,
        bar.width,
        bar.height,
        blend_color(&colors.background, &colors.foreground, 0.08),
        1.0,
    );
    under.add_color(
        bar.x,
        bar.y,
        bar.width,
        1.0,
        blend_color(&colors.background, &colors.foreground, 0.2),
        1.0,
    );
    let hovered = match app.hover() {
        Hover::Footer(button) => Some(button),
        _ => None,
    };
    for button in FooterButton::ALL {
        let rect = footer_button_rect(button, width, window_height);
        if hovered == Some(button) {
            under.add_color(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                blend_color(&colors.background, &colors.foreground, 0.22),
                1.0,
            );
        }
        let size = 15.0;
        gpu.add_text_overlay(
            rect.x + (rect.width - size * 0.6) / 2.0 - 1.0,
            rect.y + (rect.height - size * 1.3) / 2.0,
            button.icon(),
            colors.foreground,
            size,
        );
    }
    if let Some(button) = hovered {
        // Dica acima do botão, sem sair da sidebar.
        let label = button.tooltip();
        let text_width = mono_text_width(label, 12.0);
        let rect = footer_button_rect(button, width, window_height);
        let tip_width = text_width + 14.0;
        let tip_x = (rect.x + rect.width / 2.0 - tip_width / 2.0)
            .min(width - tip_width - 4.0)
            .max(4.0);
        let tip_y = bar.y - 26.0;
        under.add_color(tip_x, tip_y, tip_width, 22.0, colors.foreground, 1.0);
        gpu.add_text_overlay(tip_x + 7.0, tip_y + 4.0, label, colors.background, 12.0);
    }
}

/// Alfinete (tab persistente) desenhado com retângulos: não depende de a
/// fonte ter o símbolo. `(x, y)` é o canto de cima da cabeça.
fn draw_pin(batch: &mut QuadBatch, x: f32, y: f32, color: glyphon::Color) {
    // Cabeça, gola e agulha.
    batch.add_color(x + 1.0, y, 8.0, 5.0, color, 1.0);
    batch.add_color(x - 1.0, y + 5.0, 12.0, 2.0, color, 1.0);
    batch.add_color(x + 4.0, y + 7.0, 2.0, 7.0, color, 1.0);
}

/// Status dos plugins no rodapé da sidebar, um por linha.
fn render_plugin_statuses(
    app: &AppState,
    width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    under: &mut QuadBatch,
) {
    const LINE: f32 = 20.0;
    let statuses = app.plugin_statuses();
    if statuses.is_empty() {
        return;
    }
    let max_chars = ((width - 28.0) / (12.0 * 0.6)) as usize;
    let top = window_height - 10.0 - statuses.len() as f32 * LINE;
    under.add_color(
        8.0,
        top - 6.0,
        width - 16.0,
        1.0,
        blend_color(&colors.background, &colors.foreground, 0.2),
        1.0,
    );
    let dim = blend_color(&colors.background, &colors.foreground, 0.6);
    for (i, (name, text)) in statuses.iter().enumerate() {
        let line = format!("{name}: {text}");
        gpu.add_text_overlay(
            14.0,
            top + i as f32 * LINE,
            truncate_title(&line, max_chars),
            dim,
            12.0,
        );
    }
}

/// Pergunta antes de fechar uma tab com programa rodando.
fn render_close_confirmation(
    app: &AppState,
    window_width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) -> Rect {
    let pane = app.is_pending_close_pane();
    let program = if pane {
        app.pending_close_pane_program()
    } else {
        app.pending_close_tab_index()
            .and_then(|index| app.tab_manager.tabs().get(index))
            .and_then(|tab| tab.busy_process())
    }
    .unwrap_or_else(|| "a program".to_string());
    let question = crate::ui::i18n::tr(if pane {
        crate::ui::i18n::Text::ClosePaneQuestion
    } else {
        crate::ui::i18n::Text::CloseTabQuestion
    });
    let width = 460.0f32.min(window_width - 40.0);
    let height = 120.0;
    let x = ((window_width - width) / 2.0).round();
    let y = ((window_height - height) / 2.0).round();
    over.add_color(
        x,
        y,
        width,
        height,
        blend_color(&colors.background, &colors.foreground, 0.06),
        1.0,
    );
    let persistent = app.pending_close_is_persistent();
    let accent = if persistent {
        colors.ansi[4]
    } else {
        colors.ansi[1]
    };
    over.add_color(x, y, width, 3.0, accent, 1.0);
    let border = blend_color(&colors.background, &colors.foreground, 0.35);
    over.add_color(x, y + height - 1.0, width, 1.0, border, 1.0);
    over.add_color(x, y, 1.0, height, border, 1.0);
    over.add_color(x + width - 1.0, y, 1.0, height, border, 1.0);
    if persistent {
        // Tab persistente: encerrar de vez ou só tirar da janela.
        let dim = blend_color(&colors.background, &colors.foreground, 0.75);
        gpu.add_text_overlay(
            x + 20.0,
            y + 20.0,
            crate::ui::i18n::tr(crate::ui::i18n::Text::ClosePersistentQuestion),
            colors.foreground,
            16.0,
        );
        let program = app
            .tab_manager
            .tabs()
            .get(app.pending_close_tab_index().unwrap_or(usize::MAX))
            .and_then(|tab| tab.busy_process());
        gpu.add_text_overlay(
            x + 20.0,
            y + 48.0,
            crate::ui::i18n::persistent_close_detail(program.as_deref()),
            dim,
            13.0,
        );
        gpu.add_text_overlay(
            x + 20.0,
            y + 82.0,
            crate::ui::i18n::tr(crate::ui::i18n::Text::ClosePersistentHint),
            colors.ansi_bright[6],
            13.0,
        );
        return Rect::new(x, y, width, height);
    }
    gpu.add_text_overlay(x + 20.0, y + 20.0, question, colors.foreground, 16.0);
    gpu.add_text_overlay(
        x + 20.0,
        y + 48.0,
        crate::ui::i18n::still_running(&program),
        blend_color(&colors.background, &colors.foreground, 0.75),
        13.0,
    );
    gpu.add_text_overlay(
        x + 20.0,
        y + 82.0,
        crate::ui::i18n::tr(crate::ui::i18n::Text::ConfirmCloseHint),
        colors.ansi_bright[6],
        13.0,
    );
    Rect::new(x, y, width, height)
}

/// Modos ativos do painel, numa linha (para o inspetor).
fn modes_summary(grid: &crate::terminal::grid::Grid) -> String {
    use crate::terminal::modes::{CursorKeysMode, MouseMode};
    let modes = grid.modes();
    let mut parts = Vec::new();
    if modes.alternate_buffer {
        parts.push("alt-screen".to_string());
    }
    if modes.mouse_mode != MouseMode::None {
        let sgr = if modes.sgr_mouse { " sgr" } else { "" };
        parts.push(format!("mouse:{:?}{sgr}", modes.mouse_mode));
    }
    if modes.cursor_keys == CursorKeysMode::Application {
        parts.push("app-cursor".to_string());
    }
    if modes.bracketed_paste {
        parts.push("bracketed-paste".to_string());
    }
    if modes.focus_events {
        parts.push("focus-events".to_string());
    }
    if !modes.auto_wrap {
        parts.push("no-wrap".to_string());
    }
    if let Some(shape) = modes.cursor_shape {
        parts.push(format!("cursor:{}", shape.as_style()));
    }
    let (row, col) = grid.cursor_position();
    parts.push(format!("cursor@{},{}", row + 1, col + 1));
    parts.join("  ")
}

/// Inspetor de protocolo: bytes recentes do painel em foco e modos ativos.
fn render_inspector(
    app: &AppState,
    window_width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) {
    const ROW: f32 = 17.0;
    let Some(tab) = app.active_tab() else {
        return;
    };
    let Some(buffer) = tab.buffer.as_ref() else {
        return;
    };
    let width = (window_width * 0.55).max(360.0).min(window_width - 20.0);
    let height = (window_height * 0.45).max(200.0).min(window_height - 60.0);
    let x = window_width - width - 10.0;
    let y = window_height - height - 10.0;
    over.add_color(
        x,
        y,
        width,
        height,
        blend_color(&colors.background, &colors.foreground, 0.04),
        1.0,
    );
    over.add_color(x, y, width, 3.0, colors.ansi[5], 1.0);
    let border = blend_color(&colors.background, &colors.foreground, 0.35);
    over.add_color(x, y + height - 1.0, width, 1.0, border, 1.0);
    over.add_color(x, y, 1.0, height, border, 1.0);
    over.add_color(x + width - 1.0, y, 1.0, height, border, 1.0);

    gpu.add_text_overlay(
        x + 12.0,
        y + 10.0,
        crate::ui::i18n::inspector_title(tab.focused_pane()),
        colors.foreground,
        13.0,
    );
    let max_chars = ((width - 24.0) / (12.0 * 0.6)) as usize;
    gpu.add_text_overlay(
        x + 12.0,
        y + 30.0,
        truncate_title(&modes_summary(buffer.grid()), max_chars),
        colors.ansi_bright[6],
        12.0,
    );
    let rows = ((height - 60.0) / ROW).max(1.0) as usize;
    let lines = buffer.inspector_lines(rows);
    let dim = blend_color(&colors.background, &colors.foreground, 0.85);
    if lines.is_empty() {
        gpu.add_text_overlay(
            x + 12.0,
            y + 54.0,
            crate::ui::i18n::tr(crate::ui::i18n::Text::WaitingForOutput),
            dim,
            12.0,
        );
    }
    for (i, line) in lines.iter().enumerate() {
        gpu.add_text_overlay(
            x + 12.0,
            y + 54.0 + i as f32 * ROW,
            truncate_title(line, max_chars),
            dim,
            12.0,
        );
    }
}

/// Seletor de conexão SSH: campos (host, usuário, porta) e a lista de
/// conexões recentes e hosts do `~/.ssh/config`, filtrada pelo que se digita.
fn render_quick_connect(
    ui: &crate::ui::quick_connect::QuickConnectUI,
    window_width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) -> (Rect, Option<ListGeometry>) {
    use crate::ssh::QuickConnectField;
    const ROW: f32 = 28.0;
    let width = 620.0f32.min(window_width - 40.0);
    let items: Vec<String> = ui
        .filtered_recent()
        .iter()
        .map(|recent| format!("\u{21ba}  {}", recent.connection_string))
        .chain(ui.filtered_hosts().iter().map(|host| {
            let target = host
                .host_name
                .clone()
                .unwrap_or_else(|| host.pattern.clone());
            let user = host
                .user
                .as_deref()
                .map(|u| format!("{u}@"))
                .unwrap_or_default();
            format!("\u{2192}  {:<18} {user}{target}", host.pattern)
        }))
        .collect();
    let visible = items.len().min(10);
    let height = (150.0 + visible.max(1) as f32 * ROW).min(window_height - 40.0);
    let x = ((window_width - width) / 2.0).round();
    let y = (window_height * 0.12).clamp(40.0, 120.0).round();

    over.add_color(
        x,
        y,
        width,
        height,
        blend_color(&colors.background, &colors.foreground, 0.06),
        1.0,
    );
    over.add_color(x, y, width, 3.0, colors.ansi[2], 1.0);
    let border = blend_color(&colors.background, &colors.foreground, 0.35);
    over.add_color(x, y + height - 1.0, width, 1.0, border, 1.0);
    over.add_color(x, y, 1.0, height, border, 1.0);
    over.add_color(x + width - 1.0, y, 1.0, height, border, 1.0);
    gpu.add_text_overlay(
        x + 16.0,
        y + 14.0,
        crate::ui::i18n::tr(crate::ui::i18n::Text::ConnectViaSsh),
        colors.foreground,
        16.0,
    );

    // Campos: o focado fica destacado.
    let fields = [
        (
            QuickConnectField::Host,
            crate::ui::i18n::tr(crate::ui::i18n::Text::SshFieldHost),
        ),
        (
            QuickConnectField::User,
            crate::ui::i18n::tr(crate::ui::i18n::Text::SshFieldUser),
        ),
        (
            QuickConnectField::Port,
            crate::ui::i18n::tr(crate::ui::i18n::Text::SshFieldPort),
        ),
    ];
    let field_width = (width - 32.0 - 2.0 * 8.0) / 3.0;
    for (i, (field, label)) in fields.iter().enumerate() {
        let fx = x + 16.0 + i as f32 * (field_width + 8.0);
        let focused = ui.focused_field() == *field;
        let bg = if focused {
            blend_color(&colors.background, &colors.foreground, 0.18)
        } else {
            blend_color(&colors.background, &colors.foreground, 0.1)
        };
        over.add_color(fx, y + 44.0, field_width, 34.0, bg, 1.0);
        if focused {
            over.add_color(fx, y + 76.0, field_width, 2.0, colors.ansi[2], 1.0);
        }
        let value = ui.get_field(*field);
        let (text, color) = if value.is_empty() {
            (
                label.to_string(),
                blend_color(&colors.background, &colors.foreground, 0.45),
            )
        } else {
            (value.to_string(), colors.foreground)
        };
        gpu.add_text_overlay(fx + 8.0, y + 53.0, text, color, 14.0);
    }

    let list_y = y + 92.0;
    let selected = ui.selection_index();
    let first = follow_selection(ui.first_visible(), selected, items.len(), visible);
    for (row, (index, item)) in items
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
        .enumerate()
    {
        let iy = list_y + row as f32 * ROW;
        if index == selected {
            over.add_color(
                x + 8.0,
                iy,
                width - 16.0,
                ROW,
                colors.selection_background,
                1.0,
            );
        }
        gpu.add_text_overlay(x + 16.0, iy + 6.0, item.clone(), colors.foreground, 13.0);
    }
    if items.is_empty() {
        let hint = crate::ui::i18n::tr(crate::ui::i18n::Text::SshNoMatch);
        gpu.add_text_overlay(
            x + 16.0,
            list_y + 6.0,
            hint,
            blend_color(&colors.background, &colors.foreground, 0.55),
            13.0,
        );
    }
    let footer = match ui.error_message() {
        Some(error) => (error.to_string(), colors.ansi_bright[1]),
        None => (
            crate::ui::i18n::tr(crate::ui::i18n::Text::SshHint).to_string(),
            blend_color(&colors.background, &colors.foreground, 0.55),
        ),
    };
    gpu.add_text_overlay(x + 16.0, y + height - 28.0, footer.0, footer.1, 12.0);
    (
        Rect::new(x, y, width, height),
        Some(ListGeometry {
            x: x + 8.0,
            y: list_y,
            width: width - 16.0,
            row_height: ROW,
            first,
            visible,
            total: items.len(),
        }),
    )
}

/// Quebra o texto em linhas de até `width` caracteres (por palavra).
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split(' ') {
            let needed =
                line.chars().count() + word.chars().count() + usize::from(!line.is_empty());
            if needed > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

/// Painel do assistente de IA: campo, espera, resposta ou erro.
fn render_assistant(
    view: &crate::app::state::AssistantView,
    window_width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) -> Rect {
    use crate::app::state::AssistantView;
    const ROW: f32 = 19.0;
    let width = 680.0f32.min(window_width - 40.0);
    let max_chars = ((width - 40.0) / (13.0 * 0.6)) as usize;
    let dim = blend_color(&colors.background, &colors.foreground, 0.55);
    let (title, body, footer): (String, Vec<String>, &str) = match view {
        AssistantView::Prompt(text) => (
            crate::ui::i18n::tr(crate::ui::i18n::Text::AiSuggestTitle).to_string(),
            vec![format!("{text}\u{2588}")],
            crate::ui::i18n::tr(crate::ui::i18n::Text::AiAskHint),
        ),
        AssistantView::Waiting(title) => (
            format!(
                "{} \u{00b7} {title}",
                crate::ui::i18n::tr(crate::ui::i18n::Text::AiTitle)
            ),
            vec![crate::ui::i18n::tr(crate::ui::i18n::Text::Thinking).to_string()],
            crate::ui::i18n::tr(crate::ui::i18n::Text::EscCancel),
        ),
        AssistantView::Answer {
            title,
            text,
            command,
        } => (
            format!(
                "{} \u{00b7} {title}",
                crate::ui::i18n::tr(crate::ui::i18n::Text::AiTitle)
            ),
            wrap_text(text, max_chars),
            if command.is_some() {
                crate::ui::i18n::tr(crate::ui::i18n::Text::AiInsertHint)
            } else {
                crate::ui::i18n::tr(crate::ui::i18n::Text::EnterEscClose)
            },
        ),
        AssistantView::Error(text) => (
            crate::ui::i18n::tr(crate::ui::i18n::Text::AiTitle).to_string(),
            wrap_text(text, max_chars),
            crate::ui::i18n::tr(crate::ui::i18n::Text::EscClose),
        ),
    };
    let rows = body.len().clamp(1, 18);
    let height = 90.0 + rows as f32 * ROW;
    let x = ((window_width - width) / 2.0).round();
    let y = (window_height - height - 40.0).max(40.0).round();
    over.add_color(
        x,
        y,
        width,
        height,
        blend_color(&colors.background, &colors.foreground, 0.06),
        1.0,
    );
    over.add_color(x, y, width, 3.0, colors.ansi_bright[5], 1.0);
    let border = blend_color(&colors.background, &colors.foreground, 0.35);
    over.add_color(x, y + height - 1.0, width, 1.0, border, 1.0);
    over.add_color(x, y, 1.0, height, border, 1.0);
    over.add_color(x + width - 1.0, y, 1.0, height, border, 1.0);
    gpu.add_text_overlay(x + 20.0, y + 14.0, title, colors.foreground, 15.0);
    let body_color = match view {
        AssistantView::Error(_) => colors.ansi_bright[1],
        AssistantView::Waiting(_) => dim,
        _ => colors.foreground,
    };
    for (i, line) in body.iter().take(rows).enumerate() {
        gpu.add_text_overlay(
            x + 20.0,
            y + 44.0 + i as f32 * ROW,
            line.clone(),
            body_color,
            13.0,
        );
    }
    gpu.add_text_overlay(x + 20.0, y + height - 26.0, footer, dim, 12.0);
    Rect::new(x, y, width, height)
}

/// Painel com o histórico de avisos (mais recente primeiro).
fn render_notifications_panel(
    app: &AppState,
    window_width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) -> Rect {
    const ROW: f32 = 22.0;
    let entries: Vec<_> = app.notifications().history().take(14).collect();
    let panel_width = 560.0f32.min(window_width - 40.0);
    let rows = entries.len().max(1) as f32;
    let panel_height = (70.0 + rows * ROW).min(window_height - 60.0);
    let panel_x = (window_width - panel_width - 16.0).round();
    let panel_y = TAB_BAR_HEIGHT as f32 + 8.0;

    over.add_color(
        panel_x,
        panel_y,
        panel_width,
        panel_height,
        blend_color(&colors.background, &colors.foreground, 0.06),
        1.0,
    );
    over.add_color(
        panel_x,
        panel_y,
        panel_width,
        3.0,
        colors.ansi_bright[3],
        1.0,
    );
    let border = blend_color(&colors.background, &colors.foreground, 0.35);
    over.add_color(
        panel_x,
        panel_y + panel_height - 1.0,
        panel_width,
        1.0,
        border,
        1.0,
    );
    over.add_color(panel_x, panel_y, 1.0, panel_height, border, 1.0);
    over.add_color(
        panel_x + panel_width - 1.0,
        panel_y,
        1.0,
        panel_height,
        border,
        1.0,
    );

    let total = app.notifications().total_unread();
    gpu.add_text_overlay(
        panel_x + 16.0,
        panel_y + 14.0,
        crate::ui::i18n::notifications_title(total),
        colors.foreground,
        15.0,
    );
    let dim = blend_color(&colors.background, &colors.foreground, 0.6);
    if entries.is_empty() {
        gpu.add_text_overlay(
            panel_x + 16.0,
            panel_y + 46.0,
            crate::ui::i18n::tr(crate::ui::i18n::Text::NoNotifications),
            dim,
            13.0,
        );
        return Rect::new(panel_x, panel_y, panel_width, panel_height);
    }
    let max_chars = ((panel_width - 110.0) / (13.0 * 0.6)) as usize;
    for (i, entry) in entries.iter().enumerate() {
        let y = panel_y + 46.0 + i as f32 * ROW;
        if y + ROW > panel_y + panel_height {
            break;
        }
        let when = entry
            .at
            .elapsed()
            .map(|age| format_age(age.as_secs()))
            .unwrap_or_default();
        gpu.add_text_overlay(panel_x + 16.0, y, when, dim, 12.0);
        let color = if app.notifications().has_unread(entry.pane) {
            colors.ansi_bright[3]
        } else {
            colors.foreground
        };
        gpu.add_text_overlay(
            panel_x + 90.0,
            y,
            truncate_title(&entry.summary(), max_chars),
            color,
            13.0,
        );
    }
    Rect::new(panel_x, panel_y, panel_width, panel_height)
}

/// Idade curta: "agora", "12s", "5m", "3h", "2d".
fn format_age(secs: u64) -> String {
    match secs {
        0..=4 => crate::ui::i18n::tr(crate::ui::i18n::Text::Now).to_string(),
        5..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        3600..=86399 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

/// Uma tab na barra horizontal.
#[allow(clippy::too_many_arguments)]
fn render_tab_button(
    tab: &crate::ui::tabs::Tab,
    title: &str,
    index: usize,
    is_active: bool,
    glow: Option<glyphon::Color>,
    colors: &RenderColors,
    gpu: &mut GpuState,
    under: &mut QuadBatch,
) {
    let tab_bar_height = TAB_BAR_HEIGHT as f32;
    let x = index as f32 * TAB_WIDTH as f32;
    let bg = if is_active {
        colors.background
    } else {
        blend_color(&colors.background, &colors.foreground, 0.14)
    };
    under.add_color(x, 0.0, TAB_WIDTH as f32, tab_bar_height, bg, 1.0);
    if let Some(glow) = glow {
        // Tab com aviso não lido: faixa na cor (animada) do aviso.
        under.add_color(x, 0.0, TAB_WIDTH as f32, 3.0, glow, 1.0);
    } else if is_active {
        // Faixa de destaque na tab ativa.
        under.add_color(x, 0.0, TAB_WIDTH as f32, 2.0, colors.ansi[4], 1.0);
    }
    under.add_color(
        x + TAB_WIDTH as f32 - 1.0,
        0.0,
        1.0,
        tab_bar_height,
        blend_color(&colors.background, &colors.foreground, 0.25),
        1.0,
    );
    // Indicador para tabs com programa rodando em segundo plano.
    if !is_active && tab.busy_process().is_some() {
        under.add_color(
            x,
            tab_bar_height - 3.0,
            TAB_WIDTH as f32,
            3.0,
            colors.ansi[3],
            1.0,
        );
    }
    let text_color = if is_active {
        colors.foreground
    } else {
        blend_color(&colors.background, &colors.foreground, 0.7)
    };
    gpu.add_text_overlay(x + 10.0, 6.0, truncate_title(title, 16), text_color, 13.0);
}

/// Corta o título em `max_chars` caracteres (sem quebrar UTF-8).
fn truncate_title(title: &str, max_chars: usize) -> String {
    if title.chars().count() <= max_chars {
        return title.to_string();
    }
    let mut cut: String = title.chars().take(max_chars.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}

/// Desenha os painéis da tab: cada grid na sua área, separadores entre eles
/// e os painéis fora de foco esmaecidos.
#[allow(clippy::too_many_arguments)]
fn render_panes(
    tab: &crate::ui::tabs::Tab,
    app: &AppState,
    layout: &GridLayout,
    colors: &RenderColors,
    window_focused: bool,
    gpu: &mut GpuState,
    under: &mut QuadBatch,
    over: &mut QuadBatch,
) {
    let area = app.pane_area();
    let rects = tab.layout().rects(area);
    let (cw, ch) = (layout.cell_width, layout.cell_height);
    let multiple = rects.len() > 1;
    if let Some([r, g, b]) = tab.background {
        // Fundo da tab: só a cor de baixo dos painéis muda (as células com
        // cor própria continuam por cima).
        let w = area.cols as f32 * cw;
        let h = area.rows as f32 * ch;
        under.add_color(
            layout.origin_x,
            layout.origin_y,
            w,
            h,
            glyphon::Color::rgb(r, g, b),
            1.0,
        );
    }
    let hover = app.hover();
    for (id, outer) in rects {
        let Some(buffer) = tab.pane_buffer(id) else {
            continue;
        };
        if outer.cols == 0 || outer.rows == 0 {
            continue;
        }
        let focused = id == tab.focused_pane();
        // Tab dividida: a primeira linha do painel é a barra de título.
        let rect = if multiple { outer.content() } else { outer };
        if multiple && rect != outer {
            let (bx, by, bw, _) = pane_pixels(layout, outer);
            let hot_close = matches!(hover, Hover::Pane { pane, close: true } if pane == id);
            draw_pane_title_bar(
                gpu,
                under,
                over,
                colors,
                Rect::new(bx, by, bw, ch),
                &app.pane_label(tab, id),
                focused,
                hot_close,
            );
        }
        let pane_layout = GridLayout {
            origin_x: layout.origin_x + rect.col as f32 * cw,
            origin_y: layout.origin_y + rect.row as f32 * ch,
            ..*layout
        };
        render_grid(
            id,
            buffer.grid(),
            &pane_layout,
            colors,
            &app.config.terminal.cursor_style,
            window_focused && focused,
            gpu,
            under,
            over,
        );
        let (w, h) = (rect.cols as f32 * cw, rect.rows as f32 * ch);
        if app.notifications().has_unread(id) {
            let age = app
                .notifications()
                .unread_age(id)
                .unwrap_or(crate::ui::glow::GLOW_DURATION);
            let motion = app.motion();
            let ring = crate::ui::glow::notice_color(&glow_accents(colors), age, motion, 0.0);
            let thickness = crate::ui::glow::notice_ring_width(age, motion);
            draw_ring(over, &pane_layout, w, h, ring, thickness);
        }
        if multiple && !focused {
            over.add_color(
                pane_layout.origin_x,
                pane_layout.origin_y,
                w,
                h,
                colors.background,
                0.35,
            );
        }
    }
    draw_dividers(tab, area, layout, colors, under);
}

/// Barra de título de um painel da tab dividida (#32): programa e título à
/// esquerda, × à direita. Em foco, fundo mais forte e texto normal; fora de
/// foco, texto apagado.
#[allow(clippy::too_many_arguments)]
fn draw_pane_title_bar(
    gpu: &mut GpuState,
    under: &mut QuadBatch,
    over: &mut QuadBatch,
    colors: &RenderColors,
    bar: Rect,
    label: &str,
    focused: bool,
    hot_close: bool,
) {
    let strength = if focused { 0.16 } else { 0.07 };
    under.add_color(
        bar.x,
        bar.y,
        bar.width,
        bar.height,
        blend_color(&colors.background, &colors.foreground, strength),
        1.0,
    );
    if focused {
        // Traço de destaque embaixo da barra do painel em foco.
        under.add_color(
            bar.x,
            bar.y + bar.height - 2.0,
            bar.width,
            2.0,
            colors.ansi[4],
            1.0,
        );
    }
    let close = pane_close_rect(bar.x, bar.y, bar.width, bar.height);
    // Mesmo tamanho dos textos da sidebar: reaproveita os glifos já
    // rasterizados no atlas (um tamanho novo custava ~300 KB ao dividir).
    let size = 13.0_f32.min(bar.height * 0.8);
    let room = ((close.x - bar.x - 12.0) / (size * 0.6)).max(0.0) as usize;
    if room >= 3 {
        let text = if focused {
            colors.foreground
        } else {
            blend_color(&colors.foreground, &colors.background, 0.45)
        };
        gpu.add_text_overlay(
            bar.x + 8.0,
            bar.y + (bar.height - size * 1.25) / 2.0,
            truncate_title(label, room),
            text,
            size,
        );
    }
    draw_small_button(over, gpu, close, "\u{00d7}", hot_close, colors);
}

/// Imagens inline visíveis em cada painel, em pixels da janela, recortadas
/// pela área do painel.
fn pane_images(
    tab: &crate::ui::tabs::Tab,
    app: &AppState,
    layout: &GridLayout,
) -> Vec<crate::renderer::image_quad::ImageDraw> {
    let (cw, ch) = (layout.cell_width, layout.cell_height);
    let mut draws = Vec::new();
    for (id, rect) in tab.layout().content_rects(app.pane_area()) {
        let Some(buffer) = tab.pane_buffer(id) else {
            continue;
        };
        let (px, py) = (
            layout.origin_x + rect.col as f32 * cw,
            layout.origin_y + rect.row as f32 * ch,
        );
        let clip = [px, py, rect.cols as f32 * cw, rect.rows as f32 * ch];
        for image in buffer.grid().visible_images() {
            draws.push(crate::renderer::image_quad::ImageDraw {
                id: image.id,
                pixels: image.pixels,
                x: px + image.col as f32 * cw,
                y: py + image.row as f32 * ch,
                width: image.cols as f32 * cw,
                height: image.rows as f32 * ch,
                clip,
            });
        }
    }
    draws
}

/// Anel ao redor de um painel cujo programa pediu atenção.
fn draw_ring(
    over: &mut QuadBatch,
    pane: &GridLayout,
    w: f32,
    h: f32,
    ring: glyphon::Color,
    thickness: f32,
) {
    let t = thickness;
    let (x, y) = (pane.origin_x - t, pane.origin_y - t);
    let (w, h) = (w + 2.0 * t, h + 2.0 * t);
    over.add_color(x, y, w, t, ring, 1.0);
    over.add_color(x, y + h - t, w, t, ring, 1.0);
    over.add_color(x, y, t, h, ring, 1.0);
    over.add_color(x + w - t, y, t, h, ring, 1.0);
}

/// Cores de destaque do tema por onde o aviso passeia: amarelo, magenta,
/// azul e ciano (claros). A primeira é a cor parada.
fn glow_accents(colors: &RenderColors) -> [glyphon::Color; 4] {
    [
        colors.ansi_bright[3],
        colors.ansi_bright[5],
        colors.ansi_bright[4],
        colors.ansi_bright[6],
    ]
}

/// Cor do aviso de uma tab (anel, faixa e contador), se ela tem aviso não lido.
fn tab_glow(app: &AppState, tab_id: u32, colors: &RenderColors) -> Option<glyphon::Color> {
    let age = app.notifications().tab_unread_age(tab_id)?;
    Some(crate::ui::glow::notice_color(
        &glow_accents(colors),
        age,
        app.motion(),
        0.0,
    ))
}

/// Instante de referência do indicador giratório (o mesmo para todas as tabs).
fn spinner_clock() -> std::time::Duration {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    EPOCH.get_or_init(std::time::Instant::now).elapsed()
}

/// Título como aparece: com a opção ligada, o indicador do programa no
/// começo (◐, ✳, ⠋…) vira o giratório do terminal.
fn display_title(app: &AppState, title: &str) -> String {
    if app.config.window.replace_program_spinner {
        if let Some(rest) = crate::ui::tab_animation::strip_program_spinner(title) {
            let motion = app.motion();
            let frame = if motion.style.moves() {
                crate::ui::tab_animation::spinner_frame(spinner_clock(), motion.speed)
            } else {
                crate::ui::tab_animation::SPINNER_FRAMES[0]
            };
            return format!("{frame} {rest}");
        }
    }
    title.to_string()
}

/// Linhas finas no meio das células de separador.
fn draw_dividers(
    tab: &crate::ui::tabs::Tab,
    area: crate::ui::pane_layout::CellRect,
    layout: &GridLayout,
    colors: &RenderColors,
    under: &mut QuadBatch,
) {
    let (cw, ch) = (layout.cell_width, layout.cell_height);
    let line = blend_color(&colors.background, &colors.foreground, 0.3);
    for divider in tab.layout().dividers(area) {
        match divider.direction {
            SplitDirection::Horizontal => under.add_color(
                layout.origin_x + divider.position as f32 * cw + (cw / 2.0).floor(),
                layout.origin_y + divider.start as f32 * ch,
                1.0,
                divider.len as f32 * ch,
                line,
                1.0,
            ),
            SplitDirection::Vertical => under.add_color(
                layout.origin_x + divider.start as f32 * cw,
                layout.origin_y + divider.position as f32 * ch + (ch / 2.0).floor(),
                divider.len as f32 * cw,
                1.0,
                line,
                1.0,
            ),
        }
    }
}

/// Desenha o grid: fundos e cursor por baixo do texto, sublinhados por cima.
#[allow(clippy::too_many_arguments)]
fn render_grid(
    pane: u64,
    grid: &crate::terminal::grid::Grid,
    layout: &GridLayout,
    colors: &RenderColors,
    cursor_style: &str,
    is_focused: bool,
    gpu: &mut GpuState,
    under: &mut QuadBatch,
    over: &mut QuadBatch,
) {
    // O programa pode pedir outra forma de cursor (DECSCUSR, ex.: nvim no insert).
    let cursor_style = grid
        .modes()
        .cursor_shape
        .map(|shape| shape.as_style())
        .unwrap_or(cursor_style);
    let cursor_shown = grid.show_cursor() && grid.scroll_offset() == 0;
    let (cursor_row, cursor_col) = grid.cursor_position();
    let block_cursor = cursor_shown && is_focused && cursor_style == "block";
    let line_thickness = (layout.cell_height / 14.0).round().max(1.0);

    let visible = grid.visible_lines();
    let mut lines = Vec::with_capacity(visible.len());
    for (row, cells) in visible.iter().enumerate() {
        let block_col = (block_cursor && row == cursor_row).then_some(cursor_col);
        let rendered = build_line(cells, colors, block_col);

        push_line_quads(&rendered, row, layout, line_thickness, under, over);
        lines.push(rendered.spans);
    }
    gpu.set_pane_lines(pane, layout.origin_x, layout.origin_y, &lines);

    if !cursor_shown || block_cursor {
        return;
    }
    let (x, y) = layout.cell_origin(cursor_row, cursor_col);
    let shape = match (is_focused, cursor_style) {
        // Janela sem foco: cursor vazado.
        (false, _) => CursorShape::Hollow,
        (true, "underline") => CursorShape::Underline,
        _ => CursorShape::Bar,
    };
    draw_cursor(
        over,
        (x, y),
        layout,
        shape,
        line_thickness.max(2.0),
        colors.cursor_background,
    );
}

/// Fundos (por baixo do texto) e sublinhado/tachado (por cima) de uma linha.
fn push_line_quads(
    rendered: &crate::renderer::cells::RenderedLine,
    row: usize,
    layout: &GridLayout,
    line_thickness: f32,
    under: &mut QuadBatch,
    over: &mut QuadBatch,
) {
    let (cell_width, cell_height) = (layout.cell_width, layout.cell_height);
    for run in &rendered.backgrounds {
        let (x, y) = layout.cell_origin(row, run.col);
        under.add_color(
            x,
            y,
            run.len as f32 * cell_width,
            cell_height,
            run.color,
            1.0,
        );
    }
    for deco in &rendered.decorations {
        let (x, y) = layout.cell_origin(row, deco.col);
        let y = match deco.kind {
            DecorationKind::Underline => y + cell_height - line_thickness - 1.0,
            DecorationKind::Strikethrough => y + (cell_height / 2.0).round(),
        };
        over.add_color(
            x,
            y,
            deco.len as f32 * cell_width,
            line_thickness,
            deco.color,
            1.0,
        );
    }
}

/// Formas de cursor desenhadas por cima do texto (o bloco vai invertido na célula).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CursorShape {
    Hollow,
    Underline,
    Bar,
}

fn draw_cursor(
    over: &mut QuadBatch,
    (x, y): (f32, f32),
    layout: &GridLayout,
    shape: CursorShape,
    stroke: f32,
    color: glyphon::Color,
) {
    let (w, h) = (layout.cell_width, layout.cell_height);
    match shape {
        CursorShape::Hollow => {
            over.add_color(x, y, w, 1.0, color, 1.0);
            over.add_color(x, y + h - 1.0, w, 1.0, color, 1.0);
            over.add_color(x, y, 1.0, h, color, 1.0);
            over.add_color(x + w - 1.0, y, 1.0, h, color, 1.0);
        }
        CursorShape::Underline => over.add_color(x, y + h - stroke, w, stroke, color, 1.0),
        CursorShape::Bar => over.add_color(x, y, stroke, h, color, 1.0),
    }
}

/// Barra de busca no rodapé, com o texto digitado e o número de resultados.
fn render_search_bar(
    app: &AppState,
    window_width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) {
    let bar_height = 30.0;
    let bar_width = 420.0f32.min(window_width - 20.0);
    let x = window_width - bar_width - 10.0;
    let y = window_height - bar_height - 10.0;
    over.add_color(
        x,
        y,
        bar_width,
        bar_height,
        blend_color(&colors.background, &colors.foreground, 0.12),
        1.0,
    );
    over.add_color(x, y, bar_width, 1.0, colors.ansi[4], 1.0);
    let query = app.get_search_query();
    let label = crate::ui::i18n::search_label(&query);
    gpu.add_text_overlay(x + 10.0, y + 7.0, label, colors.foreground, 14.0);
    let status = app.get_search_status();
    if !status.is_empty() {
        gpu.add_text_overlay(
            x + bar_width - 110.0,
            y + 7.0,
            status,
            blend_color(&colors.background, &colors.foreground, 0.7),
            13.0,
        );
    }
}

/// Aviso rápido no canto inferior direito ("Copiado · 12 caracteres").
/// A faixa de cima é verde (ok) ou vermelha (falhou).
fn render_toast(
    toast: &crate::ui::toast::Toast,
    window_width: f32,
    bottom: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) {
    let size = 13.0;
    let height = 28.0;
    let width = (mono_text_width(&toast.text, size) + 24.0).min(window_width - 20.0);
    let x = window_width - width - 10.0;
    let y = bottom - height - 10.0;
    over.add_color(
        x,
        y,
        width,
        height,
        blend_color(&colors.background, &colors.foreground, 0.12),
        1.0,
    );
    let accent = match toast.kind {
        crate::ui::toast::ToastKind::Info => colors.ansi[2],
        crate::ui::toast::ToastKind::Error => colors.ansi[1],
    };
    over.add_color(x, y, width, 2.0, accent, 1.0);
    gpu.add_text_overlay(
        x + 12.0,
        y + 7.0,
        toast.text.clone(),
        colors.foreground,
        size,
    );
}

/// Largura aproximada de um texto na fonte monoespaçada (avanço ~0,6 em).
fn mono_text_width(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.6
}

/// Desenha a command palette: painel opaco, lista com rolagem e atalhos à direita.
fn render_command_palette(
    palette: &CommandPalette,
    window_width: f32,
    window_height: f32,
    colors: &crate::renderer::text::RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) -> (Rect, Option<ListGeometry>) {
    const ITEM_HEIGHT: f32 = 30.0;
    const INPUT_HEIGHT: f32 = 40.0;
    const PAD: f32 = 8.0;
    const TEXT_SIZE: f32 = 14.0;

    let panel_width = 600.0f32.min(window_width - 40.0).max(200.0);
    let panel_x = ((window_width - panel_width) / 2.0).round();
    let panel_y = (window_height * 0.12).clamp(40.0, 120.0).round();

    let items = palette.items();
    let room = ((window_height - panel_y - 20.0 - INPUT_HEIGHT - 3.0 * PAD) / ITEM_HEIGHT).floor();
    let visible = (room.max(1.0) as usize).min(14).min(items.len());
    let first = follow_selection(
        palette.first_visible(),
        palette.selected_index,
        items.len(),
        visible,
    );
    let list_height = visible.max(1) as f32 * ITEM_HEIGHT;
    let panel_height = INPUT_HEIGHT + list_height + 3.0 * PAD;

    // Painel opaco com borda, como o de boas-vindas.
    let panel_bg = blend_color(&colors.background, &colors.foreground, 0.06);
    let border = blend_color(&colors.background, &colors.foreground, 0.35);
    over.add_color(panel_x, panel_y, panel_width, panel_height, panel_bg, 1.0);
    over.add_color(panel_x, panel_y, panel_width, 1.0, border, 1.0);
    over.add_color(
        panel_x,
        panel_y + panel_height - 1.0,
        panel_width,
        1.0,
        border,
        1.0,
    );
    over.add_color(panel_x, panel_y, 1.0, panel_height, border, 1.0);
    over.add_color(
        panel_x + panel_width - 1.0,
        panel_y,
        1.0,
        panel_height,
        border,
        1.0,
    );

    // Caixa de input.
    let input_y = panel_y + PAD;
    let input_bg = blend_color(&colors.background, &colors.foreground, 0.12);
    over.add_color(
        panel_x + PAD,
        input_y,
        panel_width - 2.0 * PAD,
        INPUT_HEIGHT,
        input_bg,
        1.0,
    );
    let (query_text, query_color) = if palette.query.is_empty() {
        (
            palette.placeholder().to_string(),
            blend_color(&colors.background, &colors.foreground, 0.5),
        )
    } else {
        (palette.query.clone(), colors.foreground)
    };
    gpu.add_text_overlay(
        panel_x + 2.0 * PAD,
        input_y + 11.0,
        query_text,
        query_color,
        16.0,
    );

    let list_y = input_y + INPUT_HEIGHT + PAD;
    if items.is_empty() {
        gpu.add_text_overlay(
            panel_x + 2.0 * PAD,
            list_y + 7.0,
            palette.empty_text(),
            blend_color(&colors.background, &colors.foreground, 0.5),
            TEXT_SIZE,
        );
        return (Rect::new(panel_x, panel_y, panel_width, panel_height), None);
    }

    let shortcut_color = blend_color(&colors.background, &colors.foreground, 0.55);
    for (row, (index, item)) in items
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
        .enumerate()
    {
        let y = list_y + row as f32 * ITEM_HEIGHT;
        if index == palette.selected_index {
            over.add_color(
                panel_x + PAD,
                y,
                panel_width - 2.0 * PAD,
                ITEM_HEIGHT,
                colors.selection_background,
                1.0,
            );
        }
        gpu.add_text_overlay(
            panel_x + 2.0 * PAD,
            y + 7.0,
            item.label,
            colors.foreground,
            TEXT_SIZE,
        );
        if !item.shortcut.is_empty() {
            let x =
                panel_x + panel_width - 2.0 * PAD - mono_text_width(item.shortcut, TEXT_SIZE - 1.0);
            gpu.add_text_overlay(x, y + 8.0, item.shortcut, shortcut_color, TEXT_SIZE - 1.0);
        }
    }

    // Indicadores de que há mais itens acima/abaixo.
    let more = blend_color(&colors.background, &colors.foreground, 0.4);
    if first > 0 {
        over.add_color(
            panel_x + PAD,
            list_y - 2.0,
            panel_width - 2.0 * PAD,
            1.0,
            more,
            1.0,
        );
    }
    if first + visible < items.len() {
        over.add_color(
            panel_x + PAD,
            list_y + list_height + 1.0,
            panel_width - 2.0 * PAD,
            1.0,
            more,
            1.0,
        );
    }
    (
        Rect::new(panel_x, panel_y, panel_width, panel_height),
        Some(ListGeometry {
            x: panel_x + PAD,
            y: list_y,
            width: panel_width - 2.0 * PAD,
            row_height: ITEM_HEIGHT,
            first,
            visible,
            total: items.len(),
        }),
    )
}

/// Onde fica cada parte da tela de configurações (desenho e clique usam
/// as mesmas contas).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsLayout {
    pub panel: Rect,
    /// Uma por seção, na ordem de [`crate::ui::settings::SECTIONS`].
    pub sections: Vec<Rect>,
    /// Caixa de busca (só na seção de atalhos).
    pub search: Option<Rect>,
    /// Linhas da seção aberta.
    pub list: ListGeometry,
    pub save: Rect,
    pub cancel: Rect,
}

const SETTINGS_ROW: f32 = 32.0;
const SETTINGS_SECTIONS_WIDTH: f32 = 170.0;

/// Geometria da tela para a janela `window_width` x `window_height`.
pub fn settings_layout(
    screen: &SettingsScreen,
    window_width: f32,
    window_height: f32,
) -> SettingsLayout {
    use crate::ui::settings::{SettingsSection, SECTIONS};
    let width = 760.0f32.min(window_width - 40.0).max(320.0);
    let height = 520.0f32.min(window_height - 40.0).max(240.0);
    let x = ((window_width - width) / 2.0).round();
    let y = ((window_height - height) / 2.0).round();
    let sections = (0..SECTIONS.len())
        .map(|i| {
            Rect::new(
                x + 12.0,
                y + 56.0 + i as f32 * SETTINGS_ROW,
                SETTINGS_SECTIONS_WIDTH,
                SETTINGS_ROW - 2.0,
            )
        })
        .collect();
    let content_x = x + SETTINGS_SECTIONS_WIDTH + 28.0;
    let content_width = x + width - 16.0 - content_x;
    let search = (screen.section() == SettingsSection::Shortcuts)
        .then(|| Rect::new(content_x, y + 56.0, content_width, 30.0));
    let rows_y = if search.is_some() { y + 94.0 } else { y + 56.0 };
    let rows_bottom = y + height - 56.0;
    let total = screen.rows().len();
    let room = (((rows_bottom - rows_y) / SETTINGS_ROW).floor().max(1.0)) as usize;
    let visible = room.min(total);
    let first = follow_selection(screen.first_visible(), screen.selected(), total, visible);
    let button_y = y + height - 44.0;
    SettingsLayout {
        panel: Rect::new(x, y, width, height),
        sections,
        search,
        list: ListGeometry {
            x: content_x,
            y: rows_y,
            width: content_width,
            row_height: SETTINGS_ROW,
            first,
            visible,
            total,
        },
        save: Rect::new(x + width - 204.0, button_y, 92.0, 30.0),
        cancel: Rect::new(x + width - 104.0, button_y, 92.0, 30.0),
    }
}

/// Clique da tela fora da lista: seção, Salvar ou Cancelar.
pub fn settings_click_at(
    screen: &SettingsScreen,
    window_width: f32,
    window_height: f32,
    x: f32,
    y: f32,
) -> Option<crate::app::SettingsClick> {
    use crate::app::SettingsClick;
    let layout = settings_layout(screen, window_width, window_height);
    if let Some(index) = layout.sections.iter().position(|r| r.contains(x, y)) {
        return Some(SettingsClick::Section(index));
    }
    if layout.save.contains(x, y) {
        return Some(SettingsClick::Save);
    }
    layout
        .cancel
        .contains(x, y)
        .then_some(SettingsClick::Cancel)
}

/// Desenha a tela de configurações: seções à esquerda, opções à direita e
/// os botões Salvar/Cancelar.
fn render_settings_screen(
    screen: &SettingsScreen,
    window_width: f32,
    window_height: f32,
    colors: &crate::renderer::text::RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) -> (Rect, ListGeometry) {
    use crate::ui::i18n::{tr, Text};
    use crate::ui::settings::{RowKind, SettingsSection, SECTIONS};
    let layout = settings_layout(screen, window_width, window_height);
    let Rect {
        x,
        y,
        width,
        height,
    } = layout.panel;
    let dim = blend_color(&colors.background, &colors.foreground, 0.6);
    let border = blend_color(&colors.background, &colors.foreground, 0.35);

    // Painel opaco, como os demais.
    over.add_color(
        x,
        y,
        width,
        height,
        blend_color(&colors.background, &colors.foreground, 0.06),
        1.0,
    );
    over.add_color(x, y, width, 3.0, colors.ansi[4], 1.0);
    outline(over, layout.panel, 1.0, border);
    gpu.add_text_overlay(
        x + 20.0,
        y + 16.0,
        tr(Text::SettingsTitle),
        colors.foreground,
        20.0,
    );

    // Seções.
    for (rect, section) in layout.sections.iter().zip(SECTIONS) {
        let active = *section == screen.section();
        if active {
            over.add_color(
                rect.x,
                rect.y,
                rect.width,
                rect.height,
                colors.selection_background,
                1.0,
            );
            over.add_color(rect.x, rect.y, 3.0, rect.height, colors.ansi[4], 1.0);
        }
        let color = if active { colors.foreground } else { dim };
        gpu.add_text_overlay(rect.x + 12.0, rect.y + 7.0, section.label(), color, 14.0);
    }
    over.add_color(
        x + SETTINGS_SECTIONS_WIDTH + 18.0,
        y + 52.0,
        1.0,
        height - 108.0,
        border,
        1.0,
    );

    // Busca da seção de atalhos.
    if let Some(search) = layout.search {
        over.add_color(
            search.x,
            search.y,
            search.width,
            search.height,
            blend_color(&colors.background, &colors.foreground, 0.12),
            1.0,
        );
        let (text, color) = if screen.query().is_empty() {
            (tr(Text::ShortcutsFilter).to_string(), dim)
        } else {
            (screen.query().to_string(), colors.foreground)
        };
        gpu.add_text_overlay(search.x + 10.0, search.y + 7.0, text, color, 13.0);
    }

    // Linhas da seção.
    let list = layout.list;
    let rows = screen.rows();
    let char_width = 14.0 * 0.6;
    if rows.is_empty() {
        gpu.add_text_overlay(
            list.x + 10.0,
            list.y + 8.0,
            tr(Text::ShortcutsNone),
            dim,
            14.0,
        );
    }
    for (row_index, (index, row)) in rows
        .iter()
        .enumerate()
        .skip(list.first)
        .take(list.visible)
        .enumerate()
    {
        let ry = list.y + row_index as f32 * SETTINGS_ROW;
        if index == screen.selected() {
            over.add_color(
                list.x,
                ry,
                list.width,
                SETTINGS_ROW - 2.0,
                colors.selection_background,
                1.0,
            );
        }
        if row.kind == RowKind::Text {
            // Campo de texto: caixa à direita do rótulo, com cursor na linha
            // selecionada.
            let field_x = list.x + (list.width * 0.42).max(180.0);
            let field_w = list.x + list.width - 10.0 - field_x;
            over.add_color(
                field_x,
                ry + 3.0,
                field_w,
                SETTINGS_ROW - 8.0,
                blend_color(&colors.background, &colors.foreground, 0.12),
                1.0,
            );
            let fit = ((field_w - 12.0) / char_width).max(4.0) as usize;
            let (shown, cursor) = match screen.text_cursor() {
                Some((selected, cursor)) if selected == index => {
                    // Janela do texto que acompanha o cursor.
                    let start = (cursor + 1).saturating_sub(fit);
                    let text: String = row.value.chars().skip(start).take(fit).collect();
                    (text, Some(cursor.saturating_sub(start)))
                }
                _ => (truncate_title(&row.value, fit), None),
            };
            gpu.add_text_overlay(field_x + 6.0, ry + 8.0, shown, colors.foreground, 14.0);
            if let Some(at) = cursor {
                over.add_color(
                    field_x + 6.0 + at as f32 * char_width,
                    ry + 7.0,
                    1.5,
                    17.0,
                    colors.foreground,
                    1.0,
                );
            }
            let label_room = ((field_x - list.x - 20.0) / char_width).max(4.0) as usize;
            gpu.add_text_overlay(
                list.x + 10.0,
                ry + 8.0,
                truncate_title(&row.label, label_room),
                colors.foreground,
                14.0,
            );
            continue;
        }
        let value = match row.kind {
            RowKind::Choice => format!("\u{2039} {} \u{203a}", row.value),
            RowKind::Action if row.value.is_empty() => String::new(),
            RowKind::Action => format!("[ {} ]", row.value),
            _ => row.value.clone(),
        };
        let value_color = match row.kind {
            RowKind::Action => colors.ansi[4],
            RowKind::Shortcut => colors.ansi[6],
            RowKind::Info => dim,
            _ => colors.foreground,
        };
        let max_value = ((list.width * 0.55) / char_width) as usize;
        let value = truncate_title(&value, max_value.max(6));
        let value_width = mono_text_width(&value, 14.0);
        let label_room = ((list.width - value_width - 30.0) / char_width).max(4.0) as usize;
        gpu.add_text_overlay(
            list.x + 10.0,
            ry + 8.0,
            truncate_title(&row.label, label_room),
            colors.foreground,
            14.0,
        );
        gpu.add_text_overlay(
            list.x + list.width - 10.0 - value_width,
            ry + 8.0,
            value,
            value_color,
            14.0,
        );
    }
    // Há mais linhas: setinhas indicam para onde rola.
    if list.first > 0 {
        gpu.add_text_overlay(
            list.x + list.width - 14.0,
            list.y - 14.0,
            "\u{25b4}",
            dim,
            11.0,
        );
    }
    if list.first + list.visible < list.total {
        gpu.add_text_overlay(
            list.x + list.width - 14.0,
            list.y + list.visible as f32 * SETTINGS_ROW,
            "\u{25be}",
            dim,
            11.0,
        );
    }

    if screen.section() == SettingsSection::Appearance {
        render_animation_preview(screen, &layout, colors, gpu, over);
    }

    // Rodapé: dica e botões.
    let hint = if screen.is_capturing() {
        tr(Text::ShortcutsPress)
    } else if screen.section() == SettingsSection::Shortcuts {
        tr(Text::SettingsHintShortcuts)
    } else {
        tr(Text::SettingsHint)
    };
    let hint_room = ((layout.save.x - x - 30.0) / (12.0 * 0.6)) as usize;
    gpu.add_text_overlay(
        x + 20.0,
        y + height - 36.0,
        truncate_title(hint, hint_room),
        dim,
        12.0,
    );
    for (rect, label, accent) in [
        (layout.save, tr(Text::Save), true),
        (layout.cancel, tr(Text::Cancel), false),
    ] {
        let bg = if accent {
            colors.ansi[4]
        } else {
            blend_color(&colors.background, &colors.foreground, 0.16)
        };
        let fg = if accent {
            colors.background
        } else {
            colors.foreground
        };
        over.add_color(rect.x, rect.y, rect.width, rect.height, bg, 1.0);
        let text_x = rect.x + (rect.width - mono_text_width(label, 14.0)) / 2.0;
        gpu.add_text_overlay(text_x, rect.y + 7.0, label, fg, 14.0);
    }
    (layout.panel, list)
}

/// Prévia da animação da tab na Aparência: um cartão de exemplo com aviso,
/// animando por alguns segundos depois de cada troca.
fn render_animation_preview(
    screen: &SettingsScreen,
    layout: &SettingsLayout,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) {
    use crate::ui::tab_animation::{spinner_frame, TabAnimation};
    let list = layout.list;
    let top = list.y + list.visible as f32 * SETTINGS_ROW + 14.0;
    let bottom = layout.save.y - 12.0;
    if bottom - top < 56.0 {
        return;
    }
    let card = Rect::new(
        list.x + 10.0,
        top + 18.0,
        220.0f32.min(list.width - 20.0),
        44.0,
    );
    gpu.add_text_overlay(
        list.x + 10.0,
        top,
        crate::ui::i18n::tr(crate::ui::i18n::Text::Preview),
        blend_color(&colors.background, &colors.foreground, 0.6),
        12.0,
    );
    let motion = screen.preview_motion(
        true,
        crate::ui::tab_animation::system_prefers_reduced_motion(),
    );
    let age = screen.preview_age();
    let accents = glow_accents(colors);
    let color = crate::ui::glow::notice_color(&accents, age, motion, 0.0);
    over.add_color(
        card.x,
        card.y,
        card.width,
        card.height,
        blend_color(&colors.background, &colors.foreground, 0.12),
        1.0,
    );
    match motion.style {
        TabAnimation::Wave => {
            for k in 0..4 {
                let c = crate::ui::glow::notice_color(&accents, age, motion, 1.0 - k as f32 / 4.0);
                let step = card.height / 4.0;
                over.add_color(card.x, card.y + k as f32 * step, 4.0, step + 0.5, c, 1.0);
            }
        }
        TabAnimation::Dot => {}
        _ => over.add_color(card.x, card.y, 4.0, card.height, color, 1.0),
    }
    let ring = crate::ui::glow::notice_ring_width(age, motion);
    if motion.style == TabAnimation::Ring {
        outline(over, card, ring, color);
    }
    gpu.add_text_overlay(
        card.x + 14.0,
        card.y + 13.0,
        "claude",
        colors.foreground,
        13.0,
    );
    let badge_x = card.x + card.width - 32.0;
    over.add_color(badge_x, card.y + 12.0, 22.0, 18.0, color, 1.0);
    gpu.add_text_overlay(badge_x + 7.0, card.y + 13.0, "1", colors.background, 12.0);
    let mark = match motion.style {
        TabAnimation::Spinner if crate::ui::glow::is_notice_animating(age, motion) => {
            Some(spinner_frame(age, motion.speed))
        }
        TabAnimation::Spinner => Some(crate::ui::tab_animation::SPINNER_FRAMES[0]),
        TabAnimation::Dot => Some("\u{25cf}"),
        _ => None,
    };
    if let Some(mark) = mark {
        gpu.add_text_overlay(badge_x - 16.0, card.y + 12.0, mark, color, 13.0);
    }
}

/// Renderiza o overlay de onboarding com os atalhos configurados.
fn render_onboarding(
    keybindings: &crate::config::KeyBindings,
    window_width: f32,
    window_height: f32,
    colors: &RenderColors,
    gpu: &mut GpuState,
    over: &mut QuadBatch,
) -> Rect {
    let entries = crate::ui::onboarding::Onboarding::shortcuts(keybindings);
    let row_height = 28.0;
    let panel_width = 480.0f32.min(window_width - 40.0);
    let panel_height = (120.0 + entries.len() as f32 * row_height).min(window_height - 40.0);
    let panel_x = ((window_width - panel_width) / 2.0).round();
    let panel_y = ((window_height - panel_height) / 2.0).round();

    // Painel opaco: o terminal não pode aparecer por trás do texto.
    over.add_color(
        panel_x,
        panel_y,
        panel_width,
        panel_height,
        blend_color(&colors.background, &colors.foreground, 0.06),
        1.0,
    );
    let border = blend_color(&colors.background, &colors.foreground, 0.35);
    over.add_color(panel_x, panel_y, panel_width, 1.0, border, 1.0);
    over.add_color(
        panel_x,
        panel_y + panel_height - 1.0,
        panel_width,
        1.0,
        border,
        1.0,
    );
    over.add_color(panel_x, panel_y, 1.0, panel_height, border, 1.0);
    over.add_color(
        panel_x + panel_width - 1.0,
        panel_y,
        1.0,
        panel_height,
        border,
        1.0,
    );
    over.add_color(panel_x, panel_y, panel_width, 3.0, colors.ansi[4], 1.0);

    gpu.add_text_overlay(
        panel_x + 24.0,
        panel_y + 22.0,
        crate::ui::i18n::tr(crate::ui::i18n::Text::Welcome),
        colors.foreground,
        20.0,
    );

    let label_color = blend_color(&colors.background, &colors.foreground, 0.85);
    for (i, (keys, label)) in entries.iter().enumerate() {
        let y = panel_y + 66.0 + i as f32 * row_height;
        gpu.add_text_overlay(panel_x + 24.0, y, keys.clone(), colors.ansi_bright[6], 15.0);
        gpu.add_text_overlay(panel_x + 200.0, y, *label, label_color, 15.0);
    }

    gpu.add_text_overlay(
        panel_x + 24.0,
        panel_y + panel_height - 34.0,
        crate::ui::i18n::tr(crate::ui::i18n::Text::PressAnyKey),
        blend_color(&colors.background, &colors.foreground, 0.6),
        13.0,
    );
    Rect::new(panel_x, panel_y, panel_width, panel_height)
}

/// Gerenciador de múltiplas janelas
pub struct WindowManager {
    /// Mapa de janelas por ID
    windows: HashMap<WindowId, Window>,
    /// Mapa de Winit WindowId para nosso WindowId
    winit_to_id: HashMap<WinitWindowId, WindowId>,
    /// ID da janela ativa
    active_window: Option<WindowId>,
    /// Contador de janelas abertas
    window_count: usize,
    /// Canal de comunicação entre janelas
    event_proxy: Option<EventLoopProxy<WindowEventMsg>>,
    /// Configuração global
    config: Config,
}

impl WindowManager {
    /// Cria um novo gerenciador de janelas
    pub fn new(config: Config) -> Self {
        Self {
            windows: HashMap::new(),
            winit_to_id: HashMap::new(),
            active_window: None,
            window_count: 0,
            event_proxy: None,
            config,
        }
    }

    /// Define o proxy de eventos para comunicação
    pub fn set_event_proxy(&mut self, proxy: EventLoopProxy<WindowEventMsg>) {
        self.event_proxy = Some(proxy);
    }

    /// Retorna a configuração global usada por novas janelas.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Cria a primeira janela (janela principal)
    ///
    /// # Nota
    /// No winit 0.30+, a criação de janelas é feita via `ActiveEventLoop`.
    /// Este método aceita uma closure que cria a janela.
    pub fn create_primary_window<F>(
        &mut self,
        create_info: WindowCreateInfo,
        window_factory: F,
    ) -> Result<WindowId>
    where
        F: FnOnce() -> Result<Arc<WinitWindow>>,
    {
        let window_id = WindowId::new();

        let winit_window =
            window_factory().map_err(|e| anyhow::anyhow!("Failed to create window: {}", e))?;

        let mut window = Window::new(window_id, winit_window.clone(), create_info.config)?;

        // Inicializa AppState
        let cols = 80;
        let rows = 24;
        window.initialize_app_state(cols, rows)?;

        // Atualiza título
        window.update_title();

        // Registra mapeamento
        self.winit_to_id.insert(winit_window.id(), window_id);
        self.windows.insert(window_id, window);
        self.active_window = Some(window_id);
        self.window_count += 1;

        info!(window_id = ?window_id, "Primary window created");
        Ok(window_id)
    }

    /// Cria uma nova janela adicional
    pub fn create_window<F>(
        &mut self,
        create_info: WindowCreateInfo,
        window_factory: F,
    ) -> Result<WindowId>
    where
        F: FnOnce() -> Result<Arc<WinitWindow>>,
    {
        let window_id = WindowId::new();

        let winit_window =
            window_factory().map_err(|e| anyhow::anyhow!("Failed to create window: {}", e))?;

        let cwd = create_info.cwd.clone();
        let mut window = Window::new(window_id, winit_window.clone(), create_info.config)?;

        // Inicializa AppState
        window.initialize_app_state_in(80, 24, cwd.as_deref())?;
        window.update_title();

        // Registra mapeamento
        self.winit_to_id.insert(winit_window.id(), window_id);
        self.windows.insert(window_id, window);
        self.window_count += 1;

        info!(window_id = ?window_id, "Additional window created");
        Ok(window_id)
    }

    /// Fecha uma janela específica
    pub fn close_window(&mut self, window_id: WindowId) -> bool {
        if let Some(window) = self.windows.remove(&window_id) {
            self.winit_to_id.remove(&window.winit_window.id());
            self.window_count -= 1;

            // Se era a janela ativa, seleciona outra
            if self.active_window == Some(window_id) {
                self.active_window = self.windows.keys().next().copied();
            }

            info!(window_id = ?window_id, remaining = self.window_count, "Window closed");

            // Retorna true se ainda há janelas abertas
            self.window_count > 0
        } else {
            true
        }
    }

    /// Obtém uma janela pelo ID
    pub fn get_window(&self, window_id: WindowId) -> Option<&Window> {
        self.windows.get(&window_id)
    }

    /// Obtém uma janela mutável pelo ID
    pub fn get_window_mut(&mut self, window_id: WindowId) -> Option<&mut Window> {
        self.windows.get_mut(&window_id)
    }

    /// Obtém uma janela pelo Winit WindowId
    pub fn get_window_by_winit_id(&self, winit_id: WinitWindowId) -> Option<&Window> {
        self.winit_to_id
            .get(&winit_id)
            .and_then(|id| self.windows.get(id))
    }

    /// Obtém uma janela mutável pelo Winit WindowId
    pub fn get_window_mut_by_winit_id(&mut self, winit_id: WinitWindowId) -> Option<&mut Window> {
        self.winit_to_id
            .get(&winit_id)
            .copied()
            .and_then(|id| self.windows.get_mut(&id))
    }

    /// Retorna a janela ativa
    pub fn active_window(&self) -> Option<&Window> {
        self.active_window.and_then(|id| self.windows.get(&id))
    }

    /// Retorna a janela ativa (mutável)
    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.active_window.and_then(|id| self.windows.get_mut(&id))
    }

    /// Retorna o ID da janela ativa
    pub fn active_window_id(&self) -> Option<WindowId> {
        self.active_window
    }

    /// Define a janela ativa
    pub fn set_active_window(&mut self, window_id: WindowId) {
        if self.windows.contains_key(&window_id) {
            // Desativa a janela anterior
            if let Some(prev_id) = self.active_window {
                if let Some(window) = self.windows.get_mut(&prev_id) {
                    window.set_state(WindowState::Inactive);
                }
            }

            // Ativa a nova janela
            self.active_window = Some(window_id);
            if let Some(window) = self.windows.get_mut(&window_id) {
                window.set_state(WindowState::Active);
                window.winit_window.focus_window();
            }

            debug!(window_id = ?window_id, "Active window changed");
        }
    }

    /// Retorna o número de janelas abertas
    pub fn window_count(&self) -> usize {
        self.window_count
    }

    /// Verifica se há janelas abertas
    pub fn has_windows(&self) -> bool {
        self.window_count > 0
    }

    /// Retorna iterador sobre todas as janelas
    pub fn windows(&self) -> impl Iterator<Item = (&WindowId, &Window)> {
        self.windows.iter()
    }

    /// Retorna iterador mutável sobre todas as janelas
    pub fn windows_mut(&mut self) -> impl Iterator<Item = (&WindowId, &mut Window)> {
        self.windows.iter_mut()
    }

    /// Processa eventos de janela do winit
    /// Retorna Some(WindowId) se o evento requer fechar a janela
    pub fn handle_window_event(
        &mut self,
        winit_id: WinitWindowId,
        event: &WindowEvent,
    ) -> Option<WindowId> {
        let window_id = self.winit_to_id.get(&winit_id).copied();

        if let Some(id) = window_id {
            if let Some(window) = self.windows.get_mut(&id) {
                window.handle_window_event(event);

                // Atualiza janela ativa em eventos de foco
                if matches!(event, WindowEvent::Focused(true)) {
                    self.set_active_window(id);
                }

                // Fecha janela se solicitado
                if matches!(event, WindowEvent::CloseRequested) {
                    return Some(id);
                }
            }
        }

        window_id
    }

    /// Envia evento para uma janela específica
    pub fn send_event(&self, _window_id: WindowId, event: WindowEventMsg) {
        if let Some(proxy) = &self.event_proxy {
            let _ = proxy.send_event(event);
        }
    }

    /// Broadcast evento para todas as janelas
    pub fn broadcast_event(&self, event: WindowEventMsg) {
        if let Some(proxy) = &self.event_proxy {
            let _ = proxy.send_event(event);
        }
    }

    /// Atualiza o título de todas as janelas
    pub fn update_all_titles(&mut self) {
        for window in self.windows.values() {
            window.update_title();
        }
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hit-test dos botões novos (#23): × e ⋯ do cartão e × do painel.
    #[cfg(unix)]
    #[test]
    fn test_hover_at_finds_card_buttons_and_pane_close() {
        let mut config = Config::default();
        config.sidebar.visible = true;
        let sh = |cmd: &str| vec!["-c".to_string(), cmd.to_string()];
        let mut app = AppState::with_command(config, 80, 24, "/bin/sh", &sh("sleep 30")).unwrap();
        if !app.is_sidebar_visible() {
            app.toggle_sidebar();
        }
        app.open_tab_running("/bin/sh", &sh("sleep 30")).unwrap();
        let width = app.sidebar_width();
        let layout = GridLayout::below_tab_bar(9.0, 18.0).with_left_inset(width);
        let rows = sidebar_rows(&app);
        assert_eq!(rows.len(), 2);
        let second = rows[1];
        let id = app.tab_manager.tabs()[second.tab].id;
        let close = card_button_rect(CardButton::Close, second.y, width);
        let more = card_button_rect(CardButton::More, second.y, width);
        assert_eq!(
            hover_at(&app, &layout, 2000.0, close.x + 2.0, close.y + 2.0),
            Hover::Card {
                tab: id,
                button: Some(CardButton::Close)
            }
        );
        assert_eq!(
            hover_at(&app, &layout, 2000.0, more.x + 2.0, more.y + 2.0),
            Hover::Card {
                tab: id,
                button: Some(CardButton::More)
            }
        );
        assert_eq!(
            hover_at(&app, &layout, 2000.0, 30.0, second.y + 50.0),
            Hover::Card {
                tab: id,
                button: None
            }
        );
        // Tab sem divisão: o terminal não tem ×.
        let inside = (layout.origin_x + 100.0, layout.origin_y + 50.0);
        assert_eq!(
            hover_at(&app, &layout, 2000.0, inside.0, inside.1),
            Hover::None
        );

        app.split_active_pane_running(SplitDirection::Vertical, "/bin/sh", &sh("sleep 30"))
            .unwrap();
        let tab = app.active_tab().unwrap();
        let rects = tab.layout().rects(app.pane_area());
        assert_eq!(rects.len(), 2);
        for (pane, rect) in rects {
            let (px, py, w, _) = pane_pixels(&layout, rect);
            let button = pane_close_rect(px, py, w, layout.cell_height);
            // A barra de título dá foco ao painel (#32); o meio do painel não.
            assert_eq!(
                pane_title_at(&app, &layout, px + 20.0, py + 2.0),
                Some(pane)
            );
            assert_eq!(pane_title_at(&app, &layout, px + 20.0, py + 40.0), None);
            assert_eq!(
                hover_at(&app, &layout, 2000.0, button.x + 3.0, button.y + 3.0),
                Hover::Pane { pane, close: true }
            );
            assert_eq!(
                hover_at(&app, &layout, 2000.0, px + 5.0, py + 40.0),
                Hover::Pane { pane, close: false }
            );
        }
    }

    #[test]
    fn test_wrap_text() {
        assert_eq!(
            wrap_text("one two three four", 10),
            vec!["one two", "three four"]
        );
        assert_eq!(wrap_text("a\nb", 10), vec!["a", "b"]);
        assert!(wrap_text("", 10).is_empty());
    }

    #[test]
    fn test_format_age() {
        assert_eq!(format_age(2), "now");
        assert_eq!(format_age(42), "42s");
        assert_eq!(format_age(300), "5m");
        assert_eq!(format_age(7200), "2h");
        assert_eq!(format_age(200_000), "2d");
    }

    #[test]
    fn test_sidebar_details() {
        let info = crate::ui::sidebar_info::TabSidebarInfo {
            branch: Some("main".into()),
            ports: vec![3000, 5173],
            panes: 2,
            ..Default::default()
        };
        assert_eq!(
            sidebar_details(&info),
            "\u{e0a0} main  :3000 :5173  2 panes"
        );
        assert_eq!(sidebar_details(&Default::default()), "");
    }

    #[test]
    fn test_draw_cursor_shapes() {
        let layout = GridLayout::below_tab_bar(9.0, 18.0);
        let color = glyphon::Color::rgb(255, 255, 255);
        let quads = |shape| {
            let mut batch = QuadBatch::new();
            draw_cursor(&mut batch, (0.0, 0.0), &layout, shape, 2.0, color);
            batch.len()
        };
        assert_eq!(quads(CursorShape::Hollow), 4);
        assert_eq!(quads(CursorShape::Underline), 1);
        assert_eq!(quads(CursorShape::Bar), 1);
    }

    #[test]
    fn test_window_id_generation() {
        let id1 = WindowId::new();
        let id2 = WindowId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_window_state_transitions() {
        // Testa transições de estado
        assert!(WindowState::Active.is_active());
        assert!(!WindowState::Inactive.is_active());
        assert!(!WindowState::Minimized.is_active());
    }

    #[test]
    fn test_window_manager_default() {
        let manager = WindowManager::default();
        assert_eq!(manager.window_count(), 0);
        assert!(!manager.has_windows());
    }

    #[test]
    fn test_window_create_info_default() {
        let info = WindowCreateInfo::default();
        assert_eq!(info.title, "Code Terminal");
        assert_eq!(info.size, (800, 600));
        assert!(info.position.is_none());
    }
}
