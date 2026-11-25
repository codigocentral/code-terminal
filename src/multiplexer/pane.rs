//! Session Panes - Sistema de painéis do multiplexador
//!
//! Este módulo implementa o sistema de painéis (panes) similar ao tmux,
//! onde cada janela pode ter múltiplos painéis dividindo a tela.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{MultiplexerError, MultiplexerResult};
pub use crate::types::PaneId;

/// Contador global para geração de IDs de painel
static PANE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Gera um novo ID de painel único
fn generate_pane_id() -> PaneId {
    PANE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Estado de um painel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneState {
    /// Ativo (recebendo input)
    Active,
    /// Em background (visível mas não ativo)
    Background,
    /// Em zoom (ocupa toda a janela)
    Zoomed,
    /// Destruído
    Destroyed,
}

impl std::fmt::Display for PaneState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaneState::Active => write!(f, "active"),
            PaneState::Background => write!(f, "background"),
            PaneState::Zoomed => write!(f, "zoomed"),
            PaneState::Destroyed => write!(f, "destroyed"),
        }
    }
}

/// Tipo de conteúdo do painel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneContent {
    /// Shell padrão
    Shell,
    /// Comando específico
    Command,
    /// Visualizador de log
    Log,
    /// Monitor de sistema
    Monitor,
}

/// Um painel no multiplexador
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pane {
    /// ID único
    pub id: PaneId,
    /// Estado atual
    pub state: PaneState,
    /// Posição na janela (x, y) em células
    pub position: (u16, u16),
    /// Tamanho (cols, rows) em células
    pub size: (u16, u16),
    /// Tipo de conteúdo
    pub content_type: PaneContent,
    /// Comando em execução (se houver)
    pub command: Option<String>,
    /// PID do processo
    pub pid: Option<u32>,
    /// Título do painel
    pub title: String,
    /// Buffer de scrollback
    pub scrollback: ScrollbackBuffer,
    /// Posição de scroll
    pub scroll_position: usize,
    /// Se está em modo de cópia
    pub copy_mode: bool,
    /// Seleção ativa
    pub selection: Option<Selection>,
    /// Cursor (col, row)
    pub cursor: (u16, u16),
    /// Modo de cursor
    pub cursor_mode: CursorMode,
    /// Histórico de comandos
    pub command_history: Vec<String>,
    /// Flags
    pub flags: PaneFlags,
    /// Quando foi criado
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Metadados
    pub metadata: HashMap<String, String>,
}

/// Flags de um painel
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PaneFlags {
    /// Painel está sincronizado (broadcast input)
    pub synchronized: bool,
    /// Painel está monitorado para atividade
    pub monitor_activity: bool,
    /// Painel está monitorado para silêncio
    pub monitor_silence: bool,
    /// Painel permite scroll
    pub allow_scroll: bool,
    /// Painel é somente leitura
    pub read_only: bool,
}

/// Buffer de scrollback
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollbackBuffer {
    /// Linhas armazenadas
    pub lines: Vec<Line>,
    /// Capacidade máxima
    pub capacity: usize,
    /// Número atual de linhas
    pub count: usize,
}

/// Uma linha no buffer
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Line {
    /// Células da linha
    pub cells: Vec<Cell>,
    /// Se a linha foi quebrada (wrap)
    pub wrapped: bool,
}

/// Uma célula de terminal
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cell {
    /// Caractere
    pub ch: char,
    /// Atributos de estilo
    pub attributes: CellAttributes,
}

/// Atributos de célula
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CellAttributes {
    /// Cor de foreground (índice 0-255 ou RGB)
    pub fg_color: Option<Color>,
    /// Cor de background (índice 0-255 ou RGB)
    pub bg_color: Option<Color>,
    /// Negrito
    pub bold: bool,
    /// Itálico
    pub italic: bool,
    /// Sublinhado
    pub underline: bool,
    /// Piscando
    pub blink: bool,
    /// Invertido
    pub reverse: bool,
    /// Invisível
    pub invisible: bool,
    /// Riscado
    pub strikethrough: bool,
}

/// Cor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    /// Índice de cor (0-255)
    Indexed(u8),
    /// RGB
    Rgb(u8, u8, u8),
    /// Cor padrão
    Default,
}

