//! Browser Automation API - API de Automação do Navegador
//!
//! Este módulo fornece uma API completa para automação de interações
//! com páginas web, permitindo que AI Agents controlem o navegador
//! de forma programática.
//!
//! # Features
//!
//! - **Accessibility Tree**: Extração da árvore de acessibilidade da página
//! - **Element Interaction**: Clique, preenchimento de formulários
//! - **JavaScript Execution**: Execução de código JS e extração de resultados
//! - **Screenshots**: Captura de tela da página inteira ou elementos específicos
//! - **Wait Conditions**: Espera por elementos, navegação ou condições JS
//!
//! # Example
//!
//! ```rust,no_run
//! use code_terminal::browser::automation::AutomationController;
//! use code_terminal::browser::BrowserId;
//!
//! async fn automate_login(browser_id: BrowserId) {
//!     let automation = AutomationController::new(browser_id);
//!     
//!     // Navega para a página
//!     automation.navigate("https://example.com/login").await.unwrap();
//!     
//!     // Preenche formulário
//!     automation.fill("#username", "user@example.com").await.unwrap();
//!     automation.fill("#password", "secret123").await.unwrap();
//!     
//!     // Clica no botão de login
//!     automation.click("button[type='submit']").await.unwrap();
//!     
//!     // Espera navegação completar
//!     automation.wait_for_navigation().await.unwrap();
//!     
//!     // Tira screenshot
//!     let screenshot = automation.screenshot_full().await.unwrap();
//! }
//! ```

pub mod accessibility;
pub mod click;
pub mod fill;
pub mod integration;
pub mod javascript;
pub mod screenshot;
pub mod selector;
pub mod wait;

pub use accessibility::{AccessibilityExtractor, AccessibilityNode, AccessibilityTree, NodeType};
pub use click::{ClickController, ClickOptions, ClickResult, MouseButton};
pub use fill::{FillController, FillOptions, FillResult, FormData};
pub use integration::{AutomationIntegration, AutomationSupport};
pub use javascript::{ExecutionContext, JavaScriptEngine, ScriptResult};
pub use screenshot::{ScreenshotController, ScreenshotFormat, ScreenshotOptions, ScreenshotResult};
pub use selector::{Selector, SelectorEngine, SelectorResult};
pub use wait::{WaitCondition, WaitController, WaitOptions, WaitResult};

use crate::browser::{BrowserError, BrowserId, BrowserResult};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::time::Duration;

/// Controlador principal de automação
#[derive(Debug, Clone)]
pub struct AutomationController {
    /// ID do browser associado
    browser_id: BrowserId,
    /// Estado compartilhado
    state: Arc<RwLock<AutomationState>>,
    /// Configuração
    config: AutomationConfig,
}

/// Estado da automação
#[derive(Debug, Clone, Default)]
struct AutomationState {
    /// Se está executando uma ação
    is_busy: bool,
    /// Ação atual
    current_action: Option<String>,
    /// Contador de ações executadas
    action_count: u64,
    /// Último erro
    last_error: Option<String>,
}

/// Configuração de automação
#[derive(Debug, Clone)]
pub struct AutomationConfig {
    /// Timeout padrão para operações
    pub default_timeout: Duration,
    /// Intervalo de polling para waits
    pub poll_interval: Duration,
    /// Delay entre ações (ms)
    pub action_delay_ms: u64,
    /// Se deve scrollar elementos para visibilidade
    pub auto_scroll: bool,
    /// Se deve tirar screenshot em erros
    pub screenshot_on_error: bool,
    /// Se deve simular comportamento humano
    pub human_like_behavior: bool,
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(30),
            poll_interval: Duration::from_millis(100),
            action_delay_ms: 50,
            auto_scroll: true,
            screenshot_on_error: false,
            human_like_behavior: true,
        }
    }
}

impl AutomationController {
    /// Cria um novo controlador de automação
    pub fn new(browser_id: BrowserId) -> Self {
        Self::with_config(browser_id, AutomationConfig::default())
    }

    /// Cria com configuração customizada
    pub fn with_config(browser_id: BrowserId, config: AutomationConfig) -> Self {
        Self {
            browser_id,
            state: Arc::new(RwLock::new(AutomationState::default())),
            config,
        }
    }

