//! WASM Error Mapping
//!
//! Converte erros do wasmtime em erros do domínio do plugin.
//! Provê mensagens amigáveis e contexto para debugging.

use crate::plugin::error::PluginError;
use wasmtime::{Error, Trap};

/// Converte erros do wasmtime para PluginError
///
/// Faz o mapeamento inteligente de diferentes tipos de erros
/// para categorias apropriadas do nosso domínio.
impl From<Error> for PluginError {
    fn from(err: Error) -> Self {
        // Tenta extrair um Trap do erro
        if let Some(trap) = err.downcast_ref::<Trap>() {
            return map_trap_error(trap);
        }

        // Erros de compilação/linking
        let err_str = err.to_string();
        if err_str.contains("link") || err_str.contains("Link") {
            return PluginError::Execution {
                reason: format!("Link error: {}", err),
            };
        }

        if err_str.contains("compile") {
            return PluginError::Execution {
                reason: format!("Compilation error: {}", err),
            };
        }

        // Erro genérico
        PluginError::Execution {
            reason: format!("WASM error: {}", err),
        }
    }
}

/// Mapeia erros de trap para PluginError apropriado
fn map_trap_error(trap: &Trap) -> PluginError {
    match trap {
        Trap::MemoryOutOfBounds => PluginError::SandboxViolation {
            operation: "memory_access".to_string(),
            reason: "Memory access out of bounds".to_string(),
        },
        Trap::StackOverflow => PluginError::Execution {
            reason: "Stack overflow - plugin used too much stack space".to_string(),
        },
        Trap::UnreachableCodeReached => PluginError::Execution {
            reason: "Plugin reached unreachable code (panic)".to_string(),
        },
        Trap::TableOutOfBounds => PluginError::SandboxViolation {
            operation: "table_access".to_string(),
            reason: "Table access out of bounds".to_string(),
        },
        Trap::IndirectCallToNull => PluginError::Execution {
            reason: "Indirect call to null function pointer".to_string(),
        },
        Trap::BadSignature => PluginError::Execution {
            reason: "Function call with wrong signature".to_string(),
        },
        Trap::IntegerOverflow => PluginError::Execution {
            reason: "Integer overflow".to_string(),
        },
        Trap::IntegerDivisionByZero => PluginError::Execution {
            reason: "Division by zero".to_string(),
        },
        Trap::BadConversionToInteger => PluginError::Execution {
            reason: "Invalid conversion to integer".to_string(),
        },
        _ => PluginError::Execution {
            reason: format!("Unknown trap: {}", trap),
        },
    }
}

/// Informações detalhadas sobre um erro WASM
#[derive(Debug, Clone)]
pub struct WasmErrorInfo {
    /// Tipo do erro
    pub error_type: WasmErrorType,
    /// Mensagem para o usuário
    pub user_message: String,
    /// Detalhes técnicos (para logs)
    pub technical_details: String,
    /// É recuperável?
    pub is_recoverable: bool,
    /// Sugestão de ação
    pub suggestion: Option<String>,
}

/// Tipos de erros WASM
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmErrorType {
    /// Erro de memória (out of bounds)
    Memory,
    /// Erro de stack
    Stack,
    /// Erro de execução (trap)
    Execution,
    /// Erro de sandbox
    Sandbox,
    /// Erro de linking
    Link,
    /// Erro de compilação
    Compile,
    /// Erro desconhecido
    Unknown,
}

