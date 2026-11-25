//! Host Functions - System
//!
//! Funções para informações do sistema e ambiente.

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};
use std::path::Path;
use std::sync::LazyLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

static HOST_START: LazyLock<Instant> = LazyLock::new(Instant::now);

fn i32_bytes(value: i32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn i64_bytes(value: i64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn f32_bytes(value: f32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn utf8_param(params: &[u8]) -> Result<&str, HostFunctionError> {
    std::str::from_utf8(params).map_err(|_| HostFunctionError::InvalidParams)
}

fn path_param(params: &[u8]) -> Result<&Path, HostFunctionError> {
    Ok(Path::new(utf8_param(params)?))
}

fn path_string(path: Option<std::path::PathBuf>) -> Result<Vec<u8>, HostFunctionError> {
    path.ok_or_else(|| HostFunctionError::ExecutionError("Directory not available".to_string()))
        .map(|p| p.to_string_lossy().into_owned().into_bytes())
}

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(GetHostVersion),
        Box::new(GetApiVersion),
        Box::new(GetPlatform),
        Box::new(GetEnvVar),
        Box::new(GetCurrentTime),
        Box::new(GetUptime),
        Box::new(GetMemoryUsage),
        Box::new(GetCpuUsage),
        Box::new(GetHostname),
        Box::new(GetHomeDir),
        Box::new(GetConfigDir),
        Box::new(GetDataDir),
        Box::new(GetTempDir),
        Box::new(ReadFile),
        Box::new(WriteFile),
        Box::new(FileExists),
        Box::new(ListDir),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Platform {
    Windows = 0,
    Macos = 1,
    Linux = 2,
    Bsd = 3,
    Other = 99,
}

impl Platform {
    pub fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Self::Macos
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(all(
            not(target_os = "windows"),
            not(target_os = "macos"),
            not(target_os = "linux")
        ))]
        {
            Self::Other
        }
    }

    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Windows),
            1 => Some(Self::Macos),
            2 => Some(Self::Linux),
            3 => Some(Self::Bsd),
            99 => Some(Self::Other),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Bsd => "bsd",
            Self::Other => "other",
        }
    }
}

pub struct GetHostVersion;

impl HostFunction for GetHostVersion {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_host_version",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém versão do host como string (out_ptr, out_len)"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(env!("CARGO_PKG_VERSION").as_bytes().to_vec())
    }
}

pub struct GetApiVersion;

impl HostFunction for GetApiVersion {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_api_version",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém versão da API (major, minor, patch) via ponteiro"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok([1u32.to_le_bytes(), 0u32.to_le_bytes(), 0u32.to_le_bytes()].concat())
    }
}

pub struct GetPlatform;

impl HostFunction for GetPlatform {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_platform",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna plataforma atual (0=Windows, 1=macOS, 2=Linux)"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(i32_bytes(Platform::current() as i32))
    }
}

pub struct GetEnvVar;

impl HostFunction for GetEnvVar {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_env_var",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength,
                HostParamType::Pointer,
                HostParamType::I32,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Obtém variável de ambiente (name, out_ptr, out_len)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let key = utf8_param(params)?;
        std::env::var(key)
            .map(|value| value.into_bytes())
            .map_err(|_| HostFunctionError::NotFound)
    }
}

pub struct GetCurrentTime;

impl HostFunction for GetCurrentTime {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_current_time",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I64),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna timestamp atual em milissegundos desde epoch"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| HostFunctionError::ExecutionError(e.to_string()))?
            .as_millis() as i64;
        Ok(i64_bytes(millis))
    }
}

pub struct GetUptime;

impl HostFunction for GetUptime {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_uptime",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I64),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna uptime do terminal em milissegundos"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(i64_bytes(HOST_START.elapsed().as_millis() as i64))
    }
}

pub struct GetMemoryUsage;

impl HostFunction for GetMemoryUsage {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_memory_usage",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém uso de memória (used_kb, total_kb) via ponteiro"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok([0i64.to_le_bytes(), 0i64.to_le_bytes()].concat())
    }
}

pub struct GetCpuUsage;

impl HostFunction for GetCpuUsage {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_cpu_usage",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::F32),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna uso de CPU como porcentagem (0.0-100.0)"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(f32_bytes(0.0))
    }
}

pub struct GetHostname;

impl HostFunction for GetHostname {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_hostname",
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
        "Obtém hostname do sistema"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        hostname::get()
            .map(|host| host.to_string_lossy().into_owned().into_bytes())
            .map_err(|e| HostFunctionError::ExecutionError(e.to_string()))
    }
}

pub struct GetHomeDir;

impl HostFunction for GetHomeDir {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_home_dir",
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
        "Obtém diretório home do usuário"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        path_string(dirs::home_dir())
    }
}

pub struct GetConfigDir;

impl HostFunction for GetConfigDir {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_config_dir",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém diretório de configuração do codeterm"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        path_string(dirs::config_dir().map(|path| path.join("codeterm")))
    }
}

