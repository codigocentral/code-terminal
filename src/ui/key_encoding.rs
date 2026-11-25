//! Tradução de teclas para os bytes que o programa no terminal espera.
//!
//! Segue o xterm: setas e Home/End em `CSI`/`SS3` (respeitando o modo de
//! cursor da aplicação, DECCKM), teclas de edição e F1–F12 com o parâmetro
//! de modificadores (`CSI 1;5A` para Ctrl+Seta), Ctrl+letra como caractere
//! de controle e Alt como prefixo `ESC`.

use winit::keyboard::{Key, NamedKey};

/// Modificadores ativos no momento da tecla.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl KeyMods {
    /// Parâmetro de modificador do xterm (1 = nenhum).
    fn xterm_param(self) -> u8 {
        1 + self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8
    }
}

const ESC: u8 = 0x1b;

/// Seta/Home/End: `ESC [ X`, `ESC O X` (modo aplicação) ou `ESC [ 1 ; m X`.
fn cursor_key(final_byte: u8, mods: KeyMods, app_cursor: bool) -> Vec<u8> {
    let m = mods.xterm_param();
    if m > 1 {
        format!("\x1b[1;{}{}", m, final_byte as char).into_bytes()
    } else if app_cursor {
        vec![ESC, b'O', final_byte]
    } else {
        vec![ESC, b'[', final_byte]
    }
}

/// Teclas com til: `ESC [ n ~` ou `ESC [ n ; m ~`.
fn tilde_key(number: u8, mods: KeyMods) -> Vec<u8> {
    let m = mods.xterm_param();
    if m > 1 {
        format!("\x1b[{};{}~", number, m).into_bytes()
    } else {
        format!("\x1b[{}~", number).into_bytes()
    }
}

/// F1–F4: `ESC O P..S` ou `ESC [ 1 ; m P..S`.
fn ss3_function_key(final_byte: u8, mods: KeyMods) -> Vec<u8> {
    let m = mods.xterm_param();
    if m > 1 {
        format!("\x1b[1;{}{}", m, final_byte as char).into_bytes()
    } else {
        vec![ESC, b'O', final_byte]
    }
}

fn with_alt(mut bytes: Vec<u8>, alt: bool) -> Vec<u8> {
    if alt {
        bytes.insert(0, ESC);
    }
    bytes
}

