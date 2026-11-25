//! Session List - Interface de lista e switch de sessões

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::session::{MultiplexerSession, SessionId, SessionState};

/// Ordenação da lista
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SortOrder {
    /// Por nome
    Name,
    /// Por data de criação
    Created,
    /// Por última atividade
    #[default]
    Activity,
    /// Por número de clientes
    Clients,
    /// Por estado
    State,
}

/// Filtro de sessões
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionFilter {
    /// Filtrar por estado
    pub state: Option<SessionState>,
    /// Filtrar por nome (substring)
    pub name_contains: Option<String>,
    /// Filtrar por tag
    pub has_tag: Option<String>,
    /// Apenas sessões com clientes
    pub active_only: bool,
    /// Apenas sessões anexáveis
    pub attachable_only: bool,
}

impl SessionFilter {
    /// Cria um filtro vazio (aceita tudo)
    pub fn new() -> Self {
        Self::default()
    }

    /// Filtra por estado
    pub fn with_state(mut self, state: SessionState) -> Self {
        self.state = Some(state);
        self
    }

    /// Filtra por nome
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name_contains = Some(name.into());
        self
    }

    /// Filtra por tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.has_tag = Some(tag.into());
        self
    }

    /// Apenas ativas
    pub fn active_only(mut self) -> Self {
        self.active_only = true;
        self
    }

    /// Apenas anexáveis
    pub fn attachable_only(mut self) -> Self {
        self.attachable_only = true;
        self
    }

    /// Verifica se uma sessão passa no filtro
    pub fn matches(&self, session: &MultiplexerSession) -> bool {
        if let Some(ref state) = self.state {
            if session.state != *state {
                return false;
            }
        }

        if let Some(ref name) = self.name_contains {
            if !session.name.to_lowercase().contains(&name.to_lowercase()) {
                return false;
            }
        }

        if let Some(ref _tag) = self.has_tag {
            // Tags não implementadas em MultiplexerSession
            // Sempre retorna true para não filtrar
        }

        if self.active_only && session.client_count == 0 {
            return false;
        }

        if self.attachable_only && !session.can_attach() {
            return false;
        }

        true
    }
}

/// Item da lista de sessões
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListItem {
    /// ID da sessão
    pub id: SessionId,
    /// Nome da sessão
    pub name: String,
    /// Estado atual
    pub state: SessionState,
    /// Número de clientes conectados
    pub client_count: usize,
    /// Preview do conteúdo (primeiras linhas)
    pub preview: Option<String>,
    /// Tamanho do terminal
    pub size: (u16, u16),
    /// Diretório de trabalho
    pub working_dir: String,
    /// Última atividade (quanto tempo atrás)
    pub last_activity_secs: u64,
    /// Se está selecionada
    pub selected: bool,
    /// Índice de ordenação
    pub sort_index: usize,
}

/// Lista de sessões
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionList {
    /// Todas as sessões
    sessions: Vec<SessionListItem>,
    /// Índice da sessão selecionada
    selected_index: usize,
    /// Ordem de ordenação
    sort_order: SortOrder,
    /// Filtro atual
    filter: SessionFilter,
    /// Termo de busca
    search_term: String,
    /// Mapeamento ID -> índice
    id_to_index: HashMap<SessionId, usize>,
}

