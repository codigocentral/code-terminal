//! Host Functions
//!
//! Este módulo define as funções que o host expõe para os plugins WASM.

pub mod config;
pub mod events;
pub mod executable;
pub mod logging;
pub mod mouse;
pub mod process;
pub mod registry;
pub mod search;
pub mod system;
pub mod terminal;
pub mod ui;

// Re-export das funções executáveis mais importantes
pub use executable::{
    ClearMode, CommandOutput, ExecutableHostFunctions, ExecutionContext, NotificationLevel,
    SandboxConfig, SystemExec, TerminalClear, TerminalCommand, TerminalRead, TerminalWrite,
    UiShowNotification,
};

// Re-export dos novos módulos de host functions
pub use mouse::{GetMousePos, MouseButton, SetMousePos, SimulateMouseClick, SimulateMouseScroll};
pub use process::{
    ProcessGetOutput, ProcessGetStatus, ProcessHandle, ProcessKill, ProcessList, ProcessManager,
    ProcessSpawn, ProcessStatus, ProcessWait,
};
pub use search::{
    GetSearchResults, SearchClear, SearchContext, SearchFind, SearchNext, SearchPrev, SearchResult,
};

use std::fmt;

pub const HOST_MODULE_NAME: &str = "host";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostFunctionId {
    pub category: HostFunctionCategory,
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostFunctionCategory {
    Terminal,
    Config,
    Events,
    Logging,
    Ui,
    System,
}

impl fmt::Display for HostFunctionCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Terminal => write!(f, "terminal"),
            Self::Config => write!(f, "config"),
            Self::Events => write!(f, "events"),
            Self::Logging => write!(f, "logging"),
            Self::Ui => write!(f, "ui"),
            Self::System => write!(f, "system"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFunctionPermission {
    Safe,
    Restricted,
    RequiresPermission,
    Dangerous,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostFunctionSignature {
    pub params: Vec<HostParamType>,
    pub return_type: Option<HostReturnType>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostParamType {
    I32,
    I64,
    F32,
    F64,
    Pointer,
    PointerWithLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostReturnType {
    I32,
    I64,
    F32,
    F64,
    Pointer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostFunctionError {
    InvalidParams,
    PermissionDenied,
    ExecutionError(String),
    NotFound,
}

impl fmt::Display for HostFunctionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidParams => write!(f, "Invalid parameters"),
            Self::PermissionDenied => write!(f, "Permission denied"),
            Self::ExecutionError(msg) => write!(f, "Execution error: {}", msg),
            Self::NotFound => write!(f, "Function not found"),
        }
    }
}

impl std::error::Error for HostFunctionError {}

pub trait HostFunction: Send + Sync {
    fn id(&self) -> HostFunctionId;
    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: None,
        }
    }
    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }
    fn description(&self) -> &'static str {
        ""
    }
    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError>;
}

pub struct HostFunctions {
    functions: Vec<Box<dyn HostFunction>>,
}

impl HostFunctions {
    pub fn new() -> Self {
        let mut functions = Vec::new();
        functions.extend(terminal::all_functions());
        functions.extend(config::all_functions());
        functions.extend(events::all_functions());
        functions.extend(logging::all_functions());
        functions.extend(ui::all_functions());
        functions.extend(system::all_functions());
        functions.extend(search::all_functions());
        functions.extend(process::all_functions());
        functions.extend(mouse::all_functions());

        Self { functions }
    }

    pub fn get(&self, id: &HostFunctionId) -> Option<&dyn HostFunction> {
        self.functions
            .iter()
            .find(|f| f.id() == *id)
            .map(|f| f.as_ref())
    }

    pub fn all(&self) -> &[Box<dyn HostFunction>] {
        &self.functions
    }

    pub fn by_category(&self, category: HostFunctionCategory) -> Vec<&dyn HostFunction> {
        self.functions
            .iter()
            .filter(|f| f.id().category == category)
            .map(|f| f.as_ref())
            .collect()
    }
}

impl Default for HostFunctions {
    fn default() -> Self {
        Self::new()
    }
}

