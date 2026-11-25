//! Host Functions - UI
//!
//! Funções para manipulação de interface do usuário.

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

static UI_STATE: LazyLock<Mutex<UiHostState>> =
    LazyLock::new(|| Mutex::new(UiHostState::default()));
static NEXT_OVERLAY_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_STATUS_ITEM_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone)]
struct UiHostState {
    notifications: Vec<(NotificationLevel, String, String)>,
    dialogs: Vec<(String, String, String)>,
    status_text: Option<String>,
    overlays: HashMap<u32, String>,
    window_title: String,
    window_position: (i32, i32),
    window_size: (i32, i32),
    mouse_position: (i32, i32),
    context_menus: Vec<String>,
    status_items: HashMap<u32, (String, String, i32)>,
}

impl Default for UiHostState {
    fn default() -> Self {
        Self {
            notifications: Vec::new(),
            dialogs: Vec::new(),
            status_text: None,
            overlays: HashMap::new(),
            window_title: "Codeterm".to_string(),
            window_position: (0, 0),
            window_size: (800, 600),
            mouse_position: (0, 0),
            context_menus: Vec::new(),
            status_items: HashMap::new(),
        }
    }
}

fn lock_state() -> Result<std::sync::MutexGuard<'static, UiHostState>, HostFunctionError> {
    UI_STATE
        .lock()
        .map_err(|_| HostFunctionError::ExecutionError("Failed to lock UI host state".to_string()))
}

fn i32_bytes(value: i32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn i32_pair_bytes(first: i32, second: i32) -> Vec<u8> {
    [first.to_le_bytes(), second.to_le_bytes()].concat()
}

fn read_i32(params: &[u8], offset: usize) -> Result<i32, HostFunctionError> {
    let bytes: [u8; 4] = params
        .get(offset..offset + 4)
        .ok_or(HostFunctionError::InvalidParams)?
        .try_into()
        .map_err(|_| HostFunctionError::InvalidParams)?;
    Ok(i32::from_le_bytes(bytes))
}

fn text_param(params: &[u8]) -> Result<String, HostFunctionError> {
    String::from_utf8(params.to_vec()).map_err(|_| HostFunctionError::InvalidParams)
}

fn split_nul_text(params: &[u8]) -> Result<Vec<String>, HostFunctionError> {
    let text = text_param(params)?;
    Ok(text.split('\0').map(ToString::to_string).collect())
}

fn title_message(params: &[u8]) -> Result<(String, String), HostFunctionError> {
    let parts = split_nul_text(params)?;
    let title = parts.first().cloned().unwrap_or_default();
    let message = parts.get(1).cloned().unwrap_or_default();
    Ok((title, message))
}

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(ShowNotification),
        Box::new(ShowAlertDialog),
        Box::new(ShowConfirmDialog),
        Box::new(ShowPromptDialog),
        Box::new(SetStatusText),
        Box::new(ClearStatusText),
        Box::new(ShowOverlay),
        Box::new(HideOverlay),
        Box::new(UpdateOverlay),
        Box::new(GetWindowPosition),
        Box::new(GetWindowSize),
        Box::new(SetWindowTitle),
        Box::new(GetMousePosition),
        Box::new(ShowContextMenu),
        Box::new(RegisterStatusBarItem),
        Box::new(UnregisterStatusBarItem),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
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

pub struct ShowNotification;

impl HostFunction for ShowNotification {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "show_notification",
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

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Mostra notificação (level, title, message)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let level = NotificationLevel::from_u32(read_i32(params, 0)? as u32)
            .ok_or(HostFunctionError::InvalidParams)?;
        let (title, message) = title_message(&params[4..])?;
        let mut state = lock_state()?;
        state.notifications.push((level, title, message));
        Ok(i32_bytes(0))
    }
}

pub struct ShowAlertDialog;

impl HostFunction for ShowAlertDialog {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "show_alert_dialog",
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
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Mostra dialog de alerta (title, message)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let (title, message) = title_message(params)?;
        let mut state = lock_state()?;
        state.dialogs.push(("alert".to_string(), title, message));
        Ok(i32_bytes(0))
    }
}

pub struct ShowConfirmDialog;

impl HostFunction for ShowConfirmDialog {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "show_confirm_dialog",
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
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Mostra dialog de confirmação (title, message) -> 1=confirm, 0=cancel"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let (title, message) = title_message(params)?;
        let mut state = lock_state()?;
        state.dialogs.push(("confirm".to_string(), title, message));
        Ok(i32_bytes(1))
    }
}

pub struct ShowPromptDialog;