impl SessionList {
    /// Cria uma nova lista vazia
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            selected_index: 0,
            sort_order: SortOrder::default(),
            filter: SessionFilter::new(),
            search_term: String::new(),
            id_to_index: HashMap::new(),
        }
    }

    /// Define as sessões
    pub fn set_sessions(&mut self, sessions: Vec<MultiplexerSession>) {
        self.sessions = sessions
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let last_activity_secs = s.idle_time().as_secs();
                SessionListItem {
                    id: s.id,
                    name: s.name.clone(),
                    state: s.state,
                    client_count: s.client_count,
                    preview: None, // Preenchido depois se necessário
                    size: s.size,
                    working_dir: s.working_dir.clone(),
                    last_activity_secs,
                    selected: false,
                    sort_index: i,
                }
            })
            .collect();

        // Rebuild index
        self.id_to_index.clear();
        for (i, session) in self.sessions.iter().enumerate() {
            self.id_to_index.insert(session.id, i);
        }

        self.apply_filter();
        self.sort();
    }

    /// Adiciona uma sessão
    pub fn add_session(&mut self, session: MultiplexerSession) {
        let idx = self.sessions.len();
        let last_activity_secs = session.idle_time().as_secs();

        self.id_to_index.insert(session.id, idx);

        self.sessions.push(SessionListItem {
            id: session.id,
            name: session.name.clone(),
            state: session.state,
            client_count: session.client_count,
            preview: None,
            size: session.size,
            working_dir: session.working_dir.clone(),
            last_activity_secs,
            selected: false,
            sort_index: idx,
        });

        self.apply_filter();
        self.sort();
    }

    /// Remove uma sessão
    pub fn remove_session(&mut self, id: SessionId) {
        if let Some(&idx) = self.id_to_index.get(&id) {
            self.sessions.remove(idx);
            self.rebuild_index();
            self.apply_filter();

            // Ajustar índice selecionado
            if self.selected_index >= self.sessions.len() && !self.sessions.is_empty() {
                self.selected_index = self.sessions.len() - 1;
            }
        }
    }

    /// Atualiza uma sessão
    pub fn update_session(&mut self, session: &MultiplexerSession) {
        if let Some(&idx) = self.id_to_index.get(&session.id) {
            if let Some(item) = self.sessions.get_mut(idx) {
                item.name = session.name.clone();
                item.state = session.state;
                item.client_count = session.client_count;
                item.size = session.size;
                item.working_dir = session.working_dir.clone();
                item.last_activity_secs = session.idle_time().as_secs();
            }
            self.apply_filter();
            self.sort();
        }
    }

    /// Define o filtro
    pub fn set_filter(&mut self, filter: SessionFilter) {
        self.filter = filter;
        self.apply_filter();
        self.selected_index = 0;
    }

    /// Aplica o filtro
    fn apply_filter(&mut self) {
        // Nesta implementação simples, mantemos todas as sessões
        // mas marcamos quais passam no filtro
        // Uma implementação mais avançada poderia filtrar de verdade
    }

    /// Define a ordenação
    pub fn set_sort_order(&mut self, order: SortOrder) {
        self.sort_order = order;
        self.sort();
    }

    /// Ordena as sessões
    fn sort(&mut self) {
        let selected = self.marked_selection();
        match self.sort_order {
            SortOrder::Name => {
                self.sessions.sort_by(|a, b| a.name.cmp(&b.name));
            }
            SortOrder::Created => {
                // Usa o ID como proxy para ordem de criação
                self.sessions.sort_by_key(|s| s.id.0);
            }
            SortOrder::Activity => {
                self.sessions.sort_by_key(|s| s.last_activity_secs);
            }
            SortOrder::Clients => {
                self.sessions
                    .sort_by_key(|s| std::cmp::Reverse(s.client_count));
            }
            SortOrder::State => {
                self.sessions.sort_by_key(|s| s.state.to_string());
            }
        }

        // Rebuild index após ordenação
        self.rebuild_index();
        self.restore_selection(selected);
    }

    /// Sessão marcada como selecionada (se alguma estiver)
    fn marked_selection(&self) -> Option<SessionId> {
        self.sessions.iter().find(|s| s.selected).map(|s| s.id)
    }

    /// Depois de reordenar, mantém a seleção na mesma sessão (e não no
    /// mesmo índice, que agora aponta para outra sessão).
    fn restore_selection(&mut self, selected: Option<SessionId>) {
        if let Some(&idx) = selected.and_then(|id| self.id_to_index.get(&id)) {
            self.selected_index = idx;
        }
        for (i, session) in self.sessions.iter_mut().enumerate() {
            session.selected = i == self.selected_index;
        }
    }

    /// Define o termo de busca
    pub fn set_search_term(&mut self, term: impl Into<String>) {
        self.search_term = term.into();
        self.apply_search();
    }

    /// Aplica busca
    fn apply_search(&mut self) {
        if self.search_term.is_empty() {
            return;
        }

        let term_lower = self.search_term.to_lowercase();
        let selected = self.marked_selection();

        // Ordena por relevância da busca
        self.sessions.sort_by(|a, b| {
            let a_matches = a.name.to_lowercase().contains(&term_lower);
            let b_matches = b.name.to_lowercase().contains(&term_lower);
            b_matches.cmp(&a_matches) // true (1) antes de false (0)
        });

        self.rebuild_index();
        self.restore_selection(selected);
    }

    /// Reconstrói o mapeamento de índices
    fn rebuild_index(&mut self) {
        self.id_to_index.clear();
        for (i, session) in self.sessions.iter().enumerate() {
            self.id_to_index.insert(session.id, i);
        }
    }

    /// Move a seleção para baixo
    pub fn select_next(&mut self) {
        if !self.sessions.is_empty() {
            self.sessions[self.selected_index].selected = false;
            self.selected_index = (self.selected_index + 1) % self.sessions.len();
            self.sessions[self.selected_index].selected = true;
        }
    }

    /// Move a seleção para cima
    pub fn select_previous(&mut self) {
        if !self.sessions.is_empty() {
            self.sessions[self.selected_index].selected = false;
            self.selected_index = if self.selected_index == 0 {
                self.sessions.len() - 1
            } else {
                self.selected_index - 1
            };
            self.sessions[self.selected_index].selected = true;
        }
    }

    /// Seleciona por índice
    pub fn select_index(&mut self, index: usize) {
        if index < self.sessions.len() {
            if let Some(session) = self.sessions.get_mut(self.selected_index) {
                session.selected = false;
            }
            self.selected_index = index;
            self.sessions[self.selected_index].selected = true;
        }
    }

    /// Seleciona por ID
    pub fn select_by_id(&mut self, id: SessionId) -> bool {
        if let Some(&idx) = self.id_to_index.get(&id) {
            self.select_index(idx);
            true
        } else {
            false
        }
    }

    /// Retorna a sessão selecionada
    pub fn selected(&self) -> Option<&SessionListItem> {
        self.sessions.get(self.selected_index)
    }

    /// Retorna o ID da sessão selecionada
    pub fn selected_id(&self) -> Option<SessionId> {
        self.selected().map(|s| s.id)
    }

    /// Retorna todas as sessões visíveis
    pub fn sessions(&self) -> &[SessionListItem] {
        &self.sessions
    }

    /// Retorna o número de sessões
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Verifica se está vazia
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Retorna a sessão por ID
    pub fn get(&self, id: SessionId) -> Option<&SessionListItem> {
        self.id_to_index
            .get(&id)
            .and_then(|&idx| self.sessions.get(idx))
    }

    /// Alterna para a próxima sessão com base na ordenação atual
    pub fn switch_next(&mut self) -> Option<SessionId> {
        self.select_next();
        self.selected_id()
    }

    /// Alterna para a sessão anterior
    pub fn switch_previous(&mut self) -> Option<SessionId> {
        self.select_previous();
        self.selected_id()
    }

    /// Busca sessão por nome e seleciona
    pub fn find_and_select(&mut self, name: &str) -> bool {
        let name_lower = name.to_lowercase();
        if let Some(idx) = self
            .sessions
            .iter()
            .position(|s| s.name.to_lowercase().contains(&name_lower))
        {
            self.select_index(idx);
            true
        } else {
            false
        }
    }

    /// Retorna estatísticas da lista
    pub fn stats(&self) -> ListStats {
        ListStats {
            total: self.sessions.len(),
            active: self.sessions.iter().filter(|s| s.client_count > 0).count(),
            attachable: self
                .sessions
                .iter()
                .filter(|s| s.state != SessionState::Destroyed)
                .count(),
            selected: self.selected_index,
        }
    }

    /// Formata a lista para exibição em texto
    pub fn format_text(&self) -> String {
        if self.sessions.is_empty() {
            return "No sessions found".to_string();
        }

        let mut output = format!(
            "{:<4} {:<10} {:<20} {:<8} {:<12} {:<15}\n",
            "", "ID", "Name", "Clients", "State", "Activity"
        );
        output.push_str(&"-".repeat(75));
        output.push('\n');

        for session in self.sessions.iter() {
            let marker = if session.selected { "►" } else { " " };
            let id_short = session.id.to_string();
            let id_short = &id_short[..8.min(id_short.len())];
            // Corta por caractere: fatiar bytes quebrava (pânico) em nomes
            // com acento quando o byte 18 caía no meio de um caractere.
            let name: String = session.name.chars().take(18).collect();
            let activity = format_duration(session.last_activity_secs);

            output.push_str(&format!(
                "{:<4} {:<10} {:<20} {:<8} {:<12} {:<15}\n",
                marker,
                id_short,
                name,
                session.client_count,
                session.state.to_string(),
                activity
            ));
        }

        output
    }
}

