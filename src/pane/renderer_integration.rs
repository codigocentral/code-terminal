//! Renderer Integration - Conexão SessionManager com Renderer
//!
//! Este módulo implementa a Tarefa 5.2 e 5.3:
//! - Conectar SessionManager ao Renderer
//! - Attach/Detach com Rendering real
//!
//! Responsável por:
//! - Sincronizar estado das sessões com o renderer
//! - Gerenciar ciclos de vida de panes renderizados
//! - Notificar renderer de mudanças em sessões

use super::session_integration::{AttachManager, ClientAttachContext, SessionPaneIntegration};
use super::{PaneError, PaneId, PaneResult};
use crate::multiplexer::session::MultiplexerSession;
use crate::multiplexer::socket_api::{
    RenderConfigInfo, RenderConfigUpdate, RenderQuality, RenderSyncMode,
};
use crate::renderer::pane::PaneRenderer;
use crate::session::{Session, SessionManager};
use crate::ui::layout::{LayoutManager, Pane as LayoutPane};
use crate::ui::tabs::TabManager;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{info, trace};

/// Integração completa entre SessionManager, Panes e Renderer
///
/// Esta é a estrutura central da Fase 5 que conecta todos os componentes
pub struct SessionRendererIntegration {
    /// Integração sessão-pane (já existe)
    session_integration: SessionPaneIntegration,

    /// Renderer de panes (já existe)
    pane_renderer: Option<PaneRenderer>,

    /// Layout manager para posicionamento
    layout_manager: LayoutManager,

    /// Mapeamento de sessões para layouts
    session_layouts: HashMap<String, LayoutManager>,

    /// Gerenciador de attach
    attach_manager: AttachManager,

    /// Sessões atualmente renderizadas
    active_render_sessions: HashMap<String, RenderSessionState>,

    /// Flag de dirty para re-renderização
    needs_render: bool,
}

/// Estado de uma sessão em renderização
#[derive(Debug, Clone)]
pub struct RenderSessionState {
    /// ID da sessão
    pub session_id: String,
    /// IDs dos panes sendo renderizados
    pub pane_ids: Vec<PaneId>,
    /// Pane ativo
    pub active_pane: Option<PaneId>,
    /// Último frame renderizado
    pub last_frame: u64,
    /// Timestamp do último update
    pub last_update: std::time::Instant,
    /// Timestamp do primeiro frame renderizado
    pub first_frame_at: Option<std::time::Instant>,
    /// Timestamp do último frame renderizado
    pub last_frame_at: Option<std::time::Instant>,
    /// Frames estimados como perdidos entre frames reais.
    pub dropped_frames: u64,
    /// Memória GPU reportada pelo renderer quando disponível.
    pub gpu_memory_mb: Option<f64>,
    /// Configuração de renderização aplicada à sessão
    pub render_config: RenderConfigInfo,
}

impl RenderSessionState {
    /// Cria estado inicial de renderização para uma sessão.
    pub fn new(session_id: String, pane_ids: Vec<PaneId>, active_pane: Option<PaneId>) -> Self {
        Self {
            session_id,
            pane_ids,
            active_pane,
            last_frame: 0,
            last_update: std::time::Instant::now(),
            first_frame_at: None,
            last_frame_at: None,
            dropped_frames: 0,
            gpu_memory_mb: None,
            render_config: RenderConfigInfo::default(),
        }
    }

    /// Registra que um frame foi renderizado.
    pub fn record_frame(&mut self) {
        let now = std::time::Instant::now();
        if let Some(last_frame_at) = self.last_frame_at {
            let target_frame_ms = 1000.0 / self.render_config.target_fps.max(1) as f64;
            let elapsed_ms = now.duration_since(last_frame_at).as_secs_f64() * 1000.0;
            if elapsed_ms > target_frame_ms * 1.5 {
                let expected_frames = (elapsed_ms / target_frame_ms).floor() as u64;
                self.dropped_frames = self
                    .dropped_frames
                    .saturating_add(expected_frames.saturating_sub(1));
            }
        }
        self.last_frame += 1;
        self.last_update = now;
        if self.first_frame_at.is_none() {
            self.first_frame_at = Some(now);
        }
        self.last_frame_at = Some(now);
    }

