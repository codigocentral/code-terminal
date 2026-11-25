//! Address Bar - Barra de endereços do navegador

use serde::{Deserialize, Serialize};
use url::Url;

/// Estado da barra de endereços
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressBarState {
    /// Texto atual
    pub text: String,
    /// Texto selecionado
    pub selection: Option<(usize, usize)>,
    /// Posição do cursor
    pub cursor_position: usize,
    /// Sugestões
    pub suggestions: Vec<UrlSuggestion>,
    /// Índice da sugestão selecionada
    pub selected_suggestion: Option<usize>,
    /// Modo de edição
    pub editing: bool,
    /// Focado
    pub focused: bool,
    /// Histórico de URLs
    pub url_history: Vec<String>,
}

impl Default for AddressBarState {
    fn default() -> Self {
        Self::new()
    }
}

impl AddressBarState {
    /// Cria um novo estado
    pub fn new() -> Self {
        Self {
            text: String::new(),
            selection: None,
            cursor_position: 0,
            suggestions: Vec::new(),
            selected_suggestion: None,
            editing: false,
            focused: false,
            url_history: Vec::new(),
        }
    }

    /// Define o texto
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor_position = self.text.len();
        self.update_suggestions();
    }

    /// Obtém a URL do texto atual
    pub fn parse_url(&self) -> Option<Url> {
        // Tenta parse direto
        if let Ok(url) = Url::parse(&self.text) {
            if url.scheme().starts_with("http") {
                return Some(url);
            }
        }

        // Tenta adicionar https://
        let with_scheme = format!("https://{}", self.text);
        if let Ok(url) = Url::parse(&with_scheme) {
            if url.host().is_some() {
                return Some(url);
            }
        }

        None
    }

    /// Atualiza sugestões baseado no texto atual
    fn update_suggestions(&mut self) {
        self.suggestions.clear();

        if self.text.is_empty() {
            return;
        }

        // Busca no histórico
        for url in &self.url_history {
            if url.to_lowercase().contains(&self.text.to_lowercase()) {
                if let Ok(parsed) = Url::parse(url) {
                    self.suggestions.push(UrlSuggestion {
                        url: parsed,
                        title: None,
                        source: SuggestionSource::History,
                    });
                }
            }
        }

        // Limita sugestões
        self.suggestions.truncate(10);
    }

    /// Seleciona próxima sugestão
    pub fn select_next_suggestion(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }

        self.selected_suggestion = match self.selected_suggestion {
            Some(idx) if idx < self.suggestions.len() - 1 => Some(idx + 1),
            None => Some(0),
            other => other,
        };

        // Atualiza texto com sugestão selecionada (cursor vai para o fim,
        // senão poderia ficar além do texto novo)
        if let Some(idx) = self.selected_suggestion {
            self.text = self.suggestions[idx].url.to_string();
            self.cursor_position = self.text.len();
        }
    }

    /// Seleciona sugestão anterior
    pub fn select_previous_suggestion(&mut self) {
        if self.suggestions.is_empty() {
            return;
        }

        self.selected_suggestion = match self.selected_suggestion {
            Some(idx) if idx > 0 => Some(idx - 1),
            _ => None,
        };

        // Atualiza texto com sugestão selecionada (cursor vai para o fim,
        // senão poderia ficar além do texto novo)
        if let Some(idx) = self.selected_suggestion {
            self.text = self.suggestions[idx].url.to_string();
            self.cursor_position = self.text.len();
        }
    }

    /// Adiciona URL ao histórico
    pub fn add_to_history(&mut self, url: impl Into<String>) {
        let url = url.into();

        // Remove se já existe
        self.url_history.retain(|u| u != &url);

        // Adiciona no início
        self.url_history.insert(0, url);

        // Limita tamanho
        self.url_history.truncate(100);
    }

    /// Limpa seleção
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Seleciona todo o texto
    pub fn select_all(&mut self) {
        self.selection = Some((0, self.text.len()));
    }

    /// Move cursor para o início
    pub fn move_to_start(&mut self) {
        self.cursor_position = 0;
        self.clear_selection();
    }

    /// Move cursor para o final
    pub fn move_to_end(&mut self) {
        self.cursor_position = self.text.len();
        self.clear_selection();
    }

    /// Insere texto na posição do cursor
    pub fn insert_text(&mut self, text: &str) {
        // Remove seleção se houver
        if let Some((start, end)) = self.selection {
            self.text.replace_range(start..end, "");
            self.cursor_position = start;
            self.clear_selection();
        }

        // Insere texto
        self.text.insert_str(self.cursor_position, text);
        self.cursor_position += text.len();

        self.update_suggestions();
    }

    /// Deleta caractere antes do cursor
    pub fn backspace(&mut self) {
        // Remove seleção se houver
        if let Some((start, end)) = self.selection {
            self.text.replace_range(start..end, "");
            self.cursor_position = start;
            self.clear_selection();
            self.update_suggestions();
            return;
        }

        // Recua até o início do caractere anterior (o cursor é um offset em
        // bytes; `- 1` caía no meio de caracteres multibyte e dava pânico)
        if let Some((previous, _)) = self.text[..self.cursor_position].char_indices().next_back() {
            self.text.remove(previous);
            self.cursor_position = previous;
            self.update_suggestions();
        }
    }

    /// Deleta caractere após o cursor
    pub fn delete(&mut self) {
        // Remove seleção se houver
        if let Some((start, end)) = self.selection {
            self.text.replace_range(start..end, "");
            self.cursor_position = start;
            self.clear_selection();
            self.update_suggestions();
            return;
        }

        if self.cursor_position < self.text.len() {
            self.text.remove(self.cursor_position);
            self.update_suggestions();
        }
    }
}

