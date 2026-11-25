//! Erros e resultados do sistema de hooks

use std::fmt;
use thiserror::Error;

/// Erros que podem ocorrer no sistema de hooks
#[derive(Error, Debug, Clone)]
pub enum HookError {
    /// Agente não encontrado
    #[error("Agente não encontrado: {0}")]
    AgentNotFound(String),

    /// Agente já registrado
    #[error("Agente já registrado: {0}")]
    AgentAlreadyRegistered(String),

    /// Agente não está ativo
    #[error("Agente não está ativo: {0}")]
    AgentNotActive(String),

    /// Timeout na comunicação
    #[error("Timeout após {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    /// Erro de comunicação
    #[error("Erro de comunicação: {0}")]
    CommunicationError(String),

    /// Hook não suportado
    #[error("Hook não suportado: {hook_type}")]
    UnsupportedHook { hook_type: String },

    /// Erro de serialização
    #[error("Erro de serialização: {0}")]
    SerializationError(String),

    /// Erro de configuração
    #[error("Erro de configuração: {0}")]
    ConfigError(String),

    /// Permissão negada
    #[error("Permissão negada: {0}")]
    PermissionDenied(String),

    /// Erro do agente específico
    #[error("Erro do agente {agent}: {message}")]
    AgentError { agent: String, message: String },

    /// Recurso não disponível
    #[error("Recurso não disponível: {0}")]
    ResourceUnavailable(String),

    /// Estado inválido
    #[error("Estado inválido: {0}")]
    InvalidState(String),

    /// Erro interno
    #[error("Erro interno: {0}")]
    InternalError(String),
}

impl HookError {
    /// Cria um erro de agente não encontrado
    pub fn agent_not_found(name: impl Into<String>) -> Self {
        HookError::AgentNotFound(name.into())
    }

    /// Cria um erro de timeout
    pub fn timeout(timeout_ms: u64) -> Self {
        HookError::Timeout { timeout_ms }
    }

    /// Cria um erro de comunicação
    pub fn communication(msg: impl Into<String>) -> Self {
        HookError::CommunicationError(msg.into())
    }

    /// Cria um erro de agente
    pub fn agent_error(agent: impl Into<String>, message: impl Into<String>) -> Self {
        HookError::AgentError {
            agent: agent.into(),
            message: message.into(),
        }
    }

    /// Cria um erro de configuração
    pub fn configuration_error(msg: impl Into<String>) -> Self {
        HookError::ConfigError(msg.into())
    }

    /// Verifica se o erro é recuperável
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            HookError::Timeout { .. }
                | HookError::CommunicationError(_)
                | HookError::AgentNotActive(_)
                | HookError::ResourceUnavailable(_)
        )
    }

    /// Verifica se o erro é de permissão
    pub fn is_permission_denied(&self) -> bool {
        matches!(self, HookError::PermissionDenied(_))
    }

    /// Verifica se o erro é de agente
    pub fn is_agent_error(&self) -> bool {
        matches!(self, HookError::AgentError { .. })
    }

    /// Retorna o código de erro para API
    pub fn error_code(&self) -> &'static str {
        match self {
            HookError::AgentNotFound(_) => "AGENT_NOT_FOUND",
            HookError::AgentAlreadyRegistered(_) => "AGENT_ALREADY_REGISTERED",
            HookError::AgentNotActive(_) => "AGENT_NOT_ACTIVE",
            HookError::Timeout { .. } => "TIMEOUT",
            HookError::CommunicationError(_) => "COMMUNICATION_ERROR",
            HookError::UnsupportedHook { .. } => "UNSUPPORTED_HOOK",
            HookError::SerializationError(_) => "SERIALIZATION_ERROR",
            HookError::ConfigError(_) => "CONFIG_ERROR",
            HookError::PermissionDenied(_) => "PERMISSION_DENIED",
            HookError::AgentError { .. } => "AGENT_ERROR",
            HookError::ResourceUnavailable(_) => "RESOURCE_UNAVAILABLE",
            HookError::InvalidState(_) => "INVALID_STATE",
            HookError::InternalError(_) => "INTERNAL_ERROR",
        }
    }
}

/// Resultado de operações de hook
pub type HookResult<T> = Result<T, HookError>;

/// Erro estendido com contexto
#[derive(Debug, Clone)]
pub struct ContextualError {
    /// Erro original
    pub error: HookError,
    /// Contexto adicional
    pub context: String,
    /// Timestamp do erro
    pub timestamp: std::time::SystemTime,
}

impl ContextualError {
    /// Cria um novo erro com contexto
    pub fn new(error: HookError, context: impl Into<String>) -> Self {
        Self {
            error,
            context: context.into(),
            timestamp: std::time::SystemTime::now(),
        }
    }

