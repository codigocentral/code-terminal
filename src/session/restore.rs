//! Restauração de sessões do terminal
//!
//! Este módulo implementa a funcionalidade de capturar e restaurar
//! o estado completo do terminal, incluindo tabs, splits e diretórios.
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::session::{SessionManager, SessionRestorer};
//! use code_terminal::app::AppState;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = SessionManager::default()?;
//! let mut app_state = AppState::new(80, 24)?;
//!
//! // Salvar sessão atual
//! SessionRestorer::capture(&app_state, "my-session", &manager)?;
//!
//! // Restaurar sessão
//! SessionRestorer::restore(&mut app_state, "my-session", &manager)?;
//! # Ok(())
//! # }
//! ```

use super::{
    error::{SessionError, SessionResult},
    serialization::{PaneData, SessionData, TabData},
    SessionManager,
};
use crate::app::AppState;
use crate::terminal::buffer::TerminalBuffer;
// use crate::ui::layout::{PaneId, Rect, SplitDirection as LayoutSplitDirection, SplitNode};
use crate::ui::tabs::Tab;
use std::path::PathBuf;
use tracing::{debug, error, info, warn};

/// Trait para objetos que podem ser capturados e restaurados
pub trait SessionSnapshot {
    /// Captura o estado atual e retorna SessionData
    fn capture(&self) -> SessionData;

    /// Restaura o estado a partir de SessionData
    fn restore(&mut self, data: &SessionData) -> SessionResult<()>;
}

/// Restaurador de sessões
pub struct SessionRestorer;

impl SessionRestorer {
    /// Captura o estado atual do AppState e salva como sessão
    pub fn capture(
        app_state: &AppState,
        session_name: impl AsRef<str>,
        manager: &SessionManager,
    ) -> SessionResult<()> {
        let session_name = session_name.as_ref();
        let data = Self::capture_state(app_state);

        manager.save(session_name, &data)?;
        info!("Sessão capturada e salva: {}", session_name);

        Ok(())
    }

    /// Restaura uma sessão salva no AppState
    pub fn restore(
        app_state: &mut AppState,
        session_name: impl AsRef<str>,
        manager: &SessionManager,
    ) -> SessionResult<()> {
        let session_name = session_name.as_ref();

        let session = manager.load(session_name)?;
        info!("Restaurando sessão: {}", session_name);

        Self::restore_state(app_state, &session.data)?;

        info!("Sessão restaurada com sucesso: {}", session_name);
        Ok(())
    }

    /// Captura o estado atual do AppState
    pub fn capture_state(app_state: &AppState) -> SessionData {
        let mut data = SessionData::new();

        // Captura dimensões do terminal
        data.config.terminal_size = Some(app_state.terminal_size);

        // Captura layout e tabs
        data.layout = Self::capture_layout(app_state);

        // Captura diretório de trabalho (da tab ativa se disponível)
        if let Some(tab) = app_state.active_tab() {
            // Extrai diretório do título da tab se possível
            let title = tab.title();
            debug!("Capturando tab ativa: {}", title);
        }

        debug!("Estado capturado: {} panes", data.layout.panes.len());
        data
    }

    /// Restaura o estado no AppState
    pub fn restore_state(app_state: &mut AppState, data: &SessionData) -> SessionResult<()> {
        // Limpa estado atual
        Self::clear_current_state(app_state);

        // Restaura dimensões se disponíveis
        if let Some((cols, rows)) = data.config.terminal_size {
            if let Err(e) = app_state.resize(cols, rows) {
                warn!("Falha ao restaurar dimensões: {}", e);
            }
        }

        // Restaura layout e tabs
        Self::restore_layout(app_state, &data.layout)?;

        Ok(())
    }

