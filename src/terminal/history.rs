//! Histórico compacto do grid (as linhas que rolaram para fora da tela).
//!
//! A tela visível fica em células prontas para desenhar ([`Cell`], 24 bytes
//! cada). No histórico, guardar o mesmo formato custava ~38 MB por aba de
//! 160 colunas com 10 mil linhas (#1). Aqui cada linha vira **uma** alocação
//! com o texto em UTF-8 e as rodadas de estilo (células seguidas com a mesma
//! cor, atributos e marcas), sem os brancos do fim:
//!
//! ```text
//! [rodadas: u16][rodada 0]...[rodada n-1][texto UTF-8]
//! rodada = [células: u16][frente: 4][fundo: 4][atributos: u8][marcas: u8]
//! ```
//!
//! Uma linha só com o estilo padrão não grava rodada nenhuma, e uma linha em
//! branco não aloca nada. A conversão é sem perda: cores 24-bit, atributos,
//! caracteres largos (a célula de continuação vira uma rodada com a marca
//! `spacer`), destaque da busca e seleção voltam iguais.

use std::collections::VecDeque;

use crate::terminal::grid::Cell;
use crate::terminal::parser::{Attributes, Color};

/// Bytes de uma rodada de estilo.
const RUN_BYTES: usize = 12;
/// Bytes do cabeçalho (quantas rodadas).
const HEADER_BYTES: usize = 2;

const FLAG_HIGHLIGHT: u8 = 1;
const FLAG_SELECTED: u8 = 1 << 1;
const FLAG_SPACER: u8 = 1 << 2;

/// Uma linha do histórico, compactada.
#[derive(Clone, Debug, Default)]
struct HistoryLine {
    /// Rodadas e texto (formato no topo do módulo). Vazio = linha em branco.
    data: Box<[u8]>,
    /// Células da linha original (as do fim, em branco, não estão em `data`).
    width: u16,
    /// Alguma célula tem o destaque da busca (limpar não precisa abrir as
    /// outras linhas).
    highlighted: bool,
}

/// Estilo de uma rodada.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Style {
    fg: Color,
    bg: Color,
    attrs: u8,
    flags: u8,
}

impl Style {
    const DEFAULT: Style = Style {
        fg: Color::Default,
        bg: Color::Default,
        attrs: 0,
        flags: 0,
    };

    fn of(cell: &Cell) -> Self {
        let mut flags = 0;
        if cell.highlight {
            flags |= FLAG_HIGHLIGHT;
        }
        if cell.selected {
            flags |= FLAG_SELECTED;
        }
        if cell.spacer {
            flags |= FLAG_SPACER;
        }
        Style {
            fg: cell.fg,
            bg: cell.bg,
            attrs: attrs_to_bits(&cell.attrs),
            flags,
        }
    }

    fn cell(&self, char: char) -> Cell {
        Cell {
            char,
            fg: self.fg,
            bg: self.bg,
            attrs: bits_to_attrs(self.attrs),
            highlight: self.flags & FLAG_HIGHLIGHT != 0,
            selected: self.flags & FLAG_SELECTED != 0,
            spacer: self.flags & FLAG_SPACER != 0,
        }
    }
}

fn attrs_to_bits(a: &Attributes) -> u8 {
    u8::from(a.bold)
        | u8::from(a.italic) << 1
        | u8::from(a.underline) << 2
        | u8::from(a.blink) << 3
        | u8::from(a.reverse) << 4
        | u8::from(a.hidden) << 5
        | u8::from(a.strikethrough) << 6
        | u8::from(a.dim) << 7
}

fn bits_to_attrs(bits: u8) -> Attributes {
    Attributes {
        bold: bits & 1 != 0,
        italic: bits & (1 << 1) != 0,
        underline: bits & (1 << 2) != 0,
        blink: bits & (1 << 3) != 0,
        reverse: bits & (1 << 4) != 0,
        hidden: bits & (1 << 5) != 0,
        strikethrough: bits & (1 << 6) != 0,
        dim: bits & (1 << 7) != 0,
    }
}

