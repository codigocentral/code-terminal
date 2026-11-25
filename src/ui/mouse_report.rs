//! Envio de eventos de mouse ao programa do terminal (less, nvim, htop, tmux).
//!
//! Quando o programa liga um modo de mouse (`CSI ? 1000/1002/1003 h`), cliques,
//! arrasto e rodinha vão para ele em vez de selecionar texto ou rolar o
//! histórico. Segura Shift para selecionar mesmo assim, como no xterm.

use crate::terminal::modes::{MouseMode, TerminalModes};

/// Botão do mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    fn code(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Middle => 1,
            Self::Right => 2,
        }
    }
}

/// O que aconteceu com o mouse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press(MouseButton),
    Release(MouseButton),
    /// Movimento; com o botão segurado, se houver.
    Motion(Option<MouseButton>),
    WheelUp,
    WheelDown,
}

/// Modificadores relatados junto com o evento.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// O programa quer receber eventos de mouse? Shift força a seleção local.
pub fn reporting_active(modes: &TerminalModes, shift: bool) -> bool {
    modes.mouse_mode != MouseMode::None && !shift
}

/// Codifica o evento para o modo ativo. `None` quando o modo não pede esse
/// tipo de evento (ex.: movimento sem 1002/1003) ou a posição não cabe no
/// formato antigo.
pub fn encode_mouse(
    modes: &TerminalModes,
    action: MouseAction,
    row: usize,
    col: usize,
    mods: MouseMods,
) -> Option<Vec<u8>> {
    let mode = modes.mouse_mode;
    let (mut code, release) = button_code(mode, action)?;
    if mode != MouseMode::X10 {
        code += 4 * mods.shift as u8 + 8 * mods.alt as u8 + 16 * mods.ctrl as u8;
    }

    let (x, y) = (col + 1, row + 1);
    if modes.sgr_mouse {
        let suffix = if release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{x};{y}{suffix}").into_bytes());
    }

    // Formato antigo: soltar é sempre o botão 3, e coordenadas vão até 223.
    if release {
        code = (code & !0b11) | 3;
    }
    if x > 223 || y > 223 {
        return None;
    }
    Some(vec![
        0x1b,
        b'[',
        b'M',
        32 + code,
        32 + x as u8,
        32 + y as u8,
    ])
}

/// Código do botão e se é "soltar", ou `None` se o modo não pede o evento.
fn button_code(mode: MouseMode, action: MouseAction) -> Option<(u8, bool)> {
    if mode == MouseMode::None {
        return None;
    }
    match action {
        MouseAction::Press(button) => Some((button.code(), false)),
        MouseAction::Release(_) if mode == MouseMode::X10 => None,
        MouseAction::Release(button) => Some((button.code(), true)),
        MouseAction::Motion(button) => {
            let wanted = match mode {
                MouseMode::AnyEvent => true,
                MouseMode::ButtonEvent => button.is_some(),
                _ => false,
            };
            // Sem botão: código 3 ("nenhum") + 32 de movimento.
            wanted.then(|| (button.map_or(3, MouseButton::code) + 32, false))
        }
        MouseAction::WheelUp => Some((64, false)),
        MouseAction::WheelDown => Some((65, false)),
    }
}

/// Rodinha na tela alternativa sem modo de mouse (less, man, vim sem mouse):
/// vira setas, como no xterm com "alternate scroll".
pub fn wheel_as_arrows(modes: &TerminalModes, up: bool, lines: usize) -> Option<Vec<u8>> {
    if !modes.alternate_buffer || modes.mouse_mode != MouseMode::None || lines == 0 {
        return None;
    }
    let app_cursor = matches!(
        modes.cursor_keys,
        crate::terminal::modes::CursorKeysMode::Application
    );
    let arrow: &[u8] = match (up, app_cursor) {
        (true, true) => b"\x1bOA",
        (true, false) => b"\x1b[A",
        (false, true) => b"\x1bOB",
        (false, false) => b"\x1b[B",
    };
    Some(arrow.repeat(lines))
}