// Re-exports do registry
pub use registry::{
    allocate_in_wasm, free_in_wasm, read_memory_caller, read_string_caller, write_memory_caller,
    write_string_caller, HostFunctionRegistry,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_functions_new() {
        let host = HostFunctions::new();
        assert!(!host.functions.is_empty());
    }

    #[test]
    fn test_host_function_id_equality() {
        let id1 = HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "write",
        };
        let id2 = HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "write",
        };
        let id3 = HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "read",
        };

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_host_function_category_display() {
        assert_eq!(HostFunctionCategory::Terminal.to_string(), "terminal");
        assert_eq!(HostFunctionCategory::Config.to_string(), "config");
        assert_eq!(HostFunctionCategory::System.to_string(), "system");
    }

    #[test]
    fn test_host_functions_by_category() {
        let host = HostFunctions::new();
        let terminal_funcs = host.by_category(HostFunctionCategory::Terminal);
        assert!(!terminal_funcs.is_empty());
    }

    #[test]
    fn test_host_function_error_display() {
        let err = HostFunctionError::InvalidParams;
        assert_eq!(err.to_string(), "Invalid parameters");

        let err = HostFunctionError::PermissionDenied;
        assert_eq!(err.to_string(), "Permission denied");
    }

    #[test]
    fn test_host_function_permission_default() {
        let perm = HostFunctionPermission::Safe;
        assert_eq!(perm, HostFunctionPermission::Safe);
    }

    #[test]
    fn test_every_host_function_has_unique_id_and_metadata() {
        let host = HostFunctions::default();
        let mut seen = std::collections::HashSet::new();
        for function in host.all() {
            let id = function.id();
            assert!(!id.name.is_empty());
            assert!(seen.insert(id), "id duplicado: {:?}", id);
            assert!(
                !function.description().trim().is_empty(),
                "{} sem descrição",
                id.name
            );
            // A assinatura é bem formada e estável entre chamadas
            assert_eq!(function.signature(), function.signature(), "{}", id.name);
            let _ = function.permission();

            // get() encontra exatamente esta função
            let found = host.get(&id).unwrap();
            assert_eq!(found.id(), id);
        }

        // As categorias particionam o conjunto
        let categories = [
            HostFunctionCategory::Terminal,
            HostFunctionCategory::Config,
            HostFunctionCategory::Events,
            HostFunctionCategory::Logging,
            HostFunctionCategory::Ui,
            HostFunctionCategory::System,
        ];
        let total: usize = categories.iter().map(|c| host.by_category(*c).len()).sum();
        assert_eq!(total, host.all().len());

        let missing = HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "nao_existe",
        };
        assert!(host.get(&missing).is_none());
    }

    #[test]
    fn test_category_and_error_display_all_variants() {
        let names: Vec<String> = [
            HostFunctionCategory::Terminal,
            HostFunctionCategory::Config,
            HostFunctionCategory::Events,
            HostFunctionCategory::Logging,
            HostFunctionCategory::Ui,
            HostFunctionCategory::System,
        ]
        .iter()
        .map(|c| c.to_string())
        .collect();
        assert_eq!(
            names,
            vec!["terminal", "config", "events", "logging", "ui", "system"]
        );

        assert_eq!(
            HostFunctionError::ExecutionError("x".into()).to_string(),
            "Execution error: x"
        );
        assert_eq!(
            HostFunctionError::NotFound.to_string(),
            "Function not found"
        );
        let boxed: Box<dyn std::error::Error> = Box::new(HostFunctionError::NotFound);
        assert_eq!(boxed.to_string(), "Function not found");
    }

    struct Minimal;
    impl HostFunction for Minimal {
        fn id(&self) -> HostFunctionId {
            HostFunctionId {
                category: HostFunctionCategory::Events,
                name: "minimal",
            }
        }
        fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
            Ok(params.to_vec())
        }
    }

    #[test]
    fn test_trait_defaults() {
        let f = Minimal;
        assert_eq!(
            f.signature(),
            HostFunctionSignature {
                params: vec![],
                return_type: None
            }
        );
        assert_eq!(f.permission(), HostFunctionPermission::Safe);
        assert_eq!(f.description(), "");
        assert_eq!(f.execute(b"eco").unwrap(), b"eco");
    }
}
