//! Wry Platform - Implementação cross-platform de WebView usando wry
//!
//! Esta implementação usa a crate `wry` que fornece WebView nativo
//! em todas as plataformas (Windows/WebView2, macOS/WKWebView, Linux/WebKitGTK)

use crate::browser::events::{
    ConsoleEvent, ConsoleLevel, LoadEvent, LoadEventType, NavigationEvent, NavigationEventType,
};
use crate::browser::{
    BrowserConfig, BrowserError, BrowserEvent, BrowserResult, WebViewHandle, WebViewState,
};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use url::Url;
use wry::{http::Request, Rect, WebView, WebViewBuilder};

fn is_allowed_navigation_url(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "about")
}

fn parse_allowed_navigation_url(raw: &str) -> BrowserResult<Url> {
    let url = Url::parse(raw)?;
    if is_allowed_navigation_url(&url) {
        Ok(url)
    } else {
        Err(BrowserError::InvalidUrl(format!(
            "Unsupported browser URL scheme: {}",
            url.scheme()
        )))
    }
}

fn send_wry_event(sender: &Sender<WryEvent>, event: WryEvent) {
    if let Err(err) = sender.send(event) {
        tracing::warn!("Failed to send Wry event: {}", err);
    }
}

fn log_wry_command_result<T, E: std::fmt::Display>(action: &str, result: Result<T, E>) {
    if let Err(err) = result {
        tracing::warn!("Wry command '{}' failed: {}", action, err);
    }
}

/// Comandos enviados para a thread do WebView
#[derive(Debug, Clone)]
pub enum WryCommand {
    Navigate(String),
    GoBack,
    GoForward,
    Reload,
    Stop,
    ExecuteScript(String),
    SetZoom(f64),
    OpenDevTools,
    CloseDevTools,
    Destroy,
}

/// Eventos recebidos da thread do WebView
#[derive(Debug, Clone)]
pub enum WryEvent {
    NavigationStarted(String),
    NavigationFinished(String),
    LoadProgress(u8),
    TitleChanged(String),
    UrlChanged(String),
    ConsoleMessage(ConsoleLevel, String),
    Error(String),
}

/// Implementação do WebView usando wry
pub struct WryWebView {
    handle: WebViewHandle,
    state: Arc<Mutex<WebViewState>>,
    current_url: Arc<Mutex<Option<Url>>>,
    title: Arc<Mutex<Option<String>>>,
    load_progress: Arc<Mutex<u8>>,
    command_sender: Option<Sender<WryCommand>>,
    event_receiver: Option<Receiver<WryEvent>>,
}

impl WryWebView {
    /// Cria uma nova instância do WebView wry
    pub fn new(handle: WebViewHandle) -> Self {
        Self {
            handle,
            state: Arc::new(Mutex::new(WebViewState::Uninitialized)),
            current_url: Arc::new(Mutex::new(None)),
            title: Arc::new(Mutex::new(None)),
            load_progress: Arc::new(Mutex::new(0)),
            command_sender: None,
            event_receiver: None,
        }
    }

