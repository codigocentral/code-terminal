//! Scrollback Buffer Otimizado com Delta Encoding
//!
//! Este módulo implementa um scrollback buffer eficiente usando:
//! - Delta encoding: armazena apenas as diferenças entre linhas consecutivas
//! - Run-length encoding (RLE) para sequências de células idênticas
//! - Compressão de atributos compartilhados
//! - Pool de linhas reutilizáveis

use crate::terminal::grid::Cell;
use std::collections::VecDeque;

/// Tamanho máximo de uma linha antes de forçar truncamento
const MAX_LINE_LENGTH: usize = 512;

/// Threshold para ativar compressão RLE
const RLE_THRESHOLD: usize = 4;

/// Representação compacta de uma linha do scrollback
#[derive(Clone, Debug)]
pub enum CompactLine {
    /// Linha completa (quando não há linha anterior para comparar)
    Full(Vec<CompactCell>),
    /// Delta encoding: diferença da linha anterior
    Delta {
        /// Número de linhas a voltar na referência
        back_ref: u16,
        /// Modificações: (coluna_start, coluna_end, células)
        changes: Vec<(u16, u16, Vec<CompactCell>)>,
    },
    /// Linha vazia (otimização comum)
    Empty { width: u16 },
    /// Run-length encoded
    Rle(Vec<(CompactCell, u16)>),
}

/// Representação compacta de uma célula
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompactCell {
    /// Caractere (u32 para suportar Unicode completo)
    pub char_code: u32,
    /// Índice da cor foreground na paleta
    pub fg_idx: u8,
    /// Índice da cor background na paleta
    pub bg_idx: u8,
    /// Flags de atributos (bold, italic, etc.)
    pub attr_flags: u8,
}

impl CompactCell {
    /// Cria uma célula compacta vazia (espaço)
    pub fn empty() -> Self {
        Self {
            char_code: ' ' as u32,
            fg_idx: 0, // Default foreground
            bg_idx: 1, // Default background
            attr_flags: 0,
        }
    }

    /// Verifica se a célula é vazia (espaço com cores default)
    pub fn is_empty(&self) -> bool {
        self.char_code == ' ' as u32 && self.fg_idx == 0 && self.bg_idx == 1 && self.attr_flags == 0
    }

    /// Tamanho em bytes na representação compacta
    pub fn size_bytes() -> usize {
        std::mem::size_of::<Self>()
    }
}

impl From<&Cell> for CompactCell {
    fn from(cell: &Cell) -> Self {
        Self {
            char_code: cell.char as u32,
            fg_idx: compact_color(&cell.fg),
            bg_idx: compact_color(&cell.bg),
            attr_flags: compact_attrs(&cell.attrs),
        }
    }
}

/// Paleta de cores para compressão
#[derive(Clone, Debug)]
pub struct ColorPalette {
    /// Cores usadas frequentemente (índices 0-15)
    /// 0 = default fg, 1 = default bg, 2-15 = ANSI colors
    colors: Vec<crate::terminal::parser::Color>,
    /// Mapeamento reverso para lookup rápido
    color_to_idx: std::collections::HashMap<crate::terminal::parser::Color, u8>,
}

impl ColorPalette {
    pub fn new() -> Self {
        use crate::terminal::parser::Color;

        let colors = vec![
            Color::Default,  // 0: Default foreground
            Color::Default,  // 1: Default background (tratado especial)
            Color::Ansi(0),  // 2: Black
            Color::Ansi(1),  // 3: Red
            Color::Ansi(2),  // 4: Green
            Color::Ansi(3),  // 5: Yellow
            Color::Ansi(4),  // 6: Blue
            Color::Ansi(5),  // 7: Magenta
            Color::Ansi(6),  // 8: Cyan
            Color::Ansi(7),  // 9: White
            Color::Ansi(8),  // 10: Bright Black
            Color::Ansi(9),  // 11: Bright Red
            Color::Ansi(10), // 12: Bright Green
            Color::Ansi(11), // 13: Bright Yellow
            Color::Ansi(12), // 14: Bright Blue
            Color::Ansi(13), // 15: Bright Magenta
            Color::Ansi(14), // 16: Bright Cyan
            Color::Ansi(15), // 17: Bright White
        ];

        let mut color_to_idx = std::collections::HashMap::new();
        for (i, color) in colors.iter().enumerate() {
            color_to_idx.insert(*color, i as u8);
        }

        Self {
            colors,
            color_to_idx,
        }
    }

