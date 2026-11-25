#[derive(Debug, Clone, PartialEq)]
pub enum SearchBarState {
    Hidden,
    Visible,
    Active,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub text: String,
}

impl SearchMatch {
    pub fn new(line: usize, start_col: usize, end_col: usize, text: String) -> Self {
        Self {
            line,
            start_col,
            end_col,
            text,
        }
    }
}

/// Resultados guardados por busca (#34). Uma letra comum num histórico de
/// 10 mil linhas dava 50 mil resultados (~9 MB); passando disso, ficam os
/// mais novos e o resto só conta.
pub const MAX_MATCHES: usize = 2000;

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub current_match_index: usize,
    pub total_matches: usize,
    pub direction: SearchDirection,
    pub options: SearchOptions,
}

impl SearchResult {
    pub fn new(query: String, options: SearchOptions) -> Self {
        Self {
            query,
            matches: Vec::new(),
            current_match_index: 0,
            total_matches: 0,
            direction: SearchDirection::Forward,
            options,
        }
    }

    pub fn current_match(&self) -> Option<&SearchMatch> {
        self.matches.get(self.current_match_index)
    }

    pub fn next_match(&mut self) -> bool {
        if self.matches.is_empty() {
            return false;
        }

        self.current_match_index = match self.direction {
            SearchDirection::Forward => (self.current_match_index + 1) % self.matches.len(),
            SearchDirection::Backward => {
                if self.current_match_index == 0 {
                    self.matches.len() - 1
                } else {
                    self.current_match_index - 1
                }
            }
        };

        true
    }

    pub fn previous_match(&mut self) -> bool {
        if self.matches.is_empty() {
            return false;
        }

        let old_direction = self.direction.clone();

        self.direction = match self.direction {
            SearchDirection::Forward => SearchDirection::Backward,
            SearchDirection::Backward => SearchDirection::Forward,
        };

        let result = self.next_match();

        self.direction = old_direction;

        result
    }

    pub fn has_matches(&self) -> bool {
        !self.matches.is_empty()
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }
}

#[derive(Debug, Clone)]
pub struct SearchBar {
    pub state: SearchBarState,
    pub query: String,
    pub result: Option<SearchResult>,
    pub options: SearchOptions,
    pub visible_line: usize,
}

impl Default for SearchBar {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchBar {
    pub fn new() -> Self {
        Self {
            state: SearchBarState::Hidden,
            query: String::new(),
            result: None,
            options: SearchOptions::default(),
            visible_line: 0,
        }
    }

    pub fn show(&mut self) {
        self.state = SearchBarState::Visible;
        self.query.clear();
        self.result = None;
    }

    pub fn hide(&mut self) {
        self.state = SearchBarState::Hidden;
        self.query.clear();
        self.result = None;
    }

    pub fn activate(&mut self) {
        self.state = SearchBarState::Active;
    }

    pub fn deactivate(&mut self) {
        self.state = SearchBarState::Visible;
    }

    pub fn is_visible(&self) -> bool {
        self.state != SearchBarState::Hidden
    }

    pub fn is_active(&self) -> bool {
        self.state == SearchBarState::Active
    }

    pub fn toggle_case_sensitive(&mut self) {
        self.options.case_sensitive = !self.options.case_sensitive;
    }

    pub fn toggle_whole_word(&mut self) {
        self.options.whole_word = !self.options.whole_word;
    }

    pub fn toggle_regex(&mut self) {
        self.options.regex = !self.options.regex;
    }

    pub fn search_in_lines(&self, lines: &[&str]) -> SearchResult {
        self.search_in_line_iter(lines.iter().map(|line| line.to_string()))
    }