    /// Inicializa o WebView com uma janela pai
    ///
    /// # Arguments
    /// * `window` - Janela winit para embeddar o WebView
    /// * `config` - Configuração do browser
    pub fn initialize(
        &mut self,
        window: &winit::window::Window,
        config: &BrowserConfig,
    ) -> BrowserResult<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| BrowserError::WebViewCreationFailed("Lock poisoned".to_string()))?;

        if *state != WebViewState::Uninitialized {
            return Err(BrowserError::WebViewCreationFailed(
                "WebView already initialized".to_string(),
            ));
        }

        *state = WebViewState::Initializing;
        drop(state);

        // Cria canais de comunicação
        let (cmd_tx, cmd_rx) = channel::<WryCommand>();
        let (evt_tx, evt_rx) = channel::<WryEvent>();

        self.command_sender = Some(cmd_tx);
        self.event_receiver = Some(evt_rx);

        // Clona dados para a thread
        let state_clone = Arc::clone(&self.state);
        let url_clone = Arc::clone(&self.current_url);
        let title_clone = Arc::clone(&self.title);
        let progress_clone = Arc::clone(&self.load_progress);

        // Obtém o handle da janela
        #[cfg(target_os = "windows")]
        let parent = {
            use winit::platform::windows::WindowExtWindows;
            window.hwnd() as isize
        };

        #[cfg(target_os = "macos")]
        let parent = {
            use winit::platform::macos::WindowExtMacOS;
            window.ns_view() as isize
        };

        #[cfg(target_os = "linux")]
        let parent = {
            use winit::platform::wayland::WindowExtWayland;
            use winit::platform::x11::WindowExtX11;

            if let Some(wayland_surface) = window.wl_surface() {
                wayland_surface.as_ptr() as isize
            } else if let Some(xlib_window) = window.xlib_window() {
                xlib_window as isize
            } else {
                return Err(BrowserError::WebViewCreationFailed(
                    "Could not get window handle".to_string(),
                ));
            }
        };

        // Cria o WebView em uma thread separada para não bloquear
        let _ = crate::utils::spawn_named("webview", move || {
            let result = Self::run_webview_thread(
                parent,
                config.clone(),
                cmd_rx,
                evt_tx,
                state_clone,
                url_clone,
                title_clone,
                progress_clone,
            );

            if let Err(e) = result {
                tracing::error!("WebView thread error: {}", e);
            }
        });

        Ok(())
    }

    /// Thread principal do WebView
    fn run_webview_thread(
        parent: isize,
        config: BrowserConfig,
        command_receiver: Receiver<WryCommand>,
        event_sender: Sender<WryEvent>,
        state: Arc<Mutex<WebViewState>>,
        current_url: Arc<Mutex<Option<Url>>>,
        title: Arc<Mutex<Option<String>>>,
        load_progress: Arc<Mutex<u8>>,
    ) -> BrowserResult<()> {
        use wry::WebViewBuilder;

        // Cria o WebView builder
        let mut builder =
            WebViewBuilder::new().with_initialization_script(&Self::get_initialization_script());

        // Configura DevTools
        if config.devtools.enabled {
            builder = builder.with_devtools(true);
        }

        // Configura o handler de navegação
        let event_sender_nav = event_sender.clone();
        let state_nav = Arc::clone(&state);
        let url_nav = Arc::clone(&current_url);

        builder = builder.with_navigation_handler(move |url: String| -> bool {
            tracing::debug!("Navigation request: {}", url);

            let parsed_url = match parse_allowed_navigation_url(&url) {
                Ok(url) => url,
                Err(err) => {
                    tracing::warn!("Blocked navigation request '{}': {}", url, err);
                    send_wry_event(&event_sender_nav, WryEvent::Error(err.to_string()));
                    return false;
                }
            };

            // Atualiza estado
            if let Ok(mut s) = state_nav.lock() {
                *s = WebViewState::Loading;
            }

            // Atualiza URL
            if let Ok(mut u) = url_nav.lock() {
                *u = Some(parsed_url);
            }

            // Envia evento
            send_wry_event(&event_sender_nav, WryEvent::NavigationStarted(url.clone()));

            true // Permite navegação
        });

        // Configura handler de título
        let event_sender_title = event_sender.clone();
        let title_title = Arc::clone(&title);

        builder = builder.with_title_change_handler(move |title_text| {
            tracing::debug!("Title changed: {}", title_text);

            if let Ok(mut t) = title_title.lock() {
                *t = Some(title_text.clone());
            }

            send_wry_event(&event_sender_title, WryEvent::TitleChanged(title_text));
        });

        // Configura handler de load
        let event_sender_load = event_sender.clone();
        let state_load = Arc::clone(&state);
        let progress_load = Arc::clone(&load_progress);

        builder = builder.with_on_page_load_handler(move |event, url| match event {
            wry::PageLoadEvent::Started => {
                tracing::debug!("Page load started: {}", url);
                send_wry_event(&event_sender_load, WryEvent::NavigationStarted(url));
            }
            wry::PageLoadEvent::Finished => {
                tracing::debug!("Page load finished: {}", url);

                if let Ok(mut s) = state_load.lock() {
                    *s = WebViewState::Ready;
                }

                if let Ok(mut p) = progress_load.lock() {
                    *p = 100;
                }

                send_wry_event(&event_sender_load, WryEvent::NavigationFinished(url));
                send_wry_event(&event_sender_load, WryEvent::LoadProgress(100));
            }
        });

        // Cria o WebView
        let webview = builder
            .build()
            .map_err(|e| BrowserError::WebViewCreationFailed(e.to_string()))?;

        // Navega para homepage
        let homepage = match &config.homepage {
            super::HomepageConfig::Url(url) => url.clone(),
            _ => "about:blank".to_string(),
        };

        if let Err(err) = parse_allowed_navigation_url(&homepage).and_then(|_| {
            webview
                .load_url(&homepage)
                .map_err(|e| BrowserError::NavigationFailed(e.to_string()))
        }) {
            tracing::warn!("Failed to load browser homepage '{}': {}", homepage, err);
            send_wry_event(&event_sender, WryEvent::Error(err.to_string()));
        }

        // Atualiza estado
        if let Ok(mut s) = state.lock() {
            *s = WebViewState::Ready;
        }

        // Loop de eventos - processa comandos
        loop {
            match command_receiver.try_recv() {
                Ok(cmd) => {
                    match cmd {
                        WryCommand::Navigate(url) => {
                            let result = parse_allowed_navigation_url(&url).and_then(|_| {
                                webview
                                    .load_url(&url)
                                    .map_err(|e| BrowserError::NavigationFailed(e.to_string()))
                            });
                            log_wry_command_result("navigate", result);
                        }
                        WryCommand::GoBack => {
                            // wry não tem API direta para back/forward
                            // Precisamos implementar via JavaScript ou history API
                            log_wry_command_result(
                                "go_back",
                                webview.evaluate_script("history.back()"),
                            );
                        }
                        WryCommand::GoForward => {
                            log_wry_command_result(
                                "go_forward",
                                webview.evaluate_script("history.forward()"),
                            );
                        }
                        WryCommand::Reload => {
                            log_wry_command_result(
                                "reload",
                                webview.evaluate_script("location.reload()"),
                            );
                        }
                        WryCommand::Stop => {
                            log_wry_command_result(
                                "stop",
                                webview.evaluate_script("window.stop()"),
                            );
                        }
                        WryCommand::ExecuteScript(script) => {
                            log_wry_command_result(
                                "execute_script",
                                webview.evaluate_script(&script),
                            );
                        }
                        WryCommand::SetZoom(zoom) => {
                            let script = format!("document.body.style.zoom = '{}%'", zoom * 100.0);
                            log_wry_command_result("set_zoom", webview.evaluate_script(&script));
                        }
                        WryCommand::OpenDevTools => {
                            #[cfg(debug_assertions)]
                            webview.open_devtools();
                        }
                        WryCommand::CloseDevTools => {
                            #[cfg(debug_assertions)]
                            webview.close_devtools();
                        }
                        WryCommand::Destroy => {
                            break;
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Não há comandos, continua
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Canal fechado, sai
                    break;
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Limpa
        drop(webview);

        if let Ok(mut s) = state.lock() {
            *s = WebViewState::Destroyed;
        }

        Ok(())
    }

    /// Retorna o script de inicialização injetado em todas as páginas
    fn get_initialization_script() -> String {
        r#"
        // Code Terminal WebView API
        window.codeTerminal = {
            // Envia mensagem para o host
            postMessage: function(message) {
                if (window.ipc) {
                    window.ipc.postMessage(JSON.stringify(message));
                }
            },
            
            // Notifica que página está pronta
            notifyReady: function() {
                this.postMessage({ type: 'ready' });
            },
            
            // Notifica navegação
            notifyNavigation: function(url) {
                this.postMessage({ type: 'navigation', url: url });
            }
        };
        
        // Notifica quando DOM está pronto
        if (document.readyState === 'loading') {
            document.addEventListener('DOMContentLoaded', function() {
                window.codeTerminal.notifyReady();
            });
        } else {
            window.codeTerminal.notifyReady();
        }
        
        // Intercepta cliques em links
        document.addEventListener('click', function(e) {
            const link = e.target.closest('a');
            if (link && link.href) {
                window.codeTerminal.notifyNavigation(link.href);
            }
        });
        "#
        .to_string()
    }

    /// Navega para uma URL
    pub fn navigate(&self, url: &str) -> BrowserResult<()> {
        parse_allowed_navigation_url(url)?;

        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::Navigate(url.to_string()))
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Volta no histórico
    pub fn go_back(&self) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::GoBack)
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Avança no histórico
    pub fn go_forward(&self) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::GoForward)
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Recarrega
    pub fn reload(&self) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::Reload)
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Para o carregamento
    pub fn stop(&self) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::Stop)
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Executa JavaScript
    pub fn execute_script(&self, script: &str) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::ExecuteScript(script.to_string()))
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Define zoom
    pub fn set_zoom(&self, zoom: f64) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::SetZoom(zoom))
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Abre DevTools
    pub fn open_devtools(&self) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::OpenDevTools)
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Fecha DevTools
    pub fn close_devtools(&self) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            sender
                .send(WryCommand::CloseDevTools)
                .map_err(|_| BrowserError::WebViewNotInitialized)?;
            Ok(())
        } else {
            Err(BrowserError::WebViewNotInitialized)
        }
    }

    /// Retorna o estado atual
    pub fn state(&self) -> WebViewState {
        match self.state.lock() {
            Ok(state) => *state,
            Err(_) => {
                tracing::error!("WryWebView state mutex poisoned");
                WebViewState::Error
            }
        }
    }

    /// Retorna a URL atual
    pub fn current_url(&self) -> Option<Url> {
        match self.current_url.lock() {
            Ok(url) => url.clone(),
            Err(_) => {
                tracing::error!("WryWebView current_url mutex poisoned");
                None
            }
        }
    }

    /// Retorna o título
    pub fn title(&self) -> Option<String> {
        match self.title.lock() {
            Ok(title) => title.clone(),
            Err(_) => {
                tracing::error!("WryWebView title mutex poisoned");
                None
            }
        }
    }

    /// Retorna o progresso de carregamento
    pub fn load_progress(&self) -> u8 {
        match self.load_progress.lock() {
            Ok(progress) => *progress,
            Err(_) => {
                tracing::error!("WryWebView load_progress mutex poisoned");
                0
            }
        }
    }

    /// Processa eventos pendentes
    pub fn process_events(&mut self) -> Vec<WryEvent> {
        let mut events = Vec::new();

        if let Some(receiver) = &self.event_receiver {
            while let Ok(event) = receiver.try_recv() {
                // Atualiza estado interno baseado no evento
                match &event {
                    WryEvent::NavigationStarted(url) => {
                        match Url::parse(url) {
                            Ok(parsed_url) => {
                                if let Ok(mut u) = self.current_url.lock() {
                                    *u = Some(parsed_url);
                                }
                            }
                            Err(error) => {
                                tracing::warn!(url = %url, error = %error, "Invalid navigation URL event");
                            }
                        }
                        if let Ok(mut s) = self.state.lock() {
                            *s = WebViewState::Loading;
                        }
                    }
                    WryEvent::NavigationFinished(_) => {
                        if let Ok(mut s) = self.state.lock() {
                            *s = WebViewState::Ready;
                        }
                        if let Ok(mut p) = self.load_progress.lock() {
                            *p = 100;
                        }
                    }
                    WryEvent::LoadProgress(progress) => {
                        if let Ok(mut p) = self.load_progress.lock() {
                            *p = *progress;
                        }
                    }
                    WryEvent::TitleChanged(title) => {
                        if let Ok(mut t) = self.title.lock() {
                            *t = Some(title.clone());
                        }
                    }
                    _ => {}
                }

                events.push(event);
            }
        }

        events
    }

    /// Destrói o WebView
    pub fn destroy(&mut self) -> BrowserResult<()> {
        if let Some(sender) = &self.command_sender {
            if let Err(error) = sender.send(WryCommand::Destroy) {
                tracing::warn!(error = %error, "Failed to send Wry destroy command");
            }
        }

        self.command_sender = None;
        self.event_receiver = None;

        if let Ok(mut s) = self.state.lock() {
            *s = WebViewState::Destroyed;
        }

        Ok(())
    }
}

