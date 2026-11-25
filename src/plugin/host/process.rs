//! Host Functions - Process
//!
//! Funções para spawn e controle de processos.

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(ProcessSpawn),
        Box::new(ProcessKill),
        Box::new(ProcessWait),
        Box::new(ProcessGetStatus),
        Box::new(ProcessGetOutput),
        Box::new(ProcessList),
    ]
}

/// Gerenciador global de processos
static PROCESS_MANAGER: LazyLock<Mutex<ProcessManager>> =
    LazyLock::new(|| Mutex::new(ProcessManager::new()));

pub struct ProcessManager {
    next_id: u32,
    processes: HashMap<u32, ProcessHandle>,
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            processes: HashMap::new(),
        }
    }

    pub fn spawn(&mut self, command: &str, args: &[String]) -> Result<u32, String> {
        // Sandbox: valida comando
        if !is_command_allowed(command) {
            return Err(format!("Command '{}' is not in whitelist", command));
        }

        let id = self.next_id;
        self.next_id += 1;

        let handle = ProcessHandle {
            id,
            command: command.to_string(),
            args: args.to_vec(),
            status: ProcessStatus::Running,
            exit_code: None,
            stdout: None,
            stderr: None,
        };

        // Nota: em implementação real, usaríamos tokio::process::Command
        // para processos assíncronos. Aqui apenas simulamos.

        self.processes.insert(id, handle);
        Ok(id)
    }

    pub fn kill(&mut self, id: u32) -> Result<(), String> {
        if let Some(handle) = self.processes.get_mut(&id) {
            handle.status = ProcessStatus::Killed;
            handle.exit_code = Some(137); // SIGKILL
            Ok(())
        } else {
            Err(format!("Process {} not found", id))
        }
    }

    pub fn wait(&mut self, id: u32) -> Result<Option<i32>, String> {
        if let Some(handle) = self.processes.get_mut(&id) {
            if handle.status == ProcessStatus::Running {
                // Simula término
                handle.status = ProcessStatus::Completed;
                handle.exit_code = Some(0);
            }
            Ok(handle.exit_code)
        } else {
            Err(format!("Process {} not found", id))
        }
    }

    pub fn get_status(&self, id: u32) -> Result<ProcessStatus, String> {
        self.processes
            .get(&id)
            .map(|h| h.status)
            .ok_or_else(|| format!("Process {} not found", id))
    }

    pub fn get_output(&self, id: u32) -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
        self.processes
            .get(&id)
            .map(|h| {
                h.stdout
                    .as_ref()
                    .map(|s| (s.clone(), h.stderr.clone().unwrap_or_default()))
            })
            .ok_or_else(|| format!("Process {} not found", id))
    }

    pub fn list(&self) -> Vec<&ProcessHandle> {
        self.processes.values().collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Running,
    Completed,
    Killed,
    Error,
}

pub struct ProcessHandle {
    pub id: u32,
    pub command: String,
    pub args: Vec<String>,
    pub status: ProcessStatus,
    pub exit_code: Option<i32>,
    pub stdout: Option<Vec<u8>>,
    pub stderr: Option<Vec<u8>>,
}

/// Verifica se comando está na whitelist
fn is_command_allowed(command: &str) -> bool {
    let allowed = [
        "echo", "cat", "ls", "pwd", "grep", "head", "tail", "wc", "sort", "uniq", "date", "whoami",
        "which", "dirname", "basename", "find", "sed", "awk", "tr", "cut", "rev", "tee", "xargs",
    ];

    let cmd_name = std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(command);

    allowed.contains(&cmd_name)
}

pub struct ProcessSpawn;

impl HostFunction for ProcessSpawn {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "process_spawn",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength, // command
                HostParamType::PointerWithLength, // args JSON
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Executa comando (command, args_json) -> process_id ou -1 em erro"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        // Parse: [cmd_len: i32] [cmd] [args_len: i32] [args_json]
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }

        let cmd_len = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as usize;
        // Tamanho negativo vira usize enorme: a soma precisa ser verificada
        let cmd_end = 4usize
            .checked_add(cmd_len)
            .ok_or(HostFunctionError::InvalidParams)?;

        if params.len() < cmd_end.saturating_add(4) {
            return Err(HostFunctionError::InvalidParams);
        }

        let command = String::from_utf8(params[4..cmd_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let args_len = i32::from_le_bytes([
            params[cmd_end],
            params[cmd_end + 1],
            params[cmd_end + 2],
            params[cmd_end + 3],
        ]) as usize;
        let args_end = (cmd_end + 4)
            .checked_add(args_len)
            .ok_or(HostFunctionError::InvalidParams)?;

        if params.len() < args_end {
            return Err(HostFunctionError::InvalidParams);
        }

        let args_json = String::from_utf8(params[cmd_end + 4..args_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let args: Vec<String> =
            serde_json::from_str(&args_json).map_err(|_| HostFunctionError::InvalidParams)?;

        let mut manager = PROCESS_MANAGER.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock process manager".to_string())
        })?;

        match manager.spawn(&command, &args) {
            Ok(id) => Ok((id as i32).to_le_bytes().to_vec()),
            Err(e) => {
                let error_json = serde_json::json!({
                    "error": e,
                    "process_id": -1,
                });
                Ok(error_json.to_string().into_bytes())
            }
        }
    }
}