    /// Converte índice para cor
    pub fn get_color(&self, idx: u8) -> crate::terminal::parser::Color {
        if (idx as usize) < self.colors.len() {
            self.colors[idx as usize]
        } else {
            crate::terminal::parser::Color::Default
        }
    }

    /// Adiciona uma cor à paleta se houver espaço
    pub fn add_color(&mut self, color: crate::terminal::parser::Color) -> u8 {
        if let Some(&idx) = self.color_to_idx.get(&color) {
            return idx;
        }

        if self.colors.len() < 256 {
            let idx = self.colors.len() as u8;
            self.colors.push(color);
            self.color_to_idx.insert(color, idx);
            idx
        } else {
            0 // Fallback para default
        }
    }
}

impl Default for ColorPalette {
    fn default() -> Self {
        Self::new()
    }
}

/// Compacta uma cor para índice
fn compact_color(color: &crate::terminal::parser::Color) -> u8 {
    use crate::terminal::parser::Color;

    match color {
        Color::Default => 0,
        Color::Ansi(n) => {
            if *n < 16 {
                *n + 2 // Offset para deixar espaço para defaults
            } else {
                0 // Fallback - cores 256 não são pré-mapeadas
            }
        }
        Color::Indexed(_) => 0,   // Fallback - cores 256 não são pré-mapeadas
        Color::Rgb(_, _, _) => 0, // Fallback - RGB requer paleta dinâmica
    }
}

/// Compacta atributos para flags
fn compact_attrs(attrs: &crate::terminal::parser::Attributes) -> u8 {
    let mut flags = 0u8;
    if attrs.bold {
        flags |= 0x01;
    }
    if attrs.italic {
        flags |= 0x02;
    }
    if attrs.underline {
        flags |= 0x04;
    }
    if attrs.blink {
        flags |= 0x08;
    }
    if attrs.reverse {
        flags |= 0x10;
    }
    if attrs.hidden {
        flags |= 0x20;
    }
    if attrs.strikethrough {
        flags |= 0x40;
    }
    if attrs.dim {
        flags |= 0x80;
    }
    flags
}

/// Buffer de scrollback otimizado com delta encoding
#[derive(Clone, Debug)]
pub struct CompactScrollback {
    /// Linhas compactadas
    lines: VecDeque<CompactLine>,
    /// Limite máximo de linhas
    limit: usize,
    /// Paleta de cores
    palette: ColorPalette,
    /// Contador de acessos para LRU
    access_count: u64,
    /// Último acesso de cada linha
    last_access: VecDeque<u64>,
    /// Tamanho total estimado em bytes
    estimated_size: usize,
}

