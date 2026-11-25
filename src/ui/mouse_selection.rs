//! Mouse Selection Handler
//!
//! Gerencia seleção de texto via mouse, incluindo:
//! - Click-and-drag para selecionar texto
//! - Double-click para selecionar palavra
//! - Triple-click para selecionar linha
//! - Right-click para context menu
//! - Seleção retangular (Alt+drag)

use crate::terminal::grid::{Grid, SelectionKind};
use std::time::{Duration, Instant};

/// Botão do mouse
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Evento de mouse para seleção
#[derive(Debug, Clone, Copy)]
pub enum MouseSelectionEvent {
    /// Botão pressionado
    Press {
        row: usize,
        col: usize,
        button: MouseButton,
        alt: bool,
    },
    /// Botão solto
    Release {
        row: usize,
        col: usize,
        button: MouseButton,
    },
    /// Movimento com botão pressionado
    Drag { row: usize, col: usize, alt: bool },
}

/// Estado do handler de seleção
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionState {
    /// Inativo (sem seleção)
    Inactive,
    /// Selecionando (mouse pressionado)
    Selecting,
    /// Seleção finalizada (mouse solto)
    Finished,
}

/// Handler de seleção por mouse
#[derive(Debug)]
pub struct MouseSelectionHandler {
    /// Estado atual
    state: SelectionState,
    /// Seleção retangular ativa
    rectangular: bool,
    /// Último click (para detectar double/triple)
    last_click: Option<(Instant, usize, usize)>,
    /// Contador de clicks consecutivos
    click_count: u32,
    /// Botão atualmente pressionado
    pressed_button: Option<MouseButton>,
    /// Timeout para resetar contador de clicks
    click_timeout: Duration,
}

impl Default for MouseSelectionHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseSelectionHandler {
    /// Cria um novo handler
    pub fn new() -> Self {
        Self {
            state: SelectionState::Inactive,
            rectangular: false,
            last_click: None,
            click_count: 0,
            pressed_button: None,
            click_timeout: Duration::from_millis(500),
        }
    }

    /// Processa evento de mouse e atualiza o grid
    pub fn handle_event(&mut self, event: MouseSelectionEvent, grid: &mut Grid) {
        match event {
            MouseSelectionEvent::Press {
                row,
                col,
                button,
                alt,
            } => {
                self.handle_press(row, col, button, alt, grid);
            }
            MouseSelectionEvent::Release { row, col, button } => {
                self.handle_release(row, col, button, grid);
            }
            MouseSelectionEvent::Drag { row, col, alt } => {
                self.handle_drag(row, col, alt, grid);
            }
        }
    }

    /// Processa pressionamento de botão
    fn handle_press(
        &mut self,
        row: usize,
        col: usize,
        button: MouseButton,
        alt: bool,
        grid: &mut Grid,
    ) {
        if button != MouseButton::Left {
            return;
        }

        self.pressed_button = Some(button);
        self.rectangular = alt;

        // Verifica se é double-click ou triple-click
        let is_multi_click = self.check_multi_click(row, col);

        if is_multi_click {
            if self.click_count == 2 {
                // Double-click: seleciona palavra
                grid.select_word_at(row, col);
                self.state = SelectionState::Finished;
            } else if self.click_count >= 3 {
                // Triple-click: seleciona linha
                grid.select_line_at(row);
                self.state = SelectionState::Finished;
            }
        } else {
            // Click simples: inicia nova seleção
            grid.clear_selection();
            let _ = alt;
            let kind = SelectionKind::Normal;
            grid.start_selection(row, col, kind);
            self.state = SelectionState::Selecting;
        }
    }

    /// Processa soltura de botão
    fn handle_release(&mut self, _row: usize, _col: usize, button: MouseButton, grid: &mut Grid) {
        if button != MouseButton::Left {
            return;
        }

        self.pressed_button = None;

        if self.state == SelectionState::Selecting {
            grid.end_selection();
            self.state = SelectionState::Finished;
        }
    }

    /// Processa movimento de drag
    fn handle_drag(&mut self, row: usize, col: usize, alt: bool, grid: &mut Grid) {
        if self.state != SelectionState::Selecting {
            return;
        }

        // Atualiza seleção
        if self.rectangular || alt {
            // Seleção retangular
            self.update_rectangular_selection(row, col, grid);
        } else {
            // Seleção normal
            grid.update_selection(row, col);
        }
    }

    /// Atualiza seleção retangular
    fn update_rectangular_selection(&mut self, row: usize, col: usize, grid: &mut Grid) {
        // Para seleção retangular, mantemos as coordenadas exatas
        // sem expandir para linhas completas
        grid.update_selection(row, col);
    }

