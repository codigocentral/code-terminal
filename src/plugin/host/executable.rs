//! Host Functions Executáveis
//!
//! Implementação das host functions com execute() funcional,
//! permitindo que plugins WASM interajam com o terminal.

use std::sync::Arc;
use tokio::sync::RwLock;

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};

/// Contexto de execução para host functions
#[derive(Clone)]
pub struct ExecutionContext {
    /// Referência ao estado do terminal (weak para evitar ciclos)
    pub terminal_state: Option<Arc<RwLock<dyn TerminalState>>>,
    /// Canal para enviar comandos ao terminal
    pub command_tx: Option<tokio::sync::mpsc::Sender<TerminalCommand>>,
    /// ID do plugin executando
    pub plugin_id: String,
    /// Permissões concedidas ao plugin
    pub permissions: Vec<String>,
}

impl std::fmt::Debug for ExecutionContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExecutionContext")
            .field(
                "terminal_state",
                &self.terminal_state.as_ref().map(|_| "<TerminalState>"),
            )
            .field("command_tx", &self.command_tx)
            .field("plugin_id", &self.plugin_id)
            .field("permissions", &self.permissions)
            .finish()
    }
}

impl ExecutionContext {
    pub fn new(plugin_id: impl Into<String>) -> Self {
        Self {
            terminal_state: None,
            command_tx: None,
            plugin_id: plugin_id.into(),
            permissions: Vec::new(),
        }
    }

    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn has_permission(&self, perm: &str) -> bool {
        self.permissions.contains(&perm.to_string())
    }
}

/// Trait para estado do terminal
pub trait TerminalState: Send + Sync {
    fn write_output(&self, data: &[u8]) -> Result<usize, String>;
    fn read_input(&self, buf: &mut [u8]) -> Result<usize, String>;
    fn clear_buffer(&self, mode: ClearMode) -> Result<(), String>;
    fn show_notification(&self, level: NotificationLevel, title: &str, message: &str);
    fn execute_command(&self, command: &str, args: &[&str]) -> Result<CommandOutput, String>;
}

/// Modo de limpeza do buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ClearMode {
    Screen = 0,
    Scrollback = 1,
    All = 2,
}

impl ClearMode {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Screen),
            1 => Some(Self::Scrollback),
            2 => Some(Self::All),
            _ => None,
        }
    }
}

/// Nível de notificação
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationLevel {
    Info = 0,
    Success = 1,
    Warning = 2,
    Error = 3,
}

impl NotificationLevel {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Info),
            1 => Some(Self::Success),
            2 => Some(Self::Warning),
            3 => Some(Self::Error),
            _ => None,
        }
    }
}

/// Output de comando executado
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

/// Comandos que podem ser enviados ao terminal
#[derive(Debug, Clone)]
pub enum TerminalCommand {
    Write(Vec<u8>),
    Clear(ClearMode),
    ShowNotification {
        level: u32,
        title: String,
        message: String,
    },
    Execute {
        command: String,
        args: Vec<String>,
    },
}

/// Calcula `start + len` sem estourar; tamanhos negativos vindos do plugin
/// (convertidos para `usize` enorme) viram `InvalidParams` em vez de pânico.
fn checked_end(start: usize, len: usize) -> Result<usize, HostFunctionError> {
    start
        .checked_add(len)
        .ok_or(HostFunctionError::InvalidParams)
}

// ============================================================================
// 1. Terminal Write
// ============================================================================

pub struct TerminalWrite {
    ctx: Arc<RwLock<ExecutionContext>>,
}

impl TerminalWrite {
    pub fn new(ctx: Arc<RwLock<ExecutionContext>>) -> Self {
        Self { ctx }
    }
}

impl HostFunction for TerminalWrite {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "terminal_write",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Escreve dados no terminal (stdout do processo filho). Parâmetro: (data_ptr, data_len). Retorna: bytes escritos ou -1 em erro."
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        // Parâmetro: string a ser escrita
        let data =
            String::from_utf8(params.to_vec()).map_err(|_| HostFunctionError::InvalidParams)?;

        let ctx = self.ctx.blocking_read();

        // Verifica permissão
        if !ctx.has_permission("terminal:write") {
            return Err(HostFunctionError::PermissionDenied);
        }

