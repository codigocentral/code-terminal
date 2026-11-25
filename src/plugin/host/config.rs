//! Host Functions - Config
//!
//! Funções para acesso à configuração do terminal e do plugin.

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

/// Configuração global do plugin
static PLUGIN_CONFIG: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// `start + len` sem estourar: tamanhos negativos vindos do plugin viram
/// `usize` enorme e antes causavam pânico na soma.
fn checked_end(start: usize, len: usize) -> Result<usize, HostFunctionError> {
    start
        .checked_add(len)
        .ok_or(HostFunctionError::InvalidParams)
}

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(GetConfig),
        Box::new(SetConfig),
        Box::new(GetTheme),
        Box::new(GetKeybindings),
        Box::new(GetPluginConfig),
        Box::new(SetPluginConfig),
        Box::new(GetFontConfig),
        Box::new(GetTerminalConfig),
        Box::new(SubscribeConfigChanges),
        Box::new(UnsubscribeConfigChanges),
    ]
}

pub struct GetConfig;

impl HostFunction for GetConfig {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "get_config",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength, // key
                HostParamType::Pointer,           // out_ptr
                HostParamType::I32,               // out_len
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém valor de configuração por chave (key, out_ptr, out_len)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }

        let key_len = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as usize;
        let key_end = checked_end(4, key_len)?;

        if params.len() < checked_end(key_end, 4)? {
            return Err(HostFunctionError::InvalidParams);
        }

        let key = String::from_utf8(params[4..key_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let max_len = i32::from_le_bytes([
            params[key_end],
            params[key_end + 1],
            params[key_end + 2],
            params[key_end + 3],
        ]) as usize;

        let config = PLUGIN_CONFIG
            .lock()
            .map_err(|_| HostFunctionError::ExecutionError("Failed to lock config".to_string()))?;

        match config.get(&key) {
            Some(value) => {
                let bytes = value.as_bytes();
                let to_return = bytes.len().min(max_len);
                Ok(bytes[..to_return].to_vec())
            }
            None => Ok((-1i32).to_le_bytes().to_vec()),
        }
    }
}

pub struct SetConfig;

impl HostFunction for SetConfig {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "set_config",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength, // key
                HostParamType::PointerWithLength, // value
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Dangerous
    }

    fn description(&self) -> &'static str {
        "Define valor de configuração (key, value) - perigoso"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }

        let key_len = i32::from_le_bytes([params[0], params[1], params[2], params[3]]) as usize;
        let key_end = checked_end(4, key_len)?;

        if params.len() < checked_end(key_end, 4)? {
            return Err(HostFunctionError::InvalidParams);
        }

        let key = String::from_utf8(params[4..key_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let val_len = i32::from_le_bytes([
            params[key_end],
            params[key_end + 1],
            params[key_end + 2],
            params[key_end + 3],
        ]) as usize;
        let val_end = checked_end(key_end + 4, val_len)?;

        if params.len() < val_end {
            return Err(HostFunctionError::InvalidParams);
        }

        let value = String::from_utf8(params[key_end + 4..val_end].to_vec())
            .map_err(|_| HostFunctionError::InvalidParams)?;

        let mut config = PLUGIN_CONFIG
            .lock()
            .map_err(|_| HostFunctionError::ExecutionError("Failed to lock config".to_string()))?;

        config.insert(key, value);
        Ok(0i32.to_le_bytes().to_vec())
    }
}

pub struct GetTheme;

impl HostFunction for GetTheme {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "get_theme",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém nome do tema atual"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        // Retorna tema padrão
        Ok(b"default".to_vec())
    }
}

pub struct GetKeybindings;

impl HostFunction for GetKeybindings {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "get_keybindings",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém keybindings como JSON"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let keybindings = serde_json::json!({
            "new_tab": "ctrl+shift+t",
            "close_tab": "ctrl+shift+w",
            "next_tab": "ctrl+tab",
            "prev_tab": "ctrl+shift+tab",
            "split_horizontal": "ctrl+shift+e",
            "split_vertical": "ctrl+shift+o",
        });
        Ok(keybindings.to_string().into_bytes())
    }
}

pub struct GetPluginConfig;