    /// Verifica se é multi-click (double ou triple)
    fn check_multi_click(&mut self, row: usize, col: usize) -> bool {
        let now = Instant::now();

        if let Some((last_time, last_row, last_col)) = self.last_click {
            let elapsed = now.duration_since(last_time);

            if elapsed < self.click_timeout && last_row == row && last_col == col {
                // Mesma posição dentro do timeout
                self.click_count += 1;
            } else {
                // Posição diferente ou timeout expirado
                self.click_count = 1;
            }
        } else {
            self.click_count = 1;
        }

        self.last_click = Some((now, row, col));

        // Reseta contador após triple-click
        if self.click_count > 3 {
            self.click_count = 1;
            return false;
        }

        self.click_count >= 2
    }

    /// Retorna se está em modo de seleção
    pub fn is_selecting(&self) -> bool {
        self.state == SelectionState::Selecting
    }

    /// Retorna se há seleção finalizada
    pub fn has_selection(&self) -> bool {
        self.state == SelectionState::Finished
    }

    /// Retorna o estado atual
    pub fn state(&self) -> SelectionState {
        self.state
    }

    /// Reseta o estado (chamar quando perde foco, etc)
    pub fn reset(&mut self) {
        self.state = SelectionState::Inactive;
        self.pressed_button = None;
    }

    /// Cancela seleção atual
    pub fn cancel_selection(&mut self, grid: &mut Grid) {
        grid.clear_selection();
        self.state = SelectionState::Inactive;
    }
}

/// Converte coordenadas de pixel para célula do grid
pub fn pixel_to_cell(
    x: f64,
    y: f64,
    cell_width: f64,
    cell_height: f64,
    rows: usize,
    cols: usize,
) -> (usize, usize) {
    let col = (x / cell_width) as usize;
    let row = (y / cell_height) as usize;
    (
        row.min(rows.saturating_sub(1)),
        col.min(cols.saturating_sub(1)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_creation() {
        let handler = MouseSelectionHandler::new();
        assert!(!handler.is_selecting());
        assert!(!handler.has_selection());
        assert_eq!(handler.state(), SelectionState::Inactive);
    }

    #[test]
    fn test_pixel_to_cell() {
        let (row, col) = pixel_to_cell(100.0, 50.0, 10.0, 20.0, 24, 80);
        assert_eq!(col, 10);
        assert_eq!(row, 2);
    }

    #[test]
    fn test_pixel_to_cell_clamped() {
        // Fora dos limites
        let (row, col) = pixel_to_cell(1000.0, 500.0, 10.0, 20.0, 24, 80);
        assert_eq!(col, 79);
        assert_eq!(row, 23);
    }

    #[test]
    fn test_click_starts_selection() {
        let mut handler = MouseSelectionHandler::new();
        let mut grid = Grid::new(80, 24);

        handler.handle_event(
            MouseSelectionEvent::Press {
                row: 5,
                col: 10,
                button: MouseButton::Left,
                alt: false,
            },
            &mut grid,
        );

        assert!(handler.is_selecting());
        assert!(grid.has_selection());
    }

    #[test]
    fn test_drag_updates_selection() {
        let mut handler = MouseSelectionHandler::new();
        let mut grid = Grid::new(80, 24);

        // Pressiona
        handler.handle_event(
            MouseSelectionEvent::Press {
                row: 0,
                col: 0,
                button: MouseButton::Left,
                alt: false,
            },
            &mut grid,
        );

        // Arrasta
        handler.handle_event(
            MouseSelectionEvent::Drag {
                row: 0,
                col: 5,
                alt: false,
            },
            &mut grid,
        );

        assert!(handler.is_selecting());
    }

    #[test]
    fn test_release_finishes_selection() {
        let mut handler = MouseSelectionHandler::new();
        let mut grid = Grid::new(80, 24);

        // Pressiona
        handler.handle_event(
            MouseSelectionEvent::Press {
                row: 0,
                col: 0,
                button: MouseButton::Left,
                alt: false,
            },
            &mut grid,
        );

        // Solta
        handler.handle_event(
            MouseSelectionEvent::Release {
                row: 0,
                col: 5,
                button: MouseButton::Left,
            },
            &mut grid,
        );

        assert!(!handler.is_selecting());
        assert!(handler.has_selection());
    }

    #[test]
    fn test_cancel_selection() {
        let mut handler = MouseSelectionHandler::new();
        let mut grid = Grid::new(80, 24);

        handler.handle_event(
            MouseSelectionEvent::Press {
                row: 0,
                col: 0,
                button: MouseButton::Left,
                alt: false,
            },
            &mut grid,
        );

        assert!(grid.has_selection());

        handler.cancel_selection(&mut grid);

        assert!(!handler.has_selection());
        assert!(!grid.has_selection());
    }
}
