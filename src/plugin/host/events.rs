//! Host Functions - Events
//!
//! Funções para sistema de eventos e callbacks.

use super::{
    HostFunction, HostFunctionCategory, HostFunctionError, HostFunctionId, HostFunctionPermission,
    HostFunctionSignature, HostParamType, HostReturnType,
};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex};

static SUBSCRIPTIONS: LazyLock<Mutex<HashMap<u32, (EventType, u32)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static COMMANDS: LazyLock<Mutex<HashMap<String, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static EVENT_QUEUE: LazyLock<Mutex<VecDeque<QueuedEvent>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
static TIMERS: LazyLock<Mutex<HashMap<u32, TimerSpec>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_SUBSCRIPTION_ID: AtomicU32 = AtomicU32::new(1);
static NEXT_TIMER_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone)]
struct QueuedEvent {
    event_type: EventType,
    data: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct TimerSpec {
    delay_ms: i64,
    repeating: bool,
}

fn i32_bytes(value: i32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

fn read_i32(params: &[u8]) -> Result<i32, HostFunctionError> {
    let bytes: [u8; 4] = params
        .get(..4)
        .ok_or(HostFunctionError::InvalidParams)?
        .try_into()
        .map_err(|_| HostFunctionError::InvalidParams)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_i64(params: &[u8]) -> Result<i64, HostFunctionError> {
    let bytes: [u8; 8] = params
        .get(..8)
        .ok_or(HostFunctionError::InvalidParams)?
        .try_into()
        .map_err(|_| HostFunctionError::InvalidParams)?;
    Ok(i64::from_le_bytes(bytes))
}

fn event_type_from_i32(value: i32) -> Result<EventType, HostFunctionError> {
    EventType::from_u32(value as u32).ok_or(HostFunctionError::InvalidParams)
}

fn text_param(params: &[u8]) -> Result<String, HostFunctionError> {
    String::from_utf8(params.to_vec()).map_err(|_| HostFunctionError::InvalidParams)
}

fn lock_error() -> HostFunctionError {
    HostFunctionError::ExecutionError("Failed to lock event host state".to_string())
}

pub fn all_functions() -> Vec<Box<dyn HostFunction>> {
    vec![
        Box::new(SubscribeEvent),
        Box::new(UnsubscribeEvent),
        Box::new(EmitEvent),
        Box::new(RegisterCommand),
        Box::new(UnregisterCommand),
        Box::new(ExecuteCommand),
        Box::new(GetEventQueueSize),
        Box::new(FlushEvents),
        Box::new(SetTimeout),
        Box::new(SetInterval),
        Box::new(ClearTimer),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum EventType {
    KeyPress = 0,
    KeyRelease = 1,
    MouseMove = 2,
    MouseClick = 3,
    MouseScroll = 4,
    Resize = 5,
    Focus = 6,
    Blur = 7,
    TabCreated = 8,
    TabClosed = 9,
    TabSwitched = 10,
    GridChanged = 11,
    SelectionChanged = 12,
    ThemeChanged = 13,
    ConfigChanged = 14,
    Custom = 100,
}

impl EventType {
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::KeyPress),
            1 => Some(Self::KeyRelease),
            2 => Some(Self::MouseMove),
            3 => Some(Self::MouseClick),
            4 => Some(Self::MouseScroll),
            5 => Some(Self::Resize),
            6 => Some(Self::Focus),
            7 => Some(Self::Blur),
            8 => Some(Self::TabCreated),
            9 => Some(Self::TabClosed),
            10 => Some(Self::TabSwitched),
            11 => Some(Self::GridChanged),
            12 => Some(Self::SelectionChanged),
            13 => Some(Self::ThemeChanged),
            14 => Some(Self::ConfigChanged),
            100..=u32::MAX => Some(Self::Custom),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::KeyPress => "key_press",
            Self::KeyRelease => "key_release",
            Self::MouseMove => "mouse_move",
            Self::MouseClick => "mouse_click",
            Self::MouseScroll => "mouse_scroll",
            Self::Resize => "resize",
            Self::Focus => "focus",
            Self::Blur => "blur",
            Self::TabCreated => "tab_created",
            Self::TabClosed => "tab_closed",
            Self::TabSwitched => "tab_switched",
            Self::GridChanged => "grid_changed",
            Self::SelectionChanged => "selection_changed",
            Self::ThemeChanged => "theme_changed",
            Self::ConfigChanged => "config_changed",
            Self::Custom => "custom",
        }
    }
}

pub struct SubscribeEvent;

impl HostFunction for SubscribeEvent {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "subscribe_event",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32, HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Inscreve para eventos (event_type, flags) -> subscription_id"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 8 {
            return Err(HostFunctionError::InvalidParams);
        }

        let event_type = event_type_from_i32(read_i32(params)?)?;
        let flags = read_i32(&params[4..])? as u32;
        let subscription_id = NEXT_SUBSCRIPTION_ID.fetch_add(1, Ordering::SeqCst);

        SUBSCRIPTIONS
            .lock()
            .map_err(|_| lock_error())?
            .insert(subscription_id, (event_type, flags));

        Ok(i32_bytes(subscription_id as i32))
    }
}

