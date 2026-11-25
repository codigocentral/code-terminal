//! WebView - Abstração do WebView para o browser

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

use super::events::{
    BrowserEventHandler, ConsoleEvent, ConsoleLevel, LoadEvent, LoadEventType, NavigationEvent,
    NavigationEventType,
};
use super::{BrowserConfig, BrowserError, BrowserEvent, BrowserResult};

/// Handle para um WebView
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WebViewHandle(pub usize);

impl std::fmt::Display for WebViewHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "webview-{}", self.0)
    }
}

/// Estado do WebView
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WebViewState {
    /// Não inicializado
    Uninitialized,
    /// Inicializando
    Initializing,
    /// Pronto
    Ready,
    /// Carregando
    Loading,
    /// Em erro
    Error,
    /// Destruído
    Destroyed,
}

impl std::fmt::Display for WebViewState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebViewState::Uninitialized => write!(f, "uninitialized"),
            WebViewState::Initializing => write!(f, "initializing"),
            WebViewState::Ready => write!(f, "ready"),
            WebViewState::Loading => write!(f, "loading"),
            WebViewState::Error => write!(f, "error"),
            WebViewState::Destroyed => write!(f, "destroyed"),
        }
    }
}

/// Abstração de WebView
///
/// Esta é uma abstração que será implementada por plataformas específicas.
/// As implementações reais estarão em:
/// - `platform_windows.rs` - WebView2 para Windows
/// - `platform_macos.rs` - WKWebView para macOS
/// - `platform_linux.rs` - WebKitGTK para Linux
pub struct BrowserWebView {
    /// Handle único
    handle: WebViewHandle,
    /// Estado atual
    state: WebViewState,
    /// Configuração
    config: BrowserConfig,
    /// URL atual
    current_url: Option<Url>,
    /// Título da página
    title: Option<String>,
    /// Progresso de carregamento
    load_progress: u8,
    /// Handler de eventos
    event_handler: Option<Box<dyn BrowserEventHandler>>,
    /// Scripts injetados
    injected_scripts: Vec<String>,
    /// Estilos injetados
    injected_styles: Vec<String>,
}

impl std::fmt::Debug for BrowserWebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserWebView")
            .field("handle", &self.handle)
            .field("state", &self.state)
            .field("current_url", &self.current_url)
            .field("title", &self.title)
            .field("load_progress", &self.load_progress)
            .finish()
    }
}

impl BrowserWebView {
    /// Cria um novo WebView
    pub fn new(handle: WebViewHandle, config: BrowserConfig) -> Self {
        Self {
            handle,
            state: WebViewState::Uninitialized,
            config,
            current_url: None,
            title: None,
            load_progress: 0,
            event_handler: None,
            injected_scripts: Vec::new(),
            injected_styles: Vec::new(),
        }
    }

    /// Inicializa o WebView
    ///
    /// Usa a implementação wry cross-platform.
    /// Requer uma janela winit para embeddar o WebView.
    ///
    /// # Arguments
    /// * `window` - Janela winit para embeddar o WebView
    pub fn initialize_with_window(
        &mut self,
        window: std::sync::Arc<winit::window::Window>,
    ) -> BrowserResult<()> {
        if self.state != WebViewState::Uninitialized {
            return Err(BrowserError::WebViewCreationFailed(
                "WebView already initialized".to_string(),
            ));
        }

        self.state = WebViewState::Initializing;

        // Usa a implementação wry real
        let mut wry_impl =
            crate::browser::webview_impl::WryWebView::new(self.handle, self.config.clone());

        wry_impl.initialize_with_window(window)?;

        // Atualiza estado após inicialização bem-sucedida
        self.state = WebViewState::Ready;

        tracing::info!("BrowserWebView {} initialized with wry", self.handle);
        Ok(())
    }

    /// Inicializa o WebView em modo headless (para automação/testes)
    pub fn initialize_headless(&mut self) -> BrowserResult<()> {
        if self.state != WebViewState::Uninitialized {
            return Err(BrowserError::WebViewCreationFailed(
                "WebView already initialized".to_string(),
            ));
        }

        self.state = WebViewState::Initializing;

        // Usa a implementação wry em modo headless
        let mut wry_impl =
            crate::browser::webview_impl::WryWebView::new(self.handle, self.config.clone());

        wry_impl.initialize_headless()?;

        self.state = WebViewState::Ready;

        tracing::info!(
            "BrowserWebView {} initialized in headless mode",
            self.handle
        );
        Ok(())
    }

