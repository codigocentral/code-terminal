//! Selector API - API de Seletores
//!
//! Este módulo fornece funcionalidades para encontrar elementos
//! na página usando diferentes estratégias de seleção.

use crate::browser::{BrowserId, BrowserResult};
use serde::{Deserialize, Serialize};

/// Motor de seletores
#[derive(Debug, Clone)]
pub struct SelectorEngine {
    browser_id: BrowserId,
}

impl SelectorEngine {
    /// Cria um novo motor
    pub fn new(browser_id: BrowserId) -> Self {
        Self { browser_id }
    }

    /// Retorna o browser associado.
    pub fn browser_id(&self) -> BrowserId {
        self.browser_id
    }

    /// Encontra elemento único
    pub async fn find_one(&self, selector: &Selector) -> BrowserResult<Option<SelectorResult>> {
        tracing::debug!("Finding element with selector: {:?}", selector);

        // Simulação
        Ok(Some(SelectorResult {
            found: true,
            selector: selector.to_string(),
            element_id: Some("element_1".to_string()),
            tag_name: Some("div".to_string()),
            text_content: Some("Example".to_string()),
        }))
    }

    /// Encontra múltiplos elementos
    pub async fn find_all(&self, selector: &Selector) -> BrowserResult<Vec<SelectorResult>> {
        tracing::debug!("Finding all elements with selector: {:?}", selector);

        // Simulação
        Ok(vec![
            SelectorResult {
                found: true,
                selector: selector.to_string(),
                element_id: Some("element_1".to_string()),
                tag_name: Some("div".to_string()),
                text_content: Some("Example 1".to_string()),
            },
            SelectorResult {
                found: true,
                selector: selector.to_string(),
                element_id: Some("element_2".to_string()),
                tag_name: Some("div".to_string()),
                text_content: Some("Example 2".to_string()),
            },
        ])
    }

    /// Verifica se elemento existe
    pub async fn exists(&self, selector: &Selector) -> BrowserResult<bool> {
        let result = self.find_one(selector).await?;
        Ok(result.map(|r| r.found).unwrap_or(false))
    }

    /// Conta elementos
    pub async fn count(&self, selector: &Selector) -> BrowserResult<usize> {
        let results = self.find_all(selector).await?;
        Ok(results.len())
    }
}

/// Seletor de elemento
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Selector {
    /// CSS Selector
    Css(String),
    /// XPath
    XPath(String),
    /// Texto do elemento
    Text(String),
    /// Role ARIA
    Role { role: String, name: Option<String> },
    /// ID do elemento
    Id(String),
    /// Nome do elemento
    Name(String),
    /// Test ID (data-testid)
    TestId(String),
    /// Atributo data-*
    Data { key: String, value: String },
    /// Seletor combinado (AND)
    All(Vec<Selector>),
    /// Seletor alternativo (OR)
    Any(Vec<Selector>),
}

impl Selector {
    /// Cria seletor CSS
    pub fn css(selector: impl Into<String>) -> Self {
        Selector::Css(selector.into())
    }

    /// Cria seletor XPath
    pub fn xpath(xpath: impl Into<String>) -> Self {
        Selector::XPath(xpath.into())
    }

    /// Cria seletor por texto
    pub fn text(text: impl Into<String>) -> Self {
        Selector::Text(text.into())
    }

    /// Cria seletor por ID
    pub fn id(id: impl Into<String>) -> Self {
        Selector::Id(id.into())
    }

    /// Cria seletor por test ID
    pub fn test_id(id: impl Into<String>) -> Self {
        Selector::TestId(id.into())
    }

    /// Cria seletor por role ARIA
    pub fn role(role: impl Into<String>) -> Self {
        Selector::Role {
            role: role.into(),
            name: None,
        }
    }

    /// Cria seletor por role com nome
    pub fn role_with_name(role: impl Into<String>, name: impl Into<String>) -> Self {
        Selector::Role {
            role: role.into(),
            name: Some(name.into()),
        }
    }

    /// Combina seletores (AND)
    pub fn and(self, other: Selector) -> Self {
        Selector::All(vec![self, other])
    }

    /// Alternativa entre seletores (OR)
    pub fn or(self, other: Selector) -> Self {
        Selector::Any(vec![self, other])
    }
}

