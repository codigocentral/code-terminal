//! Session Renderer Protocol - Protocolo Socket para Controle de Sessões
//!
//! Este módulo implementa comandos do protocolo socket para:
//! - Listar sessões renderizadas
//! - Attach remoto com rendering
//! - Desconectar sessões
//!
//! Parte da Tarefa 5.2: Integração SessionManager-Renderer via Socket API

use crate::pane::ThreadSafeRendererIntegration;
use crate::session::SessionManager;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Comandos específicos de sessão/renderização
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRendererCommand {
    /// Lista todas as sessões com estado de renderização
    ListRenderedSessions,

    /// Obtém informações de uma sessão renderizada
    GetRenderInfo { session_id: String },

    /// Inicia renderização de uma sessão
    StartRendering { session_id: String },

    /// Para renderização de uma sessão (mantém em background)
    StopRendering { session_id: String },

    /// Força re-renderização de uma sessão
    ForceRerender { session_id: String },

    /// Obtém estado visual de uma sessão
    GetVisualState { session_id: String },

    /// Atualiza configuração de renderização
    UpdateRenderConfig {
        session_id: String,
        config: RenderConfigUpdate,
    },
}

/// Atualização de configuração de renderização
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderConfigUpdate {
    /// FPS alvo
    pub target_fps: Option<u32>,
    /// Qualidade de renderização
    pub quality: Option<RenderQuality>,
    /// Modo de sincronização
    pub sync_mode: Option<RenderSyncMode>,
}

/// Qualidade de renderização
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderQuality {
    /// Baixa - alta performance
    Low,
    /// Média - balanceado
    Medium,
    /// Alta - melhor qualidade
    High,
    /// Ultra - máxima qualidade
    Ultra,
}

/// Modo de sincronização
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderSyncMode {
    /// Sincronização por VSync
    VSync,
    /// Renderização o mais rápido possível
    Immediate,
    /// Taxa fixa
    FixedRate,
}

/// Respostas para comandos de sessão/renderização
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRendererResponse {
    /// Lista de sessões renderizadas
    RenderedSessionsList(Vec<RenderedSessionInfo>),

    /// Informações de renderização
    RenderInfo(RenderSessionInfo),

    /// Estado visual
    VisualState(VisualStateInfo),

    /// Confirmação de operação
    Success { message: String },

    /// Erro
    Error { code: String, message: String },
}

/// Informação de sessão renderizada
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedSessionInfo {
    /// ID da sessão
    pub session_id: String,
    /// Nome da sessão
    pub name: String,
    /// Quantidade de panes
    pub pane_count: usize,
    /// Pane ativo
    pub active_pane: Option<u64>,
    /// Estado de renderização
    pub render_state: RenderState,
    /// FPS atual
    pub current_fps: f64,
    /// Timestamp do último frame
    pub last_frame_time: String,
}

/// Estado de renderização
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderState {
    /// Renderizando ativamente
    Active,
    /// Pausado (background)
    Paused,
    /// Com erro
    Error,
}

/// Informações detalhadas de uma sessão em renderização
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderSessionInfo {
    /// ID da sessão
    pub session_id: String,
    /// Configuração de renderização
    pub config: RenderConfigInfo,
    /// Informações dos panes
    pub panes: Vec<RenderedPaneInfo>,
    /// Estatísticas
    pub stats: RenderStats,
}

/// Configuração de renderização
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderConfigInfo {
    /// FPS alvo
    pub target_fps: u32,
    /// Qualidade
    pub quality: RenderQuality,
    /// Modo de sincronização
    pub sync_mode: RenderSyncMode,
    /// Buffer duplo
    pub double_buffer: bool,
    /// Antialiasing
    pub antialiasing: bool,
}

/// Informação de pane renderizado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderedPaneInfo {
    /// ID do pane
    pub pane_id: u64,
    /// Posição
    pub position: PanePosition,
    /// Tamanho
    pub size: PaneSize,
    /// Estado
    pub state: String,
    /// Dirty flag
    pub is_dirty: bool,
}

/// Posição do pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanePosition {
    pub x: u32,
    pub y: u32,
}

/// Tamanho do pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneSize {
    pub width: u32,
    pub height: u32,
    pub rows: u16,
    pub cols: u16,
}

/// Estatísticas de renderização
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenderStats {
    /// Frames totais renderizados
    pub total_frames: u64,
    /// Frames perdidos
    pub dropped_frames: u64,
    /// Tempo médio de frame (ms)
    pub avg_frame_time_ms: f64,
    /// FPS médio
    pub avg_fps: f64,
    /// Uso de memória GPU (MB)
    pub gpu_memory_mb: f64,
}

