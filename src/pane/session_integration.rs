//! Session Integration - Integração Multiplexer com Pane Unificado
//!
//! Este módulo fornece a integração entre:
//! - Sistema de sessões do multiplexer (attach/detach)
//! - Pane unificado (UnifiedPane)
//! - Renderização real

use super::content::PaneState;
use super::{PaneId, PaneResult, UnifiedPaneManager};
use crate::multiplexer::pane::{
    Pane as MultiplexerPane, PaneManager as MpPaneManager, PaneState as MpPaneState,
};
use crate::multiplexer::MultiplexerSession;
use crate::renderer::pane::PaneRenderer;
use crate::session::{LayoutData, PaneData, SessionData, SplitData, TabData};
use std::collections::HashMap;

/// Integração entre sessão do multiplexer e panes unificados
pub struct SessionPaneIntegration {
    /// Gerenciador de panes unificados
    pane_manager: UnifiedPaneManager,
    /// Mapeamento: Session ID -> Pane IDs
    session_panes: HashMap<String, Vec<PaneId>>,
    /// Mapeamento: Multiplexer Pane ID -> Unified Pane ID  
    pane_id_mapping: HashMap<crate::multiplexer::pane::PaneId, PaneId>,
    /// Renderer de panes
    renderer: Option<PaneRenderer>,
}

impl SessionPaneIntegration {
    /// Cria uma nova integração
    pub fn new() -> Self {
        Self {
            pane_manager: UnifiedPaneManager::new(),
            session_panes: HashMap::new(),
            pane_id_mapping: HashMap::new(),
            renderer: None,
        }
    }

    /// Configura o renderer
    pub fn with_renderer(mut self, renderer: PaneRenderer) -> Self {
        self.renderer = Some(renderer);
        self
    }

    /// Cria um pane unificado a partir de um pane do multiplexer
    pub fn create_pane_from_multiplexer(
        &mut self,
        mp_pane: &MultiplexerPane,
    ) -> PaneResult<PaneId> {
        let (cols, rows) = mp_pane.size;
        let unified_id = self.pane_manager.create_pane(cols, rows)?;

        // Configura o pane unificado com dados do multiplexer
        if let Some(unified) = self.pane_manager.get_pane_mut(unified_id) {
            unified.metadata.title = mp_pane.title.clone();
            unified.metadata.command = mp_pane.command.clone();
            unified.metadata.pid = mp_pane.pid;

            // Converte estado
            unified.state = match mp_pane.state {
                MpPaneState::Active => PaneState::Active,
                MpPaneState::Background => PaneState::Background,
                MpPaneState::Zoomed => PaneState::Zoomed,
                MpPaneState::Destroyed => PaneState::Destroyed,
            };

            // Posição
            unified.move_to(mp_pane.position.0 as u32, mp_pane.position.1 as u32);

            // Metadados adicionais
            if let Some(cwd) = mp_pane.metadata.get("cwd") {
                unified.metadata.cwd = Some(cwd.clone());
            }
        }

        // Registra o mapeamento
        self.pane_id_mapping.insert(mp_pane.id, unified_id);

        Ok(unified_id)
    }

    /// Sincroniza estado do multiplexer para panes unificados
    pub fn sync_from_multiplexer(&mut self, mp_manager: &MpPaneManager) -> PaneResult<()> {
        for mp_pane in mp_manager.list_panes() {
            if let Some(&unified_id) = self.pane_id_mapping.get(&mp_pane.id) {
                if let Some(unified) = self.pane_manager.get_pane_mut(unified_id) {
                    // Atualiza estado
                    unified.state = match mp_pane.state {
                        MpPaneState::Active => PaneState::Active,
                        MpPaneState::Background => PaneState::Background,
                        MpPaneState::Zoomed => PaneState::Zoomed,
                        MpPaneState::Destroyed => PaneState::Destroyed,
                    };

                    // Atualiza posição e tamanho
                    unified.cell_size = mp_pane.size;
                    unified.move_to(mp_pane.position.0 as u32, mp_pane.position.1 as u32);

                    // Atualiza metadados
                    unified.metadata.title = mp_pane.title.clone();
                }
            } else {
                // Pane novo no multiplexer, cria no unificado
                self.create_pane_from_multiplexer(mp_pane)?;
            }
        }

        Ok(())
    }

