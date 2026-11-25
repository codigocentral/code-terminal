//! Wait API - API de Espera
//!
//! Este módulo fornece funcionalidades para esperar por condições
//! específicas antes de prosseguir com a automação.
//!
//! # Exemplo
//!
//! ```rust,no_run
//! use code_terminal::browser::automation::wait::{WaitController, WaitCondition, WaitOptions};
//! use code_terminal::browser::BrowserId;
//!
//! async fn wait_example(browser_id: BrowserId) {
//!     let controller = WaitController::new(browser_id);
//!     
//!     // Espera elemento ficar visível
//!     controller.wait_for(WaitCondition::ElementVisible {
//!         selector: "#modal".to_string()
//!     }, WaitOptions::default()).await.unwrap();
//!     
//!     // Espera função retornar true
//!     controller.wait_for(WaitCondition::FunctionTrue {
//!         script: "document.readyState === 'complete'".to_string()
//!     }, WaitOptions::default()).await.unwrap();
//! }
//! ```

use crate::browser::{BrowserError, BrowserId, BrowserResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::{sleep, Instant};

/// Controlador de esperas
#[derive(Debug, Clone)]
pub struct WaitController {
    browser_id: BrowserId,
}

impl WaitController {
    /// Cria um novo controlador
    pub fn new(browser_id: BrowserId) -> Self {
        Self { browser_id }
    }

    /// Retorna o browser associado.
    pub fn browser_id(&self) -> BrowserId {
        self.browser_id
    }

    /// Espera por uma condição
    pub async fn wait_for(
        &self,
        condition: WaitCondition,
        options: WaitOptions,
    ) -> BrowserResult<WaitResult> {
        let start = Instant::now();
        let timeout = Duration::from_millis(options.timeout_ms);
        let poll_interval = Duration::from_millis(options.poll_interval_ms);

        tracing::debug!("Waiting for condition: {:?}", condition);

        loop {
            // Verifica timeout
            if start.elapsed() >= timeout {
                return Ok(WaitResult {
                    success: false,
                    condition_met: false,
                    elapsed_ms: start.elapsed().as_millis() as u64,
                    timed_out: true,
                    error: Some("Timeout waiting for condition".to_string()),
                });
            }

            // Verifica condição
            match self.check_condition(&condition).await {
                Ok(true) => {
                    return Ok(WaitResult {
                        success: true,
                        condition_met: true,
                        elapsed_ms: start.elapsed().as_millis() as u64,
                        timed_out: false,
                        error: None,
                    });
                }
                Ok(false) => {
                    // Continua esperando
                }
                Err(e) => {
                    if options.stop_on_error {
                        return Err(e);
                    }
                    tracing::warn!("Error checking condition: {}", e);
                }
            }

            // Espera antes de verificar novamente
            sleep(poll_interval).await;
        }
    }

    /// Espera por múltiplas condições (all)
    pub async fn wait_for_all(
        &self,
        conditions: Vec<WaitCondition>,
        options: WaitOptions,
    ) -> BrowserResult<Vec<WaitResult>> {
        let mut results = Vec::new();

        for condition in conditions {
            let result = self.wait_for(condition, options).await?;
            results.push(result);
        }

        Ok(results)
    }

    /// Espera por qualquer condição (any)
    pub async fn wait_for_any(
        &self,
        conditions: Vec<WaitCondition>,
        options: WaitOptions,
    ) -> BrowserResult<(usize, WaitResult)> {
        // Na implementação real, usaria select! para esperar em paralelo
        // Por enquanto, implementação sequencial
        for (i, condition) in conditions.into_iter().enumerate() {
            let result = self.wait_for(condition, options).await?;
            if result.success {
                return Ok((i, result));
            }
        }

        Err(BrowserError::JavaScriptFailed(
            "No condition met".to_string(),
        ))
    }

    /// Espera por navegação completar
    pub async fn wait_for_navigation(&self, options: WaitOptions) -> BrowserResult<WaitResult> {
        self.wait_for(WaitCondition::NavigationComplete, options)
            .await
    }

    /// Espera por elemento ficar visível
    pub async fn wait_for_element_visible(
        &self,
        selector: impl AsRef<str>,
        options: WaitOptions,
    ) -> BrowserResult<WaitResult> {
        self.wait_for(
            WaitCondition::ElementVisible {
                selector: selector.as_ref().to_string(),
            },
            options,
        )
        .await
    }

    /// Espera por elemento desaparecer
    pub async fn wait_for_element_hidden(
        &self,
        selector: impl AsRef<str>,
        options: WaitOptions,
    ) -> BrowserResult<WaitResult> {
        self.wait_for(
            WaitCondition::ElementHidden {
                selector: selector.as_ref().to_string(),
            },
            options,
        )
        .await
    }

    /// Espera por texto aparecer
    pub async fn wait_for_text(
        &self,
        text: impl AsRef<str>,
        options: WaitOptions,
    ) -> BrowserResult<WaitResult> {
        self.wait_for(
            WaitCondition::TextPresent {
                text: text.as_ref().to_string(),
            },
            options,
        )
        .await
    }

    /// Espera por função retornar true
    pub async fn wait_for_function(
        &self,
        script: impl AsRef<str>,
        options: WaitOptions,
    ) -> BrowserResult<WaitResult> {
        self.wait_for(
            WaitCondition::FunctionTrue {
                script: script.as_ref().to_string(),
            },
            options,
        )
        .await
    }

    /// Verifica uma condição
    async fn check_condition(&self, condition: &WaitCondition) -> BrowserResult<bool> {
        match condition {
            WaitCondition::ElementVisible { selector } => {
                // Na implementação real, executaria JS no WebView
                tracing::trace!("Checking visibility of '{}'", selector);
                Ok(true) // Simulação
            }
            WaitCondition::ElementHidden { selector } => {
                tracing::trace!("Checking hidden state of '{}'", selector);
                Ok(false) // Simulação
            }
            WaitCondition::ElementPresent { selector } => {
                tracing::trace!("Checking presence of '{}'", selector);
                Ok(true) // Simulação
            }
            WaitCondition::ElementNotPresent { selector } => {
                tracing::trace!("Checking absence of '{}'", selector);
                Ok(false) // Simulação
            }
            WaitCondition::TextPresent { text } => {
                tracing::trace!("Checking for text '{}'", text);
                Ok(true) // Simulação
            }
            WaitCondition::TextNotPresent { text } => {
                tracing::trace!("Checking absence of text '{}'", text);
                Ok(false) // Simulação
            }
            WaitCondition::FunctionTrue { script } => {
                tracing::trace!("Checking function: {}", script);
                Ok(true) // Simulação
            }
            WaitCondition::NavigationComplete => {
                tracing::trace!("Checking navigation complete");
                Ok(true) // Simulação
            }
            WaitCondition::NetworkIdle => {
                tracing::trace!("Checking network idle");
                Ok(true) // Simulação
            }
            WaitCondition::Custom { name, check } => {
                tracing::trace!("Checking custom condition: {}", name);
                Ok(check()) // Simulação
            }
        }
    }
}

/// Condição de espera
pub enum WaitCondition {
    /// Elemento está visível
    ElementVisible { selector: String },
    /// Elemento está oculto
    ElementHidden { selector: String },
    /// Elemento existe no DOM
    ElementPresent { selector: String },
    /// Elemento não existe no DOM
    ElementNotPresent { selector: String },
    /// Texto está presente na página
    TextPresent { text: String },
    /// Texto não está presente na página
    TextNotPresent { text: String },
    /// Função retorna true
    FunctionTrue { script: String },
    /// Navegação completou
    NavigationComplete,
    /// Rede está inativa
    NetworkIdle,
    /// Condição customizada
    Custom {
        name: String,
        check: Box<dyn Fn() -> bool + Send>,
    },
}

impl std::fmt::Debug for WaitCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaitCondition::ElementVisible { selector } => write!(f, "ElementVisible({})", selector),
            WaitCondition::ElementHidden { selector } => write!(f, "ElementHidden({})", selector),
            WaitCondition::ElementPresent { selector } => write!(f, "ElementPresent({})", selector),
            WaitCondition::ElementNotPresent { selector } => {
                write!(f, "ElementNotPresent({})", selector)
            }
            WaitCondition::TextPresent { text } => write!(f, "TextPresent({})", text),
            WaitCondition::TextNotPresent { text } => write!(f, "TextNotPresent({})", text),
            WaitCondition::FunctionTrue { script } => write!(f, "FunctionTrue({})", script),
            WaitCondition::NavigationComplete => write!(f, "NavigationComplete"),
            WaitCondition::NetworkIdle => write!(f, "NetworkIdle"),
            WaitCondition::Custom { name, .. } => write!(f, "Custom({})", name),
        }
    }
}