    /// FPS médio calculado a partir dos frames realmente renderizados.
    pub fn average_fps(&self) -> f64 {
        let Some(first_frame_at) = self.first_frame_at else {
            return 0.0;
        };
        let Some(last_frame_at) = self.last_frame_at else {
            return 0.0;
        };
        if self.last_frame < 2 {
            return 0.0;
        }

        let elapsed = last_frame_at.duration_since(first_frame_at).as_secs_f64();
        if elapsed <= f64::EPSILON {
            0.0
        } else {
            (self.last_frame.saturating_sub(1)) as f64 / elapsed
        }
    }

    /// Tempo médio de frame em milissegundos.
    pub fn average_frame_time_ms(&self) -> f64 {
        let fps = self.average_fps();
        if fps <= f64::EPSILON {
            0.0
        } else {
            1000.0 / fps
        }
    }

    /// Timestamp legível do último frame real.
    pub fn last_frame_time_label(&self) -> String {
        self.last_frame_at
            .map(|instant| format!("{:?} ago", instant.elapsed()))
            .unwrap_or_else(|| "never".to_string())
    }

    /// Memória GPU reportada pela camada renderer.
    pub fn gpu_memory_mb(&self) -> f64 {
        self.gpu_memory_mb.unwrap_or(0.0)
    }
}

/// Comando para atualização do renderer
#[derive(Debug, Clone)]
pub enum RendererCommand {
    /// Atualizar conteúdo de um pane
    UpdatePane(PaneId),
    /// Atualizar layout de uma sessão
    UpdateLayout(String),
    /// Mudar pane ativo
    SetActivePane(PaneId),
    /// Sessão foi anexada
    SessionAttached(String),
    /// Sessão foi desanexada
    SessionDetached(String),
    /// Forçar re-renderização completa
    ForceRerender,
}

impl SessionRendererIntegration {
    /// Cria uma nova integração
    pub fn new() -> Self {
        Self {
            session_integration: SessionPaneIntegration::new(),
            pane_renderer: None,
            layout_manager: LayoutManager::new(800, 600),
            session_layouts: HashMap::new(),
            attach_manager: AttachManager::new(),
            active_render_sessions: HashMap::new(),
            needs_render: false,
        }
    }

    /// Configura com renderer
    pub fn with_renderer(mut self, renderer: PaneRenderer) -> Self {
        self.pane_renderer = Some(renderer);
        self
    }

    /// Configura com layout manager
    pub fn with_layout_manager(mut self, layout: LayoutManager) -> Self {
        self.layout_manager = layout;
        self
    }

    // ========== TAREFA 5.2: Conectar SessionManager ao Renderer ==========

    /// Conecta uma sessão do SessionManager ao sistema de renderização
    ///
    /// Esta função é o coração da Tarefa 5.2
    pub fn connect_session(&mut self, session: &MultiplexerSession) -> PaneResult<Vec<PaneId>> {
        info!(session_id = %session.id, "Conectando sessão ao renderer");

        // 1. Usa a integração existente para criar panes unificados
        let pane_ids = self.session_integration.attach_session(session)?;

        // 2. Cria um layout manager específico para esta sessão
        let mut session_layout = LayoutManager::new(800, 600);

        // 3. Para cada pane, cria um layout pane
        for (i, &pane_id) in pane_ids.iter().enumerate() {
            if self
                .session_integration
                .pane_manager()
                .get_pane(pane_id)
                .is_some()
            {
                // Cria TabManager para o pane de layout
                // Cria um novo TabManager (não podemos clonar pois Tab não é Clone)
                let tab_manager = TabManager::new();

                if i == 0 {
                    // Primeiro pane é o root
                    session_layout.create_root_pane(tab_manager);
                } else {
                    // Panes subsequentes são splits
                    let root_id = session_layout
                        .panes()
                        .keys()
                        .next()
                        .copied()
                        .ok_or_else(|| PaneError::InvalidState("No root pane".to_string()))?;

                    // Alterna entre horizontal e vertical
                    let direction = if i % 2 == 1 {
                        crate::ui::layout::SplitDirection::Vertical
                    } else {
                        crate::ui::layout::SplitDirection::Horizontal
                    };

                    session_layout.split_pane(root_id, direction, tab_manager);
                }
            }
        }

        // 4. Armazena o layout da sessão
        self.session_layouts
            .insert(session.id.to_string(), session_layout);

        // 5. Registra como sessão ativa em renderização
        let render_state = RenderSessionState::new(
            session.id.to_string(),
            pane_ids.clone(),
            pane_ids.first().copied(),
        );

        self.active_render_sessions
            .insert(session.id.to_string(), render_state);

        // 6. Marca para re-renderização
        self.needs_render = true;

        info!(session_id = %session.id, pane_count = pane_ids.len(), "Sessão conectada com sucesso");

        Ok(pane_ids)
    }