impl CompactScrollback {
    /// Cria um novo scrollback com limite especificado
    pub fn with_limit(limit: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(limit.min(100)),
            limit,
            palette: ColorPalette::new(),
            access_count: 0,
            last_access: VecDeque::with_capacity(limit.min(100)),
            estimated_size: 0,
        }
    }

    /// Adiciona uma linha ao scrollback
    pub fn push_line(&mut self, cells: &[Cell]) {
        let cells = if cells.len() > MAX_LINE_LENGTH {
            &cells[..MAX_LINE_LENGTH]
        } else {
            cells
        };

        // Remove linha mais antiga se atingiu o limite
        if self.lines.len() >= self.limit {
            if let Some(removed) = self.lines.pop_front() {
                self.estimated_size = self
                    .estimated_size
                    .saturating_sub(estimate_line_size(&removed));
            }
            self.last_access.pop_front();
        }

        // Compacta a linha
        let compact_line = self.compact_line(cells);
        self.estimated_size += estimate_line_size(&compact_line);

        self.lines.push_back(compact_line);
        self.last_access.push_back(self.access_count);
        self.access_count += 1;

        // Trigger de compressão se muito grande
        if self.estimated_size > self.target_max_size() {
            self.compress_older_lines();
        }
    }

    /// Compacta uma linha de células
    fn compact_line(&mut self, cells: &[Cell]) -> CompactLine {
        // Verifica se é linha vazia
        if cells.iter().all(|c| c.char == ' ') {
            return CompactLine::Empty {
                width: cells.len() as u16,
            };
        }

        // Verifica se RLE é vantajoso
        if should_use_rle(cells) {
            return CompactLine::Rle(encode_rle(cells));
        }

        // Tenta delta encoding se houver linha anterior
        if !self.lines.is_empty() {
            if let Some(prev_line) = self.lines.back() {
                if let Some(delta) = self.compute_delta(prev_line, cells, 1) {
                    return delta;
                }
            }
        }

        // Fallback para linha completa
        CompactLine::Full(cells.iter().map(CompactCell::from).collect())
    }

    /// Computa delta entre uma linha de referência e a nova linha
    fn compute_delta(
        &self,
        reference: &CompactLine,
        new_cells: &[Cell],
        back_ref: u16,
    ) -> Option<CompactLine> {
        // Só faz delta encoding para linhas completas similares
        let ref_cells = match reference {
            CompactLine::Full(cells) => cells,
            _ => return None, // Delta só a partir de linhas completas
        };

        // Se tamanhos diferentes, não faz delta
        if ref_cells.len() != new_cells.len() {
            return None;
        }

        // Encontra regiões modificadas
        let mut changes: Vec<(u16, u16, Vec<CompactCell>)> = Vec::new();
        let mut current_start: Option<usize> = None;
        let mut current_cells: Vec<CompactCell> = Vec::new();

        for (i, (ref_cell, new_cell)) in ref_cells.iter().zip(new_cells.iter()).enumerate() {
            let new_compact = CompactCell::from(new_cell);

            if *ref_cell != new_compact {
                if current_start.is_none() {
                    current_start = Some(i);
                }
                current_cells.push(new_compact);
            } else if let Some(start) = current_start {
                // Região terminou
                changes.push((start as u16, i as u16, std::mem::take(&mut current_cells)));
                current_start = None;
            }
        }

        // Fecha região aberta
        if let Some(start) = current_start {
            changes.push((start as u16, new_cells.len() as u16, current_cells));
        }

        // Só vale a pena se economizar memória (> 50% similar)
        // Conta as colunas modificadas de cada região (end - start), não o
        // índice final: uma mudança só na última coluna não é "linha toda".
        let modified_cols: usize = changes
            .iter()
            .map(|(start, end, _)| (*end - *start) as usize)
            .sum();
        if modified_cols > new_cells.len() / 2 {
            return None; // Muitas mudanças, delta não é eficiente
        }

        Some(CompactLine::Delta { back_ref, changes })
    }

    /// Retorna o tamanho máximo alvo baseado no limite
    fn target_max_size(&self) -> usize {
        // ~100 bytes por linha em média * limite
        self.limit * 100
    }

    /// Comprime linhas antigas quando necessário
    fn compress_older_lines(&mut self) {
        // Marca linhas antigas para serem convertidas para representação mais compacta
        // Isso é chamado raramente, apenas quando o buffer cresce muito

        let cutoff = self.access_count.saturating_sub(self.limit as u64 / 2);

        for i in 0..self.lines.len().saturating_sub(10) {
            if let Some(&last_acc) = self.last_access.get(i) {
                if last_acc < cutoff {
                    // Linha antiga, podemos tornála mais compacta se possível
                    // Por exemplo, convertendo para RLE se aplicável
                    if let CompactLine::Full(cells) = &self.lines[i] {
                        if should_use_rle_full(cells) {
                            let old_size = estimate_line_size(&self.lines[i]);
                            let new_line = CompactLine::Rle(encode_rle_compact(cells));
                            let new_size = estimate_line_size(&new_line);

                            if new_size < old_size {
                                self.estimated_size =
                                    self.estimated_size.saturating_sub(old_size - new_size);
                                self.lines[i] = new_line;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Retorna o número de linhas
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Verifica se está vazio
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Retorna o limite
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Retorna a paleta de cores usada pelo scrollback.
    pub fn palette(&self) -> &ColorPalette {
        &self.palette
    }

    /// Limpa todo o scrollback
    pub fn clear(&mut self) {
        self.lines.clear();
        self.last_access.clear();
        self.estimated_size = 0;
    }

    /// Estimativa de uso de memória em bytes
    pub fn estimated_memory_usage(&self) -> usize {
        self.estimated_size + std::mem::size_of::<Self>()
    }

    /// Retorna estatísticas detalhadas
    pub fn stats(&self) -> ScrollbackStats {
        let mut full_count = 0;
        let mut delta_count = 0;
        let mut empty_count = 0;
        let mut rle_count = 0;

        for line in &self.lines {
            match line {
                CompactLine::Full(_) => full_count += 1,
                CompactLine::Delta { .. } => delta_count += 1,
                CompactLine::Empty { .. } => empty_count += 1,
                CompactLine::Rle(_) => rle_count += 1,
            }
        }

        ScrollbackStats {
            total_lines: self.lines.len(),
            full_lines: full_count,
            delta_lines: delta_count,
            empty_lines: empty_count,
            rle_lines: rle_count,
            estimated_bytes: self.estimated_size,
            compression_ratio: self.calculate_compression_ratio(),
        }
    }

    fn calculate_compression_ratio(&self) -> f32 {
        // Estima o tamanho não comprimido
        let uncompressed: usize = self
            .lines
            .iter()
            .map(|line| match line {
                CompactLine::Full(cells) => cells.len() * CompactCell::size_bytes(),
                CompactLine::Delta { changes, .. } => changes
                    .iter()
                    .map(|(_, _, cells)| cells.len() * CompactCell::size_bytes())
                    .sum(),
                CompactLine::Empty { width } => *width as usize * CompactCell::size_bytes(),
                CompactLine::Rle(runs) => runs
                    .iter()
                    .map(|(_, count)| *count as usize * CompactCell::size_bytes())
                    .sum(),
            })
            .sum();

        if uncompressed == 0 {
            1.0
        } else {
            self.estimated_size as f32 / uncompressed as f32
        }
    }
}

/// Estatísticas do scrollback
#[derive(Clone, Copy, Debug, Default)]
pub struct ScrollbackStats {
    pub total_lines: usize,
    pub full_lines: usize,
    pub delta_lines: usize,
    pub empty_lines: usize,
    pub rle_lines: usize,
    pub estimated_bytes: usize,
    pub compression_ratio: f32,
}

/// Verifica se RLE é vantajoso para um array de células
fn should_use_rle(cells: &[Cell]) -> bool {
    if cells.len() < RLE_THRESHOLD {
        return false;
    }

    let mut run_length = 1;
    let mut max_run = 1;

    for window in cells.windows(2) {
        if cells_equal(&window[0], &window[1]) {
            run_length += 1;
            max_run = max_run.max(run_length);
        } else {
            run_length = 1;
        }
    }

    // RLE vale a pena se há runs de pelo menos RLE_THRESHOLD
    max_run >= RLE_THRESHOLD
}

/// Verifica se RLE é vantajoso para células compactas
fn should_use_rle_full(cells: &[CompactCell]) -> bool {
    if cells.len() < RLE_THRESHOLD {
        return false;
    }

    let mut run_length = 1;
    let mut max_run = 1;

    for window in cells.windows(2) {
        if window[0] == window[1] {
            run_length += 1;
            max_run = max_run.max(run_length);
        } else {
            run_length = 1;
        }
    }

    max_run >= RLE_THRESHOLD
}

/// Compara duas células
fn cells_equal(a: &Cell, b: &Cell) -> bool {
    a.char == b.char
        && std::mem::discriminant(&a.fg) == std::mem::discriminant(&b.fg)
        && std::mem::discriminant(&a.bg) == std::mem::discriminant(&b.bg)
}

/// Codifica células usando RLE
fn encode_rle(cells: &[Cell]) -> Vec<(CompactCell, u16)> {
    let mut result = Vec::new();
    if cells.is_empty() {
        return result;
    }

    let mut current = CompactCell::from(&cells[0]);
    let mut count: u16 = 1;

    for cell in &cells[1..] {
        let compact = CompactCell::from(cell);
        if compact == current && count < u16::MAX {
            count += 1;
        } else {
            result.push((current, count));
            current = compact;
            count = 1;
        }
    }

    result.push((current, count));
    result
}

/// Codifica células compactas usando RLE
fn encode_rle_compact(cells: &[CompactCell]) -> Vec<(CompactCell, u16)> {
    let mut result = Vec::new();
    if cells.is_empty() {
        return result;
    }

    let mut current = cells[0];
    let mut count: u16 = 1;

    for &cell in &cells[1..] {
        if cell == current && count < u16::MAX {
            count += 1;
        } else {
            result.push((current, count));
            current = cell;
            count = 1;
        }
    }

    result.push((current, count));
    result
}

/// Estima o tamanho em bytes de uma linha compactada
fn estimate_line_size(line: &CompactLine) -> usize {
    match line {
        CompactLine::Full(cells) => {
            std::mem::size_of::<CompactLine>() + cells.len() * CompactCell::size_bytes()
        }
        CompactLine::Delta { changes, .. } => {
            let changes_size: usize = changes
                .iter()
                .map(|(_, _, cells)| {
                    std::mem::size_of::<(u16, u16, ())>() + cells.len() * CompactCell::size_bytes()
                })
                .sum();
            std::mem::size_of::<CompactLine>() + changes_size
        }
        CompactLine::Empty { .. } => std::mem::size_of::<CompactLine>(),
        CompactLine::Rle(runs) => {
            std::mem::size_of::<CompactLine>() + runs.len() * (CompactCell::size_bytes() + 2)
        }
    }
}

impl Default for CompactScrollback {
    fn default() -> Self {
        Self::with_limit(10000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::parser::{Attributes, Color};

    fn create_test_cell(c: char, fg: Color, bg: Color) -> Cell {
        Cell::new(c, fg, bg, Attributes::default())
    }

    #[test]
    fn test_compact_cell_size() {
        assert_eq!(CompactCell::size_bytes(), 8);
    }

    #[test]
    fn test_empty_line_detection() {
        let cells: Vec<Cell> = (0..80).map(|_| Cell::empty()).collect();
        let mut scrollback = CompactScrollback::with_limit(100);
        let line = scrollback.compact_line(&cells);

        match line {
            CompactLine::Empty { width } => assert_eq!(width, 80),
            _ => panic!("Expected Empty line"),
        }
    }

    #[test]
    fn test_push_line_truncates_long_lines() {
        let cells: Vec<Cell> = (0..MAX_LINE_LENGTH + 10).map(|_| Cell::empty()).collect();
        let mut scrollback = CompactScrollback::with_limit(100);

        scrollback.push_line(&cells);

        match scrollback.lines.front().unwrap() {
            CompactLine::Empty { width } => assert_eq!(*width as usize, MAX_LINE_LENGTH),
            _ => panic!("Expected truncated empty line"),
        }
    }

    #[test]
    fn test_rle_encoding() {
        // Linha com repetições
        let cells: Vec<Cell> = (0..80)
            .map(|i| {
                if i < 40 {
                    Cell::empty()
                } else {
                    create_test_cell('X', Color::Ansi(1), Color::Default)
                }
            })
            .collect();

        let encoded = encode_rle(&cells);
        assert_eq!(encoded.len(), 2); // Dois runs: 40 espaços, 40 X's
        assert_eq!(encoded[0].1, 40);
        assert_eq!(encoded[1].1, 40);
    }

    #[test]
    fn test_scrollback_push_and_stats() {
        let mut scrollback = CompactScrollback::with_limit(10);

        // Adiciona algumas linhas
        for i in 0..5 {
            let cells: Vec<Cell> = (0..80)
                .map(|j| {
                    if j == i {
                        create_test_cell('*', Color::Ansi(1), Color::Default)
                    } else {
                        Cell::empty()
                    }
                })
                .collect();
            scrollback.push_line(&cells);
        }

        assert_eq!(scrollback.len(), 5);

        let stats = scrollback.stats();
        assert_eq!(stats.total_lines, 5);
        assert!(stats.estimated_bytes > 0);
    }

    #[test]
    fn test_scrollback_limit() {
        let mut scrollback = CompactScrollback::with_limit(5);

        // Adiciona mais linhas que o limite
        for _ in 0..10 {
            let cells: Vec<Cell> = (0..80).map(|_| Cell::empty()).collect();
            scrollback.push_line(&cells);
        }

        assert_eq!(scrollback.len(), 5);
    }

    #[test]
    fn test_compression_ratio() {
        let mut scrollback = CompactScrollback::with_limit(100);

        // Adiciona linhas vazias (alta compressão)
        for _ in 0..10 {
            let cells: Vec<Cell> = (0..80).map(|_| Cell::empty()).collect();
            scrollback.push_line(&cells);
        }

        let stats = scrollback.stats();
        // Linhas vazias devem ter compressão muito alta
        assert!(stats.compression_ratio < 0.1);
    }

    /// Linha sem runs repetidos (evita RLE): "abcdabcd..." com a cor dada.
    fn varied_line(len: usize, fg: Color) -> Vec<Cell> {
        (0..len)
            .map(|i| create_test_cell((b'a' + (i % 4) as u8) as char, fg, Color::Default))
            .collect()
    }

    #[test]
    fn test_compact_cell_helpers_and_attrs() {
        assert!(CompactCell::empty().is_empty());
        let attrs = Attributes {
            bold: true,
            italic: true,
            underline: true,
            blink: true,
            reverse: true,
            hidden: true,
            strikethrough: true,
            dim: true,
        };
        let cell = Cell::new('z', Color::Ansi(3), Color::Rgb(1, 2, 3), attrs);
        let compact = CompactCell::from(&cell);
        assert_eq!(compact.char_code, 'z' as u32);
        assert_eq!(compact.fg_idx, 5);
        assert_eq!(compact.bg_idx, 0, "RGB cai no default");
        assert_eq!(compact.attr_flags, 0xFF);
        assert!(!compact.is_empty());

        assert_eq!(compact_color(&Color::Ansi(200)), 0);
        assert_eq!(compact_color(&Color::Indexed(9)), 0);
    }

    #[test]
    fn test_palette_lookup_and_growth() {
        let mut palette = ColorPalette::default();
        assert_eq!(palette.get_color(3), Color::Ansi(1));
        assert_eq!(palette.get_color(250), Color::Default);
        // Cor já existente devolve o índice conhecido
        assert_eq!(palette.add_color(Color::Ansi(1)), 3);

        let first = palette.add_color(Color::Rgb(1, 1, 1));
        assert_eq!(first, 18);
        assert_eq!(palette.get_color(first), Color::Rgb(1, 1, 1));
        for i in 0..=255u8 {
            palette.add_color(Color::Indexed(i));
        }
        // Paleta cheia: fallback para o índice 0
        assert_eq!(palette.add_color(Color::Rgb(9, 9, 9)), 0);
    }

    /// Regressão: a heurística somava o índice final das regiões, então uma
    /// única célula alterada no fim da linha descartava o delta.
    #[test]
    fn test_delta_for_change_at_end_of_line() {
        let mut scrollback = CompactScrollback::with_limit(10);
        let first = varied_line(20, Color::Default);
        let mut second = first.clone();
        second[19] = create_test_cell('Z', Color::Default, Color::Default);
        scrollback.push_line(&first);
        scrollback.push_line(&second);

        match scrollback.lines.back().unwrap() {
            CompactLine::Delta { back_ref, changes } => {
                assert_eq!(*back_ref, 1);
                assert_eq!(changes.len(), 1);
                assert_eq!((changes[0].0, changes[0].1), (19, 20));
            }
            other => panic!("esperava delta, veio {:?}", other),
        }
        let stats = scrollback.stats();
        assert_eq!((stats.full_lines, stats.delta_lines), (1, 1));
        assert!(stats.compression_ratio > 0.0);
    }

    #[test]
    fn test_delta_rejected_for_different_or_heavily_changed_lines() {
        let mut scrollback = CompactScrollback::with_limit(10);
        scrollback.push_line(&varied_line(20, Color::Default));
        // Tamanho diferente: linha completa
        scrollback.push_line(&varied_line(21, Color::Default));
        // Mesma largura, mas tudo mudou de cor: linha completa
        scrollback.push_line(&varied_line(21, Color::Ansi(2)));
        // Mudança no meio da linha: delta com região fechada antes do fim
        let mut middle = varied_line(21, Color::Ansi(2));
        middle[5] = create_test_cell('#', Color::Ansi(2), Color::Default);
        scrollback.push_line(&middle);

        let stats = scrollback.stats();
        assert_eq!(stats.full_lines, 3);
        assert_eq!(stats.delta_lines, 1);
        match scrollback.lines.back().unwrap() {
            CompactLine::Delta { changes, .. } => assert_eq!((changes[0].0, changes[0].1), (5, 6)),
            other => panic!("esperava delta, veio {:?}", other),
        }

        // Delta não usa como referência uma linha que não é completa
        let mut after_empty = CompactScrollback::with_limit(10);
        after_empty.push_line(&(0..10).map(|_| Cell::empty()).collect::<Vec<_>>());
        after_empty.push_line(&varied_line(10, Color::Default));
        assert_eq!(after_empty.stats().full_lines, 1);
    }

    #[test]
    fn test_accessors_clear_and_memory() {
        let mut scrollback = CompactScrollback::default();
        assert!(scrollback.is_empty());
        assert_eq!(scrollback.limit(), 10000);
        assert_eq!(scrollback.palette().get_color(0), Color::Default);
        assert_eq!(scrollback.stats().compression_ratio, 1.0);
        let base = scrollback.estimated_memory_usage();

        scrollback.push_line(&varied_line(30, Color::Default));
        assert!(!scrollback.is_empty());
        assert!(scrollback.estimated_memory_usage() > base);

        scrollback.clear();
        assert_eq!(scrollback.len(), 0);
        assert_eq!(scrollback.estimated_memory_usage(), base);
    }

    #[test]
    fn test_compress_older_lines_converts_old_full_lines_to_rle() {
        // Linhas "Full" com runs só aparecem quando o RLE inicial não foi
        // escolhido; força isso inserindo direto e depois comprimindo.
        let mut scrollback = CompactScrollback::with_limit(40);
        let run: Vec<CompactCell> = std::iter::repeat_n(
            CompactCell {
                char_code: 'x' as u32,
                fg_idx: 0,
                bg_idx: 0,
                attr_flags: 0,
            },
            64,
        )
        .collect();
        for _ in 0..30 {
            let line = CompactLine::Full(run.clone());
            scrollback.estimated_size += estimate_line_size(&line);
            scrollback.lines.push_back(line);
            scrollback.last_access.push_back(0);
        }
        scrollback.access_count = 1000;
        let before = scrollback.estimated_size;

        scrollback.compress_older_lines();

        let stats = scrollback.stats();
        assert_eq!(
            stats.rle_lines, 20,
            "as 10 linhas mais recentes ficam como estão"
        );
        assert_eq!(stats.full_lines, 10);
        assert!(scrollback.estimated_size < before);
        assert!(!should_use_rle_full(&run[..3]));
        assert!(encode_rle_compact(&[]).is_empty());
        assert!(encode_rle(&[]).is_empty());
    }

    #[test]
    fn test_push_triggers_compression_when_over_budget() {
        // Limite 1 => orçamento de 100 bytes, estourado já na primeira linha
        let mut scrollback = CompactScrollback::with_limit(1);
        scrollback.push_line(&varied_line(40, Color::Default));
        scrollback.push_line(&varied_line(40, Color::Ansi(1)));
        assert_eq!(scrollback.len(), 1);
    }
}