/// Caractere de controle de Ctrl+tecla (`Ctrl+C` = 0x03), se houver.
pub fn control_byte(ch: char) -> Option<u8> {
    match ch.to_ascii_lowercase() {
        c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '/' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

fn named_key(key: NamedKey, mods: KeyMods, app_cursor: bool) -> Option<Vec<u8>> {
    let bytes = match key {
        NamedKey::ArrowUp => cursor_key(b'A', mods, app_cursor),
        NamedKey::ArrowDown => cursor_key(b'B', mods, app_cursor),
        NamedKey::ArrowRight => cursor_key(b'C', mods, app_cursor),
        NamedKey::ArrowLeft => cursor_key(b'D', mods, app_cursor),
        NamedKey::Home => cursor_key(b'H', mods, app_cursor),
        NamedKey::End => cursor_key(b'F', mods, app_cursor),
        NamedKey::Insert => tilde_key(2, mods),
        NamedKey::Delete => tilde_key(3, mods),
        NamedKey::PageUp => tilde_key(5, mods),
        NamedKey::PageDown => tilde_key(6, mods),
        NamedKey::F1 => ss3_function_key(b'P', mods),
        NamedKey::F2 => ss3_function_key(b'Q', mods),
        NamedKey::F3 => ss3_function_key(b'R', mods),
        NamedKey::F4 => ss3_function_key(b'S', mods),
        NamedKey::F5 => tilde_key(15, mods),
        NamedKey::F6 => tilde_key(17, mods),
        NamedKey::F7 => tilde_key(18, mods),
        NamedKey::F8 => tilde_key(19, mods),
        NamedKey::F9 => tilde_key(20, mods),
        NamedKey::F10 => tilde_key(21, mods),
        NamedKey::F11 => tilde_key(23, mods),
        NamedKey::F12 => tilde_key(24, mods),
        // Shift+Enter manda ESC CR, como o Alt+Enter: é o que o Claude Code e
        // outros prompts de várias linhas leem como "nova linha" sem enviar
        // (o mesmo que o `/terminal-setup` configura em outros terminais).
        NamedKey::Enter => with_alt(vec![b'\r'], mods.alt || mods.shift),
        NamedKey::Tab if mods.shift => b"\x1b[Z".to_vec(),
        NamedKey::Tab => with_alt(vec![b'\t'], mods.alt),
        NamedKey::Backspace if mods.ctrl => with_alt(vec![0x08], mods.alt),
        NamedKey::Backspace => with_alt(vec![0x7f], mods.alt),
        NamedKey::Escape => vec![ESC],
        NamedKey::Space if mods.ctrl => with_alt(vec![0x00], mods.alt),
        NamedKey::Space => with_alt(vec![b' '], mods.alt),
        _ => return None,
    };
    Some(bytes)
}

/// Converte uma tecla em bytes para o PTY.
///
/// `text` é o texto que o sistema produziu (já com layout e acentos);
/// `app_cursor` indica o modo DECCKM da aplicação.
pub fn encode_key(
    logical_key: &Key,
    text: Option<&str>,
    mods: KeyMods,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    match logical_key {
        Key::Named(named) => {
            if let Some(bytes) = named_key(*named, mods, app_cursor) {
                return Some(bytes);
            }
        }
        Key::Character(chars) if mods.ctrl => {
            let mut it = chars.chars();
            if let (Some(ch), None) = (it.next(), it.next()) {
                if let Some(byte) = control_byte(ch) {
                    return Some(with_alt(vec![byte], mods.alt));
                }
            }
        }
        Key::Character(chars) if mods.alt => {
            return Some(with_alt(chars.as_bytes().to_vec(), true));
        }
        _ => {}
    }

    text.filter(|t| !t.is_empty())
        .map(|t| t.as_bytes().to_vec())
}

/// Bytes de uma colagem: quebras viram `\r` e, com bracketed paste ativo,
/// o texto vai entre `ESC[200~` e `ESC[201~` (o shell não executa linha a linha).
pub fn paste_bytes(content: &str, bracketed: bool) -> Vec<u8> {
    let normalized = content.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        // Remove marcadores embutidos, que permitiriam escapar do modo.
        let safe = normalized.replace("\x1b[201~", "").replace("\x1b[200~", "");
        format!("\x1b[200~{}\x1b[201~", safe).into_bytes()
    } else {
        normalized.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: KeyMods = KeyMods {
        shift: false,
        alt: false,
        ctrl: false,
    };
    const CTRL: KeyMods = KeyMods {
        shift: false,
        alt: false,
        ctrl: true,
    };
    const ALT: KeyMods = KeyMods {
        shift: false,
        alt: true,
        ctrl: false,
    };
    const SHIFT: KeyMods = KeyMods {
        shift: true,
        alt: false,
        ctrl: false,
    };

    fn named(key: NamedKey, mods: KeyMods, app: bool) -> Vec<u8> {
        encode_key(&Key::Named(key), None, mods, app).unwrap()
    }

    fn ch(c: &str, text: Option<&str>, mods: KeyMods) -> Option<Vec<u8>> {
        encode_key(&Key::Character(c.into()), text, mods, false)
    }

    /// Regressão: as setas não mandavam nada para o terminal.
    #[test]
    fn test_arrows_normal_and_application_mode() {
        assert_eq!(named(NamedKey::ArrowUp, NONE, false), b"\x1b[A");
        assert_eq!(named(NamedKey::ArrowLeft, NONE, false), b"\x1b[D");
        assert_eq!(named(NamedKey::ArrowUp, NONE, true), b"\x1bOA");
        assert_eq!(named(NamedKey::ArrowRight, CTRL, true), b"\x1b[1;5C");
        assert_eq!(named(NamedKey::Home, NONE, false), b"\x1b[H");
        assert_eq!(named(NamedKey::End, SHIFT, false), b"\x1b[1;2F");
    }

    #[test]
    fn test_editing_and_function_keys() {
        assert_eq!(named(NamedKey::Delete, NONE, false), b"\x1b[3~");
        assert_eq!(named(NamedKey::PageUp, NONE, false), b"\x1b[5~");
        assert_eq!(named(NamedKey::PageDown, CTRL, false), b"\x1b[6;5~");
        assert_eq!(named(NamedKey::F1, NONE, false), b"\x1bOP");
        assert_eq!(named(NamedKey::F4, SHIFT, false), b"\x1b[1;2S");
        assert_eq!(named(NamedKey::F5, NONE, false), b"\x1b[15~");
        assert_eq!(named(NamedKey::F12, NONE, false), b"\x1b[24~");
    }

    #[test]
    fn test_enter_tab_backspace_escape() {
        assert_eq!(named(NamedKey::Enter, NONE, false), b"\r");
        assert_eq!(named(NamedKey::Enter, ALT, false), b"\x1b\r");
        assert_eq!(named(NamedKey::Enter, SHIFT, false), b"\x1b\r");
        assert_eq!(named(NamedKey::Tab, NONE, false), b"\t");
        assert_eq!(named(NamedKey::Tab, SHIFT, false), b"\x1b[Z");
        assert_eq!(named(NamedKey::Backspace, NONE, false), [0x7f]);
        assert_eq!(named(NamedKey::Backspace, CTRL, false), [0x08]);
        assert_eq!(named(NamedKey::Escape, NONE, false), [0x1b]);
        assert_eq!(named(NamedKey::Space, CTRL, false), [0x00]);
    }

    #[test]
    fn test_ctrl_letters_and_symbols() {
        assert_eq!(ch("c", None, CTRL), Some(vec![0x03]));
        assert_eq!(ch("C", None, CTRL), Some(vec![0x03]));
        assert_eq!(ch("d", None, CTRL), Some(vec![0x04]));
        assert_eq!(ch("[", None, CTRL), Some(vec![0x1b]));
        assert_eq!(ch("\\", None, CTRL), Some(vec![0x1c]));
        assert_eq!(
            ch("g", None, KeyMods { alt: true, ..CTRL }),
            Some(vec![0x1b, 0x07])
        );
        assert_eq!(control_byte('é'), None);
    }

    #[test]
    fn test_alt_prefixes_escape_and_text_passes_through() {
        assert_eq!(ch("b", Some("b"), ALT), Some(b"\x1bb".to_vec()));
        assert_eq!(ch("ç", Some("ç"), NONE), Some("ç".as_bytes().to_vec()));
        assert_eq!(ch("a", Some("A"), SHIFT), Some(b"A".to_vec()));
        assert_eq!(ch("a", None, NONE), None);
    }

    #[test]
    fn test_dead_or_unknown_named_key_falls_back_to_text() {
        assert_eq!(
            encode_key(&Key::Named(NamedKey::CapsLock), None, NONE, false),
            None
        );
    }

    #[test]
    fn test_paste_bytes() {
        assert_eq!(paste_bytes("a\nb\r\nc", false), b"a\rb\rc");
        assert_eq!(paste_bytes("ls\n", true), b"\x1b[200~ls\r\x1b[201~");
        assert_eq!(
            paste_bytes("x\x1b[201~rm -rf", true),
            b"\x1b[200~xrm -rf\x1b[201~"
        );
    }
}