    /// Conecta uma sessão persistida pelo SessionManager ao sistema de renderização.
    pub fn connect_persisted_session(&mut self, session: &Session) -> PaneResult<Vec<PaneId>> {
        let session_id = session.metadata.name.clone();
        info!(session_id = %session_id, "Conectando sessão persistida ao renderer");

        if let Some(state) = self.active_render_sessions.get(&session_id) {
            self.needs_render = true;
            return Ok(state.pane_ids.clone());
        }

        let pane_ids = self
            .session_integration
            .restore_session_data(&session_id, &session.data)?;

        let mut session_layout = LayoutManager::new(800, 600);
        for (i, _) in pane_ids.iter().enumerate() {
            let tab_manager = TabManager::new();
            if i == 0 {
                session_layout.create_root_pane(tab_manager);
            } else {
                let root_id = session_layout
                    .panes()
                    .keys()
                    .next()
                    .copied()
                    .ok_or_else(|| PaneError::InvalidState("No root pane".to_string()))?;
                let direction = if i % 2 == 1 {
                    crate::ui::layout::SplitDirection::Vertical
                } else {
                    crate::ui::layout::SplitDirection::Horizontal
                };
                session_layout.split_pane(root_id, direction, tab_manager);
            }
        }

        self.session_layouts
            .insert(session_id.clone(), session_layout);

        let active_pane = self
            .session_integration
            .pane_manager()
            .active_pane()
            .map(|pane| pane.id)
            .filter(|id| pane_ids.contains(id))
            .or_else(|| pane_ids.first().copied());

        self.active_render_sessions.insert(
            session_id.clone(),
            RenderSessionState::new(session_id.clone(), pane_ids.clone(), active_pane),
        );
        self.needs_render = true;

        info!(session_id = %session_id, pane_count = pane_ids.len(), "Sessão persistida conectada com sucesso");

        Ok(pane_ids)
    }

    /// Desconecta uma sessão do renderer
    pub fn disconnect_session(&mut self, session_id: &str) -> PaneResult<()> {
        info!(session_id = %session_id, "Desconectando sessão do renderer");

        // 1. Remove dos layouts
        self.session_layouts.remove(session_id);

        // 2. Remove das sessões ativas
        self.active_render_sessions.remove(session_id);

        // 3. Realiza detach na integração
        self.session_integration.detach_session(session_id)?;

        // 4. Marca para re-renderização
        self.needs_render = true;

        info!(session_id = %session_id, "Sessão desconectada");

        Ok(())
    }

    /// Sincroniza estado do SessionManager com o Renderer
    ///
    /// Deve ser chamada periodicamente para manter o estado sincronizado
    pub fn sync_sessions(&mut self, _session_manager: &SessionManager) -> PaneResult<()> {
        // Para cada sessão ativa, verifica se precisa atualizar
        for session_id in self.active_render_sessions.keys() {
            // Verifica se sessão ainda existe no SessionManager
            // Nota: SessionManager precisaria ter um método para verificar isso

            // Atualiza timestamp
            trace!(session_id = %session_id, "Sincronizando sessão");
        }

        Ok(())
    }