impl std::fmt::Display for Selector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Selector::Css(s) => write!(f, "css:{}", s),
            Selector::XPath(s) => write!(f, "xpath:{}", s),
            Selector::Text(s) => write!(f, "text:{}", s),
            Selector::Role { role, name } => {
                if let Some(n) = name {
                    write!(f, "role:{}[name={}]", role, n)
                } else {
                    write!(f, "role:{}", role)
                }
            }
            Selector::Id(s) => write!(f, "id:{}", s),
            Selector::Name(s) => write!(f, "name:{}", s),
            Selector::TestId(s) => write!(f, "testid:{}", s),
            Selector::Data { key, value } => write!(f, "data-{}:{}", key, value),
            Selector::All(selectors) => {
                write!(
                    f,
                    "all({})",
                    selectors
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                        .join(" AND ")
                )
            }
            Selector::Any(selectors) => {
                write!(
                    f,
                    "any({})",
                    selectors
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                        .join(" OR ")
                )
            }
        }
    }
}

/// Resultado de busca por seletor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorResult {
    /// Se encontrou
    pub found: bool,
    /// Seletor usado
    pub selector: String,
    /// ID do elemento (se aplicável)
    pub element_id: Option<String>,
    /// Nome da tag
    pub tag_name: Option<String>,
    /// Texto do elemento
    pub text_content: Option<String>,
}

impl SelectorResult {
    /// Retorna se encontrou elemento
    pub fn is_found(&self) -> bool {
        self.found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selector_css() {
        let sel = Selector::css("#myId");
        assert!(matches!(&sel, Selector::Css(s) if s == "#myId"));
        assert_eq!(sel.to_string(), "css:#myId");
    }

    #[test]
    fn test_selector_xpath() {
        let sel = Selector::xpath("//div[@class='test']");
        assert!(matches!(sel, Selector::XPath(s) if s == "//div[@class='test']"));
    }

    #[test]
    fn test_selector_text() {
        let sel = Selector::text("Click me");
        assert!(matches!(sel, Selector::Text(s) if s == "Click me"));
    }

    #[test]
    fn test_selector_role() {
        let sel = Selector::role("button");
        assert!(matches!(sel, Selector::Role { role, name } if role == "button" && name.is_none()));

        let sel = Selector::role_with_name("button", "Submit");
        assert!(
            matches!(sel, Selector::Role { role, name } if role == "button" && name == Some("Submit".to_string()))
        );
    }

    #[test]
    fn test_selector_combination() {
        let sel1 = Selector::css(".button");
        let sel2 = Selector::text("Submit");

        let combined = sel1.and(sel2);
        assert!(matches!(combined, Selector::All(_)));
    }

    #[test]
    fn test_selector_display() {
        assert_eq!(Selector::css("#id").to_string(), "css:#id");
        assert_eq!(Selector::test_id("my-test").to_string(), "testid:my-test");
        assert_eq!(Selector::id("unique").to_string(), "id:unique");
    }

    #[tokio::test]
    async fn test_engine_find_exists_and_count() {
        let browser_id = BrowserId::new();
        let engine = SelectorEngine::new(browser_id);
        assert_eq!(engine.browser_id(), browser_id);

        let selector = Selector::css("#app");
        let one = engine.find_one(&selector).await.unwrap().unwrap();
        assert!(one.is_found());
        assert_eq!(one.selector, "css:#app");
        assert_eq!(one.element_id.as_deref(), Some("element_1"));

        let all = engine.find_all(&selector).await.unwrap();
        let ids: Vec<_> = all.iter().filter_map(|r| r.element_id.clone()).collect();
        assert_eq!(ids, vec!["element_1", "element_2"]);

        assert!(engine.exists(&selector).await.unwrap());
        assert_eq!(engine.count(&selector).await.unwrap(), 2);
    }

    #[test]
    fn test_display_for_every_variant() {
        let cases = vec![
            (Selector::xpath("//div"), "xpath://div"),
            (Selector::text("Entrar"), "text:Entrar"),
            (Selector::role("button"), "role:button"),
            (
                Selector::role_with_name("button", "Salvar"),
                "role:button[name=Salvar]",
            ),
            (Selector::id("main"), "id:main"),
            (Selector::Name("email".into()), "name:email"),
            (Selector::test_id("submit"), "testid:submit"),
            (
                Selector::Data {
                    key: "row".into(),
                    value: "3".into(),
                },
                "data-row:3",
            ),
            (
                Selector::css("a").and(Selector::text("b")),
                "all(css:a AND text:b)",
            ),
            (
                Selector::id("x").or(Selector::test_id("y")),
                "any(id:x OR testid:y)",
            ),
        ];
        for (selector, expected) in cases {
            assert_eq!(selector.to_string(), expected);
        }
    }

    #[test]
    fn test_selector_serde_tagging() {
        let json = serde_json::to_value(Selector::role_with_name("link", "Home")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type": "Role", "value": {"role": "link", "name": "Home"}})
        );
        let back: Selector = serde_json::from_value(json).unwrap();
        assert_eq!(back.to_string(), "role:link[name=Home]");
    }
}