    /// Sincroniza estado dos panes unificados para o multiplexer
    pub fn sync_to_multiplexer(&self, mp_manager: &mut MpPaneManager) -> PaneResult<()> {
        for (mp_id, unified_id) in &self.pane_id_mapping {
            if let Some(unified) = self.pane_manager.get_pane(*unified_id) {
                if let Some(mp_pane) = mp_manager.get_pane_mut(*mp_id) {
                    // Atualiza estado
                    mp_pane.state = match unified.state {
                        PaneState::Active => MpPaneState::Active,
                        PaneState::Background => MpPaneState::Background,
                        PaneState::Zoomed => MpPaneState::Zoomed,
                        PaneState::Minimized | PaneState::Destroyed => MpPaneState::Destroyed,
                    };

                    // Atualiza posição e tamanho
                    mp_pane.size = unified.cell_size;
                    mp_pane.position = (unified.rect.x as u16, unified.rect.y as u16);

                    // Atualiza scrollback se necessário
                    // TODO: Sincronizar conteúdo do terminal
                }
            }
        }

        Ok(())
    }

    /// Anexa uma sessão existente
    ///
    /// Quando um cliente se conecta a uma sessão existente,
    /// restaura os panes da sessão
    pub fn attach_session(&mut self, session: &MultiplexerSession) -> PaneResult<Vec<PaneId>> {
        let pane_ids = Vec::new();

        // Recupera panes da sessão
        // Nota: MultiplexerSession não tem campo panes atualmente
        // Isso precisa ser implementado quando a estrutura for atualizada
        for _mp_pane in Vec::<MultiplexerPane>::new() {
            // let unified_id = self.create_pane_from_multiplexer(mp_pane)?;
            // pane_ids.push(unified_id);
        }

        // Registra na sessão
        self.session_panes
            .insert(session.id.to_string(), pane_ids.clone());

        // Ativa o primeiro pane
        if let Some(&first_id) = pane_ids.first() {
            self.pane_manager.set_active_pane(first_id)?;
        }

        Ok(pane_ids)
    }

    /// Restaura panes renderizáveis a partir de uma sessão persistida.
    pub fn restore_session_data(
        &mut self,
        session_id: &str,
        data: &SessionData,
    ) -> PaneResult<Vec<PaneId>> {
        let persisted_panes = collect_layout_panes(&data.layout);
        let terminal_size = data.config.terminal_size.unwrap_or((80, 24));
        let mut pane_ids = Vec::new();
        let mut active_unified_id = None;

        if persisted_panes.is_empty() {
            let id = self
                .pane_manager
                .create_pane(terminal_size.0, terminal_size.1)?;
            if let Some(pane) = self.pane_manager.get_pane_mut(id) {
                pane.metadata.title = session_id.to_string();
                pane.metadata.cwd = data
                    .working_directory
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string());
                pane.metadata.command = data.config.shell.clone();
            }
            pane_ids.push(id);
            active_unified_id = Some(id);
        } else {
            for pane_data in persisted_panes {
                let id = self
                    .pane_manager
                    .create_pane(terminal_size.0, terminal_size.1)?;
                apply_persisted_pane_data(
                    self.pane_manager.get_pane_mut(id),
                    pane_data,
                    data,
                    session_id,
                );

                if data.layout.active_pane == Some(pane_data.id) {
                    active_unified_id = Some(id);
                }

                pane_ids.push(id);
            }
        }

        self.session_panes
            .insert(session_id.to_string(), pane_ids.clone());

        if let Some(active_id) = active_unified_id.or_else(|| pane_ids.first().copied()) {
            self.pane_manager.set_active_pane(active_id)?;
        }