/// Barra de endereços
#[derive(Debug)]
pub struct AddressBar {
    state: AddressBarState,
}

impl Default for AddressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl AddressBar {
    /// Cria uma nova barra de endereços
    pub fn new() -> Self {
        Self {
            state: AddressBarState::new(),
        }
    }

    /// Retorna o estado
    pub fn state(&self) -> &AddressBarState {
        &self.state
    }

    /// Retorna o estado mutável
    pub fn state_mut(&mut self) -> &mut AddressBarState {
        &mut self.state
    }

    /// Define a URL
    pub fn set_url(&mut self, url: &Url) {
        self.state.set_text(url.as_str());
        self.state.add_to_history(url.as_str());
        self.state.editing = false;
    }

    /// Inicia edição
    pub fn start_editing(&mut self) {
        self.state.editing = true;
        self.state.focused = true;
        self.state.select_all();
    }

    /// Finaliza edição
    pub fn finish_editing(&mut self) -> Option<Url> {
        self.state.editing = false;
        self.state.focused = false;
        self.state.clear_selection();
        self.state.suggestions.clear();
        self.state.selected_suggestion = None;

        let url = self.state.parse_url()?;
        self.state.add_to_history(url.as_str());
        Some(url)
    }

    /// Cancela edição
    pub fn cancel_editing(&mut self) {
        self.state.editing = false;
        self.state.focused = false;
        self.state.clear_selection();
        self.state.suggestions.clear();
        self.state.selected_suggestion = None;
    }
}

/// Sugestão de URL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlSuggestion {
    /// URL
    pub url: Url,
    /// Título da página (se conhecido)
    pub title: Option<String>,
    /// Fonte da sugestão
    pub source: SuggestionSource,
}

/// Fonte da sugestão
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SuggestionSource {
    /// Histórico
    History,
    /// Bookmarks
    Bookmark,
    /// Pesquisa
    Search,
    /// Sugestão do motor de busca
    SearchEngine,
    /// Autocomplete
    Autocomplete,
}

impl std::fmt::Display for SuggestionSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SuggestionSource::History => write!(f, "History"),
            SuggestionSource::Bookmark => write!(f, "Bookmark"),
            SuggestionSource::Search => write!(f, "Search"),
            SuggestionSource::SearchEngine => write!(f, "Search Engine"),
            SuggestionSource::Autocomplete => write!(f, "Autocomplete"),
        }
    }
}

/// Input de URL
#[derive(Debug, Clone)]
pub struct UrlInput {
    /// Texto bruto
    pub raw: String,
    /// URL parseada (se válida)
    pub parsed: Option<Url>,
    /// Tipo de input
    pub input_type: UrlInputType,
}

/// Tipo de input de URL
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlInputType {
    /// URL completa
    FullUrl,
    /// Domínio sem scheme
    Domain,
    /// Termo de busca
    SearchQuery,
    /// Inválido
    Invalid,
}

impl UrlInput {
    /// Parse de string
    pub fn parse(input: impl Into<String>) -> Self {
        let raw = input.into();

        // Tenta parse como URL
        if let Ok(url) = Url::parse(&raw) {
            if url.scheme().starts_with("http") || url.scheme() == "file" {
                return Self {
                    raw: raw.clone(),
                    parsed: Some(url),
                    input_type: UrlInputType::FullUrl,
                };
            }
        }

        // Tenta adicionar https://
        let with_scheme = format!("https://{}", raw);
        if let Ok(url) = Url::parse(&with_scheme) {
            if url.host().is_some() && !raw.contains(' ') {
                return Self {
                    raw: raw.clone(),
                    parsed: Some(url),
                    input_type: UrlInputType::Domain,
                };
            }
        }

        // É uma busca
        if !raw.is_empty() {
            Self {
                raw: raw.clone(),
                parsed: None,
                input_type: UrlInputType::SearchQuery,
            }
        } else {
            Self {
                raw: raw.clone(),
                parsed: None,
                input_type: UrlInputType::Invalid,
            }
        }
    }