fn color_to_bytes(color: Color) -> [u8; 4] {
    match color {
        Color::Default => [0, 0, 0, 0],
        Color::Ansi(n) => [1, n, 0, 0],
        Color::Indexed(n) => [2, n, 0, 0],
        Color::Rgb(r, g, b) => [3, r, g, b],
    }
}

fn bytes_to_color(b: &[u8]) -> Color {
    match b[0] {
        1 => Color::Ansi(b[1]),
        2 => Color::Indexed(b[1]),
        3 => Color::Rgb(b[1], b[2], b[3]),
        _ => Color::Default,
    }
}

/// A célula some no fim da linha compactada (volta como `Cell::empty()`).
fn is_trailing_blank(cell: &Cell) -> bool {
    cell.char == ' ' && Style::of(cell) == Style::DEFAULT
}

impl HistoryLine {
    fn encode(cells: &[Cell]) -> Self {
        let width = cells.len().min(u16::MAX as usize);
        let cells = &cells[..width];
        let used = cells
            .iter()
            .rposition(|cell| !is_trailing_blank(cell))
            .map_or(0, |last| last + 1);
        let cells = &cells[..used];
        let highlighted = cells.iter().any(|cell| cell.highlight);
        if cells.is_empty() {
            return Self {
                data: Box::default(),
                width: width as u16,
                highlighted,
            };
        }

        // Rodadas: só quando algo foge do estilo padrão.
        let mut runs: Vec<(u16, Style)> = Vec::new();
        let mut plain = true;
        let mut text_len = 0;
        for cell in cells {
            text_len += cell.char.len_utf8();
            let style = Style::of(cell);
            plain &= style == Style::DEFAULT;
            match runs.last_mut() {
                Some((count, last)) if *last == style => *count += 1,
                _ => runs.push((1, style)),
            }
        }
        if plain {
            runs.clear();
        }

        let mut data = Vec::with_capacity(HEADER_BYTES + runs.len() * RUN_BYTES + text_len);
        data.extend_from_slice(&(runs.len() as u16).to_le_bytes());
        for (count, style) in &runs {
            data.extend_from_slice(&count.to_le_bytes());
            data.extend_from_slice(&color_to_bytes(style.fg));
            data.extend_from_slice(&color_to_bytes(style.bg));
            data.push(style.attrs);
            data.push(style.flags);
        }
        let mut buf = [0u8; 4];
        for cell in cells {
            data.extend_from_slice(cell.char.encode_utf8(&mut buf).as_bytes());
        }
        Self {
            data: data.into_boxed_slice(),
            width: width as u16,
            highlighted,
        }
    }

    /// Escreve as células da linha em `out` (substitui o que havia).
    fn decode_into(&self, out: &mut Vec<Cell>) {
        out.clear();
        out.reserve(self.width as usize);
        if !self.data.is_empty() {
            let runs = u16::from_le_bytes([self.data[0], self.data[1]]) as usize;
            let text_start = HEADER_BYTES + runs * RUN_BYTES;
            // `data` só é escrito por `encode`, a partir de `char`s: é UTF-8.
            let text = std::str::from_utf8(&self.data[text_start..]).unwrap_or_default();
            let mut chars = text.chars();
            if runs == 0 {
                out.extend(chars.map(|c| Style::DEFAULT.cell(c)));
            } else {
                for run in self.data[HEADER_BYTES..text_start].chunks_exact(RUN_BYTES) {
                    let count = u16::from_le_bytes([run[0], run[1]]) as usize;
                    let style = Style {
                        fg: bytes_to_color(&run[2..6]),
                        bg: bytes_to_color(&run[6..10]),
                        attrs: run[10],
                        flags: run[11],
                    };
                    out.extend(chars.by_ref().take(count).map(|c| style.cell(c)));
                }
            }
        }
        out.resize(self.width as usize, Cell::empty());
    }

    fn cells(&self) -> Vec<Cell> {
        let mut cells = Vec::new();
        self.decode_into(&mut cells);
        cells
    }

    fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.data.len()
    }
}