    /// Retorna o ID do browser
    pub fn browser_id(&self) -> BrowserId {
        self.browser_id
    }

    /// Retorna a configuração
    pub fn config(&self) -> &AutomationConfig {
        &self.config
    }

    /// Navega para uma URL
    pub async fn navigate(&self, url: impl AsRef<str>) -> BrowserResult<()> {
        self.start_action("navigate").await?;

        // Simulação: na implementação real, isso interagiria com o WebView
        tracing::info!("Navigating to: {}", url.as_ref());

        tokio::time::sleep(Duration::from_millis(self.config.action_delay_ms)).await;

        self.finish_action().await;
        Ok(())
    }

    /// Clica em um elemento
    pub async fn click(&self, selector: impl AsRef<str>) -> BrowserResult<ClickResult> {
        self.click_with_options(selector, ClickOptions::default())
            .await
    }

    /// Clica com opções avançadas
    pub async fn click_with_options(
        &self,
        selector: impl AsRef<str>,
        options: ClickOptions,
    ) -> BrowserResult<ClickResult> {
        self.start_action("click").await?;

        tracing::info!("Clicking on: {}", selector.as_ref());

        let controller = ClickController::new(self.browser_id);
        let result = controller.click(selector, options).await;

        self.finish_action().await;
        result
    }

    /// Preenche um campo de formulário
    pub async fn fill(
        &self,
        selector: impl AsRef<str>,
        value: impl AsRef<str>,
    ) -> BrowserResult<FillResult> {
        self.fill_with_options(selector, value, FillOptions::default())
            .await
    }

    /// Preenche com opções avançadas
    pub async fn fill_with_options(
        &self,
        selector: impl AsRef<str>,
        value: impl AsRef<str>,
        options: FillOptions,
    ) -> BrowserResult<FillResult> {
        self.start_action("fill").await?;

        tracing::info!(
            "Filling {} with value length: {}",
            selector.as_ref(),
            value.as_ref().len()
        );

        let controller = FillController::new(self.browser_id);
        let result = controller.fill(selector, value, options).await;

        self.finish_action().await;
        result
    }

    /// Preenche múltiplos campos de uma vez
    pub async fn fill_form(&self, form_data: FormData) -> BrowserResult<Vec<FillResult>> {
        self.start_action("fill_form").await?;

        tracing::info!("Filling form with {} fields", form_data.len());

        let controller = FillController::new(self.browser_id);
        let result = async {
            let mut results = Vec::new();
            for (selector, value) in form_data.iter() {
                let result = controller
                    .fill(selector, value, FillOptions::default())
                    .await?;
                results.push(result);

                // Delay entre campos
                tokio::time::sleep(Duration::from_millis(self.config.action_delay_ms)).await;
            }
            Ok(results)
        }
        .await;

        // Finaliza mesmo em erro, senão o controlador fica "ocupado" para sempre
        self.finish_action().await;
        result
    }

    /// Executa JavaScript na página
    pub async fn execute_script(&self, script: impl AsRef<str>) -> BrowserResult<ScriptResult> {
        self.execute_script_in_context(script, ExecutionContext::Page)
            .await
    }

    /// Executa JS em contexto específico (main world ou isolated)
    pub async fn execute_script_in_context(
        &self,
        script: impl AsRef<str>,
        context: ExecutionContext,
    ) -> BrowserResult<ScriptResult> {
        self.start_action("execute_script").await?;

        tracing::debug!("Executing script in {:?} context", context);

        let engine = JavaScriptEngine::new(self.browser_id);
        let result = engine.execute(script, context).await;

        self.finish_action().await;
        result
    }

    /// Extrai a árvore de acessibilidade
    pub async fn get_accessibility_tree(&self) -> BrowserResult<AccessibilityTree> {
        self.start_action("accessibility_tree").await?;

        let extractor = AccessibilityExtractor::new(self.browser_id);
        let result = extractor.extract_full_tree().await;

        self.finish_action().await;
        result
    }

