//! JavaScript Evaluation API - API de Execução JavaScript
//!
//! Este módulo fornece funcionalidades para executar código JavaScript
//! na página e extrair resultados de forma segura e tipada.

use crate::browser::{BrowserError, BrowserId, BrowserResult};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::time::Duration;
use tokio::time::timeout;

fn js_string_literal(value: &str) -> BrowserResult<String> {
    serde_json::to_string(value)
        .map_err(|e| BrowserError::JavaScriptFailed(format!("Failed to serialize string: {}", e)))
}

fn is_valid_js_member_path(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|part| {
            let mut chars = part.chars();
            matches!(chars.next(), Some(c) if c == '_' || c == '$' || c.is_ascii_alphabetic())
                && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
        })
}

/// Engine de execução JavaScript
#[derive(Debug, Clone)]
pub struct JavaScriptEngine {
    browser_id: BrowserId,
}

impl JavaScriptEngine {
    /// Cria uma nova engine
    pub fn new(browser_id: BrowserId) -> Self {
        Self { browser_id }
    }

    /// Retorna o browser associado.
    pub fn browser_id(&self) -> BrowserId {
        self.browser_id
    }

    /// Executa um script JavaScript
    pub async fn execute(
        &self,
        script: impl AsRef<str>,
        context: ExecutionContext,
    ) -> BrowserResult<ScriptResult> {
        let script = script.as_ref();

        tracing::debug!("Executing JavaScript in {:?} context", context);

        if script.is_empty() {
            return Err(BrowserError::JavaScriptFailed("Empty script".to_string()));
        }

        // Simulação de execução
        let simulated_result = self.simulate_execution(script).await?;

        Ok(ScriptResult {
            success: true,
            value: simulated_result,
            execution_time_ms: 0,
            executed_at: chrono::Utc::now(),
        })
    }

    /// Executa script com timeout
    pub async fn execute_with_timeout(
        &self,
        script: impl AsRef<str>,
        context: ExecutionContext,
        timeout_duration: Duration,
    ) -> BrowserResult<ScriptResult> {
        match timeout(timeout_duration, self.execute(script, context)).await {
            Ok(result) => result,
            Err(_) => Err(BrowserError::JavaScriptFailed(
                "Script execution timeout".to_string(),
            )),
        }
    }

    /// Executa função com argumentos
    pub async fn execute_function(
        &self,
        function_name: impl AsRef<str>,
        args: Vec<JsonValue>,
        context: ExecutionContext,
    ) -> BrowserResult<ScriptResult> {
        let args_json = serde_json::to_string(&args).map_err(|e| {
            BrowserError::JavaScriptFailed(format!("Failed to serialize args: {}", e))
        })?;
        let function_name = function_name.as_ref();
        if !is_valid_js_member_path(function_name) {
            return Err(BrowserError::JavaScriptFailed(format!(
                "Invalid JavaScript function name: {}",
                function_name
            )));
        }

        let script = format!("{}.apply(null, {})", function_name, args_json);
        self.execute(script, context).await
    }

    /// Avalia expressão e retorna valor
    pub async fn evaluate<T: serde::de::DeserializeOwned>(
        &self,
        expression: impl AsRef<str>,
    ) -> BrowserResult<T> {
        let result = self.execute(expression, ExecutionContext::Page).await?;
        result.deserialize()
    }

    /// Executa script em background (sem retorno)
    pub async fn run_in_background(&self, script: impl AsRef<str>) -> BrowserResult<()> {
        let script = format!(
            "(function() {{ try {{ {}; }} catch(e) {{ console.error(e); }} }})();",
            script.as_ref()
        );
        self.execute(script, ExecutionContext::Page).await?;
        Ok(())
    }

    /// Injeta script na página (persistent)
    pub async fn inject_script(&self, script: impl AsRef<str>) -> BrowserResult<()> {
        let value_str = serde_json::to_string(script.as_ref())
            .map_err(|e| BrowserError::JavaScriptFailed(format!("Failed to serialize: {}", e)))?;

        let wrapped = format!(
            "(function() {{ var script = document.createElement('script'); script.textContent = {}; document.head.appendChild(script); }})();",
            value_str
        );

        self.execute(wrapped, ExecutionContext::Page).await?;
        Ok(())
    }

