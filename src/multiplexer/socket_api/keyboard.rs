//! Keyboard API - Envio de input/teclas via socket

use super::types::*;
use super::{ApiError, ApiResult};
use parking_lot::RwLock;
use serde_json::Value;
use std::sync::Arc;
use tracing::debug;

type InputBackend = Arc<dyn Fn(Option<TabId>, String) -> ApiResult<()> + Send + Sync>;

/// API de teclado
pub struct KeyboardApi {
    shortcuts: Arc<RwLock<Vec<KeyboardShortcut>>>,
    input_backend: Arc<RwLock<Option<InputBackend>>>,
}

impl Default for KeyboardApi {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyboardApi {
    /// Cria uma nova instância da API
    pub fn new() -> Self {
        let shortcuts = vec![
            KeyboardShortcut {
                name: "new_tab".to_string(),
                shortcut: "ctrl+shift+t".to_string(),
                description: "Create a new tab".to_string(),
            },
            KeyboardShortcut {
                name: "close_tab".to_string(),
                shortcut: "ctrl+shift+w".to_string(),
                description: "Close current tab".to_string(),
            },
            KeyboardShortcut {
                name: "next_tab".to_string(),
                shortcut: "ctrl+tab".to_string(),
                description: "Go to next tab".to_string(),
            },
            KeyboardShortcut {
                name: "previous_tab".to_string(),
                shortcut: "ctrl+shift+tab".to_string(),
                description: "Go to previous tab".to_string(),
            },
            KeyboardShortcut {
                name: "copy".to_string(),
                shortcut: "ctrl+shift+c".to_string(),
                description: "Copy selection".to_string(),
            },
            KeyboardShortcut {
                name: "paste".to_string(),
                shortcut: "ctrl+shift+v".to_string(),
                description: "Paste from clipboard".to_string(),
            },
            KeyboardShortcut {
                name: "search".to_string(),
                shortcut: "ctrl+shift+f".to_string(),
                description: "Open search".to_string(),
            },
            KeyboardShortcut {
                name: "zoom_in".to_string(),
                shortcut: "ctrl+plus".to_string(),
                description: "Increase font size".to_string(),
            },
            KeyboardShortcut {
                name: "zoom_out".to_string(),
                shortcut: "ctrl+minus".to_string(),
                description: "Decrease font size".to_string(),
            },
        ];

        Self {
            shortcuts: Arc::new(RwLock::new(shortcuts)),
            input_backend: Arc::new(RwLock::new(None)),
        }
    }

    /// Configura o backend responsável por escrever input no terminal real.
    pub fn set_input_backend<F>(&self, backend: F)
    where
        F: Fn(Option<TabId>, String) -> ApiResult<()> + Send + Sync + 'static,
    {
        *self.input_backend.write() = Some(Arc::new(backend));
    }

    /// Remove o backend de input configurado.
    pub fn clear_input_backend(&self) {
        *self.input_backend.write() = None;
    }

    /// Envia keystrokes para uma tab/pane
    pub fn send(&self, params: SendKeysParams) -> ApiResult<()> {
        let (keys, tab_id, target) = match params {
            SendKeysParams::Simple { keys } => (keys, None, "active tab".to_string()),
            SendKeysParams::Detailed {
                keys,
                tab_id,
                pane_id,
            } => (
                keys,
                tab_id,
                format!("tab {:?}, pane {:?}", tab_id, pane_id),
            ),
        };

        let processed = self.process_sequences(&keys);
        debug!(
            target = %target,
            byte_count = processed.len(),
            "Keyboard send requested"
        );
        self.write_input(tab_id, processed)
    }

    fn input_backend_missing() -> ApiError {
        ApiError::NotImplemented("keyboard input backend is not connected to a PTY yet".to_string())
    }

    fn shortcut_backend_missing(name: &str) -> ApiError {
        ApiError::NotImplemented(format!(
            "shortcut '{}' is not connected to an application action yet",
            name
        ))
    }

    /// Processa sequências especiais de caracteres
    ///
    /// Varre o texto uma vez só: `\\` vira uma barra literal e não pode ser
    /// reinterpretada junto com o caractere seguinte (`\\n` é barra + `n`).
    fn process_sequences(&self, text: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                result.push(c);
                continue;
            }
            match chars.next() {
                Some('n') | Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        }
        result
    }

    /// Envia uma tecla individual com modificadores
    pub fn key(&self, event: KeyEvent, tab_id: Option<TabId>) -> ApiResult<()> {
        let sequence = self.key_event_to_sequence(event);
        debug!(byte_count = sequence.len(), "Keyboard key requested");
        self.write_input(tab_id, sequence)
    }