impl Drop for WryWebView {
    fn drop(&mut self) {
        if let Err(error) = self.destroy() {
            tracing::warn!(error = %error, "Failed to destroy WryWebView during drop");
        }
    }
}

/// Verifica se WebView está disponível na plataforma
pub fn is_webview_available() -> bool {
    // wry tenta usar o WebView nativo da plataforma
    // Se não estiver disponível, vai falhar na criação
    true
}

/// Retorna informações sobre o runtime do WebView
pub fn get_webview_info() -> HashMap<String, String> {
    let mut info = HashMap::new();

    #[cfg(target_os = "windows")]
    {
        info.insert("platform".to_string(), "Windows".to_string());
        info.insert("engine".to_string(), "WebView2".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        info.insert("platform".to_string(), "macOS".to_string());
        info.insert("engine".to_string(), "WKWebView".to_string());
    }

    #[cfg(target_os = "linux")]
    {
        info.insert("platform".to_string(), "Linux".to_string());
        info.insert("engine".to_string(), "WebKitGTK".to_string());
    }

    info.insert(
        "wry_version".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    );

    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webview_handle() {
        let handle = WebViewHandle(1);
        let webview = WryWebView::new(handle);

        assert_eq!(webview.state(), WebViewState::Uninitialized);
    }

    #[test]
    fn test_webview_info() {
        let info = get_webview_info();
        assert!(info.contains_key("platform"));
        assert!(info.contains_key("engine"));
    }

    #[test]
    fn test_is_webview_available() {
        assert!(is_webview_available());
    }
}
