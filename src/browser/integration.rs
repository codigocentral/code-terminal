//! Browser Integration - Integração do browser com painéis

use super::{
    BrowserConfig, BrowserError, BrowserId, BrowserInstance, BrowserManager, BrowserResult,
};
use crate::multiplexer::pane::{Pane, PaneContent, PaneId};
use url::Url;

/// Integração de browser com painel
#[derive(Debug)]
pub struct BrowserPane {
    /// ID do painel
    pub pane_id: PaneId,
    /// ID do browser
    pub browser_id: BrowserId,
    /// Configuração
    pub config: BrowserConfig,
    /// URL atual
    pub current_url: Option<Url>,
}

impl BrowserPane {
    /// Cria uma nova integração browser-painel
    pub fn new(pane_id: PaneId, browser_id: BrowserId, config: BrowserConfig) -> Self {
        Self {
            pane_id,
            browser_id,
            config,
            current_url: None,
        }
    }

    /// Navega para uma URL
    pub fn navigate(&mut self, url: Url) {
        self.current_url = Some(url);
    }

    /// Retorna a URL atual
    pub fn current_url(&self) -> Option<&Url> {
        self.current_url.as_ref()
    }
}

/// Integração do browser com o sistema de painéis
#[derive(Debug)]
pub struct BrowserIntegration {
    /// Gerenciador de browsers
    pub browser_manager: BrowserManager,
    /// Mapeamento pane_id -> browser_id
    pane_browsers: std::collections::HashMap<PaneId, BrowserId>,
    /// Mapeamento browser_id -> pane_id
    browser_panes: std::collections::HashMap<BrowserId, PaneId>,
}

impl Default for BrowserIntegration {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserIntegration {
    /// Cria uma nova integração
    pub fn new() -> Self {
        Self {
            browser_manager: BrowserManager::new(),
            pane_browsers: std::collections::HashMap::new(),
            browser_panes: std::collections::HashMap::new(),
        }
    }

    /// Define a configuração padrão
    pub fn with_default_config(mut self, config: BrowserConfig) -> Self {
        self.browser_manager = BrowserManager::new().with_default_config(config);
        self
    }

    /// Cria um browser em um painel
    pub fn create_browser_in_pane(
        &mut self,
        pane_id: PaneId,
        config: Option<BrowserConfig>,
        initial_url: Option<&str>,
    ) -> BrowserResult<BrowserId> {
        // Verifica se o painel já tem um browser
        if self.pane_browsers.contains_key(&pane_id) {
            return Err(BrowserError::IntegrationError(format!(
                "Pane {} already has a browser",
                pane_id
            )));
        }

        // Cria o browser
        let browser_id = self.browser_manager.create_browser(config)?;

        // Registra o mapeamento
        self.pane_browsers.insert(pane_id, browser_id);
        self.browser_panes.insert(browser_id, pane_id);

        // Navega para URL inicial se fornecida; se falhar, desfaz a criação
        // para não deixar o painel "ocupado" por um browser que o chamador
        // nunca recebeu
        if let Some(url) = initial_url {
            if let Err(error) = self.browser_manager.navigate(browser_id, url) {
                self.pane_browsers.remove(&pane_id);
                self.browser_panes.remove(&browser_id);
                let _ = self.browser_manager.destroy_browser(browser_id);
                return Err(error);
            }
        }

        Ok(browser_id)
    }

    /// Obtém o browser de um painel
    pub fn get_browser_for_pane(&self, pane_id: PaneId) -> Option<BrowserInstance> {
        let browser_id = self.pane_browsers.get(&pane_id)?;
        self.browser_manager.get_browser(*browser_id)
    }

    /// Obtém o painel de um browser
    pub fn get_pane_for_browser(&self, browser_id: BrowserId) -> Option<PaneId> {
        self.browser_panes.get(&browser_id).copied()
    }

