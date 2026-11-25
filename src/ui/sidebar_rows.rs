//! Geometria das linhas da sidebar de tabs: que tab está em cada linha,
//! recuo das tabs de um grupo, a seta de recolher e onde um arraste solta.
//!
//! O desenho (`window.rs`) e o mouse (`main.rs`) usam as mesmas contas daqui,
//! então o que se vê é o que se clica.

use crate::ui::overlay_hits::Rect;
use crate::ui::tabs::{DropTarget, TabManager};

/// Altura de cada cartão de tab na sidebar.
pub const SIDEBAR_ENTRY_HEIGHT: f32 = 74.0;

/// Recuo das tabs de dentro de um grupo (cabe o conector da árvore).
pub const CHILD_INDENT: f32 = 26.0;

/// x da linha vertical da árvore (alinhada com a seta do grupo).
pub const TREE_X: f32 = 10.0;

/// Altura, a partir do topo do cartão, em que o conector chega ao título.
pub const TREE_JOIN_Y: f32 = 17.0;

/// Espessura das linhas da árvore.
pub const TREE_LINE: f32 = 1.5;

/// Lado do ponto de status no fim do conector.
pub const TREE_DOT: f32 = 6.0;

/// Largura da área da seta (recolher/abrir) no começo do cartão de um grupo.
pub const TOGGLE_WIDTH: f32 = 24.0;

/// Margem esquerda do título numa tab solta.
const TITLE_X: f32 = 14.0;

/// Uma linha visível da sidebar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarRow {
    /// Índice da tab.
    pub tab: usize,
    /// Topo do cartão, em pixels da janela.
    pub y: f32,
    /// Está dentro de um grupo.
    pub nested: bool,
    /// Quantas tabs estão dentro desta (0 = não é grupo).
    pub children: usize,
    /// Grupo recolhido.
    pub collapsed: bool,
    /// Última tab visível do grupo (o conector termina em └─).
    pub last_in_group: bool,
}

impl SidebarRow {
    /// Onde começa o texto do cartão.
    pub fn text_x(&self) -> f32 {
        if self.nested {
            TITLE_X + CHILD_INDENT
        } else if self.children > 0 {
            TOGGLE_WIDTH
        } else {
            TITLE_X
        }
    }

    /// (x, y) dentro da seta de recolher deste cartão.
    pub fn toggle_contains(&self, x: f32, y: f32) -> bool {
        self.children > 0 && x < TOGGLE_WIDTH && y >= self.y && y < self.y + 34.0
    }
}

/// Linhas visíveis, começando em `top`.
pub fn sidebar_rows(tabs: &TabManager, top: f32) -> Vec<SidebarRow> {
    let visible = tabs.visible_tabs();
    visible
        .iter()
        .enumerate()
        .map(|(row, &tab)| {
            let parent = tabs.parent_index(tab);
            let next_parent = visible
                .get(row + 1)
                .and_then(|&next| tabs.parent_index(next));
            SidebarRow {
                tab,
                y: top + row as f32 * SIDEBAR_ENTRY_HEIGHT,
                nested: parent.is_some(),
                children: tabs.child_count(tab),
                collapsed: tabs.tabs()[tab].is_collapsed(),
                last_in_group: parent.is_some() && next_parent != parent,
            }
        })
        .collect()
}

/// Linhas da árvore de um cartão: ├─ / └─ nas tabs de dentro (com o │
/// seguindo pelo grupo) e o tronco que desce do cartão de cima de um grupo
/// aberto até a primeira de dentro.
pub fn tree_lines(row: &SidebarRow) -> Vec<Rect> {
    let h = SIDEBAR_ENTRY_HEIGHT;
    let x = TREE_X - TREE_LINE / 2.0;
    if row.nested {
        let join = row.y + TREE_JOIN_Y;
        let bottom = if row.last_in_group {
            join + TREE_LINE
        } else {
            row.y + h
        };
        let dot_x = tree_dot(row).map_or(row.text_x() - 8.0, |dot| dot.x);
        vec![
            Rect::new(x, row.y, TREE_LINE, bottom - row.y),
            Rect::new(x, join, dot_x - x, TREE_LINE),
        ]
    } else if row.children > 0 && !row.collapsed {
        let top = row.y + 30.0;
        vec![Rect::new(x, top, TREE_LINE, row.y + h - top)]
    } else {
        Vec::new()
    }
}

/// Ponto de status na ponta do conector de uma tab de dentro.
pub fn tree_dot(row: &SidebarRow) -> Option<Rect> {
    row.nested.then(|| {
        Rect::new(
            row.text_x() - 8.0 - TREE_DOT,
            row.y + TREE_JOIN_Y - TREE_DOT / 2.0 + TREE_LINE / 2.0,
            TREE_DOT,
            TREE_DOT,
        )
    })
}

