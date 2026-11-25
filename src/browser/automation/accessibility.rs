//! Accessibility Tree API - API de Árvore de Acessibilidade
//!
//! Este módulo fornece funcionalidades para extrair a árvore de
//! acessibilidade de páginas web, permitindo que AI Agents entendam
//! a estrutura semântica da página.
//!
//! # Exemplo
//!
//! ```rust,no_run
//! use code_terminal::browser::automation::accessibility::AccessibilityExtractor;
//! use code_terminal::browser::BrowserId;
//!
//! async fn extract_page_structure(browser_id: BrowserId) {
//!     let extractor = AccessibilityExtractor::new(browser_id);
//!     let tree = extractor.extract_full_tree().await.unwrap();
//!     
//!     println!("Página: {}", tree.title);
//!     println!("Elementos interativos: {}", tree.interactive_count());
//!     
//!     // Encontra todos os botões
//!     let buttons = tree.find_by_role("button");
//!     for button in buttons {
//!         println!("Botão: {}", button.name.as_deref().unwrap_or("Sem nome"));
//!     }
//! }
//! ```

use crate::browser::{BrowserError, BrowserId, BrowserResult};
use serde::{Deserialize, Serialize};

/// Árvore de acessibilidade completa
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityTree {
    /// Título da página
    pub title: String,
    /// URL da página
    pub url: String,
    /// Raiz da árvore
    pub root: AccessibilityNode,
    /// Metadados
    pub metadata: TreeMetadata,
}

impl AccessibilityTree {
    /// Cria uma nova árvore vazia
    pub fn new(title: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            url: url.into(),
            root: AccessibilityNode::new("root", NodeType::Root),
            metadata: TreeMetadata::default(),
        }
    }

    /// Retorna o número total de nós
    pub fn node_count(&self) -> usize {
        self.root.node_count()
    }

    /// Retorna o número de nós interativos
    pub fn interactive_count(&self) -> usize {
        self.root.interactive_count()
    }

    /// Encontra nós por tipo
    pub fn find_by_type(&self, node_type: NodeType) -> Vec<&AccessibilityNode> {
        self.root.find_by_type(node_type)
    }

    /// Encontra nós por role ARIA
    pub fn find_by_role(&self, role: impl AsRef<str>) -> Vec<&AccessibilityNode> {
        self.root.find_by_role(role)
    }

    /// Encontra nós por nome
    pub fn find_by_name(&self, name: impl AsRef<str>) -> Vec<&AccessibilityNode> {
        self.root.find_by_name(name)
    }

    /// Encontra nós por selector CSS
    pub fn find_by_selector(&self, selector: impl AsRef<str>) -> Vec<&AccessibilityNode> {
        self.root.find_by_selector(selector)
    }

    /// Encontra nós visíveis
    pub fn find_visible(&self) -> Vec<&AccessibilityNode> {
        self.root.find_visible()
    }

    /// Encontra nós interativos (clicáveis, focáveis)
    pub fn find_interactive(&self) -> Vec<&AccessibilityNode> {
        self.root.find_interactive()
    }

    /// Encontra nós com texto
    pub fn find_with_text(&self, text: impl AsRef<str>) -> Vec<&AccessibilityNode> {
        self.root.find_with_text(text)
    }

    /// Retorna representação em texto da árvore
    pub fn to_text_tree(&self) -> String {
        let mut output = format!("📄 {}\n", self.title);
        output.push_str(&format!("🔗 {}\n", self.url));
        output.push_str("═".repeat(50).as_str());
        output.push('\n');
        output.push_str(&self.root.to_text_tree(0));
        output
    }

    /// Retorna representação simplificada (apenas nós interativos)
    pub fn to_interactive_summary(&self) -> String {
        let mut output = format!("Elementos Interativos em: {}\n", self.title);
        output.push_str("─".repeat(40).as_str());
        output.push('\n');

        let interactive = self.find_interactive();
        for (i, node) in interactive.iter().enumerate() {
            output.push_str(&format!(
                "{}. {} - {}",
                i + 1,
                node.node_type.as_str(),
                node.name.as_deref().unwrap_or("(sem nome)")
            ));

            if let Some(selector) = &node.selector {
                output.push_str(&format!(" [{}]", selector));
            }
            output.push('\n');
        }

        output.push_str(&format!("\nTotal: {} elementos", interactive.len()));
        output
    }

    /// Converte para formato JSON
    pub fn to_json(&self) -> BrowserResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| BrowserError::Unknown(format!("JSON serialization failed: {}", e)))
    }

    /// Converte para formato Markdown
    pub fn to_markdown(&self) -> String {
        let mut md = format!("# {}\n\n", self.title);
        md.push_str(&format!("**URL:** {}\n\n", self.url));
        md.push_str(&format!(
            "**Total de elementos:** {}\n\n",
            self.node_count()
        ));

        md.push_str("## Elementos Interativos\n\n");
        let interactive = self.find_interactive();
        for node in interactive {
            md.push_str(&format!(
                "- **{}**: `{}`\n",
                node.node_type.as_str(),
                node.name.as_deref().unwrap_or("(sem nome)")
            ));
        }

        md
    }
}

