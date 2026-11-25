//! Árvore de divisões (splits) de uma tab, medida em células.
//!
//! Cada folha é um painel; cada nó interno divide a área em dois, lado a lado
//! (`Horizontal`) ou um sobre o outro (`Vertical`), com uma coluna/linha de
//! separador entre eles. Trabalhar em células (e não em pixels) deixa o
//! redimensionamento do PTY direto e os testes exatos.

/// Identificador de um painel dentro da janela.
pub type PaneId = u64;

/// Direção da divisão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    /// Painéis lado a lado (separador vertical).
    Horizontal,
    /// Painéis um sobre o outro (separador horizontal).
    Vertical,
}

/// Direção para mover o foco.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Linhas da barra de título de cada painel quando a tab está dividida
/// (#32). Tab com um painel só não tem barra: o terminal ocupa tudo.
pub const PANE_TITLE_ROWS: u16 = 1;

/// Retângulo em células: coluna/linha do canto e tamanho.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellRect {
    pub col: u16,
    pub row: u16,
    pub cols: u16,
    pub rows: u16,
}

impl CellRect {
    pub fn new(col: u16, row: u16, cols: u16, rows: u16) -> Self {
        Self {
            col,
            row,
            cols,
            rows,
        }
    }

    /// A célula (linha, coluna) está dentro do retângulo?
    pub fn contains(&self, row: usize, col: usize) -> bool {
        row >= self.row as usize
            && row < (self.row + self.rows) as usize
            && col >= self.col as usize
            && col < (self.col + self.cols) as usize
    }

    /// Área do terminal do painel: sem a linha da barra de título (#32).
    /// Painel de uma linha só fica sem barra.
    pub fn content(&self) -> CellRect {
        if self.rows > PANE_TITLE_ROWS {
            CellRect {
                row: self.row + PANE_TITLE_ROWS,
                rows: self.rows - PANE_TITLE_ROWS,
                ..*self
            }
        } else {
            *self
        }
    }

    fn center(&self) -> (f32, f32) {
        (
            self.col as f32 + self.cols as f32 / 2.0,
            self.row as f32 + self.rows as f32 / 2.0,
        )
    }
}

/// Separador entre dois painéis (para desenhar e arrastar).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Divider {
    pub direction: SplitDirection,
    /// Para `Horizontal`, a coluna do separador; para `Vertical`, a linha.
    pub position: u16,
    /// Início e tamanho ao longo do separador.
    pub start: u16,
    pub len: u16,
}

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Leaf(PaneId),
    Split {
        direction: SplitDirection,
        /// Fração da área que fica com o primeiro filho.
        ratio: f32,
        first: Box<Node>,
        second: Box<Node>,
    },
}

