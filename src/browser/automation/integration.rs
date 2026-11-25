//! Automation Integration - Conecta Automation API ao WebView real
//!
//! Este módulo fornece a ponte entre o AutomationController e o WryWebView,
//! permitindo que as operações de automação controlem o navegador real.

use crate::browser::automation::fill::ValidationResult;
use crate::browser::automation::{
    AutomationConfig, AutomationController, ClickResult, FillResult, MouseButton,
    ScreenshotOptions, ScreenshotResult, ScriptResult, SelectorResult,
};
use crate::browser::{
    webview_impl::WryWebView, BrowserError, BrowserId, BrowserManager, BrowserResult,
};
use parking_lot::RwLock;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// Integração entre AutomationController e WebView real
pub struct AutomationIntegration {
    /// Referência ao BrowserManager
    browser_manager: Arc<RwLock<BrowserManager>>,
    /// Configuração
    config: AutomationConfig,
}

impl AutomationIntegration {
    /// Cria uma nova integração
    pub fn new(browser_manager: Arc<RwLock<BrowserManager>>) -> Self {
        Self {
            browser_manager,
            config: AutomationConfig::default(),
        }
    }

    /// Define a configuração
    pub fn with_config(mut self, config: AutomationConfig) -> Self {
        self.config = config;
        self
    }

    /// Navega para uma URL
    pub async fn navigate(&self, browser_id: BrowserId, url: &str) -> BrowserResult<()> {
        let manager = self.browser_manager.read();

        // Obtém a instância do browser
        let instance = manager
            .get_browser(browser_id)
            .ok_or_else(|| BrowserError::InstanceNotFound(browser_id.to_string()))?;

        // Verifica se está pronto
        if !matches!(
            instance.state,
            crate::browser::BrowserInstanceState::Ready
                | crate::browser::BrowserInstanceState::Navigating
        ) {
            return Err(BrowserError::InvalidState(format!(
                "Browser not ready: {:?}",
                instance.state
            )));
        }

        // Navega
        manager.navigate(browser_id, url)?;

        tracing::info!("Automation: navigated browser {} to {}", browser_id, url);
        Ok(())
    }

    /// Executa JavaScript no contexto da página
    pub async fn execute_script(
        &self,
        browser_id: BrowserId,
        script: &str,
    ) -> BrowserResult<ScriptResult> {
        // Obtém o WebView real
        let webview = self.get_webview(browser_id)?;

        // Executa o script com timeout
        let result = timeout(self.config.default_timeout, async {
            webview.execute_javascript(script)
        })
        .await;

        match result {
            Ok(Ok(())) => Ok(ScriptResult::success(serde_json::Value::String(
                "Script executed successfully".to_string(),
            ))),
            Ok(Err(e)) => Ok(ScriptResult::error(format!("JavaScript error: {}", e))),
            Err(_) => Ok(ScriptResult::error("Script execution timeout".to_string())),
        }
    }

    /// Busca elementos usando seletor CSS
    pub async fn query_selector(
        &self,
        browser_id: BrowserId,
        selector: &str,
    ) -> BrowserResult<SelectorResult> {
        let script = format!(
            r#"
            (() => {{
                const element = document.querySelector('{}');
                if (!element) return null;
                return {{
                    tag: element.tagName.toLowerCase(),
                    id: element.id || null,
                    class: element.className || null,
                    text: element.textContent?.substring(0, 100) || null,
                    rect: element.getBoundingClientRect(),
                    visible: element.offsetParent !== null
                }};
            }})()
            "#,
            Self::escape_js_string(selector)
        );

        let result = self.execute_script(browser_id, &script).await?;

        let output = result.as_str().unwrap_or("");
        if output == "null" || output.is_empty() {
            Ok(SelectorResult {
                found: false,
                selector: selector.to_string(),
                element_id: None,
                tag_name: None,
                text_content: None,
            })
        } else {
            Ok(SelectorResult {
                found: true,
                selector: selector.to_string(),
                element_id: None,
                tag_name: None,
                text_content: Some(output.to_string()),
            })
        }
    }

