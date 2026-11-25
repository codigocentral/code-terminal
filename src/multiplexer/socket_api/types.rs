//! Tipos compartilhados da Socket API

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

// Re-exportação de tipos comuns
pub use crate::types::{PaneId, TabId};

/// ID de um workspace
pub type WorkspaceId = String;

/// Informações de uma tab
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: TabId,
    pub index: usize,
    pub title: String,
    pub is_active: bool,
    pub working_dir: Option<String>,
    pub process: Option<String>,
    pub pid: Option<u32>,
}

/// Informações de um pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub id: PaneId,
    pub index: usize,
    pub tab_id: TabId,
    pub title: String,
    pub is_active: bool,
    pub working_dir: Option<String>,
    pub size: PaneSize,
    pub position: PanePosition,
}

/// Tamanho de um pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSize {
    pub cols: u16,
    pub rows: u16,
    pub width_percent: f32,
    pub height_percent: f32,
}

/// Posição de um pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanePosition {
    pub x: u16,
    pub y: u16,
}

/// Informações de um workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub id: WorkspaceId,
    pub name: String,
    pub is_current: bool,
    pub tab_count: usize,
    pub working_dir: Option<String>,
}

/// Direção de divisão de pane
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// Direção de foco/navegação
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    Next,
    Previous,
}

/// Layout de panes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PaneLayout {
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainVertical,
    Tiled,
    Custom(String),
}

/// Evento de teclado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEvent {
    pub key: String,
    #[serde(default)]
    pub modifiers: Vec<Modifier>,
}

/// Modificadores de tecla
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
    Meta,
    Super,
}

/// Tipo de estado query
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StateQueryType {
    Tabs,
    Panes,
    Workspace,
    Selection,
    Scrollback,
    Config { key: Option<String> },
    All,
}

/// Resultado de state query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateQueryResult {
    pub query_type: StateQueryType,
    pub data: serde_json::Value,
    pub timestamp: u64,
}

/// Estado completo do terminal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalState {
    pub version: String,
    pub workspace: WorkspaceInfo,
    pub tabs: Vec<TabInfo>,
    pub panes: Vec<PaneInfo>,
    pub active_tab: TabId,
    pub active_pane: PaneId,
    pub terminal_size: TerminalSize,
    pub config: HashMap<String, serde_json::Value>,
}

/// Tamanho do terminal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalSize {
    pub cols: u16,
    pub rows: u16,
    pub width_pixels: Option<u32>,
    pub height_pixels: Option<u32>,
}

/// Evento de notificação
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEvent {
    pub event_type: String,
    pub data: serde_json::Value,
    pub timestamp: u64,
}

/// Resposta de operação
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl OperationResult {
    pub fn ok() -> Self {
        Self {
            success: true,
            message: None,
            data: None,
        }
    }

    pub fn ok_with_message(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            data: None,
        }
    }

    pub fn ok_with_data(data: serde_json::Value) -> Self {
        Self {
            success: true,
            message: None,
            data: Some(data),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(message.into()),
            data: None,
        }
    }
}

/// Parâmetros de criação de tab
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CreateTabParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}

/// Parâmetros de divisão de pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitPaneParams {
    pub direction: SplitDirection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}

/// Parâmetros de redimensionamento
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizePaneParams {
    pub direction: Direction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absolute_size: Option<u16>,
}

/// Parâmetros de envio de teclas
#[derive(Debug, Clone, Serialize)]
pub enum SendKeysParams {
    Simple {
        keys: String,
    },
    Detailed {
        keys: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<TabId>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pane_id: Option<PaneId>,
    },
}

impl<'de> Deserialize<'de> for SendKeysParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            keys: String,
            tab_id: Option<TabId>,
            pane_id: Option<PaneId>,
        }

        let helper = Helper::deserialize(deserializer)?;
        if helper.tab_id.is_some() || helper.pane_id.is_some() {
            Ok(SendKeysParams::Detailed {
                keys: helper.keys,
                tab_id: helper.tab_id,
                pane_id: helper.pane_id,
            })
        } else {
            Ok(SendKeysParams::Simple { keys: helper.keys })
        }
    }
}

/// Shortcuts de teclado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardShortcut {
    pub name: String,
    pub shortcut: String,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_info_serialization() {
        let info = TabInfo {
            id: 1,
            index: 0,
            title: "test".to_string(),
            is_active: true,
            working_dir: Some("/home".to_string()),
            process: Some("bash".to_string()),
            pid: Some(1234),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("1234"));
    }

    #[test]
    fn test_split_direction_serialization() {
        let dir = SplitDirection::Horizontal;
        let json = serde_json::to_string(&dir).unwrap();
        assert_eq!(json, "\"horizontal\"");
    }

    #[test]
    fn test_operation_result() {
        let result = OperationResult::ok();
        assert!(result.success);
        assert!(result.message.is_none());

        let result = OperationResult::error("failed");
        assert!(!result.success);
        assert_eq!(result.message, Some("failed".to_string()));
    }

    #[test]
    fn test_send_keys_params() {
        // Test simple params
        let json = r#"{"keys":"hello"}"#;
        let params: SendKeysParams = serde_json::from_str(json).unwrap();
        match params {
            SendKeysParams::Simple { keys } => assert_eq!(keys, "hello"),
            _ => panic!("Expected Simple params"),
        }

        // Test detailed params
        let json = r#"{"keys":"hello","tab_id":1,"pane_id":2}"#;
        let params: SendKeysParams = serde_json::from_str(json).unwrap();
        match params {
            SendKeysParams::Detailed {
                keys,
                tab_id,
                pane_id,
            } => {
                assert_eq!(keys, "hello");
                assert_eq!(tab_id, Some(1));
                assert_eq!(pane_id, Some(2));
            }
            _ => panic!("Expected Detailed params"),
        }
    }
}