pub struct GetDataDir;

impl HostFunction for GetDataDir {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_data_dir",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém diretório de dados do codeterm"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        path_string(dirs::data_dir().map(|path| path.join("codeterm")))
    }
}

pub struct GetTempDir;

impl HostFunction for GetTempDir {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "get_temp_dir",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém diretório temporário do sistema"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(std::env::temp_dir()
            .to_string_lossy()
            .into_owned()
            .into_bytes())
    }
}

pub struct ReadFile;

impl HostFunction for ReadFile {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "read_file",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength,
                HostParamType::Pointer,
                HostParamType::I32,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Lê arquivo do sistema (path, out_ptr, out_len)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        std::fs::read(path_param(params)?)
            .map_err(|e| HostFunctionError::ExecutionError(e.to_string()))
    }
}

pub struct WriteFile;

impl HostFunction for WriteFile {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "write_file",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength,
                HostParamType::PointerWithLength,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Escreve arquivo no sistema (path, data)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let separator = params
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(HostFunctionError::InvalidParams)?;
        let path = path_param(&params[..separator])?;
        let data = &params[separator + 1..];

        std::fs::write(path, data).map_err(|e| HostFunctionError::ExecutionError(e.to_string()))?;
        Ok(i32_bytes(data.len() as i32))
    }
}

pub struct FileExists;

impl HostFunction for FileExists {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "file_exists",
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
        "Verifica se arquivo existe (path) -> 1=sim, 0=não"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(i32_bytes(if path_param(params)?.exists() { 1 } else { 0 }))
    }
}

pub struct ListDir;

impl HostFunction for ListDir {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::System,
            name: "list_dir",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength,
                HostParamType::Pointer,
                HostParamType::I32,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Lista diretório (path, out_ptr, out_len) -> JSON array"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let mut entries = Vec::new();

