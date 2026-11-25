//! Configuração SSH
//!
//! Tipos para representar e parsear configurações SSH do arquivo ~/.ssh/config

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Representa um host SSH configurado
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SshHost {
    /// Alias/nome do host (do directive Host)
    pub alias: String,
    /// Hostname real (HostName directive)
    pub hostname: Option<String>,
    /// Usuário (User directive)
    pub user: Option<String>,
    /// Porta (Port directive)
    pub port: Option<u16>,
    /// Arquivos de identidade (IdentityFile directives)
    pub identity_files: Vec<SshIdentityFile>,
    /// Outras opções arbitrárias
    pub options: HashMap<String, String>,
    /// Grupos/tags para organização
    pub groups: Vec<String>,
    /// Descrição opcional (comment)
    pub description: Option<String>,
}

impl SshHost {
    /// Cria um novo host com alias
    pub fn new(alias: impl Into<String>) -> Self {
        Self {
            alias: alias.into(),
            ..Default::default()
        }
    }

    /// Cria um host a partir de uma string de conexão (user@host:port)
    pub fn from_connection_string(s: &str) -> Option<Self> {
        let (user, rest) = if let Some(at_pos) = s.find('@') {
            (Some(s[..at_pos].to_string()), &s[at_pos + 1..])
        } else {
            (None, s)
        };

        let (hostname, port) = if let Some(colon_pos) = rest.rfind(':') {
            let host_part = &rest[..colon_pos];
            let port_part = &rest[colon_pos + 1..];
            let port = port_part.parse::<u16>().ok();
            (Some(host_part.to_string()), port)
        } else {
            (Some(rest.to_string()), None)
        };

        if hostname.is_none() || hostname.as_ref().is_none_or(|h| h.is_empty()) {
            return None;
        }

        Some(Self {
            alias: hostname.clone().unwrap_or_default(),
            hostname,
            user,
            port,
            ..Default::default()
        })
    }

    /// Define o hostname
    pub fn with_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.hostname = Some(hostname.into());
        self
    }

    /// Define o usuário
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Define a porta
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Adiciona arquivo de identidade
    pub fn with_identity_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.identity_files.push(SshIdentityFile::new(path));
        self
    }

    /// Adiciona uma opção
    pub fn with_option(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.options.insert(key.into(), value.into());
        self
    }

    /// Retorna a string de conexão para exibição
    pub fn connection_string(&self) -> String {
        let host = self.hostname.as_deref().unwrap_or(&self.alias);
        let user_prefix = self
            .user
            .as_deref()
            .map(|u| format!("{}@", u))
            .unwrap_or_default();
        let port_suffix = self.port.map(|p| format!(":{}", p)).unwrap_or_default();
        format!("{}{}{}", user_prefix, host, port_suffix)
    }

    /// Retorna o hostname efetivo (hostname ou alias se não definido)
    pub fn effective_hostname(&self) -> &str {
        self.hostname.as_deref().unwrap_or(&self.alias)
    }

    /// Retorna o usuário efetivo
    pub fn effective_user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    /// Retorna a porta efetiva (22 se não definida)
    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(22)
    }

    /// Verifica se o host match uma query de busca
    pub fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();

        self.alias.to_lowercase().contains(&query)
            || self
                .hostname
                .as_ref()
                .is_some_and(|h| h.to_lowercase().contains(&query))
            || self
                .user
                .as_ref()
                .is_some_and(|u| u.to_lowercase().contains(&query))
            || self
                .groups
                .iter()
                .any(|g| g.to_lowercase().contains(&query))
            || self
                .description
                .as_ref()
                .is_some_and(|d| d.to_lowercase().contains(&query))
    }
}

/// Arquivo de identidade SSH
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SshIdentityFile {
    /// Caminho para o arquivo de chave
    pub path: PathBuf,
    /// Se a chave tem passphrase
    pub encrypted: bool,
    /// Comentário da chave (se disponível)
    pub comment: Option<String>,
}

impl SshIdentityFile {
    /// Cria um novo arquivo de identidade
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            encrypted: false,
            comment: None,
        }
    }

    /// Verifica se o arquivo existe
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Retorna o nome do arquivo
    pub fn filename(&self) -> Option<&str> {
        self.path.file_name().and_then(|n| n.to_str())
    }
}

/// Configuração SSH completa
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SshConfig {
    /// Hosts configurados
    pub hosts: HashMap<String, SshHost>,
    /// Caminho do arquivo de configuração
    #[serde(skip)]
    pub config_path: Option<PathBuf>,
    /// Hosts incluídos via Include directive
    pub includes: Vec<PathBuf>,
    /// Opções globais (aplicadas antes de qualquer host)
    pub global_options: HashMap<String, String>,
}

impl SshConfig {
    /// Cria uma configuração vazia
    pub fn new() -> Self {
        Self::default()
    }