impl HostFunction for ShowPromptDialog {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "show_prompt_dialog",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength,
                HostParamType::PointerWithLength,
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
        "Mostra dialog de input (title, message, default_value, out_ptr, out_len)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let parts = split_nul_text(params)?;
        let title = parts.first().cloned().unwrap_or_default();
        let message = parts.get(1).cloned().unwrap_or_default();
        let default_value = parts.get(2).cloned().unwrap_or_default();
        let mut state = lock_state()?;
        state.dialogs.push(("prompt".to_string(), title, message));
        Ok(default_value.into_bytes())
    }
}

pub struct SetStatusText;

impl HostFunction for SetStatusText {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "set_status_text",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Define texto na barra de status"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let text = text_param(params)?;
        let len = text.len() as i32;
        let mut state = lock_state()?;
        state.status_text = Some(text);
        Ok(i32_bytes(len))
    }
}

pub struct ClearStatusText;

impl HostFunction for ClearStatusText {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "clear_status_text",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Limpa texto da barra de status"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let mut state = lock_state()?;
        state.status_text = None;
        Ok(i32_bytes(0))
    }
}

pub struct ShowOverlay;

impl HostFunction for ShowOverlay {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "show_overlay",
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
        "Mostra overlay HTML customizado -> overlay_id"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let html = text_param(params)?;
        let overlay_id = NEXT_OVERLAY_ID.fetch_add(1, Ordering::Relaxed);
        let mut state = lock_state()?;
        state.overlays.insert(overlay_id, html);
        Ok(i32_bytes(overlay_id as i32))
    }
}

pub struct HideOverlay;

impl HostFunction for HideOverlay {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "hide_overlay",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Esconde overlay (overlay_id)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let overlay_id = read_i32(params, 0)? as u32;
        let mut state = lock_state()?;
        let removed = state.overlays.remove(&overlay_id).is_some();
        Ok(i32_bytes(if removed { 1 } else { 0 }))
    }
}

pub struct UpdateOverlay;

impl HostFunction for UpdateOverlay {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "update_overlay",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32, HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Atualiza conteúdo do overlay (overlay_id, html)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let overlay_id = read_i32(params, 0)? as u32;
        let html = text_param(&params[4..])?;
        let mut state = lock_state()?;
        let overlay = state
            .overlays
            .get_mut(&overlay_id)
            .ok_or(HostFunctionError::NotFound)?;
        *overlay = html;
        Ok(i32_bytes(1))
    }
}

pub struct GetWindowPosition;

impl HostFunction for GetWindowPosition {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "get_window_position",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém posição da janela (x, y)"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let state = lock_state()?;
        Ok(i32_pair_bytes(
            state.window_position.0,
            state.window_position.1,
        ))
    }
}

pub struct GetWindowSize;

impl HostFunction for GetWindowSize {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "get_window_size",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém tamanho da janela (width, height)"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let state = lock_state()?;
        Ok(i32_pair_bytes(state.window_size.0, state.window_size.1))
    }
}

pub struct SetWindowTitle;

impl HostFunction for SetWindowTitle {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "set_window_title",
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
        "Define título da janela"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let title = text_param(params)?;
        let len = title.len() as i32;
        let mut state = lock_state()?;
        state.window_title = title;
        Ok(i32_bytes(len))
    }
}

pub struct GetMousePosition;

impl HostFunction for GetMousePosition {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "get_mouse_position",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::Pointer],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Obtém posição do mouse (x, y)"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let state = lock_state()?;
        Ok(i32_pair_bytes(
            state.mouse_position.0,
            state.mouse_position.1,
        ))
    }
}

pub struct ShowContextMenu;

impl HostFunction for ShowContextMenu {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "show_context_menu",
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
        "Mostra menu de contexto (items_json) -> selected_index"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let items_json = text_param(params)?;
        let mut state = lock_state()?;
        state.context_menus.push(items_json);
        Ok(i32_bytes(0))
    }
}

pub struct RegisterStatusBarItem;

impl HostFunction for RegisterStatusBarItem {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "register_status_bar_item",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![
                HostParamType::PointerWithLength,
                HostParamType::PointerWithLength,
                HostParamType::I32,
            ],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn permission(&self) -> HostFunctionPermission {
        HostFunctionPermission::Restricted
    }

    fn description(&self) -> &'static str {
        "Registra item na barra de status (id, text, position) -> item_id"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let parts = split_nul_text(params)?;
        let id = parts.first().cloned().unwrap_or_default();
        let text = parts.get(1).cloned().unwrap_or_default();
        let position = parts
            .get(2)
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(0);
        let item_id = NEXT_STATUS_ITEM_ID.fetch_add(1, Ordering::Relaxed);
        let mut state = lock_state()?;
        state.status_items.insert(item_id, (id, text, position));
        Ok(i32_bytes(item_id as i32))
    }
}

pub struct UnregisterStatusBarItem;