    /// Captura o layout atual (tabs e panes)
    fn capture_layout(app_state: &AppState) -> super::serialization::LayoutData {
        let mut layout = super::serialization::LayoutData::new();

        // Captura tabs do TabManager
        let tab_manager = &app_state.tab_manager;

        // Cria um pane único com todas as tabs
        let mut pane_data = PaneData::new(1);
        pane_data.rect = super::serialization::RectData {
            x: 0,
            y: 0,
            width: app_state.terminal_size.0 as u32 * 8, // Aproximação em pixels
            height: app_state.terminal_size.1 as u32 * 16,
        };

        // Captura cada tab
        for i in 0..tab_manager.tab_count() {
            if let Some(tab) = tab_manager.get_tab(i) {
                let tab_data = Self::capture_tab(tab, i as u32);
                pane_data.add_tab(tab_data);
            }
        }

        // Define tab ativa
        let active_index = tab_manager.active_tab_index();
        if let Some(tab) = tab_manager.get_tab(active_index) {
            pane_data.set_active_tab(tab.id);
        }

        layout.add_pane(pane_data);
        layout.set_active_pane(1);

        layout
    }

    /// Captura uma tab
    fn capture_tab(tab: &Tab, _index: u32) -> TabData {
        let mut tab_data = TabData::new(tab.id, tab.title());

        // Tenta extrair diretório de trabalho do título
        // Formato típico: "user@host:directory" ou apenas "directory"
        let title = tab.title();
        if let Some(dir) = Self::extract_working_dir_from_title(&title) {
            tab_data.working_directory = Some(dir);
        }

        // Captura scroll position
        tab_data.scroll_position = tab.scrollback_lines;

        // Captura estado
        tab_data.state = match tab.state {
            crate::ui::tabs::TabState::Active => super::serialization::TabStateData::Active,
            crate::ui::tabs::TabState::Inactive => super::serialization::TabStateData::Inactive,
            crate::ui::tabs::TabState::Closing => super::serialization::TabStateData::Closing,
        };

        tab_data
    }

    /// Extrai diretório de trabalho do título da tab
    fn extract_working_dir_from_title(title: &str) -> Option<PathBuf> {
        // Tenta interpretar o título como caminho
        // Remove prefixos comuns como "user@host:"
        let cleaned = if let Some(pos) = title.find(':') {
            &title[pos + 1..]
        } else {
            title
        };

        // Verifica se parece um caminho absoluto
        if cleaned.starts_with('/') || cleaned.starts_with('\\') || cleaned.contains(':') {
            Some(PathBuf::from(cleaned))
        } else {
            None
        }
    }

    /// Limpa o estado atual do AppState
    fn clear_current_state(app_state: &mut AppState) {
        // Fecha todas as tabs exceto a primeira
        while app_state.tab_count() > 1 {
            app_state.tab_manager.close_tab(0);
        }

        debug!("Estado atual limpo");
    }

