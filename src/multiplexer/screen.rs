//! Redesenho da tela de uma sessão para um cliente que acabou de anexar.
//!
//! O daemon guarda o grid de cada sessão; ao anexar, o cliente recebe a tela
//! atual (texto, cores, atributos, cursor) e os modos que mudam a entrada
//! (tela alternativa, teclas de cursor, colagem, mouse). Depois disso só
//! recebe a saída nova do processo.

use crate::terminal::grid::{Cell, Grid};
use crate::terminal::modes::{CursorKeysMode, MouseMode, TerminalModes};
use crate::terminal::parser::{Attributes, Color};
use std::fmt::Write;

/// Sequência que reproduz `grid` num terminal limpo.
pub fn screen_to_ansi(grid: &Grid) -> Vec<u8> {
    let modes = grid.modes();
    let mut out = String::new();
    // Volta a um estado conhecido: tela normal, sem atributos, sem margens.
    out.push_str("\x1b[?1049l\x1b[0m\x1b[r");
    if modes.alternate_buffer {
        out.push_str("\x1b[?1049h");
    }
    out.push_str("\x1b[H\x1b[2J");
    for (row, cells) in grid.visible_lines().iter().enumerate() {
        write_row(&mut out, row, cells);
    }
    out.push_str("\x1b[0m");
    write_modes(&mut out, modes);
    let (row, col) = grid.cursor_position();
    let _ = write!(out, "\x1b[{};{}H", row + 1, col + 1);
    out.push_str(if modes.show_cursor {
        "\x1b[?25h"
    } else {
        "\x1b[?25l"
    });
    out.into_bytes()
}

/// Histórico e tela de `grid` para um terminal novo: o anexo de uma tab
/// persistente e o registro compactado do daemon.
///
/// Diferente de [`screen_to_ansi`], as linhas saem em sequência (com
/// `\r\n`): o que estava no histórico volta para o histórico, e um terminal
/// de outro tamanho só quebra ou rola as linhas. Na tela alternativa, a
/// principal (com o histórico) vai primeiro e a alternativa é desenhada por
/// cima, como estava. Título (OSC 2), diretório (OSC 7) e modos também vão.
pub fn history_to_ansi(grid: &Grid) -> Vec<u8> {
    let modes = grid.modes();
    let mut out = String::from("\x1b[0m");
    let (cells, history, cursor) = match grid.saved_main_screen() {
        Some(main) => main,
        None => (
            grid.get_cells().as_slice(),
            grid.scrollback_lines(),
            grid.cursor_position(),
        ),
    };
    for row in history.iter() {
        write_line(&mut out, &row);
        out.push_str("\r\n");
    }
    // A tela principal até o cursor ou a última linha com texto: linhas em
    // branco no fim empurrariam o conteúdo para cima.
    let last_text = cells
        .iter()
        .rposition(|row| row.iter().any(|cell| !is_blank(cell)))
        .map_or(0, |row| row + 1);
    let used = last_text.max(cursor.0 + 1).min(cells.len());
    for (i, row) in cells[..used].iter().enumerate() {
        if i > 0 {
            out.push_str("\r\n");
        }
        write_line(&mut out, row);
    }
    // Cursor: sobe da última linha escrita até a dele (posição relativa,
    // vale para qualquer altura de terminal).
    let up = used.saturating_sub(1).saturating_sub(cursor.0);
    if up > 0 {
        let _ = write!(out, "\x1b[{up}A");
    }
    let _ = write!(out, "\x1b[{}G", cursor.1 + 1);
    if let Some(title) = grid.title() {
        let clean: String = title.chars().filter(|c| !c.is_control()).collect();
        let _ = write!(out, "\x1b]2;{clean}\x1b\\");
    }
    if let Some(dir) = grid.working_dir().and_then(|dir| dir.to_str()) {
        let clean: String = dir.chars().filter(|c| !c.is_control()).collect();
        let _ = write!(out, "\x1b]7;file://localhost{clean}\x1b\\");
    }
    if modes.alternate_buffer {
        out.push_str("\x1b[?1049h\x1b[H\x1b[2J");
        for (row, cells) in grid.get_cells().iter().enumerate() {
            write_row(&mut out, row, cells);
        }
        out.push_str("\x1b[0m");
        let (row, col) = grid.cursor_position();
        let _ = write!(out, "\x1b[{};{}H", row + 1, col + 1);
    }
    write_modes(&mut out, modes);
    out.push_str(if modes.show_cursor {
        "\x1b[?25h"
    } else {
        "\x1b[?25l"
    });
    out.into_bytes()
}

/// Uma linha no ponto atual, sem os brancos do fim; termina sem atributos
/// (senão o fundo vazaria para a linha seguinte).
fn write_line(out: &mut String, cells: &[Cell]) {
    let used = cells
        .iter()
        .rposition(|cell| !is_blank(cell))
        .map_or(0, |last| last + 1);
    if used == 0 {
        return;
    }
    let mut current: Option<(Color, Color, Attributes)> = None;
    for cell in &cells[..used] {
        if cell.spacer {
            continue;
        }
        let style = (cell.fg, cell.bg, cell.attrs);
        if current != Some(style) {
            out.push_str(&sgr(&style.0, &style.1, &style.2));
            current = Some(style);
        }
        out.push(if cell.char == '\0' { ' ' } else { cell.char });
    }
    out.push_str("\x1b[0m");
}

