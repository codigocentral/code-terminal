//! Quick Connect UI - Interface para conexão rápida SSH
//!
//! Este módulo fornece componentes para interface de conexão rápida a hosts SSH,
//! com suporte a fuzzy search, histórico de conexões e parsing de strings
//! no formato `user@host:port`.
//!
//! # Exemplo de uso
//!
//! ```rust
//! use code_terminal::ssh::{QuickConnectDialog, QuickConnectRecent};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Parse de string de conexão
//! let mut dialog = QuickConnectDialog::new();
//! dialog.parse_and_fill("usuario@servidor:2222");
//! let params = dialog.validate()?;
//! assert_eq!(params.user.as_deref(), Some("usuario"));
//! assert_eq!(params.hostname, "servidor");
//! assert_eq!(params.port, 2222);
//!
//! // Filtrar o diálogo usando o histórico de conexões
//! let recent = vec![
//!     QuickConnectRecent::new("admin@192.168.1.100", 22),
//!     QuickConnectRecent::new("user@example.com", 2222),
//! ];
//! dialog.clear_fields();
//! dialog.update_filter(&recent, &[]);
//! # Ok(())
//! # }
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::host::SshHost;

/// Estado do diálogo de Quick Connect
#[derive(Debug, Clone, PartialEq)]
pub enum QuickConnectState {
    /// Diálogo fechado
    Hidden,
    /// Diálogo visível mas não ativo
    Visible,
    /// Diálogo ativo (recebendo input)
    Active,
}

/// Campos de entrada do Quick Connect
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum QuickConnectField {
    /// Campo de hostname/IP
    #[default]
    Host,
    /// Campo de usuário
    User,
    /// Campo de porta
    Port,
    /// Campo de chave SSH
    IdentityFile,
}

impl QuickConnectField {
    /// Retorna o nome do campo para exibição
    pub fn label(&self) -> &'static str {
        match self {
            Self::Host => "Host",
            Self::User => "User",
            Self::Port => "Port",
            Self::IdentityFile => "Identity File",
        }
    }

    /// Retorna o placeholder do campo
    pub fn placeholder(&self) -> &'static str {
        match self {
            Self::Host => "hostname or IP address",
            Self::User => "username (optional)",
            Self::Port => "22",
            Self::IdentityFile => "~/.ssh/id_rsa (optional)",
        }
    }

    /// Retorna o próximo campo na ordem de tabulação
    pub fn next(&self) -> Self {
        match self {
            Self::Host => Self::User,
            Self::User => Self::Port,
            Self::Port => Self::IdentityFile,
            Self::IdentityFile => Self::Host,
        }
    }

    /// Retorna o campo anterior na ordem de tabulação
    pub fn previous(&self) -> Self {
        match self {
            Self::Host => Self::IdentityFile,
            Self::User => Self::Host,
            Self::Port => Self::User,
            Self::IdentityFile => Self::Port,
        }
    }

    /// Retorna todos os campos em ordem
    pub fn all() -> &'static [Self] {
        &[Self::Host, Self::User, Self::Port, Self::IdentityFile]
    }
}

/// Conexão recente para Quick Connect
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuickConnectRecent {
    /// String de conexão (user@host:port)
    pub connection_string: String,
    /// Hostname
    pub hostname: String,
    /// Usuário
    pub user: Option<String>,
    /// Porta
    pub port: u16,
    /// Arquivo de identidade usado
    pub identity_file: Option<String>,
    /// Timestamp da última conexão
    pub last_used: DateTime<Utc>,
    /// Número de vezes que foi conectado
    pub use_count: u32,
}

impl QuickConnectRecent {
    /// Cria uma nova conexão recente
    pub fn new(connection_string: &str, port: u16) -> Self {
        let (user, hostname) = if let Some(at_pos) = connection_string.find('@') {
            (
                Some(connection_string[..at_pos].to_string()),
                connection_string[at_pos + 1..].to_string(),
            )
        } else {
            (None, connection_string.to_string())
        };

        Self {
            connection_string: connection_string.to_string(),
            hostname,
            user,
            port,
            identity_file: None,
            last_used: Utc::now(),
            use_count: 1,
        }
    }