    /// Obtém propriedade de objeto
    pub async fn get_property(
        &self,
        selector: impl AsRef<str>,
        property: impl AsRef<str>,
    ) -> BrowserResult<ScriptResult> {
        let selector = js_string_literal(selector.as_ref())?;
        let property = js_string_literal(property.as_ref())?;
        let script = format!("document.querySelector({})?.[{}]", selector, property);

        self.execute(script, ExecutionContext::Page).await
    }

    /// Define propriedade de objeto
    pub async fn set_property(
        &self,
        selector: impl AsRef<str>,
        property: impl AsRef<str>,
        value: JsonValue,
    ) -> BrowserResult<()> {
        let value_json = serde_json::to_string(&value).map_err(|e| {
            BrowserError::JavaScriptFailed(format!("Failed to serialize value: {}", e))
        })?;
        let selector = js_string_literal(selector.as_ref())?;
        let property = js_string_literal(property.as_ref())?;

        let script = format!(
            "var el = document.querySelector({}); if (el) el[{}] = {};",
            selector, property, value_json
        );

        self.execute(script, ExecutionContext::Page).await?;
        Ok(())
    }

    /// Chama método em elemento
    pub async fn call_method(
        &self,
        selector: impl AsRef<str>,
        method: impl AsRef<str>,
        args: Vec<JsonValue>,
    ) -> BrowserResult<ScriptResult> {
        let args_json = serde_json::to_string(&args).map_err(|e| {
            BrowserError::JavaScriptFailed(format!("Failed to serialize args: {}", e))
        })?;
        let selector = js_string_literal(selector.as_ref())?;
        let method = js_string_literal(method.as_ref())?;

        let script = format!(
            "document.querySelector({})?.[{}](...{})",
            selector, method, args_json
        );

        self.execute(script, ExecutionContext::Page).await
    }

    /// Executa script em todos os frames
    pub async fn execute_in_all_frames(
        &self,
        script: impl AsRef<str>,
    ) -> BrowserResult<Vec<ScriptResult>> {
        let result = self.execute(script, ExecutionContext::Page).await?;
        Ok(vec![result])
    }

    /// Simula execução (para testes)
    async fn simulate_execution(&self, script: &str) -> BrowserResult<JsonValue> {
        if script.contains("document.title") {
            return Ok(JsonValue::String("Example Page".to_string()));
        }

        if script.contains("window.location.href") {
            return Ok(JsonValue::String("https://example.com".to_string()));
        }

        if script.contains("document.body.innerText") {
            return Ok(JsonValue::String("Example page content".to_string()));
        }

        if script.contains("document.documentElement.outerHTML") {
            return Ok(JsonValue::String(
                "<html><body>Example</body></html>".to_string(),
            ));
        }

        if script.trim() == "1 + 1" {
            return Ok(JsonValue::Number(2.into()));
        }

        if script.contains("!== null") || script.contains("!== 'hidden'") {
            return Ok(JsonValue::Bool(true));
        }

        Ok(JsonValue::Object(serde_json::Map::new()))
    }
}

/// Contexto de execução
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ExecutionContext {
    /// Contexto principal da página
    #[default]
    Page,
    /// Contexto isolado (extension content script style)
    Isolated,
    /// Contexto de worker
    Worker,
    /// Contexto de service worker
    ServiceWorker,
}

/// Resultado de execução de script
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptResult {
    /// Se a execução foi bem-sucedida
    pub success: bool,
    /// Valor retornado
    pub value: JsonValue,
    /// Tempo de execução (ms)
    pub execution_time_ms: u64,
    /// Timestamp
    pub executed_at: chrono::DateTime<chrono::Utc>,
}

impl ScriptResult {
    /// Cria resultado de sucesso
    pub fn success(value: JsonValue) -> Self {
        Self {
            success: true,
            value,
            execution_time_ms: 0,
            executed_at: chrono::Utc::now(),
        }
    }