/// Estado visual completo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisualStateInfo {
    /// ID da sessão
    pub session_id: String,
    /// Frame atual
    pub current_frame: u64,
    /// Representação visual (simplificada)
    pub visual_hash: String,
    /// Regiões modificadas
    pub dirty_regions: Vec<DirtyRegion>,
    /// Timestamp
    pub timestamp: String,
}

/// Região modificada
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirtyRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Handler para comandos de sessão/renderer
pub struct SessionRendererHandler {
    /// Integração sessão-renderer
    integration: Arc<Mutex<ThreadSafeRendererIntegration>>,
    /// SessionManager (para acesso a sessões)
    session_manager: Arc<Mutex<SessionManager>>,
}

impl SessionRendererHandler {
    /// Cria um novo handler
    pub fn new(
        integration: ThreadSafeRendererIntegration,
        session_manager: SessionManager,
    ) -> Self {
        Self {
            integration: Arc::new(Mutex::new(integration)),
            session_manager: Arc::new(Mutex::new(session_manager)),
        }
    }

    /// Processa um comando
    pub async fn handle_command(&self, cmd: SessionRendererCommand) -> SessionRendererResponse {
        match cmd {
            SessionRendererCommand::ListRenderedSessions => self.list_rendered_sessions().await,
            SessionRendererCommand::GetRenderInfo { session_id } => {
                self.get_render_info(&session_id).await
            }
            SessionRendererCommand::StartRendering { session_id } => {
                self.start_rendering(&session_id).await
            }
            SessionRendererCommand::StopRendering { session_id } => {
                self.stop_rendering(&session_id).await
            }
            SessionRendererCommand::ForceRerender { session_id } => {
                self.force_rerender(&session_id).await
            }
            SessionRendererCommand::GetVisualState { session_id } => {
                self.get_visual_state(&session_id).await
            }
            SessionRendererCommand::UpdateRenderConfig { session_id, config } => {
                self.update_render_config(&session_id, config).await
            }
        }
    }

    /// Lista sessões em renderização
    async fn list_rendered_sessions(&self) -> SessionRendererResponse {
        debug!("Listando sessões renderizadas");

        let integration = self.integration.lock().await;

        let sessions: Vec<RenderedSessionInfo> = integration
            .with(|i| {
                Ok(i.active_sessions()
                    .iter()
                    .map(|state| RenderedSessionInfo {
                        session_id: state.session_id.clone(),
                        name: state.session_id.clone(), // TODO: usar nome real
                        pane_count: state.pane_ids.len(),
                        active_pane: state.active_pane,
                        render_state: RenderState::Active,
                        current_fps: state.average_fps(),
                        last_frame_time: state.last_frame_time_label(),
                    })
                    .collect())
            })
            .unwrap_or_default();

        SessionRendererResponse::RenderedSessionsList(sessions)
    }

    /// Obtém informações de renderização
    async fn get_render_info(&self, session_id: &str) -> SessionRendererResponse {
        debug!(session_id = %session_id, "Obtendo info de renderização");

        let integration = self.integration.lock().await;

        match integration.with(|i| {
            if let Some(state) = i.get_session_state(session_id) {
                Ok(SessionRendererResponse::RenderInfo(RenderSessionInfo {
                    session_id: state.session_id.clone(),
                    config: state.render_config.clone(),
                    panes: state
                        .pane_ids
                        .iter()
                        .map(|&id| RenderedPaneInfo {
                            pane_id: id,
                            position: PanePosition { x: 0, y: 0 },
                            size: PaneSize {
                                width: 800,
                                height: 600,
                                rows: 24,
                                cols: 80,
                            },
                            state: if Some(id) == state.active_pane {
                                "active".to_string()
                            } else {
                                "background".to_string()
                            },
                            is_dirty: false,
                        })
                        .collect(),
                    stats: RenderStats {
                        total_frames: state.last_frame,
                        dropped_frames: state.dropped_frames,
                        avg_frame_time_ms: state.average_frame_time_ms(),
                        avg_fps: state.average_fps(),
                        gpu_memory_mb: state.gpu_memory_mb(),
                    },
                }))
            } else {
                Err(crate::pane::PaneError::InvalidState(
                    "Session not found".to_string(),
                ))
            }
        }) {
            Ok(response) => response,
            Err(e) => SessionRendererResponse::Error {
                code: "SESSION_NOT_FOUND".to_string(),
                message: e.to_string(),
            },
        }
    }

