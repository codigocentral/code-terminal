//! Session Renderer API - API Socket para Controle de Sessões Renderizadas
//!
//! Implementa o handler para os comandos definidos em session_renderer_protocol.rs
//! e integra com SessionRendererIntegration

use super::{
    ApiError, ApiResult, PanePosition, PaneSize, RenderConfigUpdate, RenderSessionInfo,
    RenderState, RenderStats, RenderedPaneInfo, RenderedSessionInfo, SessionRendererResponse,
    VisualStateInfo,
};
use crate::pane::renderer_integration::{
    SessionRendererIntegration, ThreadSafeRendererIntegration,
};
use parking_lot::RwLock;
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, info};

/// API para controle de sessões renderizadas via socket
pub struct SessionRendererApi {
    /// Integração com renderer thread-safe (opcional até ser configurada)
    integration: Arc<RwLock<Option<ThreadSafeRendererIntegration>>>,
}

impl SessionRendererApi {
    /// Cria uma nova API de session renderer
    pub fn new() -> Self {
        Self {
            integration: Arc::new(RwLock::new(None)),
        }
    }

    /// Configura a integração com renderer
    pub fn set_integration(&self, integration: SessionRendererIntegration) {
        let mut int = self.integration.write();
        *int = Some(ThreadSafeRendererIntegration::new(integration));
        info!("SessionRendererApi: Integration configured");
    }

    /// Verifica se a integração está configurada
    pub fn is_configured(&self) -> bool {
        self.integration.read().is_some()
    }

    /// Processa um método da API
    pub fn handle_method(&self, method: &str, params: Option<Value>) -> ApiResult<Value> {
        // Parse dos parâmetros
        let params = params.unwrap_or(Value::Null);

        // Processa o comando
        match method {
            "list" | "session_renderer.list" => self.list_rendered_sessions(),
            "get_info" | "session_renderer.get_info" => self.get_render_info(&params),
            "start" | "session_renderer.start" => self.start_rendering(&params),
            "stop" | "session_renderer.stop" => self.stop_rendering(&params),
            "force_rerender" | "session_renderer.force_rerender" => self.force_rerender(&params),
            "get_visual_state" | "session_renderer.get_visual_state" => {
                self.get_visual_state(&params)
            }
            "update_config" | "session_renderer.update_config" => {
                self.update_render_config(&params)
            }
            // Aliases para compatibilidade
            "list_rendered_sessions" | "session_renderer.list_rendered_sessions" => {
                self.list_rendered_sessions()
            }
            "get_render_info" | "session_renderer.get_render_info" => self.get_render_info(&params),
            "start_rendering" | "session_renderer.start_rendering" => self.start_rendering(&params),
            "stop_rendering" | "session_renderer.stop_rendering" => self.stop_rendering(&params),
            _ => Err(ApiError::MethodNotFound(format!(
                "Unknown session_renderer method: {}",
                method
            ))),
        }
    }

    /// Lista todas as sessões renderizadas
    fn list_rendered_sessions(&self) -> ApiResult<Value> {
        debug!("Listing rendered sessions");

        let integration_guard = self.integration.read();
        let thread_safe = integration_guard.as_ref().ok_or_else(|| {
            ApiError::InvalidState("SessionRendererIntegration not configured".to_string())
        })?;

        // Obtém sessões de forma thread-safe
        let sessions: Vec<RenderedSessionInfo> = thread_safe
            .with(|integration| {
                Ok::<_, crate::pane::PaneError>(
                    integration
                        .active_render_sessions()
                        .iter()
                        .map(|(session_id, state)| RenderedSessionInfo {
                            session_id: session_id.clone(),
                            name: state.session_id.clone(),
                            pane_count: state.pane_ids.len(),
                            active_pane: state.active_pane,
                            render_state: RenderState::Active,
                            current_fps: state.average_fps(),
                            last_frame_time: state.last_frame_time_label(),
                        })
                        .collect(),
                )
            })
            .map_err(|e| ApiError::InternalError(e.to_string()))?;

        let response = SessionRendererResponse::RenderedSessionsList(sessions);
        serde_json::to_value(response)
            .map_err(|e| ApiError::InternalError(format!("Serialization error: {}", e)))
    }

