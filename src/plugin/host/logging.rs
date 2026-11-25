//! Host Functions - Logging
//!
//! Funções para logging estruturado de plugins.

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};
use std::sync::atomic::{AtomicU32, Ordering};
use tracing::{debug, error, info, trace, warn};

static LOG_LEVEL: AtomicU32 = AtomicU32::new(LogLevel::Info as u32);

pub(crate) fn current_log_level() -> u32 {
    LOG_LEVEL.load(Ordering::Relaxed)
}

pub(crate) fn set_current_log_level(level: LogLevel) {
    LOG_LEVEL.store(level as u32, Ordering::Relaxed);
}

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(LogTrace),
        Box::new(LogDebug),
        Box::new(LogInfo),
        Box::new(LogWarn),
        Box::new(LogError),
        Box::new(LogWithMetadata),
        Box::new(GetLogLevel),
        Box::new(SetLogLevel),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Off = 5,
}

impl LogLevel {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Trace),
            1 => Some(Self::Debug),
            2 => Some(Self::Info),
            3 => Some(Self::Warn),
            4 => Some(Self::Error),
            5 => Some(Self::Off),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Off => "OFF",
        }
    }

    fn from_params(params: &[u8]) -> Result<Self, HostFunctionError> {
        let bytes: [u8; 4] = params
            .get(..4)
            .ok_or(HostFunctionError::InvalidParams)?
            .try_into()
            .map_err(|_| HostFunctionError::InvalidParams)?;

        Self::from_u32(u32::from_le_bytes(bytes)).ok_or(HostFunctionError::InvalidParams)
    }
}

fn message_from_params(params: &[u8]) -> Result<&str, HostFunctionError> {
    std::str::from_utf8(params).map_err(|_| HostFunctionError::InvalidParams)
}

fn execute_log(level: LogLevel, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
    let message = message_from_params(params)?;

    match level {
        LogLevel::Trace => trace!(target: "plugin", "{}", message),
        LogLevel::Debug => debug!(target: "plugin", "{}", message),
        LogLevel::Info => info!(target: "plugin", "{}", message),
        LogLevel::Warn => warn!(target: "plugin", "{}", message),
        LogLevel::Error => error!(target: "plugin", "{}", message),
        LogLevel::Off => {}
    }

    Ok((params.len() as i32).to_le_bytes().to_vec())
}

pub struct LogTrace;

impl HostFunction for LogTrace {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Logging,
            name: "log_trace",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Log nível trace (mensagens de debug detalhadas)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        execute_log(LogLevel::Trace, params)
    }
}

pub struct LogDebug;

impl HostFunction for LogDebug {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Logging,
            name: "log_debug",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Log nível debug (informações de debug)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        execute_log(LogLevel::Debug, params)
    }
}

pub struct LogInfo;

impl HostFunction for LogInfo {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Logging,
            name: "log_info",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Log nível info (informações gerais)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        execute_log(LogLevel::Info, params)
    }
}

pub struct LogWarn;

impl HostFunction for LogWarn {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Logging,
            name: "log_warn",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Log nível warn (avisos)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        execute_log(LogLevel::Warn, params)
    }
}

pub struct LogError;

impl HostFunction for LogError {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Logging,
            name: "log_error",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Log nível error (erros)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        execute_log(LogLevel::Error, params)
    }
}

pub struct LogWithMetadata;

impl HostFunction for LogWithMetadata {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Logging,
            name: "log_with_metadata",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::I32,
                HostParamType::PointerWithLength,
                HostParamType::PointerWithLength,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Log com metadados (level, message, metadata_json)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let level = LogLevel::from_params(params)?;
        let payload = message_from_params(&params[4..])?;

        match level {
            LogLevel::Trace => trace!(target: "plugin", metadata = "", "{}", payload),
            LogLevel::Debug => debug!(target: "plugin", metadata = "", "{}", payload),
            LogLevel::Info => info!(target: "plugin", metadata = "", "{}", payload),
            LogLevel::Warn => warn!(target: "plugin", metadata = "", "{}", payload),
            LogLevel::Error => error!(target: "plugin", metadata = "", "{}", payload),
            LogLevel::Off => {}
        }

        Ok(((params.len() - 4) as i32).to_le_bytes().to_vec())
    }
}

pub struct GetLogLevel;

impl HostFunction for GetLogLevel {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Logging,
            name: "get_log_level",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna nível de log atual do plugin"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        Ok(current_log_level().to_le_bytes().to_vec())
    }
}

pub struct SetLogLevel;

impl HostFunction for SetLogLevel {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Logging,
            name: "set_log_level",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Define nível de log do plugin"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let level = LogLevel::from_params(params)?;
        set_current_log_level(level);
        Ok(0i32.to_le_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert_eq!(functions.len(), 8);
    }

    #[test]
    fn test_log_level_from_u32() {
        assert_eq!(LogLevel::from_u32(0), Some(LogLevel::Trace));
        assert_eq!(LogLevel::from_u32(2), Some(LogLevel::Info));
        assert_eq!(LogLevel::from_u32(100), None);
    }

    #[test]
    fn test_log_level_names() {
        assert_eq!(LogLevel::Trace.name(), "TRACE");
        assert_eq!(LogLevel::Error.name(), "ERROR");
    }

    #[test]
    fn test_log_functions_are_safe() {
        assert_eq!(LogInfo.permission(), HostFunctionPermission::Safe);
        assert_eq!(LogError.permission(), HostFunctionPermission::Safe);
    }

    #[test]
    fn test_log_execute_returns_bytes_written() {
        let result = LogInfo.execute(b"hello plugin").unwrap();
        assert_eq!(i32::from_le_bytes(result.try_into().unwrap()), 12);
    }

    #[test]
    fn test_log_execute_rejects_invalid_utf8() {
        assert!(LogWarn.execute(&[0xff]).is_err());
    }

    #[test]
    fn test_get_and_set_log_level() {
        let set_result = SetLogLevel
            .execute(&(LogLevel::Error as u32).to_le_bytes())
            .unwrap();
        assert_eq!(i32::from_le_bytes(set_result.try_into().unwrap()), 0);

        let level = GetLogLevel.execute(&[]).unwrap();
        assert_eq!(
            u32::from_le_bytes(level.try_into().unwrap()),
            LogLevel::Error as u32
        );
    }

    #[test]
    fn test_log_with_metadata_uses_level_prefix() {
        let mut params = (LogLevel::Debug as u32).to_le_bytes().to_vec();
        params.extend_from_slice(br#"{"message":"debug","metadata":{"plugin":"test"}}"#);

        let result = LogWithMetadata.execute(&params).unwrap();
        assert_eq!(
            i32::from_le_bytes(result.try_into().unwrap()),
            (params.len() - 4) as i32
        );
    }
}