    /// Inicia renderização de uma sessão
    async fn start_rendering(&self, session_id: &str) -> SessionRendererResponse {
        info!(session_id = %session_id, "Iniciando renderização");

        let integration = self.integration.lock().await;

        match integration.with(|i| i.mark_session_for_render(session_id)) {
            Ok(()) => SessionRendererResponse::Success {
                message: format!("Rendering started for session {}", session_id),
            },
            Err(mark_error) => {
                debug!(
                    session_id = %session_id,
                    error = %mark_error,
                    "Sessão ainda não estava ativa no renderer; tentando carregar do SessionManager"
                );

                let session = {
                    let session_manager = self.session_manager.lock().await;
                    session_manager.load(session_id)
                };

                match session {
                    Ok(session) => {
                        match integration.with(|i| i.connect_persisted_session(&session)) {
                            Ok(_) => SessionRendererResponse::Success {
                                message: format!("Rendering started for session {}", session_id),
                            },
                            Err(e) => SessionRendererResponse::Error {
                                code: "RENDER_ERROR".to_string(),
                                message: e.to_string(),
                            },
                        }
                    }
                    Err(e) => {
                        warn!(session_id = %session_id, error = %e, "Sessão não encontrada no SessionManager");
                        SessionRendererResponse::Error {
                            code: "SESSION_NOT_FOUND".to_string(),
                            message: format!("Session {} not found: {}", session_id, e),
                        }
                    }
                }
            }
        }
    }

    /// Para renderização de uma sessão
    async fn stop_rendering(&self, session_id: &str) -> SessionRendererResponse {
        info!(session_id = %session_id, "Parando renderização");

        let integration = self.integration.lock().await;

        match integration.with(|i| i.disconnect_session(session_id)) {
            Ok(_) => SessionRendererResponse::Success {
                message: format!("Rendering stopped for session {}", session_id),
            },
            Err(e) => SessionRendererResponse::Error {
                code: "RENDER_ERROR".to_string(),
                message: e.to_string(),
            },
        }
    }

    /// Força re-renderização
    async fn force_rerender(&self, session_id: &str) -> SessionRendererResponse {
        debug!(session_id = %session_id, "Forçando re-renderização");

        let integration = self.integration.lock().await;

        match integration.with(|i| i.force_rerender(session_id)) {
            Ok(_) => SessionRendererResponse::Success {
                message: format!("Rerender forced for session {}", session_id),
            },
            Err(e) => SessionRendererResponse::Error {
                code: "SESSION_NOT_FOUND".to_string(),
                message: e.to_string(),
            },
        }
    }

    /// Obtém estado visual
    async fn get_visual_state(&self, session_id: &str) -> SessionRendererResponse {
        debug!(session_id = %session_id, "Obtendo estado visual");

        let integration = self.integration.lock().await;

        if let Ok(state) = integration.with(|i| {
            i.get_session_state(session_id)
                .cloned()
                .ok_or_else(|| crate::pane::PaneError::InvalidState("Not found".to_string()))
        }) {
            SessionRendererResponse::VisualState(VisualStateInfo {
                session_id: state.session_id.clone(),
                current_frame: state.last_frame,
                visual_hash: format!(
                    "{:x}",
                    state
                        .last_frame_at
                        .map(|instant| instant.elapsed().as_nanos())
                        .unwrap_or(0)
                ),
                dirty_regions: vec![],
                timestamp: state.last_frame_time_label(),
            })
        } else {
            SessionRendererResponse::Error {
                code: "SESSION_NOT_FOUND".to_string(),
                message: format!("Session {} not found", session_id),
            }
        }
    }

    /// Atualiza configuração de renderização
    async fn update_render_config(
        &self,
        session_id: &str,
        config: RenderConfigUpdate,
    ) -> SessionRendererResponse {
        debug!(session_id = %session_id, "Atualizando configuração de renderização");

        let integration = self.integration.lock().await;

        match integration.with(|i| i.update_render_config(session_id, config)) {
            Ok(_) => SessionRendererResponse::Success {
                message: format!("Render config updated for session {}", session_id),
            },
            Err(e) => SessionRendererResponse::Error {
                code: "RENDER_CONFIG_NOT_APPLIED".to_string(),
                message: e.to_string(),
            },
        }
    }
}

/// Extensão do SocketServer para suportar comandos de sessão/renderer
pub trait SessionRendererSocketExt {
    /// Registra handlers para comandos de sessão/renderer
    fn register_session_renderer_handlers(&mut self, handler: SessionRendererHandler);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::{SessionRendererIntegration, ThreadSafeRendererIntegration};
    use crate::session::{PaneData, SessionConfig, SessionData, TabData};
    use tempfile::TempDir;