    // ========== TAREFA 5.3: Attach/Detach com Rendering ==========

    /// Anexa um cliente a uma sessão com rendering completo
    ///
    /// Implementação da Tarefa 5.3
    pub fn attach_session_with_rendering(
        &mut self,
        context: ClientAttachContext,
        session: &MultiplexerSession,
    ) -> PaneResult<AttachRenderResult> {
        info!(
            session_id = %session.id,
            client_id = %context.client_id,
            "Anexando sessão com rendering"
        );

        // Salva o terminal_size antes de mover context
        let terminal_size = context.terminal_size;

        // 1. Realiza attach via AttachManager
        let attach_result = self.attach_manager.attach(context, session)?;

        // 2. Conecta a sessão ao renderer (se ainda não estiver)
        if !self
            .active_render_sessions
            .contains_key(&session.id.to_string())
        {
            self.connect_session(session)?;
        }

        // 3. Configura o tamanho do terminal do cliente
        self.update_terminal_size(&session.id.to_string(), terminal_size)?;

        // 4. Marca para re-renderização
        self.needs_render = true;

        info!(
            session_id = %session.id,
            pane_count = attach_result.pane_ids.len(),
            "Sessão anexada com rendering"
        );

        Ok(AttachRenderResult {
            pane_ids: attach_result.pane_ids,
            active_pane: attach_result.active_pane,
            session_data: attach_result.session_data,
            render_session_id: session.id.to_string(),
        })
    }

    /// Desanexa um cliente com persistência do estado visual
    pub fn detach_session_with_rendering(
        &mut self,
        session_id: &str,
    ) -> PaneResult<serde_json::Value> {
        info!(session_id = %session_id, "Desanexando sessão com rendering");

        // 1. Captura estado atual de renderização
        let render_state = self.active_render_sessions.get(session_id).ok_or_else(|| {
            PaneError::InvalidState(format!("Session {} not in rendering", session_id))
        })?;

        // 2. Serializa estado visual
        let visual_state = serde_json::json!({
            "session_id": session_id,
            "pane_ids": render_state.pane_ids,
            "active_pane": render_state.active_pane,
            "last_frame": render_state.last_frame,
        });

        // 3. Realiza detach
        let session_data = self.attach_manager.detach(session_id)?;

        // 4. Mantém a sessão conectada ao renderer (background)
        // mas remove das ativas para este cliente

        // 5. Marca para re-renderização
        self.needs_render = true;

        info!(session_id = %session_id, "Sessão desanexada, estado visual preservado");

        Ok(serde_json::json!({
            "session_data": session_data,
            "visual_state": visual_state,
        }))
    }

    /// Atualiza tamanho do terminal
    fn update_terminal_size(&mut self, session_id: &str, size: (u16, u16)) -> PaneResult<()> {
        if let Some(_layout) = self.session_layouts.get_mut(session_id) {
            // TODO: Redimensionar panes proporcionalmente
            trace!(session_id = %session_id, cols = size.0, rows = size.1, "Terminal redimensionado");
        }

        Ok(())
    }

    // ========== RENDERIZAÇÃO ==========

    /// Renderiza todas as sessões ativas
    ///
    /// Esta é a função principal de renderização
    pub fn render(&mut self) -> PaneResult<()> {
        if !self.needs_render {
            return Ok(());
        }

        // 1. Atualiza estado de todos os panes
        self.session_integration.pane_manager_mut().update_all();

        // 2. Para cada sessão ativa
        let session_ids: Vec<String> = self.active_render_sessions.keys().cloned().collect();
        for session_id in session_ids {
            self.render_session(&session_id)?;
            if let Some(state) = self.active_render_sessions.get_mut(&session_id) {
                state.record_frame();
            }
        }

        // 3. Limpa flag de dirty
        self.needs_render = false;

        Ok(())
    }