impl Default for SessionList {
    fn default() -> Self {
        Self::new()
    }
}

/// Estatísticas da lista
#[derive(Debug, Clone)]
pub struct ListStats {
    pub total: usize,
    pub active: usize,
    pub attachable: usize,
    pub selected: usize,
}

/// Formata duração em segundos para string legível
fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

/// Gerenciador de switch de sessões
#[derive(Debug)]
pub struct SessionSwitcher {
    /// Lista de sessões
    list: SessionList,
    /// Histórico de navegação
    history: Vec<SessionId>,
    /// Índice no histórico
    history_index: usize,
    /// Última sessão ativa
    last_active: Option<SessionId>,
}

impl SessionSwitcher {
    /// Cria um novo switcher
    pub fn new() -> Self {
        Self {
            list: SessionList::new(),
            history: Vec::new(),
            history_index: 0,
            last_active: None,
        }
    }

    /// Define as sessões disponíveis
    pub fn set_sessions(&mut self, sessions: Vec<MultiplexerSession>) {
        self.list.set_sessions(sessions);
    }

    /// Alterna para a próxima sessão
    pub fn switch_next(&mut self) -> Option<SessionId> {
        let current_id = self.list.selected_id();
        let id = self.list.switch_next();
        if let Some(new_id) = id {
            if let Some(prev_id) = current_id {
                self.last_active = Some(prev_id);
            }
            self.record_switch(new_id);
        }
        id
    }