impl HostFunction for UnregisterStatusBarItem {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Ui,
            name: "unregister_status_bar_item",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Remove item da barra de status (item_id)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let item_id = read_i32(params, 0)? as u32;
        let mut state = lock_state()?;
        let removed = state.status_items.remove(&item_id).is_some();
        Ok(i32_bytes(if removed { 1 } else { 0 }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_result_i32(bytes: &[u8]) -> i32 {
        i32::from_le_bytes(bytes[..4].try_into().unwrap())
    }

    fn read_result_pair(bytes: &[u8]) -> (i32, i32) {
        (
            i32::from_le_bytes(bytes[..4].try_into().unwrap()),
            i32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        )
    }

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert_eq!(functions.len(), 16);
    }

    #[test]
    fn test_notification_level_from_u32() {
        assert_eq!(
            NotificationLevel::from_u32(0),
            Some(NotificationLevel::Info)
        );
        assert_eq!(
            NotificationLevel::from_u32(2),
            Some(NotificationLevel::Warning)
        );
        assert_eq!(NotificationLevel::from_u32(10), None);
    }

    #[test]
    fn test_show_notification_is_restricted() {
        assert_eq!(
            ShowNotification.permission(),
            HostFunctionPermission::Restricted
        );
    }

    #[test]
    fn test_set_status_text_is_safe() {
        assert_eq!(SetStatusText.permission(), HostFunctionPermission::Safe);
    }

    #[test]
    fn test_notification_and_status_text_execute() {
        let mut params = Vec::new();
        params.extend_from_slice(&(NotificationLevel::Warning as i32).to_le_bytes());
        params.extend_from_slice(b"Aviso\0Mensagem");

        let result = ShowNotification.execute(&params).unwrap();
        assert_eq!(read_result_i32(&result), 0);

        let result = SetStatusText.execute(b"Pronto").unwrap();
        assert_eq!(read_result_i32(&result), 6);
        assert_eq!(lock_state().unwrap().status_text.as_deref(), Some("Pronto"));

        let result = ClearStatusText.execute(&[]).unwrap();
        assert_eq!(read_result_i32(&result), 0);
        assert!(lock_state().unwrap().status_text.is_none());
    }

    #[test]
    fn test_dialogs_return_deterministic_values() {
        let alert = ShowAlertDialog.execute(b"Titulo\0Mensagem").unwrap();
        assert_eq!(read_result_i32(&alert), 0);

        let confirm = ShowConfirmDialog.execute(b"Confirmar\0Executar?").unwrap();
        assert_eq!(read_result_i32(&confirm), 1);

        let prompt = ShowPromptDialog
            .execute(b"Nome\0Informe o nome\0Codeterm")
            .unwrap();
        assert_eq!(String::from_utf8(prompt).unwrap(), "Codeterm");
    }

    #[test]
    fn test_overlay_lifecycle_execute() {
        let overlay_id = read_result_i32(&ShowOverlay.execute(b"<div>1</div>").unwrap());
        assert!(overlay_id > 0);

        let mut update = overlay_id.to_le_bytes().to_vec();
        update.extend_from_slice(b"<div>2</div>");
        let updated = UpdateOverlay.execute(&update).unwrap();
        assert_eq!(read_result_i32(&updated), 1);

        let hidden = HideOverlay.execute(&overlay_id.to_le_bytes()).unwrap();
        assert_eq!(read_result_i32(&hidden), 1);

        let missing_update = UpdateOverlay.execute(&update).unwrap_err();
        assert_eq!(missing_update, HostFunctionError::NotFound);
    }

    #[test]
    fn test_geometry_execute() {
        let position = GetWindowPosition.execute(&[]).unwrap();
        assert_eq!(read_result_pair(&position), (0, 0));

        let size = GetWindowSize.execute(&[]).unwrap();
        assert_eq!(read_result_pair(&size), (800, 600));

        let mouse = GetMousePosition.execute(&[]).unwrap();
        assert_eq!(read_result_pair(&mouse), (0, 0));
    }

    #[test]
    fn test_window_title_context_menu_and_status_item_execute() {
        let title = SetWindowTitle.execute(b"Novo titulo").unwrap();
        assert_eq!(read_result_i32(&title), 11);
        assert_eq!(lock_state().unwrap().window_title, "Novo titulo");

        let menu = ShowContextMenu.execute(br#"["Copiar","Colar"]"#).unwrap();
        assert_eq!(read_result_i32(&menu), 0);

        let item_id = read_result_i32(&RegisterStatusBarItem.execute(b"git\0main\0\x01").unwrap());
        assert!(item_id > 0);

        let removed = UnregisterStatusBarItem
            .execute(&item_id.to_le_bytes())
            .unwrap();
        assert_eq!(read_result_i32(&removed), 1);
    }
}