    /// Navega em um painel
    pub fn navigate_in_pane(&self, pane_id: PaneId, url: impl AsRef<str>) -> BrowserResult<()> {
        let browser_id = self.pane_browsers.get(&pane_id).ok_or_else(|| {
            BrowserError::IntegrationError(format!("No browser in pane {}", pane_id))
        })?;

        self.browser_manager.navigate(*browser_id, url)
    }

    /// Volta no histórico de um painel
    pub fn go_back_in_pane(&self, pane_id: PaneId) -> BrowserResult<Option<Url>> {
        let browser_id = self.pane_browsers.get(&pane_id).ok_or_else(|| {
            BrowserError::IntegrationError(format!("No browser in pane {}", pane_id))
        })?;

        self.browser_manager.go_back(*browser_id)
    }

    /// Avança no histórico de um painel
    pub fn go_forward_in_pane(&self, pane_id: PaneId) -> BrowserResult<Option<Url>> {
        let browser_id = self.pane_browsers.get(&pane_id).ok_or_else(|| {
            BrowserError::IntegrationError(format!("No browser in pane {}", pane_id))
        })?;

        self.browser_manager.go_forward(*browser_id)
    }

    /// Recarrega um painel
    pub fn reload_pane(&self, pane_id: PaneId) -> BrowserResult<()> {
        let browser_id = self.pane_browsers.get(&pane_id).ok_or_else(|| {
            BrowserError::IntegrationError(format!("No browser in pane {}", pane_id))
        })?;

        self.browser_manager.reload(*browser_id)
    }

    /// Para o carregamento em um painel
    pub fn stop_pane(&self, pane_id: PaneId) -> BrowserResult<()> {
        let browser_id = self.pane_browsers.get(&pane_id).ok_or_else(|| {
            BrowserError::IntegrationError(format!("No browser in pane {}", pane_id))
        })?;

        self.browser_manager.stop(*browser_id)
    }

    /// Destrói o browser de um painel
    pub fn destroy_browser_in_pane(&mut self, pane_id: PaneId) -> BrowserResult<()> {
        let browser_id = self.pane_browsers.remove(&pane_id).ok_or_else(|| {
            BrowserError::IntegrationError(format!("No browser in pane {}", pane_id))
        })?;

        self.browser_panes.remove(&browser_id);
        self.browser_manager.destroy_browser(browser_id)
    }

    /// Lista todos os browsers em painéis
    pub fn list_pane_browsers(&self) -> Vec<(PaneId, BrowserId)> {
        self.pane_browsers.iter().map(|(p, b)| (*p, *b)).collect()
    }

    /// Verifica se um painel tem browser
    pub fn pane_has_browser(&self, pane_id: PaneId) -> bool {
        self.pane_browsers.contains_key(&pane_id)
    }

    /// Divide um painel e cria browser no novo
    pub fn split_pane_with_browser(
        &mut self,
        window: &mut crate::multiplexer::window::Window,
        source_pane_id: PaneId,
        vertical: bool,
        config: Option<BrowserConfig>,
        initial_url: Option<&str>,
    ) -> BrowserResult<PaneId> {
        // Verifica se o painel existe
        let _source_pane = window.get_pane(source_pane_id).ok_or_else(|| {
            BrowserError::IntegrationError(format!("Pane {} not found", source_pane_id))
        })?;

        // Cria novo painel dividindo o existente
        let new_pane_id = window
            .split_active_pane(vertical, Some(50))
            .map_err(|e| BrowserError::IntegrationError(e.to_string()))?;

        // Cria browser no novo painel
        self.create_browser_in_pane(new_pane_id, config, initial_url)?;

        Ok(new_pane_id)
    }