impl Clone for WaitCondition {
    fn clone(&self) -> Self {
        match self {
            WaitCondition::ElementVisible { selector } => WaitCondition::ElementVisible {
                selector: selector.clone(),
            },
            WaitCondition::ElementHidden { selector } => WaitCondition::ElementHidden {
                selector: selector.clone(),
            },
            WaitCondition::ElementPresent { selector } => WaitCondition::ElementPresent {
                selector: selector.clone(),
            },
            WaitCondition::ElementNotPresent { selector } => WaitCondition::ElementNotPresent {
                selector: selector.clone(),
            },
            WaitCondition::TextPresent { text } => {
                WaitCondition::TextPresent { text: text.clone() }
            }
            WaitCondition::TextNotPresent { text } => {
                WaitCondition::TextNotPresent { text: text.clone() }
            }
            WaitCondition::FunctionTrue { script } => WaitCondition::FunctionTrue {
                script: script.clone(),
            },
            WaitCondition::NavigationComplete => WaitCondition::NavigationComplete,
            WaitCondition::NetworkIdle => WaitCondition::NetworkIdle,
            WaitCondition::Custom { name, .. } => WaitCondition::Custom {
                name: name.clone(),
                check: Box::new(|| false),
            },
        }
    }
}

impl WaitCondition {
    /// Cria condição de elemento visível
    pub fn element_visible(selector: impl Into<String>) -> Self {
        WaitCondition::ElementVisible {
            selector: selector.into(),
        }
    }

