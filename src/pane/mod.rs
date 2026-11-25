//! Pane System - Sistema Unificado de Painéis
//!
//! Este módulo unifica as diferentes representações de Pane:
//! - multiplexer::Pane (estado de sessão)
//! - ui::layout::Pane (layout e tabs)
//! - renderer (renderização)
//!
//! Provê um trait `PaneContent` comum e uma estrutura `UnifiedPane`
//! que integra todas as funcionalidades.

use std::sync::atomic::{AtomicU64, Ordering};

pub mod browser_pane;
pub mod content;
pub mod input;
pub mod renderer_integration;
pub mod session_integration;
pub mod terminal_pane;
#[cfg(test)]
pub(crate) mod test_support;
pub mod unified;

pub use browser_pane::{BrowserPane, BrowserPaneBuilder};
pub use content::{PaneContent, PaneContentType, PaneState};
pub use terminal_pane::TerminalPane;

/// Contador global para IDs de pane
static PANE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Gera um novo ID de pane único
pub fn generate_pane_id() -> PaneId {
    PANE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Texto visível do grid, uma linha por linha do terminal.
pub(crate) fn grid_text(grid: &crate::terminal::grid::Grid) -> String {
    let (cols, rows) = grid.size();
    let cells = grid.get_cells();
    let mut text = String::new();
    for row in 0..rows {
        for col in 0..cols {
            if let Some(cell) = cells.get(row).and_then(|r| r.get(col)) {
                text.push(cell.char);
            }
        }
        if row + 1 < rows {
            text.push('\n');
        }
    }
    text
}
pub use input::{InputEvent, InputRouter, KeyCode, KeyModifiers, MouseButton, MouseEventKind};
pub use renderer_integration::{
    AttachRenderResult, RenderSessionState, RendererCommand, SessionRendererIntegration,
    ThreadSafeRendererIntegration,
};
pub use session_integration::{
    AttachManager, AttachResult, ClientAttachContext, SessionPaneIntegration,
};
pub use unified::{RenderFlags, UnifiedPane, UnifiedPaneId, UnifiedPaneManager};

/// Re-exportação do PaneId do crate::types
///
/// Use este tipo para IDs de painel em todo o sistema.
pub use crate::types::PaneId;

/// Metadados de um pane
#[derive(Debug, Clone, Default)]
pub struct PaneMetadata {
    /// Título do pane
    pub title: String,
    /// Diretório de trabalho
    pub cwd: Option<String>,
    /// Comando em execução
    pub command: Option<String>,
    /// PID do processo
    pub pid: Option<u32>,
    /// Dados customizados
    pub custom: std::collections::HashMap<String, String>,
}

/// Informações de renderização de um pane
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderInfo {
    /// Posição na tela (x, y) em pixels
    pub position: (u32, u32),
    /// Tamanho (width, height) em pixels
    pub size: (u32, u32),
    /// Se o pane está ativo (focado)
    pub is_active: bool,
    /// Se precisa ser re-renderizado
    pub is_dirty: bool,
}

impl RenderInfo {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            position: (x, y),
            size: (width, height),
            is_active: false,
            is_dirty: true,
        }
    }

    /// Retorna o retângulo de renderização
    pub fn rect(&self) -> crate::ui::layout::Rect {
        crate::ui::layout::Rect::new(self.position.0, self.position.1, self.size.0, self.size.1)
    }
}

/// Resultado de uma operação de pane
pub type PaneResult<T> = Result<T, PaneError>;

/// Erros do sistema de pane
#[derive(Debug, thiserror::Error)]
pub enum PaneError {
    #[error("Pane not found: {0}")]
    NotFound(PaneId),

    #[error("Pane already exists: {0}")]
    AlreadyExists(PaneId),

    #[error("Invalid pane state: {0}")]
    InvalidState(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("PTY error: {0}")]
    Pty(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_id_creation() {
        // PaneId agora é u64, IDs são atribuídos sequencialmente
        let id1: PaneId = 1;
        let id2: PaneId = 2;
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_pane_id_display() {
        let id: PaneId = 42;
        let s = format!("{}", id);
        assert_eq!(s, "42");
    }

    #[test]
    fn test_render_info() {
        let info = RenderInfo::new(10, 20, 800, 600);
        assert_eq!(info.position, (10, 20));
        assert_eq!(info.size, (800, 600));
        assert!(info.is_dirty);
    }
}