/// Linha sob a altura `y`.
pub fn row_at(rows: &[SidebarRow], y: f32) -> Option<&SidebarRow> {
    rows.iter()
        .find(|row| y >= row.y && y < row.y + SIDEBAR_ENTRY_HEIGHT)
}

/// Onde solta uma tab arrastada até a altura `y`: o quarto de cima e o de
/// baixo de um cartão são o espaço entre cartões; o meio é "dentro".
pub fn drop_target_in(rows: &[SidebarRow], tab_count: usize, y: f32) -> DropTarget {
    let Some(first) = rows.first() else {
        return DropTarget::Gap(tab_count);
    };
    if y < first.y {
        return DropTarget::Gap(first.tab);
    }
    let Some((i, row)) = rows
        .iter()
        .enumerate()
        .find(|(_, row)| y < row.y + SIDEBAR_ENTRY_HEIGHT)
    else {
        return DropTarget::Gap(tab_count);
    };
    let frac = (y - row.y) / SIDEBAR_ENTRY_HEIGHT;
    if frac < 0.25 {
        DropTarget::Gap(row.tab)
    } else if frac > 0.75 {
        DropTarget::Gap(rows.get(i + 1).map_or(tab_count, |next| next.tab))
    } else {
        DropTarget::Nest(row.tab)
    }
}

/// O que o arraste vai fazer, para desenhar: em que cartão (tab de cima do
/// grupo) a tab entra, ou em que linha fica a marca do espaço. `None` quando
/// soltar ali não muda nada (sobre ela mesma).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropHint {
    /// Destacar o cartão da tab (índice).
    Into(usize),
    /// Marca antes da linha `row` (`rows.len()` = depois da última).
    Line(usize),
}