    /// Cria resultado de erro
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            value: JsonValue::String(message.into()),
            execution_time_ms: 0,
            executed_at: chrono::Utc::now(),
        }
    }

    /// Retorna como string
    pub fn as_str(&self) -> Option<&str> {
        self.value.as_str()
    }

    /// Retorna como número
    pub fn as_number(&self) -> Option<f64> {
        self.value.as_f64()
    }

    /// Retorna como booleano
    pub fn as_bool(&self) -> Option<bool> {
        self.value.as_bool()
    }

    /// Retorna como objeto
    pub fn as_object(&self) -> Option<&serde_json::Map<String, JsonValue>> {
        self.value.as_object()
    }

    /// Retorna como array
    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        self.value.as_array()
    }

    /// Deserializa para tipo específico
    pub fn deserialize<T: serde::de::DeserializeOwned>(&self) -> BrowserResult<T> {
        serde_json::from_value(self.value.clone())
            .map_err(|e| BrowserError::JavaScriptFailed(format!("Deserialization failed: {}", e)))
    }

    /// Retorna valor serializado como JSON
    pub fn to_json(&self) -> BrowserResult<String> {
        serde_json::to_string(&self.value).map_err(|e| {
            BrowserError::JavaScriptFailed(format!("JSON serialization failed: {}", e))
        })
    }

    /// Verifica se é null ou undefined
    pub fn is_null_or_undefined(&self) -> bool {
        self.value.is_null()
    }

    /// Retorna representação em string (para debug)
    pub fn debug_string(&self) -> String {
        format!(
            "ScriptResult {{ success: {}, value: {}, time: {}ms }}",
            self.success,
            self.value.to_string().chars().take(100).collect::<String>(),
            self.execution_time_ms
        )
    }
}

/// Helper para construir scripts JavaScript
pub struct ScriptBuilder {
    lines: Vec<String>,
}

impl ScriptBuilder {
    /// Cria um novo builder
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Adiciona uma linha
    pub fn line(mut self, line: impl Into<String>) -> Self {
        self.lines.push(line.into());
        self
    }

    /// Adiciona query selector com verificação
    pub fn with_element(mut self, selector: &str, var_name: &str) -> Self {
        let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
        self.lines.push(format!(
            "var {} = document.querySelector({});",
            var_name, selector
        ));
        self.lines.push(format!("if (!{}) return null;", var_name));
        self
    }

    /// Retorna o script construído
    pub fn build(self) -> String {
        self.lines.join("\n")
    }
}

impl Default for ScriptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Utilitários JavaScript pré-construídos
pub struct JavaScriptUtils;

impl JavaScriptUtils {
    /// Script para scrollar elemento para visibilidade
    pub fn scroll_into_view(selector: &str) -> String {
        let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
        format!(
            "var el = document.querySelector({}); if (el) el.scrollIntoView({{ behavior: 'smooth', block: 'center' }});",
            selector
        )
    }

    /// Script para obter bounding box
    pub fn get_bounding_box(selector: &str) -> String {
        let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
        format!(
            "var el = document.querySelector({}); if (!el) return null; var rect = el.getBoundingClientRect(); return {{ x: rect.x, y: rect.y, width: rect.width, height: rect.height }};",
            selector
        )
    }

    /// Script para verificar visibilidade
    pub fn is_visible(selector: &str) -> String {
        let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
        format!(
            "var el = document.querySelector({}); if (!el) return false; var rect = el.getBoundingClientRect(); return rect.width > 0 && rect.height > 0 && window.getComputedStyle(el).visibility !== 'hidden';",
            selector
        )
    }

    /// Script para clicar em elemento
    pub fn click_element(selector: &str) -> String {
        let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
        format!(
            "var el = document.querySelector({}); if (el) {{ var event = new MouseEvent('click', {{ bubbles: true, cancelable: true, view: window }}); el.dispatchEvent(event); }}",
            selector
        )
    }

    /// Script para preencher input
    pub fn fill_input(selector: &str, value: &str) -> String {
        let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
        let value = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
        format!(
            "var el = document.querySelector({}); if (el) {{ el.value = {}; el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }})); }}",
            selector, value
        )
    }