/// Nó da árvore de acessibilidade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityNode {
    /// ID único do nó
    pub id: String,
    /// Tipo do nó
    #[serde(rename = "type")]
    pub node_type: NodeType,
    /// Nome acessível
    pub name: Option<String>,
    /// Descrição
    pub description: Option<String>,
    /// Valor (para inputs)
    pub value: Option<String>,
    /// Estado do nó
    pub state: NodeState,
    /// Propriedades ARIA
    pub aria: AriaProperties,
    /// Selector CSS
    pub selector: Option<String>,
    /// XPath
    pub xpath: Option<String>,
    /// Posição na página
    pub bounding_box: Option<BoundingBox>,
    /// Filhos
    pub children: Vec<AccessibilityNode>,
}

impl AccessibilityNode {
    /// Cria um novo nó
    pub fn new(id: impl Into<String>, node_type: NodeType) -> Self {
        Self {
            id: id.into(),
            node_type,
            name: None,
            description: None,
            value: None,
            state: NodeState::default(),
            aria: AriaProperties::default(),
            selector: None,
            xpath: None,
            bounding_box: None,
            children: Vec::new(),
        }
    }

    /// Define o nome
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Define a descrição
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Define o valor
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Define o selector
    pub fn with_selector(mut self, selector: impl Into<String>) -> Self {
        self.selector = Some(selector.into());
        self
    }

    /// Define o bounding box
    pub fn with_bounding_box(mut self, x: f64, y: f64, width: f64, height: f64) -> Self {
        self.bounding_box = Some(BoundingBox {
            x,
            y,
            width,
            height,
        });
        self
    }

    /// Adiciona um filho
    pub fn add_child(&mut self, child: AccessibilityNode) {
        self.children.push(child);
    }