        Ok(pane_ids)
    }

    /// Desanexa de uma sessão
    ///
    /// Quando um cliente se desconecta, preserva o estado
    /// mas remove da visualização atual
    pub fn detach_session(&mut self, session_id: &str) -> PaneResult<()> {
        if let Some(pane_ids) = self.session_panes.remove(session_id) {
            // Marca todos os panes como background
            for id in pane_ids {
                if let Some(pane) = self.pane_manager.get_pane_mut(id) {
                    pane.state = PaneState::Background;
                }
            }
        }

        Ok(())
    }

    /// Prepara para sessão em background
    ///
    /// Serializa o estado para persistência
    pub fn prepare_for_background(&self, session_id: &str) -> PaneResult<serde_json::Value> {
        let pane_ids = self.session_panes.get(session_id).ok_or_else(|| {
            super::PaneError::InvalidState(format!("Session {} not found", session_id))
        })?;

        let panes_data: Vec<_> = pane_ids
            .iter()
            .filter_map(|id| self.pane_manager.get_pane(*id))
            .map(|pane| pane.serialize())
            .collect();

        Ok(serde_json::json!({
            "session_id": session_id,
            "panes": panes_data,
            "active_pane": self.pane_manager.active_pane().map(|p| p.id.to_string()),
        }))
    }

    /// Restaura de sessão em background
    ///
    /// Recupera o estado após um attach
    pub fn restore_from_background(
        &mut self,
        session_id: &str,
        data: &serde_json::Value,
    ) -> PaneResult<Vec<PaneId>> {
        let panes_array = data
            .get("panes")
            .and_then(|p| p.as_array())
            .ok_or_else(|| super::PaneError::InvalidState("Invalid session data".to_string()))?;

        let mut pane_ids = Vec::new();

        for pane_data in panes_array {
            // TODO: Criar pane a partir dos dados serializados
            // Por enquanto, cria um novo
            let cols = pane_data
                .get("cell_size")
                .and_then(|s| s.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_u64())
                .unwrap_or(80) as u16;

            let rows = pane_data
                .get("cell_size")
                .and_then(|s| s.as_array())
                .and_then(|a| a.get(1))
                .and_then(|v| v.as_u64())
                .unwrap_or(24) as u16;

            let id = self.pane_manager.create_pane(cols, rows)?;
            pane_ids.push(id);

            // Restaura metadados
            if let Some(pane) = self.pane_manager.get_pane_mut(id) {
                if let Some(metadata) = pane_data.get("metadata") {
                    if let Some(title) = metadata.get("title").and_then(|t| t.as_str()) {
                        pane.metadata.title = title.to_string();
                    }
                    if let Some(cwd) = metadata.get("cwd").and_then(|c| c.as_str()) {
                        pane.metadata.cwd = Some(cwd.to_string());
                    }
                }
            }
        }

        self.session_panes
            .insert(session_id.to_string(), pane_ids.clone());

        Ok(pane_ids)
    }

    /// Renderiza todos os panes visíveis
    pub fn render(&mut self) -> PaneResult<()> {
        // Atualiza estado de todos os panes
        self.pane_manager.update_all();

        // Marca todos como dirty se necessário
        for pane in self.pane_manager.list_panes() {
            if pane.state == PaneState::Active || pane.state == PaneState::Background {
                // TODO: Chamar renderer real
            }
        }

        Ok(())
    }

    /// Retorna referência ao gerenciador de panes
    pub fn pane_manager(&self) -> &UnifiedPaneManager {
        &self.pane_manager
    }

    /// Retorna referência mutável ao gerenciador de panes
    pub fn pane_manager_mut(&mut self) -> &mut UnifiedPaneManager {
        &mut self.pane_manager
    }

    /// Retorna o mapeamento de IDs
    pub fn pane_mapping(&self) -> &HashMap<crate::multiplexer::pane::PaneId, PaneId> {
        &self.pane_id_mapping
    }

    /// Converte ID do multiplexer para ID unificado
    pub fn mp_to_unified(&self, mp_id: crate::multiplexer::pane::PaneId) -> Option<PaneId> {
        self.pane_id_mapping.get(&mp_id).copied()
    }

    /// Converte ID unificado para ID do multiplexer
    pub fn unified_to_mp(&self, unified_id: PaneId) -> Option<crate::multiplexer::pane::PaneId> {
        self.pane_id_mapping
            .iter()
            .find(|(_, &v)| v == unified_id)
            .map(|(k, _)| *k)
    }
}

fn collect_layout_panes(layout: &LayoutData) -> Vec<&PaneData> {
    let mut panes = Vec::new();
    panes.extend(layout.panes.iter());
    if let Some(root_split) = &layout.root_split {
        collect_split_panes(root_split, &mut panes);
    }
    panes
}

fn collect_split_panes<'a>(split: &'a SplitData, panes: &mut Vec<&'a PaneData>) {
    match split {
        SplitData::Pane(pane) => panes.push(pane),
        SplitData::Split { first, second, .. } => {
            collect_split_panes(first, panes);
            collect_split_panes(second, panes);
        }
    }
}

