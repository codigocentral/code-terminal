pub mod cells;
pub mod gpu;
pub mod image_quad;
pub mod pane;
pub mod pipeline;
pub mod quad;
pub mod render_thread;
pub mod surface;
pub mod text;

pub use render_thread::{
    RenderCommand, RenderData, RenderStats, RenderThread, RenderThreadHandle, ThreadSafeRenderer,
};

/// Largura de uma célula de terminal em pixels.
pub const CELL_WIDTH: u32 = 10;
/// Altura de uma célula de terminal em pixels.
pub const CELL_HEIGHT: u32 = 20;
/// Altura da barra de tabs em pixels.
pub const TAB_BAR_HEIGHT: u32 = 28;
/// Largura de cada tab na barra em pixels.
pub const TAB_WIDTH: u32 = 140;
/// Margem interna da tab na barra.
pub const TAB_PADDING: u32 = 8;
/// Largura dos botões da barra de tabs.
pub const TAB_BAR_BUTTON_WIDTH: u32 = 32;

/// Espaço entre a borda da janela e o grid, em pixels.
pub const GRID_PADDING: f32 = 6.0;

/// Geometria do grid na janela: onde ele começa e o tamanho real de cada célula,
/// medido na fonte. Usada para desenhar, calcular colunas/linhas e mapear o mouse.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLayout {
    pub origin_x: f32,
    pub origin_y: f32,
    pub cell_width: f32,
    pub cell_height: f32,
}

impl GridLayout {
    /// Layout abaixo da barra de tabs, com o padding padrão.
    pub fn below_tab_bar(cell_width: f32, cell_height: f32) -> Self {
        Self {
            origin_x: GRID_PADDING,
            origin_y: TAB_BAR_HEIGHT as f32 + GRID_PADDING / 2.0,
            cell_width: cell_width.max(1.0),
            cell_height: cell_height.max(1.0),
        }
    }

    /// Desloca o grid para a direita (espaço da sidebar).
    pub fn with_left_inset(mut self, inset: f32) -> Self {
        self.origin_x += inset.max(0.0);
        self
    }

    /// Quantas colunas e linhas cabem numa janela de `width` x `height` pixels.
    pub fn grid_size(&self, width: u32, height: u32) -> (u16, u16) {
        let usable_w = (width as f32 - self.origin_x - GRID_PADDING).max(0.0);
        let usable_h = (height as f32 - self.origin_y - GRID_PADDING / 2.0).max(0.0);
        let cols = (usable_w / self.cell_width)
            .floor()
            .clamp(1.0, u16::MAX as f32) as u16;
        let rows = (usable_h / self.cell_height)
            .floor()
            .clamp(1.0, u16::MAX as f32) as u16;
        (cols, rows)
    }

    /// Célula (linha, coluna) sob o ponto `(x, y)` em pixels, limitada ao grid.
    pub fn cell_at(&self, x: f64, y: f64, cols: usize, rows: usize) -> (usize, usize) {
        let col = ((x as f32 - self.origin_x) / self.cell_width)
            .floor()
            .max(0.0) as usize;
        let row = ((y as f32 - self.origin_y) / self.cell_height)
            .floor()
            .max(0.0) as usize;
        (
            row.min(rows.saturating_sub(1)),
            col.min(cols.saturating_sub(1)),
        )
    }

    /// Canto superior esquerdo da célula, em pixels.
    pub fn cell_origin(&self, row: usize, col: usize) -> (f32, f32) {
        (
            self.origin_x + col as f32 * self.cell_width,
            self.origin_y + row as f32 * self.cell_height,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_size_uses_measured_cells_and_tab_bar() {
        let layout = GridLayout::below_tab_bar(8.0, 16.0);
        // 800 - 6 - 6 = 788 / 8 = 98; 600 - 31 - 3 = 566 / 16 = 35
        assert_eq!(layout.grid_size(800, 600), (98, 35));
    }

    #[test]
    fn test_grid_size_never_zero() {
        let layout = GridLayout::below_tab_bar(8.0, 16.0);
        assert_eq!(layout.grid_size(0, 0), (1, 1));
    }

    #[test]
    fn test_cell_at_maps_and_clamps() {
        let layout = GridLayout::below_tab_bar(8.0, 16.0);
        let (x, y) = layout.cell_origin(2, 5);
        assert_eq!(
            layout.cell_at(x as f64 + 1.0, y as f64 + 1.0, 80, 24),
            (2, 5)
        );
        assert_eq!(layout.cell_at(0.0, 0.0, 80, 24), (0, 0));
        assert_eq!(layout.cell_at(1.0e6, 1.0e6, 80, 24), (23, 79));
    }
}