    /// Retorna o número total de nós (incluindo este e descendentes)
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.node_count()).sum::<usize>()
    }

    /// Retorna o número de nós interativos
    pub fn interactive_count(&self) -> usize {
        let is_interactive = self.node_type.is_interactive();
        let children_count = self
            .children
            .iter()
            .map(|c| c.interactive_count())
            .sum::<usize>();

        if is_interactive {
            1 + children_count
        } else {
            children_count
        }
    }

    /// Encontra nós por tipo
    pub fn find_by_type(&self, node_type: NodeType) -> Vec<&AccessibilityNode> {
        let mut results = Vec::new();

        if self.node_type == node_type {
            results.push(self);
        }

        for child in &self.children {
            results.extend(child.find_by_type(node_type));
        }

        results
    }

    /// Encontra nós por role ARIA
    pub fn find_by_role(&self, role: impl AsRef<str>) -> Vec<&AccessibilityNode> {
        let role = role.as_ref();
        let mut results = Vec::new();

        if self.aria.role.as_deref() == Some(role) {
            results.push(self);
        }

        for child in &self.children {
            results.extend(child.find_by_role(role));
        }

        results
    }

    /// Encontra nós por nome
    pub fn find_by_name(&self, name: impl AsRef<str>) -> Vec<&AccessibilityNode> {
        let name = name.as_ref().to_lowercase();
        let mut results = Vec::new();

        if self
            .name
            .as_deref()
            .map(|n| n.to_lowercase().contains(&name))
            .unwrap_or(false)
        {
            results.push(self);
        }

        for child in &self.children {
            results.extend(child.find_by_name(&name));
        }

        results
    }

    /// Encontra nós por selector CSS
    pub fn find_by_selector(&self, selector: impl AsRef<str>) -> Vec<&AccessibilityNode> {
        let selector = selector.as_ref();
        let mut results = Vec::new();

        if self.selector.as_deref() == Some(selector) {
            results.push(self);
        }

        for child in &self.children {
            results.extend(child.find_by_selector(selector));
        }

        results
    }

    /// Encontra nós visíveis
    pub fn find_visible(&self) -> Vec<&AccessibilityNode> {
        let mut results = Vec::new();

        if self.state.visible {
            results.push(self);
        }

        for child in &self.children {
            results.extend(child.find_visible());
        }

        results
    }

    /// Encontra nós interativos
    pub fn find_interactive(&self) -> Vec<&AccessibilityNode> {
        let mut results = Vec::new();

        if self.node_type.is_interactive() || self.state.focusable {
            results.push(self);
        }

        for child in &self.children {
            results.extend(child.find_interactive());
        }

        results
    }

    /// Encontra nós com texto específico
    pub fn find_with_text(&self, text: impl AsRef<str>) -> Vec<&AccessibilityNode> {
        let text = text.as_ref().to_lowercase();
        let mut results = Vec::new();

        let has_text = self
            .name
            .as_deref()
            .map(|n| n.to_lowercase().contains(&text))
            .unwrap_or(false)
            || self
                .value
                .as_deref()
                .map(|v| v.to_lowercase().contains(&text))
                .unwrap_or(false);

        if has_text {
            results.push(self);
        }

        for child in &self.children {
            results.extend(child.find_with_text(&text));
        }

        results
    }

    /// Retorna representação em texto com indentação
    pub fn to_text_tree(&self, depth: usize) -> String {
        let indent = "  ".repeat(depth);
        let mut output = String::new();

        let icon = self.node_type.icon();
        let name = self.name.as_deref().unwrap_or("");

        output.push_str(&format!("{}{} {}", indent, icon, self.node_type.as_str()));

        if !name.is_empty() {
            output.push_str(&format!(" \"{}\"", name));
        }

        if let Some(selector) = &self.selector {
            output.push_str(&format!(" [{}]", selector));
        }

        if self.state.disabled {
            output.push_str(" [disabled]");
        }

        output.push('\n');

        for child in &self.children {
            output.push_str(&child.to_text_tree(depth + 1));
        }

        output
    }
}

/// Tipo de nó de acessibilidade
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeType {
    /// Raiz do documento
    Root,
    /// Seção/Container
    Section,
    /// Cabeçalho
    Heading,
    /// Parágrafo
    Paragraph,
    /// Link
    Link,
    /// Botão
    Button,
    /// Input de texto
    TextInput,
    /// Checkbox
    Checkbox,
    /// Radio button
    Radio,
    /// Select/Dropdown
    Select,
    /// Textarea
    TextArea,
    /// Label
    Label,
    /// Imagem
    Image,
    /// Lista
    List,
    /// Item de lista
    ListItem,
    /// Tabela
    Table,
    /// Linha de tabela
    TableRow,
    /// Célula de tabela
    TableCell,
    /// Formulário
    Form,
    /// Navegação
    Navigation,
    /// Artigo
    Article,
    /// Complementar/Aside
    Complementary,
    /// Conteúdo principal
    Main,
    /// Banner/Cabeçalho
    Banner,
    /// Conteúdo informativo
    ContentInfo,
    /// Genérico
    Generic,
}

