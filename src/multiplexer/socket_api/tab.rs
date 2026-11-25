//! Tab API - Gerenciamento de tabs via socket

use super::types::*;
use super::{ApiError, ApiResult};
// TabId vem de super::types (crate::types) que é u64
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

/// Estado interno de uma tab
#[derive(Debug, Clone)]
struct TabState {
    id: TabId,
    index: usize,
    title: String,
    is_active: bool,
    working_dir: Option<String>,
}

/// API de tabs
pub struct TabApi {
    tabs: Arc<RwLock<HashMap<TabId, TabState>>>,
    next_id: Arc<RwLock<TabId>>,
    active_id: Arc<RwLock<Option<TabId>>>,
}

impl TabApi {
    /// Cria uma nova instância da API
    pub fn new() -> Self {
        let mut tabs = HashMap::new();
        let id: TabId = 1;
        tabs.insert(
            id,
            TabState {
                id,
                index: 0,
                title: "bash".to_string(),
                is_active: true,
                working_dir: None,
            },
        );

        Self {
            tabs: Arc::new(RwLock::new(tabs)),
            next_id: Arc::new(RwLock::new(2 as TabId)),
            active_id: Arc::new(RwLock::new(Some(1 as TabId))),
        }
    }

    /// Lista todas as tabs
    pub fn list(&self) -> ApiResult<Vec<TabInfo>> {
        let tabs = self.tabs.read();

        let mut list: Vec<TabInfo> = tabs
            .values()
            .map(|tab| TabInfo {
                id: tab.id,
                index: tab.index,
                title: tab.title.clone(),
                is_active: tab.is_active,
                working_dir: tab.working_dir.clone(),
                process: None,
                pid: None,
            })
            .collect();

        list.sort_by_key(|t| t.index);

        debug!("Listed {} tabs", list.len());
        Ok(list)
    }

    /// Retorna informações de uma tab específica
    pub fn get(&self, id: TabId) -> ApiResult<TabInfo> {
        let tabs = self.tabs.read();

        tabs.get(&id)
            .map(|tab| TabInfo {
                id: tab.id,
                index: tab.index,
                title: tab.title.clone(),
                is_active: tab.is_active,
                working_dir: tab.working_dir.clone(),
                process: None,
                pid: None,
            })
            .ok_or_else(|| ApiError::NotFound(format!("Tab not found: {}", id)))
    }

    /// Retorna informações da tab ativa
    pub fn get_active(&self) -> ApiResult<TabInfo> {
        let active = self.active_id.read();

        match *active {
            Some(id) => self.get(id),
            None => Err(ApiError::NotFound("No active tab".to_string())),
        }
    }

    /// Cria uma nova tab
    pub fn create(&self, params: CreateTabParams) -> ApiResult<TabInfo> {
        let mut tabs = self.tabs.write();
        let mut next_id = self.next_id.write();
        let mut active = self.active_id.write();

        let id: TabId = *next_id;
        *next_id += 1;

        let index = tabs.len();
        let title = params.title.unwrap_or_else(|| "bash".to_string());
        let working_dir = params.working_dir;

        let state = TabState {
            id,
            index,
            title: title.clone(),
            is_active: true,
            working_dir: working_dir.clone(),
        };

        for tab in tabs.values_mut() {
            tab.is_active = false;
        }
        tabs.insert(id, state);
        *active = Some(id);

        let info = TabInfo {
            id,
            index,
            title,
            is_active: true,
            working_dir,
            process: None,
            pid: None,
        };

        debug!(
            "Created tab: {} ({}) at index {}",
            info.title, info.id, info.index
        );
        Ok(info)
    }

    /// Fecha uma tab
    pub fn close(&self, id: TabId, _force: bool) -> ApiResult<()> {
        let mut tabs = self.tabs.write();
        let mut active = self.active_id.write();

        if tabs.remove(&id).is_none() {
            return Err(ApiError::NotFound(format!("Tab not found: {}", id)));
        }

        // Se fechou a tab ativa, atualiza
        if *active == Some(id) {
            *active = tabs.keys().next().copied();
        }

        for tab in tabs.values_mut() {
            tab.is_active = Some(tab.id) == *active;
        }

        // Reordena índices
        let mut sorted_tabs: Vec<_> = tabs.values_mut().collect();
        sorted_tabs.sort_by_key(|t| t.index);
        for (new_index, tab) in sorted_tabs.iter_mut().enumerate() {
            tab.index = new_index;
        }

        debug!("Closed tab: {}", id);
        Ok(())
    }

