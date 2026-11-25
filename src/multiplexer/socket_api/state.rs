//! State API - Query e subscrição de estado via socket

use super::types::*;
use super::{ApiCapabilities, ApiError, ApiResult, API_VERSION};
use crate::config::Config;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, trace};

/// ID de subscrição
pub type SubscriptionId = String;

/// Callback de subscrição
pub type StateCallback = Box<dyn Fn(NotificationEvent) + Send + Sync>;

/// Estado interno
#[derive(Debug, Clone)]
struct TabStateInfo {
    id: TabId,
    title: String,
    is_active: bool,
}

/// API de estado
pub struct StateApi {
    config: Arc<RwLock<Config>>,
    tabs: Arc<RwLock<HashMap<TabId, TabStateInfo>>>,
    active_tab: Arc<RwLock<Option<TabId>>>,
    subscriptions: Arc<Mutex<HashMap<SubscriptionId, (StateQueryType, StateCallback)>>>,
    next_subscription_id: Arc<Mutex<u64>>,
}

impl StateApi {
    /// Cria uma nova instância da API
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        let mut tabs = HashMap::new();
        tabs.insert(
            1,
            TabStateInfo {
                id: 1 as TabId,
                title: "bash".to_string(),
                is_active: true,
            },
        );

        Self {
            config,
            tabs: Arc::new(RwLock::new(tabs)),
            active_tab: Arc::new(RwLock::new(Some(1))),
            subscriptions: Arc::new(Mutex::new(HashMap::new())),
            next_subscription_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Retorna informações sobre a API
    pub fn get_api_info(&self) -> ApiResult<Value> {
        let caps = ApiCapabilities::current();
        serde_json::to_value(caps).map_err(|e| ApiError::InternalError(e.to_string()))
    }

    /// Retorna o estado atual do terminal
    pub fn get_state(&self) -> ApiResult<TerminalState> {
        let tabs = self.tabs.read();
        let config = self.config.read();
        let active = self.active_tab.read();

        let tabs_list: Vec<TabInfo> = tabs
            .values()
            .map(|tab| TabInfo {
                id: tab.id,
                index: tab.id as usize - 1,
                title: tab.title.clone(),
                is_active: tab.is_active,
                working_dir: None,
                process: None,
                pid: None,
            })
            .collect();

        let active_tab = active.unwrap_or(0);

        let workspace = WorkspaceInfo {
            id: "default".to_string(),
            name: "Default".to_string(),
            is_current: true,
            tab_count: tabs_list.len(),
            working_dir: None,
        };

        // Converte config para HashMap
        let mut config_map = HashMap::new();
        config_map.insert("font_size".to_string(), serde_json::json!(config.font.size));
        config_map.insert(
            "font_family".to_string(),
            serde_json::json!(config.font.family),
        );
        config_map.insert("theme".to_string(), serde_json::json!(config.colors.name));

        let state = TerminalState {
            version: API_VERSION.to_string(),
            workspace,
            tabs: tabs_list,
            panes: Vec::new(),
            active_tab,
            active_pane: 0,
            terminal_size: TerminalSize {
                cols: 80,
                rows: 24,
                width_pixels: None,
                height_pixels: None,
            },
            config: config_map,
        };

        debug!("Retrieved terminal state: {} tabs", state.tabs.len());
        Ok(state)
    }

    /// Realiza uma query de estado específica
    pub fn query(&self, query_type: StateQueryType) -> ApiResult<StateQueryResult> {
        let tabs = self.tabs.read();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let data = match &query_type {
            StateQueryType::Tabs => {
                let tabs_list: Vec<TabInfo> = tabs
                    .values()
                    .map(|tab| TabInfo {
                        id: tab.id,
                        index: tab.id as usize - 1,
                        title: tab.title.clone(),
                        is_active: tab.is_active,
                        working_dir: None,
                        process: None,
                        pid: None,
                    })
                    .collect();
                serde_json::to_value(tabs_list)
                    .map_err(|e| ApiError::InternalError(e.to_string()))?
            }
            StateQueryType::Panes => {
                serde_json::json!([])
            }
            StateQueryType::Workspace => {
                serde_json::json!({
                    "id": "default",
                    "name": "Default",
                    "tab_count": tabs.len(),
                    "working_dir": null,
                })
            }
            StateQueryType::Selection => {
                serde_json::json!({
                    "has_selection": false,
                    "text": null,
                })
            }
            StateQueryType::Scrollback => {
                serde_json::json!({
                    "scrollback_lines": 0,
                    "visible_lines": 24,
                    "scroll_position": 0,
                })
            }
            StateQueryType::Config { key } => {
                let config = self.config.read();

                if let Some(k) = key {
                    match k.as_str() {
                        "font.size" => serde_json::json!(config.font.size),
                        "font.family" => serde_json::json!(config.font.family),
                        "theme" => serde_json::json!(config.colors.name),
                        "window.opacity" => serde_json::json!(config.window.opacity),
                        "terminal.shell" => serde_json::json!(&config.terminal.shell),
                        _ => serde_json::json!({"error": "unknown key"}),
                    }
                } else {
                    // Retorna config completa
                    serde_json::json!({
                        "font": {
                            "size": config.font.size,
                            "family": config.font.family,
                        },
                        "theme": config.colors.name,
                        "window": {
                            "opacity": config.window.opacity,
                        },
                        "terminal": {
                            "shell": config.terminal.shell,
                        },
                    })
                }
            }
            StateQueryType::All => {
                // Retorna estado completo como JSON
                let state = self.get_state()?;
                serde_json::to_value(state).map_err(|e| ApiError::InternalError(e.to_string()))?
            }
        };

        drop(tabs);

        let result = StateQueryResult {
            query_type: query_type.clone(),
            data,
            timestamp,
        };

        trace!("State query completed: {:?}", query_type);
        Ok(result)
    }

    /// Subscreve a mudanças de estado
    pub fn subscribe(
        &self,
        query_type: StateQueryType,
        callback: StateCallback,
    ) -> ApiResult<SubscriptionId> {
        let mut next_id = self.next_subscription_id.lock().map_err(|_| {
            ApiError::InternalError("Failed to acquire subscription id lock".to_string())
        })?;

        let id = format!("sub_{}", *next_id);
        *next_id += 1;

        let mut subs = self.subscriptions.lock().map_err(|_| {
            ApiError::InternalError("Failed to acquire subscriptions lock".to_string())
        })?;

        subs.insert(id.clone(), (query_type, callback));

        debug!("Created subscription: {}", id);
        Ok(id)
    }

    /// Cancela uma subscrição
    pub fn unsubscribe(&self, id: &str) -> ApiResult<()> {
        let mut subs = self.subscriptions.lock().map_err(|_| {
            ApiError::InternalError("Failed to acquire subscriptions lock".to_string())
        })?;

        if subs.remove(id).is_none() {
            return Err(ApiError::NotFound(format!(
                "Subscription not found: {}",
                id
            )));
        }

        debug!("Cancelled subscription: {}", id);
        Ok(())
    }

    /// Lista subscrições ativas
    pub fn list_subscriptions(&self) -> ApiResult<Vec<(SubscriptionId, StateQueryType)>> {
        let subs = self.subscriptions.lock().map_err(|_| {
            ApiError::InternalError("Failed to acquire subscriptions lock".to_string())
        })?;

        let list: Vec<_> = subs
            .iter()
            .map(|(id, (qt, _))| (id.clone(), qt.clone()))
            .collect();

        Ok(list)
    }

    /// Notifica todos os subscribers sobre uma mudança
    pub fn notify(&self, event_type: &str, data: Value) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let event = NotificationEvent {
            event_type: event_type.to_string(),
            data,
            timestamp,
        };

        let subs = match self.subscriptions.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        for (query_type, callback) in subs.values() {
            // Verifica se o evento é relevante para esta subscrição
            // "all" (o tipo padrão do state.subscribe) recebe qualquer evento.
            let should_notify = matches!(query_type, StateQueryType::All)
                || matches!(
                    (event_type, query_type),
                    (
                        "tab.created" | "tab.closed" | "tab.switched",
                        StateQueryType::Tabs
                    ) | (
                        "pane.created" | "pane.closed" | "pane.focused",
                        StateQueryType::Panes
                    ) | ("config.changed", StateQueryType::Config { .. })
                        | ("selection.changed", StateQueryType::Selection)
                        | ("scrollback.changed", StateQueryType::Scrollback)
                        | (_, StateQueryType::All)
                );

            if should_notify {
                callback(event.clone());
            }
        }
    }

