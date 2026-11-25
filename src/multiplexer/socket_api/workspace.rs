//! Workspace API - Gerenciamento de workspaces via socket

use super::types::*;
use super::{ApiError, ApiResult};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, warn};

/// API de workspaces
pub struct WorkspaceApi {
    workspaces: Arc<RwLock<HashMap<WorkspaceId, WorkspaceInfo>>>,
    current_workspace: Arc<RwLock<WorkspaceId>>,
}

impl WorkspaceApi {
    /// Cria uma nova instância da API
    pub fn new() -> Self {
        let mut workspaces = HashMap::new();
        let default_id = "default".to_string();

        // Cria workspace padrão
        workspaces.insert(
            default_id.clone(),
            WorkspaceInfo {
                id: default_id.clone(),
                name: "Default".to_string(),
                is_current: true,
                tab_count: 0,
                working_dir: None,
            },
        );

        Self {
            workspaces: Arc::new(RwLock::new(workspaces)),
            current_workspace: Arc::new(RwLock::new(default_id)),
        }
    }

    /// Lista todos os workspaces
    pub fn list(&self) -> ApiResult<Vec<WorkspaceInfo>> {
        let workspaces = self
            .workspaces
            .read()
            .map_err(|_| ApiError::InternalError("Failed to acquire read lock".to_string()))?;

        let current = self.current_workspace.read().map_err(|_| {
            ApiError::InternalError("Failed to acquire current workspace lock".to_string())
        })?;

        let mut list: Vec<WorkspaceInfo> = workspaces
            .values()
            .cloned()
            .map(|mut w| {
                w.is_current = w.id == *current;
                w
            })
            .collect();

        // Ordena por nome
        list.sort_by(|a, b| a.name.cmp(&b.name));

        debug!("Listed {} workspaces", list.len());
        Ok(list)
    }

    /// ID do workspace atual (o guard é liberado antes de retornar)
    fn current_id(&self) -> ApiResult<String> {
        self.current_workspace
            .read()
            .map(|current| current.clone())
            .map_err(|_| {
                ApiError::InternalError("Failed to acquire current workspace lock".to_string())
            })
    }

    /// Ajusta `is_current` de acordo com o workspace atual.
    ///
    /// O valor guardado no mapa não é atualizado no `switch`, então as
    /// respostas precisam calculá-lo na hora (como já fazia o `list`).
    fn with_current_flag(&self, mut info: WorkspaceInfo) -> ApiResult<WorkspaceInfo> {
        info.is_current = info.id == self.current_id()?;
        Ok(info)
    }

    /// Retorna o workspace atual
    pub fn current(&self) -> ApiResult<WorkspaceInfo> {
        let current = self.current_id()?;
        self.get(&current).map_err(|error| match error {
            ApiError::NotFound(_) => ApiError::NotFound("Current workspace not found".to_string()),
            other => other,
        })
    }

    /// Retorna um workspace pelo ID
    pub fn get(&self, id: &str) -> ApiResult<WorkspaceInfo> {
        let info = {
            let workspaces = self
                .workspaces
                .read()
                .map_err(|_| ApiError::InternalError("Failed to acquire read lock".to_string()))?;

            workspaces
                .get(id)
                .cloned()
                .ok_or_else(|| ApiError::NotFound(format!("Workspace not found: {}", id)))?
        };
        self.with_current_flag(info)
    }

    /// Cria um novo workspace
    pub fn create(&self, name: Option<String>) -> ApiResult<WorkspaceInfo> {
        let id = format!("ws_{}", uuid::Uuid::new_v4().simple());
        let name = name.unwrap_or_else(|| "New Workspace".to_string());

        let info = WorkspaceInfo {
            id: id.clone(),
            name,
            is_current: false,
            tab_count: 0,
            working_dir: None,
        };

        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        workspaces.insert(id, info.clone());

        debug!("Created workspace: {} ({})", info.name, info.id);
        Ok(info)
    }

    /// Muda para outro workspace
    pub fn switch(&self, id: &str) -> ApiResult<WorkspaceInfo> {
        {
            let workspaces = self
                .workspaces
                .read()
                .map_err(|_| ApiError::InternalError("Failed to acquire read lock".to_string()))?;

            if !workspaces.contains_key(id) {
                return Err(ApiError::NotFound(format!("Workspace not found: {}", id)));
            }
        }

        // Nenhum outro lock é mantido aqui: `close` pega os dois locks na
        // ordem workspaces -> current, e segurar current esperando
        // workspaces poderia travar.
        *self.current_workspace.write().map_err(|_| {
            ApiError::InternalError("Failed to acquire current workspace lock".to_string())
        })? = id.to_string();

        let info = self.get(id)?;
        debug!("Switched to workspace: {} ({})", info.name, info.id);
        Ok(info)
    }