impl NodeType {
    /// Retorna string do tipo
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Root => "root",
            NodeType::Section => "section",
            NodeType::Heading => "heading",
            NodeType::Paragraph => "paragraph",
            NodeType::Link => "link",
            NodeType::Button => "button",
            NodeType::TextInput => "text_input",
            NodeType::Checkbox => "checkbox",
            NodeType::Radio => "radio",
            NodeType::Select => "select",
            NodeType::TextArea => "textarea",
            NodeType::Label => "label",
            NodeType::Image => "image",
            NodeType::List => "list",
            NodeType::ListItem => "list_item",
            NodeType::Table => "table",
            NodeType::TableRow => "table_row",
            NodeType::TableCell => "table_cell",
            NodeType::Form => "form",
            NodeType::Navigation => "navigation",
            NodeType::Article => "article",
            NodeType::Complementary => "complementary",
            NodeType::Main => "main",
            NodeType::Banner => "banner",
            NodeType::ContentInfo => "content_info",
            NodeType::Generic => "generic",
        }
    }

    /// Retorna ícone para o tipo
    pub fn icon(&self) -> &'static str {
        match self {
            NodeType::Root => "📄",
            NodeType::Section => "📦",
            NodeType::Heading => "📰",
            NodeType::Paragraph => "📝",
            NodeType::Link => "🔗",
            NodeType::Button => "🔘",
            NodeType::TextInput => "⌨️",
            NodeType::Checkbox => "☑️",
            NodeType::Radio => "⭕",
            NodeType::Select => "📋",
            NodeType::TextArea => "📄",
            NodeType::Label => "🏷️",
            NodeType::Image => "🖼️",
            NodeType::List => "📑",
            NodeType::ListItem => "•",
            NodeType::Table => "📊",
            NodeType::TableRow => "➡️",
            NodeType::TableCell => "▫️",
            NodeType::Form => "📋",
            NodeType::Navigation => "🧭",
            NodeType::Article => "📰",
            NodeType::Complementary => "📎",
            NodeType::Main => "📌",
            NodeType::Banner => "🚩",
            NodeType::ContentInfo => "ℹ️",
            NodeType::Generic => "📎",
        }
    }

    /// Retorna se o tipo é interativo
    pub fn is_interactive(&self) -> bool {
        matches!(
            self,
            NodeType::Button
                | NodeType::Link
                | NodeType::TextInput
                | NodeType::Checkbox
                | NodeType::Radio
                | NodeType::Select
                | NodeType::TextArea
        )
    }

    /// Retorna se é um input de formulário
    pub fn is_form_input(&self) -> bool {
        matches!(
            self,
            NodeType::TextInput
                | NodeType::Checkbox
                | NodeType::Radio
                | NodeType::Select
                | NodeType::TextArea
        )
    }
}

/// Estado de um nó
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeState {
    /// Se está focado
    pub focused: bool,
    /// Se está focável
    pub focusable: bool,
    /// Se está desabilitado
    pub disabled: bool,
    /// Se está visível
    pub visible: bool,
    /// Se está expandido (para elementos colapsáveis)
    pub expanded: Option<bool>,
    /// Se está selecionado
    pub selected: Option<bool>,
    /// Se está checado (para checkboxes)
    pub checked: Option<bool>,
    /// Se está pressionado (para botões toggle)
    pub pressed: Option<bool>,
    /// Nível do heading (1-6)
    pub level: Option<u8>,
}

/// Propriedades ARIA
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AriaProperties {
    /// Role ARIA explícita
    pub role: Option<String>,
    /// Label ARIA
    pub label: Option<String>,
}

/// Bounding box (posição e dimensões)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BoundingBox {
    /// Posição X
    pub x: f64,
    /// Posição Y
    pub y: f64,
    /// Largura
    pub width: f64,
    /// Altura
    pub height: f64,
}

impl BoundingBox {
    /// Retorna o centro do elemento
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Verifica se um ponto está dentro do bounding box
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }
}

/// Metadados da árvore
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreeMetadata {
    /// Timestamp da extração
    pub extracted_at: Option<String>,
    /// Tempo de extração (ms)
    pub extraction_time_ms: Option<u64>,
    /// Versão do protocolo
    pub protocol_version: String,
}

/// Extrator de árvore de acessibilidade
#[derive(Debug, Clone)]
pub struct AccessibilityExtractor {
    browser_id: BrowserId,
}

impl AccessibilityExtractor {
    /// Cria um novo extrator
    pub fn new(browser_id: BrowserId) -> Self {
        Self { browser_id }
    }

    /// Retorna o browser associado.
    pub fn browser_id(&self) -> BrowserId {
        self.browser_id
    }

