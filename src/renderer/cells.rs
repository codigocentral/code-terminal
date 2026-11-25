//! Conversão das células do grid em spans coloridos e faixas de fundo.
//!
//! O renderer desenha cada linha do terminal como texto rico (um span por
//! trecho com a mesma cor/estilo) e pinta os fundos que diferem do fundo da
//! janela como retângulos. Esta conversão é pura, para poder ser testada sem GPU.

use crate::renderer::text::{RenderColors, TextSpan};
use crate::terminal::grid::Cell;
use crate::terminal::parser::Color as TermColor;
use glyphon::Color;

/// Trecho contínuo de células com o mesmo fundo (diferente do fundo da janela).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackgroundRun {
    /// Coluna inicial.
    pub col: usize,
    /// Número de células.
    pub len: usize,
    /// Cor do fundo.
    pub color: Color,
}

/// Tipo de traço desenhado sobre as células.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationKind {
    Underline,
    Strikethrough,
}

/// Trecho contínuo de células com sublinhado ou tachado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecorationRun {
    pub col: usize,
    pub len: usize,
    pub color: Color,
    pub kind: DecorationKind,
}

/// Linha pronta para o renderer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderedLine {
    pub spans: Vec<TextSpan>,
    pub backgrounds: Vec<BackgroundRun>,
    pub decorations: Vec<DecorationRun>,
}