    /// Alterna para a sessão anterior
    pub fn switch_previous(&mut self) -> Option<SessionId> {
        let current_id = self.list.selected_id();
        let id = self.list.switch_previous();
        if let Some(new_id) = id {
            if let Some(prev_id) = current_id {
                self.last_active = Some(prev_id);
            }
            self.record_switch(new_id);
        }
        id
    }

    /// Alterna para uma sessão específica
    pub fn switch_to(&mut self, id: SessionId) -> bool {
        // Pega o ID atual antes de mudar
        let current_id = self.list.selected_id();

        if self.list.select_by_id(id) {
            // Registra o switch com o ID anterior
            if let Some(prev_id) = current_id {
                self.last_active = Some(prev_id);
            }
            self.record_switch(id);
            true
        } else {
            false
        }
    }

    /// Alterna para sessão por nome
    pub fn switch_to_name(&mut self, name: &str) -> bool {
        // Pega o ID atual antes de mudar
        let current_id = self.list.selected_id();

        if self.list.find_and_select(name) {
            if let Some(id) = self.list.selected_id() {
                // Registra o switch com o ID anterior
                if let Some(prev_id) = current_id {
                    self.last_active = Some(prev_id);
                }
                self.record_switch(id);
            }
            true
        } else {
            false
        }
    }