    /// Inicialização legado (placeholder para compatibilidade)
    ///
    /// # Deprecated
    /// Use `initialize_with_window` para inicialização real.
    pub fn initialize(&mut self) -> BrowserResult<()> {
        // Para compatibilidade, apenas marca como Ready
        // A inicialização real requer uma janela
        self.state = WebViewState::Ready;
        tracing::warn!(
            "BrowserWebView {} initialized without window - functionality limited",
            self.handle
        );
        Ok(())
    }

    /// Navega para uma URL
    pub fn navigate(&mut self, url: &Url) -> BrowserResult<()> {
        self.ensure_ready()?;

        let old_url = self.current_url.clone();
        self.current_url = Some(url.clone());
        self.state = WebViewState::Loading;
        self.load_progress = 0;

        // Emite evento de navegação
        self.emit_event(BrowserEvent::Navigation(
            NavigationEvent::new(NavigationEventType::Started, url.clone())
                .with_previous(old_url.unwrap_or_else(|| url.clone())),
        ));

        // Em implementação real, aqui seria chamado o método de navegação
        // da engine específica

        Ok(())
    }

    /// Volta no histórico
    pub fn go_back(&mut self) -> BrowserResult<()> {
        self.ensure_ready()?;
        // Implementação da plataforma
        Ok(())
    }

    /// Avança no histórico
    pub fn go_forward(&mut self) -> BrowserResult<()> {
        self.ensure_ready()?;
        // Implementação da plataforma
        Ok(())
    }

    /// Recarrega
    pub fn reload(&mut self) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(url) = self.current_url.clone() {
            self.navigate(&url)?;
        }

        Ok(())
    }

    /// Para o carregamento
    pub fn stop(&mut self) -> BrowserResult<()> {
        self.ensure_ready()?;

        self.state = WebViewState::Ready;

        if let Some(url) = &self.current_url {
            self.emit_event(BrowserEvent::Load(LoadEvent::new(
                LoadEventType::Stopped,
                url.clone(),
            )));
        }

        Ok(())
    }

    /// Executa JavaScript
    pub fn execute_javascript(&self, script: &str) -> BrowserResult<String> {
        self.ensure_ready()?;

        // Em implementação real, executaria JS no contexto do WebView
        // e retornaria o resultado

        Ok(format!(
            "Executed: {}",
            script.chars().take(50).collect::<String>()
        ))
    }

    /// Injeta script
    pub fn inject_script(&mut self, script: impl Into<String>) {
        self.injected_scripts.push(script.into());
    }

    /// Injeta CSS
    pub fn inject_style(&mut self, css: impl Into<String>) {
        self.injected_styles.push(css.into());
    }

    /// Define o handler de eventos
    pub fn set_event_handler(&mut self, handler: Box<dyn BrowserEventHandler>) {
        self.event_handler = Some(handler);
    }

    /// Retorna o handle
    pub fn handle(&self) -> WebViewHandle {
        self.handle
    }

    /// Retorna o estado
    pub fn state(&self) -> WebViewState {
        self.state
    }

    /// Retorna a URL atual
    pub fn current_url(&self) -> Option<&Url> {
        self.current_url.as_ref()
    }

    /// Retorna o título
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Retorna o progresso de carregamento
    pub fn load_progress(&self) -> u8 {
        self.load_progress
    }

    /// Define o zoom
    pub fn set_zoom(&mut self, _zoom: f64) -> BrowserResult<()> {
        self.ensure_ready()?;
        // Implementação da plataforma
        Ok(())
    }

    /// Retorna para a página inicial
    pub fn go_home(&mut self) -> BrowserResult<()> {
        let home_url = match &self.config.homepage {
            super::HomepageConfig::Url(url) => Url::parse(url),
            _ => Url::parse("about:blank"),
        }
        .map_err(|e| BrowserError::InvalidUrl(e.to_string()))?;

        self.navigate(&home_url)
    }

    /// Destrói o WebView
    pub fn destroy(&mut self) -> BrowserResult<()> {
        self.state = WebViewState::Destroyed;
        self.event_handler = None;
        Ok(())
    }

    /// Verifica se está pronto
    fn ensure_ready(&self) -> BrowserResult<()> {
        match self.state {
            WebViewState::Ready | WebViewState::Loading => Ok(()),
            WebViewState::Uninitialized | WebViewState::Initializing => {
                Err(BrowserError::WebViewNotInitialized)
            }
            WebViewState::Error => Err(BrowserError::WebViewCreationFailed(
                "WebView in error state".to_string(),
            )),
            WebViewState::Destroyed => Err(BrowserError::WebViewCreationFailed(
                "WebView destroyed".to_string(),
            )),
        }
    }

    /// Emite um evento
    fn emit_event(&mut self, event: BrowserEvent) {
        if let Some(handler) = &mut self.event_handler {
            handler.on_event(event);
        }
    }

    /// Atualiza progresso de carregamento (chamado pela implementação da plataforma)
    pub fn update_load_progress(&mut self, progress: u8) {
        self.load_progress = progress.clamp(0, 100);

        if let Some(url) = &self.current_url {
            self.emit_event(BrowserEvent::Load(
                LoadEvent::new(LoadEventType::Progress, url.clone()).with_progress(progress),
            ));
        }

        if progress >= 100 {
            self.state = WebViewState::Ready;
            if let Some(url) = &self.current_url {
                self.emit_event(BrowserEvent::Load(LoadEvent::new(
                    LoadEventType::Finished,
                    url.clone(),
                )));
            }
        }
    }

    /// Atualiza título (chamado pela implementação da plataforma)
    pub fn update_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    /// Reporta erro de carregamento (chamado pela implementação da plataforma)
    pub fn report_load_error(&mut self, error: impl Into<String>) {
        self.state = WebViewState::Error;
        if let Some(url) = &self.current_url {
            self.emit_event(BrowserEvent::Load(LoadEvent::new(
                LoadEventType::Failed,
                url.clone(),
            )));
        }
        self.emit_event(BrowserEvent::Error(error.into()));
    }

    /// Reporta mensagem do console (chamado pela implementação da plataforma)
    pub fn report_console_message(&mut self, level: ConsoleLevel, message: impl Into<String>) {
        self.emit_event(BrowserEvent::Console(ConsoleEvent::new(level, message)));
    }
}