    /// Clica em um elemento
    pub async fn click_element(
        &self,
        browser_id: BrowserId,
        selector: &str,
    ) -> BrowserResult<ClickResult> {
        let script = format!(
            r#"
            (() => {{
                const element = document.querySelector('{}');
                if (!element) return {{ success: false, error: 'Element not found' }};
                
                try {{
                    // Scrolla para o elemento
                    element.scrollIntoView({{ behavior: 'smooth', block: 'center' }});
                    
                    // Simula clique
                    const clickEvent = new MouseEvent('click', {{
                        bubbles: true,
                        cancelable: true,
                        view: window
                    }});
                    element.dispatchEvent(clickEvent);
                    
                    return {{ success: true, tag: element.tagName.toLowerCase() }};
                }} catch (e) {{
                    return {{ success: false, error: e.message }};
                }}
            }})()
            "#,
            Self::escape_js_string(selector)
        );

        let result = self.execute_script(browser_id, &script).await?;
        let output = result.as_str().unwrap_or("");

        let success = output.contains("success: true");
        Ok(ClickResult {
            success,
            selector: selector.to_string(),
            button: MouseButton::Left,
            coordinates: None,
            clicked_at: chrono::Utc::now(),
            element_found: success,
            element_visible: success,
            element_enabled: success,
            double_click: false,
            triple_click: false,
        })
    }

    /// Preenche um campo de input
    pub async fn fill_input(
        &self,
        browser_id: BrowserId,
        selector: &str,
        value: &str,
    ) -> BrowserResult<FillResult> {
        let script = format!(
            r#"
            (() => {{
                const element = document.querySelector('{}');
                if (!element) return {{ success: false, error: 'Element not found' }};
                
                try {{
                    // Verifica se é um input válido
                    const tag = element.tagName.toLowerCase();
                    if (tag !== 'input' && tag !== 'textarea' && !element.isContentEditable) {{
                        return {{ success: false, error: 'Element is not an input' }};
                    }}
                    
                    // Foca no elemento
                    element.focus();
                    
                    // Define o valor
                    element.value = '{}';
                    
                    // Dispara eventos
                    element.dispatchEvent(new Event('input', {{ bubbles: true }}));
                    element.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    
                    return {{ success: true }};
                }} catch (e) {{
                    return {{ success: false, error: e.message }};
                }}
            }})()
            "#,
            Self::escape_js_string(selector),
            Self::escape_js_string(value)
        );

        let result = self.execute_script(browser_id, &script).await?;
        let output = result.as_str().unwrap_or("");

        let success = output.contains("success: true");
        Ok(FillResult {
            success,
            selector: selector.to_string(),
            value_filled: value.to_string(),
            validation: ValidationResult::default(),
            filled_at: chrono::Utc::now(),
        })
    }