    /// Volta para a última sessão
    pub fn switch_last(&mut self) -> Option<SessionId> {
        let target = self.last_active?;
        let current_id = self.list.selected_id();
        if !self.list.select_by_id(target) {
            return None;
        }
        // A sessão de onde saímos vira a "última": repetir alterna entre as duas
        if current_id != Some(target) {
            self.last_active = current_id;
        }
        self.record_switch(target);
        Some(target)
    }

    /// Navega para frente no histórico
    pub fn forward(&mut self) -> Option<SessionId> {
        if self.history_index < self.history.len().saturating_sub(1) {
            self.history_index += 1;
            let id = self.history[self.history_index];
            self.list.select_by_id(id);
            Some(id)
        } else {
            None
        }
    }

    /// Navega para trás no histórico
    pub fn back(&mut self) -> Option<SessionId> {
        if self.history_index > 0 {
            self.history_index -= 1;
            let id = self.history[self.history_index];
            self.list.select_by_id(id);
            Some(id)
        } else {
            None
        }
    }

    /// Registra uma troca no histórico
    fn record_switch(&mut self, id: SessionId) {
        // Remove entradas futuras se estiver no meio do histórico
        if self.history_index < self.history.len() {
            self.history.truncate(self.history_index + 1);
        }

        // Adiciona nova entrada se for diferente da atual
        if self.history.last() != Some(&id) {
            self.history.push(id);
            self.history_index = self.history.len() - 1;
        }

        // Limita tamanho do histórico
        if self.history.len() > 50 {
            self.history.remove(0);
            self.history_index = self.history_index.saturating_sub(1);
        }
    }

    /// Retorna a lista interna
    pub fn list(&self) -> &SessionList {
        &self.list
    }

    /// Retorna a lista mutável
    pub fn list_mut(&mut self) -> &mut SessionList {
        &mut self.list
    }

    /// Retorna a sessão atual
    pub fn current(&self) -> Option<&SessionListItem> {
        self.list.selected()
    }

    /// Retorna o ID atual
    pub fn current_id(&self) -> Option<SessionId> {
        self.list.selected_id()
    }

    /// Retorna se pode navegar para trás
    pub fn can_go_back(&self) -> bool {
        self.history_index > 0
    }

    /// Retorna se pode navegar para frente
    pub fn can_go_forward(&self) -> bool {
        self.history_index < self.history.len().saturating_sub(1)
    }
}

