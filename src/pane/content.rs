//! PaneContent Trait - Interface comum para conteúdo de pane
//!
//! Define o trait que todo conteúdo de pane deve implementar,
//! permitindo que o sistema trabalhe com diferentes tipos de
//! conteúdo (terminal, logs, monitor, etc) de forma uniforme.

use super::{PaneId, PaneMetadata, PaneResult, RenderInfo};
use crate::terminal::grid::Grid;

/// Estado de um pane
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaneState {
    /// Ativo e recebendo input
    Active,
    /// Em background (visível mas não focado)
    #[default]
    Background,
    /// Em zoom (ocupa toda a janela)
    Zoomed,
    /// Minimizado
    Minimized,
    /// Destruído/pendente de remoção
    Destroyed,
}

/// Tipo de conteúdo do pane
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneContentType {
    /// Shell/Terminal padrão
    Shell,
    /// Comando específico
    Command,
    /// Visualizador de log
    LogViewer,
    /// Monitor de sistema
    SystemMonitor,
    /// Saída de build
    BuildOutput,
    /// Custom (plugin/extensão)
    Custom(&'static str),
}

/// Trait que define a interface comum para todo conteúdo de pane
///
/// Implementado por:
/// - TerminalBuffer (para terminais normais)
/// - LogViewer (para visualização de logs)
/// - SystemMonitor (para monitoramento)
/// - etc.
pub trait PaneContent: Send + Sync {
    /// Retorna o ID do pane
    fn pane_id(&self) -> PaneId;

    /// Retorna o tipo de conteúdo
    fn content_type(&self) -> PaneContentType;

    /// Retorna o estado atual
    fn state(&self) -> PaneState;

    /// Define o estado
    fn set_state(&mut self, state: PaneState);

    /// Retorna os metadados
    fn metadata(&self) -> &PaneMetadata;

    /// Atualiza os metadados
    fn set_metadata(&mut self, metadata: PaneMetadata);

    /// Retorna informações de renderização
    fn render_info(&self) -> RenderInfo;

    /// Atualiza informações de renderização
    fn set_render_info(&mut self, info: RenderInfo);

    /// Marca o pane como needing re-render
    fn mark_dirty(&mut self);

    /// Marca o pane como limpo (após renderização)
    fn mark_clean(&mut self);

    /// Verifica se o pane está dirty
    fn is_dirty(&self) -> bool;

    /// Retorna o grid de terminal (se aplicável)
    fn grid(&self) -> Option<&Grid> {
        None
    }

    /// Retorna o grid mutável (se aplicável)
    fn grid_mut(&mut self) -> Option<&mut Grid> {
        None
    }

    /// Processa input do usuário
    /// Retorna true se o input foi consumido
    fn process_input(&mut self, input: &crate::pane::input::InputEvent) -> PaneResult<bool>;

    /// Atualiza o conteúdo (chamado no frame)
    fn update(&mut self) -> PaneResult<()>;

    /// Retorna o conteúdo como string (para cópia/seleção)
    fn get_content_text(&self) -> String;

    /// Retorna texto selecionado
    fn get_selection_text(&self) -> Option<String> {
        None
    }

    /// Define seleção
    fn set_selection(&mut self, start: (u16, u16), end: (u16, u16)) {
        let _ = (start, end);
    }

    /// Limpa seleção
    fn clear_selection(&mut self) {}

    /// Retorna a posição do cursor (col, row)
    fn cursor_position(&self) -> (u16, u16) {
        (0, 0)
    }

    /// Define a posição do cursor
    fn set_cursor_position(&mut self, col: u16, row: u16) {
        let _ = (col, row);
    }

    /// Retorna o tamanho do conteúdo em células (cols, rows)
    fn content_size(&self) -> (u16, u16);

    /// Redimensiona o conteúdo
    fn resize(&mut self, cols: u16, rows: u16) -> PaneResult<()>;

    /// Scroll para cima
    fn scroll_up(&mut self, lines: usize);

    /// Scroll para baixo
    fn scroll_down(&mut self, lines: usize);

    /// Scroll para o topo
    fn scroll_to_top(&mut self);

    /// Scroll para o final
    fn scroll_to_bottom(&mut self);

    /// Retorna a posição de scroll atual
    fn scroll_position(&self) -> usize {
        0
    }

    /// Entra em modo de cópia
    fn enter_copy_mode(&mut self) {}

    /// Sai do modo de cópia
    fn exit_copy_mode(&mut self) {}

    /// Verifica se está em modo de cópia
    fn is_in_copy_mode(&self) -> bool {
        false
    }

    /// Serializa o estado para persistência
    fn serialize(&self) -> serde_json::Value {
        serde_json::json!({
            "pane_id": self.pane_id().to_string(),
            "content_type": format!("{:?}", self.content_type()),
            "state": format!("{:?}", self.state()),
        })
    }

    /// Deserializa o estado (implementação opcional)
    fn deserialize(&mut self, _data: &serde_json::Value) -> PaneResult<()> {
        Ok(())
    }
}

// NOTA: TerminalBuffer NÃO implementa PaneContent diretamente
// porque o ciclo de vida do PTY deve ser mediado por TerminalPane.
// Para usar TerminalBuffer como conteúdo de pane, use o TerminalPane
// do módulo terminal_pane ou o UnifiedPane que gerencia o TerminalBuffer internamente

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_state_default() {
        let state = PaneState::default();
        assert_eq!(state, PaneState::Background);
    }

    #[test]
    fn test_pane_content_type_variants() {
        assert_eq!(format!("{:?}", PaneContentType::Shell), "Shell");
        assert_eq!(format!("{:?}", PaneContentType::Command), "Command");
        assert_eq!(format!("{:?}", PaneContentType::LogViewer), "LogViewer");
    }
}