/// Uma linha: posiciona e escreve, trocando o SGR só quando o estilo muda.
/// Espaços em branco no fim (com fundo padrão) não são escritos.
fn write_row(out: &mut String, row: usize, cells: &[Cell]) {
    let used = cells
        .iter()
        .rposition(|cell| !is_blank(cell))
        .map_or(0, |last| last + 1);
    if used == 0 {
        return;
    }
    let _ = write!(out, "\x1b[{};1H", row + 1);
    let mut current: Option<(Color, Color, Attributes)> = None;
    for cell in &cells[..used] {
        if cell.spacer {
            continue;
        }
        let style = (cell.fg, cell.bg, cell.attrs);
        if current != Some(style) {
            out.push_str(&sgr(&style.0, &style.1, &style.2));
            current = Some(style);
        }
        out.push(if cell.char == '\0' { ' ' } else { cell.char });
    }
}

fn is_blank(cell: &Cell) -> bool {
    (cell.char == ' ' || cell.char == '\0')
        && cell.bg == Color::Default
        && !cell.attrs.reverse
        && !cell.attrs.underline
}

/// SGR completo (começa com reset) para um estilo.
fn sgr(fg: &Color, bg: &Color, attrs: &Attributes) -> String {
    let mut codes = vec!["0".to_string()];
    for (on, code) in [
        (attrs.bold, "1"),
        (attrs.dim, "2"),
        (attrs.italic, "3"),
        (attrs.underline, "4"),
        (attrs.blink, "5"),
        (attrs.reverse, "7"),
        (attrs.hidden, "8"),
        (attrs.strikethrough, "9"),
    ] {
        if on {
            codes.push(code.to_string());
        }
    }
    if let Some(code) = color_code(fg, false) {
        codes.push(code);
    }
    if let Some(code) = color_code(bg, true) {
        codes.push(code);
    }
    format!("\x1b[{}m", codes.join(";"))
}

fn color_code(color: &Color, background: bool) -> Option<String> {
    let (base, bright, extended) = if background {
        (40, 100, 48)
    } else {
        (30, 90, 38)
    };
    match *color {
        Color::Default => None,
        Color::Ansi(n) if n < 8 => Some((base + u16::from(n)).to_string()),
        Color::Ansi(n) if n < 16 => Some((bright + u16::from(n - 8)).to_string()),
        Color::Ansi(n) | Color::Indexed(n) => Some(format!("{extended};5;{n}")),
        Color::Rgb(r, g, b) => Some(format!("{extended};2;{r};{g};{b}")),
    }
}