    /// Cria condição de elemento oculto
    pub fn element_hidden(selector: impl Into<String>) -> Self {
        WaitCondition::ElementHidden {
            selector: selector.into(),
        }
    }

    /// Cria condição de elemento presente
    pub fn element_present(selector: impl Into<String>) -> Self {
        WaitCondition::ElementPresent {
            selector: selector.into(),
        }
    }

    /// Cria condição de elemento ausente
    pub fn element_not_present(selector: impl Into<String>) -> Self {
        WaitCondition::ElementNotPresent {
            selector: selector.into(),
        }
    }

    /// Cria condição de texto presente
    pub fn text_present(text: impl Into<String>) -> Self {
        WaitCondition::TextPresent { text: text.into() }
    }

    /// Cria condição de função verdadeira
    pub fn function_true(script: impl Into<String>) -> Self {
        WaitCondition::FunctionTrue {
            script: script.into(),
        }
    }

    /// Cria condição de navegação completa
    pub fn navigation_complete() -> Self {
        WaitCondition::NavigationComplete
    }

    /// Cria condição de rede inativa
    pub fn network_idle() -> Self {
        WaitCondition::NetworkIdle
    }

    /// Retorna descrição da condição
    pub fn description(&self) -> String {
        match self {
            WaitCondition::ElementVisible { selector } => format!("element '{}' visible", selector),
            WaitCondition::ElementHidden { selector } => format!("element '{}' hidden", selector),
            WaitCondition::ElementPresent { selector } => format!("element '{}' present", selector),
            WaitCondition::ElementNotPresent { selector } => {
                format!("element '{}' not present", selector)
            }
            WaitCondition::TextPresent { text } => format!("text '{}' present", text),
            WaitCondition::TextNotPresent { text } => format!("text '{}' not present", text),
            WaitCondition::FunctionTrue { script } => format!(
                "function true: {}",
                script.chars().take(50).collect::<String>()
            ),
            WaitCondition::NavigationComplete => "navigation complete".to_string(),
            WaitCondition::NetworkIdle => "network idle".to_string(),
            WaitCondition::Custom { name, .. } => format!("custom: {}", name),
        }
    }
}

