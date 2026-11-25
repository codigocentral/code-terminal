//! Quick Connect UI - Interface para conexão rápida SSH
//!
//! Este módulo fornece componentes de UI para o diálogo de Quick Connect,
//! permitindo que usuários se conectem rapidamente a hosts SSH.

use crate::ssh::{
    FilterMode, QuickConnectDialog, QuickConnectField, QuickConnectParams, QuickConnectRecent,
    QuickConnectSelection, SshHost, SshManager,
};
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Componente de UI para o Quick Connect
pub struct QuickConnectUI {
    /// Diálogo de Quick Connect (lógica)
    pub dialog: QuickConnectDialog,
    /// Gerenciador SSH
    ssh_manager: Arc<RwLock<SshManager>>,
    /// Callback quando uma conexão é confirmada
    on_connect: Option<Box<dyn Fn(&QuickConnectParams) + Send + Sync>>,
    /// Callback quando o diálogo é cancelado
    on_cancel: Option<Box<dyn Fn() + Send + Sync>>,
    /// Visibilidade da UI
    visible: bool,
    /// Primeiro item visível da lista na última vez que foi desenhada
    /// (rolagem lembrada entre frames, como na paleta).
    first_visible: usize,
}

impl std::fmt::Debug for QuickConnectUI {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickConnectUI")
            .field("dialog", &self.dialog)
            .field("visible", &self.visible)
            .finish_non_exhaustive()
    }
}

impl QuickConnectUI {
    /// Cria uma nova UI de Quick Connect
    pub fn new(ssh_manager: Arc<RwLock<SshManager>>) -> Self {
        Self {
            dialog: QuickConnectDialog::new(),
            ssh_manager,
            on_connect: None,
            on_cancel: None,
            visible: false,
            first_visible: 0,
        }
    }

    /// Define o callback de conexão
    pub fn on_connect<F>(&mut self, callback: F)
    where
        F: Fn(&QuickConnectParams) + Send + Sync + 'static,
    {
        self.on_connect = Some(Box::new(callback));
    }

    /// Define o callback de cancelamento
    pub fn on_cancel<F>(&mut self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.on_cancel = Some(Box::new(callback));
    }

    /// Abre o diálogo de Quick Connect
    pub fn show(&mut self) {
        self.dialog.show();
        self.visible = true;
        self.first_visible = 0;
        self.update_filter();
        info!("Quick Connect UI opened");
    }

    /// Fecha o diálogo
    pub fn hide(&mut self) {
        self.dialog.hide();
        self.visible = false;
        if let Some(ref callback) = self.on_cancel {
            callback();
        }
        debug!("Quick Connect UI closed");
    }

    /// Verifica se a UI está visível
    pub fn is_visible(&self) -> bool {
        self.visible && self.dialog.is_visible()
    }

    /// Verifica se o diálogo está ativo (recebendo input)
    pub fn is_active(&self) -> bool {
        self.dialog.is_active()
    }

    /// Ativa o diálogo
    pub fn activate(&mut self) {
        self.dialog.activate();
    }

    /// Desativa o diálogo
    pub fn deactivate(&mut self) {
        self.dialog.deactivate();
    }

    /// Processa entrada de texto
    pub fn type_char(&mut self, c: char) {
        self.dialog.type_char(c);
        self.update_filter();
    }

    /// Processa backspace
    pub fn backspace(&mut self) {
        self.dialog.backspace();
        self.update_filter();
    }

    /// Processa tecla Tab (próximo campo)
    pub fn next_field(&mut self) {
        self.dialog.next_field();
    }

    /// Processa Shift+Tab (campo anterior)
    pub fn previous_field(&mut self) {
        self.dialog.previous_field();
    }

    /// Move seleção para cima
    pub fn select_up(&mut self) {
        self.dialog.select_up();
    }

    /// Move seleção para baixo
    pub fn select_down(&mut self) {
        self.dialog.select_down();
    }

    /// Primeiro item visível da lista (rolagem do último desenho).
    pub fn first_visible(&self) -> usize {
        self.first_visible
    }

    /// Guarda a rolagem que o desenho usou.
    pub fn set_first_visible(&mut self, first: usize) {
        self.first_visible = first;
    }

    /// Seleciona o item `index` da lista (recentes e depois hosts).
    /// Índice fora da lista é ignorado.
    pub fn select_index(&mut self, index: usize) {
        let total = self.dialog.filtered_recent.len() + self.dialog.filtered_hosts.len();
        if index < total {
            self.dialog.selection_index = index;
        }
    }

    /// Aplica a seleção atual aos campos
    pub fn apply_selection(&mut self) {
        self.dialog.apply_selection();
    }

    /// Valida os campos atuais e retorna os parâmetros de conexão.
    pub fn validate(&self) -> Result<QuickConnectParams, crate::ssh::QuickConnectError> {
        self.dialog.validate()
    }