    /// Carrega configuração do arquivo padrão (~/.ssh/config)
    pub fn load_from_default() -> Result<Self, SshConfigError> {
        let config_path = Self::default_config_path()?;

        if !config_path.exists() {
            return Ok(Self::new());
        }

        Self::load_from_path(&config_path)
    }

    /// Retorna o caminho padrão do arquivo de configuração
    pub fn default_config_path() -> Result<PathBuf, SshConfigError> {
        dirs::home_dir()
            .map(|h| h.join(".ssh").join("config"))
            .ok_or(SshConfigError::HomeNotFound)
    }

    /// Carrega configuração de um arquivo específico
    pub fn load_from_path(path: &Path) -> Result<Self, SshConfigError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| SshConfigError::ReadError(path.to_path_buf(), e))?;

        let mut config = Self::parse(&content)?;
        config.config_path = Some(path.to_path_buf());

        Ok(config)
    }

    /// Parse conteúdo do arquivo de configuração SSH
    pub fn parse(content: &str) -> Result<Self, SshConfigError> {
        let mut config = Self::new();
        let mut current_host: Option<SshHost> = None;

        for line in content.lines() {
            let line = line.trim();

            // Ignora comentários e linhas vazias
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Parse da diretiva
            let parts: Vec<&str> = line.splitn(2, char::is_whitespace).collect();
            if parts.len() != 2 {
                continue;
            }

            let directive = parts[0].to_lowercase();
            let value = parts[1].trim();

            match directive.as_str() {
                "host" => {
                    // Salva host anterior se existir
                    if let Some(host) = current_host.take() {
                        config.hosts.insert(host.alias.clone(), host);
                    }

                    // Inicia novo host
                    current_host = Some(SshHost::new(value));
                }
                "hostname" => {
                    if let Some(ref mut host) = current_host {
                        host.hostname = Some(value.to_string());
                    }
                }
                "user" => {
                    if let Some(ref mut host) = current_host {
                        host.user = Some(value.to_string());
                    }
                }
                "port" => {
                    if let Some(ref mut host) = current_host {
                        host.port = value.parse().ok();
                    }
                }
                "identityfile" => {
                    if let Some(ref mut host) = current_host {
                        host.identity_files.push(SshIdentityFile::new(value));
                    }
                }
                "include" => {
                    config.includes.push(PathBuf::from(value));
                }
                _ => {
                    // Opção arbitrária
                    if let Some(ref mut host) = current_host {
                        host.options.insert(directive, value.to_string());
                    } else {
                        config.global_options.insert(directive, value.to_string());
                    }
                }
            }
        }

        // Salva último host
        if let Some(host) = current_host {
            config.hosts.insert(host.alias.clone(), host);
        }

        Ok(config)
    }

    /// Adiciona um host à configuração
    pub fn add_host(&mut self, host: SshHost) {
        self.hosts.insert(host.alias.clone(), host);
    }

    /// Remove um host
    pub fn remove_host(&mut self, alias: &str) -> Option<SshHost> {
        self.hosts.remove(alias)
    }

    /// Busca hosts por query
    pub fn search_hosts(&self, query: &str) -> Vec<SshHost> {
        if query.is_empty() {
            return self.hosts.values().cloned().collect();
        }

        self.hosts
            .values()
            .filter(|h| h.matches(query))
            .cloned()
            .collect()
    }

    /// Retorna hosts ordenados por alias
    pub fn sorted_hosts(&self) -> Vec<&SshHost> {
        let mut hosts: Vec<_> = self.hosts.values().collect();
        hosts.sort_by(|a, b| a.alias.cmp(&b.alias));
        hosts
    }

    /// Serializa para formato do arquivo ~/.ssh/config
    pub fn to_config_string(&self) -> String {
        let mut output = String::new();

        // Opções globais
        for (key, value) in &self.global_options {
            output.push_str(&format!("{} {}\n", key, value));
        }

        if !self.global_options.is_empty() {
            output.push('\n');
        }

        // Hosts
        for host in self.sorted_hosts() {
            output.push_str(&format!("Host {}\n", host.alias));

            if let Some(ref hostname) = host.hostname {
                output.push_str(&format!("    HostName {}\n", hostname));
            }

            if let Some(ref user) = host.user {
                output.push_str(&format!("    User {}\n", user));
            }

            if let Some(port) = host.port {
                output.push_str(&format!("    Port {}\n", port));
            }

            for identity in &host.identity_files {
                output.push_str(&format!("    IdentityFile {}\n", identity.path.display()));
            }

            for (key, value) in &host.options {
                output.push_str(&format!("    {} {}\n", key, value));
            }

            output.push('\n');
        }

        output
    }
}

/// Erros de configuração SSH
#[derive(Debug, thiserror::Error)]
pub enum SshConfigError {
    #[error("Home directory not found")]
    HomeNotFound,