    /// Extrai árvore a partir de um nó específico
    pub async fn get_accessibility_tree_for_node(
        &self,
        root_selector: impl AsRef<str>,
    ) -> BrowserResult<AccessibilityTree> {
        self.start_action("accessibility_tree").await?;

        let extractor = AccessibilityExtractor::new(self.browser_id);
        let result = extractor.extract_from_node(root_selector).await;

        self.finish_action().await;
        result
    }

    /// Tira screenshot da página inteira
    pub async fn screenshot_full(&self) -> BrowserResult<ScreenshotResult> {
        self.screenshot(ScreenshotOptions::full_page()).await
    }

    /// Tira screenshot de uma região específica
    pub async fn screenshot_region(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> BrowserResult<ScreenshotResult> {
        self.screenshot(ScreenshotOptions::region(x, y, width, height))
            .await
    }

    /// Tira screenshot de um elemento específico
    pub async fn screenshot_element(
        &self,
        selector: impl Into<String>,
    ) -> BrowserResult<ScreenshotResult> {
        self.screenshot(ScreenshotOptions::element(selector)).await
    }

    /// Tira screenshot com opções customizadas
    pub async fn screenshot(&self, options: ScreenshotOptions) -> BrowserResult<ScreenshotResult> {
        self.start_action("screenshot").await?;

        let controller = ScreenshotController::new(self.browser_id);
        let result = controller.capture(options).await;

        self.finish_action().await;
        result
    }

    /// Espera por uma condição
    pub async fn wait_for(&self, condition: WaitCondition) -> BrowserResult<WaitResult> {
        self.wait_with_options(condition, WaitOptions::default())
            .await
    }

    /// Espera com opções customizadas
    pub async fn wait_with_options(
        &self,
        condition: WaitCondition,
        options: WaitOptions,
    ) -> BrowserResult<WaitResult> {
        self.start_action("wait").await?;

        let controller = WaitController::new(self.browser_id);
        let result = controller.wait_for(condition, options).await;

        self.finish_action().await;
        result
    }

    /// Espera por navegação completar
    pub async fn wait_for_navigation(&self) -> BrowserResult<WaitResult> {
        self.wait_for(WaitCondition::NavigationComplete).await
    }

    /// Espera por elemento ficar visível
    pub async fn wait_for_selector(&self, selector: impl AsRef<str>) -> BrowserResult<WaitResult> {
        self.wait_for(WaitCondition::ElementVisible {
            selector: selector.as_ref().to_string(),
        })
        .await
    }

    /// Espera por elemento desaparecer
    pub async fn wait_for_selector_hidden(
        &self,
        selector: impl AsRef<str>,
    ) -> BrowserResult<WaitResult> {
        self.wait_for(WaitCondition::ElementHidden {
            selector: selector.as_ref().to_string(),
        })
        .await
    }

    /// Espera por texto aparecer na página
    pub async fn wait_for_text(&self, text: impl AsRef<str>) -> BrowserResult<WaitResult> {
        self.wait_for(WaitCondition::TextPresent {
            text: text.as_ref().to_string(),
        })
        .await
    }

    /// Espera por função JavaScript retornar true
    pub async fn wait_for_function(&self, script: impl AsRef<str>) -> BrowserResult<WaitResult> {
        self.wait_for(WaitCondition::FunctionTrue {
            script: script.as_ref().to_string(),
        })
        .await
    }

    /// Seleciona texto em um elemento
    pub async fn select_text(&self, selector: impl AsRef<str>) -> BrowserResult<String> {
        self.start_action("select_text").await?;

        let script = select_text_script(selector.as_ref());

        let engine = JavaScriptEngine::new(self.browser_id);
        let result = engine.execute(script, ExecutionContext::Page).await;

        // Finaliza mesmo em erro, senão o controlador fica "ocupado" para sempre
        self.finish_action().await;

        result?
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::JavaScriptFailed("Failed to select text".to_string()))
    }