/// Aviso de foco (`CSI ? 1004 h`): `CSI I` ao ganhar e `CSI O` ao perder.
pub fn focus_report(modes: &TerminalModes, focused: bool) -> Option<&'static [u8]> {
    modes
        .focus_events
        .then_some(if focused { b"\x1b[I" } else { b"\x1b[O" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(mode: MouseMode, sgr: bool) -> TerminalModes {
        let mut modes = TerminalModes::new();
        modes.mouse_mode = mode;
        modes.sgr_mouse = sgr;
        modes
    }

    const NO_MODS: MouseMods = MouseMods {
        shift: false,
        alt: false,
        ctrl: false,
    };

    #[test]
    fn test_no_mode_reports_nothing() {
        let m = modes(MouseMode::None, true);
        assert!(!reporting_active(&m, false));
        assert_eq!(
            encode_mouse(&m, MouseAction::Press(MouseButton::Left), 0, 0, NO_MODS),
            None
        );
    }

    #[test]
    fn test_sgr_press_release_wheel() {
        let m = modes(MouseMode::VT200, true);
        assert!(reporting_active(&m, false));
        assert!(!reporting_active(&m, true));
        let press = encode_mouse(&m, MouseAction::Press(MouseButton::Left), 4, 9, NO_MODS);
        assert_eq!(press.unwrap(), b"\x1b[<0;10;5M");
        let release = encode_mouse(&m, MouseAction::Release(MouseButton::Right), 0, 0, NO_MODS);
        assert_eq!(release.unwrap(), b"\x1b[<2;1;1m");
        let wheel = encode_mouse(&m, MouseAction::WheelDown, 1, 1, NO_MODS);
        assert_eq!(wheel.unwrap(), b"\x1b[<65;2;2M");
        let ctrl = MouseMods {
            ctrl: true,
            ..NO_MODS
        };
        let wheel = encode_mouse(&m, MouseAction::WheelUp, 0, 0, ctrl);
        assert_eq!(wheel.unwrap(), b"\x1b[<80;1;1M");
    }

    #[test]
    fn test_legacy_encoding_and_limits() {
        let m = modes(MouseMode::VT200, false);
        let press = encode_mouse(&m, MouseAction::Press(MouseButton::Left), 0, 0, NO_MODS);
        assert_eq!(press.unwrap(), vec![0x1b, b'[', b'M', 32, 33, 33]);
        let release = encode_mouse(&m, MouseAction::Release(MouseButton::Left), 0, 0, NO_MODS);
        assert_eq!(release.unwrap(), vec![0x1b, b'[', b'M', 35, 33, 33]);
        assert_eq!(
            encode_mouse(&m, MouseAction::Press(MouseButton::Left), 0, 300, NO_MODS),
            None
        );
    }

    #[test]
    fn test_motion_depends_on_mode() {
        let held = MouseAction::Motion(Some(MouseButton::Left));
        let free = MouseAction::Motion(None);
        let vt200 = modes(MouseMode::VT200, true);
        assert_eq!(encode_mouse(&vt200, held, 0, 0, NO_MODS), None);
        let button = modes(MouseMode::ButtonEvent, true);
        assert_eq!(
            encode_mouse(&button, held, 2, 3, NO_MODS).unwrap(),
            b"\x1b[<32;4;3M"
        );
        assert_eq!(encode_mouse(&button, free, 0, 0, NO_MODS), None);
        let any = modes(MouseMode::AnyEvent, true);
        assert_eq!(
            encode_mouse(&any, free, 0, 0, NO_MODS).unwrap(),
            b"\x1b[<35;1;1M"
        );
    }

    #[test]
    fn test_x10_only_presses_without_mods() {
        let m = modes(MouseMode::X10, true);
        let shift = MouseMods {
            shift: true,
            ..NO_MODS
        };
        assert_eq!(
            encode_mouse(&m, MouseAction::Press(MouseButton::Middle), 0, 0, shift).unwrap(),
            b"\x1b[<1;1;1M"
        );
        assert_eq!(
            encode_mouse(&m, MouseAction::Release(MouseButton::Middle), 0, 0, NO_MODS),
            None
        );
    }

    #[test]
    fn test_wheel_as_arrows_only_on_alternate_screen() {
        let mut m = TerminalModes::new();
        assert_eq!(wheel_as_arrows(&m, true, 3), None);
        m.alternate_buffer = true;
        assert_eq!(wheel_as_arrows(&m, true, 2).unwrap(), b"\x1b[A\x1b[A");
        m.cursor_keys = crate::terminal::modes::CursorKeysMode::Application;
        assert_eq!(wheel_as_arrows(&m, false, 1).unwrap(), b"\x1bOB");
        m.mouse_mode = MouseMode::VT200;
        assert_eq!(wheel_as_arrows(&m, false, 1), None);
    }

    #[test]
    fn test_focus_report() {
        let mut m = TerminalModes::new();
        assert_eq!(focus_report(&m, true), None);
        m.focus_events = true;
        assert_eq!(focus_report(&m, true), Some(&b"\x1b[I"[..]));
        assert_eq!(focus_report(&m, false), Some(&b"\x1b[O"[..]));
    }
}