pub struct ProcessKill;

impl HostFunction for ProcessKill {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "process_kill",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32], // process_id
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Mata processo (process_id) -> 0=sucesso, -1=erro"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let id = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as u32;

        let mut manager = PROCESS_MANAGER.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock process manager".to_string())
        })?;

        match manager.kill(id) {
            Ok(_) => Ok(0i32.to_le_bytes().to_vec()),
            Err(_) => Ok((-1i32).to_le_bytes().to_vec()),
        }
    }
}

pub struct ProcessWait;

impl HostFunction for ProcessWait {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "process_wait",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32], // process_id
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Espera término do processo (process_id) -> exit_code ou -1"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let id = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as u32;

        let mut manager = PROCESS_MANAGER.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock process manager".to_string())
        })?;

        match manager.wait(id) {
            Ok(exit_code) => Ok({ exit_code.unwrap_or(-1) }.to_le_bytes().to_vec()),
            Err(_) => Ok((-1i32).to_le_bytes().to_vec()),
        }
    }
}

pub struct ProcessGetStatus;

impl HostFunction for ProcessGetStatus {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "process_get_status",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32], // process_id
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Obtém status do processo (process_id) -> 0=running, 1=completed, 2=killed, 3=error, -1=not found"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let id = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as u32;

        let manager = PROCESS_MANAGER.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock process manager".to_string())
        })?;

        match manager.get_status(id) {
            Ok(status) => {
                let code: i32 = match status {
                    ProcessStatus::Running => 0,
                    ProcessStatus::Completed => 1,
                    ProcessStatus::Killed => 2,
                    ProcessStatus::Error => 3,
                };
                Ok(code.to_le_bytes().to_vec())
            }
            Err(_) => Ok((-1i32).to_le_bytes().to_vec()),
        }
    }
}

pub struct ProcessGetOutput;

impl HostFunction for ProcessGetOutput {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "process_get_output",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::I32,     // process_id
                HostParamType::Pointer, // out_stdout
                HostParamType::Pointer, // out_stderr
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Obtém stdout/stderr do processo -> JSON {stdout, stderr}"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let id = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as u32;

        let manager = PROCESS_MANAGER.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock process manager".to_string())
        })?;

        match manager.get_output(id) {
            Ok(Some((stdout, stderr))) => {
                let result = serde_json::json!({
                    "stdout": String::from_utf8_lossy(&stdout),
                    "stderr": String::from_utf8_lossy(&stderr),
                });
                Ok(result.to_string().into_bytes())
            }
            Ok(None) => {
                let result = serde_json::json!({
                    "stdout": "",
                    "stderr": "",
                    "status": "no_output",
                });
                Ok(result.to_string().into_bytes())
            }
            Err(_) => Ok((-1i32).to_le_bytes().to_vec()),
        }
    }
}

pub struct ProcessList;