    /// Obtém o HTML da página
    pub async fn get_page_html(&self) -> BrowserResult<String> {
        self.execute_script("document.documentElement.outerHTML")
            .await?
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::JavaScriptFailed("Failed to get HTML".to_string()))
    }

    /// Obtém o texto visível da página
    pub async fn get_page_text(&self) -> BrowserResult<String> {
        self.execute_script("document.body.innerText")
            .await?
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::JavaScriptFailed("Failed to get text".to_string()))
    }

    /// Obtém o título da página
    pub async fn get_page_title(&self) -> BrowserResult<String> {
        self.execute_script("document.title")
            .await?
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::JavaScriptFailed("Failed to get title".to_string()))
    }

    /// Obtém a URL atual
    pub async fn get_page_url(&self) -> BrowserResult<String> {
        self.execute_script("window.location.href")
            .await?
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::JavaScriptFailed("Failed to get URL".to_string()))
    }

    /// Retorna se existe um elemento
    pub async fn element_exists(&self, selector: impl AsRef<str>) -> BrowserResult<bool> {
        self.execute_script(element_exists_script(selector.as_ref()))
            .await?
            .as_bool()
            .ok_or_else(|| {
                BrowserError::JavaScriptFailed("Failed to check element existence".to_string())
            })
    }

    /// Retorna se um elemento está visível
    pub async fn element_visible(&self, selector: impl AsRef<str>) -> BrowserResult<bool> {
        self.execute_script(element_visible_script(selector.as_ref()))
            .await?
            .as_bool()
            .ok_or_else(|| BrowserError::JavaScriptFailed("Failed to check visibility".to_string()))
    }

    /// Rola a página para um elemento
    pub async fn scroll_to(&self, selector: impl AsRef<str>) -> BrowserResult<()> {
        self.execute_script(scroll_to_script(selector.as_ref()))
            .await?;
        Ok(())
    }

    /// Retorna estatísticas da automação
    pub fn get_stats(&self) -> AutomationStats {
        let state = self.state.read();
        AutomationStats {
            action_count: state.action_count,
            is_busy: state.is_busy,
            current_action: state.current_action.clone(),
            last_error: state.last_error.clone(),
        }
    }

    /// Marca início de uma ação
    async fn start_action(&self, action_name: &str) -> BrowserResult<()> {
        let mut state = self.state.write();

        if state.is_busy {
            return Err(BrowserError::IntegrationError(
                "Another automation action is already in progress".to_string(),
            ));
        }

        state.is_busy = true;
        state.current_action = Some(action_name.to_string());
        state.action_count += 1;

        Ok(())
    }

    /// Marca fim de uma ação
    async fn finish_action(&self) {
        let mut state = self.state.write();
        state.is_busy = false;
        state.current_action = None;
    }
}

/// Literal JS (string JSON) para o seletor, evitando injeção via aspas
fn selector_literal(selector: &str) -> String {
    serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string())
}

fn select_text_script(selector: &str) -> String {
    format!(
        r#"
            (function() {{
                const element = document.querySelector({});
                if (!element) return null;
                const selection = window.getSelection();
                const range = document.createRange();
                range.selectNodeContents(element);
                selection.removeAllRanges();
                selection.addRange(range);
                return selection.toString();
            }})()
        "#,
        selector_literal(selector)
    )
}

fn element_exists_script(selector: &str) -> String {
    format!(
        r#"
            document.querySelector({}) !== null
        "#,
        selector_literal(selector)
    )
}

fn element_visible_script(selector: &str) -> String {
    format!(
        r#"
            (function() {{
                const el = document.querySelector({});
                if (!el) return false;
                const rect = el.getBoundingClientRect();
                return rect.width > 0 && rect.height > 0 &&
                       rect.top >= 0 && rect.left >= 0 &&
                       window.getComputedStyle(el).visibility !== 'hidden';
            }})()
        "#,
        selector_literal(selector)
    )
}

fn scroll_to_script(selector: &str) -> String {
    format!(
        r#"
            document.querySelector({})?.scrollIntoView({{ behavior: 'smooth', block: 'center' }})
        "#,
        selector_literal(selector)
    )
}

/// Estatísticas de automação
#[derive(Debug, Clone)]
pub struct AutomationStats {
    /// Número total de ações executadas
    pub action_count: u64,
    /// Se está ocupado
    pub is_busy: bool,
    /// Ação atual (se ocupado)
    pub current_action: Option<String>,
    /// Último erro
    pub last_error: Option<String>,
}

/// Builder para automação
#[derive(Debug)]
pub struct AutomationBuilder {
    browser_id: BrowserId,
    config: AutomationConfig,
}