/// Opções de espera
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WaitOptions {
    /// Timeout em milissegundos
    pub timeout_ms: u64,
    /// Intervalo de polling em milissegundos
    pub poll_interval_ms: u64,
    /// Se deve parar em erro
    pub stop_on_error: bool,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            timeout_ms: 30000,
            poll_interval_ms: 100,
            stop_on_error: false,
        }
    }
}

impl WaitOptions {
    /// Cria opções com timeout específico
    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout_ms: timeout.as_millis() as u64,
            ..Default::default()
        }
    }

    /// Define timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout_ms = timeout.as_millis() as u64;
        self
    }

    /// Define intervalo de polling
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval_ms = interval.as_millis() as u64;
        self
    }

    /// Habilita parada em erro
    pub fn stop_on_error(mut self) -> Self {
        self.stop_on_error = true;
        self
    }
}

/// Resultado de espera
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitResult {
    /// Se foi bem-sucedido
    pub success: bool,
    /// Se a condição foi atingida
    pub condition_met: bool,
    /// Tempo decorrido em milissegundos
    pub elapsed_ms: u64,
    /// Se houve timeout
    pub timed_out: bool,
    /// Erro (se houver)
    pub error: Option<String>,
}

impl WaitResult {
    /// Retorna se a espera foi bem-sucedida
    pub fn is_success(&self) -> bool {
        self.success && self.condition_met
    }

    /// Retorna se houve timeout
    pub fn is_timeout(&self) -> bool {
        self.timed_out
    }

    /// Retorna tempo em milissegundos
    pub fn elapsed_ms(&self) -> u64 {
        self.elapsed_ms
    }
}

/// Utilitários de espera
pub struct WaitUtils;

impl WaitUtils {
    /// Espera um tempo fixo
    pub async fn sleep(duration: Duration) {
        sleep(duration).await;
    }

    /// Espera um número de segundos
    pub async fn sleep_secs(secs: u64) {
        sleep(Duration::from_secs(secs)).await;
    }

    /// Espera um número de milissegundos
    pub async fn sleep_ms(ms: u64) {
        sleep(Duration::from_millis(ms)).await;
    }

    /// Espera até condição ser verdadeira (blocking)
    pub async fn wait_until<F>(mut condition: F, timeout: Duration, poll_interval: Duration) -> bool
    where
        F: FnMut() -> bool,
    {
        let start = Instant::now();

        while start.elapsed() < timeout {
            if condition() {
                return true;
            }
            sleep(poll_interval).await;
        }

        false
    }