    /// Fecha a tab ativa
    pub fn close_active(&self, force: bool) -> ApiResult<()> {
        let active = self.active_id.read();

        match *active {
            Some(id) => {
                drop(active);
                self.close(id, force)
            }
            None => Err(ApiError::NotFound("No active tab".to_string())),
        }
    }

    /// Muda para uma tab específica (por índice)
    pub fn switch_by_index(&self, index: usize) -> ApiResult<TabInfo> {
        let tabs = self.tabs.read();

        let target_id = tabs.values().find(|t| t.index == index).map(|t| t.id);

        drop(tabs);

        match target_id {
            Some(id) => self.switch(id),
            None => Err(ApiError::NotFound(format!(
                "Tab index not found: {}",
                index
            ))),
        }
    }

    /// Muda para uma tab específica (por ID)
    pub fn switch(&self, id: TabId) -> ApiResult<TabInfo> {
        let mut tabs = self.tabs.write();

        if !tabs.contains_key(&id) {
            return Err(ApiError::NotFound(format!("Tab not found: {}", id)));
        }

        for tab in tabs.values_mut() {
            tab.is_active = tab.id == id;
        }
        drop(tabs);

        let mut active = self.active_id.write();
        *active = Some(id);
        drop(active);

        self.get(id)
    }

    /// Próxima tab
    pub fn next(&self) -> ApiResult<TabInfo> {
        let tabs = self.tabs.read();
        let active = self.active_id.read();
        let count = tabs.len();

        if count == 0 {
            return Err(ApiError::NotFound("No tabs".to_string()));
        }

        let current_index = active
            .and_then(|id| tabs.get(&id))
            .map(|t| t.index)
            .unwrap_or(0);

        drop(tabs);
        drop(active);

        let next = (current_index + 1) % count;
        self.switch_by_index(next)
    }

    /// Tab anterior
    pub fn previous(&self) -> ApiResult<TabInfo> {
        let tabs = self.tabs.read();
        let active = self.active_id.read();
        let count = tabs.len();

        if count == 0 {
            return Err(ApiError::NotFound("No tabs".to_string()));
        }

        let current_index = active
            .and_then(|id| tabs.get(&id))
            .map(|t| t.index)
            .unwrap_or(0);

        drop(tabs);
        drop(active);

        let prev = if current_index == 0 {
            count - 1
        } else {
            current_index - 1
        };
        self.switch_by_index(prev)
    }

    /// Move uma tab para uma nova posição
    pub fn move_tab(&self, from_index: usize, to_index: usize) -> ApiResult<TabInfo> {
        let mut tabs = self.tabs.write();
        let count = tabs.len();

        if from_index >= count {
            return Err(ApiError::NotFound(format!(
                "Source index out of bounds: {} (max: {})",
                from_index,
                count - 1
            )));
        }

        if to_index >= count {
            return Err(ApiError::NotFound(format!(
                "Destination index out of bounds: {} (max: {})",
                to_index,
                count - 1
            )));
        }

        // Encontra o ID da tab no índice de origem
        let from_id = tabs.values().find(|t| t.index == from_index).map(|t| t.id);

        if let Some(id) = from_id {
            if let Some(tab) = tabs.get_mut(&id) {
                tab.index = to_index;
            }

            // Ajusta índices das outras tabs
            for t in tabs.values_mut() {
                if t.id != id {
                    if from_index < to_index && t.index > from_index && t.index <= to_index {
                        t.index -= 1;
                    } else if from_index > to_index && t.index >= to_index && t.index < from_index {
                        t.index += 1;
                    }
                }
            }

            let tab = tabs.get(&id).unwrap();
            let info = TabInfo {
                id: tab.id,
                index: tab.index,
                title: tab.title.clone(),
                is_active: tab.is_active,
                working_dir: tab.working_dir.clone(),
                process: None,
                pid: None,
            };

            return Ok(info);
        }

        Err(ApiError::NotFound(format!(
            "No tab at index {}",
            from_index
        )))
    }

