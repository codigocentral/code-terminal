//! Sistema de autenticação para controle remoto
//!
//! Suporta autenticação por token Bearer e opcionalmente por IP.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

/// Token de autenticação
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthToken(String);

impl AuthToken {
    /// Criar novo token
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// Verificar se token é válido (não vazio)
    pub fn is_valid(&self) -> bool {
        !self.0.is_empty()
    }

    /// Retornar string do token
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Gerar token aleatório seguro
    pub fn generate() -> Self {
        use rand::Rng;
        const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        const TOKEN_LEN: usize = 32;

        let mut rng = rand::thread_rng();
        let token: String = (0..TOKEN_LEN)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();

        Self(token)
    }
}

impl From<String> for AuthToken {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for AuthToken {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Credenciais de autenticação
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    /// Token de autenticação (Bearer)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// Endereço IP do cliente
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
}

impl Credentials {
    /// Criar credenciais com token
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: Some(token.into()),
            client_ip: None,
        }
    }

    /// Verificar se tem token
    pub fn has_token(&self) -> bool {
        self.token.as_ref().is_some_and(|t| !t.is_empty())
    }
}

/// Contexto de autenticação
#[derive(Debug, Clone)]
pub struct AuthContext {
    /// Cliente autenticado
    pub authenticated: bool,
    /// Token usado (se houver)
    pub token: Option<AuthToken>,
    /// IP do cliente
    pub client_ip: Option<IpAddr>,
    /// Timestamp da autenticação
    pub authenticated_at: Option<std::time::Instant>,
}

impl AuthContext {
    /// Criar contexto não autenticado
    pub fn unauthenticated() -> Self {
        Self {
            authenticated: false,
            token: None,
            client_ip: None,
            authenticated_at: None,
        }
    }

    /// Criar contexto autenticado
    pub fn authenticated(token: AuthToken, ip: Option<IpAddr>) -> Self {
        Self {
            authenticated: true,
            token: Some(token),
            client_ip: ip,
            authenticated_at: Some(std::time::Instant::now()),
        }
    }

    /// Verificar se está autenticado
    pub fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    /// Verificar se é anônimo (não autenticado)
    pub fn is_anonymous(&self) -> bool {
        !self.authenticated
    }
}

impl Default for AuthContext {
    fn default() -> Self {
        Self::unauthenticated()
    }
}

/// Configuração de autenticação
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthConfig {
    /// Habilitar autenticação
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Token de autenticação (se vazio, qualquer token é aceito em dev)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// IPs permitidos (whitelist)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_ips: Option<Vec<String>>,

    /// IPs bloqueados (blacklist)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_ips: Option<Vec<String>>,

    /// Permitir conexões locais (localhost) sem autenticação
    #[serde(default = "default_allow_localhost")]
    pub allow_localhost: bool,
}

fn default_enabled() -> bool {
    false
}

fn default_allow_localhost() -> bool {
    false
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            token: None,
            allowed_ips: None,
            blocked_ips: None,
            allow_localhost: default_allow_localhost(),
        }
    }
}

impl AuthConfig {
    /// Criar configuração com token específico
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            enabled: true,
            token: Some(token.into()),
            allowed_ips: None,
            blocked_ips: None,
            allow_localhost: false,
        }
    }

    /// Verificar se configuração requer autenticação
    pub fn requires_auth(&self) -> bool {
        self.enabled
    }

    /// Adicionar IP à whitelist
    pub fn allow_ip(&mut self, ip: impl Into<String>) {
        if self.allowed_ips.is_none() {
            self.allowed_ips = Some(Vec::new());
        }
        self.allowed_ips.as_mut().unwrap().push(ip.into());
    }

    /// Adicionar IP à blacklist
    pub fn block_ip(&mut self, ip: impl Into<String>) {
        if self.blocked_ips.is_none() {
            self.blocked_ips = Some(Vec::new());
        }
        self.blocked_ips.as_mut().unwrap().push(ip.into());
    }
}

/// Erros de autenticação
#[derive(Debug, Clone, PartialEq)]
pub enum AuthError {
    /// Autenticação necessária
    AuthenticationRequired,
    /// Token inválido
    InvalidToken,
    /// Token expirado
    TokenExpired,
    /// IP bloqueado
    IpBlocked,
    /// IP não permitido
    IpNotAllowed,
    /// Múltiplas falhas de autenticação
    TooManyAttempts,
    /// Erro interno
    Internal(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::AuthenticationRequired => write!(f, "Authentication required"),
            AuthError::InvalidToken => write!(f, "Invalid authentication token"),
            AuthError::TokenExpired => write!(f, "Token has expired"),
            AuthError::IpBlocked => write!(f, "IP address is blocked"),
            AuthError::IpNotAllowed => write!(f, "IP address not in allowlist"),
            AuthError::TooManyAttempts => write!(f, "Too many authentication attempts"),
            AuthError::Internal(msg) => write!(f, "Internal authentication error: {}", msg),
        }
    }
}