        // Envia comando ao terminal via canal
        if let Some(ref tx) = ctx.command_tx {
            let bytes = data.as_bytes().to_vec();
            let len = bytes.len() as i32;

            match tx.try_send(TerminalCommand::Write(bytes)) {
                Ok(_) => {
                    // Retorna número de bytes escritos como i32 little-endian
                    Ok(len.to_le_bytes().to_vec())
                }
                Err(_) => Err(HostFunctionError::ExecutionError(
                    "Failed to send write command".to_string(),
                )),
            }
        } else {
            Err(HostFunctionError::ExecutionError(
                "Terminal not available".to_string(),
            ))
        }
    }
}

// ============================================================================
// 2. Terminal Read
// ============================================================================

pub struct TerminalRead {
    ctx: Arc<RwLock<ExecutionContext>>,
    input_buffer: Arc<RwLock<Vec<u8>>>,
}

impl TerminalRead {
    pub fn new(ctx: Arc<RwLock<ExecutionContext>>, input_buffer: Arc<RwLock<Vec<u8>>>) -> Self {
        Self { ctx, input_buffer }
    }
}

impl HostFunction for TerminalRead {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "terminal_read",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Lê input do terminal. Parâmetro: (out_ptr, max_len). Retorna: bytes lidos ou -1 em erro."
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        // Parâmetros: max_len (i32) - quantidade máxima de bytes a ler
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let max_len = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as usize;

        let ctx = self.ctx.blocking_read();

        // Verifica permissão
        if !ctx.has_permission("terminal:read") {
            return Err(HostFunctionError::PermissionDenied);
        }

        // Lê do buffer de input
        let mut buffer = self.input_buffer.blocking_write();
        let to_read = max_len.min(buffer.len());

        if to_read == 0 {
            // Não há dados disponíveis
            return Ok((-1i32).to_le_bytes().to_vec());
        }

        let data: Vec<u8> = buffer.drain(0..to_read).collect();
        let len_read = data.len() as i32;

        // Retorna: [len_read (4 bytes)] [data (len_read bytes)]
        let mut result = len_read.to_le_bytes().to_vec();
        result.extend(data);

        Ok(result)
    }
}

// ============================================================================
// 3. Terminal Clear
// ============================================================================

pub struct TerminalClear {
    ctx: Arc<RwLock<ExecutionContext>>,
}

impl TerminalClear {
    pub fn new(ctx: Arc<RwLock<ExecutionContext>>) -> Self {
        Self { ctx }
    }
}

impl HostFunction for TerminalClear {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Terminal,
            name: "terminal_clear",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32], // mode: 0=screen, 1=scrollback, 2=all
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Limpa o terminal. Parâmetro: mode (0=screen, 1=scrollback, 2=all). Retorna: 0 em sucesso, -1 em erro."
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        // Parâmetro: mode (i32)
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let mode_val = i32::from_le_bytes([params[0], params[1], params[2], params[3]]);
        let mode = ClearMode::from_u32(mode_val as u32).ok_or(HostFunctionError::InvalidParams)?;

        let ctx = self.ctx.blocking_read();

        // Verifica permissão
        if !ctx.has_permission("terminal:clear") {
            return Err(HostFunctionError::PermissionDenied);
        }

        // Envia comando ao terminal
        if let Some(ref tx) = ctx.command_tx {
            match tx.try_send(TerminalCommand::Clear(mode)) {
                Ok(_) => Ok(0i32.to_le_bytes().to_vec()),
                Err(_) => Err(HostFunctionError::ExecutionError(
                    "Failed to send clear command".to_string(),
                )),
            }
        } else {
            Err(HostFunctionError::ExecutionError(
                "Terminal not available".to_string(),
            ))
        }
    }
}

// ============================================================================
// 4. UI Show Notification
// ============================================================================

pub struct UiShowNotification {
    ctx: Arc<RwLock<ExecutionContext>>,
}

impl UiShowNotification {
    pub fn new(ctx: Arc<RwLock<ExecutionContext>>) -> Self {
        Self { ctx }
    }
}

impl HostFunction for UiShowNotification {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "show_notification",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::I32,               // level
                HostParamType::PointerWithLength, // title
                HostParamType::PointerWithLength, // message
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Mostra notificação na UI. Parâmetros: (level, title, message). Retorna: 0 em sucesso."
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        // Estrutura dos parâmetros:
        // [level: i32 (4 bytes)] [title_len: i32 (4 bytes)] [title: bytes] [message_len: i32 (4 bytes)] [message: bytes]
        if params.len() < 12 {
            return Err(HostFunctionError::InvalidParams);
        }

        let level = i32::from_le_bytes([params[0], params[1], params[2], params[3]]);
        let title_len = i32::from_le_bytes([params[4], params[5], params[6], params[7]]) as usize;