pub struct UnsubscribeEvent;

impl HostFunction for UnsubscribeEvent {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "unsubscribe_event",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Cancela inscrição de evento (subscription_id)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let subscription_id = read_i32(params)? as u32;
        let removed = SUBSCRIPTIONS
            .lock()
            .map_err(|_| lock_error())?
            .remove(&subscription_id)
            .is_some();

        Ok(i32_bytes(if removed { 1 } else { 0 }))
    }
}

pub struct EmitEvent;

impl HostFunction for EmitEvent {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "emit_event",
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
        "Emite evento customizado (event_type, data_json)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        if params.len() < 4 {
            return Err(HostFunctionError::InvalidParams);
        }

        let event_type = event_type_from_i32(read_i32(params)?)?;
        let data = params[4..].to_vec();

        EVENT_QUEUE
            .lock()
            .map_err(|_| lock_error())?
            .push_back(QueuedEvent { event_type, data });

        Ok(i32_bytes(0))
    }
}

pub struct RegisterCommand;

impl HostFunction for RegisterCommand {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "register_command",
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
        "Registra comando customizado (command_name, description)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let payload: serde_json::Value =
            serde_json::from_slice(params).map_err(|_| HostFunctionError::InvalidParams)?;
        let name = payload
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or(HostFunctionError::InvalidParams)?;
        let description = payload
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        COMMANDS
            .lock()
            .map_err(|_| lock_error())?
            .insert(name.to_string(), description.to_string());

        Ok(i32_bytes(0))
    }
}

pub struct UnregisterCommand;

impl HostFunction for UnregisterCommand {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "unregister_command",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::PointerWithLength],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Remove comando registrado (command_name)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let name = text_param(params)?;
        let removed = COMMANDS
            .lock()
            .map_err(|_| lock_error())?
            .remove(&name)
            .is_some();

        Ok(i32_bytes(if removed { 1 } else { 0 }))
    }
}

pub struct ExecuteCommand;

impl HostFunction for ExecuteCommand {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "execute_command",
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
        "Executa comando (command_name, args_json)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let payload: serde_json::Value =
            serde_json::from_slice(params).map_err(|_| HostFunctionError::InvalidParams)?;
        let name = payload
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or(HostFunctionError::InvalidParams)?;

        let exists = COMMANDS
            .lock()
            .map_err(|_| lock_error())?
            .contains_key(name);

        if exists {
            Ok(i32_bytes(0))
        } else {
            Err(HostFunctionError::NotFound)
        }
    }
}

pub struct GetEventQueueSize;

impl HostFunction for GetEventQueueSize {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "get_event_queue_size",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Retorna tamanho da fila de eventos pendentes"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let size = EVENT_QUEUE.lock().map_err(|_| lock_error())?.len();
        Ok(i32_bytes(size as i32))
    }
}

pub struct FlushEvents;