    /// Adiciona mais contexto
    pub fn with_context(mut self, additional: impl Into<String>) -> Self {
        self.context = format!("{} | {}", self.context, additional.into());
        self
    }
}

impl fmt::Display for ContextualError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (contexto: {})", self.error, self.context)
    }
}

impl std::error::Error for ContextualError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Helper para adicionar contexto a resultados
pub trait HookResultExt<T> {
    /// Adiciona contexto ao erro
    fn with_context(self, context: impl Into<String>) -> Result<T, ContextualError>;
}

impl<T> HookResultExt<T> for HookResult<T> {
    fn with_context(self, context: impl Into<String>) -> Result<T, ContextualError> {
        self.map_err(|e| ContextualError::new(e, context))
    }
}

/// Categorias de erro para tratamento diferenciado
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Erro fatal - requer reinicialização
    Fatal,
    /// Erro de transiente - pode ser tentado novamente
    Transient,
    /// Erro de validação - entrada inválida
    Validation,
    /// Erro de configuração - necessita ajuste
    Configuration,
    /// Erro de permissão - acesso negado
    Permission,
    /// Erro desconhecido
    Unknown,
}

impl HookError {
    /// Classifica o erro em uma categoria
    pub fn category(&self) -> ErrorCategory {
        match self {
            HookError::Timeout { .. }
            | HookError::CommunicationError(_)
            | HookError::ResourceUnavailable(_) => ErrorCategory::Transient,
            HookError::ConfigError(_) => ErrorCategory::Configuration,
            HookError::PermissionDenied(_) => ErrorCategory::Permission,
            HookError::SerializationError(_) | HookError::UnsupportedHook { .. } => {
                ErrorCategory::Validation
            }
            HookError::InternalError(_) | HookError::InvalidState(_) => ErrorCategory::Fatal,
            _ => ErrorCategory::Unknown,
        }
    }
}

// Extension trait para criar erros
pub trait HookErrorExt {
    fn config_error(msg: impl Into<String>) -> Self;
    fn internal_error(msg: impl Into<String>) -> Self;
    fn permission_denied(msg: impl Into<String>) -> Self;
    fn connection_error(msg: impl Into<String>) -> Self;
    fn configuration_error(msg: impl Into<String>) -> Self;
    fn agent_error(msg: impl Into<String>) -> Self;
    fn serialization_error(msg: impl Into<String>) -> Self;
}

impl HookErrorExt for HookError {
    fn config_error(msg: impl Into<String>) -> Self {
        HookError::ConfigError(msg.into())
    }

    fn internal_error(msg: impl Into<String>) -> Self {
        HookError::InternalError(msg.into())
    }

    fn permission_denied(msg: impl Into<String>) -> Self {
        HookError::PermissionDenied(msg.into())
    }

    fn connection_error(msg: impl Into<String>) -> Self {
        HookError::CommunicationError(msg.into())
    }

    fn configuration_error(msg: impl Into<String>) -> Self {
        HookError::ConfigError(msg.into())
    }

    fn agent_error(msg: impl Into<String>) -> Self {
        HookError::AgentError {
            agent: "unknown".to_string(),
            message: msg.into(),
        }
    }

    fn serialization_error(msg: impl Into<String>) -> Self {
        HookError::SerializationError(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        let err = HookError::agent_not_found("test");
        assert_eq!(err.error_code(), "AGENT_NOT_FOUND");

        let err = HookError::timeout(5000);
        assert_eq!(err.error_code(), "TIMEOUT");

        let err = HookError::permission_denied("test");
        assert_eq!(err.error_code(), "PERMISSION_DENIED");
    }

    #[test]
    fn test_is_recoverable() {
        assert!(HookError::timeout(1000).is_recoverable());
        assert!(HookError::communication("test").is_recoverable());
        assert!(!HookError::agent_not_found("test").is_recoverable());
        assert!(!HookError::permission_denied("test").is_recoverable());
    }

    #[test]
    fn test_error_categories() {
        assert_eq!(
            HookError::timeout(1000).category(),
            ErrorCategory::Transient
        );
        assert_eq!(
            HookError::config_error("test").category(),
            ErrorCategory::Configuration
        );
        assert_eq!(
            HookError::permission_denied("test").category(),
            ErrorCategory::Permission
        );
        assert_eq!(
            HookError::internal_error("test").category(),
            ErrorCategory::Fatal
        );
    }

    #[test]
    fn test_contextual_error() {
        let inner = HookError::agent_not_found("claude");
        let ctx = ContextualError::new(inner, "registrando agente");

        assert!(ctx.context.contains("registrando"));
        assert!(ctx.to_string().contains("contexto"));
    }
}