    /// Retorna a URL para navegação
    pub fn navigation_url(&self, search_engine: &str) -> Option<Url> {
        match self.input_type {
            UrlInputType::FullUrl | UrlInputType::Domain => self.parsed.clone(),
            UrlInputType::SearchQuery => {
                let query = urlencoding::encode(&self.raw);
                Url::parse(&format!("{}{}", search_engine, query)).ok()
            }
            UrlInputType::Invalid => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_bar_state() {
        let mut state = AddressBarState::new();

        state.set_text("https://example.com");
        assert_eq!(state.text, "https://example.com");
        assert!(state.parse_url().is_some());
    }

    #[test]
    fn test_address_bar_editing() {
        let mut bar = AddressBar::new();

        // Define URL
        let url = Url::parse("https://example.com").unwrap();
        bar.set_url(&url);

        assert!(!bar.state().editing);

        // Inicia edição
        bar.start_editing();
        assert!(bar.state().editing);
        assert!(bar.state().focused);

        // Finaliza edição
        let result = bar.finish_editing();
        assert!(result.is_some());
        assert!(!bar.state().editing);
    }

    #[test]
    fn test_url_input_parse() {
        let input = UrlInput::parse("https://example.com");
        assert!(matches!(input.input_type, UrlInputType::FullUrl));
        assert!(input.parsed.is_some());

        let input = UrlInput::parse("example.com");
        assert!(matches!(input.input_type, UrlInputType::Domain));
        assert!(input.parsed.is_some());

        let input = UrlInput::parse("search query");
        assert!(matches!(input.input_type, UrlInputType::SearchQuery));
        assert!(input.parsed.is_none());
    }

    #[test]
    fn test_url_input_navigation() {
        let input = UrlInput::parse("https://example.com");
        let url = input.navigation_url("https://google.com/search?q=");
        assert!(url.is_some());

        let input = UrlInput::parse("search query");
        let url = input.navigation_url("https://google.com/search?q=");
        assert!(url.is_some());
        assert!(url.unwrap().as_str().contains("search%20query"));
    }

    #[test]
    fn test_suggestion_navigation() {
        let mut state = AddressBarState::new();

        // Adiciona histórico
        state.add_to_history("https://example.com");
        state.add_to_history("https://example.org");
        state.add_to_history("https://example.net");

        // Digita algo
        state.set_text("example");

        // Deve ter sugestões
        assert!(!state.suggestions.is_empty());

        // Seleciona próxima
        state.select_next_suggestion();
        assert_eq!(state.selected_suggestion, Some(0));

        // Seleciona próxima
        state.select_next_suggestion();
        assert_eq!(state.selected_suggestion, Some(1));
    }

    #[test]
    fn test_text_insertion() {
        let mut state = AddressBarState::new();
        state.set_text("hello");

        state.insert_text(" world");
        assert_eq!(state.text, "hello world");

        // Testa com seleção
        state.select_all();
        state.insert_text("new");
        assert_eq!(state.text, "new");
    }

    #[test]
    fn test_backspace() {
        let mut state = AddressBarState::new();
        state.set_text("hello");

        state.backspace();
        assert_eq!(state.text, "hell");

        // Com seleção
        state.select_all();
        state.backspace();
        assert!(state.text.is_empty());
    }

    /// Regressão: backspace removia o byte anterior ao cursor e entrava em
    /// pânico no meio de caracteres multibyte.
    #[test]
    fn test_backspace_and_delete_with_multibyte_text() {
        let mut state = AddressBarState::new();
        state.set_text("café.com");
        state.move_to_end();
        for _ in 0..4 {
            state.backspace();
        }
        assert_eq!(state.text, "café");
        state.backspace(); // remove o "é" (2 bytes)
        assert_eq!(state.text, "caf");
        assert_eq!(state.cursor_position, 3);

        state.set_text("ãé");
        state.move_to_start();
        state.delete();
        assert_eq!(state.text, "é");
        state.move_to_end();
        state.backspace();
        assert!(state.text.is_empty());
        // Backspace no início não faz nada
        state.backspace();
        state.delete();
        assert!(state.text.is_empty());
    }

    #[test]
    fn test_selection_replaced_by_insert_backspace_and_delete() {
        let mut state = AddressBarState::new();
        state.set_text("antigo");
        state.select_all();
        state.insert_text("novo");
        assert_eq!(state.text, "novo");
        assert!(state.selection.is_none());

        state.selection = Some((1, 3));
        state.backspace();
        assert_eq!(state.text, "no");
        assert_eq!(state.cursor_position, 1);

        state.selection = Some((0, 1));
        state.delete();
        assert_eq!(state.text, "o");
        assert_eq!(state.cursor_position, 0);
    }

    #[test]
    fn test_suggestions_from_history_and_cursor() {
        let mut state = AddressBarState::default();
        state.add_to_history("https://rust-lang.org/");
        state.add_to_history("https://docs.rs/");
        state.add_to_history("https://rust-lang.org/"); // duplicata vai pro topo
        assert_eq!(state.url_history.len(), 2);
        assert_eq!(state.url_history[0], "https://rust-lang.org/");
        state.add_to_history("não é url");

        state.set_text("RUST");
        assert_eq!(state.suggestions.len(), 1);
        assert_eq!(state.suggestions[0].source, SuggestionSource::History);

        state.select_next_suggestion();
        assert_eq!(state.text, "https://rust-lang.org/");
        assert_eq!(state.cursor_position, state.text.len());
        // Já na última: fica
        state.select_next_suggestion();
        assert_eq!(state.selected_suggestion, Some(0));
        state.select_previous_suggestion();
        assert_eq!(state.selected_suggestion, None);

        // Sem sugestões, navegação não muda nada
        state.set_text("");
        assert!(state.suggestions.is_empty());
        state.select_next_suggestion();
        state.select_previous_suggestion();
        assert!(state.selected_suggestion.is_none());

        // Histórico limitado a 100
        for i in 0..120 {
            state.add_to_history(format!("https://site{i}.test/"));
        }
        assert_eq!(state.url_history.len(), 100);
    }

    #[test]
    fn test_parse_url_variants() {
        let mut state = AddressBarState::new();
        state.set_text("example.com/path");
        assert_eq!(
            state.parse_url().unwrap().as_str(),
            "https://example.com/path"
        );
        state.set_text("ftp://example.com");
        // ftp não é http: tenta https://ftp://... que não tem host válido
        assert!(state.parse_url().is_none() || state.parse_url().unwrap().scheme() == "https");
        state.set_text("");
        assert!(state.parse_url().is_none());
    }

    #[test]
    fn test_address_bar_lifecycle() {
        let mut bar = AddressBar::default();
        bar.set_url(&Url::parse("https://a.test/").unwrap());
        assert_eq!(bar.state().text, "https://a.test/");
        assert_eq!(bar.state().url_history, vec!["https://a.test/"]);

        bar.start_editing();
        assert!(bar.state().editing && bar.state().focused);
        assert_eq!(bar.state().selection, Some((0, 15)));
        bar.cancel_editing();
        assert!(!bar.state().editing);
        assert!(bar.state().selection.is_none());

        bar.start_editing();
        bar.state_mut().set_text("b.test");
        let url = bar.finish_editing().unwrap();
        assert_eq!(url.as_str(), "https://b.test/");
        assert_eq!(bar.state().url_history[0], "https://b.test/");

        bar.state_mut().set_text("");
        assert!(bar.finish_editing().is_none());
    }

    #[test]
    fn test_url_input_types_and_search() {
        let input = UrlInput::parse("file:///tmp/x.html");
        assert_eq!(input.input_type, UrlInputType::FullUrl);
        let invalid = UrlInput::parse("");
        assert_eq!(invalid.input_type, UrlInputType::Invalid);
        assert!(invalid.navigation_url("https://s.test/?q=").is_none());

        let search = UrlInput::parse("como fazer pão");
        assert_eq!(search.input_type, UrlInputType::SearchQuery);
        let url = search.navigation_url("https://s.test/?q=").unwrap();
        assert_eq!(url.as_str(), "https://s.test/?q=como%20fazer%20p%C3%A3o");

        let domain = UrlInput::parse("example.org");
        assert_eq!(
            domain.navigation_url("x").unwrap().as_str(),
            "https://example.org/"
        );

        let names: Vec<String> = [
            SuggestionSource::History,
            SuggestionSource::Bookmark,
            SuggestionSource::Search,
            SuggestionSource::SearchEngine,
            SuggestionSource::Autocomplete,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            names,
            vec![
                "History",
                "Bookmark",
                "Search",
                "Search Engine",
                "Autocomplete"
            ]
        );
    }
}