    /// Converte um evento de tecla em sequência de escape
    fn key_event_to_sequence(&self, event: KeyEvent) -> String {
        // use std::ascii::escape_default;

        // Verifica modificadores
        let has_ctrl = event.modifiers.contains(&Modifier::Ctrl);
        let has_alt = event.modifiers.contains(&Modifier::Alt);

        // Mapeia teclas especiais
        let key = match event.key.to_lowercase().as_str() {
            "enter" | "return" => "\r".to_string(),
            "tab" => "\t".to_string(),
            "backspace" => "\x7f".to_string(),
            "escape" | "esc" => "\x1b".to_string(),
            "space" => " ".to_string(),
            "up" => "\x1b[A".to_string(),
            "down" => "\x1b[B".to_string(),
            "right" => "\x1b[C".to_string(),
            "left" => "\x1b[D".to_string(),
            "home" => "\x1b[H".to_string(),
            "end" => "\x1b[F".to_string(),
            "pageup" => "\x1b[5~".to_string(),
            "pagedown" => "\x1b[6~".to_string(),
            "delete" => "\x1b[3~".to_string(),
            "insert" => "\x1b[2~".to_string(),
            "f1" => "\x1bOP".to_string(),
            "f2" => "\x1bOQ".to_string(),
            "f3" => "\x1bOR".to_string(),
            "f4" => "\x1bOS".to_string(),
            "f5" => "\x1b[15~".to_string(),
            "f6" => "\x1b[17~".to_string(),
            "f7" => "\x1b[18~".to_string(),
            "f8" => "\x1b[19~".to_string(),
            "f9" => "\x1b[20~".to_string(),
            "f10" => "\x1b[21~".to_string(),
            "f11" => "\x1b[23~".to_string(),
            "f12" => "\x1b[24~".to_string(),
            k if k.len() == 1 => {
                let c = k.chars().next().unwrap();

                if has_ctrl {
                    // Converte para controle: Ctrl+A = 0x01, etc.
                    if c.is_ascii_lowercase() {
                        ((c as u8 - b'a' + 1) as char).to_string()
                    } else if c.is_ascii_uppercase() {
                        ((c as u8 - b'A' + 1) as char).to_string()
                    } else {
                        c.to_string()
                    }
                } else {
                    c.to_string()
                }
            }
            _ => event.key,
        };

        // Aplica modificador Alt (prefixa com ESC)
        if has_alt {
            format!("\x1b{}", key)
        } else {
            key
        }
    }

    /// Envia texto literal (sem processar sequências)
    pub fn text(&self, text: String, tab_id: Option<TabId>) -> ApiResult<()> {
        debug!(byte_count = text.len(), "Keyboard text requested");
        self.write_input(tab_id, text)
    }

    /// Envia um chord (sequência de teclas)
    pub fn chord(&self, keys: Vec<String>, tab_id: Option<TabId>) -> ApiResult<()> {
        for key in keys {
            self.send(SendKeysParams::Detailed {
                keys: key,
                tab_id,
                pane_id: None,
            })?;
        }
        Ok(())
    }

    fn write_input(&self, tab_id: Option<TabId>, input: String) -> ApiResult<()> {
        let backend = self
            .input_backend
            .read()
            .clone()
            .ok_or_else(Self::input_backend_missing)?;

        backend(tab_id, input)
    }

    /// Executa um atalho de teclado pelo nome
    pub fn shortcut(&self, name: &str) -> ApiResult<()> {
        let shortcuts = self.shortcuts.read();

        let shortcut = shortcuts
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| ApiError::NotFound(format!("Shortcut not found: {}", name)))?;

        debug!("Executing shortcut: {} ({})", name, shortcut.shortcut);