/// Cor da paleta xterm de 256 cores (os 16 primeiros vêm do tema).
pub fn xterm_256(index: u8, palette: &RenderColors) -> Color {
    match index {
        0..=7 => palette.ansi[index as usize],
        8..=15 => palette.ansi_bright[index as usize - 8],
        16..=231 => {
            let i = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color::rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        232..=255 => {
            let v = 8 + (index - 232) * 10;
            Color::rgb(v, v, v)
        }
    }
}

/// Resolve uma cor do terminal; `Default` vira `default`.
pub fn resolve_color(color: &TermColor, palette: &RenderColors, default: Color) -> Color {
    match *color {
        TermColor::Default => default,
        TermColor::Ansi(n) | TermColor::Indexed(n) => xterm_256(n, palette),
        TermColor::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

/// Mistura `from` em direção a `to` (`t = 0` mantém `from`).
fn mix(from: Color, to: Color, t: f32) -> Color {
    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color::rgb(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
    )
}

/// Estilo final de uma célula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CellStyle {
    fg: Color,
    bg: Option<Color>,
    bold: bool,
    italic: bool,
}

fn cell_style(cell: &Cell, palette: &RenderColors, is_cursor: bool) -> CellStyle {
    // Negrito com cor ANSI básica usa a variante brilhante, como no xterm.
    let fg_color = match cell.fg {
        TermColor::Ansi(n) | TermColor::Indexed(n) if cell.attrs.bold && n < 8 => {
            TermColor::Ansi(n + 8)
        }
        other => other,
    };
    let mut fg = resolve_color(&fg_color, palette, palette.foreground);
    let mut bg = match cell.bg {
        TermColor::Default => None,
        ref other => Some(resolve_color(other, palette, palette.background)),
    };

    if cell.attrs.reverse {
        let old_bg = bg.unwrap_or(palette.background);
        bg = Some(fg);
        fg = old_bg;
    }
    if cell.attrs.dim {
        fg = mix(fg, bg.unwrap_or(palette.background), 0.4);
    }
    if cell.highlight {
        bg = Some(palette.highlight_background);
        fg = palette.background;
    }
    if cell.selected {
        bg = Some(palette.selection_background);
        fg = palette.selection;
    }
    if is_cursor {
        bg = Some(palette.cursor_background);
        fg = palette.background;
    }

    CellStyle {
        fg,
        bg,
        bold: cell.attrs.bold,
        italic: cell.attrs.italic,
    }
}

fn push_background(runs: &mut Vec<BackgroundRun>, col: usize, color: Color) {
    if let Some(last) = runs.last_mut() {
        if last.color == color && last.col + last.len == col {
            last.len += 1;
            return;
        }
    }
    runs.push(BackgroundRun { col, len: 1, color });
}

fn push_decoration(runs: &mut Vec<DecorationRun>, col: usize, color: Color, kind: DecorationKind) {
    if let Some(last) = runs
        .iter_mut()
        .rev()
        .find(|r| r.kind == kind && r.col + r.len == col)
    {
        if last.color == color {
            last.len += 1;
            return;
        }
    }
    runs.push(DecorationRun {
        col,
        len: 1,
        color,
        kind,
    });
}

/// Caractere desenhado: oculto, NUL e controle viram espaço.
fn visible_char(cell: &Cell) -> char {
    if cell.attrs.hidden || cell.char == '\0' || cell.char.is_control() {
        ' '
    } else {
        cell.char
    }
}

/// Sublinhado e tachado da célula, na cor do texto.
fn push_decorations(runs: &mut Vec<DecorationRun>, col: usize, cell: &Cell, color: Color) {
    if cell.attrs.underline {
        push_decoration(runs, col, color, DecorationKind::Underline);
    }
    if cell.attrs.strikethrough {
        push_decoration(runs, col, color, DecorationKind::Strikethrough);
    }
}

/// Caracteres que a fonte do terminal desenha na largura da célula. Os
/// demais (CJK, emoji, símbolos de fontes de fallback) têm avanço próprio e
/// são desenhados sozinhos, na coluna exata, para não empurrar o resto da linha.
pub fn fits_cell_grid(c: char) -> bool {
    c.is_ascii() || ('\u{a0}'..='\u{24f}').contains(&c) || ('\u{2500}'..='\u{259f}').contains(&c)
}

/// Acrescenta o caractere ao último trecho se o estilo for o mesmo e ele
/// continuar na coluna seguinte; senão abre um trecho novo em `col`.
fn push_char(spans: &mut Vec<TextSpan>, col: usize, ch: char, style: &CellStyle) {
    let isolated = !fits_cell_grid(ch);
    match spans.last_mut() {
        Some(span)
            if !isolated
                && fits_cell_grid(span.text.chars().last().unwrap_or(' '))
                && span.col + span.text.chars().count() == col
                && span.color == style.fg
                && span.bold == style.bold
                && span.italic == style.italic =>
        {
            span.text.push(ch);
        }
        _ => spans.push(TextSpan {
            text: ch.to_string(),
            col,
            color: style.fg,
            bold: style.bold,
            italic: style.italic,
        }),
    }
}

/// Espaços no fim da linha não precisam de shaping.
fn trim_trailing_spaces(spans: &mut Vec<TextSpan>) {
    while let Some(span) = spans.last_mut() {
        let trimmed_len = span.text.trim_end_matches(' ').len();
        if trimmed_len > 0 {
            // Só sobram espaços à direita: o trecho continua começando em `col`.
            span.text.truncate(trimmed_len);
            return;
        }
        spans.pop();
    }
}

/// Converte uma linha de células.
///
/// `block_cursor_col` marca a célula com cursor em bloco, desenhada invertida.
pub fn build_line(
    cells: &[Cell],
    palette: &RenderColors,
    block_cursor_col: Option<usize>,
) -> RenderedLine {
    let mut line = RenderedLine::default();

    for (col, cell) in cells.iter().enumerate() {
        let style = cell_style(cell, palette, block_cursor_col == Some(col));
        if let Some(bg) = style.bg {
            push_background(&mut line.backgrounds, col, bg);
        }
        push_decorations(&mut line.decorations, col, cell, style.fg);
        if !cell.spacer {
            push_char(&mut line.spans, col, visible_char(cell), &style);
        }
    }

    trim_trailing_spaces(&mut line.spans);
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::parser::Attributes;

    fn cell(ch: char) -> Cell {
        Cell::new(
            ch,
            TermColor::Default,
            TermColor::Default,
            Attributes::default(),
        )
    }

    fn colored(ch: char, fg: TermColor) -> Cell {
        Cell::new(ch, fg, TermColor::Default, Attributes::default())
    }

    #[test]
    fn test_wide_and_symbol_chars_get_their_own_span_at_their_column() {
        let palette = RenderColors::default();
        let mut cells = vec![
            cell('a'),
            cell('日'),
            cell(' '),
            cell('b'),
            cell('✓'),
            cell('c'),
        ];
        cells[2].spacer = true;
        let line = build_line(&cells, &palette, None);
        let spans: Vec<(&str, usize)> = line
            .spans
            .iter()
            .map(|s| (s.text.as_str(), s.col))
            .collect();
        assert_eq!(
            spans,
            vec![("a", 0), ("日", 1), ("b", 3), ("✓", 4), ("c", 5)]
        );
    }

    #[test]
    fn test_xterm_256_cube_and_grays() {
        let palette = RenderColors::default();
        assert_eq!(xterm_256(1, &palette), palette.ansi[1]);
        assert_eq!(xterm_256(9, &palette), palette.ansi_bright[1]);
        assert_eq!(xterm_256(16, &palette), Color::rgb(0, 0, 0));
        assert_eq!(xterm_256(196, &palette), Color::rgb(255, 0, 0));
        assert_eq!(xterm_256(231, &palette), Color::rgb(255, 255, 255));
        assert_eq!(xterm_256(232, &palette), Color::rgb(8, 8, 8));
        assert_eq!(xterm_256(255, &palette), Color::rgb(238, 238, 238));
    }

    #[test]
    fn test_resolve_color() {
        let palette = RenderColors::default();
        let default = Color::rgb(1, 2, 3);
        assert_eq!(
            resolve_color(&TermColor::Default, &palette, default),
            default
        );
        assert_eq!(
            resolve_color(&TermColor::Rgb(10, 20, 30), &palette, default),
            Color::rgb(10, 20, 30)
        );
        assert_eq!(
            resolve_color(&TermColor::Ansi(2), &palette, default),
            palette.ansi[2]
        );
    }

    /// Regressão: o renderer desenhava o grid inteiro na cor padrão.
    #[test]
    fn test_line_keeps_ansi_colors_in_spans() {
        let palette = RenderColors::default();
        let cells = vec![
            colored('o', TermColor::Ansi(1)),
            colored('k', TermColor::Ansi(1)),
            cell(' '),
            colored('x', TermColor::Rgb(1, 2, 3)),
        ];
        let line = build_line(&cells, &palette, None);
        assert_eq!(line.spans.len(), 3);
        assert_eq!(line.spans[0].text, "ok");
        assert_eq!(line.spans[0].color, palette.ansi[1]);
        assert_eq!(line.spans[1].text, " ");
        assert_eq!(line.spans[1].color, palette.foreground);
        assert_eq!(line.spans[2].color, Color::rgb(1, 2, 3));
        assert!(line.backgrounds.is_empty());
    }

    #[test]
    fn test_bold_basic_color_uses_bright_variant() {
        let palette = RenderColors::default();
        let mut c = colored('b', TermColor::Ansi(4));
        c.attrs.bold = true;
        let line = build_line(&[c], &palette, None);
        assert_eq!(line.spans[0].color, palette.ansi_bright[4]);
        assert!(line.spans[0].bold);
    }

    #[test]
    fn test_backgrounds_merge_into_runs() {
        let palette = RenderColors::default();
        let mut cells = vec![cell('a'); 5];
        for c in cells.iter_mut().take(3).skip(1) {
            c.bg = TermColor::Ansi(4);
        }
        let line = build_line(&cells, &palette, None);
        assert_eq!(
            line.backgrounds,
            vec![BackgroundRun {
                col: 1,
                len: 2,
                color: palette.ansi[4]
            }]
        );
    }

    #[test]
    fn test_reverse_swaps_colors() {
        let palette = RenderColors::default();
        let mut c = cell('r');
        c.attrs.reverse = true;
        let line = build_line(&[c], &palette, None);
        assert_eq!(line.spans[0].color, palette.background);
        assert_eq!(line.backgrounds[0].color, palette.foreground);
    }

    #[test]
    fn test_selection_and_cursor_paint_background() {
        let palette = RenderColors::default();
        let mut cells = vec![cell('a'), cell('b'), cell('c')];
        cells[0].selected = true;
        let line = build_line(&cells, &palette, Some(2));
        assert_eq!(line.backgrounds[0].color, palette.selection_background);
        assert_eq!(line.backgrounds[1].col, 2);
        assert_eq!(line.backgrounds[1].color, palette.cursor_background);
        assert_eq!(line.spans.last().unwrap().color, palette.background);
    }

    #[test]
    fn test_hidden_and_nul_cells_render_as_space_and_trailing_space_is_trimmed() {
        let palette = RenderColors::default();
        let mut hidden = cell('s');
        hidden.attrs.hidden = true;
        let cells = vec![
            cell('a'),
            hidden,
            cell('\0'),
            cell('b'),
            cell(' '),
            cell('\0'),
        ];
        let line = build_line(&cells, &palette, None);
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].text, "a  b");
    }

    #[test]
    fn test_blank_line_has_no_spans() {
        let palette = RenderColors::default();
        let line = build_line(&vec![cell(' '); 10], &palette, None);
        assert!(line.spans.is_empty());
    }

    #[test]
    fn test_decorations() {
        let palette = RenderColors::default();
        let mut cells = vec![cell('u'), cell('u'), cell('s')];
        cells[0].attrs.underline = true;
        cells[1].attrs.underline = true;
        cells[2].attrs.strikethrough = true;
        let line = build_line(&cells, &palette, None);
        assert_eq!(line.decorations.len(), 2);
        assert_eq!(line.decorations[0].kind, DecorationKind::Underline);
        assert_eq!(line.decorations[0].len, 2);
        assert_eq!(line.decorations[1].kind, DecorationKind::Strikethrough);
    }

    #[test]
    fn test_dim_moves_toward_background() {
        let palette = RenderColors::default();
        let mut c = cell('d');
        c.attrs.dim = true;
        let line = build_line(&[c], &palette, None);
        let dimmed = line.spans[0].color;
        assert!(dimmed.r() < palette.foreground.r());
        assert!(dimmed.r() > palette.background.r());
    }
}