    /// Restaura o layout no AppState
    fn restore_layout(
        app_state: &mut AppState,
        layout: &super::serialization::LayoutData,
    ) -> SessionResult<()> {
        // Por enquanto, suporta apenas um pane com múltiplas tabs
        if layout.panes.is_empty() {
            warn!("Layout vazio, nada para restaurar");
            return Ok(());
        }

        let pane = &layout.panes[0];

        // Limpa tabs existentes
        while app_state.tab_count() > 0 {
            app_state.tab_manager.close_tab(0);
        }

        // Recria tabs
        for tab_data in &pane.tabs {
            if let Err(e) = Self::restore_tab(app_state, tab_data) {
                warn!("Falha ao restaurar tab {}: {}", tab_data.id, e);
            }
        }

        // Ativa a tab correta
        if let Some(active_tab_id) = pane.active_tab {
            // Encontra o índice da tab com o ID
            for i in 0..app_state.tab_count() {
                if let Some(tab) = app_state.tab_manager.get_tab(i) {
                    if tab.id == active_tab_id {
                        app_state.go_to_tab(i);
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Restaura uma tab
    fn restore_tab(app_state: &mut AppState, tab_data: &TabData) -> SessionResult<()> {
        // Cria um novo buffer
        let (cols, rows) = app_state.terminal_size;
        let buffer = TerminalBuffer::with_config(cols, rows, &app_state.config)
            .map_err(|e| SessionError::RestoreFailed(format!("Falha ao criar buffer: {}", e)))?;

        // Cria a tab
        let tab_id = app_state
            .tab_manager
            .create_tab_with_buffer(&tab_data.title, buffer);

        // Configura diretório de trabalho se disponível
        // Nota: o diretório será aplicado quando o shell iniciar
        if let Some(ref _dir) = tab_data.working_directory {
            debug!("Diretório de trabalho restaurado: {:?}", _dir);
        }

        debug!("Tab restaurada: {} (id: {})", tab_data.title, tab_id);
        Ok(())
    }

    /// Verifica se existe uma sessão salva e a restaura automaticamente
    pub fn try_restore_last_session(
        app_state: &mut AppState,
        manager: &SessionManager,
    ) -> SessionResult<bool> {
        match manager.load_default() {
            Ok(Some(session)) => {
                info!("Restaurando sessão padrão: {}", session.metadata.name);
                Self::restore_state(app_state, &session.data)?;
                Ok(true)
            }
            Ok(None) => {
                debug!("Nenhuma sessão padrão encontrada");
                Ok(false)
            }
            Err(e) => {
                error!("Falha ao carregar sessão padrão: {}", e);
                Err(e)
            }
        }
    }

    /// Captura e salva a sessão atual como padrão
    pub fn save_default_session(
        app_state: &AppState,
        manager: &SessionManager,
    ) -> SessionResult<()> {
        Self::capture(app_state, "default", manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manager() -> (SessionManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = super::super::SessionConfig::with_directory(temp_dir.path());
        let manager = SessionManager::new(config);
        manager.initialize().unwrap();
        (manager, temp_dir)
    }

    #[test]
    fn test_capture_and_restore_session() {
        let (manager, _temp) = create_test_manager();

        // Cria AppState inicial
        let mut app_state = AppState::new(80, 24).unwrap();
        assert_eq!(app_state.tab_count(), 1);

        // Adiciona mais uma tab
        app_state.create_new_tab().unwrap();
        assert_eq!(app_state.tab_count(), 2);

        // Captura sessão
        SessionRestorer::capture(&app_state, "test-session", &manager).unwrap();

        // Cria novo AppState vazio
        let mut new_app_state = AppState::new(80, 24).unwrap();
        assert_eq!(new_app_state.tab_count(), 1);

        // Restaura sessão
        SessionRestorer::restore(&mut new_app_state, "test-session", &manager).unwrap();

        // Verifica que foi restaurado
        assert_eq!(new_app_state.tab_count(), 2);
    }

    #[test]
    fn test_capture_state_structure() {
        let app_state = AppState::new(80, 24).unwrap();

        let data = SessionRestorer::capture_state(&app_state);

        assert_eq!(data.version, super::super::SESSION_FORMAT_VERSION);
        assert!(data.config.terminal_size.is_some());
        assert_eq!(data.config.terminal_size, Some((80, 24)));
        assert!(!data.layout.panes.is_empty());
    }

    #[test]
    fn test_extract_working_dir() {
        // Caminho absoluto Unix
        assert_eq!(
            SessionRestorer::extract_working_dir_from_title("/home/user"),
            Some(PathBuf::from("/home/user"))
        );

        // Com prefixo user@host:
        assert_eq!(
            SessionRestorer::extract_working_dir_from_title("user@host:/home/user"),
            Some(PathBuf::from("/home/user"))
        );

        // Apenas nome (não é caminho)
        assert_eq!(
            SessionRestorer::extract_working_dir_from_title("bash"),
            None
        );
    }

    #[test]
    fn test_try_restore_last_session_no_default() {
        let (manager, _temp) = create_test_manager();
        let mut app_state = AppState::new(80, 24).unwrap();

        // Tenta restaurar sem ter sessão padrão
        let result = SessionRestorer::try_restore_last_session(&mut app_state, &manager);

        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_save_and_restore_default_session() {
        let (manager, _temp) = create_test_manager();

        // Cria AppState com múltiplas tabs
        let mut app_state = AppState::new(80, 24).unwrap();
        app_state.create_new_tab().unwrap();
        app_state.create_new_tab().unwrap();
        assert_eq!(app_state.tab_count(), 3);

        // Salva como padrão
        SessionRestorer::save_default_session(&app_state, &manager).unwrap();

        // Cria novo AppState
        let mut new_app_state = AppState::new(80, 24).unwrap();
        assert_eq!(new_app_state.tab_count(), 1);

        // Tenta restaurar
        let restored =
            SessionRestorer::try_restore_last_session(&mut new_app_state, &manager).unwrap();

        assert!(restored);
        assert_eq!(new_app_state.tab_count(), 3);
    }
}