    /// Define a mensagem de erro exibida no diálogo.
    pub fn set_error_message(&mut self, message: impl Into<String>) {
        self.dialog.set_error(message.into());
    }

    /// Registra uma conexão concluída e fecha o diálogo.
    pub fn complete_connection(&mut self, params: &QuickConnectParams) {
        info!(
            "Quick Connect: connecting to {}",
            params.connection_string()
        );

        // Adiciona às conexões recentes
        let manager = self.ssh_manager.read();
        let recent = QuickConnectRecent::new(&params.connection_string(), params.port);
        manager.add_recent_connection(recent.into());
        drop(manager);

        // Chama o callback
        if let Some(ref callback) = self.on_connect {
            callback(params);
        }

        self.hide();
    }

    /// Confirma a conexão
    pub fn confirm(&mut self) -> Option<QuickConnectParams> {
        match self.validate() {
            Ok(params) => {
                self.complete_connection(&params);
                Some(params)
            }
            Err(e) => {
                warn!("Quick Connect validation failed: {}", e);
                self.set_error_message(e.to_string());
                None
            }
        }
    }

    /// Cancela o diálogo
    pub fn cancel(&mut self) {
        self.hide();
    }

    /// Preenche com uma string de conexão (user@host:port)
    pub fn parse_and_fill(&mut self, input: &str) {
        self.dialog.parse_and_fill(input);
        self.update_filter();
    }

    /// Alterna o modo de filtro
    pub fn toggle_filter_mode(&mut self) {
        self.dialog.toggle_filter_mode();
        self.update_filter();
    }

    /// Retorna o campo atualmente em foco
    pub fn focused_field(&self) -> QuickConnectField {
        self.dialog.focused_field
    }

    /// Retorna o valor de um campo
    pub fn get_field(&self, field: QuickConnectField) -> &str {
        self.dialog.get_field(field)
    }

    /// Define o valor de um campo
    pub fn set_field(&mut self, field: QuickConnectField, value: &str) {
        self.dialog.set_field(field, value);
        self.update_filter();
    }

    /// Limpa todos os campos
    pub fn clear_fields(&mut self) {
        self.dialog.clear_fields();
    }

    /// Retorna os resultados filtrados (recentes)
    pub fn filtered_recent(&self) -> &[QuickConnectRecent] {
        &self.dialog.filtered_recent
    }

    /// Retorna os resultados filtrados (hosts)
    pub fn filtered_hosts(&self) -> &[SshHost] {
        &self.dialog.filtered_hosts
    }

    /// Retorna o índice de seleção atual
    pub fn selection_index(&self) -> usize {
        self.dialog.selection_index
    }

    /// Retorna o item selecionado
    pub fn get_selected(&self) -> Option<QuickConnectSelection> {
        self.dialog.get_selected()
    }

    /// Retorna a mensagem de erro (se houver)
    pub fn error_message(&self) -> Option<&str> {
        self.dialog.error_message.as_deref()
    }

    /// Retorna o modo de filtro atual
    pub fn filter_mode(&self) -> FilterMode {
        self.dialog.filter_mode
    }

    /// Retorna o número total de resultados
    pub fn total_results(&self) -> usize {
        self.dialog.total_results()
    }

    /// Verifica se há resultados
    pub fn has_results(&self) -> bool {
        self.dialog.has_results()
    }

    /// Atualiza os resultados do filtro
    fn update_filter(&mut self) {
        let manager = self.ssh_manager.read();

        // Obtém conexões recentes
        let recent: Vec<QuickConnectRecent> = manager
            .get_recent_connections()
            .into_iter()
            .map(|r| QuickConnectRecent {
                connection_string: format!("{}@{}", r.user.as_deref().unwrap_or(""), r.host),
                hostname: r.host,
                user: r.user,
                port: r.port,
                identity_file: None,
                last_used: chrono::Utc::now(),
                use_count: 1,
            })
            .collect();

        // Obtém todos os hosts
        let hosts = manager.get_all_hosts();
        drop(manager);

        // Atualiza o filtro no diálogo
        self.dialog.update_filter(&recent, &hosts);
    }

    /// Preenche os campos a partir de um host selecionado
    pub fn fill_from_host(&mut self, host: &SshHost) {
        self.set_field(QuickConnectField::Host, host.effective_host());
        if let Some(user) = host.effective_user() {
            self.set_field(QuickConnectField::User, user);
        }
        if host.effective_port() != 22 {
            self.set_field(QuickConnectField::Port, &host.effective_port().to_string());
        }
        if let Some(ref identity) = host.identity_file {
            self.set_field(
                QuickConnectField::IdentityFile,
                &identity.display().to_string(),
            );
        }
    }