    /// Extrai a árvore completa
    pub async fn extract_full_tree(&self) -> BrowserResult<AccessibilityTree> {
        // Na implementação real, isso executaria JavaScript no WebView
        // para extrair a árvore de acessibilidade

        let start_time = std::time::Instant::now();

        // Simulação: cria uma árvore de exemplo
        let mut tree = AccessibilityTree::new("Example Page", "https://example.com");

        // Adiciona elementos de exemplo
        let mut header = AccessibilityNode::new("header", NodeType::Banner)
            .with_name("Cabeçalho")
            .with_selector("header");

        let mut nav = AccessibilityNode::new("nav", NodeType::Navigation).with_selector("nav");

        let link1 = AccessibilityNode::new("link1", NodeType::Link)
            .with_name("Home")
            .with_selector("a[href='/']");

        nav.add_child(link1);
        header.add_child(nav);

        let mut main = AccessibilityNode::new("main", NodeType::Main).with_selector("main");

        let heading = AccessibilityNode::new("h1", NodeType::Heading)
            .with_name("Bem-vindo")
            .with_selector("h1");

        let paragraph = AccessibilityNode::new("p1", NodeType::Paragraph)
            .with_name("Este é um parágrafo de exemplo.")
            .with_selector("p");

        let mut form = AccessibilityNode::new("form1", NodeType::Form).with_selector("form");

        let input = AccessibilityNode::new("input1", NodeType::TextInput)
            .with_name("Email")
            .with_selector("input[type='email']");

        let button = AccessibilityNode::new("btn1", NodeType::Button)
            .with_name("Enviar")
            .with_selector("button[type='submit']");

        form.add_child(input);
        form.add_child(button);

        main.add_child(heading);
        main.add_child(paragraph);
        main.add_child(form);

        tree.root.add_child(header);
        tree.root.add_child(main);

        let elapsed = start_time.elapsed();
        tree.metadata.extraction_time_ms = Some(elapsed.as_millis() as u64);
        tree.metadata.extracted_at = Some(chrono::Utc::now().to_rfc3339());

        Ok(tree)
    }

    /// Extrai árvore a partir de um nó específico
    pub async fn extract_from_node(
        &self,
        selector: impl AsRef<str>,
    ) -> BrowserResult<AccessibilityTree> {
        // Na implementação real, extrairia apenas o sub-árvore
        let mut tree = self.extract_full_tree().await?;
        tree.root.selector = Some(selector.as_ref().to_string());
        Ok(tree)
    }