impl HostFunction for GetPluginConfig {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "get_plugin_config",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém configuração específica do plugin como JSON"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let config = PLUGIN_CONFIG
            .lock()
            .map_err(|_| HostFunctionError::ExecutionError("Failed to lock config".to_string()))?;

        let json: serde_json::Value = config
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();

        Ok(json.to_string().into_bytes())
    }
}

pub struct SetPluginConfig;

impl HostFunction for SetPluginConfig {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "set_plugin_config",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength], // JSON
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Define configuração do plugin (JSON)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let json_str =
            String::from_utf8(params.to_vec()).map_err(|_| HostFunctionError::InvalidParams)?;

        let new_config: HashMap<String, String> =
            serde_json::from_str(&json_str).map_err(|_| HostFunctionError::InvalidParams)?;

        let mut config = PLUGIN_CONFIG
            .lock()
            .map_err(|_| HostFunctionError::ExecutionError("Failed to lock config".to_string()))?;

        config.extend(new_config);
        Ok(0i32.to_le_bytes().to_vec())
    }
}

pub struct GetFontConfig;

impl HostFunction for GetFontConfig {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "get_font_config",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém configuração de fonte como JSON"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let font_config = serde_json::json!({
            "family": "JetBrains Mono",
            "size": 12.0,
            "line_height": 1.2,
            "ligatures": true,
        });
        Ok(font_config.to_string().into_bytes())
    }
}

pub struct GetTerminalConfig;

impl HostFunction for GetTerminalConfig {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "get_terminal_config",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém configuração do terminal como JSON"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let terminal_config = serde_json::json!({
            "shell": "/bin/bash",
            "scrollback_lines": 10000,
            "cursor_style": "block",
            "cursor_blink": true,
        });
        Ok(terminal_config.to_string().into_bytes())
    }
}

static SUBSCRIPTION_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

pub struct SubscribeConfigChanges;

impl HostFunction for SubscribeConfigChanges {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "subscribe_config_changes",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength], // key_pattern
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Registra callback para mudanças de configuração (key_pattern) -> subscription_id"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        // Simula criação de subscription
        let id = SUBSCRIPTION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok((id as i32).to_le_bytes().to_vec())
    }
}

pub struct UnsubscribeConfigChanges;