/// Traduz o alvo para o desenho, do mesmo jeito que o modelo aplica: um
/// espaço dentro de um grupo vira logo abaixo do grupo; soltar sobre uma
/// tab de dentro entra no grupo dela.
pub fn drop_hint(
    tabs: &TabManager,
    rows: &[SidebarRow],
    dragged: usize,
    target: DropTarget,
) -> Option<DropHint> {
    match target {
        DropTarget::Nest(tab) => {
            let top = tabs.parent_index(tab).unwrap_or(tab);
            let own_group = top == dragged || tabs.parent_index(dragged) == Some(top);
            (!own_group).then_some(DropHint::Into(top))
        }
        DropTarget::Gap(gap) => {
            let after_group = gap < tabs.tab_count() && tabs.parent_index(gap).is_some();
            let line = if after_group {
                let parent = tabs.parent_index(gap)?;
                let start = rows.iter().position(|row| row.tab == parent)?;
                rows[start + 1..]
                    .iter()
                    .position(|row| tabs.parent_index(row.tab) != Some(parent))
                    .map_or(rows.len(), |offset| start + 1 + offset)
            } else {
                rows.iter()
                    .position(|row| row.tab == gap)
                    .unwrap_or(rows.len())
            };
            Some(DropHint::Line(line))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(n: usize) -> TabManager {
        let mut manager = TabManager::new();
        for _ in 0..n {
            manager.create_tab("bash");
        }
        manager
    }

    const H: f32 = SIDEBAR_ENTRY_HEIGHT;

    /// Teste do layout da árvore (#25): ├─ segue até o próximo cartão, └─
    /// termina no conector, o tronco do grupo aberto desce até a primeira
    /// de dentro, e o ponto fica entre o conector e o título.
    #[test]
    fn test_tree_connectors_positions() {
        let mut m = manager(4);
        m.nest_tab(1, 0);
        m.nest_tab(2, 0); // [0 [1 2]] 3
        let rows = sidebar_rows(&m, 30.0);
        assert!(!rows[1].last_in_group && rows[2].last_in_group);
        assert!(!rows[3].last_in_group && !rows[0].last_in_group);

        // Tronco do cartão de cima: do pé da seta até o fim do cartão.
        let stem = tree_lines(&rows[0]);
        assert_eq!(stem.len(), 1);
        assert_eq!(stem[0].y + stem[0].height, rows[1].y);
        assert!(stem[0].y > rows[0].y + 20.0);

        // ├─: a vertical cobre o cartão inteiro (liga ao próximo).
        let tee = tree_lines(&rows[1]);
        assert_eq!(tee[0].y, rows[1].y);
        assert_eq!(tee[0].y + tee[0].height, rows[2].y);
        // └─: a vertical para no conector.
        let elbow = tree_lines(&rows[2]);
        let join = rows[2].y + TREE_JOIN_Y;
        assert!((elbow[0].y + elbow[0].height - (join + TREE_LINE)).abs() < 0.01);
        // Todas as verticais na mesma coluna, a da seta.
        assert_eq!(stem[0].x, tee[0].x);
        assert_eq!(tee[0].x, elbow[0].x);
        assert!((tee[0].x + TREE_LINE / 2.0 - TREE_X).abs() < 0.01);

        // Conector horizontal até o ponto; o ponto antes do título.
        let dot = tree_dot(&rows[1]).unwrap();
        assert_eq!(tee[1].y, rows[1].y + TREE_JOIN_Y);
        assert!((tee[1].x + tee[1].width - dot.x).abs() < 0.01);
        assert!(dot.x + dot.width < rows[1].text_x());
        assert!(dot.y < tee[1].y + TREE_LINE && dot.y + dot.height > tee[1].y);

        // Tab solta e grupo recolhido não desenham linhas.
        assert!(tree_lines(&rows[3]).is_empty());
        assert!(tree_dot(&rows[3]).is_none());
        m.set_collapsed(0, true);
        let rows = sidebar_rows(&m, 30.0);
        assert!(tree_lines(&rows[0]).is_empty());
    }

    #[test]
    fn test_rows_skip_collapsed_and_indent() {
        let mut m = manager(4);
        m.nest_tab(1, 0);
        m.nest_tab(2, 0); // [0 [1 2]] 3
        let rows = sidebar_rows(&m, 30.0);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].children, 2);
        assert!(rows[1].nested && rows[1].text_x() > rows[3].text_x());
        assert!(rows[0].text_x() > rows[3].text_x());
        assert_eq!(rows[3].y, 30.0 + 3.0 * H);
        m.set_collapsed(0, true);
        let rows = sidebar_rows(&m, 30.0);
        assert_eq!(rows.iter().map(|r| r.tab).collect::<Vec<_>>(), vec![0, 3]);
        assert!(rows[0].collapsed);
        assert_eq!(row_at(&rows, 30.0 + H + 1.0).map(|r| r.tab), Some(3));
        assert_eq!(row_at(&rows, 10.0), None);
        assert!(rows[0].toggle_contains(5.0, 40.0));
        assert!(!rows[0].toggle_contains(50.0, 40.0));
        assert!(!rows[1].toggle_contains(5.0, rows[1].y + 2.0));
    }

    #[test]
    fn test_drop_target_by_height() {
        let m = manager(3);
        let rows = sidebar_rows(&m, 0.0);
        assert_eq!(drop_target_in(&rows, 3, 2.0), DropTarget::Gap(0));
        assert_eq!(drop_target_in(&rows, 3, H * 0.5), DropTarget::Nest(0));
        assert_eq!(drop_target_in(&rows, 3, H * 0.9), DropTarget::Gap(1));
        assert_eq!(drop_target_in(&rows, 3, H * 2.9), DropTarget::Gap(3));
        assert_eq!(drop_target_in(&rows, 3, H * 9.0), DropTarget::Gap(3));
        assert_eq!(drop_target_in(&rows, 3, -5.0), DropTarget::Gap(0));
        assert_eq!(drop_target_in(&[], 0, 5.0), DropTarget::Gap(0));
    }

    #[test]
    fn test_drop_hint_matches_model() {
        let mut m = manager(4);
        m.nest_tab(1, 0);
        m.nest_tab(2, 0); // [0 [1 2]] 3
        let rows = sidebar_rows(&m, 0.0);
        // Sobre uma de dentro: o destaque vai para a de cima do grupo.
        assert_eq!(
            drop_hint(&m, &rows, 3, DropTarget::Nest(2)),
            Some(DropHint::Into(0))
        );
        // No próprio grupo não muda nada.
        assert_eq!(drop_hint(&m, &rows, 1, DropTarget::Nest(0)), None);
        assert_eq!(drop_hint(&m, &rows, 0, DropTarget::Nest(1)), None);
        // Espaço entre tabs do grupo: a marca fica abaixo do grupo.
        assert_eq!(
            drop_hint(&m, &rows, 3, DropTarget::Gap(2)),
            Some(DropHint::Line(3))
        );
        assert_eq!(
            drop_hint(&m, &rows, 1, DropTarget::Gap(3)),
            Some(DropHint::Line(3))
        );
        assert_eq!(
            drop_hint(&m, &rows, 1, DropTarget::Gap(4)),
            Some(DropHint::Line(4))
        );
        assert_eq!(
            drop_hint(&m, &rows, 3, DropTarget::Gap(0)),
            Some(DropHint::Line(0))
        );
    }
}