/// Seleção de texto
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Selection {
    /// Início (col, row)
    pub start: (u16, u16),
    /// Fim (col, row)
    pub end: (u16, u16),
    /// Tipo de seleção
    pub selection_type: SelectionType,
}

/// Tipo de seleção
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectionType {
    /// Seleção caractere a caractere
    Character,
    /// Seleção por linha
    Line,
    /// Seleção em bloco
    Block,
}

/// Modo de cursor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CursorMode {
    /// Cursor em bloco
    #[default]
    Block,
    /// Cursor em linha
    Line,
    /// Cursor em barra
    Bar,
}

impl ScrollbackBuffer {
    /// Cria um novo buffer de scrollback
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: Vec::with_capacity(capacity),
            capacity,
            count: 0,
        }
    }

    /// Adiciona uma linha ao buffer
    pub fn push_line(&mut self, line: Line) {
        if self.lines.len() >= self.capacity {
            self.lines.remove(0);
        }
        self.lines.push(line);
        self.count = self.lines.len();
    }

    /// Obtém uma linha pelo índice
    pub fn get_line(&self, index: usize) -> Option<&Line> {
        self.lines.get(index)
    }

    /// Limpa o buffer
    pub fn clear(&mut self) {
        self.lines.clear();
        self.count = 0;
    }

    /// Retorna o número de linhas
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Retorna se está vazio
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

impl Pane {
    /// Cria um novo painel
    pub fn new(size: (u16, u16), position: (u16, u16)) -> Self {
        let id = generate_pane_id();

        Self {
            id,
            state: PaneState::Background,
            position,
            size,
            content_type: PaneContent::Shell,
            command: None,
            pid: None,
            title: String::new(),
            scrollback: ScrollbackBuffer::new(10000),
            scroll_position: 0,
            copy_mode: false,
            selection: None,
            cursor: (0, 0),
            cursor_mode: CursorMode::default(),
            command_history: Vec::new(),
            flags: PaneFlags::default(),
            created_at: chrono::Utc::now(),
            last_activity: chrono::Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Cria um painel com comando
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.content_type = PaneContent::Command;
        self.command = Some(command.into());
        self
    }

    /// Define o título
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Ativa o painel
    pub fn activate(&mut self) {
        self.state = PaneState::Active;
        self.touch();
    }

    /// Desativa o painel
    pub fn deactivate(&mut self) {
        if self.state == PaneState::Active {
            self.state = PaneState::Background;
        }
    }

    /// Alterna o zoom
    pub fn toggle_zoom(&mut self) {
        self.state = match self.state {
            PaneState::Zoomed => PaneState::Active,
            _ => PaneState::Zoomed,
        };
        self.touch();
    }

    /// Redimensiona o painel
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = (cols, rows);
        self.touch();
    }

    /// Move o painel
    pub fn move_to(&mut self, x: u16, y: u16) {
        self.position = (x, y);
        self.touch();
    }