    /// Cria a partir de um host SSH
    pub fn from_host(host: &SshHost) -> Self {
        Self {
            connection_string: host.connection_string(),
            hostname: host.effective_host().to_string(),
            user: host.effective_user().map(|s| s.to_string()),
            port: host.effective_port(),
            identity_file: host.identity_file.as_ref().map(|p| p.display().to_string()),
            last_used: Utc::now(),
            use_count: 1,
        }
    }

    /// Atualiza o timestamp e contador
    pub fn touch(&mut self) {
        self.last_used = Utc::now();
        self.use_count += 1;
    }

    /// Retorna descrição formatada para exibição
    pub fn display_text(&self) -> String {
        let user_prefix = self
            .user
            .as_deref()
            .map(|u| format!("{}@", u))
            .unwrap_or_default();
        let port_suffix = if self.port != 22 {
            format!(":{}", self.port)
        } else {
            String::new()
        };
        format!("{}{}{}", user_prefix, self.hostname, port_suffix)
    }

    /// Retorna descrição secundária (info adicional)
    pub fn secondary_text(&self) -> String {
        let time_ago = self.time_ago();
        if let Some(ref identity) = self.identity_file {
            format!("{} · {}", identity, time_ago)
        } else {
            time_ago
        }
    }

    /// Retorna texto relativo do último uso
    fn time_ago(&self) -> String {
        let now = Utc::now();
        let duration = now.signed_duration_since(self.last_used);

        if duration.num_minutes() < 1 {
            "just now".to_string()
        } else if duration.num_minutes() < 60 {
            format!("{}m ago", duration.num_minutes())
        } else if duration.num_hours() < 24 {
            format!("{}h ago", duration.num_hours())
        } else if duration.num_days() < 7 {
            format!("{}d ago", duration.num_days())
        } else {
            self.last_used.format("%b %d").to_string()
        }
    }
}

/// Diálogo de Quick Connect
#[derive(Debug, Clone)]
pub struct QuickConnectDialog {
    /// Estado atual
    pub state: QuickConnectState,
    /// Valores dos campos
    pub fields: HashMap<QuickConnectField, String>,
    /// Campo atualmente em foco
    pub focused_field: QuickConnectField,
    /// Índice de seleção nos resultados de autocomplete
    pub selection_index: usize,
    /// Conexões recentes filtradas
    pub filtered_recent: Vec<QuickConnectRecent>,
    /// Hosts filtrados (do config)
    pub filtered_hosts: Vec<SshHost>,
    /// Mensagem de erro (se houver)
    pub error_message: Option<String>,
    /// Modo de filtro ativo
    pub filter_mode: FilterMode,
}

/// Modo de filtro para o diálogo
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterMode {
    /// Mostra todos (recentes + hosts)
    All,
    /// Apenas recentes
    Recent,
    /// Apenas hosts do config
    Config,
}

impl Default for QuickConnectDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl QuickConnectDialog {
    /// Cria um novo diálogo
    pub fn new() -> Self {
        let mut fields = HashMap::new();
        fields.insert(QuickConnectField::Host, String::new());
        fields.insert(QuickConnectField::User, String::new());
        fields.insert(QuickConnectField::Port, String::new());
        fields.insert(QuickConnectField::IdentityFile, String::new());

        Self {
            state: QuickConnectState::Hidden,
            fields,
            focused_field: QuickConnectField::Host,
            selection_index: 0,
            filtered_recent: Vec::new(),
            filtered_hosts: Vec::new(),
            error_message: None,
            filter_mode: FilterMode::All,
        }
    }

    /// Abre o diálogo
    pub fn show(&mut self) {
        self.state = QuickConnectState::Visible;
        self.focused_field = QuickConnectField::Host;
        self.selection_index = 0;
        self.error_message = None;
    }

