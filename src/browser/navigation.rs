//! Browser Navigation - Sistema de navegação do browser

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use url::Url;

/// Estado da navegação
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationState {
    /// Histórico de URLs
    history: VecDeque<Url>,
    /// Posição atual no histórico
    current_index: usize,
    /// Tamanho máximo do histórico
    max_size: usize,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self::new()
    }
}

impl NavigationState {
    /// Cria um novo estado de navegação
    pub fn new() -> Self {
        Self::with_capacity(50)
    }

    /// Cria com capacidade específica
    pub fn with_capacity(max_size: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(max_size),
            current_index: 0,
            max_size,
        }
    }

    /// Adiciona uma URL ao histórico
    pub fn push(&mut self, url: Url) {
        // Remove entradas à frente se não estamos no final
        while self.history.len() > self.current_index + 1 {
            self.history.pop_back();
        }

        // Adiciona nova URL
        self.history.push_back(url);
        self.current_index = self.history.len() - 1;

        // Mantém dentro do limite
        if self.history.len() > self.max_size {
            self.history.pop_front();
            self.current_index = self.current_index.saturating_sub(1);
        }
    }

    /// Volta no histórico
    pub fn go_back(&mut self) -> Option<Url> {
        if self.can_go_back() {
            self.current_index -= 1;
            self.history.get(self.current_index).cloned()
        } else {
            None
        }
    }

    /// Avança no histórico
    pub fn go_forward(&mut self) -> Option<Url> {
        if self.can_go_forward() {
            self.current_index += 1;
            self.history.get(self.current_index).cloned()
        } else {
            None
        }
    }

    /// Retorna se pode voltar
    pub fn can_go_back(&self) -> bool {
        self.current_index > 0
    }

    /// Retorna se pode avançar
    pub fn can_go_forward(&self) -> bool {
        self.current_index < self.history.len().saturating_sub(1)
    }

    /// Retorna a URL atual
    pub fn current(&self) -> Option<Url> {
        self.history.get(self.current_index).cloned()
    }

    /// Retorna o histórico completo
    pub fn history(&self) -> &VecDeque<Url> {
        &self.history
    }

    /// Limpa o histórico
    pub fn clear(&mut self) {
        self.history.clear();
        self.current_index = 0;
    }

    /// Retorna o tamanho do histórico
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Retorna se está vazio
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Define o tamanho máximo
    pub fn set_max_size(&mut self, max_size: usize) {
        self.max_size = max_size;

        // Remove entradas excedentes
        while self.history.len() > self.max_size {
            self.history.pop_front();
            self.current_index = self.current_index.saturating_sub(1);
        }
    }
}

/// Controlador de navegação
#[derive(Debug)]
pub struct NavigationController {
    state: NavigationState,
}

impl Default for NavigationController {
    fn default() -> Self {
        Self::new()
    }
}

impl NavigationController {
    /// Cria um novo controlador
    pub fn new() -> Self {
        Self {
            state: NavigationState::new(),
        }
    }

    /// Navega para uma URL
    pub fn navigate(&mut self, url: Url) {
        self.state.push(url);
    }

    /// Volta
    pub fn back(&mut self) -> Option<Url> {
        self.state.go_back()
    }

    /// Avança
    pub fn forward(&mut self) -> Option<Url> {
        self.state.go_forward()
    }

    /// Retorna o estado
    pub fn state(&self) -> &NavigationState {
        &self.state
    }

    /// Retorna se pode voltar
    pub fn can_go_back(&self) -> bool {
        self.state.can_go_back()
    }

    /// Retorna se pode avançar
    pub fn can_go_forward(&self) -> bool {
        self.state.can_go_forward()
    }
}

/// Ação de navegação
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationAction {
    /// Navegar para URL
    Navigate(String),
    /// Voltar
    Back,
    /// Avançar
    Forward,
    /// Recarregar
    Reload,
    /// Parar
    Stop,
    /// Home
    Home,
}

impl NavigationAction {
    /// Parse de string
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        let input = trimmed.to_lowercase();