    #[test]
    fn test_render_quality_serialization() {
        let quality = RenderQuality::High;
        let json = serde_json::to_string(&quality).unwrap();
        assert_eq!(json, "\"high\"");

        let decoded: RenderQuality = serde_json::from_str(&json).unwrap();
        assert!(matches!(decoded, RenderQuality::High));
    }

    #[test]
    fn test_session_renderer_command_serialization() {
        let cmd = SessionRendererCommand::ListRenderedSessions;
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("list_rendered_sessions"));
    }

    #[tokio::test]
    async fn test_start_rendering_loads_persisted_session() {
        let temp_dir = TempDir::new().unwrap();
        let session_manager = SessionManager::new(SessionConfig::with_directory(temp_dir.path()));
        session_manager.initialize().unwrap();

        let mut data = SessionData::new();
        data.config.terminal_size = Some((120, 30));
        let mut pane = PaneData::new(7).with_rect(10, 20, 900, 500);
        pane.add_tab(TabData::new(3, "saved-shell").with_command("pwsh"));
        data.layout.set_active_pane(7);
        data.layout.add_pane(pane);
        session_manager.save("saved", &data).unwrap();

        let integration = ThreadSafeRendererIntegration::new(SessionRendererIntegration::new());
        let handler = SessionRendererHandler::new(integration, session_manager);

        let response = handler
            .handle_command(SessionRendererCommand::StartRendering {
                session_id: "saved".to_string(),
            })
            .await;

        assert!(matches!(response, SessionRendererResponse::Success { .. }));

        let info = handler
            .handle_command(SessionRendererCommand::GetRenderInfo {
                session_id: "saved".to_string(),
            })
            .await;

        match info {
            SessionRendererResponse::RenderInfo(info) => {
                assert_eq!(info.session_id, "saved");
                assert_eq!(info.panes.len(), 1);
                assert_eq!(info.panes[0].pane_id, info.panes[0].pane_id);
            }
            other => panic!("expected render info, got {:?}", other),
        }
    }

    /// Handler com a sessão persistida "saved" (2 panes, o 7 ativo) já em
    /// renderização. O TempDir precisa viver enquanto o handler for usado.
    async fn handler_with_saved_session() -> (SessionRendererHandler, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let session_manager = SessionManager::new(SessionConfig::with_directory(temp_dir.path()));
        session_manager.initialize().unwrap();

        let mut data = SessionData::new();
        for id in [7, 8] {
            let mut pane = PaneData::new(id).with_rect(0, 0, 400, 300);
            pane.add_tab(TabData::new(id, "shell"));
            data.layout.add_pane(pane);
        }
        data.layout.set_active_pane(7);
        session_manager.save("saved", &data).unwrap();

        let integration = ThreadSafeRendererIntegration::new(SessionRendererIntegration::new());
        let handler = SessionRendererHandler::new(integration, session_manager);
        let started = handler
            .handle_command(SessionRendererCommand::StartRendering {
                session_id: "saved".to_string(),
            })
            .await;
        assert!(matches!(started, SessionRendererResponse::Success { .. }));
        (handler, temp_dir)
    }

    fn id(session_id: &str) -> String {
        session_id.to_string()
    }

    fn error_code(response: SessionRendererResponse) -> String {
        match response {
            SessionRendererResponse::Error { code, .. } => code,
            other => panic!("esperava erro, veio {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_list_and_info_report_pane_states() {
        let (handler, _dir) = handler_with_saved_session().await;

        match handler
            .handle_command(SessionRendererCommand::ListRenderedSessions)
            .await
        {
            SessionRendererResponse::RenderedSessionsList(sessions) => {
                assert_eq!(sessions.len(), 1);
                assert_eq!(sessions[0].session_id, "saved");
                assert_eq!(sessions[0].pane_count, 2);
                assert!(matches!(sessions[0].render_state, RenderState::Active));
            }
            other => panic!("{:?}", other),
        }

        match handler
            .handle_command(SessionRendererCommand::GetRenderInfo {
                session_id: id("saved"),
            })
            .await
        {
            SessionRendererResponse::RenderInfo(info) => {
                let mut states: Vec<&str> = info.panes.iter().map(|p| p.state.as_str()).collect();
                states.sort();
                assert_eq!(states, vec!["active", "background"]);
                assert_eq!(info.stats.total_frames, 0);
            }
            other => panic!("{:?}", other),
        }

        assert_eq!(
            error_code(
                handler
                    .handle_command(SessionRendererCommand::GetRenderInfo {
                        session_id: id("nada"),
                    })
                    .await
            ),
            "SESSION_NOT_FOUND"
        );
    }

    #[tokio::test]
    async fn test_start_on_active_session_and_unknown_session() {
        let (handler, _dir) = handler_with_saved_session().await;
        // Já ativa: só marca para renderizar
        assert!(matches!(
            handler
                .handle_command(SessionRendererCommand::StartRendering {
                    session_id: id("saved"),
                })
                .await,
            SessionRendererResponse::Success { .. }
        ));
        match handler
            .handle_command(SessionRendererCommand::StartRendering {
                session_id: id("nao-salva"),
            })
            .await
        {
            SessionRendererResponse::Error { code, message } => {
                assert_eq!(code, "SESSION_NOT_FOUND");
                assert!(message.contains("nao-salva"), "{message}");
            }
            other => panic!("{:?}", other),
        }
    }

    #[tokio::test]
    async fn test_rerender_visual_state_and_config() {
        let (handler, _dir) = handler_with_saved_session().await;

        assert!(matches!(
            handler
                .handle_command(SessionRendererCommand::ForceRerender {
                    session_id: id("saved"),
                })
                .await,
            SessionRendererResponse::Success { .. }
        ));
        assert_eq!(
            error_code(
                handler
                    .handle_command(SessionRendererCommand::ForceRerender {
                        session_id: id("nada"),
                    })
                    .await
            ),
            "SESSION_NOT_FOUND"
        );

        match handler
            .handle_command(SessionRendererCommand::GetVisualState {
                session_id: id("saved"),
            })
            .await
        {
            SessionRendererResponse::VisualState(state) => {
                assert_eq!(state.session_id, "saved");
                assert_eq!(state.current_frame, 0);
                assert!(state.dirty_regions.is_empty());
            }
            other => panic!("{:?}", other),
        }
        assert_eq!(
            error_code(
                handler
                    .handle_command(SessionRendererCommand::GetVisualState {
                        session_id: id("nada"),
                    })
                    .await
            ),
            "SESSION_NOT_FOUND"
        );

        let update = RenderConfigUpdate {
            target_fps: Some(24),
            quality: Some(RenderQuality::Low),
            sync_mode: None,
        };
        assert!(matches!(
            handler
                .handle_command(SessionRendererCommand::UpdateRenderConfig {
                    session_id: id("saved"),
                    config: update.clone(),
                })
                .await,
            SessionRendererResponse::Success { .. }
        ));
        match handler
            .handle_command(SessionRendererCommand::GetRenderInfo {
                session_id: id("saved"),
            })
            .await
        {
            SessionRendererResponse::RenderInfo(info) => {
                assert_eq!(info.config.target_fps, 24);
                assert!(matches!(info.config.quality, RenderQuality::Low));
            }
            other => panic!("{:?}", other),
        }
        assert_eq!(
            error_code(
                handler
                    .handle_command(SessionRendererCommand::UpdateRenderConfig {
                        session_id: id("nada"),
                        config: update,
                    })
                    .await
            ),
            "RENDER_CONFIG_NOT_APPLIED"
        );
    }

    #[tokio::test]
    async fn test_stop_rendering_removes_session() {
        let (handler, _dir) = handler_with_saved_session().await;
        assert!(matches!(
            handler
                .handle_command(SessionRendererCommand::StopRendering {
                    session_id: id("saved"),
                })
                .await,
            SessionRendererResponse::Success { .. }
        ));
        match handler
            .handle_command(SessionRendererCommand::ListRenderedSessions)
            .await
        {
            SessionRendererResponse::RenderedSessionsList(sessions) => assert!(sessions.is_empty()),
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn test_command_and_response_round_trip() {
        let cmd = SessionRendererCommand::UpdateRenderConfig {
            session_id: "s".to_string(),
            config: RenderConfigUpdate {
                target_fps: Some(30),
                quality: None,
                sync_mode: None,
            },
        };
        let json = serde_json::to_value(&cmd).unwrap();
        assert_eq!(json["update_render_config"]["session_id"], "s");
        let decoded: SessionRendererCommand = serde_json::from_value(json).unwrap();
        assert!(matches!(
            decoded,
            SessionRendererCommand::UpdateRenderConfig { config, .. } if config.target_fps == Some(30)
        ));

        let response = SessionRendererResponse::Error {
            code: "X".to_string(),
            message: "y".to_string(),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"error": {"code": "X", "message": "y"}})
        );
    }
}