        for entry in std::fs::read_dir(path_param(params)?)
            .map_err(|e| HostFunctionError::ExecutionError(e.to_string()))?
        {
            let entry = entry.map_err(|e| HostFunctionError::ExecutionError(e.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|e| HostFunctionError::ExecutionError(e.to_string()))?;
            entries.push(serde_json::json!({
                "name": entry.file_name().to_string_lossy(),
                "path": entry.path().to_string_lossy(),
                "is_dir": file_type.is_dir(),
                "is_file": file_type.is_file(),
            }));
        }

        serde_json::to_vec(&entries).map_err(|e| HostFunctionError::ExecutionError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert_eq!(functions.len(), 17);
    }

    #[test]
    fn test_platform_from_u32() {
        assert_eq!(Platform::from_u32(0), Some(Platform::Windows));
        assert_eq!(Platform::from_u32(1), Some(Platform::Macos));
        assert_eq!(Platform::from_u32(2), Some(Platform::Linux));
        assert_eq!(Platform::from_u32(50), None);
    }

    #[test]
    fn test_platform_names() {
        assert_eq!(Platform::Windows.name(), "windows");
        assert_eq!(Platform::Macos.name(), "macos");
        assert_eq!(Platform::Linux.name(), "linux");
    }

    #[test]
    fn test_dangerous_functions() {
        assert_eq!(ReadFile.permission(), HostFunctionPermission::Dangerous);
        assert_eq!(WriteFile.permission(), HostFunctionPermission::Dangerous);
    }

    #[test]
    fn test_safe_functions() {
        assert_eq!(GetHostVersion.permission(), HostFunctionPermission::Safe);
        assert_eq!(GetPlatform.permission(), HostFunctionPermission::Safe);
        assert_eq!(GetCurrentTime.permission(), HostFunctionPermission::Safe);
    }

    #[test]
    fn test_basic_system_execute_functions() {
        assert!(!GetHostVersion.execute(&[]).unwrap().is_empty());

        let platform = GetPlatform.execute(&[]).unwrap();
        let platform = i32::from_le_bytes(platform.try_into().unwrap());
        assert!(Platform::from_u32(platform as u32).is_some());

        let now = GetCurrentTime.execute(&[]).unwrap();
        assert!(i64::from_le_bytes(now.try_into().unwrap()) > 0);

        let uptime = GetUptime.execute(&[]).unwrap();
        assert!(i64::from_le_bytes(uptime.try_into().unwrap()) >= 0);
    }

    #[test]
    fn test_directory_execute_functions() {
        assert!(!GetTempDir.execute(&[]).unwrap().is_empty());
        assert!(!GetHomeDir.execute(&[]).unwrap().is_empty());
    }

    #[test]
    fn test_env_execute_function() {
        let path_value = GetEnvVar.execute(b"PATH").unwrap();
        assert!(!path_value.is_empty());
        assert!(GetEnvVar
            .execute(b"CODETERM_TEST_ENV_DOES_NOT_EXIST")
            .is_err());
    }

    #[test]
    fn test_file_execute_functions() {
        let path = std::env::temp_dir().join(format!(
            "codeterm-host-system-test-{}.txt",
            std::process::id()
        ));
        let path_bytes = path.to_string_lossy().into_owned().into_bytes();

        let mut write_params = path_bytes.clone();
        write_params.push(0);
        write_params.extend_from_slice(b"hello");

        let written = WriteFile.execute(&write_params).unwrap();
        assert_eq!(i32::from_le_bytes(written.try_into().unwrap()), 5);

        let exists = FileExists.execute(&path_bytes).unwrap();
        assert_eq!(i32::from_le_bytes(exists.try_into().unwrap()), 1);

        assert_eq!(ReadFile.execute(&path_bytes).unwrap(), b"hello");

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_list_dir_execute_function() {
        let temp_dir = std::env::temp_dir();
        let output = ListDir
            .execute(temp_dir.to_string_lossy().as_bytes())
            .unwrap();
        let entries: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert!(entries.is_array());
    }

    #[test]
    fn test_platform_extra_variants() {
        assert_eq!(Platform::from_u32(3), Some(Platform::Bsd));
        assert_eq!(Platform::from_u32(99), Some(Platform::Other));
        assert_eq!(Platform::Bsd.name(), "bsd");
        assert_eq!(Platform::Other.name(), "other");
        #[cfg(target_os = "linux")]
        assert_eq!(Platform::current(), Platform::Linux);
    }

    #[test]
    fn test_fixed_value_functions() {
        let version = GetHostVersion.execute(&[]).unwrap();
        assert_eq!(version, env!("CARGO_PKG_VERSION").as_bytes());

        let api = GetApiVersion.execute(&[]).unwrap();
        assert_eq!(api.len(), 12);
        assert_eq!(u32::from_le_bytes(api[..4].try_into().unwrap()), 1);

        assert_eq!(GetMemoryUsage.execute(&[]).unwrap(), vec![0u8; 16]);
        let cpu = GetCpuUsage.execute(&[]).unwrap();
        assert_eq!(f32::from_le_bytes(cpu.try_into().unwrap()), 0.0);

        let hostname = GetHostname.execute(&[]).unwrap();
        assert_eq!(
            String::from_utf8(hostname).unwrap(),
            hostname::get().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn test_config_and_data_dirs_end_with_codeterm() {
        for f in [&GetConfigDir as &dyn HostFunction, &GetDataDir] {
            let dir = String::from_utf8(f.execute(&[]).unwrap()).unwrap();
            assert!(dir.ends_with("codeterm"), "{dir}");
        }
        assert!(path_string(None).is_err());
    }

    #[test]
    fn test_invalid_inputs() {
        let invalid_utf8 = [0xff, 0xfe];
        assert_eq!(
            GetEnvVar.execute(&invalid_utf8),
            Err(HostFunctionError::InvalidParams)
        );
        assert_eq!(
            GetEnvVar.execute(b"CODETERM_TEST_ENV_DOES_NOT_EXIST"),
            Err(HostFunctionError::NotFound)
        );
        assert_eq!(
            ReadFile.execute(&invalid_utf8),
            Err(HostFunctionError::InvalidParams)
        );
        assert!(matches!(
            ReadFile.execute(b"/caminho/que/nao/existe"),
            Err(HostFunctionError::ExecutionError(_))
        ));
        assert_eq!(
            FileExists.execute(&invalid_utf8),
            Err(HostFunctionError::InvalidParams)
        );
        assert_eq!(
            i32::from_le_bytes(
                FileExists
                    .execute(b"/caminho/que/nao/existe")
                    .unwrap()
                    .try_into()
                    .unwrap()
            ),
            0
        );
        assert!(matches!(
            ListDir.execute(b"/caminho/que/nao/existe"),
            Err(HostFunctionError::ExecutionError(_))
        ));

        // write_file exige separador NUL entre caminho e dados
        assert_eq!(
            WriteFile.execute(b"sem-separador"),
            Err(HostFunctionError::InvalidParams)
        );
        let mut params = b"/caminho/que/nao/existe/x.txt".to_vec();
        params.push(0);
        params.extend_from_slice(b"dados");
        assert!(matches!(
            WriteFile.execute(&params),
            Err(HostFunctionError::ExecutionError(_))
        ));
    }

    #[test]
    fn test_list_dir_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("d")).unwrap();
        let output = ListDir
            .execute(dir.path().to_string_lossy().as_bytes())
            .unwrap();
        let mut entries: Vec<serde_json::Value> = serde_json::from_slice(&output).unwrap();
        entries.sort_by_key(|e| e["name"].as_str().unwrap().to_string());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["name"], "d");
        assert_eq!(entries[0]["is_dir"], true);
        assert_eq!(entries[1]["name"], "f.txt");
        assert_eq!(entries[1]["is_file"], true);
    }
}