    /// Renomeia uma tab
    pub fn rename(&self, id: TabId, title: String) -> ApiResult<TabInfo> {
        let mut tabs = self.tabs.write();

        let tab = tabs
            .get_mut(&id)
            .ok_or_else(|| ApiError::NotFound(format!("Tab not found: {}", id)))?;

        tab.title = title.clone();

        let info = TabInfo {
            id: tab.id,
            index: tab.index,
            title,
            is_active: tab.is_active,
            working_dir: tab.working_dir.clone(),
            process: None,
            pid: None,
        };

        Ok(info)
    }

    /// Duplica uma tab
    pub fn duplicate(&self, id: TabId) -> ApiResult<TabInfo> {
        let tabs = self.tabs.read();

        let tab = tabs
            .get(&id)
            .cloned()
            .ok_or_else(|| ApiError::NotFound(format!("Tab not found: {}", id)))?;

        drop(tabs);

        self.create(CreateTabParams {
            title: Some(format!("{} (copy)", tab.title)),
            working_dir: tab.working_dir,
            command: None,
        })
    }

    /// Handler para método JSON-RPC
    pub fn handle_method(&self, method: &str, params: Value) -> ApiResult<Value> {
        match method {
            "tab.list" => {
                let tabs = self.list()?;
                serde_json::to_value(tabs).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "tab.get" => {
                if let Some(id) = params.get("id").and_then(|v| v.as_u64()) {
                    let tab = self.get(id as TabId)?;
                    serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
                } else {
                    let tab = self.get_active()?;
                    serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
                }
            }
            "tab.create" => {
                let create_params: CreateTabParams = serde_json::from_value(params)
                    .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                let tab = self.create(create_params)?;
                serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "tab.close" => {
                if let Some(id) = params.get("id").and_then(|v| v.as_u64()) {
                    let force = params
                        .get("force")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    self.close(id as TabId, force)?;
                } else {
                    let force = params
                        .get("force")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    self.close_active(force)?;
                }
                Ok(Value::Null)
            }
            "tab.switch" => {
                if let Some(id) = params.get("id").and_then(|v| v.as_u64()) {
                    let tab = self.switch(id as TabId)?;
                    serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
                } else if let Some(index) = params.get("index").and_then(|v| v.as_u64()) {
                    let tab = self.switch_by_index(index as usize)?;
                    serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
                } else {
                    Err(ApiError::InvalidParams(
                        "Missing 'id' or 'index' parameter".to_string(),
                    ))
                }
            }
            "tab.next" => {
                let tab = self.next()?;
                serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "tab.previous" => {
                let tab = self.previous()?;
                serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "tab.move" => {
                let from = params.get("from").and_then(|v| v.as_u64()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'from' parameter".to_string())
                })?;
                let to = params
                    .get("to")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'to' parameter".to_string()))?;
                let tab = self.move_tab(from as usize, to as usize)?;
                serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "tab.rename" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                let title = params
                    .get("title")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ApiError::InvalidParams("Missing 'title' parameter".to_string())
                    })?;
                let tab = self.rename(id as TabId, title.to_string())?;
                serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "tab.duplicate" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                let tab = self.duplicate(id as TabId)?;
                serde_json::to_value(tab).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            _ => Err(ApiError::MethodNotFound(method.to_string())),
        }
    }
}

impl Default for TabApi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_list() {
        let api = TabApi::new();
        let tabs = api.list().unwrap();

        // A API cria uma tab padrão
        assert!(!tabs.is_empty());
    }

    #[test]
    fn test_tab_create() {
        let api = TabApi::new();

        let initial_count = api.list().unwrap().len();

        let tab = api
            .create(CreateTabParams {
                title: Some("Test".to_string()),
                working_dir: None,
                command: None,
            })
            .unwrap();

        assert_eq!(api.list().unwrap().len(), initial_count + 1);
        assert_eq!(tab.title, "Test");
    }

    #[test]
    fn test_tab_switch() {
        let api = TabApi::new();

        let tab1 = api.create(CreateTabParams::default()).unwrap();
        let _tab2 = api.create(CreateTabParams::default()).unwrap();

        let switched = api.switch(tab1.id).unwrap();
        assert!(switched.is_active);
    }

    #[test]
    fn test_tab_next_previous() {
        let api = TabApi::new();

        api.create(CreateTabParams::default()).unwrap();
        api.create(CreateTabParams::default()).unwrap();
        api.create(CreateTabParams::default()).unwrap();

        let initial = api.get_active().unwrap();

        let next = api.next().unwrap();
        assert_ne!(initial.id, next.id);

        let prev = api.previous().unwrap();
        assert_eq!(initial.id, prev.id);
    }

    #[test]
    fn test_tab_not_found() {
        let api = TabApi::new();
        let result = api.get(99999);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApiError::NotFound(_)));
    }

    fn titled(title: &str) -> CreateTabParams {
        CreateTabParams {
            title: Some(title.to_string()),
            working_dir: Some(format!("/tmp/{}", title)),
            command: None,
        }
    }

    fn titles(api: &TabApi) -> Vec<String> {
        api.list().unwrap().into_iter().map(|t| t.title).collect()
    }

    fn call(api: &TabApi, method: &str, params: Value) -> ApiResult<Value> {
        api.handle_method(method, params)
    }

    #[test]
    fn test_close_reindexes_and_reassigns_active() {
        let api = TabApi::default();
        let a = api.create(titled("a")).unwrap();
        let b = api.create(titled("b")).unwrap();
        assert_eq!(titles(&api), vec!["bash", "a", "b"]);

        // Fechar uma tab que não é a ativa mantém o foco
        api.close(a.id, false).unwrap();
        assert_eq!(titles(&api), vec!["bash", "b"]);
        let list = api.list().unwrap();
        assert_eq!(list.iter().map(|t| t.index).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(api.get_active().unwrap().id, b.id);

        // Fechar a ativa ativa outra tab
        api.close_active(true).unwrap();
        assert_eq!(titles(&api), vec!["bash"]);
        let active = api.get_active().unwrap();
        assert_eq!(active.title, "bash");
        assert!(active.is_active);

        assert!(matches!(api.close(b.id, false), Err(ApiError::NotFound(_))));

        api.close_active(false).unwrap();
        assert!(api.list().unwrap().is_empty());
        assert!(matches!(api.get_active(), Err(ApiError::NotFound(_))));
        assert!(matches!(
            api.close_active(false),
            Err(ApiError::NotFound(_))
        ));
        assert!(matches!(api.next(), Err(ApiError::NotFound(_))));
        assert!(matches!(api.previous(), Err(ApiError::NotFound(_))));
    }

    #[test]
    fn test_switch_by_index_and_errors() {
        let api = TabApi::new();
        let a = api.create(titled("a")).unwrap();
        let first = api.switch_by_index(0).unwrap();
        assert_eq!(first.title, "bash");
        assert!(first.is_active);
        assert!(!api.get(a.id).unwrap().is_active);
        assert!(matches!(api.switch_by_index(9), Err(ApiError::NotFound(_))));
        assert!(matches!(api.switch(999), Err(ApiError::NotFound(_))));
    }

    #[test]
    fn test_previous_wraps_from_first_tab() {
        let api = TabApi::new();
        api.create(titled("a")).unwrap();
        api.create(titled("b")).unwrap();
        api.switch_by_index(0).unwrap();
        assert_eq!(api.previous().unwrap().title, "b");
        assert_eq!(api.next().unwrap().title, "bash");
    }

    #[test]
    fn test_move_tab_forward_and_backward() {
        let api = TabApi::new();
        api.create(titled("a")).unwrap();
        api.create(titled("b")).unwrap();
        api.create(titled("c")).unwrap();

        let moved = api.move_tab(0, 2).unwrap();
        assert_eq!((moved.title.as_str(), moved.index), ("bash", 2));
        assert_eq!(titles(&api), vec!["a", "b", "bash", "c"]);

        let moved = api.move_tab(3, 0).unwrap();
        assert_eq!(moved.title, "c");
        assert_eq!(titles(&api), vec!["c", "a", "b", "bash"]);

        // Mover para o mesmo lugar não muda nada
        api.move_tab(1, 1).unwrap();
        assert_eq!(titles(&api), vec!["c", "a", "b", "bash"]);

        match api.move_tab(4, 0) {
            Err(ApiError::NotFound(message)) => {
                assert_eq!(message, "Source index out of bounds: 4 (max: 3)")
            }
            other => panic!("esperava erro, veio {:?}", other),
        }
        match api.move_tab(0, 7) {
            Err(ApiError::NotFound(message)) => {
                assert_eq!(message, "Destination index out of bounds: 7 (max: 3)")
            }
            other => panic!("esperava erro, veio {:?}", other),
        }
    }

    #[test]
    fn test_rename_and_duplicate() {
        let api = TabApi::new();
        let a = api.create(titled("a")).unwrap();
        let renamed = api.rename(a.id, "novo".to_string()).unwrap();
        assert_eq!(renamed.title, "novo");
        assert_eq!(renamed.working_dir.as_deref(), Some("/tmp/a"));
        assert!(matches!(
            api.rename(999, "x".to_string()),
            Err(ApiError::NotFound(_))
        ));

        let copy = api.duplicate(a.id).unwrap();
        assert_eq!(copy.title, "novo (copy)");
        assert_eq!(copy.working_dir.as_deref(), Some("/tmp/a"));
        assert_ne!(copy.id, a.id);
        assert!(copy.is_active);
        assert!(matches!(api.duplicate(999), Err(ApiError::NotFound(_))));
    }

    #[test]
    fn test_handle_method_routes() {
        let api = TabApi::new();
        let created = call(&api, "tab.create", serde_json::json!({"title": "x"})).unwrap();
        let id = created["id"].as_u64().unwrap();

        assert_eq!(
            call(&api, "tab.get", serde_json::json!({"id": id})).unwrap()["title"],
            "x"
        );
        assert_eq!(
            call(&api, "tab.get", serde_json::json!({})).unwrap()["id"],
            id
        );

        assert_eq!(
            call(&api, "tab.switch", serde_json::json!({"index": 0})).unwrap()["title"],
            "bash"
        );
        assert_eq!(
            call(&api, "tab.switch", serde_json::json!({"id": id})).unwrap()["id"],
            id
        );
        assert!(matches!(
            call(&api, "tab.switch", serde_json::json!({})),
            Err(ApiError::InvalidParams(_))
        ));

        assert_eq!(
            call(&api, "tab.next", Value::Null).unwrap()["title"],
            "bash"
        );
        assert_eq!(call(&api, "tab.previous", Value::Null).unwrap()["id"], id);

        let moved = call(&api, "tab.move", serde_json::json!({"from": 1, "to": 0})).unwrap();
        assert_eq!(moved["index"], 0);
        assert!(matches!(
            call(&api, "tab.move", serde_json::json!({"to": 0})),
            Err(ApiError::InvalidParams(_))
        ));
        assert!(matches!(
            call(&api, "tab.move", serde_json::json!({"from": 0})),
            Err(ApiError::InvalidParams(_))
        ));

        assert_eq!(
            call(
                &api,
                "tab.rename",
                serde_json::json!({"id": id, "title": "y"})
            )
            .unwrap()["title"],
            "y"
        );
        assert!(matches!(
            call(&api, "tab.rename", serde_json::json!({"title": "y"})),
            Err(ApiError::InvalidParams(_))
        ));
        assert!(matches!(
            call(&api, "tab.rename", serde_json::json!({"id": id})),
            Err(ApiError::InvalidParams(_))
        ));

        let copy = call(&api, "tab.duplicate", serde_json::json!({"id": id})).unwrap();
        assert_eq!(copy["title"], "y (copy)");
        assert!(matches!(
            call(&api, "tab.duplicate", serde_json::json!({})),
            Err(ApiError::InvalidParams(_))
        ));

        call(
            &api,
            "tab.close",
            serde_json::json!({"id": copy["id"], "force": true}),
        )
        .unwrap();
        call(&api, "tab.close", serde_json::json!({})).unwrap();
        assert_eq!(api.list().unwrap().len(), 1);

        match call(&api, "tab.explode", Value::Null) {
            Err(ApiError::MethodNotFound(method)) => assert_eq!(method, "tab.explode"),
            other => panic!("esperava MethodNotFound, veio {:?}", other),
        }
    }
}