    /// Obtém informações de renderização de uma sessão
    fn get_render_info(&self, params: &Value) -> ApiResult<Value> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::InvalidParams("Missing session_id".to_string()))?;

        debug!("Getting render info for session: {}", session_id);

        let integration_guard = self.integration.read();
        let thread_safe = integration_guard.as_ref().ok_or_else(|| {
            ApiError::InvalidState("SessionRendererIntegration not configured".to_string())
        })?;

        // Obtém informações de forma thread-safe
        let (config, panes, stats) = thread_safe
            .with(|integration| {
                let state = integration
                    .get_render_session_state(session_id)
                    .ok_or_else(|| crate::pane::PaneError::NotFound(0))?
                    .clone();

                let panes: Vec<RenderedPaneInfo> = state
                    .pane_ids
                    .iter()
                    .map(|&pane_id| RenderedPaneInfo {
                        pane_id,
                        position: PanePosition { x: 0, y: 0 },
                        size: PaneSize {
                            width: 800,
                            height: 600,
                            rows: 24,
                            cols: 80,
                        },
                        state: if Some(pane_id) == state.active_pane {
                            "active".to_string()
                        } else {
                            "background".to_string()
                        },
                        is_dirty: false,
                    })
                    .collect();

                let stats = RenderStats {
                    total_frames: state.last_frame,
                    dropped_frames: state.dropped_frames,
                    avg_frame_time_ms: state.average_frame_time_ms(),
                    avg_fps: state.average_fps(),
                    gpu_memory_mb: state.gpu_memory_mb(),
                };

                Ok::<_, crate::pane::PaneError>((state.render_config, panes, stats))
            })
            .map_err(|_e| ApiError::NotFound(format!("Session {} not found", session_id)))?;

        let info = RenderSessionInfo {
            session_id: session_id.to_string(),
            config,
            panes,
            stats,
        };

        let response = SessionRendererResponse::RenderInfo(info);
        serde_json::to_value(response)
            .map_err(|e| ApiError::InternalError(format!("Serialization error: {}", e)))
    }

    /// Inicia renderização de uma sessão
    fn start_rendering(&self, params: &Value) -> ApiResult<Value> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::InvalidParams("Missing session_id".to_string()))?;

        info!("Starting rendering for session: {}", session_id);

        let integration_guard = self.integration.read();
        let thread_safe = integration_guard.as_ref().ok_or_else(|| {
            ApiError::InvalidState("SessionRendererIntegration not configured".to_string())
        })?;

        thread_safe
            .with(|integration| integration.mark_session_for_render(session_id))
            .map_err(|e| ApiError::InternalError(format!("Failed to start rendering: {}", e)))?;

        let response = SessionRendererResponse::Success {
            message: format!("Rendering started for session {}", session_id),
        };

        serde_json::to_value(response)
            .map_err(|e| ApiError::InternalError(format!("Serialization error: {}", e)))
    }

    /// Para renderização de uma sessão
    fn stop_rendering(&self, params: &Value) -> ApiResult<Value> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::InvalidParams("Missing session_id".to_string()))?;

        info!("Stopping rendering for session: {}", session_id);

        let integration_guard = self.integration.read();
        let thread_safe = integration_guard.as_ref().ok_or_else(|| {
            ApiError::InvalidState("SessionRendererIntegration not configured".to_string())
        })?;

        thread_safe
            .with(|integration| integration.disconnect_session(session_id))
            .map_err(|e| ApiError::InternalError(format!("Failed to stop rendering: {}", e)))?;

        let response = SessionRendererResponse::Success {
            message: format!("Rendering stopped for session {}", session_id),
        };

        serde_json::to_value(response)
            .map_err(|e| ApiError::InternalError(format!("Serialization error: {}", e)))
    }

    /// Força re-renderização de uma sessão
    fn force_rerender(&self, params: &Value) -> ApiResult<Value> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::InvalidParams("Missing session_id".to_string()))?;

        debug!("Forcing rerender for session: {}", session_id);

        let integration_guard = self.integration.read();
        let thread_safe = integration_guard.as_ref().ok_or_else(|| {
            ApiError::InvalidState("SessionRendererIntegration not configured".to_string())
        })?;

        thread_safe
            .with(|integration| integration.force_rerender(session_id))
            .map_err(|e| ApiError::InternalError(format!("Failed to force rerender: {}", e)))?;

        let response = SessionRendererResponse::Success {
            message: format!("Rerender forced for session {}", session_id),
        };

        serde_json::to_value(response)
            .map_err(|e| ApiError::InternalError(format!("Serialization error: {}", e)))
    }

    /// Obtém estado visual de uma sessão
    fn get_visual_state(&self, params: &Value) -> ApiResult<Value> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::InvalidParams("Missing session_id".to_string()))?;

        debug!("Getting visual state for session: {}", session_id);

        let integration_guard = self.integration.read();
        let thread_safe = integration_guard.as_ref().ok_or_else(|| {
            ApiError::InvalidState("SessionRendererIntegration not configured".to_string())
        })?;

        let state = thread_safe
            .with(|integration| {
                integration
                    .get_render_session_state(session_id)
                    .cloned()
                    .ok_or_else(|| crate::pane::PaneError::NotFound(0))
            })
            .map_err(|_| ApiError::NotFound(format!("Session {} not found", session_id)))?;

        let visual_hash = format!(
            "{:x}",
            state
                .last_frame_at
                .map(|instant| instant.elapsed().as_nanos())
                .unwrap_or(0)
        );
        let timestamp = state.last_frame_time_label();

        let visual_state = VisualStateInfo {
            session_id: state.session_id,
            current_frame: state.last_frame,
            visual_hash,
            dirty_regions: vec![],
            timestamp,
        };

        let response = SessionRendererResponse::VisualState(visual_state);
        serde_json::to_value(response)
            .map_err(|e| ApiError::InternalError(format!("Serialization error: {}", e)))
    }

    /// Atualiza configuração de renderização
    fn update_render_config(&self, params: &Value) -> ApiResult<Value> {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::InvalidParams("Missing session_id".to_string()))?;

        let config: RenderConfigUpdate =
            serde_json::from_value(params.get("config").cloned().unwrap_or(Value::Null))
                .map_err(|e| ApiError::InvalidParams(format!("Invalid config: {}", e)))?;

        info!(
            "Updating render config for session {}: {:?}",
            session_id, config
        );

        let integration_guard = self.integration.read();
        let thread_safe = integration_guard.as_ref().ok_or_else(|| {
            ApiError::InvalidState("SessionRendererIntegration not configured".to_string())
        })?;

        thread_safe
            .with(|integration| integration.update_render_config(session_id, config))
            .map_err(|e| {
                ApiError::InternalError(format!("Failed to update render config: {}", e))
            })?;

        let response = SessionRendererResponse::Success {
            message: format!("Render config updated for session {}", session_id),
        };

        serde_json::to_value(response)
            .map_err(|e| ApiError::InternalError(format!("Serialization error: {}", e)))
    }

    /// Obtém a integração (para uso interno pelo servidor)
    pub fn integration(&self) -> Arc<RwLock<Option<ThreadSafeRendererIntegration>>> {
        Arc::clone(&self.integration)
    }
}

