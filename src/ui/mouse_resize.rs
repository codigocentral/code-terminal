//! Mouse Resize Handler
//!
//! Gerencia redimensionamento de splits via mouse:
//! - Detectar hover sobre borda de split (cursor resize)
//! - Drag para redimensionar
//! - Atualizar ratio no SplitNode
//! - Recalcular layout

use crate::types::PaneId;
use crate::ui::layout::{LayoutManager, Rect, ResizeDirection, SplitDirection};

/// Largura da zona de detecção de resize (pixels)
const RESIZE_ZONE_WIDTH: u32 = 8;

/// Tipo de resize detectado
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResizeType {
    /// Redimensionamento horizontal (divide altura)
    Horizontal,
    /// Redimensionamento vertical (divide largura)
    Vertical,
    /// Não há resize nesta posição
    None,
}

/// Informação de resize disponível
#[derive(Debug, Clone, Copy)]
pub struct ResizeInfo {
    /// Tipo de resize
    pub resize_type: ResizeType,
    /// ID do pane "primário" do split
    pub primary_pane: Option<PaneId>,
    /// ID do pane "secundário" do split
    pub secondary_pane: Option<PaneId>,
}

impl Default for ResizeInfo {
    fn default() -> Self {
        Self {
            resize_type: ResizeType::None,
            primary_pane: None,
            secondary_pane: None,
        }
    }
}

/// Handler de redimensionamento por mouse
#[derive(Debug)]
pub struct MouseResizeHandler {
    /// Se está atualmente redimensionando
    is_resizing: bool,
    /// Resize type atual
    current_resize: ResizeType,
    /// Pane que está sendo redimensionado (primário)
    resize_pane: Option<PaneId>,
    /// Posição inicial do mouse no início do drag
    drag_start_pos: (u32, u32),
    /// Ratio inicial do split
    initial_ratio: f32,
    /// Direção do split sendo redimensionado
    split_direction: Option<SplitDirection>,
}

impl Default for MouseResizeHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseResizeHandler {
    /// Cria um novo handler
    pub fn new() -> Self {
        Self {
            is_resizing: false,
            current_resize: ResizeType::None,
            resize_pane: None,
            drag_start_pos: (0, 0),
            initial_ratio: 0.5,
            split_direction: None,
        }
    }

    /// Verifica se o mouse está sobre uma borda de resize
    ///
    /// Retorna o tipo de resize e os panes envolvidos
    pub fn check_resize_zone(&self, x: u32, y: u32, layout: &LayoutManager) -> ResizeInfo {
        let panes = layout.panes();

        // Para cada par de panes adjacentes, verifica se o mouse está na borda
        for (id1, pane1) in panes.iter() {
            for (id2, pane2) in panes.iter() {
                if id1 >= id2 {
                    continue;
                }

                // Verifica se são adjacentes horizontalmente (split horizontal - divide altura)
                let h_adjacent = self.are_horizontally_adjacent(&pane1.rect, &pane2.rect);
                // Verifica se são adjacentes verticalmente (split vertical - divide largura)
                let v_adjacent = self.are_vertically_adjacent(&pane1.rect, &pane2.rect);

                if h_adjacent {
                    // Borda horizontal entre pane1 (acima) e pane2 (abaixo)
                    let border_y = pane1.rect.y + pane1.rect.height;
                    let min_x = pane1.rect.x.max(pane2.rect.x);
                    let max_x =
                        (pane1.rect.x + pane1.rect.width).min(pane2.rect.x + pane2.rect.width);

                    if x >= min_x
                        && x < max_x
                        && y >= border_y.saturating_sub(RESIZE_ZONE_WIDTH)
                        && y < border_y + RESIZE_ZONE_WIDTH
                    {
                        // Determina qual pane é primário (acima)
                        let (primary, secondary) = if pane1.rect.y < pane2.rect.y {
                            (*id1, *id2)
                        } else {
                            (*id2, *id1)
                        };

                        return ResizeInfo {
                            resize_type: ResizeType::Horizontal,
                            primary_pane: Some(primary),
                            secondary_pane: Some(secondary),
                        };
                    }
                }

                if v_adjacent {
                    // Borda vertical entre pane1 (esquerda) e pane2 (direita)
                    let border_x = pane1.rect.x + pane1.rect.width;
                    let min_y = pane1.rect.y.max(pane2.rect.y);
                    let max_y =
                        (pane1.rect.y + pane1.rect.height).min(pane2.rect.y + pane2.rect.height);

                    if y >= min_y
                        && y < max_y
                        && x >= border_x.saturating_sub(RESIZE_ZONE_WIDTH)
                        && x < border_x + RESIZE_ZONE_WIDTH
                    {
                        // Determina qual pane é primário (esquerda)
                        let (primary, secondary) = if pane1.rect.x < pane2.rect.x {
                            (*id1, *id2)
                        } else {
                            (*id2, *id1)
                        };

                        return ResizeInfo {
                            resize_type: ResizeType::Vertical,
                            primary_pane: Some(primary),
                            secondary_pane: Some(secondary),
                        };
                    }
                }
            }
        }

        ResizeInfo::default()
    }