impl std::error::Error for AuthError {}

/// Verificador de autenticação
#[derive(Debug)]
pub struct AuthVerifier {
    config: AuthConfig,
    allowed_ips: HashSet<IpAddr>,
    blocked_ips: HashSet<IpAddr>,
    failed_attempts: Arc<RwLock<Vec<std::time::Instant>>>,
    max_attempts: usize,
    attempt_window: std::time::Duration,
}

impl AuthVerifier {
    /// Criar novo verificador
    pub fn new(config: AuthConfig) -> Self {
        let allowed_ips = config
            .allowed_ips
            .as_ref()
            .map(|ips| parse_ip_list(ips))
            .unwrap_or_default();

        let blocked_ips = config
            .blocked_ips
            .as_ref()
            .map(|ips| parse_ip_list(ips))
            .unwrap_or_default();

        Self {
            config,
            allowed_ips,
            blocked_ips,
            failed_attempts: Arc::new(RwLock::new(Vec::new())),
            max_attempts: 5,
            attempt_window: std::time::Duration::from_secs(300), // 5 minutos
        }
    }

    /// Verificar credenciais
    pub async fn verify(
        &self,
        credentials: &Credentials,
        client_ip: Option<IpAddr>,
    ) -> Result<AuthContext, AuthError> {
        // Verificar rate limiting
        self.check_rate_limit().await?;

        // Se autenticação está desabilitada, permitir
        if !self.config.enabled {
            return Ok(AuthContext::unauthenticated());
        }

        // Verificar IP
        if let Some(ip) = client_ip {
            // Verificar blacklist
            if self.blocked_ips.contains(&ip) {
                return Err(AuthError::IpBlocked);
            }

            // Verificar whitelist (se definida)
            if !self.allowed_ips.is_empty() && !self.allowed_ips.contains(&ip) {
                // Verificar se é localhost e está permitido
                if !self.is_localhost(ip) || !self.config.allow_localhost {
                    return Err(AuthError::IpNotAllowed);
                }
            }

            // Se é localhost e permitido, aceitar sem token
            if self.is_localhost(ip) && self.config.allow_localhost {
                return Ok(AuthContext::authenticated(
                    AuthToken::new("localhost"),
                    Some(ip),
                ));
            }
        }

        // Verificar token
        if !credentials.has_token() {
            self.record_failed_attempt().await;
            return Err(AuthError::AuthenticationRequired);
        }

        let provided_token = credentials.token.as_ref().unwrap();

        // Se não há token configurado e estamos em modo dev, aceitar qualquer token
        if self.config.token.is_none() {
            return Ok(AuthContext::authenticated(
                AuthToken::new(provided_token),
                client_ip,
            ));
        }

        // Verificar token contra o configurado
        let expected_token = self.config.token.as_ref().unwrap();
        if provided_token != expected_token {
            self.record_failed_attempt().await;
            return Err(AuthError::InvalidToken);
        }

        Ok(AuthContext::authenticated(
            AuthToken::new(provided_token),
            client_ip,
        ))
    }

    /// Verificar rate limiting
    async fn check_rate_limit(&self) -> Result<(), AuthError> {
        let attempts = self.failed_attempts.read().await;
        let now = std::time::Instant::now();
        let recent_attempts: Vec<_> = attempts
            .iter()
            .filter(|t| now.duration_since(**t) < self.attempt_window)
            .collect();

        if recent_attempts.len() >= self.max_attempts {
            return Err(AuthError::TooManyAttempts);
        }

        Ok(())
    }

    /// Registrar tentativa falha
    async fn record_failed_attempt(&self) {
        let mut attempts = self.failed_attempts.write().await;
        let now = std::time::Instant::now();

        // Limpar tentativas antigas
        attempts.retain(|t| now.duration_since(*t) < self.attempt_window);

        // Adicionar nova tentativa
        attempts.push(now);
    }