/// Gerenciador de WebViews
pub struct WebViewManager {
    webviews: HashMap<WebViewHandle, BrowserWebView>,
    next_handle: usize,
}

impl Default for WebViewManager {
    fn default() -> Self {
        Self::new()
    }
}

impl WebViewManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            webviews: HashMap::new(),
            next_handle: 1,
        }
    }

    /// Cria um novo WebView
    pub fn create_webview(&mut self, config: BrowserConfig) -> BrowserResult<WebViewHandle> {
        let handle = WebViewHandle(self.next_handle);
        self.next_handle += 1;

        let mut webview = BrowserWebView::new(handle, config);
        webview.initialize()?;

        self.webviews.insert(handle, webview);
        Ok(handle)
    }

    /// Obtém um WebView
    pub fn get(&self, handle: WebViewHandle) -> Option<&BrowserWebView> {
        self.webviews.get(&handle)
    }

    /// Obtém um WebView mutável
    pub fn get_mut(&mut self, handle: WebViewHandle) -> Option<&mut BrowserWebView> {
        self.webviews.get_mut(&handle)
    }

    /// Destrói um WebView
    pub fn destroy(&mut self, handle: WebViewHandle) -> BrowserResult<()> {
        if let Some(webview) = self.webviews.get_mut(&handle) {
            webview.destroy()?;
        }
        self.webviews.remove(&handle);
        Ok(())
    }

    /// Retorna o número de WebViews
    pub fn count(&self) -> usize {
        self.webviews.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webview_handle() {
        let handle1 = WebViewHandle(1);
        let handle2 = WebViewHandle(2);

        assert_ne!(handle1, handle2);
        assert_eq!(handle1.to_string(), "webview-1");
    }

    #[test]
    fn test_webview_state() {
        let mut webview = BrowserWebView::new(WebViewHandle(1), BrowserConfig::default());

        assert!(matches!(webview.state(), WebViewState::Uninitialized));

        webview.initialize().unwrap();
        assert!(matches!(webview.state(), WebViewState::Ready));
    }

    #[test]
    fn test_webview_navigate() {
        let mut webview = BrowserWebView::new(WebViewHandle(1), BrowserConfig::default());

        webview.initialize().unwrap();

        let url = Url::parse("https://example.com").unwrap();
        webview.navigate(&url).unwrap();

        assert_eq!(webview.current_url(), Some(&url));
        assert!(matches!(webview.state(), WebViewState::Loading));
    }

    #[test]
    fn test_webview_not_initialized() {
        let mut webview = BrowserWebView::new(WebViewHandle(1), BrowserConfig::default());

        let url = Url::parse("https://example.com").unwrap();
        let result = webview.navigate(&url);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BrowserError::WebViewNotInitialized
        ));
    }

    #[test]
    fn test_webview_manager() {
        let mut manager = WebViewManager::new();

        let handle = manager.create_webview(BrowserConfig::default()).unwrap();
        assert_eq!(manager.count(), 1);

        assert!(manager.get(handle).is_some());

        manager.destroy(handle).unwrap();
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn test_webview_progress_update() {
        let mut webview = BrowserWebView::new(WebViewHandle(1), BrowserConfig::default());

        webview.initialize().unwrap();

        let url = Url::parse("https://example.com").unwrap();
        webview.navigate(&url).unwrap();

        webview.update_load_progress(50);
        assert_eq!(webview.load_progress(), 50);

        webview.update_load_progress(100);
        assert!(matches!(webview.state(), WebViewState::Ready));
    }

    use std::sync::{Arc, Mutex};

    type Log = Arc<Mutex<Vec<BrowserEvent>>>;

    struct Recorder(Log);
    impl BrowserEventHandler for Recorder {
        fn on_event(&mut self, event: BrowserEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn ready_webview() -> (BrowserWebView, Log) {
        let log: Log = Arc::new(Mutex::new(Vec::new()));
        let mut webview = BrowserWebView::new(WebViewHandle(7), BrowserConfig::default());
        webview.initialize().unwrap();
        webview.set_event_handler(Box::new(Recorder(log.clone())));
        (webview, log)
    }

    fn load_types(log: &Log) -> Vec<LoadEventType> {
        log.lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                BrowserEvent::Load(load) => Some(load.event_type),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_state_display_and_debug() {
        let cases = [
            (WebViewState::Uninitialized, "uninitialized"),
            (WebViewState::Initializing, "initializing"),
            (WebViewState::Ready, "ready"),
            (WebViewState::Loading, "loading"),
            (WebViewState::Error, "error"),
            (WebViewState::Destroyed, "destroyed"),
        ];
        for (state, text) in cases {
            assert_eq!(state.to_string(), text);
        }
        let webview = BrowserWebView::new(WebViewHandle(3), BrowserConfig::default());
        let debug = format!("{:?}", webview);
        assert!(debug.contains("BrowserWebView") && debug.contains("Uninitialized"));
        assert_eq!(webview.handle(), WebViewHandle(3));
        assert!(webview.title().is_none());
    }

    #[test]
    fn test_navigation_emits_started_with_previous_url() {
        let (mut webview, log) = ready_webview();
        let first = Url::parse("https://a.test/").unwrap();
        let second = Url::parse("https://b.test/").unwrap();
        webview.navigate(&first).unwrap();
        webview.navigate(&second).unwrap();

        let events = log.lock().unwrap();
        let navs: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                BrowserEvent::Navigation(n) => Some(n.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(navs.len(), 2);
        assert_eq!(navs[0].event_type, NavigationEventType::Started);
        // Na primeira navegação o "anterior" é a própria URL
        assert_eq!(navs[0].previous_url.as_ref(), Some(&first));
        assert_eq!(navs[1].previous_url.as_ref(), Some(&first));
        assert_eq!(navs[1].url, second);
    }

    #[test]
    fn test_progress_finish_stop_and_errors_emit_load_events() {
        let (mut webview, log) = ready_webview();
        // Sem URL: progresso não emite evento
        webview.update_load_progress(10);
        assert!(load_types(&log).is_empty());

        webview
            .navigate(&Url::parse("https://a.test/").unwrap())
            .unwrap();
        webview.update_load_progress(250);
        assert_eq!(webview.load_progress(), 100);
        assert_eq!(webview.state(), WebViewState::Ready);
        webview.stop().unwrap();
        webview.report_console_message(ConsoleLevel::Warn, "cuidado");
        webview.update_title("Página A");
        assert_eq!(webview.title(), Some("Página A"));

        webview.report_load_error("dns falhou");
        assert_eq!(webview.state(), WebViewState::Error);
        assert_eq!(
            load_types(&log),
            vec![
                LoadEventType::Progress,
                LoadEventType::Finished,
                LoadEventType::Stopped,
                LoadEventType::Failed
            ]
        );
        let events = log.lock().unwrap();
        assert!(events
            .iter()
            .any(|e| matches!(e, BrowserEvent::Console(c) if c.message == "cuidado")));
        assert!(events
            .iter()
            .any(|e| matches!(e, BrowserEvent::Error(m) if m == "dns falhou")));
    }

    #[test]
    fn test_operations_require_ready_state() {
        let mut webview = BrowserWebView::new(WebViewHandle(1), BrowserConfig::default());
        assert!(matches!(
            webview.go_back(),
            Err(BrowserError::WebViewNotInitialized)
        ));
        assert!(webview.execute_javascript("1").is_err());

        webview.initialize().unwrap();
        webview.go_back().unwrap();
        webview.go_forward().unwrap();
        webview.set_zoom(1.5).unwrap();
        assert_eq!(
            webview.execute_javascript(&"x".repeat(80)).unwrap(),
            format!("Executed: {}", "x".repeat(50))
        );
        // reload sem URL não navega
        webview.reload().unwrap();
        assert!(webview.current_url().is_none());

        webview.report_load_error("falha");
        let err = webview.stop().unwrap_err();
        assert!(err.to_string().contains("error state"), "{err}");

        webview.destroy().unwrap();
        let err = webview.reload().unwrap_err();
        assert!(err.to_string().contains("destroyed"), "{err}");
    }

    #[test]
    fn test_reload_renavigates_and_go_home() {
        let (mut webview, log) = ready_webview();
        let url = Url::parse("https://a.test/").unwrap();
        webview.navigate(&url).unwrap();
        webview.reload().unwrap();
        assert_eq!(webview.state(), WebViewState::Loading);
        assert_eq!(
            log.lock()
                .unwrap()
                .iter()
                .filter(|e| matches!(e, BrowserEvent::Navigation(_)))
                .count(),
            2
        );

        // Homepage padrão é uma URL
        webview.go_home().unwrap();
        assert_eq!(
            webview.current_url().unwrap().as_str(),
            "https://www.google.com/"
        );

        // Homepage em branco -> about:blank
        let mut config = BrowserConfig::default();
        config.homepage = super::super::HomepageConfig::Blank;
        let mut blank = BrowserWebView::new(WebViewHandle(4), config);
        blank.initialize().unwrap();
        blank.go_home().unwrap();
        assert_eq!(blank.current_url().unwrap().as_str(), "about:blank");

        let mut config = BrowserConfig::default();
        config.homepage = super::super::HomepageConfig::Url("https://home.test/".into());
        let mut home = BrowserWebView::new(WebViewHandle(2), config);
        home.initialize().unwrap();
        home.go_home().unwrap();
        assert_eq!(home.current_url().unwrap().as_str(), "https://home.test/");

        let mut config = BrowserConfig::default();
        config.homepage = super::super::HomepageConfig::Url("não é url".into());
        let mut bad = BrowserWebView::new(WebViewHandle(3), config);
        bad.initialize().unwrap();
        assert!(matches!(bad.go_home(), Err(BrowserError::InvalidUrl(_))));
    }

    #[test]
    fn test_injection_and_manager_lookup() {
        let mut webview = BrowserWebView::new(WebViewHandle(1), BrowserConfig::default());
        webview.inject_script("console.log(1)");
        webview.inject_style("body{}");
        assert_eq!(webview.injected_scripts, vec!["console.log(1)"]);
        assert_eq!(webview.injected_styles, vec!["body{}"]);

        let mut manager = WebViewManager::default();
        let a = manager.create_webview(BrowserConfig::default()).unwrap();
        let b = manager.create_webview(BrowserConfig::default()).unwrap();
        assert_ne!(a, b);
        manager
            .get_mut(a)
            .unwrap()
            .navigate(&Url::parse("https://a.test/").unwrap())
            .unwrap();
        assert_eq!(manager.get(a).unwrap().state(), WebViewState::Loading);
        // destruir handle desconhecido não falha
        manager.destroy(WebViewHandle(999)).unwrap();
        assert_eq!(manager.count(), 2);
    }
}