    /// Gera um dump completo do estado (para debug)
    pub fn dump(&self) -> ApiResult<Value> {
        let tabs = self.tabs.read();
        let config = self.config.read();
        let active = self.active_tab.read();

        let dump = serde_json::json!({
            "version": API_VERSION,
            "api_capabilities": ApiCapabilities::current(),
            "tabs": {
                "count": tabs.len(),
                "active_id": *active,
                "tabs": tabs.values().map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "title": t.title,
                        "is_active": t.is_active,
                    })
                }).collect::<Vec<_>>(),
            },
            "config": {
                "font": {
                    "family": config.font.family,
                    "size": config.font.size,
                },
                "theme": config.colors.name,
                "window": {
                    "opacity": config.window.opacity,
                },
            },
            "subscriptions": {
                "count": self.subscriptions.lock().map(|s| s.len()).unwrap_or(0),
            },
        });

        debug!("Generated state dump");
        Ok(dump)
    }

    /// Handler para método JSON-RPC
    pub fn handle_method(&self, method: &str, params: Value) -> ApiResult<Value> {
        match method {
            "state.get" => {
                let state = self.get_state()?;
                serde_json::to_value(state).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "state.query" => {
                let query_type: StateQueryType = serde_json::from_value(
                    params
                        .get("type")
                        .cloned()
                        .unwrap_or(Value::String("all".to_string())),
                )
                .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                let result = self.query(query_type)?;
                serde_json::to_value(result).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "state.subscribe" => {
                let query_type: StateQueryType = serde_json::from_value(
                    params
                        .get("type")
                        .cloned()
                        .unwrap_or(Value::String("all".to_string())),
                )
                .map_err(|e| ApiError::InvalidParams(e.to_string()))?;

                // Cria um callback dummy (em implementação real, seria conectado ao WebSocket/Stream)
                let callback: StateCallback = Box::new(move |event| {
                    trace!("State notification: {:?}", event);
                });

                let id = self.subscribe(query_type, callback)?;
                serde_json::to_value(id).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "state.unsubscribe" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                self.unsubscribe(id)?;
                Ok(Value::Null)
            }
            "state.list_subscriptions" => {
                let subs = self.list_subscriptions()?;
                serde_json::to_value(subs).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "state.dump" => {
                let dump = self.dump()?;
                Ok(dump)
            }
            "state.api_info" => self.get_api_info(),
            _ => Err(ApiError::MethodNotFound(method.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_api() -> StateApi {
        StateApi::new(Arc::new(RwLock::new(Config::default())))
    }

    #[test]
    fn test_get_state() {
        let api = create_test_api();
        let state = api.get_state().unwrap();

        assert_eq!(state.version, API_VERSION);
        assert!(!state.workspace.id.is_empty());
    }

    #[test]
    fn test_query_tabs() {
        let api = create_test_api();
        let result = api.query(StateQueryType::Tabs).unwrap();

        assert!(matches!(result.query_type, StateQueryType::Tabs));
        assert!(result.data.is_array());
        assert!(result.timestamp > 0);
    }

    #[test]
    fn test_query_workspace() {
        let api = create_test_api();
        let result = api.query(StateQueryType::Workspace).unwrap();

        assert!(matches!(result.query_type, StateQueryType::Workspace));
        assert!(result.data.get("id").is_some());
        assert!(result.data.get("tab_count").is_some());
    }

    #[test]
    fn test_query_config() {
        let api = create_test_api();

        // Query completa
        let result = api.query(StateQueryType::Config { key: None }).unwrap();
        assert!(result.data.get("font").is_some());

        // Query específica
        let result = api
            .query(StateQueryType::Config {
                key: Some("font.size".to_string()),
            })
            .unwrap();
        assert!(result.data.is_number());
    }

    #[test]
    fn test_subscribe_unsubscribe() {
        let api = create_test_api();

        let callback: StateCallback = Box::new(|_| {});
        let id = api.subscribe(StateQueryType::Tabs, callback).unwrap();

        assert!(!id.is_empty());

        let subs = api.list_subscriptions().unwrap();
        assert_eq!(subs.len(), 1);

        api.unsubscribe(&id).unwrap();

        let subs = api.list_subscriptions().unwrap();
        assert!(subs.is_empty());
    }

    #[test]
    fn test_dump() {
        let api = create_test_api();
        let dump = api.dump().unwrap();

        assert!(dump.get("version").is_some());
        assert!(dump.get("tabs").is_some());
        assert!(dump.get("config").is_some());
    }

    #[test]
    fn test_subscription_not_found() {
        let api = create_test_api();
        let result = api.unsubscribe("nonexistent");

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApiError::NotFound(_)));
    }

    type Events = Arc<Mutex<Vec<String>>>;

    fn recorder(events: &Events, label: &'static str) -> StateCallback {
        let events = Arc::clone(events);
        Box::new(move |event| {
            events
                .lock()
                .unwrap()
                .push(format!("{}:{}", label, event.event_type));
        })
    }

    #[test]
    fn test_api_info_and_simple_queries() {
        let api = create_test_api();
        let info = api.get_api_info().unwrap();
        assert_eq!(info["version"], API_VERSION);

        assert_eq!(
            api.query(StateQueryType::Panes).unwrap().data,
            serde_json::json!([])
        );
        let selection = api.query(StateQueryType::Selection).unwrap();
        assert_eq!(selection.data["has_selection"], false);
        let scrollback = api.query(StateQueryType::Scrollback).unwrap();
        assert_eq!(scrollback.data["visible_lines"], 24);
        assert!(scrollback.timestamp > 0);

        let all = api.query(StateQueryType::All).unwrap();
        assert_eq!(all.data["version"], API_VERSION);
        assert_eq!(all.data["tabs"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_query_config_keys() {
        let config = Config::default();
        let api = StateApi::new(Arc::new(RwLock::new(config.clone())));
        let value = |key: &str| {
            api.query(StateQueryType::Config {
                key: Some(key.to_string()),
            })
            .unwrap()
            .data
        };
        assert_eq!(value("font.size"), serde_json::json!(config.font.size));
        assert_eq!(value("font.family"), serde_json::json!(config.font.family));
        assert_eq!(value("theme"), serde_json::json!(config.colors.name));
        assert_eq!(
            value("window.opacity"),
            serde_json::json!(config.window.opacity)
        );
        assert_eq!(
            value("terminal.shell"),
            serde_json::json!(config.terminal.shell)
        );
        assert_eq!(value("nada"), serde_json::json!({"error": "unknown key"}));
    }

    /// Regressão: subscrições "all" nunca recebiam notificações.
    #[test]
    fn test_notify_routes_events_to_matching_subscriptions() {
        let api = create_test_api();
        let events: Events = Arc::new(Mutex::new(Vec::new()));
        api.subscribe(StateQueryType::All, recorder(&events, "all"))
            .unwrap();
        api.subscribe(StateQueryType::Tabs, recorder(&events, "tabs"))
            .unwrap();
        api.subscribe(StateQueryType::Panes, recorder(&events, "panes"))
            .unwrap();
        api.subscribe(
            StateQueryType::Config { key: None },
            recorder(&events, "config"),
        )
        .unwrap();
        api.subscribe(StateQueryType::Selection, recorder(&events, "sel"))
            .unwrap();
        api.subscribe(StateQueryType::Scrollback, recorder(&events, "scroll"))
            .unwrap();
        api.subscribe(StateQueryType::Workspace, recorder(&events, "ws"))
            .unwrap();

        for event in [
            "tab.created",
            "pane.focused",
            "config.changed",
            "selection.changed",
            "scrollback.changed",
            "other.event",
        ] {
            api.notify(event, serde_json::json!({}));
        }

        let mut got = events.lock().unwrap().clone();
        got.sort();
        let mut expected = vec![
            "all:tab.created",
            "all:pane.focused",
            "all:config.changed",
            "all:selection.changed",
            "all:scrollback.changed",
            "all:other.event",
            "tabs:tab.created",
            "panes:pane.focused",
            "config:config.changed",
            "sel:selection.changed",
            "scroll:scrollback.changed",
        ];
        expected.sort();
        assert_eq!(got, expected);
    }

    #[test]
    fn test_handle_method_routes() {
        let api = create_test_api();
        let call = |method: &str, params: Value| api.handle_method(method, params);

        assert_eq!(
            call("state.get", Value::Null).unwrap()["version"],
            API_VERSION
        );
        let tabs = call("state.query", serde_json::json!({"type": "tabs"})).unwrap();
        assert_eq!(tabs["data"].as_array().unwrap().len(), 1);
        let all = call("state.query", Value::Null).unwrap();
        assert_eq!(all["data"]["version"], API_VERSION);
        assert!(matches!(
            call("state.query", serde_json::json!({"type": "nada"})),
            Err(ApiError::InvalidParams(_))
        ));

        let id = call("state.subscribe", serde_json::json!({"type": "panes"})).unwrap();
        let id = id.as_str().unwrap().to_string();
        call("state.subscribe", Value::Null).unwrap();
        // O callback do subscribe via JSON só registra em log; não pode falhar
        api.notify("pane.created", serde_json::json!({}));
        assert!(matches!(
            call("state.subscribe", serde_json::json!({"type": 3})),
            Err(ApiError::InvalidParams(_))
        ));

        let subs = call("state.list_subscriptions", Value::Null).unwrap();
        assert_eq!(subs.as_array().unwrap().len(), 2);
        call("state.unsubscribe", serde_json::json!({"id": id})).unwrap();
        assert!(matches!(
            call("state.unsubscribe", serde_json::json!({})),
            Err(ApiError::InvalidParams(_))
        ));

        let dump = call("state.dump", Value::Null).unwrap();
        assert_eq!(dump["subscriptions"]["count"], 1);
        assert_eq!(
            call("state.api_info", Value::Null).unwrap()["version"],
            API_VERSION
        );

        match call("state.explode", Value::Null) {
            Err(ApiError::MethodNotFound(method)) => assert_eq!(method, "state.explode"),
            other => panic!("esperava MethodNotFound, veio {:?}", other),
        }
    }
}
