//! WebView Implementation using wry
//!
//! This module provides the actual WebView implementation using the wry crate,
//! which provides cross-platform WebView functionality using:
//! - WebKit2GTK on Linux
//! - WebView2 on Windows  
//! - WKWebView on macOS

use std::sync::{Arc, Mutex, MutexGuard};
use url::Url;
use wry::{WebView, WebViewBuilder};

use super::events::{
    BrowserEventHandler, ConsoleEvent, ConsoleLevel, LoadEvent, LoadEventType, NavigationEvent,
    NavigationEventType,
};
use super::{
    BrowserConfig, BrowserError, BrowserEvent, BrowserResult, WebViewHandle, WebViewState,
};

/// WebView implementation using wry
pub struct WryWebView {
    /// Handle único
    handle: WebViewHandle,
    /// Estado atual
    state: Arc<Mutex<WebViewState>>,
    /// Instância wry WebView
    webview: Option<WebView>,
    /// Configuração
    config: BrowserConfig,
    /// URL atual
    current_url: Arc<Mutex<Option<Url>>>,
    /// Título da página
    title: Arc<Mutex<Option<String>>>,
    /// Progresso de carregamento
    load_progress: Arc<Mutex<u8>>,
    /// Handler de eventos
    event_handler: Arc<Mutex<Option<Box<dyn BrowserEventHandler>>>>,
    /// Scripts injetados
    injected_scripts: Arc<Mutex<Vec<String>>>,
    /// Estilos injetados
    injected_styles: Arc<Mutex<Vec<String>>>,
}

impl std::fmt::Debug for WryWebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WryWebView")
            .field("handle", &self.handle)
            .field("state", &self.state)
            .field("current_url", &self.current_url)
            .field("title", &self.title)
            .field("load_progress", &self.load_progress)
            .finish()
    }
}