    /// Obtém o HTML da página
    pub async fn get_page_html(&self, browser_id: BrowserId) -> BrowserResult<String> {
        let script = r#"
            document.documentElement.outerHTML
        "#;

        let result = self.execute_script(browser_id, script).await?;
        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::ScriptExecutionFailed("Invalid output".to_string()))
    }

    /// Obtém o texto da página
    pub async fn get_page_text(&self, browser_id: BrowserId) -> BrowserResult<String> {
        let script = r#"
            document.body.innerText
        "#;

        let result = self.execute_script(browser_id, script).await?;
        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::ScriptExecutionFailed("Invalid output".to_string()))
    }

    /// Extrai a árvore de acessibilidade
    pub async fn get_accessibility_tree(&self, browser_id: BrowserId) -> BrowserResult<String> {
        let script = r#"
            (() => {
                function buildTree(element, depth = 0) {
                    if (depth > 5) return null;
                    
                    const node = {
                        tag: element.tagName?.toLowerCase(),
                        role: element.getAttribute('role'),
                        ariaLabel: element.getAttribute('aria-label'),
                        text: element.textContent?.substring(0, 50),
                        interactive: element.onclick !== undefined || 
                                    element.tagName === 'BUTTON' ||
                                    element.tagName === 'A' ||
                                    element.tagName === 'INPUT'
                    };
                    
                    const children = Array.from(element.children)
                        .map(child => buildTree(child, depth + 1))
                        .filter(c => c !== null);
                    
                    if (children.length > 0) {
                        node.children = children;
                    }
                    
                    return node;
                }
                
                return JSON.stringify(buildTree(document.body), null, 2);
            })()
        "#;

        let result = self.execute_script(browser_id, script).await?;
        result
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| BrowserError::ScriptExecutionFailed("Invalid output".to_string()))
    }

    /// Tira screenshot da página
    pub async fn screenshot(
        &self,
        _browser_id: BrowserId,
        _options: ScreenshotOptions,
    ) -> BrowserResult<ScreenshotResult> {
        // Nota: Screenshot real requer implementação adicional no wry
        // ou uso de uma crate como `screenshots`

        tracing::warn!("Screenshot requested but not fully implemented");

        Err(BrowserError::NotImplemented(
            "Screenshot requires platform-specific implementation".to_string(),
        ))
    }

    /// Espera por um elemento
    pub async fn wait_for_element(
        &self,
        browser_id: BrowserId,
        selector: &str,
        timeout_duration: Option<Duration>,
    ) -> BrowserResult<bool> {
        let timeout = timeout_duration.unwrap_or(self.config.default_timeout);
        let poll_interval = self.config.poll_interval;

        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            let result = self.query_selector(browser_id, selector).await?;
            if result.found {
                return Ok(true);
            }
            tokio::time::sleep(poll_interval).await;
        }

        Ok(false)
    }

    /// Espera por navegação completar
    pub async fn wait_for_navigation(
        &self,
        browser_id: BrowserId,
        timeout_duration: Option<Duration>,
    ) -> BrowserResult<()> {
        let timeout = timeout_duration.unwrap_or(self.config.default_timeout);

        // Polling simples - verifica se URL mudou ou se estado é Ready
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            let manager = self.browser_manager.read();

            if let Some(instance) = manager.get_browser(browser_id) {
                match instance.state {
                    crate::browser::BrowserInstanceState::Ready => return Ok(()),
                    crate::browser::BrowserInstanceState::Error => {
                        return Err(BrowserError::InvalidState(
                            "Browser in error state".to_string(),
                        ));
                    }
                    _ => {
                        // Ainda navegando, continua esperando
                    }
                }
            } else {
                return Err(BrowserError::InstanceNotFound(browser_id.to_string()));
            }

            drop(manager);
            tokio::time::sleep(self.config.poll_interval).await;
        }

        Err(BrowserError::Timeout("Navigation timeout".to_string()))
    }

    /// Volta no histórico
    pub async fn go_back(&self, browser_id: BrowserId) -> BrowserResult<()> {
        let manager = self.browser_manager.read();
        manager.go_back(browser_id)?;
        Ok(())
    }

    /// Avança no histórico
    pub async fn go_forward(&self, browser_id: BrowserId) -> BrowserResult<()> {
        let manager = self.browser_manager.read();
        manager.go_forward(browser_id)?;
        Ok(())
    }

    /// Recarrega
    pub async fn reload(&self, browser_id: BrowserId) -> BrowserResult<()> {
        let manager = self.browser_manager.read();
        manager.reload(browser_id)?;
        Ok(())
    }

    // Helper methods

    fn get_webview(&self, _browser_id: BrowserId) -> BrowserResult<WryWebView> {
        // Nota: Na implementação real, precisaríamos de acesso ao WryWebView
        // Isso requer modificação no BrowserManager para armazenar os WebViews

        Err(BrowserError::NotImplemented(
            "Direct WebView access requires BrowserManager modification".to_string(),
        ))
    }

    fn escape_js_string(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    }
}