    /// Limpa browsers destruídos
    pub fn cleanup(&mut self) -> usize {
        let destroyed: Vec<_> = self
            .browser_panes
            .iter()
            .filter(|(browser_id, _)| {
                if let Some(instance) = self.browser_manager.get_browser(**browser_id) {
                    matches!(instance.state, super::BrowserInstanceState::Destroyed)
                } else {
                    true
                }
            })
            .map(|(browser_id, pane_id)| (*browser_id, *pane_id))
            .collect();

        let count = destroyed.len();

        for (browser_id, pane_id) in destroyed {
            self.browser_panes.remove(&browser_id);
            self.pane_browsers.remove(&pane_id);
        }

        count
    }
}

/// Extension trait para Pane com métodos de browser
pub trait PaneBrowser {
    /// Verifica se é um painel de browser
    fn is_browser_pane(&self) -> bool;

    /// Obtém a URL do browser
    fn browser_url(&self) -> Option<Url>;

    /// Define como painel de browser
    fn set_as_browser(&mut self, browser_id: BrowserId);
}

impl PaneBrowser for Pane {
    fn is_browser_pane(&self) -> bool {
        self.metadata
            .get("is_browser")
            .map(|value| value == "true")
            .unwrap_or(false)
            || self.metadata.contains_key("browser_id")
    }

    fn browser_url(&self) -> Option<Url> {
        self.metadata
            .get("browser_url")
            .and_then(|url| Url::parse(url).ok())
    }

    fn set_as_browser(&mut self, browser_id: BrowserId) {
        self.content_type = PaneContent::Command;
        self.metadata
            .insert("browser_id".to_string(), browser_id.to_string());
        self.metadata
            .insert("is_browser".to_string(), "true".to_string());
    }
}

/// Helper para criar janela com painéis de browser
pub struct BrowserWindowBuilder {
    url: Option<String>,
    config: Option<BrowserConfig>,
}

impl BrowserWindowBuilder {
    /// Cria um novo builder
    pub fn new() -> Self {
        Self {
            url: None,
            config: None,
        }
    }

    /// Define a URL
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Define a configuração
    pub fn with_config(mut self, config: BrowserConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Cria uma janela com browser
    pub fn build(
        self,
        window_manager: &mut crate::multiplexer::window::WindowManager,
        integration: &mut BrowserIntegration,
    ) -> BrowserResult<(crate::multiplexer::window::WindowId, PaneId, BrowserId)> {
        // Cria janela
        let window_id = window_manager.create_window(Some("Browser".to_string()), (800, 600));

        let window = window_manager
            .get_window_mut(window_id)
            .ok_or_else(|| BrowserError::IntegrationError("Failed to create window".to_string()))?;

        // Janela nova não tem painéis: cria o primeiro para receber o browser
        // (antes o build sempre falhava com "No active pane")
        let pane_id = match window.active_pane {
            Some(pane_id) => pane_id,
            None => window.add_pane(Pane::new(window.size, (0, 0))),
        };

        let browser_id =
            integration.create_browser_in_pane(pane_id, self.config, self.url.as_deref())?;

        Ok((window_id, pane_id, browser_id))
    }
}

impl Default for BrowserWindowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_PANE_ID_COUNTER: AtomicU64 = AtomicU64::new(1000);

    fn test_pane_id() -> PaneId {
        TEST_PANE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
    }

    #[test]
    fn test_browser_integration() {
        let mut integration = BrowserIntegration::new();

        let pane_id = test_pane_id();

        // Cria browser no painel
        let browser_id = integration
            .create_browser_in_pane(pane_id, None, Some("https://example.com"))
            .unwrap();

        // Verifica mapeamento
        assert!(integration.pane_has_browser(pane_id));
        assert_eq!(integration.get_pane_for_browser(browser_id), Some(pane_id));

        // Navega
        integration
            .navigate_in_pane(pane_id, "https://example.org")
            .unwrap();

        let instance = integration.get_browser_for_pane(pane_id).unwrap();
        assert!(instance.current_url.is_some());
    }