    /// Oculta o diálogo
    pub fn hide(&mut self) {
        self.state = QuickConnectState::Hidden;
        self.clear_fields();
    }

    /// Ativa o diálogo (para input)
    pub fn activate(&mut self) {
        self.state = QuickConnectState::Active;
    }

    /// Desativa o diálogo
    pub fn deactivate(&mut self) {
        self.state = QuickConnectState::Visible;
    }

    /// Verifica se está visível
    pub fn is_visible(&self) -> bool {
        self.state != QuickConnectState::Hidden
    }

    /// Verifica se está ativo
    pub fn is_active(&self) -> bool {
        self.state == QuickConnectState::Active
    }

    /// Limpa todos os campos
    pub fn clear_fields(&mut self) {
        for field in QuickConnectField::all() {
            self.fields.insert(*field, String::new());
        }
        self.selection_index = 0;
        self.error_message = None;
        self.filtered_recent.clear();
        self.filtered_hosts.clear();
    }

    /// Obtém valor de um campo
    pub fn get_field(&self, field: QuickConnectField) -> &str {
        self.fields.get(&field).map(|s| s.as_str()).unwrap_or("")
    }

    /// Define valor de um campo
    pub fn set_field(&mut self, field: QuickConnectField, value: &str) {
        self.fields.insert(field, value.to_string());
        self.error_message = None;
    }

    /// Atualiza o campo em foco com um caractere
    pub fn type_char(&mut self, c: char) {
        if let Some(value) = self.fields.get_mut(&self.focused_field) {
            value.push(c);
            self.error_message = None;
        }
    }

    /// Remove o último caractere do campo em foco
    pub fn backspace(&mut self) {
        if let Some(value) = self.fields.get_mut(&self.focused_field) {
            value.pop();
            self.error_message = None;
        }
    }

    /// Move para o próximo campo
    pub fn next_field(&mut self) {
        self.focused_field = self.focused_field.next();
    }

    /// Move para o campo anterior
    pub fn previous_field(&mut self) {
        self.focused_field = self.focused_field.previous();
    }

    /// Move seleção para cima nos resultados
    pub fn select_up(&mut self) {
        if self.selection_index > 0 {
            self.selection_index -= 1;
        }
    }

    /// Move seleção para baixo nos resultados
    pub fn select_down(&mut self) {
        let total = self.filtered_recent.len() + self.filtered_hosts.len();
        if self.selection_index < total.saturating_sub(1) {
            self.selection_index += 1;
        }
    }

    /// Retorna o item selecionado (se houver)
    pub fn get_selected(&self) -> Option<QuickConnectSelection> {
        let recent_count = self.filtered_recent.len();

        if self.selection_index < recent_count {
            self.filtered_recent
                .get(self.selection_index)
                .map(|r| QuickConnectSelection::Recent(r.clone()))
        } else {
            let host_index = self.selection_index - recent_count;
            self.filtered_hosts
                .get(host_index)
                .map(|h| QuickConnectSelection::Host(h.clone()))
        }
    }