impl AutomationBuilder {
    /// Cria um novo builder
    pub fn new(browser_id: BrowserId) -> Self {
        Self {
            browser_id,
            config: AutomationConfig::default(),
        }
    }

    /// Define timeout padrão
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config.default_timeout = timeout;
        self
    }

    /// Define delay entre ações
    pub fn with_action_delay(mut self, delay_ms: u64) -> Self {
        self.config.action_delay_ms = delay_ms;
        self
    }

    /// Habilita auto-scroll
    pub fn with_auto_scroll(mut self, enabled: bool) -> Self {
        self.config.auto_scroll = enabled;
        self
    }

    /// Habilita screenshot em erros
    pub fn with_screenshot_on_error(mut self, enabled: bool) -> Self {
        self.config.screenshot_on_error = enabled;
        self
    }

    /// Habilita comportamento humano
    pub fn with_human_like_behavior(mut self, enabled: bool) -> Self {
        self.config.human_like_behavior = enabled;
        self
    }

    /// Constrói o controlador
    pub fn build(self) -> AutomationController {
        AutomationController::with_config(self.browser_id, self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_automation_config_default() {
        let config = AutomationConfig::default();
        assert_eq!(config.default_timeout, Duration::from_secs(30));
        assert_eq!(config.poll_interval, Duration::from_millis(100));
        assert_eq!(config.action_delay_ms, 50);
        assert!(config.auto_scroll);
        assert!(!config.screenshot_on_error);
        assert!(config.human_like_behavior);
    }

    #[test]
    fn test_automation_builder() {
        let browser_id = BrowserId::new();
        let controller = AutomationBuilder::new(browser_id)
            .with_timeout(Duration::from_secs(60))
            .with_action_delay(100)
            .with_auto_scroll(false)
            .with_screenshot_on_error(true)
            .build();

        assert_eq!(controller.config().default_timeout, Duration::from_secs(60));
        assert_eq!(controller.config().action_delay_ms, 100);
        assert!(!controller.config().auto_scroll);
        assert!(controller.config().screenshot_on_error);
    }

    #[tokio::test]
    async fn test_automation_stats() {
        let browser_id = BrowserId::new();
        let controller = AutomationController::new(browser_id);

        let stats = controller.get_stats();
        assert_eq!(stats.action_count, 0);
        assert!(!stats.is_busy);
        assert!(stats.current_action.is_none());
    }

    fn fast_controller() -> AutomationController {
        AutomationBuilder::new(BrowserId::new())
            .with_action_delay(0)
            .with_human_like_behavior(false)
            .build()
    }

    #[test]
    fn test_builder_human_like_and_browser_id() {
        let id = BrowserId::new();
        let controller = AutomationBuilder::new(id)
            .with_human_like_behavior(false)
            .build();
        assert!(!controller.config().human_like_behavior);
        assert_eq!(controller.browser_id(), id);
    }

    #[tokio::test]
    async fn test_actions_update_stats() {
        let controller = fast_controller();
        controller.navigate("https://example.com").await.unwrap();

        let click = controller.click("#ok").await.unwrap();
        assert_eq!(click.selector, "#ok");
        let fill = controller.fill("#nome", "Ana").await.unwrap();
        assert_eq!(fill.value_filled, "Ana");

        let stats = controller.get_stats();
        assert_eq!(stats.action_count, 3);
        assert!(!stats.is_busy);
        assert!(stats.current_action.is_none());
        assert!(stats.last_error.is_none());
    }

    #[tokio::test]
    async fn test_failed_actions_release_the_controller() {
        let controller = fast_controller();
        assert!(controller.click("").await.is_err());
        assert!(controller.fill("", "x").await.is_err());
        assert!(controller.execute_script("").await.is_err());
        assert!(!controller.get_stats().is_busy);
        assert!(controller.click("#depois").await.is_ok());
    }

    /// Regressão: erro no meio de fill_form deixava o controlador ocupado e
    /// toda ação seguinte falhava com "already in progress".
    #[tokio::test]
    async fn test_fill_form_error_does_not_leave_controller_busy() {
        let controller = fast_controller();
        let form = FormData::new().with_field("", "valor");
        assert!(controller.fill_form(form).await.is_err());
        assert!(!controller.get_stats().is_busy);
        controller.navigate("https://example.com").await.unwrap();
    }

    #[tokio::test]
    async fn test_fill_form_success() {
        let controller = fast_controller();
        let form = FormData::new().with_field("#a", "1").with_field("#b", "2");
        let results = controller.fill_form(form).await.unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.success));
    }

    #[tokio::test]
    async fn test_busy_controller_rejects_concurrent_action() {
        let controller = fast_controller();
        controller.start_action("manual").await.unwrap();
        assert_eq!(
            controller.get_stats().current_action.as_deref(),
            Some("manual")
        );
        let err = controller.navigate("https://a.test").await.unwrap_err();
        assert!(err.to_string().contains("already in progress"), "{err}");
        controller.finish_action().await;
        assert!(controller.navigate("https://a.test").await.is_ok());
    }

    #[tokio::test]
    async fn test_page_queries_use_simulated_engine() {
        let controller = fast_controller();
        assert_eq!(controller.get_page_title().await.unwrap(), "Example Page");
        assert_eq!(
            controller.get_page_url().await.unwrap(),
            "https://example.com"
        );
        assert_eq!(
            controller.get_page_text().await.unwrap(),
            "Example page content"
        );
        assert!(controller
            .get_page_html()
            .await
            .unwrap()
            .starts_with("<html>"));
        assert!(controller.element_exists("#x").await.unwrap());
        assert!(controller.element_visible("#x").await.unwrap());
        controller.scroll_to("#x").await.unwrap();

        let result = controller
            .execute_script_in_context("1 + 1", ExecutionContext::Isolated)
            .await
            .unwrap();
        assert_eq!(result.as_number(), Some(2.0));

        // A seleção simulada devolve objeto, não string
        let err = controller.select_text("p").await.unwrap_err();
        assert!(err.to_string().contains("Failed to select text"), "{err}");
        assert!(!controller.get_stats().is_busy);
    }

    #[tokio::test]
    async fn test_accessibility_and_screenshots() {
        let controller = fast_controller();
        let tree = controller.get_accessibility_tree().await.unwrap();
        assert_eq!(tree.title, "Example Page");
        let sub = controller
            .get_accessibility_tree_for_node("#main")
            .await
            .unwrap();
        assert_eq!(sub.root.selector.as_deref(), Some("#main"));

        let full = controller.screenshot_full().await.unwrap();
        assert_eq!(full.dimensions.width, 1920);
        let region = controller.screenshot_region(0, 0, 3, 2).await.unwrap();
        assert_eq!(region.data.len(), 24);
        let element = controller.screenshot_element("#chart").await.unwrap();
        assert_eq!(element.dimensions.height, 300);
    }

    #[tokio::test]
    async fn test_wait_helpers() {
        let controller = fast_controller();
        assert!(controller.wait_for_navigation().await.unwrap().is_success());
        assert!(controller
            .wait_for_selector("#a")
            .await
            .unwrap()
            .is_success());
        assert!(controller.wait_for_text("ok").await.unwrap().is_success());
        assert!(controller
            .wait_for_function("true")
            .await
            .unwrap()
            .is_success());

        let short = WaitOptions::with_timeout(Duration::from_millis(20))
            .poll_interval(Duration::from_millis(5));
        let hidden = controller
            .wait_with_options(WaitCondition::element_hidden("#a"), short)
            .await
            .unwrap();
        assert!(hidden.is_timeout());
    }

    #[test]
    fn test_scripts_escape_selector() {
        let evil = "'); alert(1); //";
        for script in [
            select_text_script(evil),
            element_exists_script(evil),
            element_visible_script(evil),
            scroll_to_script(evil),
        ] {
            assert!(
                script.contains(r#"document.querySelector("'); alert(1); //")"#),
                "{script}"
            );
            assert!(!script.contains("querySelector(''); alert"), "{script}");
        }
        assert!(element_exists_script("#a").contains("!== null"));
        assert!(element_visible_script("#a").contains("!== 'hidden'"));
        assert_eq!(selector_literal(r#"a"b"#), r#""a\"b""#);
    }
}