impl HostFunction for FlushEvents {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "flush_events",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Processa todos os eventos pendentes"
    }

    fn execute(&self, _params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let mut queue = EVENT_QUEUE.lock().map_err(|_| lock_error())?;
        let flushed = queue.len();
        for event in queue.iter() {
            tracing::debug!(
                event_type = event.event_type.name(),
                data_len = event.data.len(),
                "Flushing queued plugin event"
            );
        }
        queue.clear();
        Ok(i32_bytes(flushed as i32))
    }
}

pub struct SetTimeout;

impl HostFunction for SetTimeout {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "set_timeout",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I64],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Agenda callback para executar após delay_ms -> timer_id"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let delay_ms = read_i64(params)?;
        if delay_ms < 0 {
            return Err(HostFunctionError::InvalidParams);
        }

        let timer_id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);
        TIMERS.lock().map_err(|_| lock_error())?.insert(
            timer_id,
            TimerSpec {
                delay_ms,
                repeating: false,
            },
        );
        Ok(i32_bytes(timer_id as i32))
    }
}

pub struct SetInterval;

impl HostFunction for SetInterval {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "set_interval",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I64],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Agenda callback periódico a cada interval_ms -> timer_id"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let delay_ms = read_i64(params)?;
        if delay_ms <= 0 {
            return Err(HostFunctionError::InvalidParams);
        }

        let timer_id = NEXT_TIMER_ID.fetch_add(1, Ordering::SeqCst);
        TIMERS.lock().map_err(|_| lock_error())?.insert(
            timer_id,
            TimerSpec {
                delay_ms,
                repeating: true,
            },
        );
        Ok(i32_bytes(timer_id as i32))
    }
}

pub struct ClearTimer;

impl HostFunction for ClearTimer {
    fn id(&self) -> HostFunctionId {
        HostFunctionId {
            category: HostFunctionCategory::Events,
            name: "clear_timer",
        }
    }

    fn signature(&self) -> HostFunctionSignature {
        HostFunctionSignature {
            params: vec![HostParamType::I32],
            return_type: Some(HostReturnType::I32),
        }
    }

