//! Built-in Browser - Navegador integrado ao terminal
//!
//! Este módulo implementa um navegador web embutido usando WebView,
//! permitindo visualizar páginas web diretamente dentro do terminal.

pub mod address_bar;
pub mod automation;
pub mod config;
pub mod error;
pub mod events;
pub mod integration;
pub mod navigation;
pub mod remote;
pub mod webview;
pub mod webview_impl;
pub mod window_app;

#[cfg(target_os = "linux")]
pub mod platform_linux;
#[cfg(target_os = "macos")]
pub mod platform_macos;
#[cfg(target_os = "windows")]
pub mod platform_windows;

// Re-exporta a implementação real do WebView
pub use webview_impl::WryWebView;

pub use address_bar::{AddressBar, AddressBarState, UrlInput};
pub use automation::{AutomationBuilder, AutomationConfig, AutomationController, AutomationStats};
pub use config::{BrowserConfig, BrowserPreferences, HomepageConfig};
pub use error::{BrowserError, BrowserResult};
pub use events::{BrowserEvent, LoadEvent, NavigationEvent};
pub use integration::{BrowserIntegration, BrowserPane, PaneBrowser};
pub use navigation::{NavigationAction, NavigationController, NavigationState};
pub use webview::{BrowserWebView, WebViewHandle, WebViewState};

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use url::Url;

fn validate_browser_navigation_url(url: Url) -> BrowserResult<Url> {
    match url.scheme() {
        "http" | "https" | "about" => Ok(url),
        scheme => Err(BrowserError::InvalidUrl(format!(
            "Unsupported browser URL scheme: {}",
            scheme
        ))),
    }
}

/// ID único de uma instância do browser
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BrowserId(pub uuid::Uuid);

impl BrowserId {
    /// Cria um novo ID de browser
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for BrowserId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for BrowserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "browser-{}",
            self.0.to_string().split('-').next().unwrap_or("")
        )
    }
}

/// Estado de uma instância do browser
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserInstanceState {
    /// Criando
    Creating,
    /// Carregando
    Loading,
    /// Pronto
    Ready,
    /// Navegando
    Navigating,
    /// Em erro
    Error,
    /// Destruído
    Destroyed,
}

impl std::fmt::Display for BrowserInstanceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserInstanceState::Creating => write!(f, "creating"),
            BrowserInstanceState::Loading => write!(f, "loading"),
            BrowserInstanceState::Ready => write!(f, "ready"),
            BrowserInstanceState::Navigating => write!(f, "navigating"),
            BrowserInstanceState::Error => write!(f, "error"),
            BrowserInstanceState::Destroyed => write!(f, "destroyed"),
        }
    }
}

/// Informações de uma instância do browser
#[derive(Debug, Clone)]
pub struct BrowserInstance {
    /// ID único
    pub id: BrowserId,
    /// Estado atual
    pub state: BrowserInstanceState,
    /// URL atual
    pub current_url: Option<Url>,
    /// Título da página
    pub title: Option<String>,
    /// Favicon
    pub favicon: Option<Vec<u8>>,
    /// Progresso de carregamento (0-100)
    pub load_progress: u8,
    /// Configuração
    pub config: BrowserConfig,
    /// Histórico de navegação
    pub navigation: NavigationState,
    /// Metadados
    pub metadata: HashMap<String, String>,
    /// Quando foi criado
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

impl BrowserInstance {
    /// Cria uma nova instância do browser
    pub fn new(config: BrowserConfig) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: BrowserId::new(),
            state: BrowserInstanceState::Creating,
            current_url: None,
            title: None,
            favicon: None,
            load_progress: 0,
            config,
            navigation: NavigationState::new(),
            metadata: HashMap::new(),
            created_at: now,
            last_activity: now,
        }
    }

    /// Navega para uma URL
    pub fn navigate_to(&mut self, url: Url) {
        self.current_url = Some(url.clone());
        self.navigation.push(url);
        self.state = BrowserInstanceState::Navigating;
        self.touch();
    }

    /// Atualiza o título
    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
        self.touch();
    }

    /// Atualiza o progresso
    pub fn set_progress(&mut self, progress: u8) {
        self.load_progress = progress.clamp(0, 100);
        if progress >= 100 {
            self.state = BrowserInstanceState::Ready;
        }
        self.touch();
    }

    /// Define erro
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.state = BrowserInstanceState::Error;
        self.metadata.insert("error".to_string(), error.into());
        self.touch();
    }

    /// Retorna se pode voltar
    pub fn can_go_back(&self) -> bool {
        self.navigation.can_go_back()
    }

    /// Retorna se pode avançar
    pub fn can_go_forward(&self) -> bool {
        self.navigation.can_go_forward()
    }

    /// Volta no histórico
    pub fn go_back(&mut self) -> Option<Url> {
        let url = self.navigation.go_back()?;
        self.current_url = Some(url.clone());
        self.state = BrowserInstanceState::Navigating;
        self.touch();
        Some(url)
    }

    /// Avança no histórico
    pub fn go_forward(&mut self) -> Option<Url> {
        let url = self.navigation.go_forward()?;
        self.current_url = Some(url.clone());
        self.state = BrowserInstanceState::Navigating;
        self.touch();
        Some(url)
    }

    /// Recarrega
    pub fn reload(&mut self) {
        self.state = BrowserInstanceState::Loading;
        self.load_progress = 0;
        self.touch();
    }

    /// Para o carregamento
    pub fn stop(&mut self) {
        self.state = BrowserInstanceState::Ready;
        self.touch();
    }

    /// Atualiza a última atividade
    fn touch(&mut self) {
        self.last_activity = chrono::Utc::now();
    }
}