    /// Busca linha a linha, sem precisar de todas as linhas em memória ao
    /// mesmo tempo: o índice de cada resultado é a posição da linha no
    /// iterador (#34: histórico + tela, abertos um por vez).
    pub fn search_in_line_iter(&self, lines: impl Iterator<Item = String>) -> SearchResult {
        let mut result = SearchResult::new(self.query.clone(), self.options.clone());

        if self.query.is_empty() {
            return result;
        }

        let search_pattern = if self.options.regex || self.options.case_sensitive {
            self.query.clone()
        } else {
            self.query.to_lowercase()
        };
        // A expressão é compilada uma vez (antes era uma por linha).
        let regex = if self.options.regex {
            match regex::Regex::new(&search_pattern) {
                Ok(re) => Some(re),
                Err(_) => return result,
            }
        } else {
            None
        };

        // Guarda só os últimos MAX_MATCHES (os mais novos: tela e fim do
        // histórico); os mais antigos só entram na contagem.
        let mut found: std::collections::VecDeque<SearchMatch> = std::collections::VecDeque::new();
        let mut dropped = 0usize;
        for (line_idx, line) in lines.enumerate() {
            let line = line.as_str();
            let search_line: String = if self.options.case_sensitive || self.options.regex {
                line.to_string()
            } else {
                line.to_lowercase()
            };

            if let Some(re) = &regex {
                for mat in re.find_iter(&search_line) {
                    let start_byte = mat.start();
                    let end_byte = mat.end();

                    let start_char = line[..start_byte].chars().count();
                    let end_char = line[..end_byte].chars().count();

                    found.push_back(SearchMatch::new(
                        line_idx,
                        start_char,
                        end_char,
                        mat.as_str().to_string(),
                    ));
                }
            } else if self.options.whole_word {
                let mut start = 0;
                while let Some(pos) = search_line[start..].find(&search_pattern) {
                    let abs_pos = start + pos;
                    let is_word_boundary_before = abs_pos == 0
                        || !search_line
                            .chars()
                            .nth(abs_pos - 1)
                            .map(|c: char| c.is_alphanumeric())
                            .unwrap_or(false);
                    let is_word_boundary_after = abs_pos + search_pattern.len()
                        >= search_line.len()
                        || !search_line
                            .chars()
                            .nth(abs_pos + search_pattern.len())
                            .map(|c: char| c.is_alphanumeric())
                            .unwrap_or(false);

                    if is_word_boundary_before && is_word_boundary_after {
                        let start_char = line[..abs_pos].chars().count();
                        let end_char = line[..abs_pos + search_pattern.len()].chars().count();

                        found.push_back(SearchMatch::new(
                            line_idx,
                            start_char,
                            end_char,
                            search_pattern.clone(),
                        ));
                    }
                    start = abs_pos + 1;
                }
            } else {
                let mut start = 0;
                while let Some(pos) = search_line[start..].find(&search_pattern) {
                    let abs_pos = start + pos;
                    let start_char = line[..abs_pos].chars().count();
                    let end_char = line[..abs_pos + search_pattern.len()].chars().count();

                    found.push_back(SearchMatch::new(
                        line_idx,
                        start_char,
                        end_char,
                        search_pattern.clone(),
                    ));

                    start = abs_pos + 1;
                }
            }
            while found.len() > MAX_MATCHES {
                found.pop_front();
                dropped += 1;
            }
        }

        let total = dropped + found.len();
        result.matches = Vec::from(found);
        result.total_matches = total;
        result.current_match_index = 0;
        result
    }

    pub fn get_current_match_position(&self) -> Option<(usize, usize, usize)> {
        self.result
            .as_ref()
            .and_then(|r| r.current_match().map(|m| (m.line, m.start_col, m.end_col)))
    }

    pub fn get_status_text(&self) -> String {
        if let Some(ref result) = self.result {
            if result.query.is_empty() {
                "Search".to_string()
            } else if result.matches.is_empty() {
                format!("No results for \"{}\"", result.query)
            } else {
                let more = if result.total_matches > result.matches.len() {
                    "+"
                } else {
                    ""
                };
                format!(
                    "{}/{}{} matches for \"{}\"",
                    result.current_match_index + 1,
                    result.matches.len(),
                    more,
                    result.query
                )
            }
        } else {
            "Search".to_string()
        }
    }

    // === Search Incremental ===

    /// Realiza busca incremental - atualiza resultados conforme a query muda
    /// Retorna true se os resultados foram atualizados
    pub fn incremental_search(&mut self, new_query: &str) -> bool {
        // Garante que a barra está visível durante a busca
        if self.state == SearchBarState::Hidden {
            self.show();
        }

        // Verifica se precisa re-buscar (query mudou OU opções mudaram)
        let options_changed = self
            .result
            .as_ref()
            .map(|r| r.options != self.options)
            .unwrap_or(true);

        let query_changed = self.query != new_query;

        if new_query == self.query && !options_changed {
            return false;
        }

        self.query = new_query.to_string();

        if self.query.is_empty() {
            self.result = None;
            return true;
        }

        self.result = Some(SearchResult::new(self.query.clone(), self.options.clone()));

        query_changed || options_changed
    }

    /// Busca em strings (String slice)
    pub fn search_in_string_lines(&self, lines: &[String]) -> SearchResult {
        let str_refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
        self.search_in_lines(&str_refs)
    }