    fn description(&self) -> &'static str {
        "Cancela timer (timer_id)"
    }

    fn execute(&self, params: &[u8]) -> Result<Vec<u8>, HostFunctionError> {
        let timer_id = read_i32(params)? as u32;
        let removed = TIMERS.lock().map_err(|_| lock_error())?.remove(&timer_id);

        if let Some(timer) = removed {
            tracing::debug!(
                timer_id,
                delay_ms = timer.delay_ms,
                repeating = timer.repeating,
                "Cleared plugin timer"
            );
            Ok(i32_bytes(1))
        } else {
            Ok(i32_bytes(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_functions_exist() {
        let functions = all_functions();
        assert_eq!(functions.len(), 11);
    }

    #[test]
    fn test_event_type_from_u32() {
        assert_eq!(EventType::from_u32(0), Some(EventType::KeyPress));
        assert_eq!(EventType::from_u32(8), Some(EventType::TabCreated));
        assert_eq!(EventType::from_u32(100), Some(EventType::Custom));
        assert_eq!(EventType::from_u32(50), None);
    }

    #[test]
    fn test_event_type_names() {
        assert_eq!(EventType::KeyPress.name(), "key_press");
        assert_eq!(EventType::TabCreated.name(), "tab_created");
    }

    #[test]
    fn test_emit_event_is_restricted() {
        assert_eq!(EmitEvent.permission(), HostFunctionPermission::Restricted);
    }

    #[test]
    fn test_subscribe_and_unsubscribe_event() {
        let mut params = (EventType::Focus as i32).to_le_bytes().to_vec();
        params.extend_from_slice(&0i32.to_le_bytes());

        let id = SubscribeEvent.execute(&params).unwrap();
        let id = i32::from_le_bytes(id.try_into().unwrap());
        assert!(id > 0);

        let removed = UnsubscribeEvent.execute(&id.to_le_bytes()).unwrap();
        assert_eq!(i32::from_le_bytes(removed.try_into().unwrap()), 1);
    }

    #[test]
    fn test_emit_queue_size_and_flush() {
        FlushEvents.execute(&[]).unwrap();

        let mut params = (EventType::Custom as i32).to_le_bytes().to_vec();
        params.extend_from_slice(br#"{"ok":true}"#);
        EmitEvent.execute(&params).unwrap();

        let size = GetEventQueueSize.execute(&[]).unwrap();
        assert_eq!(i32::from_le_bytes(size.try_into().unwrap()), 1);

        let flushed = FlushEvents.execute(&[]).unwrap();
        assert_eq!(i32::from_le_bytes(flushed.try_into().unwrap()), 1);
    }

    #[test]
    fn test_register_execute_and_unregister_command() {
        let payload = br#"{"name":"test.command","description":"Test command"}"#;
        let result = RegisterCommand.execute(payload).unwrap();
        assert_eq!(i32::from_le_bytes(result.try_into().unwrap()), 0);

        let execute_payload = br#"{"name":"test.command","args":{}}"#;
        let result = ExecuteCommand.execute(execute_payload).unwrap();
        assert_eq!(i32::from_le_bytes(result.try_into().unwrap()), 0);

        let removed = UnregisterCommand.execute(b"test.command").unwrap();
        assert_eq!(i32::from_le_bytes(removed.try_into().unwrap()), 1);

        assert!(ExecuteCommand.execute(execute_payload).is_err());
    }

    #[test]
    fn test_timer_lifecycle() {
        let timeout = SetTimeout.execute(&250i64.to_le_bytes()).unwrap();
        let timeout_id = i32::from_le_bytes(timeout.try_into().unwrap());
        assert!(timeout_id > 0);

        let interval = SetInterval.execute(&500i64.to_le_bytes()).unwrap();
        let interval_id = i32::from_le_bytes(interval.try_into().unwrap());
        assert!(interval_id > 0);

        let removed = ClearTimer.execute(&timeout_id.to_le_bytes()).unwrap();
        assert_eq!(i32::from_le_bytes(removed.try_into().unwrap()), 1);

        let removed = ClearTimer.execute(&interval_id.to_le_bytes()).unwrap();
        assert_eq!(i32::from_le_bytes(removed.try_into().unwrap()), 1);
    }

    #[test]
    fn test_event_type_custom_range_and_gaps() {
        assert_eq!(EventType::from_u32(100), Some(EventType::Custom));
        assert_eq!(EventType::from_u32(u32::MAX), Some(EventType::Custom));
        assert_eq!(EventType::from_u32(50), None);
        let names: Vec<_> = (0..=14)
            .map(|v| EventType::from_u32(v).unwrap().name())
            .collect();
        assert_eq!(names[5], "resize");
        assert_eq!(names[14], "config_changed");
        assert_eq!(EventType::Custom.name(), "custom");
    }

    #[test]
    fn test_invalid_params_are_rejected() {
        let invalid = Err(HostFunctionError::InvalidParams);
        assert_eq!(SubscribeEvent.execute(&[0; 7]), invalid);
        // tipo de evento inexistente
        let mut p = 50i32.to_le_bytes().to_vec();
        p.extend(0i32.to_le_bytes());
        assert_eq!(SubscribeEvent.execute(&p), invalid);
        assert_eq!(EmitEvent.execute(&[0; 3]), invalid);
        assert_eq!(EmitEvent.execute(&50i32.to_le_bytes()), invalid);

        // atrasos inválidos
        assert_eq!(SetTimeout.execute(&(-1i64).to_le_bytes()), invalid);
        assert_eq!(SetInterval.execute(&0i64.to_le_bytes()), invalid);
        assert_eq!(SetTimeout.execute(&[1, 2]), invalid);
        // timer desconhecido
        let out = ClearTimer.execute(&i32::MAX.to_le_bytes()).unwrap();
        assert_eq!(i32::from_le_bytes(out.try_into().unwrap()), 0);
    }
}