    /// Renomeia um workspace
    pub fn rename(&self, id: &str, new_name: String) -> ApiResult<WorkspaceInfo> {
        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        let workspace = workspaces
            .get_mut(id)
            .ok_or_else(|| ApiError::NotFound(format!("Workspace not found: {}", id)))?;

        workspace.name = new_name.clone();
        let info = workspace.clone();
        drop(workspaces);

        debug!("Renamed workspace {} to: {}", id, new_name);
        self.with_current_flag(info)
    }

    /// Fecha um workspace
    pub fn close(&self, id: &str, force: bool) -> ApiResult<()> {
        // Não permite fechar o workspace padrão
        if id == "default" && !force {
            return Err(ApiError::InvalidState(
                "Cannot close default workspace. Use force=true to override.".to_string(),
            ));
        }

        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        let workspace = workspaces
            .get(id)
            .ok_or_else(|| ApiError::NotFound(format!("Workspace not found: {}", id)))?;

        // Verifica se há tabs no workspace
        if workspace.tab_count > 0 && !force {
            return Err(ApiError::InvalidState(format!(
                "Workspace has {} tabs. Use force=true to close anyway.",
                workspace.tab_count
            )));
        }

        workspaces.remove(id);

        // Se fechou o workspace atual, muda para o default
        let mut current = self.current_workspace.write().map_err(|_| {
            ApiError::InternalError("Failed to acquire current workspace lock".to_string())
        })?;

        if *current == id {
            *current = "default".to_string();
            warn!("Closed current workspace, switched to default");
        }

        debug!("Closed workspace: {}", id);
        Ok(())
    }

    /// Atualiza a contagem de tabs de um workspace
    pub fn update_tab_count(&self, id: &str, count: usize) -> ApiResult<()> {
        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        if let Some(workspace) = workspaces.get_mut(id) {
            workspace.tab_count = count;
        }

        Ok(())
    }

    /// Define o diretório de trabalho de um workspace
    pub fn set_working_dir(&self, id: &str, dir: String) -> ApiResult<()> {
        let mut workspaces = self
            .workspaces
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        if let Some(workspace) = workspaces.get_mut(id) {
            workspace.working_dir = Some(dir);
        }

        Ok(())
    }

    /// Handler para método JSON-RPC
    pub fn handle_method(&self, method: &str, params: Value) -> ApiResult<Value> {
        match method {
            "workspace.list" => {
                let workspaces = self.list()?;
                serde_json::to_value(workspaces).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "workspace.current" => {
                let workspace = self.current()?;
                serde_json::to_value(workspace).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "workspace.get" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                let workspace = self.get(id)?;
                serde_json::to_value(workspace).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "workspace.create" => {
                let name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let workspace = self.create(name)?;
                serde_json::to_value(workspace).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "workspace.switch" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                let workspace = self.switch(id)?;
                serde_json::to_value(workspace).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "workspace.rename" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                let name = params.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'name' parameter".to_string())
                })?;
                let workspace = self.rename(id, name.to_string())?;
                serde_json::to_value(workspace).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "workspace.close" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                let force = params
                    .get("force")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.close(id, force)?;
                Ok(Value::Null)
            }
            _ => Err(ApiError::MethodNotFound(method.to_string())),
        }
    }
}

impl Default for WorkspaceApi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_create() {
        let api = WorkspaceApi::new();
        let ws = api.create(Some("Test".to_string())).unwrap();