    /// Renderiza uma sessão específica
    fn render_session(&mut self, session_id: &str) -> PaneResult<()> {
        trace!(session_id = %session_id, "Renderizando sessão");

        // Obtém o layout da sessão
        if let Some(layout) = self.session_layouts.get(session_id) {
            // Coleta panes visíveis
            let panes: Vec<&LayoutPane> = layout.panes().values().collect();

            // Atualiza estado de renderização de cada pane
            for pane in &panes {
                let unified_id = self.find_unified_id(session_id, pane.id);

                if let Some(unified_id) = unified_id {
                    if let Some(unified_pane) =
                        self.session_integration.pane_manager().get_pane(unified_id)
                    {
                        // Verifica se pane está dirty
                        if unified_pane.is_dirty {
                            // Marca no renderer
                            if let Some(renderer) = &mut self.pane_renderer {
                                renderer.mark_pane_dirty(pane.id);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Encontra ID unificado a partir do ID de layout
    fn find_unified_id(&self, _session_id: &str, layout_pane_id: PaneId) -> Option<PaneId> {
        // Mapeamento direto: no momento estamos usando o mesmo ID
        // mas em uma implementação mais complexa poderia haver um mapeamento
        Some(layout_pane_id)
    }

    /// Marca que precisa de re-renderização
    pub fn mark_dirty(&mut self) {
        self.needs_render = true;
    }

    /// Verifica se precisa renderizar
    pub fn needs_render(&self) -> bool {
        self.needs_render
    }

    // ========== ACESSORES ==========

    /// Retorna o gerenciador de attach
    pub fn attach_manager(&self) -> &AttachManager {
        &self.attach_manager
    }

    /// Retorna o gerenciador de attach mutável
    pub fn attach_manager_mut(&mut self) -> &mut AttachManager {
        &mut self.attach_manager
    }

    /// Retorna a integração de sessão
    pub fn session_integration(&self) -> &SessionPaneIntegration {
        &self.session_integration
    }

    /// Retorna a integração de sessão mutável
    pub fn session_integration_mut(&mut self) -> &mut SessionPaneIntegration {
        &mut self.session_integration
    }

    /// Retorna o renderer
    pub fn renderer(&self) -> Option<&PaneRenderer> {
        self.pane_renderer.as_ref()
    }

    /// Retorna o renderer mutável
    pub fn renderer_mut(&mut self) -> Option<&mut PaneRenderer> {
        self.pane_renderer.as_mut()
    }

    /// Lista sessões ativas em renderização
    pub fn active_sessions(&self) -> Vec<&RenderSessionState> {
        self.active_render_sessions.values().collect()
    }

    /// Retorna estado de renderização de uma sessão
    pub fn get_session_state(&self, session_id: &str) -> Option<&RenderSessionState> {
        self.active_render_sessions.get(session_id)
    }

    /// Retorna sessões ativas em renderização (HashMap) - para API socket
    pub fn active_render_sessions(&self) -> &HashMap<String, RenderSessionState> {
        &self.active_render_sessions
    }

    /// Retorna estado de renderização de uma sessão - para API socket
    pub fn get_render_session_state(&self, session_id: &str) -> Option<&RenderSessionState> {
        self.active_render_sessions.get(session_id)
    }

    /// Marca uma sessão para renderização - para API socket
    pub fn mark_session_for_render(&mut self, session_id: &str) -> PaneResult<()> {
        if !self.active_render_sessions.contains_key(session_id) {
            return Err(PaneError::InvalidState(format!(
                "Session {} not found",
                session_id
            )));
        }

        self.needs_render = true;
        info!(session_id = %session_id, "Sessão marcada para renderização");
        Ok(())
    }

    /// Força re-renderização de uma sessão - para API socket
    pub fn force_rerender(&mut self, session_id: &str) -> PaneResult<()> {
        let state = self
            .active_render_sessions
            .get_mut(session_id)
            .ok_or_else(|| PaneError::InvalidState(format!("Session {} not found", session_id)))?;

        self.needs_render = true;

        info!(session_id = %session_id, frame = state.last_frame, "Re-renderização forçada");
        Ok(())
    }

    /// Atualiza configuração de renderização - para API socket
    pub fn update_render_config(
        &mut self,
        session_id: &str,
        config: RenderConfigUpdate,
    ) -> PaneResult<()> {
        let state = self
            .active_render_sessions
            .get_mut(session_id)
            .ok_or_else(|| PaneError::InvalidState(format!("Session {} not found", session_id)))?;

        if let Some(target_fps) = config.target_fps {
            state.render_config.target_fps = target_fps;
        }
        if let Some(quality) = config.quality {
            state.render_config.quality = quality;
        }
        if let Some(sync_mode) = config.sync_mode {
            state.render_config.sync_mode = sync_mode;
        }

        self.needs_render = true;
        info!(session_id = %session_id, "Configuração de renderização atualizada");
        Ok(())
    }
}

impl Default for RenderConfigInfo {
    fn default() -> Self {
        Self {
            target_fps: 60,
            quality: RenderQuality::High,
            sync_mode: RenderSyncMode::VSync,
            double_buffer: true,
            antialiasing: true,
        }
    }
}

impl Default for SessionRendererIntegration {
    fn default() -> Self {
        Self::new()
    }
}

/// Resultado de attach com rendering
#[derive(Debug, Clone)]
pub struct AttachRenderResult {
    /// IDs dos panes
    pub pane_ids: Vec<PaneId>,
    /// Pane ativo
    pub active_pane: Option<PaneId>,
    /// Dados da sessão
    pub session_data: serde_json::Value,
    /// ID da sessão de renderização
    pub render_session_id: String,
}

/// Thread-safe wrapper para uso em múltiplas threads
pub struct ThreadSafeRendererIntegration {
    inner: Arc<Mutex<SessionRendererIntegration>>,
}

impl ThreadSafeRendererIntegration {
    pub fn new(integration: SessionRendererIntegration) -> Self {
        Self {
            inner: Arc::new(Mutex::new(integration)),
        }
    }

    /// Executa uma operação na integração
    pub fn with<F, R>(&self, f: F) -> PaneResult<R>
    where
        F: FnOnce(&mut SessionRendererIntegration) -> PaneResult<R>,
    {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| PaneError::InvalidState("Lock poisoned".to_string()))?;
        f(&mut guard)
    }

    /// Verifica se precisa renderizar
    pub fn needs_render(&self) -> bool {
        self.inner.lock().map(|g| g.needs_render()).unwrap_or(false)
    }

    /// Renderiza
    pub fn render(&self) -> PaneResult<()> {
        self.with(|integration| integration.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_renderer_integration_creation() {
        let integration = SessionRendererIntegration::new();
        assert!(!integration.needs_render());
        assert!(integration.active_sessions().is_empty());
    }

    #[test]
    fn test_mark_dirty() {
        let mut integration = SessionRendererIntegration::new();
        assert!(!integration.needs_render());

        integration.mark_dirty();
        assert!(integration.needs_render());
    }

    #[test]
    fn test_thread_safe_wrapper() {
        let integration = SessionRendererIntegration::new();
        let safe = ThreadSafeRendererIntegration::new(integration);

        assert!(!safe.needs_render());
    }

    #[test]
    fn test_update_render_config_applies_partial_update() {
        let mut integration = SessionRendererIntegration::new();
        integration.active_render_sessions.insert(
            "session-1".to_string(),
            RenderSessionState::new("session-1".to_string(), vec![1], Some(1)),
        );

        integration
            .update_render_config(
                "session-1",
                RenderConfigUpdate {
                    target_fps: Some(30),
                    quality: Some(RenderQuality::Low),
                    sync_mode: Some(RenderSyncMode::FixedRate),
                },
            )
            .unwrap();

        let state = integration.get_session_state("session-1").unwrap();
        assert_eq!(state.render_config.target_fps, 30);
        assert!(matches!(state.render_config.quality, RenderQuality::Low));
        assert!(matches!(
            state.render_config.sync_mode,
            RenderSyncMode::FixedRate
        ));
        assert!(integration.needs_render());
    }
}
