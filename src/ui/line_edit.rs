//! Campo de texto de uma linha (renomear tab na sidebar).
//!
//! Começa com o texto todo selecionado, como ao renomear arquivo: digitar
//! troca tudo, setas desfazem a seleção. O cursor conta caracteres (não
//! bytes), então acentos e emoji andam uma posição por vez.

/// Teclas que o campo entende.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEditKey {
    Char(char),
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    SelectAll,
}

/// Estado do campo: texto, cursor e se está tudo selecionado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    chars: Vec<char>,
    cursor: usize,
    all_selected: bool,
}

/// Tamanho máximo do texto, em caracteres.
pub const MAX_LEN: usize = 200;

impl LineEdit {
    /// Campo com `text`, cursor no fim e tudo selecionado.
    pub fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().take(MAX_LEN).collect();
        Self {
            cursor: chars.len(),
            all_selected: !chars.is_empty(),
            chars,
        }
    }

    /// Texto atual.
    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// Posição do cursor (em caracteres).
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// O texto inteiro está selecionado.
    pub fn is_all_selected(&self) -> bool {
        self.all_selected
    }

    /// Apaga tudo se estiver selecionado. Devolve se apagou.
    fn take_selection(&mut self) -> bool {
        if !std::mem::take(&mut self.all_selected) {
            return false;
        }
        self.chars.clear();
        self.cursor = 0;
        true
    }

    /// Aplica uma tecla.
    pub fn apply(&mut self, key: LineEditKey) {
        match key {
            LineEditKey::Char(c) if !c.is_control() => {
                self.take_selection();
                if self.chars.len() < MAX_LEN {
                    self.chars.insert(self.cursor, c);
                    self.cursor += 1;
                }
            }
            LineEditKey::Char(_) => {}
            LineEditKey::Backspace => {
                if !self.take_selection() && self.cursor > 0 {
                    self.cursor -= 1;
                    self.chars.remove(self.cursor);
                }
            }
            LineEditKey::Delete => {
                if !self.take_selection() && self.cursor < self.chars.len() {
                    self.chars.remove(self.cursor);
                }
            }
            LineEditKey::Left => {
                if std::mem::take(&mut self.all_selected) {
                    self.cursor = 0;
                } else {
                    self.cursor = self.cursor.saturating_sub(1);
                }
            }
            LineEditKey::Right => {
                if std::mem::take(&mut self.all_selected) {
                    self.cursor = self.chars.len();
                } else {
                    self.cursor = (self.cursor + 1).min(self.chars.len());
                }
            }
            LineEditKey::Home => {
                self.all_selected = false;
                self.cursor = 0;
            }
            LineEditKey::End => {
                self.all_selected = false;
                self.cursor = self.chars.len();
            }
            LineEditKey::SelectAll => {
                self.all_selected = !self.chars.is_empty();
                self.cursor = self.chars.len();
            }
        }
    }

    /// Cola texto na posição do cursor (quebras de linha viram espaço).
    pub fn insert_str(&mut self, text: &str) {
        for c in text.chars() {
            let c = if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            };
            self.apply(LineEditKey::Char(c));
        }
    }

    /// Trecho visível em `width` caracteres, mantendo o cursor à vista:
    /// devolve o texto e a coluna do cursor dentro dele.
    pub fn visible(&self, width: usize) -> (String, usize) {
        let width = width.max(1);
        let start = (self.cursor + 1).saturating_sub(width);
        let text = self.chars.iter().skip(start).take(width).collect();
        (text, self.cursor - start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(edit: &mut LineEdit, text: &str) {
        for c in text.chars() {
            edit.apply(LineEditKey::Char(c));
        }
    }

    #[test]
    fn test_starts_selected_and_typing_replaces() {
        let mut edit = LineEdit::new("bash");
        assert!(edit.is_all_selected());
        assert_eq!(edit.cursor(), 4);
        typed(&mut edit, "api");
        assert_eq!(edit.text(), "api");
        assert!(!edit.is_all_selected());
    }

    #[test]
    fn test_arrows_cancel_selection_and_move() {
        let mut edit = LineEdit::new("abc");
        edit.apply(LineEditKey::Left);
        assert_eq!((edit.cursor(), edit.is_all_selected()), (0, false));
        edit.apply(LineEditKey::Right);
        edit.apply(LineEditKey::Char('X'));
        assert_eq!(edit.text(), "aXbc");
        edit.apply(LineEditKey::End);
        edit.apply(LineEditKey::Backspace);
        assert_eq!(edit.text(), "aXb");
        edit.apply(LineEditKey::Home);
        edit.apply(LineEditKey::Delete);
        assert_eq!(edit.text(), "Xb");
        edit.apply(LineEditKey::Left);
        assert_eq!(edit.cursor(), 0);
        edit.apply(LineEditKey::Right);
        edit.apply(LineEditKey::Right);
        edit.apply(LineEditKey::Right);
        assert_eq!(edit.cursor(), 2);

        let mut edit = LineEdit::new("abc");
        edit.apply(LineEditKey::Right);
        assert_eq!((edit.cursor(), edit.is_all_selected()), (3, false));
    }

    #[test]
    fn test_select_all_then_backspace_clears() {
        let mut edit = LineEdit::new("abc");
        edit.apply(LineEditKey::End);
        edit.apply(LineEditKey::SelectAll);
        assert!(edit.is_all_selected());
        edit.apply(LineEditKey::Backspace);
        assert_eq!(edit.text(), "");
        // Sem texto não há o que selecionar; teclas nas pontas não quebram.
        edit.apply(LineEditKey::SelectAll);
        assert!(!edit.is_all_selected());
        edit.apply(LineEditKey::Backspace);
        edit.apply(LineEditKey::Delete);
        edit.apply(LineEditKey::Left);
        assert_eq!(edit.text(), "");

        let mut edit = LineEdit::new("abc");
        edit.apply(LineEditKey::Delete);
        assert_eq!(edit.text(), "");
    }

    #[test]
    fn test_unicode_moves_by_char() {
        let mut edit = LineEdit::new("ação");
        edit.apply(LineEditKey::End);
        edit.apply(LineEditKey::Backspace);
        edit.apply(LineEditKey::Backspace);
        assert_eq!(edit.text(), "aç");
        typed(&mut edit, "ão 🚀");
        assert_eq!(edit.text(), "ação 🚀");
    }

    #[test]
    fn test_control_chars_ignored_and_paste_flattens() {
        let mut edit = LineEdit::new("");
        edit.apply(LineEditKey::Char('\u{7}'));
        assert_eq!(edit.text(), "");
        edit.insert_str("a\nb\tc");
        assert_eq!(edit.text(), "a b c");
    }

    #[test]
    fn test_length_is_capped() {
        let mut edit = LineEdit::new(&"x".repeat(MAX_LEN + 10));
        assert_eq!(edit.text().chars().count(), MAX_LEN);
        edit.apply(LineEditKey::End);
        edit.apply(LineEditKey::Char('y'));
        assert!(!edit.text().contains('y'));
    }

    #[test]
    fn test_visible_window_follows_cursor() {
        let mut edit = LineEdit::new("abcdefgh");
        edit.apply(LineEditKey::End);
        assert_eq!(edit.visible(4), ("fgh".to_string(), 3));
        edit.apply(LineEditKey::Home);
        assert_eq!(edit.visible(4), ("abcd".to_string(), 0));
        assert_eq!(edit.visible(0), ("a".to_string(), 0));
    }
}