    /// Atualiza a query e executa busca imediatamente
    pub fn search_now(&mut self, lines: &[String]) -> &SearchResult {
        let result = self.search_in_string_lines(lines);
        self.result = Some(result);

        self.result.as_ref().unwrap()
    }

    /// Busca incremental com resultados imediatos
    pub fn search_incremental(&mut self, new_query: &str, lines: &[String]) -> bool {
        let changed = self.incremental_search(new_query);

        if changed && !self.query.is_empty() {
            self.result = Some(self.search_in_string_lines(lines));
        }

        changed
    }

    /// Retorna true se há resultados de busca ativos
    pub fn has_active_results(&self) -> bool {
        self.result
            .as_ref()
            .map(|r| r.has_matches())
            .unwrap_or(false)
    }

    /// Retorna o índice do match atual
    pub fn current_match_index(&self) -> Option<usize> {
        self.result
            .as_ref()
            .filter(|r| r.has_matches())
            .map(|r| r.current_match_index)
    }

    /// Reseta para estado inicial mas mantém última query (para reuse)
    pub fn reset_keep_query(&mut self) {
        self.state = SearchBarState::Visible;
        self.result = None;
    }

    /// Limpa completamente a busca
    pub fn clear(&mut self) {
        self.query.clear();
        self.result = None;
        self.state = SearchBarState::Hidden;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #34: passando do limite, ficam os resultados mais novos; o resto só
    /// entra na contagem (e no "+" do status).
    #[test]
    fn test_line_iter_keeps_the_newest_matches_over_the_limit() {
        let mut bar = SearchBar::new();
        bar.query = "x".into();
        let lines = (0..MAX_MATCHES + 1000).map(|_| "x".to_string());
        let result = bar.search_in_line_iter(lines);
        assert_eq!(result.matches.len(), MAX_MATCHES);
        assert_eq!(result.total_matches, MAX_MATCHES + 1000);
        assert_eq!(result.matches[0].line, 1000);
        assert_eq!(result.matches.last().unwrap().line, MAX_MATCHES + 999);
        bar.result = Some(result);
        assert!(bar.get_status_text().contains(&format!("{MAX_MATCHES}+")));
        // Regex inválida: nenhum resultado.
        bar.options.regex = true;
        bar.query = "(".into();
        assert!(bar
            .search_in_line_iter(std::iter::once("(".to_string()))
            .matches
            .is_empty());
    }

    #[test]
    fn test_search_bar_default_state() {
        let bar = SearchBar::default();
        assert_eq!(bar.state, SearchBarState::Hidden);
        assert!(bar.query.is_empty());
        assert!(bar.result.is_none());
    }

    #[test]
    fn test_search_bar_show_hide() {
        let mut bar = SearchBar::new();

        bar.show();
        assert!(bar.is_visible());
        assert!(!bar.is_active());

        bar.hide();
        assert!(!bar.is_visible());
    }

    #[test]
    fn test_search_bar_activate() {
        let mut bar = SearchBar::new();

        bar.show();
        bar.activate();
        assert!(bar.is_visible());
        assert!(bar.is_active());

        bar.deactivate();
        assert!(bar.is_visible());
        assert!(!bar.is_active());
    }

    #[test]
    fn test_search_simple() {
        let mut bar = SearchBar::new();
        bar.query = "hello".to_string();
        let lines = vec!["hello world", "foo bar", "hello again"];

        let result = bar.search_in_lines(&lines);

        assert_eq!(result.match_count(), 2);
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut bar = SearchBar::new();
        bar.query = "hello".to_string();
        bar.options.case_sensitive = false;

        let lines = vec!["Hello WORLD", "hello world", "HELLO"];

        let result = bar.search_in_lines(&lines);

        assert_eq!(result.match_count(), 3);
    }

    #[test]
    fn test_search_case_sensitive() {
        let mut bar = SearchBar::new();
        bar.query = "hello".to_string();
        bar.options.case_sensitive = true;

        let lines = vec!["Hello WORLD", "hello world", "HELLO"];

        let result = bar.search_in_lines(&lines);

        assert_eq!(result.match_count(), 1);
    }

    #[test]
    fn test_search_whole_word() {
        let mut bar = SearchBar::new();
        bar.query = "hello".to_string();
        bar.options.whole_word = true;

        let lines = vec!["hello world", "shello", "hell", "hello"];

        let result = bar.search_in_lines(&lines);

        eprintln!("DEBUG: whole_word matches={}", result.match_count());
        assert_eq!(result.match_count(), 2);
    }

    #[test]
    fn test_search_navigation() {
        let mut bar = SearchBar::new();
        bar.query = "aaa".to_string();
        let lines = vec!["aaa bbb aaa", "aaa", "bbb aaa"];

        let mut result = bar.search_in_lines(&lines);

        eprintln!("DEBUG: navigation matches={}", result.match_count());
        assert_eq!(result.match_count(), 4);

        result.next_match();
        assert_eq!(result.current_match_index, 1);

        result.next_match();
        assert_eq!(result.current_match_index, 2);

        result.previous_match();
        assert_eq!(result.current_match_index, 1);
    }

    #[test]
    fn test_search_no_matches() {
        let bar = SearchBar::new();
        let lines = vec!["hello world"];

        let result = bar.search_in_lines(&lines);

        assert!(result.matches.is_empty());
    }

    #[test]
    fn test_search_empty_query() {
        let bar = SearchBar::new();
        let lines = vec!["hello world"];

        let result = bar.search_in_lines(&lines);

        assert!(result.matches.is_empty());
    }

    #[test]
    fn test_search_options_toggle() {
        let mut bar = SearchBar::new();

        assert!(!bar.options.case_sensitive);

        bar.toggle_case_sensitive();
        assert!(bar.options.case_sensitive);

        bar.toggle_whole_word();
        assert!(bar.options.whole_word);

        bar.toggle_regex();
        assert!(bar.options.regex);
    }

    #[test]
    fn test_incremental_search_empty_to_text() {
        let mut bar = SearchBar::new();

        let lines = vec!["hello world".to_string(), "test foo".to_string()];

        let changed = bar.search_incremental("hello", &lines);

        assert!(changed);
        assert_eq!(bar.query, "hello");
        assert!(bar.has_active_results());
    }

    #[test]
    fn test_incremental_search_same_query() {
        let mut bar = SearchBar::new();

        let lines = vec!["hello world".to_string()];

        bar.search_incremental("hello", &lines);
        let changed = bar.search_incremental("hello", &lines);

        assert!(!changed);
    }

    #[test]
    fn test_incremental_search_empty_query() {
        let mut bar = SearchBar::new();

        let lines = vec!["hello world".to_string()];

        bar.search_incremental("hello", &lines);
        assert!(bar.has_active_results());

        bar.search_incremental("", &lines);

        assert!(bar.query.is_empty());
        assert!(!bar.has_active_results());
    }

    #[test]
    fn test_incremental_search_no_results() {
        let mut bar = SearchBar::new();

        let lines = vec!["hello world".to_string()];

        bar.search_incremental("xyz", &lines);

        assert!(!bar.has_active_results());
        assert!(bar.result.as_ref().unwrap().matches.is_empty());
    }

    #[test]
    fn test_search_now() {
        let mut bar = SearchBar::new();
        bar.query = "foo".to_string();

        let lines = vec!["foo bar baz".to_string(), "test foo".to_string()];

        let result = bar.search_now(&lines);

        assert_eq!(result.match_count(), 2);
        assert_eq!(bar.query, "foo");
    }

    #[test]
    fn test_current_match_index() {
        let mut bar = SearchBar::new();
        bar.query = "aaa".to_string();

        let lines = vec!["aaa bbb aaa".to_string()];

        bar.search_now(&lines);

        assert_eq!(bar.current_match_index(), Some(0));

        bar.result.as_mut().unwrap().next_match();

        assert_eq!(bar.current_match_index(), Some(1));
    }

    #[test]
    fn test_clear_search() {
        let mut bar = SearchBar::new();

        let lines = vec!["hello".to_string()];

        bar.search_incremental("hello", &lines);
        assert!(bar.is_visible());

        bar.clear();

        assert!(bar.query.is_empty());
        assert!(!bar.is_visible());
        assert!(!bar.has_active_results());
    }

    #[test]
    fn test_reset_keep_query() {
        let mut bar = SearchBar::new();

        let lines = vec!["hello world".to_string()];

        bar.search_incremental("hello", &lines);
        bar.activate();

        bar.reset_keep_query();

        assert_eq!(bar.query, "hello");
        assert!(bar.is_visible());
        assert!(!bar.is_active());
    }

    #[test]
    fn test_incremental_search_updates_options() {
        let mut bar = SearchBar::new();

        let lines = vec!["Hello WORLD".to_string(), "hello world".to_string()];

        bar.search_incremental("hello", &lines);
        let case_insensitive_count = bar.result.as_ref().unwrap().match_count();

        bar.toggle_case_sensitive();
        bar.search_incremental("hello", &lines);
        let case_sensitive_count = bar.result.as_ref().unwrap().match_count();

        assert!(case_insensitive_count > case_sensitive_count);
    }
}