    /// Atualiza a última atividade
    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now();
    }

    /// Retorna o tempo desde a última atividade
    pub fn idle_time(&self) -> std::time::Duration {
        let now = chrono::Utc::now();
        now.signed_duration_since(self.last_activity)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(0))
    }

    /// Scroll para cima
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_position = self.scroll_position.saturating_add(lines);
        let max_scroll = self.scrollback.len().saturating_sub(self.size.1 as usize);
        self.scroll_position = self.scroll_position.min(max_scroll);
    }

    /// Scroll para baixo
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_position = self.scroll_position.saturating_sub(lines);
    }

    /// Scroll para o topo
    pub fn scroll_to_top(&mut self) {
        self.scroll_position = self.scrollback.len().saturating_sub(self.size.1 as usize);
    }

    /// Scroll para o final
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_position = 0;
    }

    /// Entra em modo de cópia
    pub fn enter_copy_mode(&mut self) {
        self.copy_mode = true;
        self.scroll_to_top();
        self.selection = Some(Selection {
            start: self.cursor,
            end: self.cursor,
            selection_type: SelectionType::Character,
        });
    }

    /// Sai do modo de cópia
    pub fn exit_copy_mode(&mut self) {
        self.copy_mode = false;
        self.selection = None;
        self.scroll_to_bottom();
    }

    /// Move o cursor
    pub fn move_cursor(&mut self, col: i16, row: i16) {
        let (cols, rows) = self.size;

        // Em i32 para não estourar nem fazer `clamp` com máximo < 0 (que
        // entra em pânico) quando o painel tem tamanho 0.
        let max_col = i32::from(cols.saturating_sub(1));
        let max_row = i32::from(rows.saturating_sub(1));
        let new_col = (i32::from(self.cursor.0) + i32::from(col)).clamp(0, max_col) as u16;
        let new_row = (i32::from(self.cursor.1) + i32::from(row)).clamp(0, max_row) as u16;

        self.cursor = (new_col, new_row);

        // Atualiza seleção se estiver em modo de cópia
        if self.copy_mode {
            if let Some(ref mut sel) = self.selection {
                sel.end = self.cursor;
            }
        }
    }

    /// Define o cursor
    pub fn set_cursor(&mut self, col: u16, row: u16) {
        let (cols, rows) = self.size;
        self.cursor = (
            col.min(cols.saturating_sub(1)),
            row.min(rows.saturating_sub(1)),
        );
    }

    /// Limpa o painel
    pub fn clear(&mut self) {
        self.scrollback.clear();
        self.scroll_position = 0;
        self.cursor = (0, 0);
        self.touch();
    }

    /// Retorna o texto selecionado
    pub fn get_selection_text(&self) -> Option<String> {
        let sel = self.selection.as_ref()?;

        // A seleção pode ter sido feita "para trás" (cursor acima/à esquerda
        // do início): normaliza para que start venha antes de end.
        let (start, end) = if (sel.start.1, sel.start.0) <= (sel.end.1, sel.end.0) {
            (sel.start, sel.end)
        } else {
            (sel.end, sel.start)
        };
        let (start_col, start_row) = start;
        let (end_col, end_row) = end;

        let mut text = String::new();

        match sel.selection_type {
            SelectionType::Character => {
                for row in start_row..=end_row {
                    if let Some(line) = self.scrollback.get_line(row as usize) {
                        let start = if row == start_row { start_col } else { 0 };
                        // A célula final (posição do cursor) faz parte da seleção
                        let end = if row == end_row {
                            end_col.saturating_add(1)
                        } else {
                            self.size.0
                        };

                        for col in start..end {
                            if let Some(cell) = line.cells.get(col as usize) {
                                text.push(cell.ch);
                            }
                        }

                        if row < end_row {
                            text.push('\n');
                        }
                    }
                }
            }
            SelectionType::Line => {
                for row in start_row..=end_row {
                    if let Some(line) = self.scrollback.get_line(row as usize) {
                        for cell in &line.cells {
                            text.push(cell.ch);
                        }
                        if row < end_row {
                            text.push('\n');
                        }
                    }
                }
            }
            SelectionType::Block => {
                let (left, right) = (start_col.min(end_col), start_col.max(end_col));
                for row in start_row..=end_row {
                    if let Some(line) = self.scrollback.get_line(row as usize) {
                        for col in left..=right {
                            if let Some(cell) = line.cells.get(col as usize) {
                                text.push(cell.ch);
                            }
                        }
                        if row < end_row {
                            text.push('\n');
                        }
                    }
                }
            }
        }

        Some(text)
    }

    /// Verifica se um ponto está dentro do painel
    pub fn contains_point(&self, x: u16, y: u16) -> bool {
        let (px, py) = self.position;
        let (pw, ph) = self.size;

        // Em u32: posição + tamanho pode passar de u16::MAX
        let (x, y) = (u32::from(x), u32::from(y));
        let (px, py, pw, ph) = (u32::from(px), u32::from(py), u32::from(pw), u32::from(ph));
        x >= px && x < px + pw && y >= py && y < py + ph
    }

    /// Retorna a área do painel
    pub fn area(&self) -> u32 {
        self.size.0 as u32 * self.size.1 as u32
    }
}

/// Gerenciador de painéis
#[derive(Debug, Default)]
pub struct PaneManager {
    /// Painéis por ID
    panes: HashMap<PaneId, Pane>,
    /// Painel ativo
    active_pane: Option<PaneId>,
    /// Último painel ativo
    last_pane: Option<PaneId>,
}