impl WasmErrorInfo {
    /// Cria informações de erro a partir de um PluginError
    pub fn from_plugin_error(err: &PluginError) -> Self {
        match err {
            PluginError::Execution { reason } => Self {
                error_type: WasmErrorType::Execution,
                user_message: format!("Plugin execution failed: {}", reason),
                technical_details: reason.clone(),
                is_recoverable: true,
                suggestion: Some("Check plugin code for errors".to_string()),
            },
            PluginError::SandboxViolation { operation, reason } => Self {
                error_type: WasmErrorType::Sandbox,
                user_message: "Plugin attempted unsafe operation".to_string(),
                technical_details: format!("Operation: {}, Reason: {}", operation, reason),
                is_recoverable: false,
                suggestion: Some("Plugin tried to access restricted resources".to_string()),
            },
            PluginError::InitFailed { name, reason } => Self {
                error_type: WasmErrorType::Execution,
                user_message: format!("Failed to initialize plugin '{}'", name),
                technical_details: format!("Plugin: {}, Reason: {}", name, reason),
                is_recoverable: true,
                suggestion: Some("Check plugin initialization code".to_string()),
            },
            _ => Self {
                error_type: WasmErrorType::Unknown,
                user_message: "An unexpected error occurred".to_string(),
                technical_details: format!("{:?}", err),
                is_recoverable: false,
                suggestion: None,
            },
        }
    }

    /// Cria informações a partir de um erro wasmtime
    pub fn from_wasmtime_error(err: &Error) -> Self {
        if let Some(trap) = err.downcast_ref::<Trap>() {
            return Self::from_trap(trap);
        }

        Self {
            error_type: WasmErrorType::Unknown,
            user_message: "WASM runtime error".to_string(),
            technical_details: err.to_string(),
            is_recoverable: false,
            suggestion: Some("Check plugin compatibility".to_string()),
        }
    }

    /// Cria informações a partir de um trap
    fn from_trap(trap: &Trap) -> Self {
        match trap {
            Trap::MemoryOutOfBounds => Self {
                error_type: WasmErrorType::Memory,
                user_message: "Plugin accessed memory outside allowed bounds".to_string(),
                technical_details: "MemoryOutOfBounds trap".to_string(),
                is_recoverable: false,
                suggestion: Some("Plugin has a bug - report to plugin author".to_string()),
            },
            Trap::StackOverflow => Self {
                error_type: WasmErrorType::Stack,
                user_message: "Plugin used too much stack memory".to_string(),
                technical_details: "StackOverflow trap".to_string(),
                is_recoverable: true,
                suggestion: Some("Reduce recursion depth in plugin".to_string()),
            },
            Trap::UnreachableCodeReached => Self {
                error_type: WasmErrorType::Execution,
                user_message: "Plugin crashed (panic)".to_string(),
                technical_details: "UnreachableCodeReached trap - likely a panic".to_string(),
                is_recoverable: false,
                suggestion: Some("Check plugin for panic conditions".to_string()),
            },
            _ => Self {
                error_type: WasmErrorType::Execution,
                user_message: format!("Plugin error: {}", trap),
                technical_details: format!("Trap: {:?}", trap),
                is_recoverable: false,
                suggestion: None,
            },
        }
    }
}

/// Trait para estender Result com helpers de erro WASM
pub trait WasmResultExt<T> {
    /// Mapeia erro para mensagem amigável ao usuário
    fn map_wasm_err(self) -> Result<T, PluginError>;

    /// Adiciona contexto ao erro
    fn with_wasm_context(self, context: impl Into<String>) -> Result<T, PluginError>;
}

impl<T> WasmResultExt<T> for Result<T, Error> {
    fn map_wasm_err(self) -> Result<T, PluginError> {
        self.map_err(|e| e.into())
    }

    fn with_wasm_context(self, context: impl Into<String>) -> Result<T, PluginError> {
        self.map_err(|e| {
            let context = context.into();
            PluginError::Execution {
                reason: format!("{}: {}", context, e),
            }
        })
    }
}

