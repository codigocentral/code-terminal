//! Tipos compartilhados entre módulos do code-terminal
//!
//! Este módulo contém definições de tipos que são usados em múltiplos
//! módulos do projeto, garantindo consistência e evitando dependências
//! circulares.

/// Identificador único de um painel (pane)
///
/// Usado para identificar panes em todo o sistema, incluindo:
/// - Multiplexer
/// - Renderizador
/// - Sistema de notificações
/// - Layout da UI
///
/// # Exemplos
///
/// ```
/// use code_terminal::types::PaneId;
///
/// let pane_id: PaneId = 1;
/// let next_id = pane_id + 1;
/// ```
pub type PaneId = u64;

/// Identificador único de uma aba (tab)
///
/// Similar ao PaneId, mas para identificação de abas.
pub type TabId = u64;

/// Identificador único de uma janela
///
/// Usado para identificar janelas no sistema.
pub type WindowId = u64;

/// Identificador único de uma sessão
///
/// Usado para identificar sessões do multiplexador.
pub type SessionId = u64;

/// Gera um novo ID de painel a partir de um contador
///
/// Esta é uma função auxiliar para gerar IDs sequenciais.
/// Para IDs únicos globais, use um contador atômico.
///
/// # Exemplos
///
/// ```
/// use code_terminal::types::{PaneId, next_pane_id};
///
/// let mut counter: PaneId = 0;
/// let id = next_pane_id(&mut counter);
/// assert_eq!(id, 0);
/// assert_eq!(counter, 1);
/// ```
pub fn next_pane_id(counter: &mut PaneId) -> PaneId {
    let id = *counter;
    *counter += 1;
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_id_type() {
        let id: PaneId = 42;
        assert_eq!(id, 42u64);
    }

    #[test]
    fn test_next_pane_id() {
        let mut counter: PaneId = 0;
        assert_eq!(next_pane_id(&mut counter), 0);
        assert_eq!(next_pane_id(&mut counter), 1);
        assert_eq!(next_pane_id(&mut counter), 2);
        assert_eq!(counter, 3);
    }

    #[test]
    fn test_type_aliases() {
        let pane_id: PaneId = 1;
        let tab_id: TabId = 2;
        let window_id: WindowId = 3;
        let session_id: SessionId = 4;

        // Todos são u64 internamente
        assert_eq!(pane_id as u64, 1u64);
        assert_eq!(tab_id as u64, 2u64);
        assert_eq!(window_id as u64, 3u64);
        assert_eq!(session_id as u64, 4u64);
    }
}