        // Parse e executa o shortcut
        self.parse_and_execute(&shortcut.shortcut)
    }

    /// Lista todos os atalhos disponíveis
    pub fn list_shortcuts(&self) -> ApiResult<Vec<KeyboardShortcut>> {
        let shortcuts = self.shortcuts.read();
        Ok(shortcuts.clone())
    }

    /// Parse e executa um atalho de teclado
    fn parse_and_execute(&self, shortcut: &str) -> ApiResult<()> {
        // Parse simples de atalhos tipo "ctrl+shift+t"
        let parts: Vec<&str> = shortcut.split('+').collect();

        let mut modifiers = Vec::new();
        let mut key = String::new();

        for part in parts {
            match part.to_lowercase().as_str() {
                "ctrl" => modifiers.push(Modifier::Ctrl),
                "shift" => modifiers.push(Modifier::Shift),
                "alt" => modifiers.push(Modifier::Alt),
                "meta" | "cmd" | "super" => modifiers.push(Modifier::Meta),
                k => key = k.to_string(),
            }
        }

        // Mapeia ações de atalhos conhecidos. São ações da aplicação: não
        // podem cair no PTY (ctrl+tab digitaria um TAB e ctrl+plus o texto "plus").
        let ctrl = modifiers.contains(&Modifier::Ctrl);
        let ctrl_shift = ctrl && modifiers.contains(&Modifier::Shift);
        let action = match key.to_lowercase().as_str() {
            "t" if ctrl_shift => Some("new_tab"),
            "w" if ctrl_shift => Some("close_tab"),
            "c" if ctrl_shift => Some("copy"),
            "v" if ctrl_shift => Some("paste"),
            "f" if ctrl_shift => Some("search"),
            "tab" if ctrl_shift => Some("previous_tab"),
            "tab" if ctrl => Some("next_tab"),
            "plus" if ctrl => Some("zoom_in"),
            "minus" if ctrl => Some("zoom_out"),
            _ => None,
        };
        match action {
            Some(action) => Err(Self::shortcut_backend_missing(action)),
            None => {
                // Executa como key event
                self.key(KeyEvent { key, modifiers }, None)
            }
        }
    }

    /// Handler para método JSON-RPC
    pub fn handle_method(&self, method: &str, params: Value) -> ApiResult<Value> {
        match method {
            "keyboard.send" => {
                let send_params: SendKeysParams = serde_json::from_value(params)
                    .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                self.send(send_params)?;
                Ok(Value::Null)
            }
            "keyboard.key" => {
                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId);
                let event: KeyEvent =
                    serde_json::from_value(params.get("event").cloned().unwrap_or(params))
                        .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                self.key(event, tab_id)?;
                Ok(Value::Null)
            }
            "keyboard.text" => {
                let text = params.get("text").and_then(|v| v.as_str()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'text' parameter".to_string())
                })?;
                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId);
                self.text(text.to_string(), tab_id)?;
                Ok(Value::Null)
            }
            "keyboard.chord" => {
                let keys: Vec<String> = serde_json::from_value(
                    params.get("keys").cloned().unwrap_or(Value::Array(vec![])),
                )
                .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId);
                self.chord(keys, tab_id)?;
                Ok(Value::Null)
            }
            "keyboard.shortcut" => {
                let name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'name' parameter".to_string())
                })?;
                self.shortcut(name)?;
                Ok(Value::Null)
            }
            "keyboard.list_shortcuts" => {
                let shortcuts = self.list_shortcuts()?;
                serde_json::to_value(shortcuts).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            _ => Err(ApiError::MethodNotFound(method.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_api() -> KeyboardApi {
        KeyboardApi::new()
    }

    #[test]
    fn test_process_sequences() {
        let api = create_test_api();

        assert_eq!(api.process_sequences("hello\\n"), "hello\r");
        assert_eq!(api.process_sequences("tab\\there"), "tab\there");
        assert_eq!(api.process_sequences("path\\\\file"), "path\\file");
    }

    #[test]
    fn test_key_event_to_sequence() {
        let api = create_test_api();

        // Teclas especiais
        assert_eq!(
            api.key_event_to_sequence(KeyEvent {
                key: "enter".to_string(),
                modifiers: vec![]
            }),
            "\r"
        );
        assert_eq!(
            api.key_event_to_sequence(KeyEvent {
                key: "tab".to_string(),
                modifiers: vec![]
            }),
            "\t"
        );
        assert_eq!(
            api.key_event_to_sequence(KeyEvent {
                key: "escape".to_string(),
                modifiers: vec![]
            }),
            "\x1b"
        );
        assert_eq!(
            api.key_event_to_sequence(KeyEvent {
                key: "up".to_string(),
                modifiers: vec![]
            }),
            "\x1b[A"
        );
        assert_eq!(
            api.key_event_to_sequence(KeyEvent {
                key: "f1".to_string(),
                modifiers: vec![]
            }),
            "\x1bOP"
        );

        // Com Ctrl
        assert_eq!(
            api.key_event_to_sequence(KeyEvent {
                key: "a".to_string(),
                modifiers: vec![Modifier::Ctrl]
            }),
            "\x01"
        );
        assert_eq!(
            api.key_event_to_sequence(KeyEvent {
                key: "c".to_string(),
                modifiers: vec![Modifier::Ctrl]
            }),
            "\x03"
        );

        // Com Alt
        assert_eq!(
            api.key_event_to_sequence(KeyEvent {
                key: "x".to_string(),
                modifiers: vec![Modifier::Alt]
            }),
            "\x1bx"
        );
    }

    #[test]
    fn test_list_shortcuts() {
        let api = create_test_api();
        let shortcuts = api.list_shortcuts().unwrap();

        assert!(!shortcuts.is_empty());
        assert!(shortcuts.iter().any(|s| s.name == "new_tab"));
        assert!(shortcuts.iter().any(|s| s.name == "copy"));
    }

    #[test]
    fn test_shortcut_not_found() {
        let api = create_test_api();
        let result = api.shortcut("nonexistent");

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApiError::NotFound(_)));
    }

    #[test]
    fn test_send_uses_configured_input_backend() {
        let api = create_test_api();
        let received = Arc::new(RwLock::new(Vec::new()));
        let backend_received = Arc::clone(&received);

        api.set_input_backend(move |tab_id, input| {
            backend_received.write().push((tab_id, input));
            Ok(())
        });

        api.send(SendKeysParams::Detailed {
            keys: "echo test\\n".to_string(),
            tab_id: Some(42),
            pane_id: None,
        })
        .unwrap();

        let received = received.read();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0], (Some(42), "echo test\r".to_string()));
    }

    #[test]
    fn test_text_errors_without_input_backend() {
        let api = create_test_api();
        api.clear_input_backend();

        let result = api.text("hello".to_string(), None);

        assert!(matches!(result, Err(ApiError::NotImplemented(_))));
    }

    type Received = Arc<RwLock<Vec<(Option<TabId>, String)>>>;

    fn api_with_recorder() -> (KeyboardApi, Received) {
        let api = KeyboardApi::default();
        let received: Received = Arc::new(RwLock::new(Vec::new()));
        let sink = Arc::clone(&received);
        api.set_input_backend(move |tab_id, input| {
            sink.write().push((tab_id, input));
            Ok(())
        });
        (api, received)
    }

    fn seq(api: &KeyboardApi, key: &str, modifiers: Vec<Modifier>) -> String {
        api.key_event_to_sequence(KeyEvent {
            key: key.to_string(),
            modifiers,
        })
    }

    /// Regressão: "\\n" (barra escapada + n) virava barra + CR.
    #[test]
    fn test_process_sequences_handles_escaped_backslash_first() {
        let api = create_test_api();
        assert_eq!(api.process_sequences("C:\\\\new"), "C:\\new");
        assert_eq!(api.process_sequences("a\\rb"), "a\rb");
        assert_eq!(api.process_sequences("x\\q"), "x\\q");
        assert_eq!(api.process_sequences("fim\\"), "fim\\");
        assert_eq!(api.process_sequences("açúcar\\t"), "açúcar\t");
    }

    #[test]
    fn test_key_event_special_keys_and_modifiers() {
        let api = create_test_api();
        let cases = [
            ("return", "\r"),
            ("backspace", "\x7f"),
            ("esc", "\x1b"),
            ("space", " "),
            ("down", "\x1b[B"),
            ("right", "\x1b[C"),
            ("left", "\x1b[D"),
            ("home", "\x1b[H"),
            ("end", "\x1b[F"),
            ("pageup", "\x1b[5~"),
            ("pagedown", "\x1b[6~"),
            ("delete", "\x1b[3~"),
            ("insert", "\x1b[2~"),
            ("f2", "\x1bOQ"),
            ("f3", "\x1bOR"),
            ("f4", "\x1bOS"),
            ("f5", "\x1b[15~"),
            ("f6", "\x1b[17~"),
            ("f7", "\x1b[18~"),
            ("f8", "\x1b[19~"),
            ("f9", "\x1b[20~"),
            ("f10", "\x1b[21~"),
            ("f11", "\x1b[23~"),
            ("f12", "\x1b[24~"),
        ];
        for (key, expected) in cases {
            assert_eq!(seq(&api, key, vec![]), expected, "{key}");
        }

        // Ctrl com letra maiúscula, com não-letra e tecla desconhecida
        assert_eq!(seq(&api, "B", vec![Modifier::Ctrl]), "\x02");
        assert_eq!(seq(&api, "1", vec![Modifier::Ctrl]), "1");
        assert_eq!(seq(&api, "q", vec![]), "q");
        assert_eq!(seq(&api, "Nada", vec![]), "Nada");
        assert_eq!(seq(&api, "up", vec![Modifier::Alt]), "\x1b\x1b[A");
    }

    #[test]
    fn test_key_text_and_chord_write_through_backend() {
        let (api, received) = api_with_recorder();
        api.key(
            KeyEvent {
                key: "c".to_string(),
                modifiers: vec![Modifier::Ctrl],
            },
            Some(3),
        )
        .unwrap();
        api.text("literal\\n".to_string(), None).unwrap();
        api.chord(vec!["a".to_string(), "b\\n".to_string()], Some(4))
            .unwrap();
        api.send(SendKeysParams::Simple {
            keys: "ls".to_string(),
        })
        .unwrap();

        assert_eq!(
            received.read().clone(),
            vec![
                (Some(3), "\x03".to_string()),
                (None, "literal\\n".to_string()),
                (Some(4), "a".to_string()),
                (Some(4), "b\r".to_string()),
                (None, "ls".to_string()),
            ]
        );
    }

    #[test]
    fn test_backend_errors_propagate() {
        let api = create_test_api();
        api.set_input_backend(|_, _| Err(ApiError::InvalidState("pty fechado".to_string())));
        assert!(matches!(
            api.chord(vec!["a".to_string()], None),
            Err(ApiError::InvalidState(_))
        ));
    }

    /// Regressão: next_tab digitava um TAB no shell e zoom_in o texto "plus".
    #[test]
    fn test_builtin_shortcuts_never_write_to_pty() {
        let (api, received) = api_with_recorder();
        for shortcut in api.list_shortcuts().unwrap() {
            match api.shortcut(&shortcut.name) {
                Err(ApiError::NotImplemented(message)) => {
                    assert!(message.contains(&shortcut.name), "{message}")
                }
                other => panic!(
                    "{}: esperava NotImplemented, veio {:?}",
                    shortcut.name, other
                ),
            }
        }
        assert!(received.read().is_empty());
    }

    #[test]
    fn test_unknown_shortcut_combination_is_sent_as_keys() {
        let (api, received) = api_with_recorder();
        api.parse_and_execute("alt+meta+x").unwrap();
        api.parse_and_execute("CTRL+Shift+Z").unwrap();
        assert_eq!(
            received.read().clone(),
            vec![(None, "\x1bx".to_string()), (None, "\x1a".to_string())]
        );
    }

    #[test]
    fn test_handle_method_routes() {
        let (api, received) = api_with_recorder();
        let call = |method: &str, params: Value| api.handle_method(method, params);

        assert_eq!(
            call(
                "keyboard.send",
                serde_json::json!({"keys": "a\\t", "tab_id": 1})
            )
            .unwrap(),
            Value::Null
        );
        call(
            "keyboard.key",
            serde_json::json!({"event": {"key": "enter"}, "tab_id": 2}),
        )
        .unwrap();
        call(
            "keyboard.key",
            serde_json::json!({"key": "x", "modifiers": ["alt"]}),
        )
        .unwrap();
        call(
            "keyboard.text",
            serde_json::json!({"text": "t", "tab_id": 5}),
        )
        .unwrap();
        call("keyboard.chord", serde_json::json!({"keys": ["p", "q"]})).unwrap();
        call("keyboard.chord", serde_json::json!({})).unwrap();

        assert_eq!(
            received.read().clone(),
            vec![
                (Some(1), "a\t".to_string()),
                (Some(2), "\r".to_string()),
                (None, "\x1bx".to_string()),
                (Some(5), "t".to_string()),
                (None, "p".to_string()),
                (None, "q".to_string()),
            ]
        );

        let shortcuts = call("keyboard.list_shortcuts", Value::Null).unwrap();
        assert_eq!(shortcuts.as_array().unwrap().len(), 9);
        assert!(matches!(
            call("keyboard.shortcut", serde_json::json!({"name": "copy"})),
            Err(ApiError::NotImplemented(_))
        ));

        for (method, params) in [
            ("keyboard.send", serde_json::json!({"nada": 1})),
            ("keyboard.key", serde_json::json!({"modifiers": []})),
            ("keyboard.text", serde_json::json!({})),
            ("keyboard.chord", serde_json::json!({"keys": "nao-lista"})),
            ("keyboard.shortcut", serde_json::json!({})),
        ] {
            assert!(
                matches!(call(method, params), Err(ApiError::InvalidParams(_))),
                "{method}"
            );
        }
        match call("keyboard.explode", Value::Null) {
            Err(ApiError::MethodNotFound(method)) => assert_eq!(method, "keyboard.explode"),
            other => panic!("esperava MethodNotFound, veio {:?}", other),
        }
    }
}