/// Gerenciador de instâncias do browser
#[derive(Debug)]
pub struct BrowserManager {
    /// Instâncias ativas
    instances: Arc<RwLock<HashMap<BrowserId, BrowserInstance>>>,
    /// Configuração padrão
    default_config: BrowserConfig,
    /// Contador de instâncias
    instance_count: Arc<RwLock<usize>>,
}

impl Default for BrowserManager {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            instances: Arc::new(RwLock::new(HashMap::new())),
            default_config: BrowserConfig::default(),
            instance_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Define a configuração padrão
    pub fn with_default_config(mut self, config: BrowserConfig) -> Self {
        self.default_config = config;
        self
    }

    /// Cria uma nova instância do browser
    pub fn create_browser(&self, config: Option<BrowserConfig>) -> BrowserResult<BrowserId> {
        let config = config.unwrap_or_else(|| self.default_config.clone());
        let instance = BrowserInstance::new(config);
        let id = instance.id;

        self.instances.write().insert(id, instance);
        *self.instance_count.write() += 1;

        Ok(id)
    }

    /// Obtém uma instância
    pub fn get_browser(&self, id: BrowserId) -> Option<BrowserInstance> {
        self.instances.read().get(&id).cloned()
    }

    /// Obtém uma instância mutável
    pub fn get_browser_mut(
        &self,
        id: BrowserId,
    ) -> Option<parking_lot::MappedRwLockWriteGuard<'_, BrowserInstance>> {
        parking_lot::lock_api::RwLockWriteGuard::try_map(self.instances.write(), |instances| {
            instances.get_mut(&id)
        })
        .ok()
    }

    /// Remove uma instância
    pub fn destroy_browser(&self, id: BrowserId) -> BrowserResult<()> {
        let mut instance = self
            .instances
            .write()
            .remove(&id)
            .ok_or(BrowserError::InstanceNotFound(id.to_string()))?;

        instance.state = BrowserInstanceState::Destroyed;
        *self.instance_count.write() -= 1;

        Ok(())
    }

    /// Lista todas as instâncias
    pub fn list_browsers(&self) -> Vec<BrowserInstance> {
        self.instances.read().values().cloned().collect()
    }

    /// Lista instâncias ativas
    pub fn list_active(&self) -> Vec<BrowserInstance> {
        self.instances
            .read()
            .values()
            .filter(|b| !matches!(b.state, BrowserInstanceState::Destroyed))
            .cloned()
            .collect()
    }

    /// Retorna o número de instâncias
    pub fn browser_count(&self) -> usize {
        *self.instance_count.read()
    }

    /// Navega para uma URL
    pub fn navigate(&self, id: BrowserId, url: impl AsRef<str>) -> BrowserResult<()> {
        let url = Url::parse(url.as_ref())
            .map_err(|e| BrowserError::InvalidUrl(e.to_string()))
            .and_then(validate_browser_navigation_url)?;

        let mut instances = self.instances.write();
        let instance = instances
            .get_mut(&id)
            .ok_or(BrowserError::InstanceNotFound(id.to_string()))?;

        instance.navigate_to(url);
        Ok(())
    }

    /// Volta no histórico
    pub fn go_back(&self, id: BrowserId) -> BrowserResult<Option<Url>> {
        let mut instances = self.instances.write();
        let instance = instances
            .get_mut(&id)
            .ok_or(BrowserError::InstanceNotFound(id.to_string()))?;

        Ok(instance.go_back())
    }

    /// Avança no histórico
    pub fn go_forward(&self, id: BrowserId) -> BrowserResult<Option<Url>> {
        let mut instances = self.instances.write();
        let instance = instances
            .get_mut(&id)
            .ok_or(BrowserError::InstanceNotFound(id.to_string()))?;

        Ok(instance.go_forward())
    }

    /// Recarrega
    pub fn reload(&self, id: BrowserId) -> BrowserResult<()> {
        let mut instances = self.instances.write();
        let instance = instances
            .get_mut(&id)
            .ok_or(BrowserError::InstanceNotFound(id.to_string()))?;

        instance.reload();
        Ok(())
    }

    /// Para o carregamento
    pub fn stop(&self, id: BrowserId) -> BrowserResult<()> {
        let mut instances = self.instances.write();
        let instance = instances
            .get_mut(&id)
            .ok_or(BrowserError::InstanceNotFound(id.to_string()))?;

        instance.stop();
        Ok(())
    }

    /// Limpa instâncias destruídas
    pub fn cleanup(&self) -> usize {
        let mut instances = self.instances.write();
        let to_remove: Vec<_> = instances
            .iter()
            .filter(|(_, b)| matches!(b.state, BrowserInstanceState::Destroyed))
            .map(|(id, _)| *id)
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            instances.remove(&id);
        }

        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_id_creation() {
        let id1 = BrowserId::new();
        let id2 = BrowserId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_browser_instance_creation() {
        let instance = BrowserInstance::new(BrowserConfig::default());
        assert!(matches!(instance.state, BrowserInstanceState::Creating));
        assert_eq!(instance.load_progress, 0);
    }

    #[test]
    fn test_browser_manager() {
        let manager = BrowserManager::new();
        let id = manager.create_browser(None).unwrap();

        assert_eq!(manager.browser_count(), 1);

        let instance = manager.get_browser(id).unwrap();
        assert!(matches!(instance.state, BrowserInstanceState::Creating));

        manager.destroy_browser(id).unwrap();
        assert_eq!(manager.browser_count(), 0);
    }

    #[test]
    fn test_navigation() {
        let manager = BrowserManager::new();
        let id = manager.create_browser(None).unwrap();

        manager.navigate(id, "https://example.com").unwrap();

        let instance = manager.get_browser(id).unwrap();
        assert!(instance.current_url.is_some());
        assert!(!instance.can_go_back());
        assert_eq!(instance.navigation.len(), 1);
    }

    #[test]
    fn test_navigation_rejects_unsupported_schemes() {
        let manager = BrowserManager::new();
        let id = manager.create_browser(None).unwrap();

        for url in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///C:/Windows/win.ini",
        ] {
            let result = manager.navigate(id, url);
            assert!(matches!(result, Err(BrowserError::InvalidUrl(_))));
        }

        let instance = manager.get_browser(id).unwrap();
        assert!(instance.current_url.is_none());
        assert_eq!(instance.navigation.len(), 0);
    }

    #[test]
    fn test_navigation_allows_about_blank() {
        let manager = BrowserManager::new();
        let id = manager.create_browser(None).unwrap();

        manager.navigate(id, "about:blank").unwrap();

        let instance = manager.get_browser(id).unwrap();
        assert_eq!(instance.current_url.unwrap().as_str(), "about:blank");
    }

    #[test]
    fn test_go_back_forward() {
        let manager = BrowserManager::new();
        let id = manager.create_browser(None).unwrap();

        manager.navigate(id, "https://example.com").unwrap();
        manager.navigate(id, "https://example.org").unwrap();

        // Pode voltar
        assert!(manager.get_browser(id).unwrap().can_go_back());

        // Volta
        let url = manager.go_back(id).unwrap();
        assert!(url.is_some());

        // Agora pode avançar
        assert!(manager.get_browser(id).unwrap().can_go_forward());
    }

    #[test]
    fn test_browser_id_display_and_default() {
        let id = BrowserId::default();
        let shown = id.to_string();
        let first_group = id.0.to_string().split('-').next().unwrap().to_string();
        assert_eq!(shown, format!("browser-{}", first_group));
    }

    #[test]
    fn test_instance_state_display() {
        let cases = [
            (BrowserInstanceState::Creating, "creating"),
            (BrowserInstanceState::Loading, "loading"),
            (BrowserInstanceState::Ready, "ready"),
            (BrowserInstanceState::Navigating, "navigating"),
            (BrowserInstanceState::Error, "error"),
            (BrowserInstanceState::Destroyed, "destroyed"),
        ];
        for (state, text) in cases {
            assert_eq!(state.to_string(), text);
        }
    }

    #[test]
    fn test_instance_lifecycle_methods() {
        let mut instance = BrowserInstance::new(BrowserConfig::default());
        let created = instance.last_activity;

        instance.set_title("Página");
        assert_eq!(instance.title.as_deref(), Some("Página"));
        assert!(instance.last_activity >= created);

        instance.set_progress(40);
        assert_eq!(instance.load_progress, 40);
        assert_eq!(instance.state, BrowserInstanceState::Creating);
        instance.set_progress(250);
        assert_eq!(instance.load_progress, 100);
        assert_eq!(instance.state, BrowserInstanceState::Ready);

        instance.set_error("falhou");
        assert_eq!(instance.state, BrowserInstanceState::Error);
        assert_eq!(
            instance.metadata.get("error").map(String::as_str),
            Some("falhou")
        );

        // Sem histórico, não há para onde ir
        assert!(instance.go_back().is_none());
        assert!(instance.go_forward().is_none());

        instance.navigate_to(Url::parse("https://a.test/").unwrap());
        instance.navigate_to(Url::parse("https://b.test/").unwrap());
        assert_eq!(instance.go_back().unwrap().as_str(), "https://a.test/");
        assert_eq!(instance.state, BrowserInstanceState::Navigating);
        assert_eq!(instance.go_forward().unwrap().as_str(), "https://b.test/");
        assert_eq!(
            instance.current_url.as_ref().unwrap().as_str(),
            "https://b.test/"
        );

        instance.reload();
        assert_eq!(instance.state, BrowserInstanceState::Loading);
        assert_eq!(instance.load_progress, 0);

        instance.stop();
        assert_eq!(instance.state, BrowserInstanceState::Ready);
    }

    #[test]
    fn test_manager_default_config_listing_and_cleanup() {
        let config = BrowserConfig::default().with_zoom(1.5);
        let manager = BrowserManager::default().with_default_config(config);
        let a = manager.create_browser(None).unwrap();
        let b = manager
            .create_browser(Some(BrowserConfig::default().with_zoom(2.0)))
            .unwrap();

        assert_eq!(manager.get_browser(a).unwrap().config.default_zoom, 1.5);
        assert_eq!(manager.get_browser(b).unwrap().config.default_zoom, 2.0);
        assert_eq!(manager.list_browsers().len(), 2);
        assert_eq!(manager.list_active().len(), 2);

        // Instância marcada como destruída sai de list_active e é limpa
        manager.get_browser_mut(b).unwrap().state = BrowserInstanceState::Destroyed;
        let active: Vec<_> = manager.list_active().into_iter().map(|i| i.id).collect();
        assert_eq!(active, vec![a]);
        assert_eq!(manager.cleanup(), 1);
        assert!(manager.get_browser(b).is_none());
        assert_eq!(manager.cleanup(), 0);

        assert!(manager.get_browser_mut(BrowserId::new()).is_none());
    }

    #[test]
    fn test_manager_operations_on_missing_browser() {
        let manager = BrowserManager::new();
        let missing = BrowserId::new();
        let not_found = |r: BrowserResult<()>| matches!(r, Err(BrowserError::InstanceNotFound(_)));

        assert!(not_found(manager.destroy_browser(missing)));
        assert!(not_found(manager.navigate(missing, "https://a.test")));
        assert!(not_found(manager.go_back(missing).map(|_| ())));
        assert!(not_found(manager.go_forward(missing).map(|_| ())));
        assert!(not_found(manager.reload(missing)));
        assert!(not_found(manager.stop(missing)));

        // URL que nem faz parse
        let id = manager.create_browser(None).unwrap();
        assert!(matches!(
            manager.navigate(id, "não é url"),
            Err(BrowserError::InvalidUrl(_))
        ));
    }

    #[test]
    fn test_manager_reload_stop_and_forward() {
        let manager = BrowserManager::new();
        let id = manager.create_browser(None).unwrap();
        manager.navigate(id, "https://a.test/").unwrap();
        manager.navigate(id, "https://b.test/").unwrap();
        manager.go_back(id).unwrap();
        let forward = manager.go_forward(id).unwrap().unwrap();
        assert_eq!(forward.as_str(), "https://b.test/");

        manager.reload(id).unwrap();
        assert_eq!(
            manager.get_browser(id).unwrap().state,
            BrowserInstanceState::Loading
        );
        manager.stop(id).unwrap();
        assert_eq!(
            manager.get_browser(id).unwrap().state,
            BrowserInstanceState::Ready
        );
    }

    #[test]
    fn test_browser_error_conversions() {
        let io = std::io::Error::other("disco cheio");
        let err: BrowserError = io.into();
        assert!(matches!(err, BrowserError::Io(ref m) if m.contains("disco cheio")));

        let parse = Url::parse("::").unwrap_err();
        let err: BrowserError = parse.into();
        assert!(matches!(err, BrowserError::InvalidUrl(_)));
    }
}