    /// Verifica se dois retângulos são adjacentes horizontalmente
    /// (um acima do outro, com sobreposição horizontal)
    fn are_horizontally_adjacent(&self, r1: &Rect, r2: &Rect) -> bool {
        let touches = (r1.y + r1.height == r2.y) || (r2.y + r2.height == r1.y);
        touches && Self::ranges_overlap(r1.x, r1.width, r2.x, r2.width)
    }

    /// Verifica se dois retângulos são adjacentes verticalmente
    /// (um ao lado do outro, com sobreposição vertical)
    fn are_vertically_adjacent(&self, r1: &Rect, r2: &Rect) -> bool {
        let touches = (r1.x + r1.width == r2.x) || (r2.x + r2.width == r1.x);
        touches && Self::ranges_overlap(r1.y, r1.height, r2.y, r2.height)
    }

    fn ranges_overlap(start1: u32, len1: u32, start2: u32, len2: u32) -> bool {
        let end1 = start1.saturating_add(len1);
        let end2 = start2.saturating_add(len2);
        start1 < end2 && start2 < end1
    }

    /// Inicia o redimensionamento
    ///
    /// Retorna true se o resize foi iniciado
    pub fn start_resize(
        &mut self,
        x: u32,
        y: u32,
        resize_info: ResizeInfo,
        layout: &LayoutManager,
    ) -> bool {
        if resize_info.resize_type == ResizeType::None {
            return false;
        }
        if resize_info.primary_pane.is_none() || resize_info.secondary_pane.is_none() {
            return false;
        }

        self.is_resizing = true;
        self.current_resize = resize_info.resize_type;
        self.resize_pane = resize_info.primary_pane;
        self.drag_start_pos = (x, y);

        // Determina a direção do split
        self.split_direction = match resize_info.resize_type {
            ResizeType::Horizontal => Some(SplitDirection::Horizontal),
            ResizeType::Vertical => Some(SplitDirection::Vertical),
            ResizeType::None => None,
        };

        // Calcula o ratio inicial baseado nas posições
        if let (Some(primary_id), Some(secondary_id), Some(direction)) = (
            resize_info.primary_pane,
            resize_info.secondary_pane,
            self.split_direction,
        ) {
            if let (Some(pane), Some(secondary_pane)) = (
                layout.panes().get(&primary_id),
                layout.panes().get(&secondary_id),
            ) {
                let (total, current) = match direction {
                    SplitDirection::Horizontal => {
                        // Split horizontal: divide altura
                        let total = pane.rect.height + secondary_pane.rect.height;
                        (total, pane.rect.height)
                    }
                    SplitDirection::Vertical => {
                        // Split vertical: divide largura
                        let total = pane.rect.width + secondary_pane.rect.width;
                        (total, pane.rect.width)
                    }
                };

                if total > 0 {
                    self.initial_ratio = current as f32 / total as f32;
                }
            }
        }

        true
    }