        match input.as_str() {
            "back" | "voltar" | "<" => Some(NavigationAction::Back),
            "forward" | "avançar" | "avancar" | ">" => Some(NavigationAction::Forward),
            "reload" | "refresh" | "recarregar" | "r" => Some(NavigationAction::Reload),
            "stop" | "parar" => Some(NavigationAction::Stop),
            "home" | "inicio" | "início" => Some(NavigationAction::Home),
            // Comandos ignoram caixa, mas a URL é preservada: caminho e query
            // diferenciam maiúsculas
            url if url.starts_with("http://") || url.starts_with("https://") => {
                Some(NavigationAction::Navigate(trimmed.to_string()))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation_state() {
        let mut state = NavigationState::new();

        let url1 = Url::parse("https://example.com").unwrap();
        let url2 = Url::parse("https://example.org").unwrap();
        let url3 = Url::parse("https://example.net").unwrap();

        state.push(url1.clone());
        state.push(url2.clone());
        state.push(url3.clone());

        assert_eq!(state.current(), Some(url3));
        assert!(state.can_go_back());
        assert!(!state.can_go_forward());

        // Volta
        let back = state.go_back();
        assert_eq!(back, Some(url2));
        assert!(state.can_go_forward());

        // Volta novamente
        let back = state.go_back();
        assert_eq!(back, Some(url1));
        assert!(!state.can_go_back());
    }

    #[test]
    fn test_navigation_forward_after_back() {
        let mut state = NavigationState::new();

        let url1 = Url::parse("https://example.com").unwrap();
        let url2 = Url::parse("https://example.org").unwrap();
        let url3 = Url::parse("https://example.net").unwrap();

        state.push(url1.clone());
        state.push(url2.clone());
        state.push(url3.clone());

        // Volta duas vezes
        state.go_back();
        state.go_back();

        // Avança
        let forward = state.go_forward();
        assert_eq!(forward, Some(url2.clone()));

        // Avança novamente
        let forward = state.go_forward();
        assert_eq!(forward, Some(url3));
    }

    #[test]
    fn test_new_navigation_clears_forward() {
        let mut state = NavigationState::new();

        let url1 = Url::parse("https://example.com").unwrap();
        let url2 = Url::parse("https://example.org").unwrap();
        let url3 = Url::parse("https://example.net").unwrap();

        state.push(url1.clone());
        state.push(url2.clone());

        // Volta
        state.go_back();
        assert!(state.can_go_forward());

        // Nova navegação
        state.push(url3.clone());

        // Não pode mais avançar para url2
        assert!(!state.can_go_forward());
        assert_eq!(state.current(), Some(url3));
    }

    #[test]
    fn test_navigation_action_parse() {
        assert_eq!(
            NavigationAction::parse("back"),
            Some(NavigationAction::Back)
        );
        assert_eq!(
            NavigationAction::parse("forward"),
            Some(NavigationAction::Forward)
        );
        assert_eq!(
            NavigationAction::parse("reload"),
            Some(NavigationAction::Reload)
        );
        assert_eq!(
            NavigationAction::parse("stop"),
            Some(NavigationAction::Stop)
        );
        assert_eq!(
            NavigationAction::parse("home"),
            Some(NavigationAction::Home)
        );

        assert_eq!(
            NavigationAction::parse("https://example.com"),
            Some(NavigationAction::Navigate(
                "https://example.com".to_string()
            ))
        );
    }

    #[test]
    fn test_max_size() {
        let mut state = NavigationState::with_capacity(3);

        state.push(Url::parse("https://1.com").unwrap());
        state.push(Url::parse("https://2.com").unwrap());
        state.push(Url::parse("https://3.com").unwrap());
        state.push(Url::parse("https://4.com").unwrap());

        // Deve manter apenas as últimas 3
        assert_eq!(state.len(), 3);
        assert_eq!(state.current().unwrap().as_str(), "https://4.com/");
    }

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    /// Regressão: a URL era rebaixada para minúsculas junto com os comandos.
    #[test]
    fn test_parse_preserves_url_case() {
        assert_eq!(
            NavigationAction::parse("  HTTPS://Example.com/Path?Q=A "),
            Some(NavigationAction::Navigate(
                "HTTPS://Example.com/Path?Q=A".to_string()
            ))
        );
        for (input, action) in [
            ("Voltar", NavigationAction::Back),
            (">", NavigationAction::Forward),
            ("avançar", NavigationAction::Forward),
            ("REFRESH", NavigationAction::Reload),
            ("parar", NavigationAction::Stop),
            ("início", NavigationAction::Home),
        ] {
            assert_eq!(NavigationAction::parse(input), Some(action), "{input}");
        }
        assert_eq!(NavigationAction::parse("ftp://x"), None);
    }

    #[test]
    fn test_history_truncates_forward_entries_and_limits_size() {
        let mut state = NavigationState::with_capacity(3);
        assert!(state.is_empty());
        assert!(state.current().is_none());
        assert!(state.go_back().is_none());
        assert!(state.go_forward().is_none());

        for page in ["a", "b", "c"] {
            state.push(url(&format!("https://{page}.test/")));
        }
        assert_eq!(state.go_back().unwrap(), url("https://b.test/"));
        // Navegar depois de voltar descarta o "c"
        state.push(url("https://d.test/"));
        assert!(!state.can_go_forward());
        let hosts: Vec<_> = state
            .history()
            .iter()
            .map(|u| u.host_str().unwrap().to_string())
            .collect();
        assert_eq!(hosts, vec!["a.test", "b.test", "d.test"]);

        // Excede a capacidade: o mais antigo sai
        state.push(url("https://e.test/"));
        assert_eq!(state.len(), 3);
        assert_eq!(state.history()[0], url("https://b.test/"));
        assert_eq!(state.current().unwrap(), url("https://e.test/"));

        state.set_max_size(1);
        assert_eq!(state.len(), 1);
        assert_eq!(state.current().unwrap(), url("https://e.test/"));

        state.clear();
        assert!(state.is_empty());
        assert!(!state.can_go_back());
    }

    #[test]
    fn test_controller_delegates() {
        let mut controller = NavigationController::default();
        controller.navigate(url("https://a.test/"));
        controller.navigate(url("https://b.test/"));
        assert!(controller.can_go_back());
        assert_eq!(controller.back().unwrap(), url("https://a.test/"));
        assert!(controller.can_go_forward());
        assert_eq!(controller.forward().unwrap(), url("https://b.test/"));
        assert_eq!(controller.state().len(), 2);
        assert_eq!(NavigationState::default().len(), 0);
    }
}