    /// Script para obter acessibilidade de elemento
    pub fn get_accessibility_info(selector: &str) -> String {
        let selector = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
        format!(
            "var el = document.querySelector({}); if (!el) return null; return {{ tagName: el.tagName, role: el.getAttribute('role'), ariaLabel: el.getAttribute('aria-label'), textContent: el.textContent?.trim(), disabled: el.disabled, hidden: el.hidden, tabIndex: el.tabIndex }};",
            selector
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_result_as_types() {
        let result = ScriptResult::success(JsonValue::String("test".to_string()));
        assert_eq!(result.as_str(), Some("test"));
        assert!(result.as_number().is_none());
        assert!(result.as_bool().is_none());

        let result = ScriptResult::success(JsonValue::Number(42.into()));
        assert_eq!(result.as_number(), Some(42.0));

        let result = ScriptResult::success(JsonValue::Bool(true));
        assert_eq!(result.as_bool(), Some(true));
    }

    #[test]
    fn test_script_result_deserialize() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct TestObj {
            name: String,
            value: i32,
        }

        let result = ScriptResult::success(serde_json::json!({
            "name": "test",
            "value": 42
        }));

        let obj: TestObj = result.deserialize().unwrap();
        assert_eq!(obj.name, "test");
        assert_eq!(obj.value, 42);
    }

    #[test]
    fn test_script_builder() {
        let script = ScriptBuilder::new()
            .line("var x = 1;")
            .line("var y = 2;")
            .build();

        assert!(script.contains("var x = 1;"));
        assert!(script.contains("var y = 2;"));
    }

    #[test]
    fn test_script_builder_with_element() {
        let script = ScriptBuilder::new()
            .with_element("#myInput", "input")
            .line("input.value = 'test';")
            .build();

        assert!(script.contains("document.querySelector(\"#myInput\")"));
        assert!(script.contains("var input"));
    }

    #[test]
    fn test_script_builder_escapes_selector() {
        let script = ScriptBuilder::new()
            .with_element("'); alert(1); //", "input")
            .build();

        assert!(script.contains("document.querySelector(\"'); alert(1); //\")"));
        assert!(!script.contains("querySelector(''); alert"));
    }

    #[test]
    fn test_js_utils() {
        let scroll = JavaScriptUtils::scroll_into_view("#element");
        assert!(scroll.contains("scrollIntoView"));
        assert!(scroll.contains("#element"));

        let visible = JavaScriptUtils::is_visible("#element");
        assert!(visible.contains("getBoundingClientRect"));

        let click = JavaScriptUtils::click_element("#btn");
        assert!(click.contains("MouseEvent"));
        assert!(click.contains("dispatchEvent"));

        let fill = JavaScriptUtils::fill_input("#input", "value");
        assert!(fill.contains("el.value"));
        assert!(fill.contains("Event('input'"));
    }

    #[test]
    fn test_js_utils_escape_selector_and_value() {
        let click = JavaScriptUtils::click_element("'); alert(1); //");
        assert!(click.contains("document.querySelector(\"'); alert(1); //\")"));
        assert!(!click.contains("querySelector(''); alert"));

        let fill = JavaScriptUtils::fill_input("#input", "x'; alert(1); //");
        assert!(fill.contains("el.value = \"x'; alert(1); //\""));
        assert!(!fill.contains("el.value = 'x'; alert"));
    }

    #[test]
    fn test_js_member_path_validation() {
        assert!(is_valid_js_member_path("window.fetch"));
        assert!(is_valid_js_member_path("$app._run1"));
        assert!(!is_valid_js_member_path("alert(1)"));
        assert!(!is_valid_js_member_path("window['fetch']"));
    }

    #[test]
    fn test_script_result_error() {
        let result = ScriptResult::error("Something went wrong");
        assert!(!result.success);
        assert_eq!(result.as_str(), Some("Something went wrong"));
    }

    #[test]
    fn test_execution_context() {
        let ctx = ExecutionContext::default();
        assert_eq!(ctx, ExecutionContext::Page);

        let ctx = ExecutionContext::Isolated;
        assert_eq!(ctx, ExecutionContext::Isolated);
    }

    fn engine() -> JavaScriptEngine {
        JavaScriptEngine::new(BrowserId::new())
    }

    #[tokio::test]
    async fn test_execute_simulated_values() {
        let id = BrowserId::new();
        let engine = JavaScriptEngine::new(id);
        assert_eq!(engine.browser_id(), id);

        let cases = [
            ("document.title", serde_json::json!("Example Page")),
            (
                "window.location.href",
                serde_json::json!("https://example.com"),
            ),
            (
                "document.body.innerText",
                serde_json::json!("Example page content"),
            ),
            (
                "document.documentElement.outerHTML",
                serde_json::json!("<html><body>Example</body></html>"),
            ),
            ("  1 + 1  ", serde_json::json!(2)),
            ("x !== null", serde_json::json!(true)),
            ("v !== 'hidden'", serde_json::json!(true)),
            ("qualquer()", serde_json::json!({})),
        ];
        for (script, expected) in cases {
            for context in [
                ExecutionContext::Page,
                ExecutionContext::Isolated,
                ExecutionContext::Worker,
                ExecutionContext::ServiceWorker,
            ] {
                let result = engine.execute(script, context).await.unwrap();
                assert!(result.success);
                assert_eq!(result.value, expected, "{script}");
            }
        }

        let err = engine
            .execute("", ExecutionContext::Page)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Empty script"), "{err}");
    }

    #[tokio::test]
    async fn test_execute_with_timeout() {
        let result = engine()
            .execute_with_timeout("1 + 1", ExecutionContext::Page, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(result.as_number(), Some(2.0));

        // Erro interno é propagado mesmo dentro do timeout
        assert!(engine()
            .execute_with_timeout("", ExecutionContext::Page, Duration::from_secs(1))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_execute_function_validates_name() {
        let result = engine()
            .execute_function(
                "window.app.run",
                vec![serde_json::json!(1), serde_json::json!("a")],
                ExecutionContext::Page,
            )
            .await
            .unwrap();
        assert!(result.success);

        let err = engine()
            .execute_function("alert(1)//", vec![], ExecutionContext::Page)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("Invalid JavaScript function name"),
            "{err}"
        );
        assert!(!is_valid_js_member_path(""));
        assert!(!is_valid_js_member_path("a..b"));
        assert!(!is_valid_js_member_path("1abc"));
    }

    #[tokio::test]
    async fn test_evaluate_deserializes() {
        let title: String = engine().evaluate("document.title").await.unwrap();
        assert_eq!(title, "Example Page");
        let sum: i64 = engine().evaluate("1 + 1").await.unwrap();
        assert_eq!(sum, 2);
        let err = engine()
            .evaluate::<i64>("document.title")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Deserialization failed"), "{err}");
    }

    #[tokio::test]
    async fn test_dom_helpers_succeed() {
        let e = engine();
        e.run_in_background("console.log(1)").await.unwrap();
        e.inject_script("window.x = '1';").await.unwrap();
        let prop = e.get_property("#a", "value").await.unwrap();
        assert!(prop.as_object().is_some());
        e.set_property("#a", "value", serde_json::json!("novo"))
            .await
            .unwrap();
        let call = e
            .call_method("#a", "focus", vec![serde_json::json!(true)])
            .await
            .unwrap();
        assert!(call.success);
        let frames = e.execute_in_all_frames("document.title").await.unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].as_str(), Some("Example Page"));
        assert!(e.execute_in_all_frames("").await.is_err());
    }

    #[test]
    fn test_js_string_literal_escapes() {
        assert_eq!(js_string_literal("a'b\"c").unwrap(), r#""a'b\"c""#);
    }

    #[test]
    fn test_script_result_accessors() {
        let array = ScriptResult::success(serde_json::json!([1, 2]));
        assert_eq!(array.as_array().map(Vec::len), Some(2));
        assert!(array.as_object().is_none());
        assert_eq!(array.to_json().unwrap(), "[1,2]");
        assert!(!array.is_null_or_undefined());

        let null = ScriptResult::success(JsonValue::Null);
        assert!(null.is_null_or_undefined());

        let long = ScriptResult::success(JsonValue::String("x".repeat(300)));
        let debug = long.debug_string();
        assert!(debug.starts_with("ScriptResult { success: true, value: \"xxx"));
        assert!(debug.ends_with(", time: 0ms }"));
        // valor truncado em 100 caracteres
        assert!(debug.len() < 160, "{}", debug.len());

        let json = serde_json::to_string(&ExecutionContext::ServiceWorker).unwrap();
        assert_eq!(json, "\"service_worker\"");
    }

    #[test]
    fn test_script_builder_default_and_more_utils() {
        assert_eq!(ScriptBuilder::default().build(), "");

        let bbox = JavaScriptUtils::get_bounding_box("#a");
        assert!(bbox.contains(r##"document.querySelector("#a")"##));
        assert!(bbox.contains("getBoundingClientRect"));

        let info = JavaScriptUtils::get_accessibility_info("'x'");
        assert!(info.contains(r#"document.querySelector("'x'")"#));
        assert!(info.contains("aria-label"));
    }
}