/// Linhas que rolaram para fora da tela, da mais antiga para a mais nova.
#[derive(Clone, Debug, Default)]
pub struct History {
    lines: VecDeque<HistoryLine>,
}

impl History {
    /// Histórico vazio (não reserva nada: cresce conforme chega saída).
    pub fn new() -> Self {
        Self::default()
    }

    /// Quantas linhas há.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Sem linha nenhuma.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Apaga tudo e devolve a memória.
    pub fn clear(&mut self) {
        self.lines = VecDeque::new();
    }

    /// Guarda uma linha no fim; com `limit` linhas, a mais antiga sai.
    pub fn push(&mut self, cells: &[Cell], limit: usize) {
        if limit == 0 {
            return;
        }
        while self.lines.len() >= limit {
            self.lines.pop_front();
        }
        self.lines.push_back(HistoryLine::encode(cells));
    }

    /// Células da linha `index` (0 = a mais antiga).
    pub fn get(&self, index: usize) -> Option<Vec<Cell>> {
        self.lines.get(index).map(HistoryLine::cells)
    }

    /// Escreve as células da linha `index` em `out` (reaproveita a
    /// alocação); `false` se a linha não existe.
    pub fn decode_into(&self, index: usize, out: &mut Vec<Cell>) -> bool {
        match self.lines.get(index) {
            Some(line) => {
                line.decode_into(out);
                true
            }
            None => false,
        }
    }