    #[error("Failed to read config file '{0}': {1}")]
    ReadError(PathBuf, #[source] std::io::Error),

    #[error("Failed to write config file '{0}': {1}")]
    WriteError(PathBuf, #[source] std::io::Error),

    #[error("Invalid config syntax: {0}")]
    InvalidSyntax(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_host_creation() {
        let host = SshHost::new("my-server")
            .with_hostname("example.com")
            .with_user("admin")
            .with_port(2222);

        assert_eq!(host.alias, "my-server");
        assert_eq!(host.hostname, Some("example.com".to_string()));
        assert_eq!(host.user, Some("admin".to_string()));
        assert_eq!(host.port, Some(2222));
    }

    #[test]
    fn test_connection_string() {
        let host = SshHost::new("server")
            .with_hostname("example.com")
            .with_user("admin")
            .with_port(2222);

        assert_eq!(host.connection_string(), "admin@example.com:2222");
    }

    #[test]
    fn test_connection_string_no_port() {
        let host = SshHost::new("server")
            .with_hostname("example.com")
            .with_user("admin");

        assert_eq!(host.connection_string(), "admin@example.com");
    }

    #[test]
    fn test_from_connection_string() {
        let host = SshHost::from_connection_string("admin@example.com:2222");
        assert!(host.is_some());

        let host = host.unwrap();
        assert_eq!(host.user, Some("admin".to_string()));
        assert_eq!(host.hostname, Some("example.com".to_string()));
        assert_eq!(host.port, Some(2222));
    }

    #[test]
    fn test_from_connection_string_no_port() {
        let host = SshHost::from_connection_string("admin@example.com");
        assert!(host.is_some());

        let host = host.unwrap();
        assert_eq!(host.user, Some("admin".to_string()));
        assert_eq!(host.hostname, Some("example.com".to_string()));
        assert_eq!(host.port, None);
    }

    #[test]
    fn test_from_connection_string_host_only() {
        let host = SshHost::from_connection_string("example.com");
        assert!(host.is_some());

        let host = host.unwrap();
        assert_eq!(host.user, None);
        assert_eq!(host.hostname, Some("example.com".to_string()));
    }

    #[test]
    fn test_host_matches() {
        let host = SshHost::new("prod-server")
            .with_hostname("production.example.com")
            .with_user("deploy");

        assert!(host.matches("prod"));
        assert!(host.matches("PROD"));
        assert!(host.matches("example"));
        assert!(host.matches("deploy"));
        assert!(!host.matches("staging"));
    }

    #[test]
    fn test_ssh_config_parse() {
        let config_content = r#"
Host github
    HostName github.com
    User git
    IdentityFile ~/.ssh/github_key

Host prod-server
    HostName prod.example.com
    User admin
    Port 2222
"#;

        let config = SshConfig::parse(config_content).unwrap();

        assert_eq!(config.hosts.len(), 2);
        assert!(config.hosts.contains_key("github"));
        assert!(config.hosts.contains_key("prod-server"));

        let github = config.hosts.get("github").unwrap();
        assert_eq!(github.hostname, Some("github.com".to_string()));
        assert_eq!(github.user, Some("git".to_string()));
        assert_eq!(github.port, None);
    }

    #[test]
    fn test_ssh_config_search() {
        let mut config = SshConfig::new();

        config.add_host(SshHost::new("prod-server").with_hostname("prod.example.com"));
        config.add_host(SshHost::new("dev-server").with_hostname("dev.example.com"));
        config.add_host(SshHost::new("staging").with_hostname("staging.example.com"));

        let results = config.search_hosts("prod");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].alias, "prod-server");

        let results = config.search_hosts("server");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_ssh_config_to_string() {
        let mut config = SshConfig::new();

        config.add_host(
            SshHost::new("my-server")
                .with_hostname("example.com")
                .with_user("admin")
                .with_port(2222),
        );

        let output = config.to_config_string();

        assert!(output.contains("Host my-server"));
        assert!(output.contains("HostName example.com"));
        assert!(output.contains("User admin"));
        assert!(output.contains("Port 2222"));
    }

    #[test]
    fn test_identity_file() {
        let id = SshIdentityFile::new("~/.ssh/id_rsa");
        assert_eq!(id.path, PathBuf::from("~/.ssh/id_rsa"));
        assert!(!id.encrypted);
    }

    #[test]
    fn test_effective_values() {
        let host = SshHost::new("server")
            .with_hostname("example.com")
            .with_port(2222);

        assert_eq!(host.effective_hostname(), "example.com");
        assert_eq!(host.effective_port(), 2222);
        assert_eq!(host.effective_user(), None);
    }

    #[test]
    fn test_effective_defaults() {
        let host = SshHost::new("example.com");

        assert_eq!(host.effective_hostname(), "example.com");
        assert_eq!(host.effective_port(), 22);
    }
}