    /// Retorna sugestões de autocompletar baseadas na query atual
    pub fn get_suggestions(&self, query: &str) -> Vec<String> {
        if query.is_empty() {
            return Vec::new();
        }

        let query_lower = query.to_lowercase();
        let mut suggestions = Vec::new();

        // Adiciona hosts que começam com a query
        for host in &self.dialog.filtered_hosts {
            let pattern_lower = host.pattern.to_lowercase();
            if pattern_lower.starts_with(&query_lower) && pattern_lower != query_lower {
                suggestions.push(host.pattern.clone());
            }
        }

        // Adiciona conexões recentes que começam com a query
        for recent in &self.dialog.filtered_recent {
            let conn_lower = recent.connection_string.to_lowercase();
            if conn_lower.starts_with(&query_lower) && conn_lower != query_lower {
                suggestions.push(recent.connection_string.clone());
            }
        }

        // Limita a 5 sugestões
        suggestions.truncate(5);
        suggestions
    }

    /// Executa uma ação baseada em uma tecla
    /// Retorna true se a tecla foi consumida
    pub fn handle_key(&mut self, key: QuickConnectKey) -> bool {
        match key {
            QuickConnectKey::Char(c) => {
                self.type_char(c);
                true
            }
            QuickConnectKey::Backspace => {
                self.backspace();
                true
            }
            QuickConnectKey::Tab => {
                self.next_field();
                true
            }
            QuickConnectKey::ShiftTab => {
                self.previous_field();
                true
            }
            QuickConnectKey::Up => {
                self.select_up();
                true
            }
            QuickConnectKey::Down => {
                self.select_down();
                true
            }
            QuickConnectKey::Enter => {
                self.confirm();
                true
            }
            QuickConnectKey::Escape => {
                self.cancel();
                true
            }
            QuickConnectKey::Space => {
                self.type_char(' ');
                true
            }
        }
    }
}

impl Default for QuickConnectUI {
    fn default() -> Self {
        Self::new(Arc::new(RwLock::new(SshManager::new())))
    }
}

/// Teclas suportadas pelo Quick Connect
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickConnectKey {
    /// Caractere digitado
    Char(char),
    /// Backspace
    Backspace,
    /// Tab
    Tab,
    /// Shift+Tab
    ShiftTab,
    /// Seta para cima
    Up,
    /// Seta para baixo
    Down,
    /// Enter
    Enter,
    /// Escape
    Escape,
    /// Espaço
    Space,
}

/// Renderizador do Quick Connect
///
/// Esta trait deve ser implementada pelo renderizador da aplicação
/// para desenhar o diálogo de Quick Connect na tela.
pub trait QuickConnectRenderer {
    /// Renderiza o diálogo de Quick Connect
    fn render_quick_connect(&mut self, ui: &QuickConnectUI, width: u32, height: u32);
}

/// Estado de renderização do Quick Connect
#[derive(Debug, Clone)]
pub struct QuickConnectRenderState {
    /// Posição X do diálogo
    pub x: f32,
    /// Posição Y do diálogo
    pub y: f32,
    /// Largura do diálogo
    pub width: f32,
    /// Altura do diálogo
    pub height: f32,
    /// Altura de cada linha de campo
    pub field_height: f32,
    /// Altura da lista de resultados
    pub results_height: f32,
    /// Padding interno
    pub padding: f32,
}

impl Default for QuickConnectRenderState {
    fn default() -> Self {
        Self {
            x: 100.0,
            y: 100.0,
            width: 500.0,
            height: 400.0,
            field_height: 40.0,
            results_height: 200.0,
            padding: 16.0,
        }
    }
}

impl QuickConnectRenderState {
    /// Cria um novo estado de renderização centralizado
    pub fn centered(screen_width: u32, screen_height: u32, width: f32, height: f32) -> Self {
        Self {
            x: (screen_width as f32 - width) / 2.0,
            y: (screen_height as f32 - height) / 2.0,
            width,
            height,
            field_height: 40.0,
            results_height: 200.0,
            padding: 16.0,
        }
    }

    /// Retorna a posição Y de um campo específico
    pub fn field_y(&self, field_index: usize) -> f32 {
        self.y + self.padding + (field_index as f32 * (self.field_height + 8.0))
    }

    /// Retorna a posição Y da lista de resultados
    pub fn results_y(&self) -> f32 {
        self.y + self.padding + (4.0 * (self.field_height + 8.0)) + 16.0
    }

    /// Retorna a largura do campo de input
    pub fn input_width(&self) -> f32 {
        self.width - (2.0 * self.padding) - 100.0 // 100px para o label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quick_connect_ui_creation() {
        let ui = QuickConnectUI::default();
        assert!(!ui.is_visible());
        assert!(!ui.is_active());
    }

