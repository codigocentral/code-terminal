//! Erros do Sistema de Plugins
//!
//! Define os tipos de erro que podem ocorrer no sistema de plugins.

use std::fmt;
use std::io;

/// Resultado de operações de plugin
pub type PluginResult<T> = Result<T, PluginError>;

/// Erros que podem ocorrer no sistema de plugins
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginError {
    /// Plugin não encontrado
    NotFound { name: String },
    /// Plugin já carregado
    AlreadyLoaded { name: String },
    /// Versão da API incompatível
    IncompatibleApi {
        host: String,
        plugin: String,
        reason: String,
    },
    /// Erro de inicialização
    InitFailed { name: String, reason: String },
    /// Erro de sandbox
    SandboxViolation { operation: String, reason: String },
    /// Erro de IO
    Io { operation: String, path: String },
    /// Manifest inválido
    InvalidManifest { reason: String },
    /// Dependência não satisfeita
    MissingDependency {
        plugin: String,
        dependency: String,
        required_version: String,
    },
    /// Erro de compilação (WASM)
    Compilation { reason: String },
    /// Erro de execução
    Execution { reason: String },
    /// Plugin desabilitado
    Disabled { name: String },
    /// Timeout
    Timeout { operation: String, duration_ms: u64 },
    /// Erro genérico
    Other { message: String },
}

impl PluginError {
    /// Cria um erro de plugin não encontrado
    pub fn not_found(name: impl Into<String>) -> Self {
        Self::NotFound { name: name.into() }
    }

    /// Cria um erro de plugin já carregado
    pub fn already_loaded(name: impl Into<String>) -> Self {
        Self::AlreadyLoaded { name: name.into() }
    }

    /// Cria um erro de API incompatível
    pub fn incompatible_api(
        host: impl Into<String>,
        plugin: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::IncompatibleApi {
            host: host.into(),
            plugin: plugin.into(),
            reason: reason.into(),
        }
    }

    /// Cria um erro de inicialização
    pub fn init_failed(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InitFailed {
            name: name.into(),
            reason: reason.into(),
        }
    }

    /// Cria um erro de violação de sandbox
    pub fn sandbox_violation(operation: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::SandboxViolation {
            operation: operation.into(),
            reason: reason.into(),
        }
    }

    /// Cria um erro de IO
    pub fn io(operation: impl Into<String>, path: impl Into<String>) -> Self {
        Self::Io {
            operation: operation.into(),
            path: path.into(),
        }
    }

    /// Cria um erro de manifest inválido
    pub fn invalid_manifest(reason: impl Into<String>) -> Self {
        Self::InvalidManifest {
            reason: reason.into(),
        }
    }

    /// Cria um erro de dependência não satisfeita
    pub fn missing_dependency(
        plugin: impl Into<String>,
        dependency: impl Into<String>,
        required_version: impl Into<String>,
    ) -> Self {
        Self::MissingDependency {
            plugin: plugin.into(),
            dependency: dependency.into(),
            required_version: required_version.into(),
        }
    }

    /// Retorna o nome do plugin relacionado ao erro (se houver)
    pub fn plugin_name(&self) -> Option<&str> {
        match self {
            Self::NotFound { name } => Some(name),
            Self::AlreadyLoaded { name } => Some(name),
            Self::InitFailed { name, .. } => Some(name),
            Self::Disabled { name } => Some(name),
            Self::MissingDependency { plugin, .. } => Some(plugin),
            _ => None,
        }
    }

