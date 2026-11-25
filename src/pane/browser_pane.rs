//! BrowserPane - Implementação de Pane para o Browser integrado
//!
//! Este módulo fornece uma implementação do trait `PaneContent` para o browser,
//! permitindo que o navegador seja integrado ao sistema de panes unificado.

use super::{
    content::{PaneContent, PaneContentType, PaneState},
    input::{InputEvent, KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    PaneError, PaneId, PaneMetadata, PaneResult, RenderInfo,
};
use crate::browser::integration::BrowserIntegration;
use crate::browser::{BrowserConfig, BrowserId, BrowserManager, BrowserWebView, WebViewHandle};
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::{debug, trace};
use url::Url;

/// Contador global para IDs de BrowserPane
static BROWSER_PANE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Gera um novo handle de WebView único
fn generate_webview_handle() -> WebViewHandle {
    WebViewHandle(BROWSER_PANE_ID_COUNTER.fetch_add(1, Ordering::SeqCst) as usize)
}

/// BrowserPane - Implementação de Pane para navegador web
///
/// Integra o BrowserWebView ao sistema de panes, permitindo:
/// - Renderização do navegador dentro de um pane
/// - Navegação via comandos/URL
/// - Input de teclado e mouse
/// - Sincronização de estado entre tabs e browser
#[derive(Debug)]
pub struct BrowserPane {
    /// ID único do pane
    id: PaneId,
    /// Handle do WebView
    webview_handle: WebViewHandle,
    /// Instância do WebView (quando inicializado)
    webview: Option<BrowserWebView>,
    /// Gerenciador de browsers
    browser_manager: BrowserManager,
    /// Integração com o sistema de panes
    browser_integration: BrowserIntegration,
    /// ID do browser associado
    browser_id: Option<BrowserId>,
    /// Metadados do pane
    metadata: PaneMetadata,
    /// Informações de renderização
    render_info: RenderInfo,
    /// Estado atual
    state: PaneState,
    /// Flag indicando se precisa ser re-renderizado
    dirty: bool,
    /// URL atual
    current_url: Option<Url>,
    /// Título da página
    page_title: Option<String>,
    /// Progresso de carregamento (0-100)
    load_progress: u8,
    /// Configuração do browser
    config: BrowserConfig,
    /// Modo de foco na barra de endereço
    address_bar_focused: bool,
    /// Texto da barra de endereço
    address_bar_text: String,
    /// Histórico de URLs navegadas
    navigation_history: Vec<Url>,
    /// Posição atual no histórico
    history_position: usize,
}

impl BrowserPane {
    /// Cria um novo BrowserPane
    ///
    /// # Arguments
    /// * `config` - Configuração opcional do browser
    ///
    /// # Example
    /// ```
    /// use code_terminal::pane::BrowserPane;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let pane = BrowserPane::new(None)?;
    /// # let _ = pane;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(config: Option<BrowserConfig>) -> PaneResult<Self> {
        let config = config.unwrap_or_default();
        let webview_handle = generate_webview_handle();
        let browser_manager = BrowserManager::new();
        let browser_integration = BrowserIntegration::new();

        let mut metadata = PaneMetadata::default();
        metadata.title = "Browser".to_string();

        Ok(Self {
            id: super::generate_pane_id(),
            webview_handle,
            webview: None,
            browser_manager,
            browser_integration,
            browser_id: None,
            metadata,
            render_info: RenderInfo::new(0, 0, 0, 0),
            state: PaneState::Background,
            dirty: true,
            current_url: None,
            page_title: None,
            load_progress: 0,
            config,
            address_bar_focused: false,
            address_bar_text: String::new(),
            navigation_history: Vec::new(),
            history_position: 0,
        })
    }

    /// Cria um BrowserPane com URL inicial
    pub fn with_url(mut self, url: impl AsRef<str>) -> PaneResult<Self> {
        let url = Url::parse(url.as_ref())
            .map_err(|e| PaneError::InvalidState(format!("URL inválida: {}", e)))?;
        self.current_url = Some(url.clone());
        self.address_bar_text = url.to_string();
        self.navigation_history.push(url);
        Ok(self)
    }

    /// Cria um BrowserPane com título personalizado
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.metadata.title = title.into();
        self
    }

    /// Retorna a integração browser-pane associada.
    pub fn browser_integration(&self) -> &BrowserIntegration {
        &self.browser_integration
    }

    /// Inicializa o WebView com uma janela winit
    ///
    /// # Arguments
    /// * `window` - Janela winit para embeddar o WebView
    pub fn initialize_with_window(
        &mut self,
        window: std::sync::Arc<winit::window::Window>,
    ) -> PaneResult<()> {
        if self.webview.is_some() {
            return Err(PaneError::InvalidState(
                "WebView já inicializado".to_string(),
            ));
        }

        let mut webview = BrowserWebView::new(self.webview_handle, self.config.clone());

        webview
            .initialize_with_window(window)
            .map_err(|e| PaneError::Internal(e.into()))?;

        self.webview = Some(webview);

        // Navega para URL inicial se houver
        if let Some(url) = &self.current_url {
            self.navigate_to(url.clone())?;
        }

        debug!(pane_id = self.id, "BrowserPane inicializado com WebView");
        Ok(())
    }

    /// Navega para uma URL
    pub fn navigate_to(&mut self, url: Url) -> PaneResult<()> {
        debug!(pane_id = self.id, %url, "Navegando para URL");

        self.current_url = Some(url.clone());
        self.address_bar_text = url.to_string();
        self.load_progress = 0;
        self.dirty = true;

        // Adiciona ao histórico se for uma nova navegação
        if self.history_position < self.navigation_history.len() {
            self.navigation_history.truncate(self.history_position + 1);
        }
        self.navigation_history.push(url.clone());
        self.history_position = self.navigation_history.len() - 1;

        // Atualiza o browser manager se tiver browser associado
        if let Some(browser_id) = self.browser_id {
            let _ = self.browser_manager.navigate(browser_id, url.as_str());
        }

        Ok(())
    }

    /// Navega para uma URL a partir de string
    pub fn navigate(&mut self, url: impl AsRef<str>) -> PaneResult<()> {
        let url = Url::parse(url.as_ref())
            .map_err(|e| PaneError::InvalidState(format!("URL inválida: {}", e)))?;
        self.navigate_to(url)
    }

    /// Volta no histórico
    pub fn go_back(&mut self) -> PaneResult<()> {
        if self.history_position > 0 {
            self.history_position -= 1;
            let url = self.navigation_history[self.history_position].clone();
            self.current_url = Some(url.clone());
            self.address_bar_text = url.to_string();
            self.dirty = true;

            debug!(pane_id = self.id, %url, "Voltando no histórico");

            if let Some(browser_id) = self.browser_id {
                let _ = self.browser_manager.go_back(browser_id);
            }
        }
        Ok(())
    }

    /// Avança no histórico
    pub fn go_forward(&mut self) -> PaneResult<()> {
        if self.history_position < self.navigation_history.len().saturating_sub(1) {
            self.history_position += 1;
            let url = self.navigation_history[self.history_position].clone();
            self.current_url = Some(url.clone());
            self.address_bar_text = url.to_string();
            self.dirty = true;

            debug!(pane_id = self.id, %url, "Avançando no histórico");

            if let Some(browser_id) = self.browser_id {
                let _ = self.browser_manager.go_forward(browser_id);
            }
        }
        Ok(())
    }

    /// Recarrega a página atual
    pub fn reload(&mut self) -> PaneResult<()> {
        if let Some(url) = &self.current_url {
            debug!(pane_id = self.id, %url, "Recarregando página");
            self.load_progress = 0;
            self.dirty = true;

            if let Some(browser_id) = self.browser_id {
                let _ = self.browser_manager.reload(browser_id);
            }
        }
        Ok(())
    }

    /// Para o carregamento
    pub fn stop(&mut self) -> PaneResult<()> {
        debug!(pane_id = self.id, "Parando carregamento");

        if let Some(browser_id) = self.browser_id {
            let _ = self.browser_manager.stop(browser_id);
        }
        Ok(())
    }

    /// Retorna a URL atual
    pub fn current_url(&self) -> Option<&Url> {
        self.current_url.as_ref()
    }

    /// Retorna o título da página
    pub fn page_title(&self) -> Option<&str> {
        self.page_title.as_deref()
    }

    /// Define o título da página
    pub fn set_page_title(&mut self, title: impl Into<String>) {
        self.page_title = Some(title.into());
        self.dirty = true;
    }

    /// Retorna o progresso de carregamento
    pub fn load_progress(&self) -> u8 {
        self.load_progress
    }

    /// Atualiza o progresso de carregamento
    pub fn set_load_progress(&mut self, progress: u8) {
        self.load_progress = progress.clamp(0, 100);
        self.dirty = true;
    }

    /// Retorna se pode voltar no histórico
    pub fn can_go_back(&self) -> bool {
        self.history_position > 0
    }

    /// Retorna se pode avançar no histórico
    pub fn can_go_forward(&self) -> bool {
        self.history_position < self.navigation_history.len().saturating_sub(1)
    }

    /// Foca na barra de endereço
    pub fn focus_address_bar(&mut self) {
        self.address_bar_focused = true;
        self.dirty = true;
    }

    /// Remove foco da barra de endereço
    pub fn blur_address_bar(&mut self) {
        self.address_bar_focused = false;
        self.dirty = true;
    }

    /// Retorna se a barra de endereço está focada
    pub fn is_address_bar_focused(&self) -> bool {
        self.address_bar_focused
    }

    /// Retorna o texto da barra de endereço
    pub fn address_bar_text(&self) -> &str {
        &self.address_bar_text
    }

    /// Define o texto da barra de endereço
    pub fn set_address_bar_text(&mut self, text: impl Into<String>) {
        self.address_bar_text = text.into();
        self.dirty = true;
    }

    /// Processa input na barra de endereço
    fn process_address_bar_input(&mut self, event: &InputEvent) -> bool {
        if !self.address_bar_focused {
            return false;
        }

        match event {
            InputEvent::Key { data, key_code, .. } => {
                match key_code {
                    Some(KeyCode::Enter) => {
                        // Navega para a URL digitada
                        let url_str = self.address_bar_text.trim();
                        if !url_str.is_empty() {
                            // Adiciona https:// se não tiver protocolo
                            let url_str = if url_str.contains("://") {
                                url_str.to_string()
                            } else {
                                format!("https://{}", url_str)
                            };

                            if let Ok(url) = Url::parse(&url_str) {
                                let _ = self.navigate_to(url);
                            }
                        }
                        self.blur_address_bar();
                        true
                    }
                    Some(KeyCode::Escape) => {
                        // Cancela e restaura URL atual
                        if let Some(url) = &self.current_url {
                            self.address_bar_text = url.to_string();
                        }
                        self.blur_address_bar();
                        true
                    }
                    Some(KeyCode::Backspace) => {
                        // Remove último caractere
                        self.address_bar_text.pop();
                        self.dirty = true;
                        true
                    }
                    Some(KeyCode::Char(c)) => {
                        // Adiciona caractere
                        self.address_bar_text.push(*c);
                        self.dirty = true;
                        true
                    }
                    _ => {
                        // Outras teclas - adiciona os bytes
                        if let Ok(s) = std::str::from_utf8(data) {
                            self.address_bar_text.push_str(s);
                            self.dirty = true;
                            true
                        } else {
                            false
                        }
                    }
                }
            }
            _ => false,
        }
    }

    /// Processa atalhos de navegação
    fn process_navigation_shortcuts(&mut self, event: &InputEvent) -> bool {
        match event {
            InputEvent::Key {
                key_code,
                modifiers,
                ..
            } => {
                match (key_code, modifiers) {
                    // Alt+Left - Voltar
                    (Some(KeyCode::Arrow(arrow)), KeyModifiers { alt: true, .. }) => match arrow {
                        super::input::ArrowKey::Left => {
                            let _ = self.go_back();
                            true
                        }
                        super::input::ArrowKey::Right => {
                            let _ = self.go_forward();
                            true
                        }
                        _ => false,
                    },
                    // F5 - Recarregar
                    (Some(KeyCode::F(5)), _) => {
                        let _ = self.reload();
                        true
                    }
                    // Ctrl+L - Focar barra de endereço
                    (Some(KeyCode::Char('l')), KeyModifiers { ctrl: true, .. }) => {
                        self.focus_address_bar();
                        true
                    }
                    // Ctrl+R - Recarregar
                    (Some(KeyCode::Char('r')), KeyModifiers { ctrl: true, .. }) => {
                        let _ = self.reload();
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

impl PaneContent for BrowserPane {
    fn pane_id(&self) -> PaneId {
        self.id
    }

    fn content_type(&self) -> PaneContentType {
        PaneContentType::Custom("browser")
    }

    fn state(&self) -> PaneState {
        self.state
    }

    fn set_state(&mut self, state: PaneState) {
        if self.state != state {
            debug!(pane_id = self.id, ?state, "Mudando estado do BrowserPane");
            self.state = state;
            self.dirty = true;
        }
    }

    fn metadata(&self) -> &PaneMetadata {
        &self.metadata
    }

    fn set_metadata(&mut self, metadata: PaneMetadata) {
        self.metadata = metadata;
    }

    fn render_info(&self) -> RenderInfo {
        self.render_info
    }

    fn set_render_info(&mut self, info: RenderInfo) {
        self.render_info = info;
        self.dirty = true;
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn mark_clean(&mut self) {
        self.dirty = false;
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn process_input(&mut self, event: &InputEvent) -> PaneResult<bool> {
        trace!(
            ?event,
            pane_id = self.id,
            "Processando input no BrowserPane"
        );

        // Se estiver em modo de foco na barra de endereço, processa lá
        if self.address_bar_focused {
            return Ok(self.process_address_bar_input(event));
        }

        // Primeiro tenta atalhos de navegação
        if self.process_navigation_shortcuts(event) {
            return Ok(true);
        }

        // Processa outros inputs
        match event {
            InputEvent::Key {
                key_code,
                modifiers,
                ..
            } => {
                match (key_code, modifiers) {
                    // Focar barra de endereço com '/'
                    (
                        Some(KeyCode::Char('/')),
                        KeyModifiers {
                            ctrl: false,
                            alt: false,
                            ..
                        },
                    ) => {
                        self.focus_address_bar();
                        self.address_bar_text.clear();
                        Ok(true)
                    }
                    _ => {
                        // Passa o input para o WebView se estiver inicializado
                        // Nota: Na prática, o WebView captura input diretamente
                        trace!("Input passado para WebView");
                        Ok(false)
                    }
                }
            }
            InputEvent::Mouse { kind, position, .. } => {
                match kind {
                    MouseEventKind::Click(MouseButton::Left) => {
                        // Verifica se clicou na barra de endereço (área superior)
                        let (_, rows) = self.content_size();
                        let address_bar_height = 30u32; // pixels
                        let cell_height = self.render_info.size.1 / rows as u32;

                        if position.1 as u32 * cell_height < address_bar_height {
                            self.focus_address_bar();
                            return Ok(true);
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        // Scroll para cima - passa para o WebView
                        trace!("Scroll up no BrowserPane");
                    }
                    MouseEventKind::ScrollDown => {
                        // Scroll para baixo - passa para o WebView
                        trace!("Scroll down no BrowserPane");
                    }
                    _ => {}
                }
                Ok(false)
            }
            InputEvent::Resize { cols, rows } => {
                self.resize(*cols, *rows)?;
                Ok(true)
            }
        }
    }

    fn update(&mut self) -> PaneResult<()> {
        // Atualiza estado do browser
        if let Some(browser_id) = self.browser_id {
            if let Some(instance) = self.browser_manager.get_browser(browser_id) {
                // Atualiza progresso
                if instance.load_progress != self.load_progress {
                    self.load_progress = instance.load_progress;
                    self.dirty = true;
                }

                // Atualiza título
                if instance.title != self.page_title {
                    self.page_title = instance.title.clone();
                    self.metadata.title = instance
                        .title
                        .clone()
                        .unwrap_or_else(|| "Browser".to_string());
                    self.dirty = true;
                }

                // Atualiza URL
                if instance.current_url != self.current_url {
                    self.current_url = instance.current_url.clone();
                    if let Some(url) = &self.current_url {
                        self.address_bar_text = url.to_string();
                    }
                    self.dirty = true;
                }
            }
        }

        Ok(())
    }

    fn get_content_text(&self) -> String {
        // Retorna informações da página como texto
        format!(
            "Browser: {}\nURL: {}\nProgress: {}%",
            self.page_title.as_deref().unwrap_or("Untitled"),
            self.current_url
                .as_ref()
                .map(|u| u.as_str())
                .unwrap_or("about:blank"),
            self.load_progress
        )
    }

    fn content_size(&self) -> (u16, u16) {
        // Calcula tamanho baseado no render_info
        // Assume células de 8x16 pixels
        let cell_width = 8u32;
        let cell_height = 16u32;

        let cols = (self.render_info.size.0 / cell_width).max(80) as u16;
        let rows = (self.render_info.size.1 / cell_height).max(24) as u16;

        (cols, rows)
    }

    fn resize(&mut self, cols: u16, rows: u16) -> PaneResult<()> {
        debug!(pane_id = self.id, cols, rows, "Redimensionando BrowserPane");

        // Atualiza render_info baseado no novo tamanho
        let cell_width = 8u32;
        let cell_height = 16u32;

        self.render_info.size = (cols as u32 * cell_width, rows as u32 * cell_height);

        self.dirty = true;
        Ok(())
    }

    fn scroll_up(&mut self, lines: usize) {
        // O scroll é gerenciado pelo WebView
        trace!(lines, "Scroll up no BrowserPane");
        self.dirty = true;
    }

    fn scroll_down(&mut self, lines: usize) {
        // O scroll é gerenciado pelo WebView
        trace!(lines, "Scroll down no BrowserPane");
        self.dirty = true;
    }

    fn scroll_to_top(&mut self) {
        trace!("Scroll to top no BrowserPane");
        self.dirty = true;
    }

    fn scroll_to_bottom(&mut self) {
        trace!("Scroll to bottom no BrowserPane");
        self.dirty = true;
    }

    fn serialize(&self) -> serde_json::Value {
        serde_json::json!({
            "pane_id": self.id,
            "content_type": "Browser",
            "state": format!("{:?}", self.state),
            "title": self.metadata.title,
            "url": self.current_url.as_ref().map(|u| u.to_string()),
            "page_title": self.page_title,
            "load_progress": self.load_progress,
            "can_go_back": self.can_go_back(),
            "can_go_forward": self.can_go_forward(),
        })
    }
}

/// Builder para criar BrowserPanes com configuração fluente
#[derive(Debug, Default)]
pub struct BrowserPaneBuilder {
    config: BrowserConfig,
    url: Option<String>,
    title: Option<String>,
}

impl BrowserPaneBuilder {
    /// Cria um novo builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Define a URL inicial
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Define o título
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Define a configuração
    pub fn with_config(mut self, config: BrowserConfig) -> Self {
        self.config = config;
        self
    }

    /// Constrói o BrowserPane
    pub fn build(self) -> PaneResult<BrowserPane> {
        let mut pane = BrowserPane::new(Some(self.config))?;

        if let Some(title) = self.title {
            pane.metadata.title = title;
        }

        if let Some(url) = self.url {
            pane = pane.with_url(url)?;
        }

        Ok(pane)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_pane_creation() {
        let pane = BrowserPane::new(None).unwrap();
        assert_eq!(pane.content_type(), PaneContentType::Custom("browser"));
        assert!(pane.is_dirty());
        assert_eq!(pane.state(), PaneState::Background);
    }

    #[test]
    fn test_browser_pane_with_url() {
        let pane = BrowserPane::new(None)
            .unwrap()
            .with_url("https://example.com")
            .unwrap();

        assert!(pane.current_url().is_some());
        assert_eq!(pane.address_bar_text(), "https://example.com/");
    }

    #[test]
    fn test_browser_pane_navigation() {
        let mut pane = BrowserPane::new(None).unwrap();

        // Navega para primeira URL
        pane.navigate("https://first.com").unwrap();
        assert!(!pane.can_go_back());

        // Navega para segunda URL
        pane.navigate("https://second.com").unwrap();
        assert!(pane.can_go_back());

        // Volta
        pane.go_back().unwrap();
        assert!(pane.can_go_forward());

        // Avança
        pane.go_forward().unwrap();
        assert!(!pane.can_go_forward());
    }

    #[test]
    fn test_browser_pane_address_bar() {
        let mut pane = BrowserPane::new(None).unwrap();

        assert!(!pane.is_address_bar_focused());

        pane.focus_address_bar();
        assert!(pane.is_address_bar_focused());

        pane.set_address_bar_text("test");
        assert_eq!(pane.address_bar_text(), "test");

        pane.blur_address_bar();
        assert!(!pane.is_address_bar_focused());
    }

    #[test]
    fn test_browser_pane_builder() {
        let pane = BrowserPaneBuilder::new()
            .with_url("https://example.com")
            .with_title("Test Browser")
            .build()
            .unwrap();

        assert_eq!(pane.metadata().title, "Test Browser");
        assert!(pane.current_url().is_some());
    }

    #[test]
    fn test_browser_pane_serialize() {
        let pane = BrowserPane::new(None)
            .unwrap()
            .with_url("https://example.com")
            .unwrap();

        let json = pane.serialize();
        assert_eq!(json["content_type"], "Browser");
        assert!(json["url"].is_string());
    }

    #[test]
    fn test_browser_pane_process_shortcuts() {
        let mut pane = BrowserPane::new(None).unwrap();

        // Testa atalho Ctrl+L
        let event = InputEvent::Key {
            data: vec![0x0c], // Ctrl+L
            key_code: Some(KeyCode::Char('l')),
            modifiers: KeyModifiers {
                ctrl: true,
                alt: false,
                shift: false,
                meta: false,
            },
        };

        let result = pane.process_input(&event).unwrap();
        assert!(result);
        assert!(pane.is_address_bar_focused());
    }

    #[test]
    fn test_browser_pane_resize() {
        let mut pane = BrowserPane::new(None).unwrap();
        pane.mark_clean();

        pane.resize(100, 30).unwrap();
        assert!(pane.is_dirty());

        let (cols, rows) = pane.content_size();
        assert!(cols >= 80);
        assert!(rows >= 24);
    }
}