impl Node {
    fn contains(&self, id: PaneId) -> bool {
        match self {
            Node::Leaf(leaf) => *leaf == id,
            Node::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    fn split(&mut self, target: PaneId, new: PaneId, direction: SplitDirection) -> bool {
        match self {
            Node::Leaf(leaf) if *leaf == target => {
                *self = Node::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(Node::Leaf(target)),
                    second: Box::new(Node::Leaf(new)),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { first, second, .. } => {
                first.split(target, new, direction) || second.split(target, new, direction)
            }
        }
    }

    /// Remove a folha; o irmão ocupa o lugar do nó pai.
    fn remove(self, id: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(leaf) if leaf == id => None,
            Node::Leaf(_) => Some(self),
            Node::Split {
                direction,
                ratio,
                first,
                second,
            } => match (first.remove(id), second.remove(id)) {
                (Some(first), Some(second)) => Some(Node::Split {
                    direction,
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }

    fn layout(
        &self,
        area: CellRect,
        out: &mut Vec<(PaneId, CellRect)>,
        dividers: &mut Vec<Divider>,
    ) {
        match self {
            Node::Leaf(id) => out.push((*id, area)),
            Node::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (a, b, divider) = split_area(area, *direction, *ratio);
                if let Some(divider) = divider {
                    dividers.push(divider);
                }
                first.layout(a, out, dividers);
                second.layout(b, out, dividers);
            }
        }
    }

    /// Ajusta a divisão mais próxima de `id` na direção pedida.
    /// Devolve `true` se algum separador mudou.
    fn resize(&mut self, id: PaneId, direction: SplitDirection, delta: f32) -> bool {
        let Node::Split {
            direction: dir,
            ratio,
            first,
            second,
        } = self
        else {
            return false;
        };
        let in_first = first.contains(id);
        let child = if in_first { first } else { second };
        if !child.contains(id) {
            return false;
        }
        if child.resize(id, direction, delta) {
            return true;
        }
        if *dir != direction {
            return false;
        }
        // Crescer o painel do primeiro filho aumenta a fração; do segundo, diminui.
        let signed = if in_first { delta } else { -delta };
        *ratio = (*ratio + signed).clamp(0.1, 0.9);
        true
    }
}

/// Divide `area` em duas, deixando uma célula para o separador.
fn split_area(
    area: CellRect,
    direction: SplitDirection,
    ratio: f32,
) -> (CellRect, CellRect, Option<Divider>) {
    let total = match direction {
        SplitDirection::Horizontal => area.cols,
        SplitDirection::Vertical => area.rows,
    };
    if total < 3 {
        // Sem espaço para dois painéis e o separador: o primeiro fica com tudo.
        let empty = CellRect::new(area.col, area.row, 0, 0);
        return (area, empty, None);
    }
    let usable = total - 1;
    let first_len = ((usable as f32 * ratio).round() as u16).clamp(1, usable - 1);
    let second_len = usable - first_len;
    match direction {
        SplitDirection::Horizontal => (
            CellRect::new(area.col, area.row, first_len, area.rows),
            CellRect::new(area.col + first_len + 1, area.row, second_len, area.rows),
            Some(Divider {
                direction,
                position: area.col + first_len,
                start: area.row,
                len: area.rows,
            }),
        ),
        SplitDirection::Vertical => (
            CellRect::new(area.col, area.row, area.cols, first_len),
            CellRect::new(area.col, area.row + first_len + 1, area.cols, second_len),
            Some(Divider {
                direction,
                position: area.row + first_len,
                start: area.col,
                len: area.cols,
            }),
        ),
    }
}

/// Árvore exportada (para salvar a sessão), com as folhas convertidas por
/// quem exporta (ex.: id do painel -> diretório).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayoutTree<T> {
    Pane {
        pane: T,
    },
    Split {
        horizontal: bool,
        ratio: f32,
        first: Box<LayoutTree<T>>,
        second: Box<LayoutTree<T>>,
    },
}

impl<T> LayoutTree<T> {
    /// Visita os painéis, da esquerda para a direita.
    pub fn for_each(&self, visit: &mut impl FnMut(&T)) {
        match self {
            LayoutTree::Pane { pane } => visit(pane),
            LayoutTree::Split { first, second, .. } => {
                first.for_each(visit);
                second.for_each(visit);
            }
        }
    }
}

impl Node {
    fn export<T>(&self, leaf: &mut impl FnMut(PaneId) -> T) -> LayoutTree<T> {
        match self {
            Node::Leaf(id) => LayoutTree::Pane { pane: leaf(*id) },
            Node::Split {
                direction,
                ratio,
                first,
                second,
            } => LayoutTree::Split {
                horizontal: *direction == SplitDirection::Horizontal,
                ratio: *ratio,
                first: Box::new(first.export(leaf)),
                second: Box::new(second.export(leaf)),
            },
        }
    }

    fn import<T>(tree: &LayoutTree<T>, leaf: &mut impl FnMut(&T) -> PaneId) -> Node {
        match tree {
            LayoutTree::Pane { pane } => Node::Leaf(leaf(pane)),
            LayoutTree::Split {
                horizontal,
                ratio,
                first,
                second,
            } => Node::Split {
                direction: if *horizontal {
                    SplitDirection::Horizontal
                } else {
                    SplitDirection::Vertical
                },
                ratio: ratio.clamp(0.1, 0.9),
                first: Box::new(Node::import(first, leaf)),
                second: Box::new(Node::import(second, leaf)),
            },
        }
    }
}

/// Árvore de painéis de uma tab.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaneLayout {
    root: Option<Node>,
}

impl PaneLayout {
    /// Layout com um único painel.
    pub fn single(id: PaneId) -> Self {
        Self {
            root: Some(Node::Leaf(id)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Exporta a árvore trocando cada painel por `leaf(id)`.
    pub fn export<T>(&self, mut leaf: impl FnMut(PaneId) -> T) -> Option<LayoutTree<T>> {
        self.root.as_ref().map(|root| root.export(&mut leaf))
    }

    /// Reconstrói a árvore; `leaf` cria cada painel e devolve o id dele.
    pub fn import<T>(tree: &LayoutTree<T>, mut leaf: impl FnMut(&T) -> PaneId) -> Self {
        Self {
            root: Some(Node::import(tree, &mut leaf)),
        }
    }

    pub fn contains(&self, id: PaneId) -> bool {
        self.root.as_ref().is_some_and(|root| root.contains(id))
    }

    /// Divide o painel `target`, colocando `new` à direita/abaixo.
    pub fn split(&mut self, target: PaneId, new: PaneId, direction: SplitDirection) -> bool {
        self.root
            .as_mut()
            .is_some_and(|root| root.split(target, new, direction))
    }

    /// Remove o painel; o vizinho ocupa o espaço.
    pub fn remove(&mut self, id: PaneId) {
        self.root = self.root.take().and_then(|root| root.remove(id));
    }

    /// Retângulo de cada painel na `area`, na ordem da árvore.
    pub fn rects(&self, area: CellRect) -> Vec<(PaneId, CellRect)> {
        self.layout(area).0
    }

    /// Área do terminal de cada painel: com a tab dividida, a primeira
    /// linha de cada painel é a barra de título.
    pub fn content_rects(&self, area: CellRect) -> Vec<(PaneId, CellRect)> {
        let mut rects = self.rects(area);
        if rects.len() > 1 {
            for (_, rect) in &mut rects {
                *rect = rect.content();
            }
        }
        rects
    }

    /// Painel cujo terminal está na célula (linha, coluna); a barra de
    /// título não conta.
    pub fn content_at(&self, area: CellRect, row: usize, col: usize) -> Option<(PaneId, CellRect)> {
        self.content_rects(area)
            .into_iter()
            .find(|(_, rect)| rect.contains(row, col))
    }

    /// Separadores entre painéis na `area`.
    pub fn dividers(&self, area: CellRect) -> Vec<Divider> {
        self.layout(area).1
    }

    fn layout(&self, area: CellRect) -> (Vec<(PaneId, CellRect)>, Vec<Divider>) {
        let mut rects = Vec::new();
        let mut dividers = Vec::new();
        if let Some(root) = &self.root {
            root.layout(area, &mut rects, &mut dividers);
        }
        (rects, dividers)
    }

    /// Painel vizinho de `from` na direção pedida (o mais alinhado e próximo).
    pub fn neighbor(
        &self,
        from: PaneId,
        direction: FocusDirection,
        area: CellRect,
    ) -> Option<PaneId> {
        let rects = self.rects(area);
        let current = rects.iter().find(|(id, _)| *id == from)?.1;
        let (cx, cy) = current.center();
        rects
            .iter()
            .filter(|(id, rect)| {
                *id != from
                    && rect.cols > 0
                    && match direction {
                        FocusDirection::Left => rect.col + rect.cols <= current.col,
                        FocusDirection::Right => rect.col >= current.col + current.cols,
                        FocusDirection::Up => rect.row + rect.rows <= current.row,
                        FocusDirection::Down => rect.row >= current.row + current.rows,
                    }
            })
            .min_by(|(_, a), (_, b)| {
                let dist = |r: &CellRect| {
                    let (x, y) = r.center();
                    match direction {
                        FocusDirection::Left | FocusDirection::Right => {
                            (x - cx).abs() + 2.0 * (y - cy).abs()
                        }
                        FocusDirection::Up | FocusDirection::Down => {
                            (y - cy).abs() + 2.0 * (x - cx).abs()
                        }
                    }
                };
                dist(a).total_cmp(&dist(b))
            })
            .map(|(id, _)| *id)
    }

    /// Aumenta (delta > 0) ou diminui o painel na direção da divisão.
    pub fn resize(&mut self, id: PaneId, direction: SplitDirection, delta: f32) -> bool {
        self.root
            .as_mut()
            .is_some_and(|root| root.resize(id, direction, delta))
    }

    /// Painel na célula (linha, coluna) da área.
    pub fn pane_at(&self, area: CellRect, row: usize, col: usize) -> Option<(PaneId, CellRect)> {
        self.rects(area)
            .into_iter()
            .find(|(_, rect)| rect.contains(row, col))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: CellRect = CellRect {
        col: 0,
        row: 0,
        cols: 81,
        rows: 25,
    };

    /// #32: só tab dividida perde a primeira linha de cada painel.
    #[test]
    fn test_content_rects_reserve_the_title_row_only_when_split() {
        let mut layout = PaneLayout::single(1);
        assert_eq!(layout.content_rects(AREA), vec![(1, AREA)]);
        layout.split(1, 2, SplitDirection::Vertical);
        let full = layout.rects(AREA);
        let content = layout.content_rects(AREA);
        for ((id, outer), (cid, inner)) in full.iter().zip(&content) {
            assert_eq!(id, cid);
            assert_eq!(inner.row, outer.row + 1);
            assert_eq!(inner.rows, outer.rows - 1);
            assert_eq!((inner.col, inner.cols), (outer.col, outer.cols));
        }
        // A barra de título não é terminal.
        let (_, first) = full[0];
        assert_eq!(layout.content_at(AREA, first.row as usize, 0), None);
        assert_eq!(
            layout
                .content_at(AREA, first.row as usize + 1, 0)
                .map(|(id, _)| id),
            Some(1)
        );
        // Painel de uma linha fica sem barra.
        let tiny = CellRect::new(0, 0, 10, 1);
        assert_eq!(tiny.content(), tiny);
    }

    #[test]
    fn test_single_pane_takes_the_whole_area() {
        let layout = PaneLayout::single(1);
        assert_eq!(layout.rects(AREA), vec![(1, AREA)]);
        assert!(layout.dividers(AREA).is_empty());
    }

    #[test]
    fn test_horizontal_split_leaves_a_divider_column() {
        let mut layout = PaneLayout::single(1);
        assert!(layout.split(1, 2, SplitDirection::Horizontal));
        let rects = layout.rects(AREA);
        assert_eq!(rects[0], (1, CellRect::new(0, 0, 40, 25)));
        assert_eq!(rects[1], (2, CellRect::new(41, 0, 40, 25)));
        assert_eq!(
            layout.dividers(AREA),
            vec![Divider {
                direction: SplitDirection::Horizontal,
                position: 40,
                start: 0,
                len: 25
            }]
        );
    }

    #[test]
    fn test_nested_split_and_remove() {
        let mut layout = PaneLayout::single(1);
        layout.split(1, 2, SplitDirection::Horizontal);
        layout.split(2, 3, SplitDirection::Vertical);
        let rects = layout.rects(AREA);
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[1], (2, CellRect::new(41, 0, 40, 12)));
        assert_eq!(rects[2], (3, CellRect::new(41, 13, 40, 12)));

        layout.remove(2);
        let rects = layout.rects(AREA);
        assert_eq!(rects[1], (3, CellRect::new(41, 0, 40, 25)));
        layout.remove(1);
        assert_eq!(layout.rects(AREA), vec![(3, AREA)]);
        layout.remove(3);
        assert!(layout.is_empty());
        assert!(!layout.split(3, 4, SplitDirection::Vertical));
    }

    #[test]
    fn test_neighbor_navigation() {
        let mut layout = PaneLayout::single(1);
        layout.split(1, 2, SplitDirection::Horizontal);
        layout.split(2, 3, SplitDirection::Vertical);
        assert_eq!(layout.neighbor(1, FocusDirection::Right, AREA), Some(2));
        assert_eq!(layout.neighbor(3, FocusDirection::Up, AREA), Some(2));
        assert_eq!(layout.neighbor(3, FocusDirection::Left, AREA), Some(1));
        assert_eq!(layout.neighbor(1, FocusDirection::Left, AREA), None);
        assert_eq!(layout.neighbor(2, FocusDirection::Down, AREA), Some(3));
    }

    #[test]
    fn test_resize_moves_the_nearest_matching_divider() {
        let mut layout = PaneLayout::single(1);
        layout.split(1, 2, SplitDirection::Horizontal);
        assert!(layout.resize(1, SplitDirection::Horizontal, 0.1));
        assert_eq!(layout.rects(AREA)[0].1.cols, 48);
        assert!(layout.resize(2, SplitDirection::Horizontal, 0.1));
        assert_eq!(layout.rects(AREA)[0].1.cols, 40);
        assert!(!layout.resize(1, SplitDirection::Vertical, 0.1));
        for _ in 0..20 {
            layout.resize(1, SplitDirection::Horizontal, 0.1);
        }
        assert_eq!(layout.rects(AREA)[0].1.cols, 72);
    }

    #[test]
    fn test_export_import_round_trip_keeps_ratios() {
        let mut layout = PaneLayout::single(1);
        layout.split(1, 2, SplitDirection::Horizontal);
        layout.split(2, 3, SplitDirection::Vertical);
        layout.resize(1, SplitDirection::Horizontal, 0.1);
        let tree = layout.export(|id| format!("dir{id}")).unwrap();
        let json = serde_json::to_string(&tree).unwrap();
        let back: LayoutTree<String> = serde_json::from_str(&json).unwrap();
        let mut next = 10;
        let rebuilt = PaneLayout::import(&back, |_dir| {
            next += 1;
            next
        });
        let sizes = |l: &PaneLayout| -> Vec<CellRect> {
            l.rects(AREA).into_iter().map(|(_, r)| r).collect()
        };
        assert_eq!(sizes(&rebuilt), sizes(&layout));
        assert!(PaneLayout::default().export(|id| id).is_none());
    }

    #[test]
    fn test_pane_at_and_tiny_areas() {
        let mut layout = PaneLayout::single(1);
        layout.split(1, 2, SplitDirection::Horizontal);
        assert_eq!(layout.pane_at(AREA, 3, 10).map(|p| p.0), Some(1));
        assert_eq!(layout.pane_at(AREA, 3, 40), None);
        assert_eq!(layout.pane_at(AREA, 3, 50).map(|p| p.0), Some(2));

        let tiny = CellRect::new(0, 0, 2, 5);
        let rects = layout.rects(tiny);
        assert_eq!(rects[0].1, tiny);
        assert_eq!(rects[1].1.cols, 0);
    }
}