    /// Verificar se IP é localhost
    fn is_localhost(&self, ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => v4.is_loopback(),
            IpAddr::V6(v6) => v6.is_loopback(),
        }
    }

    /// Retornar configuração
    pub fn config(&self) -> &AuthConfig {
        &self.config
    }

    /// Verificar se autenticação está habilitada
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
}

/// Parse de lista de IPs
fn parse_ip_list(ips: &[String]) -> HashSet<IpAddr> {
    ips.iter()
        .filter_map(|ip| ip.parse::<IpAddr>().ok())
        .collect()
}

/// Extrair token de header Authorization
pub fn extract_bearer_token(auth_header: &str) -> Option<String> {
    auth_header
        .strip_prefix("Bearer ")
        .map(|token| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_token_creation() {
        let token = AuthToken::new("secret123");
        assert_eq!(token.as_str(), "secret123");
        assert!(token.is_valid());

        let empty = AuthToken::new("");
        assert!(!empty.is_valid());
    }

    #[test]
    fn test_auth_token_generation() {
        let token1 = AuthToken::generate();
        let token2 = AuthToken::generate();

        assert!(token1.is_valid());
        assert!(token2.is_valid());
        assert_ne!(token1.as_str(), token2.as_str());
        assert_eq!(token1.as_str().len(), 32);
    }

    #[test]
    fn test_credentials() {
        let creds = Credentials::with_token("test");
        assert!(creds.has_token());
        assert_eq!(creds.token, Some("test".to_string()));

        let empty = Credentials {
            token: None,
            client_ip: None,
        };
        assert!(!empty.has_token());
    }

    #[test]
    fn test_auth_context() {
        let anon = AuthContext::unauthenticated();
        assert!(!anon.is_authenticated());
        assert!(anon.is_anonymous());

        let auth = AuthContext::authenticated(AuthToken::new("token"), None);
        assert!(auth.is_authenticated());
        assert!(!auth.is_anonymous());
    }

    #[test]
    fn test_auth_config() {
        let config = AuthConfig::default();
        assert!(!config.requires_auth());

        let with_token = AuthConfig::with_token("secret");
        assert!(with_token.requires_auth());
        assert_eq!(with_token.token, Some("secret".to_string()));
    }

    #[test]
    fn test_extract_bearer_token() {
        assert_eq!(
            extract_bearer_token("Bearer secret123"),
            Some("secret123".to_string())
        );
        assert_eq!(extract_bearer_token("Basic secret123"), None);
        assert_eq!(extract_bearer_token("secret123"), None);
    }

    #[tokio::test]
    async fn test_auth_verifier_disabled() {
        let config = AuthConfig::default();
        let verifier = AuthVerifier::new(config);

        let creds = Credentials::with_token("anything");
        let result = verifier.verify(&creds, None).await;

        assert!(result.is_ok());
        assert!(!result.unwrap().is_authenticated());
    }

    #[tokio::test]
    async fn test_auth_verifier_valid_token() {
        let config = AuthConfig::with_token("correct");
        let verifier = AuthVerifier::new(config);

        let creds = Credentials::with_token("correct");
        let result = verifier.verify(&creds, None).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_authenticated());
    }

    #[tokio::test]
    async fn test_auth_verifier_invalid_token() {
        let config = AuthConfig::with_token("correct");
        let verifier = AuthVerifier::new(config);

        let creds = Credentials::with_token("wrong");
        let result = verifier.verify(&creds, None).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), AuthError::InvalidToken);
    }

    #[tokio::test]
    async fn test_auth_verifier_no_token() {
        let config = AuthConfig::with_token("secret");
        let verifier = AuthVerifier::new(config);

        let creds = Credentials {
            token: None,
            client_ip: None,
        };
        let result = verifier.verify(&creds, None).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), AuthError::AuthenticationRequired);
    }

    #[tokio::test]
    async fn test_auth_verifier_localhost() {
        let config = AuthConfig {
            enabled: true,
            token: Some("secret".to_string()),
            allow_localhost: true,
            ..Default::default()
        };
        let verifier = AuthVerifier::new(config);

        // Localhost deve passar sem token
        let creds = Credentials {
            token: None,
            client_ip: None,
        };
        let result = verifier
            .verify(&creds, Some("127.0.0.1".parse().unwrap()))
            .await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_authenticated());
    }

    #[test]
    fn test_auth_error_display() {
        assert!(AuthError::InvalidToken.to_string().contains("Invalid"));
        assert!(AuthError::AuthenticationRequired
            .to_string()
            .contains("required"));
        assert!(AuthError::IpBlocked.to_string().contains("blocked"));
    }
}
