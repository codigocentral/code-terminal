/// Modos de cursor keys
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum CursorKeysMode {
    /// Modo normal (cursor keys enviam ESC [ A/B/C/D)
    #[default]
    Normal,
    /// Modo aplicativo (cursor keys enviam ESC O A/B/C/D)
    Application,
}

/// Modo de colunas
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum ColumnMode {
    /// 80 colunas (padrão)
    #[default]
    Columns80,
    /// 132 colunas
    Columns132,
}

/// Estado dos modos de terminal
#[derive(Clone, Debug, Default)]
pub struct TerminalModes {
    /// DECCKM - Cursor Keys Mode
    pub cursor_keys: CursorKeysMode,
    /// DECCOLM - Column Mode
    pub columns: ColumnMode,
    /// DECTCEM - Show cursor
    pub show_cursor: bool,
    /// Alternate Screen Buffer ativo
    pub alternate_buffer: bool,
    /// Bracketed Paste Mode
    pub bracketed_paste: bool,
    /// Auto wrap mode
    pub auto_wrap: bool,
    /// Insert mode
    pub insert_mode: bool,
    /// Origin mode
    pub origin_mode: bool,
    /// Mouse tracking mode
    pub mouse_mode: MouseMode,
    /// Coordenadas do mouse no formato SGR (1006)
    pub sgr_mouse: bool,
    /// Avisar o programa quando a janela ganha/perde foco (1004)
    pub focus_events: bool,
    /// LNM: LF também volta à coluna 0
    pub linefeed_newline: bool,
    /// Teclado numérico em modo aplicação (DECKPAM)
    pub application_keypad: bool,
    /// Forma do cursor pedida pelo programa (DECSCUSR); `None` usa a configuração
    pub cursor_shape: Option<CursorShapeMode>,
}

/// Forma do cursor pedida via DECSCUSR (`CSI n SP q`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShapeMode {
    Block,
    Underline,
    Bar,
}

impl CursorShapeMode {
    /// Converte o parâmetro do DECSCUSR (0/1/2 bloco, 3/4 sublinhado, 5/6 barra).
    pub fn from_param(param: usize) -> Option<Self> {
        match param {
            1 | 2 => Some(Self::Block),
            3 | 4 => Some(Self::Underline),
            5 | 6 => Some(Self::Bar),
            _ => None,
        }
    }

    /// Nome usado na configuração (`cursor_style`).
    pub fn as_style(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Underline => "underline",
            Self::Bar => "bar",
        }
    }
}

impl TerminalModes {
    /// Cria estado padrão
    pub fn new() -> Self {
        Self {
            cursor_keys: CursorKeysMode::Normal,
            columns: ColumnMode::Columns80,
            show_cursor: true,
            alternate_buffer: false,
            bracketed_paste: false,
            auto_wrap: true,
            insert_mode: false,
            origin_mode: false,
            mouse_mode: MouseMode::None,
            sgr_mouse: false,
            focus_events: false,
            linefeed_newline: false,
            application_keypad: false,
            cursor_shape: None,
        }
    }

    /// Reseta para estado padrão
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

/// Modos de tracking de mouse
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum MouseMode {
    /// Mouse desabilitado
    #[default]
    None,
    /// X10 compatibility mode
    X10,
    /// VT200 tracking
    VT200,
    /// VT200 highlight tracking
    VT200Highlight,
    /// Button event tracking
    ButtonEvent,
    /// Any event tracking
    AnyEvent,
}

/// Estado do alternate screen buffer
#[derive(Clone, Debug)]
pub struct AlternateBuffer {
    /// Células do buffer alternativo
    pub cells: Vec<Vec<crate::terminal::grid::Cell>>,
    /// Posição do cursor salva
    pub cursor: (usize, usize),
    /// Scrollback salvo
    pub scrollback: std::collections::VecDeque<Vec<crate::terminal::grid::Cell>>,
}

impl AlternateBuffer {
    /// Cria buffer alternativo com dimensões específicas
    pub fn new(cols: usize, rows: usize) -> Self {
        use crate::terminal::grid::Cell;

        Self {
            cells: vec![vec![Cell::default(); cols]; rows],
            cursor: (0, 0),
            scrollback: std::collections::VecDeque::with_capacity(10000),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_modes() {
        let modes = TerminalModes::new();
        assert!(modes.show_cursor);
        assert!(!modes.alternate_buffer);
        assert!(!modes.bracketed_paste);
        assert!(matches!(modes.cursor_keys, CursorKeysMode::Normal));
        assert!(matches!(modes.columns, ColumnMode::Columns80));
    }

    #[test]
    fn test_modes_reset() {
        let mut modes = TerminalModes::new();
        modes.show_cursor = false;
        modes.bracketed_paste = true;
        modes.reset();

        assert!(modes.show_cursor);
        assert!(!modes.bracketed_paste);
    }
}