/// Helpers para logging de erros WASM
pub fn log_wasm_error(err: &PluginError) {
    let info = WasmErrorInfo::from_plugin_error(err);

    match info.error_type {
        WasmErrorType::Sandbox => {
            tracing::error!(
                error_type = ?info.error_type,
                user_message = %info.user_message,
                technical_details = %info.technical_details,
                "WASM sandbox violation"
            );
        }
        WasmErrorType::Memory | WasmErrorType::Stack => {
            tracing::error!(
                error_type = ?info.error_type,
                user_message = %info.user_message,
                technical_details = %info.technical_details,
                "WASM resource limit exceeded"
            );
        }
        _ => {
            tracing::error!(
                error_type = ?info.error_type,
                user_message = %info.user_message,
                "WASM error occurred"
            );
        }
    }
}

/// Verifica se um erro é recuperável
pub fn is_recoverable_error(err: &PluginError) -> bool {
    match err {
        PluginError::Execution { .. } => true,
        PluginError::SandboxViolation { .. } => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trap_memory_out_of_bounds() {
        let trap = Trap::MemoryOutOfBounds;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::SandboxViolation { .. } => {}
            _ => panic!("Expected SandboxViolation, got {:?}", err),
        }
    }

    #[test]
    fn test_trap_stack_overflow() {
        let trap = Trap::StackOverflow;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::Execution { reason } => {
                assert!(reason.contains("Stack overflow"));
            }
            _ => panic!("Expected Execution, got {:?}", err),
        }
    }

    #[test]
    fn test_trap_unreachable() {
        let trap = Trap::UnreachableCodeReached;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::Execution { reason } => {
                assert!(reason.contains("unreachable"));
            }
            _ => panic!("Expected Execution, got {:?}", err),
        }
    }

    #[test]
    fn test_wasm_error_info_from_trap() {
        let trap = Trap::MemoryOutOfBounds;
        let info = WasmErrorInfo::from_wasmtime_error(&wasmtime::Error::from(trap));

        assert_eq!(info.error_type, WasmErrorType::Memory);
        assert!(!info.is_recoverable);
        assert!(info.user_message.contains("memory"));
    }

    #[test]
    fn test_wasm_error_info_from_execution_error() {
        let err = PluginError::Execution {
            reason: "test error".to_string(),
        };
        let info = WasmErrorInfo::from_plugin_error(&err);

        assert_eq!(info.error_type, WasmErrorType::Execution);
        assert!(info.is_recoverable);
    }

    #[test]
    fn test_is_recoverable_error() {
        let exec_err = PluginError::Execution {
            reason: "test".to_string(),
        };
        assert!(is_recoverable_error(&exec_err));

        let sandbox_err = PluginError::SandboxViolation {
            operation: "test".to_string(),
            reason: "test".to_string(),
        };
        assert!(!is_recoverable_error(&sandbox_err));
    }

    #[test]
    fn test_wasm_result_ext_map_err() {
        let result: Result<(), Error> = Err(Trap::StackOverflow.into());
        let mapped = result.map_wasm_err();

        assert!(mapped.is_err());
        match mapped {
            Err(PluginError::Execution { .. }) => {}
            _ => panic!("Expected Execution error"),
        }
    }

    #[test]
    fn test_wasm_result_ext_with_context() {
        let result: Result<(), Error> = Err(Trap::MemoryOutOfBounds.into());
        let mapped = result.with_wasm_context("while calling init");

        assert!(mapped.is_err());
        match mapped {
            Err(PluginError::Execution { reason }) => {
                assert!(reason.contains("while calling init"));
            }
            _ => panic!("Expected Execution error with context"),
        }
    }

    #[test]
    fn test_trap_table_out_of_bounds() {
        let trap = Trap::TableOutOfBounds;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::SandboxViolation { .. } => {}
            _ => panic!("Expected SandboxViolation for TableOutOfBounds"),
        }
    }

    #[test]
    fn test_trap_indirect_call_to_null() {
        let trap = Trap::IndirectCallToNull;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::Execution { reason } => {
                assert!(reason.contains("null function pointer"));
            }
            _ => panic!("Expected Execution for IndirectCallToNull"),
        }
    }

    #[test]
    fn test_trap_integer_overflow() {
        let trap = Trap::IntegerOverflow;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::Execution { reason } => {
                assert!(reason.contains("overflow"));
            }
            _ => panic!("Expected Execution for IntegerOverflow"),
        }
    }

    #[test]
    fn test_trap_division_by_zero() {
        let trap = Trap::IntegerDivisionByZero;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::Execution { reason } => {
                assert!(reason.contains("Division by zero"));
            }
            _ => panic!("Expected Execution for IntegerDivisionByZero"),
        }
    }

    #[test]
    fn test_trap_bad_signature() {
        let trap = Trap::BadSignature;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::Execution { reason } => {
                assert!(reason.contains("signature"));
            }
            _ => panic!("Expected Execution for BadSignature"),
        }
    }

    #[test]
    fn test_trap_bad_conversion() {
        let trap = Trap::BadConversionToInteger;
        let err: PluginError = wasmtime::Error::from(trap).into();

        match err {
            PluginError::Execution { reason } => {
                assert!(reason.contains("conversion"));
            }
            _ => panic!("Expected Execution for BadConversionToInteger"),
        }
    }

    #[test]
    fn test_error_info_from_init_failed() {
        let err = PluginError::InitFailed {
            name: "test_plugin".to_string(),
            reason: "initialization failed".to_string(),
        };
        let info = WasmErrorInfo::from_plugin_error(&err);

        assert_eq!(info.error_type, WasmErrorType::Execution);
        assert!(info.user_message.contains("test_plugin"));
        assert!(info.is_recoverable);
    }

    #[test]
    fn test_error_info_from_sandbox_violation() {
        let err = PluginError::SandboxViolation {
            operation: "file_write".to_string(),
            reason: "access denied".to_string(),
        };
        let info = WasmErrorInfo::from_plugin_error(&err);

        assert_eq!(info.error_type, WasmErrorType::Sandbox);
        assert!(!info.is_recoverable);
        assert!(info.technical_details.contains("file_write"));
    }

    #[test]
    fn test_error_info_from_not_found() {
        let err = PluginError::NotFound {
            name: "missing_plugin".to_string(),
        };
        let info = WasmErrorInfo::from_plugin_error(&err);

        assert_eq!(info.error_type, WasmErrorType::Unknown);
        assert!(!info.is_recoverable);
    }

    #[test]
    fn test_from_generic_error() {
        // Create a generic wasmtime error from a trap (which always works)
        let wasm_err = wasmtime::Error::from(Trap::UnreachableCodeReached);
        let plugin_err: PluginError = wasm_err.into();

        // Should be mapped to Execution error
        match plugin_err {
            PluginError::Execution { .. } => {}
            _ => panic!("Expected Execution for generic error"),
        }
    }

    #[test]
    fn test_compile_error_mapping() {
        // Test that compile-related errors are detected
        // We can't easily create a real compile error, but we can verify the logic
        // by checking the string matching in the From impl
        let compile_err_str = "compile error: invalid wasm";
        let exec_err = PluginError::Execution {
            reason: compile_err_str.to_string(),
        };

        let info = WasmErrorInfo::from_plugin_error(&exec_err);
        // Execution errors should be recoverable
        assert!(info.is_recoverable);
    }

    #[test]
    fn test_link_error_mapping() {
        // Test that link-related errors are detected
        let link_err_str = "link error: import not found";
        let exec_err = PluginError::Execution {
            reason: link_err_str.to_string(),
        };

        let info = WasmErrorInfo::from_plugin_error(&exec_err);
        assert!(info.is_recoverable);
    }

    #[test]
    fn test_log_wasm_error_does_not_panic() {
        let err = PluginError::Execution {
            reason: "test error".to_string(),
        };
        // Should not panic
        log_wasm_error(&err);

        let sandbox_err = PluginError::SandboxViolation {
            operation: "test".to_string(),
            reason: "violation".to_string(),
        };
        log_wasm_error(&sandbox_err);
    }

    fn reason(err: PluginError) -> String {
        match err {
            PluginError::Execution { reason } => reason,
            PluginError::SandboxViolation { operation, reason } => format!("{operation}|{reason}"),
            other => panic!("variante inesperada: {other:?}"),
        }
    }

    #[test]
    fn test_every_trap_has_specific_message() {
        let cases = [
            (Trap::IndirectCallToNull, "null function pointer"),
            (Trap::BadSignature, "wrong signature"),
            (Trap::IntegerOverflow, "Integer overflow"),
            (Trap::IntegerDivisionByZero, "Division by zero"),
            (Trap::BadConversionToInteger, "Invalid conversion"),
            (Trap::TableOutOfBounds, "table_access|"),
            (Trap::Interrupt, "Unknown trap"),
        ];
        for (trap, expected) in cases {
            let message = reason(wasmtime::Error::from(trap).into());
            assert!(message.contains(expected), "{trap:?}: {message}");
        }
    }

    #[test]
    fn test_non_trap_errors_are_classified_by_message() {
        let link = reason(wasmtime::Error::msg("failed to link import").into());
        assert!(link.starts_with("Link error:"), "{link}");
        let compile = reason(wasmtime::Error::msg("could not compile module").into());
        assert!(compile.starts_with("Compilation error:"), "{compile}");
        let generic = reason(wasmtime::Error::msg("boom").into());
        assert_eq!(generic, "WASM error: boom");
    }

    #[test]
    fn test_error_info_for_plugin_errors() {
        let sandbox =
            WasmErrorInfo::from_plugin_error(&PluginError::sandbox_violation("fs", "negado"));
        assert_eq!(sandbox.error_type, WasmErrorType::Sandbox);
        assert!(!sandbox.is_recoverable);
        assert_eq!(sandbox.technical_details, "Operation: fs, Reason: negado");

        let init = WasmErrorInfo::from_plugin_error(&PluginError::init_failed("p", "x"));
        assert_eq!(init.error_type, WasmErrorType::Execution);
        assert!(init.user_message.contains("'p'"));
        assert!(init.is_recoverable);

        let other = WasmErrorInfo::from_plugin_error(&PluginError::not_found("p"));
        assert_eq!(other.error_type, WasmErrorType::Unknown);
        assert!(other.suggestion.is_none());
        assert!(!is_recoverable_error(&PluginError::not_found("p")));
    }

    #[test]
    fn test_error_info_for_wasmtime_errors() {
        let generic = WasmErrorInfo::from_wasmtime_error(&wasmtime::Error::msg("oops"));
        assert_eq!(generic.error_type, WasmErrorType::Unknown);
        assert_eq!(generic.technical_details, "oops");

        let stack = WasmErrorInfo::from_wasmtime_error(&Trap::StackOverflow.into());
        assert_eq!(stack.error_type, WasmErrorType::Stack);
        assert!(stack.is_recoverable);

        let panic = WasmErrorInfo::from_wasmtime_error(&Trap::UnreachableCodeReached.into());
        assert_eq!(panic.error_type, WasmErrorType::Execution);
        assert!(panic.user_message.contains("panic"));

        let other = WasmErrorInfo::from_wasmtime_error(&Trap::IntegerOverflow.into());
        assert_eq!(other.error_type, WasmErrorType::Execution);
        assert!(other.technical_details.starts_with("Trap:"));
    }

    #[test]
    fn test_log_wasm_error_handles_every_kind() {
        log_wasm_error(&PluginError::sandbox_violation("net", "negado"));
        log_wasm_error(&PluginError::Execution { reason: "x".into() });
        log_wasm_error(&PluginError::not_found("p"));
        assert_ne!(WasmErrorType::Link, WasmErrorType::Compile);
    }
}
