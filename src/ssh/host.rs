//! Definições de host SSH e estruturas relacionadas
//!
//! Este módulo define a estrutura `SshHost` que representa um host SSH
//! com todas as suas configurações, incluindo forwarding de portas,
//! opções de segurança e autenticação.
//!
//! # Exemplo de uso
//!
//! ```rust
//! use code_terminal::ssh::{PortForward, SshHost};
//!
//! // Criar um novo host
//! let host = SshHost {
//!     host_name: Some("192.168.1.100".to_string()),
//!     user: Some("admin".to_string()),
//!     port: Some(2222),
//!     // Redireciona a porta local 8080 para o serviço remoto
//!     local_forward: vec![PortForward {
//!         local_port: 8080,
//!         bind_address: None,
//!         remote_host: "localhost".to_string(),
//!         remote_port: 80,
//!     }],
//!     ..SshHost::new("myserver")
//! };
//!
//! // String de conexão
//! assert_eq!(host.connection_string(), "admin@192.168.1.100:2222");
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SshHost {
    pub pattern: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
    pub forward_agent: Option<bool>,
    pub forward_x11: Option<bool>,
    pub strict_host_key_checking: Option<StrictHostKeyChecking>,
    pub user_known_hosts_file: Option<PathBuf>,
    pub proxy_jump: Option<String>,
    pub proxy_command: Option<String>,
    pub server_alive_interval: Option<u64>,
    pub server_alive_count_max: Option<u64>,
    pub connection_timeout: Option<u64>,
    pub compression: Option<bool>,
    pub tcp_keep_alive: Option<bool>,
    pub local_forward: Vec<PortForward>,
    pub remote_forward: Vec<PortForward>,
    pub dynamic_forward: Vec<DynamicForward>,
    pub gateway_ports: Option<bool>,
    pub escape_char: Option<char>,
    pub log_level: Option<LogLevel>,
    pub preferred_authentications: Option<String>,
    pub pub_key_authentication: Option<bool>,
    pub password_authentication: Option<bool>,
    pub host_key_algorithms: Option<String>,
    pub kex_algorithms: Option<String>,
    pub ciphers: Option<String>,
    pub macs: Option<String>,
    pub extra_options: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrictHostKeyChecking {
    #[serde(rename = "yes")]
    Yes,
    #[serde(rename = "no")]
    No,
    #[serde(rename = "ask")]
    Ask,
    #[serde(rename = "accept-new")]
    AcceptNew,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortForward {
    pub local_port: u16,
    pub bind_address: Option<String>,
    pub remote_host: String,
    pub remote_port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicForward {
    pub local_port: u16,
    pub bind_address: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    #[serde(rename = "quiet")]
    Quiet,
    #[serde(rename = "fatal")]
    Fatal,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "verbose")]
    Verbose,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "debug1")]
    Debug1,
    #[serde(rename = "debug2")]
    Debug2,
    #[serde(rename = "debug3")]
    Debug3,
}

impl SshHost {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            ..Default::default()
        }
    }

    pub fn matches(&self, hostname: &str) -> bool {
        if self.pattern == "*" {
            return true;
        }

        let patterns: Vec<&str> = self.pattern.split_whitespace().collect();
        let mut matched = false;

        for pattern in patterns {
            if let Some(negated) = pattern.strip_prefix('!') {
                if Self::pattern_matches(negated, hostname) {
                    return false;
                }
            } else if !matched && Self::pattern_matches(pattern, hostname) {
                matched = true;
            }
        }
        matched
    }

    fn pattern_matches(pattern: &str, hostname: &str) -> bool {
        if pattern == "*" {
            return true;
        }

        if pattern.contains('*') {
            let parts: Vec<&str> = pattern.split('*').collect();
            if parts.len() == 2 {
                let prefix = parts[0];
                let suffix = parts[1];
                return hostname.starts_with(prefix) && hostname.ends_with(suffix);
            }
        }

        pattern == hostname
    }

    pub fn effective_host(&self) -> &str {
        self.host_name.as_deref().unwrap_or(&self.pattern)
    }

    pub fn effective_user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    pub fn effective_port(&self) -> u16 {
        self.port.unwrap_or(22)
    }

    pub fn connection_string(&self) -> String {
        let host = self.effective_host();
        let port = self.effective_port();

        if let Some(user) = &self.user {
            format!("{}@{}:{}", user, host, port)
        } else {
            format!("{}:{}", host, port)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_host_default() {
        let host = SshHost::default();
        assert!(host.pattern.is_empty());
        assert!(host.host_name.is_none());
        assert!(host.port.is_none());
    }

    #[test]
    fn test_ssh_host_new() {
        let host = SshHost::new("myserver");
        assert_eq!(host.pattern, "myserver");
    }

    #[test]
    fn test_pattern_matches_exact() {
        let host = SshHost::new("myserver");
        assert!(host.matches("myserver"));
        assert!(!host.matches("other"));
    }

    #[test]
    fn test_pattern_matches_wildcard() {
        let host = SshHost::new("*.example.com");
        assert!(host.matches("www.example.com"));
        assert!(host.matches("mail.example.com"));
        assert!(!host.matches("example.org"));
    }

    #[test]
    fn test_pattern_matches_star() {
        let host = SshHost::new("*");
        assert!(host.matches("anything"));
        assert!(host.matches("any.thing.com"));
    }

    #[test]
    fn test_pattern_matches_negation() {
        let host = SshHost::new("*.com !bad.com");
        assert!(host.matches("good.com"));
        assert!(!host.matches("bad.com"));
    }

    #[test]
    fn test_effective_host() {
        let mut host = SshHost::new("alias");
        assert_eq!(host.effective_host(), "alias");

        host.host_name = Some("real.example.com".to_string());
        assert_eq!(host.effective_host(), "real.example.com");
    }

    #[test]
    fn test_effective_port() {
        let host = SshHost::new("myserver");
        assert_eq!(host.effective_port(), 22);

        let mut host = SshHost::new("myserver");
        host.port = Some(2222);
        assert_eq!(host.effective_port(), 2222);
    }

    #[test]
    fn test_connection_string() {
        let mut host = SshHost::new("myserver");
        assert_eq!(host.connection_string(), "myserver:22");

        host.user = Some("admin".to_string());
        assert_eq!(host.connection_string(), "admin@myserver:22");

        host.port = Some(2222);
        assert_eq!(host.connection_string(), "admin@myserver:2222");
    }

    #[test]
    fn test_multiple_patterns() {
        let host = SshHost::new("server1 server2 server3");
        assert!(host.matches("server1"));
        assert!(host.matches("server2"));
        assert!(host.matches("server3"));
        assert!(!host.matches("server4"));
    }
}
