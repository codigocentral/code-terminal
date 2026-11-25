use crate::ui::tabs::TabManager;
use std::collections::HashMap;

// Re-exportação de tipos comuns
pub use crate::types::PaneId;

/// Retângulo de posicionamento (x, y, width, height)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Retorna as coordenadas centro do retângulo
    pub fn center(&self) -> (u32, u32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }

    /// Verifica se um ponto está dentro do retângulo
    pub fn contains(&self, px: u32, py: u32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }
}

/// Direção do split
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    /// Panes empilhados verticalmente (split horizontal)
    Horizontal,
    /// Panes lado a lado (split vertical)
    Vertical,
}

/// Direção de redimensionamento
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResizeDirection {
    /// Aumenta o tamanho do pane
    Increase,
    /// Diminui o tamanho do pane
    Decrease,
}

/// Nó da árvore de layout
#[derive(Debug, Clone)]
pub enum SplitNode {
    /// Split com dois filhos
    Split {
        direction: SplitDirection,
        /// Proporção do primeiro filho (0.0 - 1.0)
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
    /// Pane folha
    Pane(PaneId),
}

/// Um pane contém um TabManager e sua posição
#[derive(Debug)]
pub struct Pane {
    pub id: PaneId,
    pub tab_manager: TabManager,
    pub rect: Rect,
}

impl Pane {
    pub fn new(id: PaneId, tab_manager: TabManager) -> Self {
        Self {
            id,
            tab_manager,
            rect: Rect::new(0, 0, 0, 0),
        }
    }
}

/// Gerenciador de layout de panes
#[derive(Debug)]
pub struct LayoutManager {
    /// Raiz da árvore de splits
    root: Option<SplitNode>,
    /// Mapa de panes por ID
    panes: HashMap<PaneId, Pane>,
    /// ID do próximo pane
    next_pane_id: PaneId,
    /// Pane ativo
    active_pane: Option<PaneId>,
    /// Retângulo da janela
    window_rect: Rect,
}

impl LayoutManager {
    /// Cria um novo layout manager
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            root: None,
            panes: HashMap::new(),
            next_pane_id: 1,
            active_pane: None,
            window_rect: Rect::new(0, 0, width, height),
        }
    }

    /// Cria o pane raiz inicial
    pub fn create_root_pane(&mut self, tab_manager: TabManager) -> PaneId {
        let id = self.next_pane_id;
        self.next_pane_id += 1;

        let pane = Pane::new(id, tab_manager);
        self.panes.insert(id, pane);
        self.root = Some(SplitNode::Pane(id));
        self.active_pane = Some(id);

        self.recalculate_layout();
        id
    }

    /// Divide um pane na direção especificada
    pub fn split_pane(
        &mut self,
        pane_id: PaneId,
        direction: SplitDirection,
        new_tab_manager: TabManager,
    ) -> Option<PaneId> {
        // Cria novo pane
        let new_id = self.next_pane_id;
        self.next_pane_id += 1;
        let new_pane = Pane::new(new_id, new_tab_manager);
        self.panes.insert(new_id, new_pane);

        // Atualiza a árvore
        if let Some(root) = self.root.take() {
            self.root = Some(self.split_node(root, pane_id, direction, new_id));
        }

        self.recalculate_layout();
        self.active_pane = Some(new_id);
        Some(new_id)
    }

    /// Função recursiva para split
    fn split_node(
        &self,
        node: SplitNode,
        target_id: PaneId,
        direction: SplitDirection,
        new_id: PaneId,
    ) -> SplitNode {
        match node {
            SplitNode::Pane(id) if id == target_id => {
                // Encontrou o pane alvo, cria split
                SplitNode::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(SplitNode::Pane(id)),
                    second: Box::new(SplitNode::Pane(new_id)),
                }
            }
            SplitNode::Split {
                direction: d,
                ratio,
                first,
                second,
            } => {
                // Tenta dividir nos filhos
                let new_first = self.split_node(*first, target_id, direction, new_id);
                if let SplitNode::Split { .. } = &new_first {
                    // Split foi feito no primeiro filho
                    SplitNode::Split {
                        direction: d,
                        ratio,
                        first: Box::new(new_first),
                        second,
                    }
                } else {
                    // Tenta no segundo filho
                    let new_second = self.split_node(*second, target_id, direction, new_id);
                    SplitNode::Split {
                        direction: d,
                        ratio,
                        first: Box::new(new_first),
                        second: Box::new(new_second),
                    }
                }
            }
            other => other,
        }
    }

    /// Remove um pane
    /// Fecha o pane ativo
    /// Retorna true se conseguiu fechar, false se não havia pane ativo ou era o último
    pub fn close_active_pane(&mut self) -> bool {
        if let Some(active_id) = self.active_pane {
            self.close_pane(active_id)
        } else {
            false
        }
    }

    /// Fecha um pane específico
    /// Retorna true se conseguiu fechar, false se não existe ou é o último
    pub fn close_pane(&mut self, pane_id: PaneId) -> bool {
        // Não permite fechar o último pane
        if self.panes.len() <= 1 {
            return false;
        }

        // Remove o pane
        self.panes.remove(&pane_id);

        // Atualiza a árvore
        if let Some(root) = self.root.take() {
            self.root = self.remove_node(root, pane_id);
        }

        // Atualiza pane ativo se necessário
        if self.active_pane == Some(pane_id) {
            self.active_pane = self.panes.keys().next().copied();
        }

        self.recalculate_layout();
        true
    }

    /// Função recursiva para remoção
    fn remove_node(&self, node: SplitNode, target_id: PaneId) -> Option<SplitNode> {
        match node {
            SplitNode::Pane(id) if id == target_id => None,
            SplitNode::Pane(id) => Some(SplitNode::Pane(id)),
            SplitNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let new_first = self.remove_node(*first, target_id);
                let new_second = self.remove_node(*second, target_id);

                match (new_first, new_second) {
                    (Some(f), Some(s)) => Some(SplitNode::Split {
                        direction,
                        ratio,
                        first: Box::new(f),
                        second: Box::new(s),
                    }),
                    (Some(f), None) => Some(f),
                    (None, Some(s)) => Some(s),
                    (None, None) => None,
                }
            }
        }
    }

    /// Recalcula as posições de todos os panes
    pub fn recalculate_layout(&mut self) {
        let positions = self.calculate_all_positions();
        for (id, rect) in positions {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.rect = rect;
            }
        }
    }

    /// Calcula todas as posições retornando mapa de id -> rect
    fn calculate_all_positions(&self) -> HashMap<PaneId, Rect> {
        let mut positions = HashMap::new();
        if let Some(ref root) = self.root {
            self.collect_positions(root, self.window_rect, &mut positions);
        }
        positions
    }

    /// Coleta posições recursivamente
    fn collect_positions(
        &self,
        node: &SplitNode,
        rect: Rect,
        positions: &mut HashMap<PaneId, Rect>,
    ) {
        match node {
            SplitNode::Pane(id) => {
                positions.insert(*id, rect);
            }
            SplitNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (first_rect, second_rect) = match direction {
                    SplitDirection::Horizontal => {
                        // Divide na altura
                        let first_height = (rect.height as f32 * ratio) as u32;
                        let first = Rect::new(rect.x, rect.y, rect.width, first_height);
                        let second = Rect::new(
                            rect.x,
                            rect.y + first_height,
                            rect.width,
                            rect.height - first_height,
                        );
                        (first, second)
                    }
                    SplitDirection::Vertical => {
                        // Divide na largura
                        let first_width = (rect.width as f32 * ratio) as u32;
                        let first = Rect::new(rect.x, rect.y, first_width, rect.height);
                        let second = Rect::new(
                            rect.x + first_width,
                            rect.y,
                            rect.width - first_width,
                            rect.height,
                        );
                        (first, second)
                    }
                };

                self.collect_positions(first, first_rect, positions);
                self.collect_positions(second, second_rect, positions);
            }
        }
    }

    /// Define o tamanho da janela
    pub fn set_window_size(&mut self, width: u32, height: u32) {
        self.window_rect = Rect::new(0, 0, width, height);
        self.recalculate_layout();
    }

    /// Retorna o pane ativo
    pub fn active_pane(&self) -> Option<&Pane> {
        self.active_pane.and_then(|id| self.panes.get(&id))
    }

    /// Retorna o pane ativo (mutável)
    pub fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.active_pane.and_then(|id| self.panes.get_mut(&id))
    }

    /// Define o pane ativo
    pub fn set_active_pane(&mut self, pane_id: PaneId) {
        if self.panes.contains_key(&pane_id) {
            self.active_pane = Some(pane_id);
        }
    }

    /// Retorna todos os panes
    pub fn panes(&self) -> &HashMap<PaneId, Pane> {
        &self.panes
    }

    /// Retorna todos os panes (mutável)
    pub fn panes_mut(&mut self) -> &mut HashMap<PaneId, Pane> {
        &mut self.panes
    }

    /// Retorna o número de panes
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Navega para o próximo pane (ordem de criação)
    pub fn next_pane(&mut self) {
        if let Some(current) = self.active_pane {
            let ids: Vec<PaneId> = self.panes.keys().copied().collect();
            if let Some(pos) = ids.iter().position(|&id| id == current) {
                let next_pos = (pos + 1) % ids.len();
                self.active_pane = Some(ids[next_pos]);
            }
        }
    }

    /// Navega para o pane anterior
    pub fn previous_pane(&mut self) {
        if let Some(current) = self.active_pane {
            let ids: Vec<PaneId> = self.panes.keys().copied().collect();
            if let Some(pos) = ids.iter().position(|&id| id == current) {
                let prev_pos = if pos == 0 { ids.len() - 1 } else { pos - 1 };
                self.active_pane = Some(ids[prev_pos]);
            }
        }
    }

    /// Encontra o pane em uma direção a partir do pane atual
    pub fn find_pane_in_direction(&self, direction: SplitDirection) -> Option<PaneId> {
        let current = self.active_pane?;
        let current_pane = self.panes.get(&current)?;
        let current_center = current_pane.rect.center();

        let mut best_candidate: Option<(PaneId, f32)> = None;

        for (id, pane) in &self.panes {
            if *id == current {
                continue;
            }

            let center = pane.rect.center();
            let is_in_direction = match direction {
                SplitDirection::Horizontal => {
                    // Acima ou abaixo
                    let dy = center.1 as f32 - current_center.1 as f32;
                    let dx = (center.0 as f32 - current_center.0 as f32).abs();
                    dy != 0.0 && dx < dy.abs() // Mais vertical que horizontal
                }
                SplitDirection::Vertical => {
                    // Esquerda ou direita
                    let dx = center.0 as f32 - current_center.0 as f32;
                    let dy = (center.1 as f32 - current_center.1 as f32).abs();
                    dx != 0.0 && dy < dx.abs() // Mais horizontal que vertical
                }
            };

            if is_in_direction {
                let distance = ((center.0 as f32 - current_center.0 as f32).powi(2)
                    + (center.1 as f32 - current_center.1 as f32).powi(2))
                .sqrt();

                match best_candidate {
                    None => best_candidate = Some((*id, distance)),
                    Some((_, d)) if distance < d => best_candidate = Some((*id, distance)),
                    _ => {}
                }
            }
        }

        best_candidate.map(|(id, _)| id)
    }

    /// Move o foco na direção especificada
    pub fn move_focus(&mut self, direction: SplitDirection) {
        if let Some(new_pane) = self.find_pane_in_direction(direction) {
            self.active_pane = Some(new_pane);
        }
    }

    /// Redimensiona o split do pane ativo
    /// direction: direção do redimensionamento
    /// delta: valor do ajuste (positivo = aumenta, negativo = diminui)
    pub fn resize_active_pane(&mut self, direction: ResizeDirection) {
        const RESIZE_STEP: f32 = 0.05; // 5% por passo
        const MIN_RATIO: f32 = 0.10; // Mínimo 10%
        const MAX_RATIO: f32 = 0.90; // Máximo 90%

        let Some(active_id) = self.active_pane else {
            return;
        };

        // Calcula novo ratio baseado na direção
        let delta = match direction {
            ResizeDirection::Increase => RESIZE_STEP,
            ResizeDirection::Decrease => -RESIZE_STEP,
        };

        // Tenta ajustar o ratio no split que contém o pane ativo
        if let Some(root) = self.root.as_ref() {
            if let Some(new_root) =
                self.adjust_split_ratio(root, active_id, delta, MIN_RATIO, MAX_RATIO)
            {
                self.root = Some(new_root);
                self.recalculate_layout();
            }
        }
    }

    /// Ajusta o ratio de um split recursivamente
    fn adjust_split_ratio(
        &self,
        node: &SplitNode,
        target_pane: PaneId,
        delta: f32,
        min: f32,
        max: f32,
    ) -> Option<SplitNode> {
        match node {
            SplitNode::Pane(_) => None, // Não é um split
            SplitNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                // Verifica se o pane alvo está neste split
                let target_in_first = self.node_contains_pane(first, target_pane);
                let target_in_second = self.node_contains_pane(second, target_pane);

                if target_in_first || target_in_second {
                    // Calcula novo ratio
                    let new_ratio = if target_in_first {
                        (ratio + delta).clamp(min, max)
                    } else {
                        (ratio - delta).clamp(min, max)
                    };

                    // Se o ratio mudou, retorna novo split
                    if (new_ratio - ratio).abs() > f32::EPSILON {
                        Some(SplitNode::Split {
                            direction: *direction,
                            ratio: new_ratio,
                            first: first.clone(),
                            second: second.clone(),
                        })
                    } else {
                        None // Ratio já está no limite
                    }
                } else {
                    // Pane não está neste split, procura nos filhos
                    let new_first = self.adjust_split_ratio(first, target_pane, delta, min, max);
                    if let Some(new_first) = new_first {
                        return Some(SplitNode::Split {
                            direction: *direction,
                            ratio: *ratio,
                            first: Box::new(new_first),
                            second: second.clone(),
                        });
                    }

                    let new_second = self.adjust_split_ratio(second, target_pane, delta, min, max);
                    if let Some(new_second) = new_second {
                        return Some(SplitNode::Split {
                            direction: *direction,
                            ratio: *ratio,
                            first: first.clone(),
                            second: Box::new(new_second),
                        });
                    }

                    None
                }
            }
        }
    }

    /// Verifica se um nó contém um pane específico
    fn node_contains_pane(&self, node: &SplitNode, pane_id: PaneId) -> bool {
        match node {
            SplitNode::Pane(id) => *id == pane_id,
            SplitNode::Split { first, second, .. } => {
                self.node_contains_pane(first, pane_id) || self.node_contains_pane(second, pane_id)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tab_manager() -> TabManager {
        TabManager::new()
    }

    #[test]
    fn test_layout_manager_creation() {
        let manager = LayoutManager::new(800, 600);
        assert_eq!(manager.pane_count(), 0);
        assert!(manager.active_pane().is_none());
    }

    #[test]
    fn test_create_root_pane() {
        let mut manager = LayoutManager::new(800, 600);
        let tab_manager = create_test_tab_manager();
        let id = manager.create_root_pane(tab_manager);

        assert_eq!(manager.pane_count(), 1);
        assert_eq!(manager.active_pane().unwrap().id, id);
        assert_eq!(manager.active_pane().unwrap().rect.width, 800);
        assert_eq!(manager.active_pane().unwrap().rect.height, 600);
    }

    #[test]
    fn test_split_horizontal() {
        let mut manager = LayoutManager::new(800, 600);
        let tab_manager = create_test_tab_manager();
        let id1 = manager.create_root_pane(tab_manager);

        let tab_manager2 = create_test_tab_manager();
        let id2 = manager
            .split_pane(id1, SplitDirection::Horizontal, tab_manager2)
            .unwrap();

        assert_eq!(manager.pane_count(), 2);

        let pane1 = manager.panes.get(&id1).unwrap();
        let pane2 = manager.panes.get(&id2).unwrap();

        // Devem ter a mesma largura
        assert_eq!(pane1.rect.width, 800);
        assert_eq!(pane2.rect.width, 800);

        // Alturas devem somar 600
        assert_eq!(pane1.rect.height + pane2.rect.height, 600);

        // Pane 2 deve estar abaixo do pane 1
        assert_eq!(pane2.rect.y, pane1.rect.y + pane1.rect.height);
    }

    #[test]
    fn test_split_vertical() {
        let mut manager = LayoutManager::new(800, 600);
        let tab_manager = create_test_tab_manager();
        let id1 = manager.create_root_pane(tab_manager);

        let tab_manager2 = create_test_tab_manager();
        let id2 = manager
            .split_pane(id1, SplitDirection::Vertical, tab_manager2)
            .unwrap();

        assert_eq!(manager.pane_count(), 2);

        let pane1 = manager.panes.get(&id1).unwrap();
        let pane2 = manager.panes.get(&id2).unwrap();

        // Devem ter a mesma altura
        assert_eq!(pane1.rect.height, 600);
        assert_eq!(pane2.rect.height, 600);

        // Larguras devem somar 800
        assert_eq!(pane1.rect.width + pane2.rect.width, 800);

        // Pane 2 deve estar à direita do pane 1
        assert_eq!(pane2.rect.x, pane1.rect.x + pane1.rect.width);
    }

    #[test]
    fn test_close_pane() {
        let mut manager = LayoutManager::new(800, 600);
        let tab_manager = create_test_tab_manager();
        let id1 = manager.create_root_pane(tab_manager);

        let tab_manager2 = create_test_tab_manager();
        let id2 = manager
            .split_pane(id1, SplitDirection::Vertical, tab_manager2)
            .unwrap();

        assert_eq!(manager.pane_count(), 2);

        // Fecha o primeiro pane
        assert!(manager.close_pane(id1));
        assert_eq!(manager.pane_count(), 1);

        // Não pode fechar o último pane
        assert!(!manager.close_pane(id2));
        assert_eq!(manager.pane_count(), 1);
    }

    #[test]
    fn test_next_previous_pane() {
        let mut manager = LayoutManager::new(800, 600);
        let id1 = manager.create_root_pane(create_test_tab_manager());
        let id2 = manager
            .split_pane(id1, SplitDirection::Vertical, create_test_tab_manager())
            .unwrap();

        manager.set_active_pane(id1);
        assert_eq!(manager.active_pane().unwrap().id, id1);

        manager.next_pane();
        assert_eq!(manager.active_pane().unwrap().id, id2);

        manager.next_pane();
        assert_eq!(manager.active_pane().unwrap().id, id1);

        manager.previous_pane();
        assert_eq!(manager.active_pane().unwrap().id, id2);
    }

    #[test]
    fn test_window_resize() {
        let mut manager = LayoutManager::new(800, 600);
        let _id1 = manager.create_root_pane(create_test_tab_manager());

        assert_eq!(manager.active_pane().unwrap().rect.width, 800);

        manager.set_window_size(1024, 768);
        assert_eq!(manager.active_pane().unwrap().rect.width, 1024);
        assert_eq!(manager.active_pane().unwrap().rect.height, 768);
    }

    #[test]
    fn test_rect_contains() {
        let rect = Rect::new(10, 20, 100, 50);

        assert!(rect.contains(10, 20)); // Canto superior esquerdo
        assert!(rect.contains(50, 40)); // Centro
        assert!(rect.contains(109, 69)); // Canto inferior direito (dentro)

        assert!(!rect.contains(9, 20)); // Fora à esquerda
        assert!(!rect.contains(10, 19)); // Fora acima
        assert!(!rect.contains(110, 20)); // Fora à direita
        assert!(!rect.contains(10, 70)); // Fora abaixo
    }

    #[test]
    fn test_resize_pane_horizontal() {
        let mut manager = LayoutManager::new(800, 600);
        let id1 = manager.create_root_pane(create_test_tab_manager());
        let _id2 = manager
            .split_pane(id1, SplitDirection::Horizontal, create_test_tab_manager())
            .unwrap();

        let initial_height = manager.panes.get(&id1).unwrap().rect.height;

        // Aumenta o tamanho do pane 1
        manager.set_active_pane(id1);
        manager.resize_active_pane(ResizeDirection::Increase);

        let new_height = manager.panes.get(&id1).unwrap().rect.height;
        assert!(
            new_height > initial_height,
            "Pane should have increased in size"
        );

        // Diminui o tamanho do pane 1
        manager.resize_active_pane(ResizeDirection::Decrease);
        let final_height = manager.panes.get(&id1).unwrap().rect.height;
        assert!(
            final_height < new_height,
            "Pane should have decreased in size"
        );
    }

    #[test]
    fn test_resize_pane_vertical() {
        let mut manager = LayoutManager::new(800, 600);
        let id1 = manager.create_root_pane(create_test_tab_manager());
        let _id2 = manager
            .split_pane(id1, SplitDirection::Vertical, create_test_tab_manager())
            .unwrap();

        let initial_width = manager.panes.get(&id1).unwrap().rect.width;

        // Aumenta o tamanho do pane 1
        manager.set_active_pane(id1);
        manager.resize_active_pane(ResizeDirection::Increase);

        let new_width = manager.panes.get(&id1).unwrap().rect.width;
        assert!(
            new_width > initial_width,
            "Pane should have increased in size"
        );
    }

    #[test]
    fn test_resize_respects_limits() {
        let mut manager = LayoutManager::new(800, 600);
        let id1 = manager.create_root_pane(create_test_tab_manager());
        let _id2 = manager
            .split_pane(id1, SplitDirection::Horizontal, create_test_tab_manager())
            .unwrap();

        manager.set_active_pane(id1);

        // Tenta diminuir muito (deve parar no mínimo 10%)
        for _ in 0..20 {
            manager.resize_active_pane(ResizeDirection::Decrease);
        }

        let pane1 = manager.panes.get(&id1).unwrap();
        // Altura mínima deve ser pelo menos 10% de 600 = 60
        assert!(pane1.rect.height >= 60, "Pane should respect minimum size");
    }

    #[test]
    fn test_nested_splits() {
        let mut manager = LayoutManager::new(800, 600);

        // Cria layout: P1 | V[ P2 / P3 ]
        // (P1 à esquerda, P2 e P3 empilhados à direita)
        let id1 = manager.create_root_pane(create_test_tab_manager());
        let id2 = manager
            .split_pane(id1, SplitDirection::Vertical, create_test_tab_manager())
            .unwrap();
        let id3 = manager
            .split_pane(id2, SplitDirection::Horizontal, create_test_tab_manager())
            .unwrap();

        // Verifica 3 panes
        assert_eq!(manager.panes.len(), 3);

        // Verifica dimensões
        let pane1 = manager.panes.get(&id1).unwrap();
        let pane2 = manager.panes.get(&id2).unwrap();
        let pane3 = manager.panes.get(&id3).unwrap();

        // P1 deve estar à esquerda de P2 e P3
        assert!(pane1.rect.x < pane2.rect.x, "P1 should be left of P2");
        assert!(pane1.rect.x < pane3.rect.x, "P1 should be left of P3");

        // P2 deve estar acima de P3 (horizontal split divide em cima/embaixo)
        assert!(pane2.rect.y < pane3.rect.y, "P2 should be above P3");

        // P2 e P3 devem ter mesma posição X
        assert_eq!(pane2.rect.x, pane3.rect.x, "P2 and P3 should have same X");
    }

    #[test]
    fn test_close_all_panes_except_last() {
        let mut manager = LayoutManager::new(800, 600);
        let id1 = manager.create_root_pane(create_test_tab_manager());
        let id2 = manager
            .split_pane(id1, SplitDirection::Vertical, create_test_tab_manager())
            .unwrap();

        // Fecha id1
        assert!(manager.close_pane(id1));
        assert!(!manager.panes.contains_key(&id1));
        assert!(manager.panes.contains_key(&id2));

        // Não pode fechar o último pane
        assert!(!manager.close_pane(id2));
        assert!(manager.panes.contains_key(&id2));
        assert_eq!(manager.panes.len(), 1);
    }

    #[test]
    fn test_multiple_panes_layout() {
        let mut manager = LayoutManager::new(800, 600);
        let id1 = manager.create_root_pane(create_test_tab_manager());
        let id2 = manager
            .split_pane(id1, SplitDirection::Vertical, create_test_tab_manager())
            .unwrap();

        let pane1 = manager.panes.get(&id1).unwrap();
        let pane2 = manager.panes.get(&id2).unwrap();

        // Pane1 deve estar na esquerda, Pane2 na direita
        assert!(pane1.rect.x < pane2.rect.x, "Pane1 should be left of Pane2");
        assert_eq!(
            pane1.rect.y, pane2.rect.y,
            "Both panes should start at same Y"
        );
    }

    #[test]
    fn test_split_ratios_sum_correctly() {
        let mut manager = LayoutManager::new(800, 600);
        let id1 = manager.create_root_pane(create_test_tab_manager());

        // Split com ratio padrão (0.5)
        let _id2 = manager
            .split_pane(id1, SplitDirection::Vertical, create_test_tab_manager())
            .unwrap();

        let pane1 = manager.panes.get(&id1).unwrap();
        let pane2 = manager.panes.get(&_id2).unwrap();

        // As larguras devem somar 800
        assert_eq!(pane1.rect.width + pane2.rect.width, 800);
        assert_eq!(pane1.rect.height, 600);
        assert_eq!(pane2.rect.height, 600);
    }

    #[test]
    fn test_navigation_with_no_panes() {
        let mut manager = LayoutManager::new(800, 600);

        // Sem panes, navegação não deve falhar (não deve panico)
        manager.next_pane();
        manager.previous_pane();
        assert!(manager.active_pane().is_none());
    }

    #[test]
    fn test_split_horizontal_calculation() {
        let mut manager = LayoutManager::new(100, 100);
        let id1 = manager.create_root_pane(create_test_tab_manager());

        // Split horizontal (stacks vertically)
        let id2 = manager
            .split_pane(id1, SplitDirection::Horizontal, create_test_tab_manager())
            .unwrap();

        let pane1 = manager.panes.get(&id1).unwrap();
        let pane2 = manager.panes.get(&id2).unwrap();

        // Mesma largura, alturas somam 100
        assert_eq!(pane1.rect.width, 100);
        assert_eq!(pane2.rect.width, 100);
        assert_eq!(pane1.rect.height + pane2.rect.height, 100);

        // Pane1 em cima, Pane2 embaixo
        assert_eq!(pane1.rect.y, 0);
        assert_eq!(pane2.rect.y, pane1.rect.height);
    }

    #[test]
    fn test_close_active_pane() {
        let mut manager = LayoutManager::new(800, 600);
        let id1 = manager.create_root_pane(create_test_tab_manager());
        let _id2 = manager
            .split_pane(id1, SplitDirection::Vertical, create_test_tab_manager())
            .unwrap();

        // Pane ativo deve ser o último criado (pane 2)
        assert_eq!(manager.active_pane().unwrap().id, 2);

        // Fecha pane ativo
        assert!(manager.close_active_pane());

        // Pane 2 deve ter sido removido
        assert!(!manager.panes.contains_key(&2));
        assert!(manager.panes.contains_key(&1));

        // Pane ativo deve ter mudado para 1
        assert_eq!(manager.active_pane().unwrap().id, 1);
    }

    #[test]
    fn test_close_active_pane_last_pane() {
        let mut manager = LayoutManager::new(800, 600);
        let _id1 = manager.create_root_pane(create_test_tab_manager());

        // Não pode fechar o último pane
        assert!(!manager.close_active_pane());
        assert_eq!(manager.panes.len(), 1);
    }
}