    /// Todas as linhas, da mais antiga para a mais nova (abre uma por vez).
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Vec<Cell>> + '_ {
        self.lines.iter().map(HistoryLine::cells)
    }

    /// Marca o destaque da busca nas colunas `start..end` da linha `index`.
    pub fn set_highlight(&mut self, index: usize, start: usize, end: usize) {
        let Some(line) = self.lines.get_mut(index) else {
            return;
        };
        let mut cells = line.cells();
        let end = end.min(cells.len());
        if start >= end {
            return;
        }
        for cell in &mut cells[start..end] {
            cell.highlight = true;
        }
        *line = HistoryLine::encode(&cells);
    }

    /// Tira o destaque da busca de todas as linhas.
    pub fn clear_highlights(&mut self) {
        for line in self.lines.iter_mut().filter(|line| line.highlighted) {
            let mut cells = line.cells();
            for cell in &mut cells {
                cell.highlight = false;
            }
            *line = HistoryLine::encode(&cells);
        }
    }

    /// A célula `col` da linha `index` tem o destaque da busca.
    pub fn is_highlighted(&self, index: usize, col: usize) -> bool {
        self.lines
            .get(index)
            .filter(|line| line.highlighted)
            .and_then(|line| line.cells().get(col).map(|cell| cell.highlight))
            .unwrap_or(false)
    }

    /// Memória ocupada pelas linhas (estimativa, sem o overhead do alocador).
    pub fn memory_bytes(&self) -> usize {
        self.lines.capacity() * std::mem::size_of::<HistoryLine>()
            + self.lines.iter().map(|line| line.data.len()).sum::<usize>()
    }

    /// Bytes médios por linha (para testes e diagnóstico).
    pub fn average_line_bytes(&self) -> usize {
        if self.lines.is_empty() {
            return 0;
        }
        self.lines
            .iter()
            .map(HistoryLine::memory_bytes)
            .sum::<usize>()
            / self.lines.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: &Cell, b: &Cell) -> bool {
        a.char == b.char
            && a.fg == b.fg
            && a.bg == b.bg
            && a.attrs == b.attrs
            && a.highlight == b.highlight
            && a.selected == b.selected
            && a.spacer == b.spacer
    }

    fn assert_round_trip(cells: &[Cell]) {
        let mut history = History::new();
        history.push(cells, 10);
        let back = history.get(0).unwrap();
        assert_eq!(back.len(), cells.len());
        for (i, (a, b)) in cells.iter().zip(&back).enumerate() {
            assert!(same(a, b), "coluna {i}: {a:?} != {b:?}");
        }
    }

    fn text_cells(text: &str, width: usize) -> Vec<Cell> {
        let mut cells: Vec<Cell> = text
            .chars()
            .map(|c| Cell {
                char: c,
                ..Cell::empty()
            })
            .collect();
        cells.resize(width, Cell::empty());
        cells
    }

    #[test]
    fn plain_line_round_trips_without_runs() {
        let cells = text_cells("linha 000001 de historico", 160);
        assert_round_trip(&cells);
        let line = HistoryLine::encode(&cells);
        // Cabeçalho + texto, sem rodadas nem os brancos do fim.
        assert_eq!(line.data.len(), HEADER_BYTES + 25);
    }

    #[test]
    fn blank_line_does_not_allocate() {
        let line = HistoryLine::encode(&vec![Cell::empty(); 160]);
        assert!(line.data.is_empty());
        assert_eq!(line.cells().len(), 160);
    }

    #[test]
    fn colors_attributes_and_flags_round_trip() {
        let mut cells = text_cells("abcdefgh", 12);
        cells[0].fg = Color::Rgb(255, 128, 1);
        cells[1].bg = Color::Rgb(0, 0, 0);
        cells[2].fg = Color::Indexed(200);
        cells[3].fg = Color::Ansi(9);
        cells[4].attrs = Attributes {
            bold: true,
            italic: true,
            underline: true,
            blink: true,
            reverse: true,
            hidden: true,
            strikethrough: true,
            dim: true,
        };
        cells[5].highlight = true;
        cells[6].selected = true;
        // Fundo colorido no fim (BCE) não é branco: fica.
        cells[11].bg = Color::Ansi(4);
        assert_round_trip(&cells);
    }

    #[test]
    fn wide_chars_and_unicode_round_trip() {
        let mut cells = text_cells("ação 日 🚀 x", 20);
        // Continuações dos largos, como o grid escreve.
        let wide: Vec<usize> = cells
            .iter()
            .enumerate()
            .filter(|(_, c)| unicode_width::UnicodeWidthChar::width(c.char) == Some(2))
            .map(|(i, _)| i)
            .collect();
        for i in wide.into_iter().rev() {
            cells.insert(
                i + 1,
                Cell {
                    spacer: true,
                    ..Cell::empty()
                },
            );
        }
        cells.truncate(20);
        assert_round_trip(&cells);
    }

    #[test]
    fn every_attribute_bit_maps_back() {
        for bits in 0..=u8::MAX {
            assert_eq!(attrs_to_bits(&bits_to_attrs(bits)), bits);
        }
    }

    #[test]
    fn limit_drops_oldest() {
        let mut history = History::new();
        for i in 0..5 {
            history.push(&text_cells(&format!("l{i}"), 4), 3);
        }
        assert_eq!(history.len(), 3);
        let first: String = history.get(0).unwrap().iter().map(|c| c.char).collect();
        assert_eq!(first.trim_end(), "l2");
        history.push(&text_cells("x", 4), 0);
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn highlights_are_set_checked_and_cleared() {
        let mut history = History::new();
        history.push(&text_cells("procura erro aqui", 20), 10);
        history.push(&text_cells("nada", 20), 10);
        history.set_highlight(0, 8, 12);
        assert!(history.is_highlighted(0, 8));
        assert!(history.is_highlighted(0, 11));
        assert!(!history.is_highlighted(0, 12));
        assert!(!history.is_highlighted(1, 0));
        history.clear_highlights();
        assert!(!history.is_highlighted(0, 8));
        assert!(history.get(0).unwrap().iter().all(|c| !c.highlight));
    }

    #[test]
    fn ten_thousand_lines_of_160_columns_fit_in_two_megabytes() {
        let mut history = History::new();
        for i in 0..10_000 {
            let text = format!(
                "linha {i:06} de historico com texto para ocupar boa parte da largura da tab"
            );
            history.push(&text_cells(&text, 160), 10_000);
        }
        // Antes: 10 000 × 160 × 24 bytes = 38,4 MB.
        assert!(
            history.memory_bytes() < 2 * 1024 * 1024,
            "{} bytes",
            history.memory_bytes()
        );
    }
}