impl PaneManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            panes: HashMap::new(),
            active_pane: None,
            last_pane: None,
        }
    }

    /// Cria um novo painel
    pub fn create_pane(&mut self, size: (u16, u16), position: (u16, u16)) -> PaneId {
        let pane = Pane::new(size, position);
        let id = pane.id;

        self.panes.insert(id, pane);

        // Se for o primeiro, ativa
        if self.active_pane.is_none() {
            self.active_pane = Some(id);
        }

        id
    }

    /// Obtém um painel
    pub fn get_pane(&self, id: PaneId) -> Option<&Pane> {
        self.panes.get(&id)
    }

    /// Obtém um painel mutável
    pub fn get_pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.panes.get_mut(&id)
    }

    /// Remove um painel
    pub fn remove_pane(&mut self, id: PaneId) -> MultiplexerResult<Pane> {
        let pane = self
            .panes
            .remove(&id)
            .ok_or_else(|| MultiplexerError::ServerError(format!("Pane {} not found", id)))?;

        if self.active_pane == Some(id) {
            self.active_pane = self
                .last_pane
                .take()
                .or_else(|| self.panes.keys().next().copied());
        }

        if self.last_pane == Some(id) {
            self.last_pane = None;
        }

        Ok(pane)
    }

    /// Lista todos os painéis
    pub fn list_panes(&self) -> Vec<&Pane> {
        self.panes.values().collect()
    }

    /// Seleciona um painel
    pub fn select_pane(&mut self, id: PaneId) -> MultiplexerResult<()> {
        if !self.panes.contains_key(&id) {
            return Err(MultiplexerError::ServerError(format!(
                "Pane {} not found",
                id
            )));
        }

        if let Some(current) = self.active_pane {
            self.last_pane = Some(current);
            if let Some(pane) = self.panes.get_mut(&current) {
                pane.deactivate();
            }
        }

        self.active_pane = Some(id);
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.activate();
        }

        Ok(())
    }

    /// Retorna o painel ativo
    pub fn active_pane(&self) -> Option<&Pane> {
        self.active_pane.and_then(|id| self.panes.get(&id))
    }

    /// Retorna o painel ativo mutável
    pub fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.active_pane.and_then(move |id| self.panes.get_mut(&id))
    }

    /// Encontra painel por posição
    pub fn find_by_position(&self, x: u16, y: u16) -> Option<&Pane> {
        self.panes.values().find(|p| p.contains_point(x, y))
    }

    /// Redimensiona todos os painéis proporcionalmente
    pub fn resize_all(&mut self, old_size: (u16, u16), new_size: (u16, u16)) {
        let (old_cols, old_rows) = old_size;
        let (new_cols, new_rows) = new_size;
        if old_cols == 0 || old_rows == 0 {
            // Sem tamanho anterior não há proporção a aplicar
            return;
        }

        let col_ratio = new_cols as f32 / old_cols as f32;
        let row_ratio = new_rows as f32 / old_rows as f32;

        for pane in self.panes.values_mut() {
            let new_pane_cols = (pane.size.0 as f32 * col_ratio) as u16;
            let new_pane_rows = (pane.size.1 as f32 * row_ratio) as u16;
            let new_x = (pane.position.0 as f32 * col_ratio) as u16;
            let new_y = (pane.position.1 as f32 * row_ratio) as u16;

            pane.size = (new_pane_cols.max(1), new_pane_rows.max(1));
            pane.position = (new_x, new_y);
        }
    }

    /// Retorna o número de painéis
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_creation() {
        let pane = Pane::new((80, 24), (0, 0));
        assert_eq!(pane.size, (80, 24));
        assert_eq!(pane.position, (0, 0));
        assert_eq!(pane.state, PaneState::Background);
    }

    #[test]
    fn test_pane_contains_point() {
        let pane = Pane::new((80, 24), (10, 5));

        assert!(pane.contains_point(10, 5));
        assert!(pane.contains_point(89, 28));
        assert!(!pane.contains_point(90, 28));
        assert!(!pane.contains_point(10, 29));
    }

    #[test]
    fn test_scrollback_buffer() {
        let mut buffer = ScrollbackBuffer::new(100);

        buffer.push_line(Line::default());
        buffer.push_line(Line::default());

        assert_eq!(buffer.len(), 2);
    }

    #[test]
    fn test_pane_manager() {
        let mut manager = PaneManager::new();

        let _id1 = manager.create_pane((80, 24), (0, 0));
        let id2 = manager.create_pane((80, 24), (0, 24));

        assert_eq!(manager.pane_count(), 2);

        manager.select_pane(id2).unwrap();
        assert_eq!(manager.active_pane().unwrap().id, id2);

        manager.remove_pane(id2).unwrap();
        assert_eq!(manager.pane_count(), 1);
    }

    #[test]
    fn test_find_by_position() {
        let mut manager = PaneManager::new();
        manager.create_pane((40, 12), (0, 0));
        manager.create_pane((40, 12), (40, 0));

        let found = manager.find_by_position(20, 5);
        assert!(found.is_some());
        assert_eq!(found.unwrap().position, (0, 0));
    }

    fn line(text: &str) -> Line {
        Line {
            cells: text
                .chars()
                .map(|ch| Cell {
                    ch,
                    attributes: CellAttributes::default(),
                })
                .collect(),
            wrapped: false,
        }
    }

    fn pane_with_lines(lines: &[&str]) -> Pane {
        let mut pane = Pane::new((6, 3), (0, 0));
        for text in lines {
            pane.scrollback.push_line(line(text));
        }
        pane
    }

    fn select(pane: &mut Pane, start: (u16, u16), end: (u16, u16), kind: SelectionType) -> String {
        pane.selection = Some(Selection {
            start,
            end,
            selection_type: kind,
        });
        pane.get_selection_text().expect("seleção")
    }

    #[test]
    fn test_display_and_builders() {
        assert_eq!(PaneState::Active.to_string(), "active");
        assert_eq!(PaneState::Background.to_string(), "background");
        assert_eq!(PaneState::Zoomed.to_string(), "zoomed");
        assert_eq!(PaneState::Destroyed.to_string(), "destroyed");

        let pane = Pane::new((10, 5), (1, 2))
            .with_command("htop")
            .with_title("monitor");
        assert_eq!(pane.content_type, PaneContent::Command);
        assert_eq!(pane.command.as_deref(), Some("htop"));
        assert_eq!(pane.title, "monitor");
        assert_eq!(pane.area(), 50);
    }

    #[test]
    fn test_state_resize_move_and_idle() {
        let mut pane = Pane::new((10, 5), (0, 0));
        pane.toggle_zoom();
        assert_eq!(pane.state, PaneState::Zoomed);
        pane.toggle_zoom();
        assert_eq!(pane.state, PaneState::Active);
        pane.deactivate();
        assert_eq!(pane.state, PaneState::Background);

        pane.resize(20, 8);
        pane.move_to(3, 4);
        assert_eq!((pane.size, pane.position), ((20, 8), (3, 4)));

        pane.last_activity = chrono::Utc::now() + chrono::Duration::hours(1);
        assert_eq!(pane.idle_time(), std::time::Duration::ZERO);
    }

    #[test]
    fn test_scrollback_capacity_get_and_clear() {
        let mut buffer = ScrollbackBuffer::new(2);
        assert!(buffer.is_empty());
        for text in ["a", "b", "c"] {
            buffer.push_line(line(text));
        }
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.get_line(0).unwrap().cells[0].ch, 'b');
        assert!(buffer.get_line(5).is_none());
        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.count, 0);
    }

    #[test]
    fn test_scrolling_and_copy_mode() {
        let mut pane = pane_with_lines(&["1", "2", "3", "4", "5"]);
        pane.scroll_up(10);
        assert_eq!(pane.scroll_position, 2, "limitado a linhas - altura");
        pane.scroll_down(1);
        assert_eq!(pane.scroll_position, 1);
        pane.scroll_to_bottom();
        assert_eq!(pane.scroll_position, 0);

        pane.set_cursor(1, 1);
        pane.enter_copy_mode();
        assert!(pane.copy_mode);
        assert_eq!(pane.scroll_position, 2);
        pane.move_cursor(2, 1);
        assert_eq!(pane.selection.unwrap().end, (3, 2));
        pane.move_cursor(-10, -10);
        assert_eq!(pane.cursor, (0, 0));
        pane.exit_copy_mode();
        assert!(pane.selection.is_none());
        assert_eq!(pane.scroll_position, 0);
    }

    #[test]
    fn test_cursor_is_clamped_to_pane() {
        let mut pane = Pane::new((5, 3), (0, 0));
        pane.set_cursor(100, 100);
        assert_eq!(pane.cursor, (4, 2));
        pane.move_cursor(i16::MAX, i16::MAX);
        assert_eq!(pane.cursor, (4, 2));

        // Regressão: painel de tamanho 0 fazia `clamp(0, -1)` e entrava em pânico
        let mut empty = Pane::new((0, 0), (0, 0));
        empty.move_cursor(1, 1);
        assert_eq!(empty.cursor, (0, 0));

        pane.clear();
        assert_eq!(pane.cursor, (0, 0));
        assert!(pane.scrollback.is_empty());
    }

    #[test]
    fn test_selection_text_by_type() {
        let mut pane = pane_with_lines(&["abcdef", "ghijkl", "mnopqr"]);
        assert!(Pane::new((1, 1), (0, 0)).get_selection_text().is_none());

        assert_eq!(
            select(&mut pane, (2, 0), (1, 1), SelectionType::Character),
            "cdef\ngh"
        );
        assert_eq!(
            select(&mut pane, (4, 0), (4, 0), SelectionType::Character),
            "e"
        );
        assert_eq!(
            select(&mut pane, (3, 0), (0, 1), SelectionType::Line),
            "abcdef\nghijkl"
        );
        assert_eq!(
            select(&mut pane, (1, 0), (2, 2), SelectionType::Block),
            "bc\nhi\nno"
        );
    }

    /// Regressão: seleção feita de baixo para cima (ou da direita para a
    /// esquerda) devolvia texto vazio.
    #[test]
    fn test_backwards_selection_is_normalized() {
        let mut pane = pane_with_lines(&["abcdef", "ghijkl", "mnopqr"]);
        assert_eq!(
            select(&mut pane, (1, 1), (2, 0), SelectionType::Character),
            "cdef\ngh"
        );
        assert_eq!(
            select(&mut pane, (2, 2), (1, 0), SelectionType::Block),
            "bc\nhi\nno"
        );
        assert_eq!(
            select(&mut pane, (3, 2), (1, 2), SelectionType::Block),
            "nop"
        );
    }

    #[test]
    fn test_contains_point_near_u16_limit() {
        let pane = Pane::new((10, 10), (u16::MAX - 5, u16::MAX - 5));
        assert!(pane.contains_point(u16::MAX, u16::MAX));
        assert!(!pane.contains_point(0, 0));
    }

    #[test]
    fn test_manager_lookup_remove_and_select() {
        let mut manager = PaneManager::default();
        let a = manager.create_pane((40, 12), (0, 0));
        let b = manager.create_pane((40, 12), (40, 0));
        let c = manager.create_pane((40, 12), (0, 12));
        assert_eq!(manager.list_panes().len(), 3);
        manager.get_pane_mut(b).unwrap().title = "b".to_string();
        assert_eq!(manager.get_pane(b).unwrap().title, "b");

        manager.select_pane(b).unwrap();
        manager.select_pane(c).unwrap();
        assert_eq!(manager.get_pane(b).unwrap().state, PaneState::Background);
        manager.active_pane_mut().unwrap().title = "c".to_string();

        // Remover o ativo volta para o último
        manager.remove_pane(c).unwrap();
        assert_eq!(manager.active_pane().unwrap().id, b);
        // Remover o "último" (não ativo) só limpa a referência
        manager.select_pane(a).unwrap();
        manager.remove_pane(b).unwrap();
        assert_eq!(manager.active_pane().unwrap().id, a);
        // Remover o ativo sem "último" escolhe outro qualquer (ou nenhum)
        manager.remove_pane(a).unwrap();
        assert!(manager.active_pane().is_none());

        assert!(manager.remove_pane(a).is_err());
        assert!(manager.select_pane(a).is_err());
    }

    #[test]
    fn test_resize_all_scales_panes() {
        let mut manager = PaneManager::new();
        let left = manager.create_pane((40, 12), (0, 0));
        let right = manager.create_pane((40, 12), (40, 12));
        manager.resize_all((80, 24), (160, 48));
        assert_eq!(manager.get_pane(left).unwrap().size, (80, 24));
        assert_eq!(manager.get_pane(right).unwrap().position, (80, 24));

        manager.resize_all((160, 48), (1, 1));
        assert_eq!(
            manager.get_pane(left).unwrap().size,
            (1, 1),
            "nunca menor que 1"
        );

        // Tamanho anterior zerado: nada muda
        manager.resize_all((0, 24), (80, 24));
        assert_eq!(manager.get_pane(left).unwrap().size, (1, 1));
    }
}