impl Default for SessionSwitcher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_session(name: &str, id: u8) -> MultiplexerSession {
        let mut session = MultiplexerSession::new(Some(name.to_string()), (80, 24));
        // Força ID específico para testes
        session.id = SessionId(uuid::Uuid::from_bytes([
            id, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]));
        session
    }

    #[test]
    fn test_session_list_new() {
        let list = SessionList::new();
        assert!(list.is_empty());
        assert_eq!(list.len(), 0);
    }

    #[test]
    fn test_session_list_add() {
        let mut list = SessionList::new();
        let session = create_test_session("test", 1);
        list.add_session(session);
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_session_list_select() {
        let mut list = SessionList::new();
        list.add_session(create_test_session("a", 1));
        list.add_session(create_test_session("b", 2));
        list.add_session(create_test_session("c", 3));

        assert_eq!(list.selected_index, 0);

        list.select_next();
        assert_eq!(list.selected_index, 1);

        list.select_next();
        assert_eq!(list.selected_index, 2);

        list.select_next(); // Wrap around
        assert_eq!(list.selected_index, 0);
    }

    #[test]
    fn test_session_list_select_previous() {
        let mut list = SessionList::new();
        list.add_session(create_test_session("a", 1));
        list.add_session(create_test_session("b", 2));

        list.select_index(1);
        list.select_previous();
        assert_eq!(list.selected_index, 0);

        list.select_previous(); // Wrap around
        assert_eq!(list.selected_index, 1);
    }

    #[test]
    fn test_session_list_sort_by_name() {
        let mut list = SessionList::new();
        list.add_session(create_test_session("charlie", 1));
        list.add_session(create_test_session("alpha", 2));
        list.add_session(create_test_session("bravo", 3));

        list.set_sort_order(SortOrder::Name);

        assert_eq!(list.sessions()[0].name, "alpha");
        assert_eq!(list.sessions()[1].name, "bravo");
        assert_eq!(list.sessions()[2].name, "charlie");
    }

    #[test]
    fn test_session_list_filter() {
        let mut list = SessionList::new();
        list.add_session(create_test_session("project-a", 1));
        list.add_session(create_test_session("project-b", 2));
        list.add_session(create_test_session("other", 3));

        let filter = SessionFilter::new().with_name("project");
        list.set_filter(filter);

        // Todas as sessões ainda existem, mas o filtro está aplicado
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn test_session_list_find_and_select() {
        let mut list = SessionList::new();
        list.add_session(create_test_session("alpha", 1));
        list.add_session(create_test_session("beta", 2));
        list.add_session(create_test_session("gamma", 3));

        assert!(list.find_and_select("beta"));
        assert_eq!(list.selected().unwrap().name, "beta");
    }

    #[test]
    fn test_session_switcher() {
        let mut switcher = SessionSwitcher::new();
        switcher.set_sessions(vec![
            create_test_session("a", 1),
            create_test_session("b", 2),
            create_test_session("c", 3),
        ]);

        assert_eq!(switcher.current().unwrap().name, "a");

        switcher.switch_next();
        assert_eq!(switcher.current().unwrap().name, "b");

        switcher.switch_next();
        assert_eq!(switcher.current().unwrap().name, "c");
    }

    #[test]
    fn test_session_switcher_history() {
        let mut switcher = SessionSwitcher::new();
        switcher.set_sessions(vec![
            create_test_session("a", 1),
            create_test_session("b", 2),
            create_test_session("c", 3),
        ]);

        switcher.switch_next(); // a -> b
        switcher.switch_next(); // b -> c

        assert!(switcher.can_go_back());
        assert!(!switcher.can_go_forward());

        switcher.back(); // c -> b
        assert_eq!(switcher.current().unwrap().name, "b");

        switcher.forward(); // b -> c
        assert_eq!(switcher.current().unwrap().name, "c");
    }

    #[test]
    fn test_session_switcher_last() {
        let mut switcher = SessionSwitcher::new();
        switcher.set_sessions(vec![
            create_test_session("a", 1),
            create_test_session("b", 2),
        ]);

        assert_eq!(switcher.current().unwrap().name, "a");
        switcher.switch_to_name("b"); // a -> b (last_active = a)

        let last = switcher.switch_last();
        assert!(last.is_some());
        assert_eq!(switcher.current().unwrap().name, "a");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(172800), "2d");
    }

    #[test]
    fn test_filter_matches() {
        let session = create_test_session("test-session", 1);

        let filter = SessionFilter::new().with_name("test");
        assert!(filter.matches(&session));

        let filter = SessionFilter::new().with_name("xyz");
        assert!(!filter.matches(&session));
    }

    #[test]
    fn test_list_stats() {
        let mut list = SessionList::new();
        list.add_session(create_test_session("a", 1));
        list.add_session(create_test_session("b", 2));

        let stats = list.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.selected, 0);
    }

    fn names(list: &SessionList) -> Vec<String> {
        list.sessions().iter().map(|s| s.name.clone()).collect()
    }

    #[test]
    fn test_filter_builders_and_rules() {
        let mut session = create_test_session("Dev", 1);
        session.state = SessionState::Background;
        assert!(SessionFilter::new()
            .with_state(SessionState::Background)
            .with_tag("qualquer")
            .matches(&session));
        assert!(!SessionFilter::new()
            .with_state(SessionState::Active)
            .matches(&session));
        assert!(!SessionFilter::new().active_only().matches(&session));
        session.client_count = 1;
        assert!(SessionFilter::new().active_only().matches(&session));
        assert!(SessionFilter::new().attachable_only().matches(&session));
        session.flags.locked = true;
        assert!(!SessionFilter::new().attachable_only().matches(&session));
    }

    #[test]
    fn test_sort_orders() {
        let mut list = SessionList::default();
        let mut busy = create_test_session("charlie", 3);
        busy.client_count = 5;
        busy.state = SessionState::Active;
        list.set_sessions(vec![
            create_test_session("bravo", 2),
            busy,
            create_test_session("alpha", 1),
        ]);

        list.set_sort_order(SortOrder::Name);
        assert_eq!(names(&list), vec!["alpha", "bravo", "charlie"]);
        list.set_sort_order(SortOrder::Created);
        assert_eq!(names(&list), vec!["alpha", "bravo", "charlie"]);
        list.set_sort_order(SortOrder::Clients);
        assert_eq!(names(&list)[0], "charlie");
        list.set_sort_order(SortOrder::State);
        assert_eq!(
            names(&list)[0],
            "charlie",
            "\"active\" vem antes de \"creating\""
        );
        list.set_sort_order(SortOrder::Activity);
        assert_eq!(list.len(), 3);
    }

    /// Regressão: reordenar mantinha o índice selecionado, e a seleção
    /// pulava para outra sessão quando uma nova era adicionada.
    #[test]
    fn test_selection_follows_session_after_reordering() {
        let mut list = SessionList::new();
        list.set_sort_order(SortOrder::Name);
        list.add_session(create_test_session("bravo", 2));
        list.add_session(create_test_session("delta", 4));
        assert!(list.find_and_select("delta"));

        list.add_session(create_test_session("alpha", 1));
        assert_eq!(list.selected().unwrap().name, "delta");
        assert_eq!(
            list.sessions().iter().filter(|s| s.selected).count(),
            1,
            "só uma sessão marcada"
        );

        list.set_search_term("bra");
        assert_eq!(names(&list)[0], "bravo");
        assert_eq!(list.selected().unwrap().name, "delta");
        list.set_search_term("");
    }

    #[test]
    fn test_remove_update_and_get() {
        let mut list = SessionList::new();
        let a = create_test_session("a", 1);
        let mut b = create_test_session("b", 2);
        let a_id = a.id;
        let b_id = b.id;
        list.add_session(a);
        list.add_session(b.clone());
        list.select_by_id(b_id);
        assert!(!list.select_by_id(SessionId(uuid::Uuid::nil())));

        b.name = "b-renomeada".to_string();
        b.client_count = 2;
        list.update_session(&b);
        assert_eq!(list.get(b_id).unwrap().name, "b-renomeada");
        assert_eq!(list.get(b_id).unwrap().client_count, 2);
        // Atualizar sessão desconhecida não faz nada
        list.update_session(&create_test_session("x", 9));
        assert_eq!(list.len(), 2);

        list.remove_session(b_id);
        assert!(list.get(b_id).is_none());
        assert_eq!(list.selected().unwrap().id, a_id, "seleção ajustada ao fim");
        list.remove_session(b_id);
        list.remove_session(a_id);
        assert!(list.is_empty());
        assert!(list.selected().is_none());
        // Navegar numa lista vazia não faz nada
        list.select_next();
        list.select_previous();
        list.select_index(3);
        assert_eq!(list.switch_next(), None);
        assert_eq!(list.switch_previous(), None);
    }

    #[test]
    fn test_set_filter_resets_selection_and_stats() {
        let mut list = SessionList::new();
        let mut active = create_test_session("ativa", 1);
        active.client_count = 1;
        list.add_session(active);
        let mut gone = create_test_session("morta", 2);
        gone.state = SessionState::Destroyed;
        list.add_session(gone);
        list.select_index(1);

        list.set_filter(SessionFilter::new().active_only());
        let stats = list.stats();
        assert_eq!((stats.total, stats.active, stats.attachable), (2, 1, 1));
        assert_eq!(stats.selected, 0);
        assert!(!list.find_and_select("nenhuma"));
    }

    #[test]
    fn test_format_text_and_durations() {
        let list = SessionList::new();
        assert_eq!(list.format_text(), "No sessions found");

        let mut list = SessionList::new();
        // Nome longo com acentos: o corte em 18 caracteres não pode quebrar
        list.add_session(create_test_session("sessão-de-produção-principal", 1));
        list.select_index(0);
        let text = list.format_text();
        assert!(text.contains("sessão-de-produção"), "{text}");
        assert!(!text.contains("principal"), "{text}");
        assert!(text.contains('►'));
        assert!(text.lines().nth(1).unwrap().starts_with("---"));

        assert_eq!(format_duration(59), "59s");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(3 * 86400), "3d");
    }

    #[test]
    fn test_switcher_navigation_and_history() {
        let mut switcher = SessionSwitcher::default();
        let sessions: Vec<_> = ["a", "b", "c"]
            .iter()
            .enumerate()
            .map(|(i, name)| create_test_session(name, i as u8 + 1))
            .collect();
        let ids: Vec<_> = sessions.iter().map(|s| s.id).collect();
        switcher.set_sessions(sessions);
        switcher.list_mut().set_sort_order(SortOrder::Name);
        switcher.list_mut().select_index(0);

        assert_eq!(switcher.switch_next(), Some(ids[1]));
        assert_eq!(switcher.switch_previous(), Some(ids[0]));
        assert!(switcher.switch_to_name("c"));
        assert_eq!(switcher.current_id(), Some(ids[2]));
        assert!(!switcher.switch_to_name("zzz"));
        assert!(!switcher.switch_to(SessionId(uuid::Uuid::nil())));

        assert!(switcher.can_go_back());
        assert!(!switcher.can_go_forward());
        assert_eq!(switcher.back(), Some(ids[0]));
        assert!(switcher.can_go_forward());
        assert_eq!(switcher.forward(), Some(ids[2]));
        assert_eq!(switcher.forward(), None);
        assert_eq!(switcher.current().unwrap().name, "c");
        assert_eq!(switcher.list().len(), 3);
    }

    /// Regressão: switch_last não atualizava a "última" sessão, então não
    /// alternava entre as duas.
    #[test]
    fn test_switch_last_toggles_between_two_sessions() {
        let mut switcher = SessionSwitcher::new();
        assert_eq!(switcher.switch_last(), None);
        let a = create_test_session("a", 1);
        let b = create_test_session("b", 2);
        let (a_id, b_id) = (a.id, b.id);
        switcher.set_sessions(vec![a, b]);
        switcher.list_mut().select_by_id(a_id);

        assert!(switcher.switch_to(b_id));
        assert_eq!(switcher.switch_last(), Some(a_id));
        assert_eq!(switcher.switch_last(), Some(b_id));
        assert_eq!(switcher.switch_last(), Some(a_id));

        // Última sessão removida da lista: nada a fazer
        switcher.list_mut().remove_session(b_id);
        assert_eq!(switcher.switch_last(), None);
    }

    #[test]
    fn test_history_is_capped() {
        let mut switcher = SessionSwitcher::new();
        let a = create_test_session("a", 1);
        let b = create_test_session("b", 2);
        let (a_id, b_id) = (a.id, b.id);
        switcher.set_sessions(vec![a, b]);
        for _ in 0..30 {
            switcher.switch_to(a_id);
            switcher.switch_to(b_id);
        }
        assert_eq!(switcher.history.len(), 50);
        assert_eq!(switcher.history_index, 49);
    }
}