/// Extensão do BrowserManager para suportar automação
pub trait AutomationSupport {
    /// Cria um controller de automação para um browser
    fn create_automation(&self, browser_id: BrowserId) -> BrowserResult<AutomationController>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_js_string() {
        assert_eq!(
            AutomationIntegration::escape_js_string("hello'world"),
            "hello\\'world"
        );
        assert_eq!(
            AutomationIntegration::escape_js_string(r#"hello"world"#),
            r#"hello\"world"#
        );
    }

    use crate::browser::BrowserInstanceState;

    fn setup() -> (
        AutomationIntegration,
        Arc<RwLock<BrowserManager>>,
        BrowserId,
    ) {
        let manager = Arc::new(RwLock::new(BrowserManager::new()));
        let id = manager.read().create_browser(None).unwrap();
        let config = AutomationConfig {
            default_timeout: Duration::from_millis(40),
            poll_interval: Duration::from_millis(5),
            ..AutomationConfig::default()
        };
        let integration = AutomationIntegration::new(manager.clone()).with_config(config);
        (integration, manager, id)
    }

    fn set_state(
        manager: &Arc<RwLock<BrowserManager>>,
        id: BrowserId,
        state: BrowserInstanceState,
    ) {
        manager.read().get_browser_mut(id).unwrap().state = state;
    }

    #[test]
    fn test_escape_js_string_control_chars() {
        assert_eq!(
            AutomationIntegration::escape_js_string("a\\b\nc\rd"),
            "a\\\\b\\nc\\rd"
        );
    }

    #[tokio::test]
    async fn test_navigate_requires_ready_browser() {
        let (integration, manager, id) = setup();

        // Recém-criado (Creating) não aceita navegação
        let err = integration
            .navigate(id, "https://a.test")
            .await
            .unwrap_err();
        assert!(matches!(err, BrowserError::InvalidState(_)), "{err}");

        set_state(&manager, id, BrowserInstanceState::Ready);
        integration.navigate(id, "https://a.test/").await.unwrap();
        let instance = manager.read().get_browser(id).unwrap();
        assert_eq!(instance.current_url.unwrap().as_str(), "https://a.test/");
        assert_eq!(instance.state, BrowserInstanceState::Navigating);

        // Navegando também aceita nova navegação
        integration.navigate(id, "https://b.test/").await.unwrap();

        // URL inválida é propagada
        let err = integration
            .navigate(id, "javascript:alert(1)")
            .await
            .unwrap_err();
        assert!(matches!(err, BrowserError::InvalidUrl(_)), "{err}");

        // Browser inexistente
        let err = integration
            .navigate(BrowserId::new(), "https://a.test")
            .await
            .unwrap_err();
        assert!(matches!(err, BrowserError::InstanceNotFound(_)), "{err}");
    }

    #[tokio::test]
    async fn test_script_based_operations_report_missing_webview() {
        let (integration, _manager, id) = setup();
        let is_not_impl = |e: BrowserError| matches!(e, BrowserError::NotImplemented(_));

        assert!(is_not_impl(
            integration.execute_script(id, "1").await.unwrap_err()
        ));
        assert!(is_not_impl(
            integration.query_selector(id, "#a").await.unwrap_err()
        ));
        assert!(is_not_impl(
            integration.click_element(id, "#a").await.unwrap_err()
        ));
        assert!(is_not_impl(
            integration.fill_input(id, "#a", "it's").await.unwrap_err()
        ));
        assert!(is_not_impl(
            integration.get_page_html(id).await.unwrap_err()
        ));
        assert!(is_not_impl(
            integration.get_page_text(id).await.unwrap_err()
        ));
        assert!(is_not_impl(
            integration.get_accessibility_tree(id).await.unwrap_err()
        ));
        assert!(is_not_impl(
            integration
                .screenshot(id, ScreenshotOptions::default())
                .await
                .unwrap_err()
        ));
        // A espera propaga o erro da consulta
        assert!(is_not_impl(
            integration
                .wait_for_element(id, "#a", Some(Duration::from_millis(20)))
                .await
                .unwrap_err()
        ));
    }

    #[tokio::test]
    async fn test_wait_for_element_with_zero_timeout_returns_false() {
        let (integration, _manager, id) = setup();
        let found = integration
            .wait_for_element(id, "#a", Some(Duration::ZERO))
            .await
            .unwrap();
        assert!(!found);
    }

    #[tokio::test]
    async fn test_wait_for_navigation_states() {
        let (integration, manager, id) = setup();

        set_state(&manager, id, BrowserInstanceState::Ready);
        integration.wait_for_navigation(id, None).await.unwrap();

        set_state(&manager, id, BrowserInstanceState::Error);
        let err = integration.wait_for_navigation(id, None).await.unwrap_err();
        assert!(matches!(err, BrowserError::InvalidState(_)), "{err}");

        // Fica navegando até estourar o timeout
        set_state(&manager, id, BrowserInstanceState::Navigating);
        let err = integration
            .wait_for_navigation(id, Some(Duration::from_millis(20)))
            .await
            .unwrap_err();
        assert!(matches!(err, BrowserError::Timeout(_)), "{err}");

        let err = integration
            .wait_for_navigation(BrowserId::new(), None)
            .await
            .unwrap_err();
        assert!(matches!(err, BrowserError::InstanceNotFound(_)), "{err}");
    }

    #[tokio::test]
    async fn test_history_operations() {
        let (integration, manager, id) = setup();
        manager.read().navigate(id, "https://a.test/").unwrap();
        manager.read().navigate(id, "https://b.test/").unwrap();

        integration.go_back(id).await.unwrap();
        let current = manager.read().get_browser(id).unwrap().current_url.unwrap();
        assert_eq!(current.as_str(), "https://a.test/");

        integration.go_forward(id).await.unwrap();
        let current = manager.read().get_browser(id).unwrap().current_url.unwrap();
        assert_eq!(current.as_str(), "https://b.test/");

        integration.reload(id).await.unwrap();
        assert_eq!(
            manager.read().get_browser(id).unwrap().state,
            BrowserInstanceState::Loading
        );

        let missing = BrowserId::new();
        assert!(integration.go_back(missing).await.is_err());
        assert!(integration.go_forward(missing).await.is_err());
        assert!(integration.reload(missing).await.is_err());
    }
}