    /// Aplica a seleção atual aos campos
    pub fn apply_selection(&mut self) {
        if let Some(selection) = self.get_selected() {
            match selection {
                QuickConnectSelection::Recent(recent) => {
                    self.set_field(QuickConnectField::Host, &recent.hostname);
                    if let Some(ref user) = recent.user {
                        self.set_field(QuickConnectField::User, user);
                    }
                    if recent.port != 22 {
                        self.set_field(QuickConnectField::Port, &recent.port.to_string());
                    }
                    if let Some(ref identity) = recent.identity_file {
                        self.set_field(QuickConnectField::IdentityFile, identity);
                    }
                }
                QuickConnectSelection::Host(host) => {
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
            }
        }
    }

    /// Atualiza os resultados de filtro
    pub fn update_filter(&mut self, recent_connections: &[QuickConnectRecent], hosts: &[SshHost]) {
        let query = self.get_field(QuickConnectField::Host).to_lowercase();

        // Filtra conexões recentes
        self.filtered_recent = if query.is_empty() {
            recent_connections.to_vec()
        } else {
            recent_connections
                .iter()
                .filter(|c| {
                    c.connection_string.to_lowercase().contains(&query)
                        || c.hostname.to_lowercase().contains(&query)
                        || c.user
                            .as_ref()
                            .is_some_and(|u| u.to_lowercase().contains(&query))
                })
                .cloned()
                .collect()
        };

        // Filtra hosts do config
        self.filtered_hosts = if query.is_empty() {
            hosts.to_vec()
        } else {
            hosts
                .iter()
                .filter(|h| {
                    h.pattern.to_lowercase().contains(&query)
                        || h.host_name
                            .as_ref()
                            .is_some_and(|hn| hn.to_lowercase().contains(&query))
                        || h.user
                            .as_ref()
                            .is_some_and(|u| u.to_lowercase().contains(&query))
                })
                .cloned()
                .collect()
        };

        // Aplica modo de filtro
        match self.filter_mode {
            FilterMode::Recent => self.filtered_hosts.clear(),
            FilterMode::Config => self.filtered_recent.clear(),
            FilterMode::All => {}
        }

        // Reseta seleção se ficou fora do range
        let total = self.filtered_recent.len() + self.filtered_hosts.len();
        if self.selection_index >= total && total > 0 {
            self.selection_index = total - 1;
        }
    }

    /// Valida a entrada e retorna uma conexão pronta
    pub fn validate(&self) -> Result<QuickConnectParams, QuickConnectError> {
        let host = self.get_field(QuickConnectField::Host);

        if host.is_empty() {
            return Err(QuickConnectError::EmptyHost);
        }

        // Valida hostname básico
        if !Self::is_valid_hostname(host) {
            return Err(QuickConnectError::InvalidHostname(host.to_string()));
        }

        let port_str = self.get_field(QuickConnectField::Port);
        let port = if port_str.is_empty() {
            22
        } else {
            port_str
                .parse::<u16>()
                .map_err(|_| QuickConnectError::InvalidPort(port_str.to_string()))?
        };

        let user = self.get_field(QuickConnectField::User);
        let user = if user.is_empty() {
            None
        } else {
            Some(user.to_string())
        };

        let identity = self.get_field(QuickConnectField::IdentityFile);
        let identity = if identity.is_empty() {
            None
        } else {
            Some(identity.to_string())
        };

        Ok(QuickConnectParams {
            hostname: host.to_string(),
            port,
            user,
            identity_file: identity,
        })
    }

    /// Valida se um hostname parece válido
    fn is_valid_hostname(host: &str) -> bool {
        if host.is_empty() {
            return false;
        }

        // Não pode ter espaços
        if host.contains(' ') {
            return false;
        }

        // Não pode começar com @ ou :
        if host.starts_with('@') || host.starts_with(':') {
            return false;
        }

        // Verifica se parece um IP ou hostname
        let parts: Vec<&str> = host.split('.').collect();

        // IP address
        if parts.len() == 4 {
            return parts.iter().all(|p| p.parse::<u8>().is_ok());
        }

        // Hostname simples ou FQDN
        host.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == ':')
    }

    /// Retorna o número total de resultados
    pub fn total_results(&self) -> usize {
        self.filtered_recent.len() + self.filtered_hosts.len()
    }

    /// Verifica se há resultados
    pub fn has_results(&self) -> bool {
        self.total_results() > 0
    }