    /// Espera com retry exponencial
    pub async fn exponential_backoff<F, Fut, T>(
        mut operation: F,
        max_attempts: u32,
        initial_delay: Duration,
    ) -> Option<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Option<T>>,
    {
        let mut delay = initial_delay;

        for _ in 0..max_attempts {
            if let Some(result) = operation().await {
                return Some(result);
            }
            sleep(delay).await;
            delay *= 2;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wait_options_default() {
        let opts = WaitOptions::default();
        assert_eq!(opts.timeout_ms, 30000);
        assert_eq!(opts.poll_interval_ms, 100);
        assert!(!opts.stop_on_error);
    }

    #[test]
    fn test_wait_options_builder() {
        let opts = WaitOptions::default()
            .timeout(Duration::from_secs(60))
            .poll_interval(Duration::from_millis(500))
            .stop_on_error();

        assert_eq!(opts.timeout_ms, 60000);
        assert_eq!(opts.poll_interval_ms, 500);
        assert!(opts.stop_on_error);
    }

    #[test]
    fn test_wait_condition_variants() {
        let visible = WaitCondition::element_visible("#test");
        let hidden = WaitCondition::element_hidden("#test");
        let present = WaitCondition::element_present("#test");
        let text = WaitCondition::text_present("Hello");
        let func = WaitCondition::function_true("true");
        let nav = WaitCondition::navigation_complete();

        assert!(matches!(visible, WaitCondition::ElementVisible { .. }));
        assert!(matches!(hidden, WaitCondition::ElementHidden { .. }));
        assert!(matches!(present, WaitCondition::ElementPresent { .. }));
        assert!(matches!(text, WaitCondition::TextPresent { .. }));
        assert!(matches!(func, WaitCondition::FunctionTrue { .. }));
        assert!(matches!(nav, WaitCondition::NavigationComplete));
    }

    #[test]
    fn test_wait_condition_description() {
        let visible = WaitCondition::element_visible("#button");
        assert_eq!(visible.description(), "element '#button' visible");

        let text = WaitCondition::text_present("Loading");
        assert_eq!(text.description(), "text 'Loading' present");

        let nav = WaitCondition::navigation_complete();
        assert_eq!(nav.description(), "navigation complete");
    }

    #[test]
    fn test_wait_result() {
        let success = WaitResult {
            success: true,
            condition_met: true,
            elapsed_ms: 500,
            timed_out: false,
            error: None,
        };

        assert!(success.is_success());
        assert!(!success.is_timeout());
        assert_eq!(success.elapsed_ms(), 500);

        let timeout = WaitResult {
            success: false,
            condition_met: false,
            elapsed_ms: 30000,
            timed_out: true,
            error: Some("Timeout".to_string()),
        };

        assert!(!timeout.is_success());
        assert!(timeout.is_timeout());
    }

    #[test]
    fn test_wait_condition_with_timeout() {
        let opts = WaitOptions::with_timeout(Duration::from_secs(10));
        assert_eq!(opts.timeout_ms, 10000);
    }

    #[tokio::test]
    async fn test_wait_utils_sleep() {
        let start = Instant::now();
        WaitUtils::sleep_ms(50).await;
        assert!(start.elapsed() >= Duration::from_millis(50));
    }

    #[tokio::test]
    async fn test_wait_until() {
        let mut counter = 0;
        let result = WaitUtils::wait_until(
            || {
                counter += 1;
                counter >= 3
            },
            Duration::from_secs(1),
            Duration::from_millis(10),
        )
        .await;

        assert!(result);
        assert_eq!(counter, 3);
    }

    fn short() -> WaitOptions {
        WaitOptions::with_timeout(Duration::from_millis(30)).poll_interval(Duration::from_millis(5))
    }

    fn all_conditions() -> Vec<(WaitCondition, bool)> {
        vec![
            (WaitCondition::element_visible("#v"), true),
            (WaitCondition::element_hidden("#h"), false),
            (WaitCondition::element_present("#p"), true),
            (WaitCondition::element_not_present("#np"), false),
            (WaitCondition::text_present("ola"), true),
            (
                WaitCondition::TextNotPresent {
                    text: "erro".into(),
                },
                false,
            ),
            (WaitCondition::function_true("ready()"), true),
            (WaitCondition::navigation_complete(), true),
            (WaitCondition::network_idle(), true),
            (
                WaitCondition::Custom {
                    name: "sempre".into(),
                    check: Box::new(|| true),
                },
                true,
            ),
            (
                WaitCondition::Custom {
                    name: "nunca".into(),
                    check: Box::new(|| false),
                },
                false,
            ),
        ]
    }

    #[tokio::test]
    async fn test_wait_for_every_condition() {
        let browser_id = BrowserId::new();
        let controller = WaitController::new(browser_id);
        assert_eq!(controller.browser_id(), browser_id);

        for (condition, met) in all_conditions() {
            let label = condition.description();
            let result = controller.wait_for(condition, short()).await.unwrap();
            assert_eq!(result.is_success(), met, "{label}");
            assert_eq!(result.is_timeout(), !met, "{label}");
            if met {
                assert!(result.error.is_none());
            } else {
                assert_eq!(
                    result.error.as_deref(),
                    Some("Timeout waiting for condition")
                );
                assert!(result.elapsed_ms() >= 30, "{label}");
            }
        }
    }

    #[tokio::test]
    async fn test_wait_helpers() {
        let controller = WaitController::new(BrowserId::new());
        assert!(controller
            .wait_for_navigation(short())
            .await
            .unwrap()
            .is_success());
        assert!(controller
            .wait_for_element_visible("#a", short())
            .await
            .unwrap()
            .is_success());
        assert!(controller
            .wait_for_element_hidden("#a", short())
            .await
            .unwrap()
            .is_timeout());
        assert!(controller
            .wait_for_text("x", short())
            .await
            .unwrap()
            .is_success());
        assert!(controller
            .wait_for_function("true", short())
            .await
            .unwrap()
            .is_success());
    }

    #[tokio::test]
    async fn test_wait_for_all_and_any() {
        let controller = WaitController::new(BrowserId::new());
        let results = controller
            .wait_for_all(
                vec![
                    WaitCondition::element_visible("#a"),
                    WaitCondition::element_hidden("#b"),
                ],
                short(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].is_success());
        assert!(results[1].is_timeout());

        // Primeira condição expira, segunda é atingida
        let (index, result) = controller
            .wait_for_any(
                vec![
                    WaitCondition::element_hidden("#b"),
                    WaitCondition::text_present("ok"),
                ],
                short(),
            )
            .await
            .unwrap();
        assert_eq!(index, 1);
        assert!(result.is_success());

        let err = controller
            .wait_for_any(vec![WaitCondition::element_not_present("#c")], short())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("No condition met"), "{err}");
    }

    #[tokio::test]
    async fn test_zero_timeout_expires_immediately() {
        let controller = WaitController::new(BrowserId::new());
        let options = WaitOptions {
            timeout_ms: 0,
            ..WaitOptions::default()
        };
        let result = controller
            .wait_for(WaitCondition::element_visible("#a"), options)
            .await
            .unwrap();
        assert!(result.is_timeout());
    }

    #[test]
    fn test_debug_clone_and_description_for_all_variants() {
        let expected = [
            ("ElementVisible(#v)", "element '#v' visible"),
            ("ElementHidden(#h)", "element '#h' hidden"),
            ("ElementPresent(#p)", "element '#p' present"),
            ("ElementNotPresent(#np)", "element '#np' not present"),
            ("TextPresent(ola)", "text 'ola' present"),
            ("TextNotPresent(erro)", "text 'erro' not present"),
            ("FunctionTrue(ready())", "function true: ready()"),
            ("NavigationComplete", "navigation complete"),
            ("NetworkIdle", "network idle"),
            ("Custom(sempre)", "custom: sempre"),
            ("Custom(nunca)", "custom: nunca"),
        ];
        for ((condition, _), (debug, description)) in all_conditions().into_iter().zip(expected) {
            let cloned = condition.clone();
            assert_eq!(format!("{:?}", condition), debug);
            assert_eq!(format!("{:?}", cloned), debug);
            assert_eq!(cloned.description(), description);
        }

        // O clone de Custom não preserva o closure: devolve sempre false
        let custom = WaitCondition::Custom {
            name: "x".into(),
            check: Box::new(|| true),
        };
        match custom.clone() {
            WaitCondition::Custom { check, .. } => assert!(!check()),
            other => panic!("variante inesperada: {other:?}"),
        }

        // Script longo é truncado na descrição
        let long = WaitCondition::function_true("a".repeat(80));
        assert_eq!(
            long.description(),
            format!("function true: {}", "a".repeat(50))
        );
    }

    #[tokio::test]
    async fn test_wait_utils_sleeps_and_retry() {
        let start = Instant::now();
        WaitUtils::sleep(Duration::from_millis(5)).await;
        WaitUtils::sleep_secs(0).await;
        assert!(start.elapsed() >= Duration::from_millis(5));

        // wait_until que nunca é satisfeito
        assert!(
            !WaitUtils::wait_until(
                || false,
                Duration::from_millis(20),
                Duration::from_millis(5)
            )
            .await
        );

        // Backoff: sucesso na terceira tentativa
        let mut attempts = 0;
        let result = WaitUtils::exponential_backoff(
            || {
                attempts += 1;
                let n = attempts;
                async move { (n >= 3).then_some(n) }
            },
            5,
            Duration::from_millis(1),
        )
        .await;
        assert_eq!(result, Some(3));

        // Esgota as tentativas
        let mut calls = 0;
        let result: Option<u32> = WaitUtils::exponential_backoff(
            || {
                calls += 1;
                async { None }
            },
            3,
            Duration::from_millis(1),
        )
        .await;
        assert!(result.is_none());
        assert_eq!(calls, 3);
    }

    #[test]
    fn test_wait_result_serde_roundtrip() {
        let result = WaitResult {
            success: true,
            condition_met: true,
            elapsed_ms: 7,
            timed_out: false,
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: WaitResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.elapsed_ms, 7);
        assert!(back.is_success());
    }
}