/// Modos que afetam o que o terminal do cliente manda de volta.
fn write_modes(out: &mut String, modes: &TerminalModes) {
    let flag = |on: bool| if on { 'h' } else { 'l' };
    let app_cursor = matches!(modes.cursor_keys, CursorKeysMode::Application);
    let _ = write!(out, "\x1b[?1{}", flag(app_cursor));
    let _ = write!(out, "\x1b[?2004{}", flag(modes.bracketed_paste));
    let _ = write!(out, "\x1b[?1004{}", flag(modes.focus_events));
    let _ = write!(out, "\x1b[?7{}", flag(modes.auto_wrap));
    out.push_str(if modes.application_keypad {
        "\x1b="
    } else {
        "\x1b>"
    });
    out.push_str("\x1b[?9l\x1b[?1000l\x1b[?1001l\x1b[?1002l\x1b[?1003l");
    let mouse = match modes.mouse_mode {
        MouseMode::None => None,
        MouseMode::X10 => Some(9),
        MouseMode::VT200 => Some(1000),
        MouseMode::VT200Highlight => Some(1001),
        MouseMode::ButtonEvent => Some(1002),
        MouseMode::AnyEvent => Some(1003),
    };
    if let Some(mode) = mouse {
        let _ = write!(out, "\x1b[?{mode}h");
    }
    let _ = write!(out, "\x1b[?1006{}", flag(modes.sgr_mouse));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::parser::Parser;

    fn grid_after(cols: usize, rows: usize, input: &[u8]) -> Grid {
        let mut grid = Grid::new(cols, rows);
        Parser::new(&mut grid).process(input);
        grid
    }

    /// Reproduzir a tela num grid novo dá a mesma tela (texto, cores, cursor).
    #[test]
    fn test_screen_round_trips_through_a_fresh_grid() {
        let original = grid_after(
            30,
            5,
            "linha 1\r\n\x1b[1;31mvermelho\x1b[0m e \x1b[38;5;200mindexado\x1b[0m\r\n\x1b[48;2;1;2;3m fundo \x1b[0m\r\nação 日本"
                .as_bytes(),
        );
        let replayed = grid_after(30, 5, &screen_to_ansi(&original));
        for (a, b) in original
            .visible_lines()
            .iter()
            .zip(replayed.visible_lines())
        {
            let chars = |row: &Vec<Cell>| row.iter().map(|c| c.char).collect::<String>();
            assert_eq!(chars(a), chars(b));
            for (ca, cb) in a.iter().zip(b.iter()) {
                assert_eq!((ca.fg, ca.bg, ca.attrs), (cb.fg, cb.bg, cb.attrs));
            }
        }
        assert_eq!(original.cursor_position(), replayed.cursor_position());
    }

    #[test]
    fn test_modes_are_restored() {
        let original = grid_after(
            20,
            4,
            b"\x1b[?1049h\x1b[?1h\x1b[?2004h\x1b[?1002h\x1b[?1006h\x1b[?25lalt",
        );
        let replayed = grid_after(20, 4, &screen_to_ansi(&original));
        let (a, b) = (original.modes(), replayed.modes());
        assert!(b.alternate_buffer);
        assert!(matches!(b.cursor_keys, CursorKeysMode::Application));
        assert_eq!(a.bracketed_paste, b.bracketed_paste);
        assert_eq!(a.mouse_mode, b.mouse_mode);
        assert!(b.sgr_mouse);
        assert!(!b.show_cursor);
        let text: String = replayed.visible_lines()[0].iter().map(|c| c.char).collect();
        assert!(text.starts_with("alt"));
    }

    fn text_rows<R: std::borrow::Borrow<Vec<Cell>>>(
        rows: impl IntoIterator<Item = R>,
    ) -> Vec<String> {
        rows.into_iter()
            .map(|row| {
                row.borrow()
                    .iter()
                    .filter(|c| !c.spacer)
                    .map(|c| c.char)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// O histórico volta como histórico, a tela como tela, cursor no lugar.
    #[test]
    fn test_history_round_trips_scrollback_and_screen() {
        let mut input = String::new();
        for i in 0..30 {
            input.push_str(&format!("\x1b[3{}mlinha {i}\x1b[0m\r\n", i % 8));
        }
        input.push_str("\x1b]2;meu titulo\x07\x1b]7;file://pc/tmp/x\x1b\\$ ls");
        let original = grid_after(20, 6, input.as_bytes());
        let replayed = grid_after(20, 6, &history_to_ansi(&original));
        assert_eq!(
            text_rows(original.scrollback_lines().iter()),
            text_rows(replayed.scrollback_lines().iter())
        );
        assert_eq!(
            text_rows(original.visible_lines()),
            text_rows(replayed.visible_lines())
        );
        assert_eq!(original.cursor_position(), replayed.cursor_position());
        assert_eq!(replayed.title(), Some("meu titulo"));
        assert_eq!(replayed.working_dir(), Some(std::path::Path::new("/tmp/x")));
        let first = replayed.scrollback_lines().get(0).unwrap();
        assert_eq!(first[0].fg, Color::Ansi(0));
    }

    /// Na tela alternativa (vim, htop), a principal vai antes com o
    /// histórico e a alternativa é redesenhada por cima.
    #[test]
    fn test_history_keeps_main_screen_under_alternate() {
        let original = grid_after(
            20,
            4,
            b"um\r\ndois\r\ntres\r\nquatro\r\ncinco\r\n$ vim\x1b[?1049h\x1b[2;3Heditor\x1b[?2004h",
        );
        let mut replayed = grid_after(20, 4, &history_to_ansi(&original));
        assert!(replayed.modes().alternate_buffer);
        assert!(replayed.modes().bracketed_paste);
        assert_eq!(text_rows(replayed.visible_lines())[1], "  editor");
        assert_eq!(original.cursor_position(), replayed.cursor_position());
        replayed.disable_alternate_buffer();
        assert_eq!(text_rows(replayed.visible_lines()).last().unwrap(), "$ vim");
        assert_eq!(text_rows(replayed.scrollback_lines().iter())[0], "um");
    }

    /// Tela com poucas linhas usadas: o cursor fica na linha do texto, não
    /// no fim da tela (linhas vazias não são escritas).
    #[test]
    fn test_history_does_not_push_short_screen_up() {
        let original = grid_after(20, 10, b"$ ");
        let replayed = grid_after(20, 10, &history_to_ansi(&original));
        assert_eq!(replayed.cursor_position(), (0, 2));
        assert_eq!(replayed.scrollback_len(), 0);
    }

    #[test]
    fn test_color_codes() {
        assert_eq!(color_code(&Color::Ansi(1), false).as_deref(), Some("31"));
        assert_eq!(color_code(&Color::Ansi(9), true).as_deref(), Some("101"));
        assert_eq!(
            color_code(&Color::Indexed(200), false).as_deref(),
            Some("38;5;200")
        );
        assert_eq!(
            color_code(&Color::Rgb(1, 2, 3), true).as_deref(),
            Some("48;2;1;2;3")
        );
        assert_eq!(color_code(&Color::Default, true), None);
    }
}