    #[test]
    fn test_browser_pane_duplicate() {
        let mut integration = BrowserIntegration::new();

        let pane_id = test_pane_id();

        // Cria primeiro browser
        integration
            .create_browser_in_pane(pane_id, None, None)
            .unwrap();

        // Tenta criar segundo (deve falhar)
        let result = integration.create_browser_in_pane(pane_id, None, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_browser_pane_navigation() {
        let mut integration = BrowserIntegration::new();

        let pane_id = test_pane_id();

        integration
            .create_browser_in_pane(pane_id, None, Some("https://first.com"))
            .unwrap();

        // Navega para segunda URL
        integration
            .navigate_in_pane(pane_id, "https://second.com")
            .unwrap();

        // Pode voltar
        let url = integration.go_back_in_pane(pane_id).unwrap();
        assert!(url.is_some());

        // Pode avançar
        let url = integration.go_forward_in_pane(pane_id).unwrap();
        assert!(url.is_some());
    }

    #[test]
    fn test_browser_cleanup() {
        let mut integration = BrowserIntegration::new();

        let pane_id = test_pane_id();
        let browser_id = integration
            .create_browser_in_pane(pane_id, None, None)
            .unwrap();

        // Destrói browser
        integration
            .browser_manager
            .destroy_browser(browser_id)
            .unwrap();

        // Limpa
        let cleaned = integration.cleanup();
        assert_eq!(cleaned, 1);
        assert!(!integration.pane_has_browser(pane_id));
    }

    #[test]
    fn test_pane_browser_trait() {
        let mut pane = Pane::new((80, 24), (0, 0));
        let browser_id = BrowserId::new();

        pane.set_as_browser(browser_id);

        assert!(pane.is_browser_pane());
        assert_eq!(
            pane.metadata.get("browser_id"),
            Some(&browser_id.to_string())
        );
    }

    use crate::multiplexer::window::WindowManager;

    /// Regressão: URL inicial inválida deixava o painel registrado com um
    /// browser que o chamador nunca recebeu.
    #[test]
    fn test_invalid_initial_url_rolls_back() {
        let mut integration = BrowserIntegration::default();
        let pane_id = test_pane_id();
        let err = integration
            .create_browser_in_pane(pane_id, None, Some("javascript:alert(1)"))
            .unwrap_err();
        assert!(matches!(err, BrowserError::InvalidUrl(_)), "{err}");
        assert!(!integration.pane_has_browser(pane_id));
        assert_eq!(integration.browser_manager.browser_count(), 0);

        // O painel continua utilizável
        integration
            .create_browser_in_pane(pane_id, None, Some("https://ok.test"))
            .unwrap();
        assert!(integration.pane_has_browser(pane_id));
    }

    #[test]
    fn test_pane_operations_without_browser_fail() {
        let mut integration = BrowserIntegration::new();
        let pane = test_pane_id();
        let is_integration = |e: BrowserError| matches!(e, BrowserError::IntegrationError(m) if m.contains("No browser"));
        assert!(is_integration(
            integration
                .navigate_in_pane(pane, "https://a.test")
                .unwrap_err()
        ));
        assert!(is_integration(
            integration.go_back_in_pane(pane).unwrap_err()
        ));
        assert!(is_integration(
            integration.go_forward_in_pane(pane).unwrap_err()
        ));
        assert!(is_integration(integration.reload_pane(pane).unwrap_err()));
        assert!(is_integration(integration.stop_pane(pane).unwrap_err()));
        assert!(is_integration(
            integration.destroy_browser_in_pane(pane).unwrap_err()
        ));
        assert!(integration.get_browser_for_pane(pane).is_none());
    }

    #[test]
    fn test_reload_stop_destroy_and_list() {
        let config = BrowserConfig::default().with_zoom(1.25);
        let mut integration = BrowserIntegration::new().with_default_config(config);
        let pane = test_pane_id();
        let browser = integration
            .create_browser_in_pane(pane, None, Some("https://a.test"))
            .unwrap();
        assert_eq!(
            integration
                .get_browser_for_pane(pane)
                .unwrap()
                .config
                .default_zoom,
            1.25
        );

        integration.reload_pane(pane).unwrap();
        assert_eq!(
            integration.get_browser_for_pane(pane).unwrap().state,
            super::super::BrowserInstanceState::Loading
        );
        integration.stop_pane(pane).unwrap();
        assert_eq!(
            integration.get_browser_for_pane(pane).unwrap().state,
            super::super::BrowserInstanceState::Ready
        );
        assert_eq!(integration.list_pane_browsers(), vec![(pane, browser)]);

        integration.destroy_browser_in_pane(pane).unwrap();
        assert!(integration.list_pane_browsers().is_empty());
        assert!(integration.get_pane_for_browser(browser).is_none());
        assert_eq!(integration.cleanup(), 0);
    }

    #[test]
    fn test_split_pane_with_browser() {
        let mut manager = WindowManager::new();
        let window_id = manager.create_window(Some("w".into()), (80, 24));
        let window = manager.get_window_mut(window_id).unwrap();
        let mut integration = BrowserIntegration::new();

        // Painel de origem inexistente
        let err = integration
            .split_pane_with_browser(window, 424242, true, None, None)
            .unwrap_err();
        assert!(err.to_string().contains("not found"), "{err}");

        let source = window.add_pane(Pane::new((80, 24), (0, 0)));
        let new_pane = integration
            .split_pane_with_browser(window, source, true, None, Some("https://a.test"))
            .unwrap();
        assert_ne!(new_pane, source);
        assert_eq!(window.pane_count(), 2);
        assert!(integration.pane_has_browser(new_pane));
        assert!(!integration.pane_has_browser(source));
    }

    /// Regressão: janela recém-criada não tem painel e o build sempre falhava.
    #[test]
    fn test_window_builder_creates_pane_and_browser() {
        let mut manager = WindowManager::new();
        let mut integration = BrowserIntegration::new();
        let (window_id, pane_id, browser_id) = BrowserWindowBuilder::default()
            .with_url("https://a.test/")
            .with_config(BrowserConfig::default())
            .build(&mut manager, &mut integration)
            .unwrap();

        let window = manager.get_window(window_id).unwrap();
        assert_eq!(window.active_pane, Some(pane_id));
        assert_eq!(integration.get_pane_for_browser(browser_id), Some(pane_id));
        let instance = integration.get_browser_for_pane(pane_id).unwrap();
        assert_eq!(instance.current_url.unwrap().as_str(), "https://a.test/");

        // Cada build cria uma janela nova, com seu próprio painel
        let mut second = BrowserIntegration::new();
        let (other_window, other_pane, _) = BrowserWindowBuilder::new()
            .build(&mut manager, &mut second)
            .unwrap();
        assert_ne!(other_window, window_id);
        assert_ne!(other_pane, pane_id);
        assert!(second.pane_has_browser(other_pane));
    }

    #[test]
    fn test_pane_browser_detection() {
        let mut pane = Pane::new((80, 24), (0, 0));
        assert!(!pane.is_browser_pane());
        assert!(pane.browser_url().is_none());

        pane.metadata
            .insert("browser_url".into(), "https://x.test/".into());
        assert_eq!(pane.browser_url().unwrap().as_str(), "https://x.test/");
        pane.metadata.insert("browser_url".into(), "lixo".into());
        assert!(pane.browser_url().is_none());

        pane.metadata.insert("is_browser".into(), "false".into());
        assert!(!pane.is_browser_pane());
        pane.metadata.insert("browser_id".into(), "b".into());
        assert!(pane.is_browser_pane());

        let mut bp = BrowserPane::new(1, BrowserId::new(), BrowserConfig::default());
        assert!(bp.current_url().is_none());
        bp.navigate(Url::parse("https://p.test/").unwrap());
        assert_eq!(bp.current_url().unwrap().as_str(), "https://p.test/");
    }
}