impl HostFunction for UnsubscribeConfigChanges {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Config,
            name: "unsubscribe_config_changes",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32], // subscription_id
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Cancela inscrição de mudanças (subscription_id)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let _id = i32::from_le_bytes([params[0], params[1], params[2], params[3]]);
        // Em implementação real, removeria a subscription
        Ok(0i32.to_le_bytes().to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert_eq!(functions.len(), 10);
    }

    #[test]
    fn test_set_config_is_dangerous() {
        assert_eq!(SetConfig.permission(), HostFunctionPermission::Dangerous);
    }

    #[test]
    fn test_get_functions_are_safe() {
        assert_eq!(GetConfig.permission(), HostFunctionPermission::Safe);
        assert_eq!(GetTheme.permission(), HostFunctionPermission::Safe);
        assert_eq!(GetPluginConfig.permission(), HostFunctionPermission::Safe);
    }

    #[test]
    fn test_config_set_and_get() {
        let set_fn = SetConfig;
        let get_fn = GetConfig;

        // Set config
        let key = b"test_key";
        let value = b"test_value";
        let params = [
            (key.len() as i32).to_le_bytes().to_vec(),
            key.to_vec(),
            (value.len() as i32).to_le_bytes().to_vec(),
            value.to_vec(),
        ]
        .concat();

        let result = set_fn.execute(&params).unwrap();
        assert_eq!(result, 0i32.to_le_bytes().to_vec());

        // Get config
        let out_buf = [0u8; 100];
        let get_params = [
            (key.len() as i32).to_le_bytes().to_vec(),
            key.to_vec(),
            out_buf.len().to_le_bytes().to_vec(),
        ]
        .concat();

        let result = get_fn.execute(&get_params).unwrap();
        assert_eq!(result, b"test_value");
    }

    #[test]
    fn test_subscribe_and_unsubscribe() {
        let sub = SubscribeConfigChanges;
        let unsub = UnsubscribeConfigChanges;

        let id_bytes = sub.execute(b"*").unwrap();
        let id = i32::from_le_bytes([id_bytes[0], id_bytes[1], id_bytes[2], id_bytes[3]]);
        assert!(id > 0);

        let result = unsub.execute(&id.to_le_bytes()).unwrap();
        assert_eq!(result, 0i32.to_le_bytes().to_vec());
    }

    fn lp(bytes: &[u8]) -> Vec<u8> {
        let mut v = (bytes.len() as i32).to_le_bytes().to_vec();
        v.extend_from_slice(bytes);
        v
    }

    fn set(key: &str, value: &str) -> Result<Vec<u8>, HostFunctionError> {
        let mut p = lp(key.as_bytes());
        p.extend(lp(value.as_bytes()));
        SetConfig.execute(&p)
    }

    fn get(key: &str, max: i32) -> Vec<u8> {
        let mut p = lp(key.as_bytes());
        p.extend(max.to_le_bytes());
        GetConfig.execute(&p).unwrap()
    }

    #[test]
    fn test_get_config_truncates_and_reports_missing() {
        set("teste.trunc", "abcdef").unwrap();
        assert_eq!(get("teste.trunc", 3), b"abc");
        assert_eq!(get("teste.nao.existe", 10), (-1i32).to_le_bytes());
    }

    #[test]
    fn test_config_rejects_malformed_params() {
        let invalid = Err(HostFunctionError::InvalidParams);
        assert_eq!(GetConfig.execute(&[0; 7]), invalid);
        assert_eq!(SetConfig.execute(&[0; 7]), invalid);

        // chave maior que o payload
        let mut p = 30i32.to_le_bytes().to_vec();
        p.extend([0; 4]);
        assert_eq!(GetConfig.execute(&p), invalid);
        assert_eq!(SetConfig.execute(&p), invalid);

        // UTF-8 inválido na chave e no valor
        let mut p = lp(&[0xff]);
        p.extend(4i32.to_le_bytes());
        assert_eq!(GetConfig.execute(&p), invalid);
        let mut p = lp(b"k");
        p.extend(lp(&[0xff]));
        assert_eq!(SetConfig.execute(&p), invalid);

        // valor maior que o payload
        let mut p = lp(b"k");
        p.extend(50i32.to_le_bytes());
        assert_eq!(SetConfig.execute(&p), invalid);

        // Regressão: tamanhos negativos estouravam a soma de offsets
        for len in [-1i32, -4] {
            let mut p = len.to_le_bytes().to_vec();
            p.extend([0; 8]);
            assert_eq!(GetConfig.execute(&p), invalid, "{len}");
            assert_eq!(SetConfig.execute(&p), invalid, "{len}");
        }
        let mut p = lp(b"k");
        p.extend((-1i32).to_le_bytes());
        assert_eq!(SetConfig.execute(&p), invalid);
    }

    #[test]
    fn test_plugin_config_json_roundtrip() {
        SetPluginConfig
            .execute(br#"{"teste.json.a":"1","teste.json.b":"2"}"#)
            .unwrap();
        let json: HashMap<String, String> =
            serde_json::from_slice(&GetPluginConfig.execute(&[]).unwrap()).unwrap();
        assert_eq!(json.get("teste.json.a").map(String::as_str), Some("1"));
        assert_eq!(get("teste.json.b", 10), b"2");

        assert_eq!(
            SetPluginConfig.execute(b"[1,2]"),
            Err(HostFunctionError::InvalidParams)
        );
        assert_eq!(
            SetPluginConfig.execute(&[0xff]),
            Err(HostFunctionError::InvalidParams)
        );
    }

    #[test]
    fn test_static_config_documents() {
        assert_eq!(GetTheme.execute(&[]).unwrap(), b"default");
        let keys: serde_json::Value =
            serde_json::from_slice(&GetKeybindings.execute(&[]).unwrap()).unwrap();
        assert_eq!(keys["new_tab"], "ctrl+shift+t");
        let font: serde_json::Value =
            serde_json::from_slice(&GetFontConfig.execute(&[]).unwrap()).unwrap();
        assert_eq!(font["family"], "JetBrains Mono");
        let terminal: serde_json::Value =
            serde_json::from_slice(&GetTerminalConfig.execute(&[]).unwrap()).unwrap();
        assert!(terminal.is_object());
        assert_eq!(
            UnsubscribeConfigChanges.execute(&[1]),
            Err(HostFunctionError::InvalidParams)
        );
    }
}