        let title_start = 8;
        let title_end = checked_end(title_start, title_len)?;

        if params.len() < checked_end(title_end, 4)? {
            return Err(HostFunctionError::InvalidParams);
        }

        let title = String::from_utf8(params[title_start..title_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let message_len = i32::from_le_bytes([
            params[title_end],
            params[title_end + 1],
            params[title_end + 2],
            params[title_end + 3],
        ]) as usize;

        let message_start = title_end + 4;
        let message_end = checked_end(message_start, message_len)?;

        if params.len() < message_end {
            return Err(HostFunctionError::InvalidParams);
        }

        let message = String::from_utf8(params[message_start..message_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let ctx = self.ctx.blocking_read();

        // Verifica permissão
        if !ctx.has_permission("ui:notification") {
            return Err(HostFunctionError::PermissionDenied);
        }

        // Envia comando ao terminal
        if let Some(ref tx) = ctx.command_tx {
            match tx.try_send(TerminalCommand::ShowNotification {
                level: level as u32,
                title,
                message,
            }) {
                Ok(_) => Ok(0i32.to_le_bytes().to_vec()),
                Err(_) => Err(HostFunctionError::ExecutionError(
                    "Failed to send notification".to_string(),
                )),
            }
        } else {
            Err(HostFunctionError::ExecutionError(
                "UI not available".to_string(),
            ))
        }
    }
}

// ============================================================================
// 5. System Exec (Sandboxed)
// ============================================================================

/// Configuração do sandbox para execução de comandos
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Comandos permitidos (whitelist)
    pub allowed_commands: Vec<String>,
    /// Diretórios permitidos para acesso
    pub allowed_dirs: Vec<String>,
    /// Timeout padrão em segundos
    pub timeout_secs: u64,
    /// Máximo de memória permitida (MB)
    pub max_memory_mb: u64,
    /// Permitir acesso à rede
    pub allow_network: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            allowed_commands: vec![
                "echo".to_string(),
                "cat".to_string(),
                "ls".to_string(),
                "pwd".to_string(),
                "grep".to_string(),
                "head".to_string(),
                "tail".to_string(),
                "wc".to_string(),
                "sort".to_string(),
                "uniq".to_string(),
                "date".to_string(),
                "whoami".to_string(),
                "which".to_string(),
            ],
            allowed_dirs: vec!["/tmp".to_string(), "/var/tmp".to_string()],
            timeout_secs: 30,
            max_memory_mb: 64,
            allow_network: false,
        }
    }
}

pub struct SystemExec {
    ctx: Arc<RwLock<ExecutionContext>>,
    sandbox: SandboxConfig,
}

impl SystemExec {
    pub fn new(ctx: Arc<RwLock<ExecutionContext>>) -> Self {
        Self {
            ctx,
            sandbox: SandboxConfig::default(),
        }
    }

    pub fn with_sandbox(mut self, sandbox: SandboxConfig) -> Self {
        self.sandbox = sandbox;
        self
    }

    /// Valida se um comando é permitido no sandbox
    fn is_command_allowed(&self, cmd: &str) -> bool {
        let cmd_name = cmd.split_whitespace().next().unwrap_or(cmd);
        let cmd_base = std::path::Path::new(cmd_name)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(cmd_name);

        self.sandbox
            .allowed_commands
            .iter()
            .any(|allowed| allowed == cmd_base)
    }
}