    /// Atualiza o redimensionamento durante o drag
    ///
    /// Retorna o delta aplicado ao ratio (se houver)
    pub fn update_resize(&mut self, x: u32, y: u32, layout: &mut LayoutManager) -> Option<f32> {
        if !self.is_resizing {
            return None;
        }

        let (start_x, start_y) = self.drag_start_pos;

        // Calcula o delta baseado na direção
        let delta_pixels = match self.current_resize {
            ResizeType::Horizontal => y as i32 - start_y as i32,
            ResizeType::Vertical => x as i32 - start_x as i32,
            ResizeType::None => return None,
        };

        // Calcula o delta no ratio
        let delta_ratio = self.calculate_ratio_delta(delta_pixels, layout)?;

        // Aplica o resize usando o LayoutManager
        let _new_ratio = (self.initial_ratio + delta_ratio).clamp(0.1, 0.9);

        // Atualiza o ratio no layout
        if let Some(pane_id) = self.resize_pane {
            let direction = match self.current_resize {
                ResizeType::Horizontal => ResizeDirection::Increase,
                ResizeType::Vertical => ResizeDirection::Increase,
                ResizeType::None => return None,
            };

            // Ajusta a direção baseado no delta
            let direction = if delta_pixels > 0 {
                direction
            } else {
                match direction {
                    ResizeDirection::Increase => ResizeDirection::Decrease,
                    ResizeDirection::Decrease => ResizeDirection::Increase,
                }
            };

            // Ativa o pane e aplica resize
            layout.set_active_pane(pane_id);
            layout.resize_active_pane(direction);
        }

        Some(delta_ratio)
    }

    /// Calcula o delta no ratio baseado no movimento em pixels
    fn calculate_ratio_delta(&self, delta_pixels: i32, layout: &LayoutManager) -> Option<f32> {
        let pane_id = self.resize_pane?;
        let pane = layout.panes().get(&pane_id)?;

        let total = match self.split_direction? {
            SplitDirection::Horizontal => {
                // Total da altura disponível
                pane.rect.height
                    + layout
                        .panes()
                        .iter()
                        .filter(|(id, _)| **id != pane_id)
                        .map(|(_, p)| p.rect.height)
                        .sum::<u32>()
            }
            SplitDirection::Vertical => {
                // Total da largura disponível
                pane.rect.width
                    + layout
                        .panes()
                        .iter()
                        .filter(|(id, _)| **id != pane_id)
                        .map(|(_, p)| p.rect.width)
                        .sum::<u32>()
            }
        };

        if total == 0 {
            return None;
        }

        Some(delta_pixels as f32 / total as f32)
    }

    /// Finaliza o redimensionamento
    pub fn end_resize(&mut self) {
        self.is_resizing = false;
        self.current_resize = ResizeType::None;
        self.resize_pane = None;
        self.split_direction = None;
    }

    /// Retorna se está atualmente redimensionando
    pub fn is_resizing(&self) -> bool {
        self.is_resizing
    }

    /// Retorna o tipo de resize atual
    pub fn current_resize_type(&self) -> ResizeType {
        self.current_resize
    }

    /// Retorna o cursor apropriado para o tipo de resize
    pub fn get_cursor(&self, resize_info: ResizeInfo) -> &'static str {
        match resize_info.resize_type {
            ResizeType::Horizontal => "ns-resize", // North-South
            ResizeType::Vertical => "ew-resize",   // East-West
            ResizeType::None => "default",
        }
    }
}