impl WryWebView {
    fn lock<'a, T>(mutex: &'a Mutex<T>, name: &str) -> BrowserResult<MutexGuard<'a, T>> {
        mutex
            .lock()
            .map_err(|_| BrowserError::InvalidState(format!("WebView {} mutex poisoned", name)))
    }

    /// Cria um novo WebView
    pub fn new(handle: WebViewHandle, config: BrowserConfig) -> Self {
        Self {
            handle,
            state: Arc::new(Mutex::new(WebViewState::Uninitialized)),
            webview: None,
            config,
            current_url: Arc::new(Mutex::new(None)),
            title: Arc::new(Mutex::new(None)),
            load_progress: Arc::new(Mutex::new(0)),
            event_handler: Arc::new(Mutex::new(None)),
            injected_scripts: Arc::new(Mutex::new(Vec::new())),
            injected_styles: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Inicializa o WebView com uma janela pai
    ///
    /// # Arguments
    ///
    /// * `window` - A janela pai do winit para embutir o WebView
    pub fn initialize_with_window(
        &mut self,
        window: std::sync::Arc<winit::window::Window>,
    ) -> BrowserResult<()> {
        if *Self::lock(&self.state, "state")? != WebViewState::Uninitialized {
            return Err(BrowserError::WebViewCreationFailed(
                "WebView already initialized".to_string(),
            ));
        }

        *Self::lock(&self.state, "state")? = WebViewState::Initializing;

        // Clona os Arcs para usar nos callbacks
        let state = Arc::clone(&self.state);
        let current_url = Arc::clone(&self.current_url);
        let title = Arc::clone(&self.title);
        let load_progress = Arc::clone(&self.load_progress);
        let event_handler = Arc::clone(&self.event_handler);
        let handle = self.handle;

        // Cria o WebViewBuilder
        let mut builder = WebViewBuilder::new()
            .with_url(self.get_home_url())
            .with_devtools(true)
            .with_accept_first_mouse(true)
            .with_back_forward_navigation_gestures(true);

        // Adiciona scripts injetados
        let scripts = Self::lock(&self.injected_scripts, "injected_scripts")?.clone();
        for script in scripts {
            builder = builder.with_initialization_script(&script);
        }

        // Configura handlers de navegação
        let _nav_state = Arc::clone(&state);
        let nav_url = Arc::clone(&current_url);
        let nav_handler = Arc::clone(&event_handler);
        let _nav_handle = handle;

        builder = builder.with_navigation_handler(move |url_str: String| {
            tracing::debug!("Navigation requested: {}", url_str);

            if let Ok(url) = Url::parse(&url_str) {
                if let Ok(mut current_url) = nav_url.lock() {
                    *current_url = Some(url.clone());
                } else {
                    tracing::error!("WebView current_url mutex poisoned during navigation");
                }

                // Emite evento de navegação
                if let Ok(mut handler) = nav_handler.lock() {
                    if let Some(ref mut h) = *handler {
                        h.on_event(BrowserEvent::Navigation(NavigationEvent::new(
                            NavigationEventType::Started,
                            url,
                        )));
                    }
                }
            }

            true // Permite navegação
        });

        // Configura handler de carregamento
        let load_state = Arc::clone(&state);
        let load_progress_arc = Arc::clone(&load_progress);
        let load_handler = Arc::clone(&event_handler);
        let _load_handle = handle;

        builder = builder.with_on_page_load_handler(move |event, url_str| match event {
            wry::PageLoadEvent::Started => {
                tracing::debug!("Page load started: {}", url_str);
                if let Ok(mut progress) = load_progress_arc.lock() {
                    *progress = 0;
                } else {
                    tracing::error!("WebView load_progress mutex poisoned on load start");
                }
            }
            wry::PageLoadEvent::Finished => {
                tracing::debug!("Page load finished: {}", url_str);
                if let Ok(mut progress) = load_progress_arc.lock() {
                    *progress = 100;
                } else {
                    tracing::error!("WebView load_progress mutex poisoned on load finish");
                }
                if let Ok(mut state) = load_state.lock() {
                    *state = WebViewState::Ready;
                } else {
                    tracing::error!("WebView state mutex poisoned on load finish");
                }

                if let Ok(url) = Url::parse(&url_str) {
                    if let Ok(mut handler) = load_handler.lock() {
                        if let Some(ref mut h) = *handler {
                            h.on_event(BrowserEvent::Load(LoadEvent::new(
                                LoadEventType::Finished,
                                url,
                            )));
                        }
                    }
                }
            }
        });

        // Configura handler de título
        let title_title = Arc::clone(&title);
        let _title_handler = Arc::clone(&event_handler);

        builder = builder.with_document_title_changed_handler(move |new_title| {
            if let Ok(mut title) = title_title.lock() {
                *title = Some(new_title.clone());
            } else {
                tracing::error!("WebView title mutex poisoned on title change");
            }
            tracing::debug!("Page title changed: {}", new_title);
        });

        // Configura IPC handler para comunicação bidirecional
        let ipc_handler = Arc::clone(&event_handler);
        builder = builder.with_ipc_handler(move |request: wry::http::Request<String>| {
            let message = request.body();
            tracing::debug!("IPC message received: {}", message);

            // Tenta parse como evento do console ou mensagem customizada
            if let Ok(mut handler) = ipc_handler.lock() {
                if let Some(ref mut h) = *handler {
                    h.on_event(BrowserEvent::Console(ConsoleEvent::new(
                        ConsoleLevel::Log,
                        message.clone(),
                    )));
                }
            }
        });

        // Build do WebView
        let webview = builder
            .build(&window)
            .map_err(|e| BrowserError::WebViewCreationFailed(e.to_string()))?;

        self.webview = Some(webview);
        *Self::lock(&self.state, "state")? = WebViewState::Ready;

        tracing::info!("WebView {} initialized successfully", self.handle);
        Ok(())
    }

    /// Inicializa o WebView em modo headless (sem janela)
    /// Útil para automação e testes
    pub fn initialize_headless(&mut self) -> BrowserResult<()> {
        // Para modo headless, precisamos criar uma janela invisível
        // Isso é uma limitação do wry - requer uma janela

        use winit::{event::Event, event_loop::EventLoop, window::WindowAttributes};

        let event_loop =
            EventLoop::new().map_err(|e| BrowserError::WebViewCreationFailed(e.to_string()))?;

        let window_attrs = WindowAttributes::default()
            .with_visible(false)
            .with_inner_size(winit::dpi::LogicalSize::new(800, 600));

        // Canal para obter a janela criada do event loop
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Arc::new(Mutex::new(Some(tx)));

        // Executa o event loop para criar a janela
        // TODO: Refatorar para usar `run_app` quando winit 0.30+ estabilizar completamente
        #[allow(deprecated)]
        let _ = event_loop.run(move |event, elwt| match event {
            Event::Resumed => {
                if let Ok(mut tx_guard) = tx.lock() {
                    if let Some(sender) = tx_guard.take() {
                        match elwt.create_window(window_attrs.clone()) {
                            Ok(window) => {
                                let _ = sender.send(Ok(Arc::new(window)));
                            }
                            Err(e) => {
                                let _ = sender
                                    .send(Err(BrowserError::WebViewCreationFailed(e.to_string())));
                            }
                        }
                    }
                } else {
                    tracing::error!("WebView window channel mutex poisoned");
                }
                elwt.exit();
            }
            Event::AboutToWait => {
                elwt.exit();
            }
            _ => {}
        });

        // Recebe o resultado da criação da janela
        let window = rx
            .recv()
            .map_err(|e| BrowserError::WebViewCreationFailed(format!("Channel error: {}", e)))?;

        self.initialize_with_window(window?)
    }

    /// Navega para uma URL
    pub fn navigate(&mut self, url: &Url) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            *Self::lock(&self.current_url, "current_url")? = Some(url.clone());
            *Self::lock(&self.state, "state")? = WebViewState::Loading;
            *Self::lock(&self.load_progress, "load_progress")? = 0;

            webview
                .load_url(url.as_str())
                .map_err(|e| BrowserError::NavigationFailed(e.to_string()))?;

            // Emite evento
            self.emit_event(BrowserEvent::Navigation(NavigationEvent::new(
                NavigationEventType::Started,
                url.clone(),
            )));

            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Volta no histórico
    pub fn go_back(&self) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            // Navegação back via JavaScript (can_go_back não disponível na API wry)
            webview
                .evaluate_script("history.back()")
                .map_err(|e| BrowserError::NavigationFailed(e.to_string()))?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Avança no histórico
    pub fn go_forward(&self) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            // Navegação forward via JavaScript (can_go_forward não disponível na API wry)
            webview
                .evaluate_script("history.forward()")
                .map_err(|e| BrowserError::NavigationFailed(e.to_string()))?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Recarrega
    pub fn reload(&mut self) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            *Self::lock(&self.state, "state")? = WebViewState::Loading;
            *Self::lock(&self.load_progress, "load_progress")? = 0;

            webview
                .reload()
                .map_err(|e| BrowserError::NavigationFailed(e.to_string()))?;

            if let Some(ref url) = *Self::lock(&self.current_url, "current_url")? {
                self.emit_event(BrowserEvent::Navigation(NavigationEvent::new(
                    NavigationEventType::Started,
                    url.clone(),
                )));
            }

            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Para o carregamento
    pub fn stop(&self) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            webview
                .evaluate_script("window.stop()")
                .map_err(|e| BrowserError::NavigationFailed(e.to_string()))?;

            *Self::lock(&self.state, "state")? = WebViewState::Ready;

            if let Some(ref url) = *Self::lock(&self.current_url, "current_url")? {
                self.emit_event(BrowserEvent::Load(LoadEvent::new(
                    LoadEventType::Stopped,
                    url.clone(),
                )));
            }

            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Executa JavaScript
    pub fn execute_javascript(&self, script: &str) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            webview
                .evaluate_script(script)
                .map_err(|e| BrowserError::ScriptExecutionFailed(e.to_string()))
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Injeta script
    pub fn inject_script(&self, script: impl Into<String>) {
        match self.injected_scripts.lock() {
            Ok(mut scripts) => scripts.push(script.into()),
            Err(_) => tracing::error!("WebView injected_scripts mutex poisoned"),
        }
    }

    /// Injeta CSS
    pub fn inject_style(&self, css: impl Into<String>) {
        let css = css.into();
        match self.injected_styles.lock() {
            Ok(mut styles) => styles.push(css.clone()),
            Err(_) => tracing::error!("WebView injected_styles mutex poisoned"),
        }

        // Injeta imediatamente se já estiver pronto
        if let Some(ref webview) = self.webview {
            let script = format!(
                r#"
                (function() {{
                    var style = document.createElement('style');
                    style.textContent = `{}`;
                    document.head.appendChild(style);
                }})();
                "#,
                css.replace('`', "\\`")
            );
            if let Err(error) = webview.evaluate_script(&script) {
                tracing::error!(error = %error, "Failed to inject WebView style");
            }
        }
    }

    /// Define o handler de eventos
    pub fn set_event_handler(&self, handler: Box<dyn BrowserEventHandler>) {
        match self.event_handler.lock() {
            Ok(mut event_handler) => *event_handler = Some(handler),
            Err(_) => tracing::error!("WebView event_handler mutex poisoned"),
        }
    }

    /// Retorna o handle
    pub fn handle(&self) -> WebViewHandle {
        self.handle
    }

    /// Retorna o estado
    pub fn state(&self) -> WebViewState {
        match self.state.lock() {
            Ok(state) => *state,
            Err(_) => {
                tracing::error!("WebView state mutex poisoned");
                WebViewState::Error
            }
        }
    }

    /// Retorna a URL atual
    pub fn current_url(&self) -> Option<Url> {
        match self.current_url.lock() {
            Ok(url) => url.clone(),
            Err(_) => {
                tracing::error!("WebView current_url mutex poisoned");
                None
            }
        }
    }

    /// Retorna o título
    pub fn title(&self) -> Option<String> {
        match self.title.lock() {
            Ok(title) => title.clone(),
            Err(_) => {
                tracing::error!("WebView title mutex poisoned");
                None
            }
        }
    }

    /// Retorna o progresso de carregamento
    pub fn load_progress(&self) -> u8 {
        match self.load_progress.lock() {
            Ok(progress) => *progress,
            Err(_) => {
                tracing::error!("WebView load_progress mutex poisoned");
                0
            }
        }
    }

    /// Define o zoom
    pub fn set_zoom(&self, zoom: f64) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            let percent = (zoom * 100.0) as i32;
            let script = format!("document.body.style.zoom = '{}%'", percent);
            webview
                .evaluate_script(&script)
                .map_err(|e| BrowserError::ScriptExecutionFailed(e.to_string()))?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Abre DevTools
    pub fn open_devtools(&self) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            webview.open_devtools();
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Fecha DevTools
    pub fn close_devtools(&self) -> BrowserResult<()> {
        self.ensure_ready()?;

        if let Some(ref webview) = self.webview {
            webview.close_devtools();
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
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
        *Self::lock(&self.state, "state")? = WebViewState::Destroyed;

        // Limpa o handler para evitar callbacks pós-destruição
        *Self::lock(&self.event_handler, "event_handler")? = None;

        // O WebView será destruído quando sair de escopo
        self.webview = None;

        tracing::info!("WebView {} destroyed", self.handle);
        Ok(())
    }

    /// Verifica se está pronto
    fn ensure_ready(&self) -> BrowserResult<()> {
        match *Self::lock(&self.state, "state")? {
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
    fn emit_event(&self, event: BrowserEvent) {
        if let Ok(mut handler) = self.event_handler.lock() {
            if let Some(ref mut h) = *handler {
                h.on_event(event);
            }
        }
    }

    /// Obtém a URL inicial
    fn get_home_url(&self) -> String {
        match &self.config.homepage {
            super::HomepageConfig::Url(url) => url.clone(),
            super::HomepageConfig::Blank => "about:blank".to_string(),
            _ => "about:blank".to_string(),
        }
    }

    /// Tira screenshot da página
    pub fn screenshot(&self) -> BrowserResult<Vec<u8>> {
        self.ensure_ready()?;

        // O wry não tem API direta para screenshot
        // Podemos usar JavaScript para capturar via canvas
        let _script = r#"
            (async () => {
                const canvas = document.createElement('canvas');
                canvas.width = window.innerWidth;
                canvas.height = window.innerHeight;
                const ctx = canvas.getContext('2d');
                
                // Desenha a página no canvas
                // Note: Isso não captura conteúdo cross-origin
                try {
                    const dataUrl = await html2canvas(document.body).toDataURL('image/png');
                    return dataUrl;
                } catch (e) {
                    return 'error: ' + e.message;
                }
            })()
        "#;

        // Por enquanto, retorna um erro indicando que precisa de implementação adicional
        // Uma implementação completa usaria uma crate como screenshots ou similar
        Err(BrowserError::NotImplemented(
            "Screenshot requer implementação adicional com html2canvas ou API nativa".to_string(),
        ))
    }

    /// Retorna o HTML da página
    ///
    /// Nota: A API wry não suporta capturar o retorno de scripts JavaScript.
    /// Este método retorna um erro indicando que a funcionalidade não está disponível.
    pub fn get_html(&self) -> BrowserResult<String> {
        self.ensure_ready()?;

        // A API wry não permite capturar o retorno de evaluate_script
        // Isso retornaria o HTML se a API suportasse
        Err(BrowserError::PlatformNotSupported(
            "get_html() não é suportado pela API wry atual".to_string(),
        ))
    }

    /// Define o user agent
    pub fn set_user_agent(&self, _user_agent: &str) -> BrowserResult<()> {
        self.ensure_ready()?;

        // O wry não permite mudar user agent após criação
        // Isso precisaria ser feito no builder
        tracing::warn!("User agent cannot be changed after initialization in wry");
        Ok(())
    }

    /// Limpa cache e cookies
    pub fn clear_data(&self) -> BrowserResult<()> {
        self.ensure_ready()?;

        self.execute_javascript(
            r#"
            (() => {
                // Limpa localStorage
                localStorage.clear();
                // Limpa sessionStorage
                sessionStorage.clear();
                // Limpa cookies
                document.cookie.split(';').forEach(cookie => {
                    const [name] = cookie.split('=');
                    document.cookie = name + '=;expires=Thu, 01 Jan 1970 00:00:00 GMT';
                });
                return 'cleared';
            })()
        "#,
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wry_webview_creation() {
        // Note: Não podemos testar inicialização real sem uma janela winit
        let webview = WryWebView::new(WebViewHandle(1), BrowserConfig::default());

        assert!(matches!(webview.state(), WebViewState::Uninitialized));
        assert_eq!(webview.handle().0, 1);
    }

    #[test]
    fn test_webview_not_initialized() {
        let webview = WryWebView::new(WebViewHandle(1), BrowserConfig::default());

        let result = webview.execute_javascript("1+1");

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BrowserError::WebViewNotInitialized
        ));
    }
}