fn apply_persisted_pane_data(
    pane: Option<&mut super::UnifiedPane>,
    pane_data: &PaneData,
    data: &SessionData,
    fallback_title: &str,
) {
    let Some(pane) = pane else {
        return;
    };

    pane.move_to(pane_data.rect.x, pane_data.rect.y);
    pane.set_size(pane_data.rect.width, pane_data.rect.height);

    let active_or_first_tab = pane_data
        .active_tab
        .and_then(|active_id| pane_data.tabs.iter().find(|tab| tab.id == active_id))
        .or_else(|| pane_data.tabs.first());

    pane.metadata.title = active_or_first_tab
        .map(|tab| tab.title.clone())
        .unwrap_or_else(|| fallback_title.to_string());
    pane.metadata.cwd = active_or_first_tab
        .and_then(tab_working_directory)
        .or_else(|| {
            data.working_directory
                .as_ref()
                .map(|path| path.to_string_lossy().to_string())
        });
    pane.metadata.command = active_or_first_tab
        .and_then(|tab| tab.command.clone())
        .or_else(|| data.config.shell.clone());
}

fn tab_working_directory(tab: &TabData) -> Option<String> {
    tab.working_directory
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
}

impl Default for SessionPaneIntegration {
    fn default() -> Self {
        Self::new()
    }
}

/// Estrutura para attach de cliente
pub struct ClientAttachContext {
    /// ID do cliente
    pub client_id: String,
    /// ID da sessão
    pub session_id: String,
    /// Tamanho do terminal do cliente
    pub terminal_size: (u16, u16),
    /// Features suportadas
    pub features: Vec<String>,
}

/// Resultado de um attach
pub struct AttachResult {
    /// IDs dos panes criados/restaurados
    pub pane_ids: Vec<PaneId>,
    /// Pane ativo
    pub active_pane: Option<PaneId>,
    /// Dados da sessão
    pub session_data: serde_json::Value,
}

/// Gerenciador de attach/detach
pub struct AttachManager {
    integration: SessionPaneIntegration,
    active_sessions: HashMap<String, ClientAttachContext>,
}

impl AttachManager {
    pub fn new() -> Self {
        Self {
            integration: SessionPaneIntegration::new(),
            active_sessions: HashMap::new(),
        }
    }

    /// Realiza attach de um cliente
    pub fn attach(
        &mut self,
        context: ClientAttachContext,
        session: &MultiplexerSession,
    ) -> PaneResult<AttachResult> {
        // Registra sessão ativa
        let session_id = context.session_id.clone();
        self.active_sessions.insert(session_id.clone(), context);

        // Realiza attach
        let pane_ids = self.integration.attach_session(session)?;
        let active_pane = self.integration.pane_manager().active_pane().map(|p| p.id);

        // Prepara dados da sessão
        let session_data = self.integration.prepare_for_background(&session_id)?;

        Ok(AttachResult {
            pane_ids,
            active_pane,
            session_data,
        })
    }

    /// Realiza detach de um cliente
    pub fn detach(&mut self, session_id: &str) -> PaneResult<serde_json::Value> {
        // Remove da lista de ativas
        self.active_sessions.remove(session_id);

        // Prepara dados para background
        let data = self.integration.prepare_for_background(session_id)?;

        // Realiza detach
        self.integration.detach_session(session_id)?;

        Ok(data)
    }

    /// Verifica se uma sessão está ativa
    pub fn is_session_active(&self, session_id: &str) -> bool {
        self.active_sessions.contains_key(session_id)
    }

    /// Lista sessões ativas
    pub fn active_session_ids(&self) -> Vec<String> {
        self.active_sessions.keys().cloned().collect()
    }

    /// Retorna a integração
    pub fn integration(&self) -> &SessionPaneIntegration {
        &self.integration
    }

    /// Retorna a integração mutável
    pub fn integration_mut(&mut self) -> &mut SessionPaneIntegration {
        &mut self.integration
    }
}

impl Default for AttachManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Testes requerem setup complexo do multiplexer
    // Estes são testes básicos de estrutura

    #[test]
    fn test_session_integration_creation() {
        let integration = SessionPaneIntegration::new();
        assert_eq!(integration.pane_manager().pane_count(), 0);
    }

    #[test]
    fn test_attach_manager_creation() {
        let manager = AttachManager::new();
        assert!(manager.active_session_ids().is_empty());
        assert!(!manager.is_session_active("test"));
    }

    #[test]
    fn test_client_attach_context() {
        let context = ClientAttachContext {
            client_id: "client-1".to_string(),
            session_id: "session-1".to_string(),
            terminal_size: (80, 24),
            features: vec!["256color".to_string(), "utf8".to_string()],
        };

        assert_eq!(context.client_id, "client-1");
        assert_eq!(context.terminal_size, (80, 24));
    }
}