impl HostFunction for SystemExec {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "system_exec",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength, // command
                HostParamType::PointerWithLength, // args (JSON array)
            ],
            return_type: Some(HostReturnType::Pointer), // JSON: {stdout, stderr, exit_code}
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Executa comando do sistema (sandboxed). Parâmetros: (command, args_json). Retorna: JSON com stdout, stderr, exit_code."
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        // Parse dos parâmetros: [cmd_len (4)] [cmd_bytes] [args_len (4)] [args_bytes]
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }

        let cmd_len = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as usize;
        let cmd_start = 4;
        let cmd_end = checked_end(cmd_start, cmd_len)?;

        if params.len() < checked_end(cmd_end, 4)? {
            return Err(HostFunctionError::InvalidParams);
        }

        let command = String::from_utf8(params[cmd_start..cmd_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let args_len = i32::from_le_bytes([
            params[cmd_end],
            params[cmd_end + 1],
            params[cmd_end + 2],
            params[cmd_end + 3],
        ]) as usize;
        let args_start = cmd_end + 4;
        let args_end = checked_end(args_start, args_len)?;

        if params.len() < args_end {
            return Err(HostFunctionError::InvalidParams);
        }

        let args_json = String::from_utf8(params[args_start..args_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let args: Vec<String> =
            serde_json::from_str(&args_json).map_err(|_| HostFunctionError::InvalidParams)?;

        let ctx = self.ctx.blocking_read();

        // Verifica permissão especial para execução
        if !ctx.has_permission("system:exec") {
            return Err(HostFunctionError::PermissionDenied);
        }

        // Validação do sandbox - verifica se comando está na whitelist
        if !self.is_command_allowed(&command) {
            let result = serde_json::json!({
                "stdout": "",
                "stderr": format!("Command '{}' is not allowed in sandbox", command),
                "exit_code": 126,
            });
            return Ok(result.to_string().into_bytes());
        }

        // Envia comando ao terminal para execução
        if let Some(ref tx) = ctx.command_tx {
            match tx.try_send(TerminalCommand::Execute { command, args }) {
                Ok(_) => {
                    // Retorna resultado assíncrono
                    // Na prática, o resultado viria de outra forma (callback ou polling)
                    let result = serde_json::json!({
                        "stdout": "",
                        "stderr": "",
                        "exit_code": 0,
                        "status": "submitted",
                    });
                    Ok(result.to_string().into_bytes())
                }
                Err(_) => {
                    let result = serde_json::json!({
                        "stdout": "",
                        "stderr": "Failed to submit command",
                        "exit_code": -1,
                    });
                    Ok(result.to_string().into_bytes())
                }
            }
        } else {
            let result = serde_json::json!({
                "stdout": "",
                "stderr": "Terminal not available",
                "exit_code": -1,
            });
            Ok(result.to_string().into_bytes())
        }
    }
}

// ============================================================================
// Factory para criar funções executáveis
// ============================================================================

pub struct ExecutableHostFunctions;

impl ExecutableHostFunctions {
    /// Cria todas as 5 host functions executáveis
    pub fn create_all(
        ctx: Arc<RwLock<ExecutionContext>>,
        input_buffer: Arc<RwLock<Vec<u8>>>,
    ) -> Vec<Box<dyn HostFunction>> {
        vec![
            Box::new(TerminalWrite::new(ctx.clone())),
            Box::new(TerminalRead::new(ctx.clone(), input_buffer)),
            Box::new(TerminalClear::new(ctx.clone())),
            Box::new(UiShowNotification::new(ctx.clone())),
            Box::new(SystemExec::new(ctx)),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clear_mode_from_u32() {
        assert_eq!(ClearMode::from_u32(0), Some(ClearMode::Screen));
        assert_eq!(ClearMode::from_u32(1), Some(ClearMode::Scrollback));
        assert_eq!(ClearMode::from_u32(2), Some(ClearMode::All));
        assert_eq!(ClearMode::from_u32(99), None);
    }

    #[test]
    fn test_notification_level_from_u32() {
        assert_eq!(
            NotificationLevel::from_u32(0),
            Some(NotificationLevel::Info)
        );
        assert_eq!(
            NotificationLevel::from_u32(1),
            Some(NotificationLevel::Success)
        );
        assert_eq!(
            NotificationLevel::from_u32(3),
            Some(NotificationLevel::Error)
        );
        assert_eq!(NotificationLevel::from_u32(99), None);
    }

    #[test]
    fn test_sandbox_default_allows_safe_commands() {
        let sandbox = SandboxConfig::default();
        assert!(sandbox.allowed_commands.contains(&"echo".to_string()));
        assert!(sandbox.allowed_commands.contains(&"ls".to_string()));
        assert!(sandbox.allowed_commands.contains(&"cat".to_string()));
    }

    #[test]
    fn test_system_exec_is_dangerous() {
        let ctx = Arc::new(RwLock::new(ExecutionContext::new("test")));
        let exec = SystemExec::new(ctx);
        assert_eq!(exec.permission(), HostFunctionPermission::Dangerous);
    }

    #[test]
    fn test_terminal_write_is_restricted() {
        let ctx = Arc::new(RwLock::new(ExecutionContext::new("test")));
        let write = TerminalWrite::new(ctx);
        assert_eq!(write.permission(), HostFunctionPermission::Restricted);
    }

    // ---------------- helpers ----------------

    type Rx = tokio::sync::mpsc::Receiver<TerminalCommand>;

    fn ctx_with(
        perms: &[&str],
        capacity: Option<usize>,
    ) -> (Arc<RwLock<ExecutionContext>>, Option<Rx>) {
        let mut ctx = ExecutionContext::new("plugin-x")
            .with_permissions(perms.iter().map(|p| p.to_string()).collect());
        let rx = capacity.map(|cap| {
            let (tx, rx) = tokio::sync::mpsc::channel(cap);
            ctx.command_tx = Some(tx);
            rx
        });
        (Arc::new(RwLock::new(ctx)), rx)
    }

    fn i32_of(bytes: &[u8]) -> i32 {
        i32::from_le_bytes(bytes[..4].try_into().unwrap())
    }

    fn lp(bytes: &[u8]) -> Vec<u8> {
        let mut v = (bytes.len() as i32).to_le_bytes().to_vec();
        v.extend_from_slice(bytes);
        v
    }

    fn notification_params(level: i32, title: &[u8], message: &[u8]) -> Vec<u8> {
        let mut p = level.to_le_bytes().to_vec();
        p.extend(lp(title));
        p.extend(lp(message));
        p
    }

    fn exec_params(cmd: &[u8], args: &[u8]) -> Vec<u8> {
        let mut p = lp(cmd);
        p.extend(lp(args));
        p
    }

    fn json(bytes: Vec<u8>) -> serde_json::Value {
        serde_json::from_slice(&bytes).unwrap()
    }

    // ---------------- ExecutionContext ----------------

    #[test]
    fn test_execution_context_permissions_and_debug() {
        let ctx = ExecutionContext::new("p1").with_permissions(vec!["terminal:write".into()]);
        assert!(ctx.has_permission("terminal:write"));
        assert!(!ctx.has_permission("terminal:read"));

        let dbg = format!("{:?}", ctx);
        assert!(dbg.contains("ExecutionContext"));
        assert!(dbg.contains("p1"));
        assert!(dbg.contains("terminal:write"));
        assert!(dbg.contains("terminal_state: None"));
    }

    struct FakeState;
    impl TerminalState for FakeState {
        fn write_output(&self, data: &[u8]) -> Result<usize, String> {
            Ok(data.len())
        }
        fn read_input(&self, _buf: &mut [u8]) -> Result<usize, String> {
            Ok(0)
        }
        fn clear_buffer(&self, _mode: ClearMode) -> Result<(), String> {
            Ok(())
        }
        fn show_notification(&self, _level: NotificationLevel, _title: &str, _message: &str) {}
        fn execute_command(&self, _command: &str, _args: &[&str]) -> Result<CommandOutput, String> {
            Ok(CommandOutput {
                stdout: vec![],
                stderr: vec![],
                exit_code: 0,
            })
        }
    }

    #[test]
    fn test_execution_context_debug_hides_terminal_state() {
        let mut ctx = ExecutionContext::new("p2");
        let state: Arc<RwLock<dyn TerminalState>> = Arc::new(RwLock::new(FakeState));
        ctx.terminal_state = Some(state);
        let dbg = format!("{:?}", ctx);
        assert!(dbg.contains("<TerminalState>"), "{dbg}");
    }

    #[test]
    fn test_notification_level_warning() {
        assert_eq!(
            NotificationLevel::from_u32(2),
            Some(NotificationLevel::Warning)
        );
    }

    // ---------------- metadata ----------------

    #[test]
    fn test_metadata_of_all_functions() {
        let (ctx, _) = ctx_with(&[], None);
        let functions = ExecutableHostFunctions::create_all(ctx, Arc::new(RwLock::new(Vec::new())));
        let expected = [
            (
                "terminal_write",
                HostFunctionCategory::Terminal,
                HostFunctionPermission::Restricted,
                1,
            ),
            (
                "terminal_read",
                HostFunctionCategory::Terminal,
                HostFunctionPermission::Restricted,
                2,
            ),
            (
                "terminal_clear",
                HostFunctionCategory::Terminal,
                HostFunctionPermission::Restricted,
                1,
            ),
            (
                "show_notification",
                HostFunctionCategory::Ui,
                HostFunctionPermission::Restricted,
                3,
            ),
            (
                "system_exec",
                HostFunctionCategory::System,
                HostFunctionPermission::Dangerous,
                2,
            ),
        ];
        assert_eq!(functions.len(), expected.len());
        for (f, (name, category, permission, params)) in functions.iter().zip(expected) {
            let id = f.id();
            assert_eq!(id.name, name);
            assert_eq!(id.category, category, "{name}");
            assert_eq!(f.permission(), permission, "{name}");
            assert_eq!(f.signature().params.len(), params, "{name}");
            assert!(f.signature().return_type.is_some(), "{name}");
            assert!(!f.description().is_empty(), "{name}");
        }
    }

    // ---------------- TerminalWrite ----------------

    #[test]
    fn test_terminal_write_sends_command() {
        let (ctx, rx) = ctx_with(&["terminal:write"], Some(4));
        let mut rx = rx.unwrap();
        let out = TerminalWrite::new(ctx).execute(b"ola").unwrap();
        assert_eq!(i32_of(&out), 3);
        match rx.try_recv().unwrap() {
            TerminalCommand::Write(data) => assert_eq!(data, b"ola"),
            other => panic!("comando inesperado: {other:?}"),
        }
    }

    #[test]
    fn test_terminal_write_errors() {
        // UTF-8 inválido
        let (ctx, _rx) = ctx_with(&["terminal:write"], Some(1));
        assert_eq!(
            TerminalWrite::new(ctx.clone()).execute(&[0xff]),
            Err(HostFunctionError::InvalidParams)
        );

        // Sem permissão
        let (no_perm, _rx2) = ctx_with(&[], Some(1));
        assert_eq!(
            TerminalWrite::new(no_perm).execute(b"x"),
            Err(HostFunctionError::PermissionDenied)
        );

        // Sem terminal
        let (no_tx, _) = ctx_with(&["terminal:write"], None);
        assert_eq!(
            TerminalWrite::new(no_tx).execute(b"x"),
            Err(HostFunctionError::ExecutionError(
                "Terminal not available".into()
            ))
        );

        // Canal cheio
        let write = TerminalWrite::new(ctx);
        write.execute(b"1").unwrap();
        assert_eq!(
            write.execute(b"2"),
            Err(HostFunctionError::ExecutionError(
                "Failed to send write command".into()
            ))
        );
    }

    // ---------------- TerminalRead ----------------

    #[test]
    fn test_terminal_read_drains_buffer() {
        let (ctx, _) = ctx_with(&["terminal:read"], None);
        let buffer = Arc::new(RwLock::new(b"abcdef".to_vec()));
        let read = TerminalRead::new(ctx, buffer.clone());

        let out = read.execute(&4i32.to_le_bytes()).unwrap();
        assert_eq!(i32_of(&out), 4);
        assert_eq!(&out[4..], b"abcd");
        assert_eq!(*buffer.blocking_read(), b"ef");

        let out = read.execute(&100i32.to_le_bytes()).unwrap();
        assert_eq!(i32_of(&out), 2);
        assert_eq!(&out[4..], b"ef");

        // Buffer vazio: -1
        let out = read.execute(&100i32.to_le_bytes()).unwrap();
        assert_eq!(out, (-1i32).to_le_bytes().to_vec());
    }

    #[test]
    fn test_terminal_read_errors() {
        let (ctx, _) = ctx_with(&["terminal:read"], None);
        let read = TerminalRead::new(ctx, Arc::new(RwLock::new(vec![1])));
        assert_eq!(read.execute(&[1, 2]), Err(HostFunctionError::InvalidParams));

        let (no_perm, _) = ctx_with(&[], None);
        let read = TerminalRead::new(no_perm, Arc::new(RwLock::new(vec![1])));
        assert_eq!(
            read.execute(&1i32.to_le_bytes()),
            Err(HostFunctionError::PermissionDenied)
        );
    }

    // ---------------- TerminalClear ----------------

    #[test]
    fn test_terminal_clear_sends_mode() {
        let (ctx, rx) = ctx_with(&["terminal:clear"], Some(4));
        let mut rx = rx.unwrap();
        let clear = TerminalClear::new(ctx);
        for (raw, mode) in [
            (0i32, ClearMode::Screen),
            (1, ClearMode::Scrollback),
            (2, ClearMode::All),
        ] {
            let out = clear.execute(&raw.to_le_bytes()).unwrap();
            assert_eq!(i32_of(&out), 0);
            match rx.try_recv().unwrap() {
                TerminalCommand::Clear(m) => assert_eq!(m, mode),
                other => panic!("comando inesperado: {other:?}"),
            }
        }
    }

    #[test]
    fn test_terminal_clear_errors() {
        let (ctx, _rx) = ctx_with(&["terminal:clear"], Some(1));
        let clear = TerminalClear::new(ctx);
        assert_eq!(clear.execute(&[0]), Err(HostFunctionError::InvalidParams));
        assert_eq!(
            clear.execute(&7i32.to_le_bytes()),
            Err(HostFunctionError::InvalidParams)
        );
        clear.execute(&0i32.to_le_bytes()).unwrap();
        assert_eq!(
            clear.execute(&0i32.to_le_bytes()),
            Err(HostFunctionError::ExecutionError(
                "Failed to send clear command".into()
            ))
        );

        let (no_perm, _) = ctx_with(&[], Some(1));
        assert_eq!(
            TerminalClear::new(no_perm).execute(&0i32.to_le_bytes()),
            Err(HostFunctionError::PermissionDenied)
        );
        let (no_tx, _) = ctx_with(&["terminal:clear"], None);
        assert_eq!(
            TerminalClear::new(no_tx).execute(&0i32.to_le_bytes()),
            Err(HostFunctionError::ExecutionError(
                "Terminal not available".into()
            ))
        );
    }

    // ---------------- UiShowNotification ----------------

    #[test]
    fn test_show_notification_sends_command() {
        let (ctx, rx) = ctx_with(&["ui:notification"], Some(2));
        let mut rx = rx.unwrap();
        let out = UiShowNotification::new(ctx)
            .execute(&notification_params(2, b"Titulo", b"Corpo"))
            .unwrap();
        assert_eq!(i32_of(&out), 0);
        match rx.try_recv().unwrap() {
            TerminalCommand::ShowNotification {
                level,
                title,
                message,
            } => {
                assert_eq!(level, 2);
                assert_eq!(title, "Titulo");
                assert_eq!(message, "Corpo");
            }
            other => panic!("comando inesperado: {other:?}"),
        }
    }

    #[test]
    fn test_show_notification_rejects_malformed_params() {
        let (ctx, _rx) = ctx_with(&["ui:notification"], Some(1));
        let f = UiShowNotification::new(ctx);

        // Curto demais
        assert_eq!(f.execute(&[0; 11]), Err(HostFunctionError::InvalidParams));

        // Título declarado maior que o payload
        let mut p = 0i32.to_le_bytes().to_vec();
        p.extend(100i32.to_le_bytes());
        p.extend([0; 4]);
        assert_eq!(f.execute(&p), Err(HostFunctionError::InvalidParams));

        // Mensagem declarada maior que o payload
        let mut p = notification_params(0, b"t", b"");
        let len = p.len();
        p[len - 4..].copy_from_slice(&50i32.to_le_bytes());
        assert_eq!(f.execute(&p), Err(HostFunctionError::InvalidParams));

        // UTF-8 inválido no título e na mensagem
        assert_eq!(
            f.execute(&notification_params(0, &[0xff], b"m")),
            Err(HostFunctionError::InvalidParams)
        );
        assert_eq!(
            f.execute(&notification_params(0, b"t", &[0xff])),
            Err(HostFunctionError::InvalidParams)
        );
    }

    /// Regressão: tamanho negativo (vira usize enorme) estourava a soma de
    /// offsets e derrubava o host com pânico.
    #[test]
    fn test_show_notification_negative_lengths_do_not_panic() {
        let (ctx, _rx) = ctx_with(&["ui:notification"], Some(1));
        let f = UiShowNotification::new(ctx);

        let mut p = 0i32.to_le_bytes().to_vec();
        p.extend((-1i32).to_le_bytes());
        p.extend([0; 8]);
        assert_eq!(f.execute(&p), Err(HostFunctionError::InvalidParams));

        let mut p = 0i32.to_le_bytes().to_vec();
        p.extend((-9i32).to_le_bytes());
        p.extend([0; 8]);
        assert_eq!(f.execute(&p), Err(HostFunctionError::InvalidParams));

        let mut p = notification_params(0, b"t", b"");
        let len = p.len();
        p[len - 4..].copy_from_slice(&(-1i32).to_le_bytes());
        assert_eq!(f.execute(&p), Err(HostFunctionError::InvalidParams));
    }

    #[test]
    fn test_show_notification_permission_and_channel_errors() {
        let params = notification_params(0, b"t", b"m");
        let (no_perm, _) = ctx_with(&[], Some(1));
        assert_eq!(
            UiShowNotification::new(no_perm).execute(&params),
            Err(HostFunctionError::PermissionDenied)
        );

        let (no_tx, _) = ctx_with(&["ui:notification"], None);
        assert_eq!(
            UiShowNotification::new(no_tx).execute(&params),
            Err(HostFunctionError::ExecutionError("UI not available".into()))
        );

        let (ctx, _rx) = ctx_with(&["ui:notification"], Some(1));
        let f = UiShowNotification::new(ctx);
        f.execute(&params).unwrap();
        assert_eq!(
            f.execute(&params),
            Err(HostFunctionError::ExecutionError(
                "Failed to send notification".into()
            ))
        );
    }

    // ---------------- SystemExec ----------------

    #[test]
    fn test_system_exec_submits_allowed_command() {
        let (ctx, rx) = ctx_with(&["system:exec"], Some(2));
        let mut rx = rx.unwrap();
        let out = SystemExec::new(ctx)
            .execute(&exec_params(b"/bin/echo", br#"["oi","mundo"]"#))
            .unwrap();
        let out = json(out);
        assert_eq!(out["status"], "submitted");
        assert_eq!(out["exit_code"], 0);
        match rx.try_recv().unwrap() {
            TerminalCommand::Execute { command, args } => {
                assert_eq!(command, "/bin/echo");
                assert_eq!(args, vec!["oi", "mundo"]);
            }
            other => panic!("comando inesperado: {other:?}"),
        }
    }

    #[test]
    fn test_system_exec_blocks_command_outside_whitelist() {
        let (ctx, rx) = ctx_with(&["system:exec"], Some(2));
        let mut rx = rx.unwrap();
        let out = json(
            SystemExec::new(ctx)
                .execute(&exec_params(b"rm -rf /", b"[]"))
                .unwrap(),
        );
        assert_eq!(out["exit_code"], 126);
        assert!(out["stderr"].as_str().unwrap().contains("not allowed"));
        assert!(rx.try_recv().is_err(), "nada deveria ser enviado");
    }

    #[test]
    fn test_system_exec_custom_sandbox() {
        let (ctx, _rx) = ctx_with(&["system:exec"], Some(2));
        let sandbox = SandboxConfig {
            allowed_commands: vec!["git".into()],
            ..SandboxConfig::default()
        };
        let exec = SystemExec::new(ctx).with_sandbox(sandbox);
        let out = json(exec.execute(&exec_params(b"git", b"[]")).unwrap());
        assert_eq!(out["status"], "submitted");
        let out = json(exec.execute(&exec_params(b"echo", b"[]")).unwrap());
        assert_eq!(out["exit_code"], 126);
    }

    #[test]
    fn test_system_exec_channel_full_and_missing_terminal() {
        let (ctx, _rx) = ctx_with(&["system:exec"], Some(1));
        let exec = SystemExec::new(ctx);
        exec.execute(&exec_params(b"ls", b"[]")).unwrap();
        let out = json(exec.execute(&exec_params(b"ls", b"[]")).unwrap());
        assert_eq!(out["exit_code"], -1);
        assert_eq!(out["stderr"], "Failed to submit command");

        let (no_tx, _) = ctx_with(&["system:exec"], None);
        let out = json(
            SystemExec::new(no_tx)
                .execute(&exec_params(b"ls", b"[]"))
                .unwrap(),
        );
        assert_eq!(out["stderr"], "Terminal not available");
    }

    #[test]
    fn test_system_exec_rejects_malformed_params() {
        let (ctx, _rx) = ctx_with(&["system:exec"], Some(1));
        let exec = SystemExec::new(ctx);
        let invalid = Err(HostFunctionError::InvalidParams);

        assert_eq!(exec.execute(&[0; 7]), invalid);
        // cmd_len maior que o payload
        let mut p = 50i32.to_le_bytes().to_vec();
        p.extend([0; 4]);
        assert_eq!(exec.execute(&p), invalid);
        // args_len maior que o payload
        let mut p = lp(b"ls");
        p.extend(50i32.to_le_bytes());
        assert_eq!(exec.execute(&p), invalid);
        // UTF-8 inválido
        assert_eq!(exec.execute(&exec_params(&[0xff], b"[]")), invalid);
        assert_eq!(exec.execute(&exec_params(b"ls", &[0xff])), invalid);
        // args não é JSON de lista de strings
        assert_eq!(exec.execute(&exec_params(b"ls", b"{}")), invalid);
        // Regressão: tamanhos negativos não podem causar pânico
        let mut p = (-1i32).to_le_bytes().to_vec();
        p.extend([0; 8]);
        assert_eq!(exec.execute(&p), invalid);
        let mut p = lp(b"ls");
        p.extend((-1i32).to_le_bytes());
        assert_eq!(exec.execute(&p), invalid);

        let (no_perm, _) = ctx_with(&[], Some(1));
        assert_eq!(
            SystemExec::new(no_perm).execute(&exec_params(b"ls", b"[]")),
            Err(HostFunctionError::PermissionDenied)
        );
    }
}