        assert_eq!(ws.name, "Test");
        assert!(!ws.id.is_empty());
    }

    #[test]
    fn test_workspace_list() {
        let api = WorkspaceApi::new();
        let list = api.list().unwrap();

        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Default");
    }

    #[test]
    fn test_workspace_switch() {
        let api = WorkspaceApi::new();
        let ws = api.create(Some("Test".to_string())).unwrap();

        let switched = api.switch(&ws.id).unwrap();
        assert_eq!(switched.id, ws.id);

        let current = api.current().unwrap();
        assert_eq!(current.id, ws.id);
    }

    #[test]
    fn test_workspace_rename() {
        let api = WorkspaceApi::new();
        let ws = api.create(Some("Old".to_string())).unwrap();

        let renamed = api.rename(&ws.id, "New".to_string()).unwrap();
        assert_eq!(renamed.name, "New");
    }

    #[test]
    fn test_workspace_close() {
        let api = WorkspaceApi::new();
        let ws = api.create(Some("ToClose".to_string())).unwrap();

        api.close(&ws.id, false).unwrap();

        let list = api.list().unwrap();
        assert!(!list.iter().any(|w| w.id == ws.id));
    }

    #[test]
    fn test_cannot_close_default() {
        let api = WorkspaceApi::new();
        let result = api.close("default", false);

        assert!(result.is_err());
    }

    #[test]
    fn test_workspace_not_found() {
        let api = WorkspaceApi::new();
        let result = api.get("nonexistent");

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApiError::NotFound(_)));
    }

    fn call(api: &WorkspaceApi, method: &str, params: Value) -> ApiResult<Value> {
        api.handle_method(method, params)
    }

    /// Regressão: get/current/switch devolviam o `is_current` guardado na
    /// criação ("default" sempre true, os outros sempre false).
    #[test]
    fn test_is_current_follows_switch() {
        let api = WorkspaceApi::default();
        let ws = api.create(Some("Dev".to_string())).unwrap();
        assert!(!ws.is_current);

        let switched = api.switch(&ws.id).unwrap();
        assert!(switched.is_current);
        assert!(api.current().unwrap().is_current);
        assert_eq!(api.current().unwrap().id, ws.id);
        assert!(!api.get("default").unwrap().is_current);
        assert!(api.get(&ws.id).unwrap().is_current);
        assert!(api.rename(&ws.id, "Dev2".to_string()).unwrap().is_current);
        assert!(
            !api.rename("default", "Base".to_string())
                .unwrap()
                .is_current
        );
    }

    #[test]
    fn test_close_rules() {
        let api = WorkspaceApi::new();
        let ws = api.create(None).unwrap();
        assert_eq!(ws.name, "New Workspace");
        api.update_tab_count(&ws.id, 2).unwrap();
        api.set_working_dir(&ws.id, "/srv".to_string()).unwrap();
        // IDs desconhecidos são ignorados
        api.update_tab_count("nada", 1).unwrap();
        api.set_working_dir("nada", "/x".to_string()).unwrap();

        let info = api.get(&ws.id).unwrap();
        assert_eq!(info.tab_count, 2);
        assert_eq!(info.working_dir.as_deref(), Some("/srv"));

        match api.close(&ws.id, false) {
            Err(ApiError::InvalidState(message)) => assert!(message.contains("2 tabs")),
            other => panic!("esperava InvalidState, veio {:?}", other),
        }
        assert!(matches!(
            api.close("nada", true),
            Err(ApiError::NotFound(_))
        ));

        // Fechar o atual com force volta para o default
        api.switch(&ws.id).unwrap();
        api.close(&ws.id, true).unwrap();
        assert_eq!(api.current().unwrap().id, "default");

        // Fechar o default com force deixa o "atual" sem workspace
        api.close("default", true).unwrap();
        match api.current() {
            Err(ApiError::NotFound(message)) => assert_eq!(message, "Current workspace not found"),
            other => panic!("esperava NotFound, veio {:?}", other),
        }
        assert!(matches!(api.switch("default"), Err(ApiError::NotFound(_))));
        assert!(matches!(
            api.rename("default", "x".to_string()),
            Err(ApiError::NotFound(_))
        ));
    }

    #[test]
    fn test_handle_method_routes() {
        let api = WorkspaceApi::new();
        let created = call(&api, "workspace.create", serde_json::json!({"name": "A"})).unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        let list = call(&api, "workspace.list", Value::Null).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 2);
        assert_eq!(
            call(&api, "workspace.current", Value::Null).unwrap()["id"],
            "default"
        );
        assert_eq!(
            call(&api, "workspace.get", serde_json::json!({"id": id})).unwrap()["name"],
            "A"
        );
        assert_eq!(
            call(&api, "workspace.switch", serde_json::json!({"id": id})).unwrap()["is_current"],
            true
        );
        assert_eq!(
            call(
                &api,
                "workspace.rename",
                serde_json::json!({"id": id, "name": "B"})
            )
            .unwrap()["name"],
            "B"
        );
        call(&api, "workspace.close", serde_json::json!({"id": id})).unwrap();

        for (method, params) in [
            ("workspace.get", serde_json::json!({})),
            ("workspace.switch", serde_json::json!({})),
            ("workspace.rename", serde_json::json!({"name": "x"})),
            ("workspace.rename", serde_json::json!({"id": "default"})),
            ("workspace.close", serde_json::json!({})),
        ] {
            assert!(
                matches!(call(&api, method, params), Err(ApiError::InvalidParams(_))),
                "{method}"
            );
        }

        match call(&api, "workspace.explode", Value::Null) {
            Err(ApiError::MethodNotFound(method)) => assert_eq!(method, "workspace.explode"),
            other => panic!("esperava MethodNotFound, veio {:?}", other),
        }
    }
}