    /// Define uma mensagem de erro
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error_message = Some(message.into());
    }

    /// Limpa a mensagem de erro
    pub fn clear_error(&mut self) {
        self.error_message = None;
    }

    /// Alterna o modo de filtro
    pub fn toggle_filter_mode(&mut self) {
        self.filter_mode = match self.filter_mode {
            FilterMode::All => FilterMode::Recent,
            FilterMode::Recent => FilterMode::Config,
            FilterMode::Config => FilterMode::All,
        };
    }

    /// Preenche com uma conexão rápida (user@host:port)
    pub fn parse_and_fill(&mut self, input: &str) {
        let (user, rest) = if let Some(at_pos) = input.find('@') {
            (Some(input[..at_pos].to_string()), &input[at_pos + 1..])
        } else {
            (None, input)
        };

        let (hostname, port) = if let Some(colon_pos) = rest.rfind(':') {
            let host_part = &rest[..colon_pos];
            let port_part = &rest[colon_pos + 1..];
            (host_part.to_string(), port_part.to_string())
        } else {
            (rest.to_string(), String::new())
        };

        self.set_field(QuickConnectField::Host, &hostname);
        if let Some(u) = user {
            self.set_field(QuickConnectField::User, &u);
        }
        if !port.is_empty() {
            self.set_field(QuickConnectField::Port, &port);
        }
    }
}

/// Seleção no diálogo (recente ou host)
#[derive(Debug, Clone)]
pub enum QuickConnectSelection {
    /// Conexão recente selecionada
    Recent(QuickConnectRecent),
    /// Host do config selecionado
    Host(SshHost),
}

/// Parâmetros para conexão rápida validados
#[derive(Debug, Clone, PartialEq)]
pub struct QuickConnectParams {
    /// Hostname
    pub hostname: String,
    /// Porta
    pub port: u16,
    /// Usuário (opcional)
    pub user: Option<String>,
    /// Arquivo de identidade (opcional)
    pub identity_file: Option<String>,
}

impl QuickConnectParams {
    /// Retorna a string de conexão
    pub fn connection_string(&self) -> String {
        let user_prefix = self
            .user
            .as_deref()
            .map(|u| format!("{}@", u))
            .unwrap_or_default();
        let port_suffix = if self.port != 22 {
            format!(":{}", self.port)
        } else {
            String::new()
        };
        format!("{}{}{}", user_prefix, self.hostname, port_suffix)
    }

    /// Converte para comando SSH
    pub fn to_ssh_command(&self) -> Vec<String> {
        let mut args = Vec::new();

        args.push("-p".to_string());
        args.push(self.port.to_string());

        if let Some(ref identity) = self.identity_file {
            args.push("-i".to_string());
            args.push(identity.clone());
        }

        let target = if let Some(ref user) = self.user {
            format!("{}@{}", user, self.hostname)
        } else {
            self.hostname.clone()
        };

        args.push(target);
        args
    }
}

/// Erros do Quick Connect
#[derive(Debug, Clone, thiserror::Error)]
pub enum QuickConnectError {
    #[error("Host is required")]
    EmptyHost,

    #[error("Invalid hostname: {0}")]
    InvalidHostname(String),