    /// Extrai apenas elementos interativos
    pub async fn extract_interactive_only(&self) -> BrowserResult<Vec<AccessibilityNode>> {
        let tree = self.extract_full_tree().await?;
        Ok(tree.root.find_interactive().into_iter().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_properties() {
        assert!(NodeType::Button.is_interactive());
        assert!(NodeType::TextInput.is_interactive());
        assert!(NodeType::TextInput.is_form_input());
        assert!(!NodeType::Paragraph.is_interactive());
        assert!(!NodeType::Section.is_form_input());
    }

    #[test]
    fn test_accessibility_node_builder() {
        let node = AccessibilityNode::new("test", NodeType::Button)
            .with_name("Click Me")
            .with_selector("#btn")
            .with_bounding_box(10.0, 20.0, 100.0, 50.0);

        assert_eq!(node.id, "test");
        assert_eq!(node.node_type, NodeType::Button);
        assert_eq!(node.name, Some("Click Me".to_string()));
        assert_eq!(node.selector, Some("#btn".to_string()));
        assert!(node.bounding_box.is_some());
    }

    #[test]
    fn test_tree_node_count() {
        let mut root = AccessibilityNode::new("root", NodeType::Root);
        root.add_child(AccessibilityNode::new("child1", NodeType::Button));
        root.add_child(AccessibilityNode::new("child2", NodeType::Paragraph));

        assert_eq!(root.node_count(), 3);
        assert_eq!(root.interactive_count(), 1);
    }

    #[test]
    fn test_find_by_type() {
        let mut root = AccessibilityNode::new("root", NodeType::Section);
        root.add_child(AccessibilityNode::new("btn1", NodeType::Button));
        root.add_child(AccessibilityNode::new("btn2", NodeType::Button));
        root.add_child(AccessibilityNode::new("p1", NodeType::Paragraph));

        let buttons = root.find_by_type(NodeType::Button);
        assert_eq!(buttons.len(), 2);
    }

    #[test]
    fn test_bounding_box() {
        let bbox = BoundingBox {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        assert_eq!(bbox.center(), (60.0, 45.0));
        assert!(bbox.contains(50.0, 30.0));
        assert!(!bbox.contains(200.0, 200.0));
    }

    #[test]
    fn test_tree_serialization() {
        let tree = AccessibilityTree::new("Test", "https://test.com");
        let json = tree.to_json().unwrap();

        assert!(json.contains("Test"));
        assert!(json.contains("https://test.com"));
    }

    const ALL_TYPES: [(NodeType, &str, &str, bool, bool); 26] = [
        (NodeType::Root, "root", "📄", false, false),
        (NodeType::Section, "section", "📦", false, false),
        (NodeType::Heading, "heading", "📰", false, false),
        (NodeType::Paragraph, "paragraph", "📝", false, false),
        (NodeType::Link, "link", "🔗", true, false),
        (NodeType::Button, "button", "🔘", true, false),
        (NodeType::TextInput, "text_input", "⌨️", true, true),
        (NodeType::Checkbox, "checkbox", "☑️", true, true),
        (NodeType::Radio, "radio", "⭕", true, true),
        (NodeType::Select, "select", "📋", true, true),
        (NodeType::TextArea, "textarea", "📄", true, true),
        (NodeType::Label, "label", "🏷️", false, false),
        (NodeType::Image, "image", "🖼️", false, false),
        (NodeType::List, "list", "📑", false, false),
        (NodeType::ListItem, "list_item", "•", false, false),
        (NodeType::Table, "table", "📊", false, false),
        (NodeType::TableRow, "table_row", "➡️", false, false),
        (NodeType::TableCell, "table_cell", "▫️", false, false),
        (NodeType::Form, "form", "📋", false, false),
        (NodeType::Navigation, "navigation", "🧭", false, false),
        (NodeType::Article, "article", "📰", false, false),
        (NodeType::Complementary, "complementary", "📎", false, false),
        (NodeType::Main, "main", "📌", false, false),
        (NodeType::Banner, "banner", "🚩", false, false),
        (NodeType::ContentInfo, "content_info", "ℹ️", false, false),
        (NodeType::Generic, "generic", "📎", false, false),
    ];

    #[test]
    fn test_every_node_type_metadata() {
        for (node_type, name, icon, interactive, form_input) in ALL_TYPES {
            assert_eq!(node_type.as_str(), name);
            assert_eq!(node_type.icon(), icon, "{name}");
            assert_eq!(node_type.is_interactive(), interactive, "{name}");
            assert_eq!(node_type.is_form_input(), form_input, "{name}");
            // serde faz ida e volta (TextArea vira "text_area" no JSON)
            let json = serde_json::to_string(&node_type).unwrap();
            let back: NodeType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, node_type, "{json}");
        }
    }

    async fn sample_tree() -> AccessibilityTree {
        let browser_id = BrowserId::new();
        let extractor = AccessibilityExtractor::new(browser_id);
        assert_eq!(extractor.browser_id(), browser_id);
        extractor.extract_full_tree().await.unwrap()
    }

    #[tokio::test]
    async fn test_extract_full_tree_structure_and_queries() {
        let tree = sample_tree().await;
        assert_eq!(tree.title, "Example Page");
        assert_eq!(tree.url, "https://example.com");
        assert!(tree.metadata.extracted_at.is_some());
        assert!(tree.metadata.extraction_time_ms.is_some());

        // root, header, nav, link, main, h1, p, form, input, button
        assert_eq!(tree.node_count(), 10);
        assert_eq!(tree.interactive_count(), 3);

        let ids = |nodes: Vec<&AccessibilityNode>| -> Vec<String> {
            nodes.iter().map(|n| n.id.clone()).collect()
        };
        assert_eq!(ids(tree.find_by_type(NodeType::Link)), vec!["link1"]);
        assert_eq!(ids(tree.find_by_name("ENVIAR")), vec!["btn1"]);
        assert_eq!(ids(tree.find_by_selector("form")), vec!["form1"]);
        assert!(tree.find_by_selector("#nada").is_empty());
        assert_eq!(
            ids(tree.find_interactive()),
            vec!["link1", "input1", "btn1"]
        );
        assert_eq!(ids(tree.find_with_text("parágrafo")), vec!["p1"]);
        // Nenhum nó de exemplo tem role ou está marcado visível
        assert!(tree.find_by_role("button").is_empty());
        assert!(tree.find_visible().is_empty());
    }

    #[test]
    fn test_role_visibility_focus_and_value_queries() {
        let mut root = AccessibilityNode::new("root", NodeType::Root);
        let mut div = AccessibilityNode::new("div", NodeType::Generic)
            .with_name("Caixa")
            .with_description("container");
        div.aria.role = Some("button".into());
        div.state.visible = true;
        div.state.focusable = true;
        let input = AccessibilityNode::new("in", NodeType::TextInput).with_value("Valor Digitado");
        root.add_child(div);
        root.add_child(input);

        assert_eq!(root.find_by_role("button").len(), 1);
        assert_eq!(root.find_visible().len(), 1);
        // focável conta como interativo na busca, mas não no contador por tipo
        assert_eq!(root.find_interactive().len(), 2);
        assert_eq!(root.interactive_count(), 1);
        let by_value = root.find_with_text("digitado");
        assert_eq!(by_value.len(), 1);
        assert_eq!(by_value[0].id, "in");
        assert_eq!(root.children[0].description.as_deref(), Some("container"));
    }

    #[test]
    fn test_text_tree_rendering() {
        let mut tree = AccessibilityTree::new("Página", "https://x.test");
        let mut button = AccessibilityNode::new("b", NodeType::Button)
            .with_name("OK")
            .with_selector("#ok");
        button.state.disabled = true;
        let mut form = AccessibilityNode::new("f", NodeType::Form);
        form.add_child(button);
        tree.root.add_child(form);

        let text = tree.to_text_tree();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "📄 Página");
        assert_eq!(lines[1], "🔗 https://x.test");
        assert_eq!(lines[2], "═".repeat(50));
        assert_eq!(lines[3], "📄 root");
        assert_eq!(lines[4], "  📋 form");
        assert_eq!(lines[5], "    🔘 button \"OK\" [#ok] [disabled]");
    }

    #[test]
    fn test_interactive_summary_and_markdown() {
        let mut tree = AccessibilityTree::new("Login", "https://login.test");
        tree.root.add_child(
            AccessibilityNode::new("u", NodeType::TextInput)
                .with_name("Usuário")
                .with_selector("#user"),
        );
        tree.root
            .add_child(AccessibilityNode::new("b", NodeType::Button));

        let summary = tree.to_interactive_summary();
        assert!(summary.starts_with("Elementos Interativos em: Login\n"));
        assert!(summary.contains("1. text_input - Usuário [#user]\n"));
        assert!(summary.contains("2. button - (sem nome)\n"));
        assert!(summary.ends_with("Total: 2 elementos"));

        let md = tree.to_markdown();
        assert!(md.starts_with("# Login\n\n**URL:** https://login.test\n\n"));
        assert!(md.contains("**Total de elementos:** 3"));
        assert!(md.contains("- **text_input**: `Usuário`"));
        assert!(md.contains("- **button**: `(sem nome)`"));
    }

    #[test]
    fn test_json_roundtrip_renames_type() {
        let mut tree = AccessibilityTree::new("T", "u");
        tree.root
            .add_child(AccessibilityNode::new("c", NodeType::Checkbox).with_name("Aceito"));
        let json = tree.to_json().unwrap();
        assert!(json.contains("\"type\": \"checkbox\""), "{json}");
        let back: AccessibilityTree = serde_json::from_str(&json).unwrap();
        assert_eq!(back.node_count(), 2);
        assert_eq!(back.root.children[0].name.as_deref(), Some("Aceito"));
    }

    #[tokio::test]
    async fn test_extract_from_node_and_interactive_only() {
        let extractor = AccessibilityExtractor::new(BrowserId::new());
        let tree = extractor.extract_from_node("#main").await.unwrap();
        assert_eq!(tree.root.selector.as_deref(), Some("#main"));

        let interactive = extractor.extract_interactive_only().await.unwrap();
        let names: Vec<_> = interactive
            .iter()
            .map(|n| n.name.clone().unwrap_or_default())
            .collect();
        assert_eq!(names, vec!["Home", "Email", "Enviar"]);
    }

    #[test]
    fn test_bounding_box_edges() {
        let bbox = BoundingBox {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        assert!(bbox.contains(0.0, 0.0));
        assert!(bbox.contains(10.0, 10.0));
        assert!(!bbox.contains(10.1, 5.0));
        assert!(!bbox.contains(5.0, -0.1));
    }
}