impl Default for SessionRendererApi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_renderer_api_creation() {
        let api = SessionRendererApi::new();
        assert!(!api.is_configured());
    }

    #[test]
    fn test_list_rendered_sessions() {
        let api = SessionRendererApi::new();

        // Cria uma integração mock
        let integration = SessionRendererIntegration::new();
        api.set_integration(integration);

        let result = api.list_rendered_sessions();
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_render_info() {
        let api = SessionRendererApi::new();
        let integration = SessionRendererIntegration::new();
        api.set_integration(integration);

        let params = serde_json::json!({"session_id": "test-session"});
        let result = api.get_render_info(&params);
        assert!(matches!(result.unwrap_err(), ApiError::NotFound(_)));
    }

    #[test]
    fn test_missing_session_id() {
        let api = SessionRendererApi::new();
        let integration = SessionRendererIntegration::new();
        api.set_integration(integration);

        let params = serde_json::json!({});
        let result = api.get_render_info(&params);
        assert!(result.is_err());
    }

    use crate::multiplexer::MultiplexerSession;

    /// API com uma sessão real conectada ao renderer; devolve o id da sessão.
    fn configured_api() -> (SessionRendererApi, String) {
        let api = SessionRendererApi::default();
        let mut integration = SessionRendererIntegration::new();
        let session = MultiplexerSession::new(Some("render".to_string()), (80, 24));
        integration.connect_session(&session).unwrap();
        api.set_integration(integration);
        (api, session.id.to_string())
    }

    fn call(api: &SessionRendererApi, method: &str, params: Value) -> ApiResult<Value> {
        api.handle_method(method, Some(params))
    }

    #[test]
    fn test_methods_require_configured_integration() {
        let api = SessionRendererApi::new();
        let params = serde_json::json!({"session_id": "x", "config": {}});
        for method in [
            "list",
            "get_info",
            "start",
            "stop",
            "force_rerender",
            "get_visual_state",
            "update_config",
        ] {
            assert!(
                matches!(
                    call(&api, method, params.clone()),
                    Err(ApiError::InvalidState(_))
                ),
                "{method}"
            );
        }
        assert!(api.integration().read().is_none());
    }

    #[test]
    fn test_methods_require_session_id() {
        let (api, _) = configured_api();
        for method in [
            "get_info",
            "start",
            "stop",
            "force_rerender",
            "get_visual_state",
            "update_config",
        ] {
            match call(&api, method, serde_json::json!({})) {
                Err(ApiError::InvalidParams(message)) => assert_eq!(message, "Missing session_id"),
                other => panic!("{method}: esperava InvalidParams, veio {:?}", other),
            }
        }
        // Sem params nenhum também vale
        assert!(matches!(
            api.handle_method("get_info", None),
            Err(ApiError::InvalidParams(_))
        ));
    }

    #[test]
    fn test_list_and_render_info_of_connected_session() {
        let (api, session_id) = configured_api();
        let list = call(&api, "session_renderer.list_rendered_sessions", Value::Null).unwrap();
        let sessions = list["rendered_sessions_list"].as_array().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["session_id"], session_id.as_str());
        // A sessão do multiplexer ainda não expõe panes ao renderer
        assert_eq!(sessions[0]["pane_count"], 0);
        assert_eq!(sessions[0]["render_state"], "active");

        let info = call(
            &api,
            "session_renderer.get_render_info",
            serde_json::json!({"session_id": session_id}),
        )
        .unwrap();
        let info = &info["render_info"];
        assert_eq!(info["session_id"], session_id.as_str());
        assert!(info["panes"].as_array().unwrap().is_empty());
        assert_eq!(info["config"]["target_fps"], 60);
        assert_eq!(info["stats"]["total_frames"], 0);
    }

    #[test]
    fn test_visual_state_and_unknown_session() {
        let (api, session_id) = configured_api();
        let state = call(
            &api,
            "get_visual_state",
            serde_json::json!({"session_id": session_id}),
        )
        .unwrap();
        let state = &state["visual_state"];
        assert_eq!(state["session_id"], session_id.as_str());
        assert_eq!(state["current_frame"], 0);
        assert!(
            state["visual_hash"].as_str().unwrap().starts_with('0'),
            "{state}"
        );

        assert!(matches!(
            call(
                &api,
                "get_visual_state",
                serde_json::json!({"session_id": "nada"})
            ),
            Err(ApiError::NotFound(_))
        ));
    }

    #[test]
    fn test_update_config_start_rerender_and_stop() {
        let (api, session_id) = configured_api();
        let ok = |method: &str, params: Value| -> String {
            call(&api, method, params).unwrap()["success"]["message"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let id = serde_json::json!({"session_id": session_id});

        let message = ok(
            "session_renderer.update_config",
            serde_json::json!({"session_id": session_id, "config": {"target_fps": 30, "quality": "low"}}),
        );
        assert!(message.contains("Render config updated"), "{message}");
        let info = call(&api, "get_info", id.clone()).unwrap();
        assert_eq!(info["render_info"]["config"]["target_fps"], 30);

        assert!(matches!(
            call(
                &api,
                "update_config",
                serde_json::json!({"session_id": session_id, "config": {"quality": "ultra-max"}})
            ),
            Err(ApiError::InvalidParams(_))
        ));

        assert!(ok("session_renderer.start_rendering", id.clone()).contains("Rendering started"));
        assert!(ok("session_renderer.force_rerender", id.clone()).contains("Rerender forced"));
        assert!(ok("session_renderer.stop_rendering", id.clone()).contains("Rendering stopped"));

        let list = call(&api, "list", Value::Null).unwrap();
        assert!(list["rendered_sessions_list"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_operations_on_unknown_session_are_internal_errors() {
        let (api, _) = configured_api();
        let unknown = serde_json::json!({"session_id": "nada", "config": {}});
        for (method, prefix) in [
            ("start", "Failed to start rendering"),
            ("force_rerender", "Failed to force rerender"),
            ("update_config", "Failed to update render config"),
        ] {
            match call(&api, method, unknown.clone()) {
                Err(ApiError::InternalError(message)) => {
                    assert!(message.starts_with(prefix), "{method}: {message}")
                }
                other => panic!("{method}: esperava InternalError, veio {:?}", other),
            }
        }
        assert!(matches!(
            call(&api, "get_info", unknown),
            Err(ApiError::NotFound(_))
        ));
    }

    #[test]
    fn test_unknown_method() {
        let api = SessionRendererApi::new();
        match call(&api, "session_renderer.explode", Value::Null) {
            Err(ApiError::MethodNotFound(message)) => assert_eq!(
                message,
                "Unknown session_renderer method: session_renderer.explode"
            ),
            other => panic!("esperava MethodNotFound, veio {:?}", other),
        }
    }
}