    /// Retorna true se o erro é recuperável
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. } | Self::Disabled { .. } | Self::SandboxViolation { .. }
        )
    }

    /// Retorna true se o erro é fatal para o plugin
    pub fn is_fatal(&self) -> bool {
        !self.is_recoverable()
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound { name } => write!(f, "Plugin '{}' não encontrado", name),
            Self::AlreadyLoaded { name } => write!(f, "Plugin '{}' já está carregado", name),
            Self::IncompatibleApi {
                host,
                plugin,
                reason,
            } => {
                write!(
                    f,
                    "API incompatível: host={}, plugin={}. {}",
                    host, plugin, reason
                )
            }
            Self::InitFailed { name, reason } => {
                write!(f, "Falha ao inicializar plugin '{}': {}", name, reason)
            }
            Self::SandboxViolation { operation, reason } => {
                write!(f, "Violação de sandbox em '{}': {}", operation, reason)
            }
            Self::Io { operation, path } => {
                write!(f, "Erro de IO em '{}': {}", operation, path)
            }
            Self::InvalidManifest { reason } => {
                write!(f, "Manifest inválido: {}", reason)
            }
            Self::MissingDependency {
                plugin,
                dependency,
                required_version,
            } => {
                write!(
                    f,
                    "Plugin '{}' requer '{} {}'",
                    plugin, dependency, required_version
                )
            }
            Self::Compilation { reason } => {
                write!(f, "Erro de compilação: {}", reason)
            }
            Self::Execution { reason } => {
                write!(f, "Erro de execução: {}", reason)
            }
            Self::Disabled { name } => {
                write!(f, "Plugin '{}' está desabilitado", name)
            }
            Self::Timeout {
                operation,
                duration_ms,
            } => {
                write!(f, "Timeout em '{}' após {}ms", operation, duration_ms)
            }
            Self::Other { message } => write!(f, "{}", message),
        }
    }
}

impl std::error::Error for PluginError {}

impl From<io::Error> for PluginError {
    fn from(err: io::Error) -> Self {
        Self::Other {
            message: format!("IO error: {}", err),
        }
    }
}

impl From<String> for PluginError {
    fn from(message: String) -> Self {
        Self::Other { message }
    }
}

impl From<&str> for PluginError {
    fn from(message: &str) -> Self {
        Self::Other {
            message: message.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_not_found() {
        let err = PluginError::not_found("meu-plugin");
        assert_eq!(err.plugin_name(), Some("meu-plugin"));
        assert!(err.is_fatal());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_error_sandbox_violation() {
        let err = PluginError::sandbox_violation("file_write", "acesso negado");
        assert!(err.is_recoverable());
        assert!(!err.is_fatal());
    }

    #[test]
    fn test_error_timeout() {
        let err = PluginError::Timeout {
            operation: "init".to_string(),
            duration_ms: 5000,
        };
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_error_display() {
        let err = PluginError::not_found("test");
        assert_eq!(err.to_string(), "Plugin 'test' não encontrado");

        let err = PluginError::incompatible_api("1.0.0", "2.0.0", "major mismatch");
        assert!(err.to_string().contains("incompatível"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err: PluginError = io_err.into();
        match err {
            PluginError::Other { .. } => (),
            _ => panic!("Expected Other variant"),
        }
    }

    #[test]
    fn test_display_and_plugin_name_for_every_variant() {
        let cases: Vec<(PluginError, &str, Option<&str>)> = vec![
            (
                PluginError::already_loaded("a"),
                "Plugin 'a' já está carregado",
                Some("a"),
            ),
            (
                PluginError::init_failed("b", "motivo"),
                "Falha ao inicializar plugin 'b': motivo",
                Some("b"),
            ),
            (
                PluginError::sandbox_violation("op", "não"),
                "Violação de sandbox em 'op': não",
                None,
            ),
            (
                PluginError::io("ler", "/x"),
                "Erro de IO em 'ler': /x",
                None,
            ),
            (
                PluginError::invalid_manifest("sem nome"),
                "Manifest inválido: sem nome",
                None,
            ),
            (
                PluginError::missing_dependency("p", "dep", ">=1.0"),
                "Plugin 'p' requer 'dep >=1.0'",
                Some("p"),
            ),
            (
                PluginError::Compilation { reason: "c".into() },
                "Erro de compilação: c",
                None,
            ),
            (
                PluginError::Execution { reason: "e".into() },
                "Erro de execução: e",
                None,
            ),
            (
                PluginError::Disabled { name: "d".into() },
                "Plugin 'd' está desabilitado",
                Some("d"),
            ),
            (
                PluginError::Timeout {
                    operation: "init".into(),
                    duration_ms: 10,
                },
                "Timeout em 'init' após 10ms",
                None,
            ),
            (PluginError::from("texto"), "texto", None),
            (PluginError::from(String::from("dono")), "dono", None),
        ];
        for (err, display, name) in cases {
            assert_eq!(err.to_string(), display);
            assert_eq!(err.plugin_name(), name, "{display}");
        }
        let boxed: Box<dyn std::error::Error> = Box::new(PluginError::from("x"));
        assert_eq!(boxed.to_string(), "x");
        assert!(PluginError::Disabled { name: "d".into() }.is_recoverable());
        assert!(PluginError::invalid_manifest("x").is_fatal());
    }
}