/// Helper para integração com winit
///
/// Retorna o cursor do winit para o tipo de resize
pub fn to_winit_cursor(resize_type: ResizeType) -> winit::window::CursorIcon {
    match resize_type {
        ResizeType::Horizontal => winit::window::CursorIcon::NsResize,
        ResizeType::Vertical => winit::window::CursorIcon::EwResize,
        ResizeType::None => winit::window::CursorIcon::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tabs::TabManager;

    #[test]
    fn test_resize_handler_creation() {
        let handler = MouseResizeHandler::new();
        assert!(!handler.is_resizing());
        assert_eq!(handler.current_resize_type(), ResizeType::None);
    }

    #[test]
    fn test_check_resize_zone_vertical() {
        let mut layout = LayoutManager::new(800, 600);
        let id1 = layout.create_root_pane(TabManager::new());
        let _id2 = layout
            .split_pane(id1, SplitDirection::Vertical, TabManager::new())
            .unwrap();

        let handler = MouseResizeHandler::new();

        // Ponto sobre a borda vertical (aproximadamente no meio)
        let resize_info = handler.check_resize_zone(400, 300, &layout);

        assert_eq!(resize_info.resize_type, ResizeType::Vertical);
        assert!(resize_info.primary_pane.is_some());
        assert!(resize_info.secondary_pane.is_some());
    }

    #[test]
    fn test_check_resize_zone_horizontal() {
        let mut layout = LayoutManager::new(800, 600);
        let id1 = layout.create_root_pane(TabManager::new());
        let _id2 = layout
            .split_pane(id1, SplitDirection::Horizontal, TabManager::new())
            .unwrap();

        let handler = MouseResizeHandler::new();

        // Ponto sobre a borda horizontal (aproximadamente no meio)
        let resize_info = handler.check_resize_zone(400, 300, &layout);

        assert_eq!(resize_info.resize_type, ResizeType::Horizontal);
        assert!(resize_info.primary_pane.is_some());
        assert!(resize_info.secondary_pane.is_some());
    }

    #[test]
    fn test_check_resize_zone_outside() {
        let mut layout = LayoutManager::new(800, 600);
        let id1 = layout.create_root_pane(TabManager::new());
        let _id2 = layout
            .split_pane(id1, SplitDirection::Vertical, TabManager::new())
            .unwrap();

        let handler = MouseResizeHandler::new();

        // Ponto longe da borda
        let resize_info = handler.check_resize_zone(100, 100, &layout);

        assert_eq!(resize_info.resize_type, ResizeType::None);
    }

    #[test]
    fn test_adjacent_detection() {
        let handler = MouseResizeHandler::new();

        // Panes adjacentes verticalmente
        let r1 = Rect::new(0, 0, 400, 600);
        let r2 = Rect::new(400, 0, 400, 600);
        assert!(handler.are_vertically_adjacent(&r1, &r2));
        assert!(!handler.are_horizontally_adjacent(&r1, &r2));

        // Panes adjacentes horizontalmente
        let r3 = Rect::new(0, 0, 800, 300);
        let r4 = Rect::new(0, 300, 800, 300);
        assert!(handler.are_horizontally_adjacent(&r3, &r4));
        assert!(!handler.are_vertically_adjacent(&r3, &r4));

        // Panes assimétricos com borda compartilhada ainda são adjacentes
        let r5 = Rect::new(0, 0, 400, 500);
        let r6 = Rect::new(400, 100, 300, 200);
        assert!(handler.are_vertically_adjacent(&r5, &r6));

        let r7 = Rect::new(100, 0, 300, 300);
        let r8 = Rect::new(0, 300, 800, 300);
        assert!(handler.are_horizontally_adjacent(&r7, &r8));

        // Panes não adjacentes
        let r9 = Rect::new(0, 0, 100, 100);
        let r10 = Rect::new(200, 200, 100, 100);
        assert!(!handler.are_vertically_adjacent(&r9, &r10));
        assert!(!handler.are_horizontally_adjacent(&r9, &r10));
    }

    #[test]
    fn test_start_and_end_resize() {
        let mut layout = LayoutManager::new(800, 600);
        let id1 = layout.create_root_pane(TabManager::new());
        let _id2 = layout
            .split_pane(id1, SplitDirection::Vertical, TabManager::new())
            .unwrap();

        let mut handler = MouseResizeHandler::new();
        let resize_info = handler.check_resize_zone(400, 300, &layout);

        // Inicia resize
        let started = handler.start_resize(400, 300, resize_info, &layout);
        assert!(started);
        assert!(handler.is_resizing());

        // Finaliza resize
        handler.end_resize();
        assert!(!handler.is_resizing());
    }

    #[test]
    fn test_get_cursor() {
        let handler = MouseResizeHandler::new();

        assert_eq!(
            handler.get_cursor(ResizeInfo {
                resize_type: ResizeType::Horizontal,
                primary_pane: None,
                secondary_pane: None
            }),
            "ns-resize"
        );
        assert_eq!(
            handler.get_cursor(ResizeInfo {
                resize_type: ResizeType::Vertical,
                primary_pane: None,
                secondary_pane: None
            }),
            "ew-resize"
        );
        assert_eq!(
            handler.get_cursor(ResizeInfo {
                resize_type: ResizeType::None,
                primary_pane: None,
                secondary_pane: None
            }),
            "default"
        );
    }
}