impl HostFunction for ProcessList {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "process_list",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Safe
    }

    fn description(&self) -> &'static str {
        "Lista todos os processos como JSON"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let manager = PROCESS_MANAGER.lock().map_err(|_| {
            HostFunctionError::ExecutionError("Failed to lock process manager".to_string())
        })?;

        let processes: Vec<_> = manager
            .list()
            .iter()
            .map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "command": &p.command,
                    "args": &p.args,
                    "status": match p.status {
                        ProcessStatus::Running => "running",
                        ProcessStatus::Completed => "completed",
                        ProcessStatus::Killed => "killed",
                        ProcessStatus::Error => "error",
                    },
                    "exit_code": p.exit_code,
                })
            })
            .collect();

        Ok(serde_json::to_string(&processes).unwrap().into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert_eq!(functions.len(), 6);
    }

    #[test]
    fn test_is_command_allowed() {
        assert!(is_command_allowed("echo"));
        assert!(is_command_allowed("ls"));
        assert!(is_command_allowed("/bin/grep"));
        assert!(!is_command_allowed("rm"));
        assert!(!is_command_allowed("sudo"));
    }

    #[test]
    fn test_process_manager_spawn() {
        let mut manager = ProcessManager::new();
        let id = manager.spawn("echo", &["hello".to_string()]).unwrap();
        assert_eq!(id, 1);

        let id2 = manager.spawn("ls", &[]).unwrap();
        assert_eq!(id2, 2);
    }

    #[test]
    fn test_process_manager_kill() {
        let mut manager = ProcessManager::new();
        let id = manager.spawn("echo", &["hello".to_string()]).unwrap();

        assert!(manager.kill(id).is_ok());
        assert_eq!(manager.get_status(id).unwrap(), ProcessStatus::Killed);
    }

    #[test]
    fn test_process_manager_not_found() {
        let manager = ProcessManager::new();
        assert!(manager.get_status(999).is_err());
    }

    #[test]
    fn test_spawn_not_allowed() {
        let mut manager = ProcessManager::new();
        assert!(manager
            .spawn("rm", &["-rf".to_string(), "/".to_string()])
            .is_err());
    }

    fn lp(bytes: &[u8]) -> Vec<u8> {
        let mut v = (bytes.len() as i32).to_le_bytes().to_vec();
        v.extend_from_slice(bytes);
        v
    }

    fn spawn_params(cmd: &[u8], args: &[u8]) -> Vec<u8> {
        let mut p = lp(cmd);
        p.extend(lp(args));
        p
    }

    fn i32_of(bytes: &[u8]) -> i32 {
        i32::from_le_bytes(bytes[..4].try_into().unwrap())
    }

    fn id_param(id: i32) -> Vec<u8> {
        id.to_le_bytes().to_vec()
    }

    fn spawn(cmd: &str, args: &str) -> i32 {
        i32_of(
            &ProcessSpawn
                .execute(&spawn_params(cmd.as_bytes(), args.as_bytes()))
                .unwrap(),
        )
    }

    #[test]
    fn test_metadata() {
        let expected = [
            ("process_spawn", HostFunctionPermission::Dangerous, 2),
            ("process_kill", HostFunctionPermission::Dangerous, 1),
            ("process_wait", HostFunctionPermission::Restricted, 1),
            ("process_get_status", HostFunctionPermission::Safe, 1),
            ("process_get_output", HostFunctionPermission::Restricted, 3),
            ("process_list", HostFunctionPermission::Safe, 2),
        ];
        for (f, (name, permission, params)) in all_functions().iter().zip(expected) {
            assert_eq!(f.id().name, name);
            assert_eq!(f.id().category, HostFunctionCategory::System);
            assert_eq!(f.permission(), permission, "{name}");
            assert_eq!(f.signature().params.len(), params, "{name}");
            assert!(!f.description().is_empty());
        }
    }

    #[test]
    fn test_spawn_wait_status_kill_lifecycle() {
        let id = spawn("/bin/echo", r#"["oi"]"#);
        assert!(id > 0);

        let status = |id: i32| i32_of(&ProcessGetStatus.execute(&id_param(id)).unwrap());
        assert_eq!(status(id), 0); // running

        // Ainda sem saída capturada
        let output: serde_json::Value =
            serde_json::from_slice(&ProcessGetOutput.execute(&id_param(id)).unwrap()).unwrap();
        assert_eq!(output["status"], "no_output");

        // wait conclui com exit 0 e é idempotente
        assert_eq!(i32_of(&ProcessWait.execute(&id_param(id)).unwrap()), 0);
        assert_eq!(i32_of(&ProcessWait.execute(&id_param(id)).unwrap()), 0);
        assert_eq!(status(id), 1); // completed

        let other = spawn("ls", "[]");
        assert_eq!(i32_of(&ProcessKill.execute(&id_param(other)).unwrap()), 0);
        assert_eq!(status(other), 2); // killed
                                      // wait de processo morto devolve o código do SIGKILL
        assert_eq!(i32_of(&ProcessWait.execute(&id_param(other)).unwrap()), 137);

        let list: Vec<serde_json::Value> =
            serde_json::from_slice(&ProcessList.execute(&[]).unwrap()).unwrap();
        let mine = list.iter().find(|p| p["id"] == id).unwrap();
        assert_eq!(mine["command"], "/bin/echo");
        assert_eq!(mine["args"], serde_json::json!(["oi"]));
        assert_eq!(mine["status"], "completed");
        let killed = list.iter().find(|p| p["id"] == other).unwrap();
        assert_eq!(killed["status"], "killed");
        assert_eq!(killed["exit_code"], 137);
    }

    #[test]
    fn test_unknown_process_returns_minus_one() {
        let missing = id_param(i32::MAX);
        for f in [
            &ProcessKill as &dyn HostFunction,
            &ProcessWait,
            &ProcessGetStatus,
            &ProcessGetOutput,
        ] {
            assert_eq!(
                f.execute(&missing).unwrap(),
                (-1i32).to_le_bytes(),
                "{}",
                f.id().name
            );
            assert_eq!(
                f.execute(&[1, 2]),
                Err(HostFunctionError::InvalidParams),
                "{}",
                f.id().name
            );
        }
    }

    #[test]
    fn test_spawn_rejects_disallowed_command_with_json_error() {
        let out = ProcessSpawn
            .execute(&spawn_params(b"rm", br#"["-rf","/"]"#))
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["process_id"], -1);
        assert!(json["error"].as_str().unwrap().contains("not in whitelist"));
    }

    #[test]
    fn test_spawn_rejects_malformed_params() {
        let invalid = Err(HostFunctionError::InvalidParams);
        assert_eq!(ProcessSpawn.execute(&[0; 7]), invalid);

        let mut p = 40i32.to_le_bytes().to_vec();
        p.extend([0; 4]);
        assert_eq!(ProcessSpawn.execute(&p), invalid);

        let mut p = lp(b"ls");
        p.extend(40i32.to_le_bytes());
        assert_eq!(ProcessSpawn.execute(&p), invalid);

        assert_eq!(ProcessSpawn.execute(&spawn_params(&[0xff], b"[]")), invalid);
        assert_eq!(ProcessSpawn.execute(&spawn_params(b"ls", &[0xff])), invalid);
        assert_eq!(
            ProcessSpawn.execute(&spawn_params(b"ls", b"\"x\"")),
            invalid
        );
    }

    /// Regressão: tamanho negativo estourava `4 + cmd_len` e derrubava o host.
    #[test]
    fn test_spawn_negative_lengths_do_not_panic() {
        let invalid = Err(HostFunctionError::InvalidParams);
        let mut p = (-1i32).to_le_bytes().to_vec();
        p.extend([0; 8]);
        assert_eq!(ProcessSpawn.execute(&p), invalid);

        let mut p = (-5i32).to_le_bytes().to_vec();
        p.extend([0; 8]);
        assert_eq!(ProcessSpawn.execute(&p), invalid);

        let mut p = lp(b"ls");
        p.extend((-1i32).to_le_bytes());
        assert_eq!(ProcessSpawn.execute(&p), invalid);
    }

    #[test]
    fn test_process_manager_direct_api() {
        let mut manager = ProcessManager::default();
        assert!(manager.list().is_empty());
        let id = manager.spawn("cat", &[]).unwrap();
        assert_eq!(manager.get_output(id).unwrap(), None);
        manager.processes.get_mut(&id).unwrap().stdout = Some(b"saida".to_vec());
        assert_eq!(
            manager.get_output(id).unwrap(),
            Some((b"saida".to_vec(), Vec::new()))
        );
        assert!(manager.get_output(99).is_err());
        assert!(manager.wait(99).is_err());
        assert!(manager.kill(99).is_err());
        assert_eq!(manager.list().len(), 1);
    }

    #[test]
    fn test_get_output_returns_captured_streams() {
        let id = spawn("echo", "[]");
        {
            let mut manager = PROCESS_MANAGER.lock().unwrap();
            let handle = manager.processes.get_mut(&(id as u32)).unwrap();
            handle.stdout = Some(b"ola\n".to_vec());
            handle.stderr = Some(b"aviso".to_vec());
        }
        let json: serde_json::Value =
            serde_json::from_slice(&ProcessGetOutput.execute(&id_param(id)).unwrap()).unwrap();
        assert_eq!(json["stdout"], "ola\n");
        assert_eq!(json["stderr"], "aviso");
    }

    #[test]
    fn test_list_reports_error_status() {
        let id = spawn("pwd", "[]");
        PROCESS_MANAGER
            .lock()
            .unwrap()
            .processes
            .get_mut(&(id as u32))
            .unwrap()
            .status = ProcessStatus::Error;
        assert_eq!(i32_of(&ProcessGetStatus.execute(&id_param(id)).unwrap()), 3);
        let list: Vec<serde_json::Value> =
            serde_json::from_slice(&ProcessList.execute(&[]).unwrap()).unwrap();
        let entry = list.iter().find(|p| p["id"] == id).unwrap();
        assert_eq!(entry["status"], "error");
    }
}