    #[test]
    fn test_quick_connect_ui_show_hide() {
        let mut ui = QuickConnectUI::default();

        ui.show();
        assert!(ui.is_visible());

        ui.hide();
        assert!(!ui.is_visible());
    }

    #[test]
    fn test_quick_connect_ui_typing() {
        let mut ui = QuickConnectUI::default();
        ui.show();

        ui.type_char('h');
        ui.type_char('o');
        ui.type_char('s');
        ui.type_char('t');

        assert_eq!(ui.get_field(QuickConnectField::Host), "host");

        ui.backspace();
        assert_eq!(ui.get_field(QuickConnectField::Host), "hos");
    }

    #[test]
    fn test_quick_connect_ui_field_navigation() {
        let mut ui = QuickConnectUI::default();
        ui.show();

        assert_eq!(ui.focused_field(), QuickConnectField::Host);

        ui.next_field();
        assert_eq!(ui.focused_field(), QuickConnectField::User);

        ui.next_field();
        assert_eq!(ui.focused_field(), QuickConnectField::Port);

        ui.previous_field();
        assert_eq!(ui.focused_field(), QuickConnectField::User);
    }

    #[test]
    fn test_quick_connect_ui_handle_key() {
        let mut ui = QuickConnectUI::default();
        ui.show();

        // Digita "test"
        ui.handle_key(QuickConnectKey::Char('t'));
        ui.handle_key(QuickConnectKey::Char('e'));
        ui.handle_key(QuickConnectKey::Char('s'));
        ui.handle_key(QuickConnectKey::Char('t'));

        assert_eq!(ui.get_field(QuickConnectField::Host), "test");

        // Backspace
        ui.handle_key(QuickConnectKey::Backspace);
        assert_eq!(ui.get_field(QuickConnectField::Host), "tes");

        // Tab navega para próximo campo
        ui.handle_key(QuickConnectKey::Tab);
        assert_eq!(ui.focused_field(), QuickConnectField::User);
    }

    #[test]
    fn test_quick_connect_render_state() {
        let state = QuickConnectRenderState::centered(1024, 768, 500.0, 400.0);

        assert_eq!(state.width, 500.0);
        assert_eq!(state.height, 400.0);

        // Verifica se está centralizado
        let expected_x = (1024.0 - 500.0) / 2.0;
        let expected_y = (768.0 - 400.0) / 2.0;
        assert_eq!(state.x, expected_x);
        assert_eq!(state.y, expected_y);
    }

    #[test]
    fn test_quick_connect_render_state_positions() {
        let state = QuickConnectRenderState::default();

        // Campo 0 (Host) deve estar no topo + padding
        let field_0_y = state.field_y(0);
        assert_eq!(field_0_y, state.y + state.padding);

        // Campo 1 (User) deve estar abaixo do campo 0
        let field_1_y = state.field_y(1);
        assert_eq!(
            field_1_y,
            state.y + state.padding + state.field_height + 8.0
        );
    }

    #[test]
    fn test_fill_from_host() {
        let mut ui = QuickConnectUI::default();
        ui.show();

        let mut host = SshHost::new("myserver");
        host.host_name = Some("192.168.1.100".to_string());
        host.user = Some("admin".to_string());
        host.port = Some(2222);

        ui.fill_from_host(&host);

        assert_eq!(ui.get_field(QuickConnectField::Host), "192.168.1.100");
        assert_eq!(ui.get_field(QuickConnectField::User), "admin");
        assert_eq!(ui.get_field(QuickConnectField::Port), "2222");
    }

    #[test]
    fn test_get_suggestions() {
        let mut ui = QuickConnectUI::default();
        ui.show();

        // Adiciona alguns dados para filtrar
        ui.set_field(QuickConnectField::Host, "ser");

        // Sem hosts na lista, não deve haver sugestões
        let suggestions = ui.get_suggestions("ser");
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_quick_connect_key_variants() {
        // Verifica se todos os tipos de tecla funcionam
        assert_eq!(QuickConnectKey::Char('a'), QuickConnectKey::Char('a'));
        assert_eq!(QuickConnectKey::Enter, QuickConnectKey::Enter);
        assert_eq!(QuickConnectKey::Escape, QuickConnectKey::Escape);
    }

    #[test]
    fn test_callbacks() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let mut ui = QuickConnectUI::default();

        let connected = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::new(AtomicBool::new(false));

        let connected_clone = Arc::clone(&connected);
        let cancelled_clone = Arc::clone(&cancelled);

        ui.on_connect(move |_| {
            connected_clone.store(true, Ordering::SeqCst);
        });

        ui.on_cancel(move || {
            cancelled_clone.store(true, Ordering::SeqCst);
        });

        // Testa cancelamento
        ui.show();
        ui.cancel();
        assert!(cancelled.load(Ordering::SeqCst));
        assert!(!ui.is_visible());
    }
}