    #[error("Invalid port: {0}")]
    InvalidPort(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dialog_creation() {
        let dialog = QuickConnectDialog::new();
        assert_eq!(dialog.state, QuickConnectState::Hidden);
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_dialog_show_hide() {
        let mut dialog = QuickConnectDialog::new();

        dialog.show();
        assert!(dialog.is_visible());
        assert!(!dialog.is_active());

        dialog.activate();
        assert!(dialog.is_active());

        dialog.hide();
        assert!(!dialog.is_visible());
    }

    #[test]
    fn test_field_navigation() {
        let mut dialog = QuickConnectDialog::new();

        assert_eq!(dialog.focused_field, QuickConnectField::Host);

        dialog.next_field();
        assert_eq!(dialog.focused_field, QuickConnectField::User);

        dialog.next_field();
        assert_eq!(dialog.focused_field, QuickConnectField::Port);

        dialog.next_field();
        assert_eq!(dialog.focused_field, QuickConnectField::IdentityFile);

        dialog.next_field();
        assert_eq!(dialog.focused_field, QuickConnectField::Host);
    }

    #[test]
    fn test_type_and_backspace() {
        let mut dialog = QuickConnectDialog::new();
        dialog.show();

        dialog.type_char('h');
        dialog.type_char('o');
        dialog.type_char('s');
        dialog.type_char('t');

        assert_eq!(dialog.get_field(QuickConnectField::Host), "host");

        dialog.backspace();
        assert_eq!(dialog.get_field(QuickConnectField::Host), "hos");
    }

    #[test]
    fn test_validate_success() {
        let mut dialog = QuickConnectDialog::new();
        dialog.show();

        dialog.set_field(QuickConnectField::Host, "example.com");
        dialog.set_field(QuickConnectField::User, "admin");
        dialog.set_field(QuickConnectField::Port, "2222");

        let result = dialog.validate();
        assert!(result.is_ok());

        let params = result.unwrap();
        assert_eq!(params.hostname, "example.com");
        assert_eq!(params.user, Some("admin".to_string()));
        assert_eq!(params.port, 2222);
    }

    #[test]
    fn test_validate_empty_host() {
        let dialog = QuickConnectDialog::new();

        let result = dialog.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), QuickConnectError::EmptyHost));
    }

    #[test]
    fn test_validate_invalid_port() {
        let mut dialog = QuickConnectDialog::new();
        dialog.show();

        dialog.set_field(QuickConnectField::Host, "example.com");
        dialog.set_field(QuickConnectField::Port, "invalid");

        let result = dialog.validate();
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            QuickConnectError::InvalidPort(_)
        ));
    }

    #[test]
    fn test_validate_default_port() {
        let mut dialog = QuickConnectDialog::new();
        dialog.show();

        dialog.set_field(QuickConnectField::Host, "example.com");

        let params = dialog.validate().unwrap();
        assert_eq!(params.port, 22);
    }

    #[test]
    fn test_recent_connection_creation() {
        let conn = QuickConnectRecent::new("admin@example.com", 2222);

        assert_eq!(conn.hostname, "example.com");
        assert_eq!(conn.user, Some("admin".to_string()));
        assert_eq!(conn.port, 2222);
        assert_eq!(conn.use_count, 1);
    }

    #[test]
    fn test_recent_connection_display() {
        let conn = QuickConnectRecent::new("admin@example.com", 2222);
        assert_eq!(conn.display_text(), "admin@example.com:2222");

        let conn = QuickConnectRecent::new("example.com", 22);
        assert_eq!(conn.display_text(), "example.com");
    }

    #[test]
    fn test_recent_connection_touch() {
        let mut conn = QuickConnectRecent::new("example.com", 22);
        let original_count = conn.use_count;

        conn.touch();

        assert_eq!(conn.use_count, original_count + 1);
    }

    #[test]
    fn test_parse_and_fill() {
        let mut dialog = QuickConnectDialog::new();

        dialog.parse_and_fill("admin@example.com:2222");

        assert_eq!(dialog.get_field(QuickConnectField::Host), "example.com");
        assert_eq!(dialog.get_field(QuickConnectField::User), "admin");
        assert_eq!(dialog.get_field(QuickConnectField::Port), "2222");
    }

    #[test]
    fn test_parse_and_fill_no_port() {
        let mut dialog = QuickConnectDialog::new();

        dialog.parse_and_fill("admin@example.com");

        assert_eq!(dialog.get_field(QuickConnectField::Host), "example.com");
        assert_eq!(dialog.get_field(QuickConnectField::User), "admin");
        assert_eq!(dialog.get_field(QuickConnectField::Port), "");
    }

    #[test]
    fn test_parse_and_fill_host_only() {
        let mut dialog = QuickConnectDialog::new();

        dialog.parse_and_fill("example.com");

        assert_eq!(dialog.get_field(QuickConnectField::Host), "example.com");
        assert_eq!(dialog.get_field(QuickConnectField::User), "");
    }

    #[test]
    fn test_quick_connect_params_ssh_command() {
        let params = QuickConnectParams {
            hostname: "example.com".to_string(),
            port: 2222,
            user: Some("admin".to_string()),
            identity_file: Some("~/.ssh/id_rsa".to_string()),
        };

        let cmd = params.to_ssh_command();
        assert!(cmd.contains(&"-p".to_string()));
        assert!(cmd.contains(&"2222".to_string()));
        assert!(cmd.contains(&"-i".to_string()));
        assert!(cmd.contains(&"~/.ssh/id_rsa".to_string()));
        assert!(cmd.contains(&"admin@example.com".to_string()));
    }

    #[test]
    fn test_connection_string() {
        let params = QuickConnectParams {
            hostname: "example.com".to_string(),
            port: 2222,
            user: Some("admin".to_string()),
            identity_file: None,
        };

        assert_eq!(params.connection_string(), "admin@example.com:2222");

        let params = QuickConnectParams {
            hostname: "example.com".to_string(),
            port: 22,
            user: None,
            identity_file: None,
        };

        assert_eq!(params.connection_string(), "example.com");
    }

    #[test]
    fn test_field_labels() {
        assert_eq!(QuickConnectField::Host.label(), "Host");
        assert_eq!(QuickConnectField::User.label(), "User");
        assert_eq!(QuickConnectField::Port.label(), "Port");
        assert_eq!(QuickConnectField::IdentityFile.label(), "Identity File");
    }

    #[test]
    fn test_update_filter() {
        let mut dialog = QuickConnectDialog::new();
        dialog.show();

        let recent = vec![
            QuickConnectRecent::new("prod.example.com", 22),
            QuickConnectRecent::new("dev.example.com", 22),
        ];

        let hosts = vec![SshHost::new("staging")];

        dialog.set_field(QuickConnectField::Host, "prod");
        dialog.update_filter(&recent, &hosts);

        assert_eq!(dialog.filtered_recent.len(), 1);
        assert_eq!(dialog.filtered_hosts.len(), 0);
    }

    #[test]
    fn test_selection_navigation() {
        let mut dialog = QuickConnectDialog::new();

        dialog.filtered_recent = vec![
            QuickConnectRecent::new("host1", 22),
            QuickConnectRecent::new("host2", 22),
        ];
        dialog.filtered_hosts = vec![SshHost::new("host3")];

        assert_eq!(dialog.selection_index, 0);

        dialog.select_down();
        assert_eq!(dialog.selection_index, 1);

        dialog.select_down();
        assert_eq!(dialog.selection_index, 2);

        dialog.select_up();
        assert_eq!(dialog.selection_index, 1);
    }

    #[test]
    fn test_get_selected() {
        let mut dialog = QuickConnectDialog::new();

        dialog.filtered_recent = vec![QuickConnectRecent::new("host1", 22)];
        dialog.filtered_hosts = vec![SshHost::new("host2")];

        dialog.selection_index = 0;
        let selected = dialog.get_selected();
        assert!(matches!(selected, Some(QuickConnectSelection::Recent(_))));

        dialog.selection_index = 1;
        let selected = dialog.get_selected();
        assert!(matches!(selected, Some(QuickConnectSelection::Host(_))));
    }

    #[test]
    fn test_filter_mode_toggle() {
        let mut dialog = QuickConnectDialog::new();

        assert_eq!(dialog.filter_mode, FilterMode::All);

        dialog.toggle_filter_mode();
        assert_eq!(dialog.filter_mode, FilterMode::Recent);

        dialog.toggle_filter_mode();
        assert_eq!(dialog.filter_mode, FilterMode::Config);

        dialog.toggle_filter_mode();
        assert_eq!(dialog.filter_mode, FilterMode::All);
    }

    #[test]
    fn test_valid_hostname() {
        assert!(QuickConnectDialog::is_valid_hostname("example.com"));
        assert!(QuickConnectDialog::is_valid_hostname("192.168.1.1"));
        assert!(QuickConnectDialog::is_valid_hostname("my-host.example.com"));
        assert!(QuickConnectDialog::is_valid_hostname("localhost"));

        assert!(!QuickConnectDialog::is_valid_hostname(""));
        assert!(!QuickConnectDialog::is_valid_hostname("invalid host"));
        assert!(!QuickConnectDialog::is_valid_hostname("@invalid"));
    }
}
