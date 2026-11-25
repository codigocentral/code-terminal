//! Grupos de tabs na sidebar: uma tab pode ficar dentro de outra (um nível).
//!
//! As tabs continuam num vetor só, na ordem da sidebar: as de um grupo vêm
//! logo depois da tab de cima. Assim o índice da tab, a linha da sidebar e os
//! atalhos de navegação seguem a mesma ordem, e o código que usa índice
//! (confirmação de fechar, avisos, Socket API) não muda.

use std::ops::Range;

use super::{Tab, TabId, TabManager};

/// Onde uma tab arrastada na sidebar foi solta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    /// Em cima da tab `index`: entra no grupo dela.
    Nest(usize),
    /// No espaço antes da tab `index` (`tab_count()` = depois da última):
    /// fica ali, fora de grupos.
    Gap(usize),
}

/// Onde pôr um bloco de tabs, calculado antes de tirá-lo do vetor (os
/// índices mudam; os ids não).
#[derive(Debug, Clone, Copy)]
enum Place {
    Before(TabId),
    AfterGroup(TabId),
    End,
}

fn position(tabs: &[Tab], id: TabId) -> Option<usize> {
    tabs.iter().position(|tab| tab.id == id)
}

/// Fim (exclusivo) do grupo que começa na tab de cima `index`.
fn group_end(tabs: &[Tab], index: usize) -> usize {
    let Some(top) = tabs.get(index) else {
        return tabs.len();
    };
    let mut end = index + 1;
    while tabs.get(end).is_some_and(|tab| tab.parent == Some(top.id)) {
        end += 1;
    }
    end
}

impl TabManager {
    /// Índice da tab com o id `id`.
    pub fn tab_index_of(&self, id: TabId) -> Option<usize> {
        position(&self.tabs, id)
    }

    /// Índice da tab de cima do grupo em que a tab `index` está.
    pub fn parent_index(&self, index: usize) -> Option<usize> {
        let parent = self.tabs.get(index)?.parent?;
        self.tab_index_of(parent)
    }

    /// Quantas tabs estão dentro da tab `index`.
    pub fn child_count(&self, index: usize) -> usize {
        let Some(tab) = self.tabs.get(index) else {
            return 0;
        };
        self.tabs
            .iter()
            .filter(|other| other.parent == Some(tab.id))
            .count()
    }

    /// A tab está num grupo recolhido (não aparece na sidebar).
    pub fn is_hidden(&self, index: usize) -> bool {
        self.parent_index(index)
            .is_some_and(|parent| self.tabs[parent].collapsed)
    }

    /// Tabs que aparecem na sidebar, em ordem (uma por linha).
    pub fn visible_tabs(&self) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&index| !self.is_hidden(index))
            .collect()
    }

    /// Tab da `n`-ésima linha visível da sidebar (atalho Ctrl+N).
    pub fn visible_tab_at(&self, n: usize) -> Option<usize> {
        self.visible_tabs().get(n).copied()
    }

    /// Tabs em que a tab `index` pode entrar: as de cima, menos ela mesma e
    /// o grupo em que já está.
    pub fn nest_candidates(&self, index: usize) -> Vec<usize> {
        let Some(tab) = self.tabs.get(index) else {
            return Vec::new();
        };
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, other)| {
                other.parent.is_none() && other.id != tab.id && Some(other.id) != tab.parent
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// A tab e o que anda junto com ela: a de cima leva o grupo inteiro.
    fn block_of(&self, index: usize) -> Range<usize> {
        if self.tabs[index].parent.is_some() {
            index..index + 1
        } else {
            index..group_end(&self.tabs, index)
        }
    }

    /// Move as tabs de `block` para onde `place` disser, mantendo a tab
    /// ativa (pelo id).
    fn relocate(&mut self, block: Range<usize>, place: Place) {
        let active = self.tabs.get(self.active_tab_index).map(|tab| tab.id);
        let moved: Vec<Tab> = self.tabs.drain(block).collect();
        let at = match place {
            Place::End => self.tabs.len(),
            Place::Before(id) => position(&self.tabs, id).unwrap_or(self.tabs.len()),
            Place::AfterGroup(id) => {
                position(&self.tabs, id).map_or(self.tabs.len(), |i| group_end(&self.tabs, i))
            }
        };
        self.tabs.splice(at..at, moved);
        if let Some(index) = active.and_then(|id| self.tab_index_of(id)) {
            self.active_tab_index = index;
        }
    }

    /// Põe a tab `child` dentro do grupo da tab `parent` (no fim dele).
    ///
    /// Só há um nível: soltar sobre uma tab de dentro de um grupo entra no
    /// mesmo grupo, e uma tab de cima leva as suas junto (todas passam a ser
    /// do novo grupo). Devolve `false` quando nada muda.
    pub fn nest_tab(&mut self, child: usize, parent: usize) -> bool {
        if child >= self.tabs.len() || parent >= self.tabs.len() {
            return false;
        }
        let target = self.parent_index(parent).unwrap_or(parent);
        let block = self.block_of(child);
        let target_id = self.tabs[target].id;
        if block.contains(&target) || self.tabs[child].parent == Some(target_id) {
            return false;
        }
        for tab in &mut self.tabs[block.clone()] {
            tab.parent = Some(target_id);
            tab.collapsed = false;
        }
        self.tabs[target].collapsed = false;
        self.relocate(block, Place::AfterGroup(target_id));
        true
    }

    /// Tira a tab `index` do grupo: ela vai para logo abaixo do grupo.
    pub fn unnest_tab(&mut self, index: usize) -> bool {
        let Some(parent) = self.parent_index(index) else {
            return false;
        };
        let parent_id = self.tabs[parent].id;
        self.tabs[index].parent = None;
        self.relocate(index..index + 1, Place::AfterGroup(parent_id));
        true
    }

    /// Solta a tab `index` no espaço antes da tab `gap` (`tab_count()` = no
    /// fim). Ela fica fora de grupos; um espaço entre tabs de um grupo conta
    /// como logo abaixo do grupo. Uma tab de cima leva o grupo junto.
    pub fn move_to_gap(&mut self, index: usize, gap: usize) -> bool {
        if index >= self.tabs.len() || gap > self.tabs.len() {
            return false;
        }
        let block = self.block_of(index);
        let place = match self.tabs.get(gap) {
            None => Place::End,
            Some(tab) => match tab.parent {
                Some(parent) => Place::AfterGroup(parent),
                None => Place::Before(tab.id),
            },
        };
        let is_child = self.tabs[index].parent.is_some();
        let lands_on_itself = match place {
            Place::Before(id) | Place::AfterGroup(id) => {
                self.tab_index_of(id).is_some_and(|i| block.contains(&i))
            }
            Place::End => false,
        };
        if !is_child && (lands_on_itself || gap == block.end) {
            return false;
        }
        self.tabs[index].parent = None;
        self.relocate(block, place);
        true
    }

    /// Aplica o fim de um arraste na sidebar.
    pub fn drop_tab(&mut self, index: usize, target: DropTarget) -> bool {
        match target {
            DropTarget::Nest(parent) if parent != index => self.nest_tab(index, parent),
            DropTarget::Nest(_) => false,
            DropTarget::Gap(gap) => self.move_to_gap(index, gap),
        }
    }

    /// Recolhe ou abre o grupo da tab `index` (precisa ter tabs dentro).
    /// Recolher com uma tab de dentro ativa passa o foco para a de cima.
    pub fn set_collapsed(&mut self, index: usize, collapsed: bool) -> bool {
        if self.child_count(index) == 0 || self.tabs[index].collapsed == collapsed {
            return false;
        }
        self.tabs[index].collapsed = collapsed;
        if collapsed && self.parent_index(self.active_tab_index) == Some(index) {
            self.set_active_tab(index);
        }
        true
    }

    /// Alterna recolher/abrir o grupo da tab `index`.
    pub fn toggle_collapsed(&mut self, index: usize) -> bool {
        let collapsed = self.tabs.get(index).is_some_and(|tab| tab.collapsed);
        self.set_collapsed(index, !collapsed)
    }

    /// Abre o grupo em que a tab `index` está, se estiver recolhido.
    pub(super) fn expand_group_of(&mut self, index: usize) {
        if let Some(parent) = self.parent_index(index) {
            self.tabs[parent].collapsed = false;
        }
    }

    /// Próxima (ou anterior) tab visível, dando a volta.
    pub(super) fn cycle_visible(&self, forward: bool) -> Option<usize> {
        let visible = self.visible_tabs();
        if visible.len() < 2 {
            return None;
        }
        let active = self.active_tab_index;
        let current = visible
            .iter()
            .position(|&i| i == active)
            .or_else(|| {
                let parent = self.parent_index(active)?;
                visible.iter().position(|&i| i == parent)
            })
            .unwrap_or(0);
        let n = visible.len();
        let next = if forward {
            (current + 1) % n
        } else {
            (current + n - 1) % n
        };
        Some(visible[next])
    }

    /// Grupo de cada tab, na ordem atual: índice da tab de cima e se o grupo
    /// está recolhido. É o que a sessão salva.
    pub fn groups(&self) -> Vec<(Option<usize>, bool)> {
        (0..self.tabs.len())
            .map(|index| (self.parent_index(index), self.tabs[index].collapsed))
            .collect()
    }

    /// Refaz os grupos salvos por [`TabManager::groups`] (mesma ordem de
    /// tabs). Dados inválidos (pai que não existe, grupo dentro de grupo)
    /// são corrigidos por [`TabManager::normalize_groups`].
    pub fn restore_groups(&mut self, groups: &[(Option<usize>, bool)]) {
        let ids: Vec<TabId> = self.tabs.iter().map(|tab| tab.id).collect();
        for (tab, (parent, collapsed)) in self.tabs.iter_mut().zip(groups) {
            tab.parent = parent.and_then(|p| ids.get(p).copied());
            tab.collapsed = *collapsed;
        }
        self.normalize_groups();
    }

    /// Deixa os grupos válidos: pai existente e de cima (grupo dentro de
    /// grupo sobe para o grupo de cima; ciclo vira tab solta), tabs de um
    /// grupo logo depois da de cima e "recolhido" só em quem tem tabs dentro.
    pub fn normalize_groups(&mut self) {
        let len = self.tabs.len();
        for i in 0..len {
            let me = self.tabs[i].id;
            let mut parent = self.tabs[i].parent;
            for _ in 0..len {
                let Some(p) = parent else { break };
                if p == me {
                    parent = None;
                    break;
                }
                match self.tab_index_of(p).map(|pi| self.tabs[pi].parent) {
                    None => {
                        parent = None;
                        break;
                    }
                    Some(None) => break,
                    Some(Some(grand)) => parent = Some(grand),
                }
            }
            let nested = parent
                .and_then(|p| self.tab_index_of(p))
                .is_some_and(|pi| self.tabs[pi].parent.is_some());
            self.tabs[i].parent = if nested { None } else { parent };
        }

        let active = self.tabs.get(self.active_tab_index).map(|tab| tab.id);
        let mut rest: Vec<Option<Tab>> = self.tabs.drain(..).map(Some).collect();
        let mut ordered = Vec::with_capacity(len);
        for i in 0..rest.len() {
            if !rest[i].as_ref().is_some_and(|tab| tab.parent.is_none()) {
                continue;
            }
            let Some(top) = rest[i].take() else { continue };
            let top_id = top.id;
            ordered.push(top);
            for slot in rest.iter_mut() {
                if slot.as_ref().is_some_and(|tab| tab.parent == Some(top_id)) {
                    ordered.extend(slot.take());
                }
            }
        }
        ordered.extend(rest.into_iter().flatten());
        self.tabs = ordered;

        for i in 0..self.tabs.len() {
            if self.child_count(i) == 0 {
                self.tabs[i].collapsed = false;
            }
        }
        if let Some(index) = active.and_then(|id| self.tab_index_of(id)) {
            self.active_tab_index = index;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gerenciador com `n` tabs sem PTY; devolve os ids em ordem.
    fn manager(n: usize) -> (TabManager, Vec<TabId>) {
        let mut manager = TabManager::new();
        let ids = (0..n).map(|_| manager.create_tab("bash")).collect();
        (manager, ids)
    }

    fn order(manager: &TabManager) -> Vec<TabId> {
        manager.tabs().iter().map(|tab| tab.id).collect()
    }

    fn parents(manager: &TabManager) -> Vec<Option<TabId>> {
        manager.tabs().iter().map(|tab| tab.parent()).collect()
    }

    #[test]
    fn test_nest_moves_tab_to_end_of_group() {
        let (mut m, ids) = manager(4);
        assert!(m.nest_tab(3, 0));
        assert_eq!(order(&m), vec![ids[0], ids[3], ids[1], ids[2]]);
        assert!(m.nest_tab(3, 0));
        assert_eq!(order(&m), vec![ids[0], ids[3], ids[2], ids[1]]);
        assert_eq!(m.child_count(0), 2);
        assert_eq!(m.parent_index(1), Some(0));
        assert_eq!(m.parent_index(3), None);
    }

    #[test]
    fn test_nest_onto_child_joins_its_group() {
        let (mut m, ids) = manager(3);
        m.nest_tab(1, 0);
        // Solta a terceira sobre a de dentro: entra no mesmo grupo (um nível).
        assert!(m.nest_tab(2, 1));
        assert_eq!(parents(&m), vec![None, Some(ids[0]), Some(ids[0])]);
    }

    #[test]
    fn test_nest_group_into_other_flattens() {
        let (mut m, ids) = manager(4);
        m.nest_tab(1, 0); // [0 [1]] 2 3
        m.nest_tab(3, 2); // [0 [1]] [2 [3]]
        assert!(m.nest_tab(2, 0)); // o grupo do 2 entra no do 0
        assert_eq!(order(&m), vec![ids[0], ids[1], ids[2], ids[3]]);
        assert_eq!(
            parents(&m),
            vec![None, Some(ids[0]), Some(ids[0]), Some(ids[0])]
        );
    }

    #[test]
    fn test_nest_noops() {
        let (mut m, _) = manager(3);
        assert!(!m.nest_tab(0, 0));
        assert!(!m.nest_tab(0, 9));
        m.nest_tab(1, 0);
        // Já está no grupo; e a de cima não entra na própria de dentro.
        assert!(!m.nest_tab(1, 0));
        assert!(!m.nest_tab(0, 1));
        assert!(!m.drop_tab(2, DropTarget::Nest(2)));
    }

    #[test]
    fn test_unnest_goes_below_group() {
        let (mut m, ids) = manager(4);
        m.nest_tab(2, 0);
        m.nest_tab(3, 0); // [0 [2 3]] 1
        assert_eq!(order(&m), vec![ids[0], ids[2], ids[3], ids[1]]);
        assert!(m.unnest_tab(1));
        assert_eq!(order(&m), vec![ids[0], ids[3], ids[2], ids[1]]);
        assert_eq!(parents(&m), vec![None, Some(ids[0]), None, None]);
        assert!(!m.unnest_tab(2));
    }

    #[test]
    fn test_move_to_gap_unnests_and_places() {
        let (mut m, ids) = manager(4);
        m.nest_tab(3, 0); // [0 [3]] 1 2
                          // A de dentro solta antes da primeira: sai do grupo e vai para o topo.
        assert!(m.move_to_gap(1, 0));
        assert_eq!(order(&m), vec![ids[3], ids[0], ids[1], ids[2]]);
        assert_eq!(m.parent_index(0), None);
        // No fim da lista.
        assert!(m.move_to_gap(0, 4));
        assert_eq!(order(&m), vec![ids[0], ids[1], ids[2], ids[3]]);
    }

    #[test]
    fn test_gap_inside_group_snaps_below_it() {
        let (mut m, ids) = manager(4);
        m.nest_tab(1, 0);
        m.nest_tab(2, 0); // [0 [1 2]] 3
        assert!(m.move_to_gap(3, 2)); // espaço entre 1 e 2
        assert_eq!(order(&m), vec![ids[0], ids[1], ids[2], ids[3]]);
        assert_eq!(m.parent_index(3), None);
        // A de dentro solta ao lado dela mesma: só sai do grupo.
        assert!(m.move_to_gap(1, 2));
        assert_eq!(order(&m), vec![ids[0], ids[2], ids[1], ids[3]]);
        assert_eq!(m.child_count(0), 1);
    }

    #[test]
    fn test_moving_group_takes_children() {
        let (mut m, ids) = manager(4);
        m.nest_tab(1, 0); // [0 [1]] 2 3
        assert!(m.move_to_gap(0, 4));
        assert_eq!(order(&m), vec![ids[2], ids[3], ids[0], ids[1]]);
        assert_eq!(m.parent_index(3), Some(2));
        // Solto no próprio lugar: nada muda.
        assert!(!m.move_to_gap(2, 4));
        assert!(!m.move_to_gap(2, 3));
        assert!(!m.move_to_gap(2, 2));
    }

    #[test]
    fn test_close_parent_promotes_children_in_place() {
        let (mut m, ids) = manager(4);
        m.nest_tab(1, 0);
        m.nest_tab(2, 0); // [0 [1 2]] 3
        m.set_collapsed(0, true);
        assert!(m.close_tab(0));
        assert_eq!(order(&m), vec![ids[1], ids[2], ids[3]]);
        assert_eq!(parents(&m), vec![None, None, None]);
        assert_eq!(m.visible_tabs(), vec![0, 1, 2]);
    }

    #[test]
    fn test_navigation_follows_visible_order() {
        let (mut m, _) = manager(4);
        m.nest_tab(1, 0);
        m.nest_tab(2, 0); // [0 [1 2]] 3
        m.switch_tab(0);
        m.next_tab();
        assert_eq!(m.active_tab_index(), 1);
        m.set_collapsed(0, true);
        m.switch_tab(0);
        m.next_tab();
        assert_eq!(m.active_tab_index(), 3);
        m.next_tab();
        assert_eq!(m.active_tab_index(), 0);
        m.previous_tab();
        assert_eq!(m.active_tab_index(), 3);
        assert_eq!(m.visible_tab_at(1), Some(3));
        assert_eq!(m.visible_tab_at(2), None);
    }

    #[test]
    fn test_collapse_moves_focus_and_activation_expands() {
        let (mut m, _) = manager(3);
        m.nest_tab(1, 0);
        m.switch_tab(1);
        assert!(m.set_collapsed(0, true));
        assert_eq!(m.active_tab_index(), 0);
        assert!(m.is_hidden(1));
        // Ativar a de dentro (aviso, Socket API) abre o grupo.
        m.switch_tab(1);
        assert!(!m.tabs()[0].is_collapsed());
        // Sem tabs dentro não há o que recolher.
        assert!(!m.set_collapsed(2, true));
        assert!(m.toggle_collapsed(0));
        assert!(m.toggle_collapsed(0));
    }

    #[test]
    fn test_active_tab_follows_moves() {
        let (mut m, ids) = manager(3);
        m.switch_tab(2);
        m.nest_tab(2, 0);
        assert_eq!(m.get_active_tab().map(|t| t.id), Some(ids[2]));
        m.move_to_gap(1, 3);
        assert_eq!(m.get_active_tab().map(|t| t.id), Some(ids[2]));
    }

    #[test]
    fn test_nest_candidates() {
        let (mut m, _) = manager(4);
        m.nest_tab(1, 0); // [0 [1]] 2 3
        assert_eq!(m.nest_candidates(1), vec![2, 3]);
        assert_eq!(m.nest_candidates(2), vec![0, 3]);
        assert!(m.nest_candidates(9).is_empty());
    }

    #[test]
    fn test_groups_round_trip() {
        let (mut m, _) = manager(4);
        m.nest_tab(1, 0);
        m.nest_tab(3, 2);
        m.set_collapsed(2, true);
        let saved = m.groups();
        assert_eq!(
            saved,
            vec![
                (None, false),
                (Some(0), false),
                (None, true),
                (Some(2), false)
            ]
        );
        let (mut fresh, _) = manager(4);
        fresh.restore_groups(&saved);
        assert_eq!(fresh.groups(), saved);
    }

    #[test]
    fn test_restore_fixes_invalid_groups() {
        let (mut m, ids) = manager(4);
        // Pai inexistente, grupo dentro de grupo, filha longe do pai e
        // "recolhido" sem tabs dentro.
        m.restore_groups(&[
            (Some(9), true),
            (Some(3), false),
            (Some(1), false),
            (None, false),
        ]);
        assert_eq!(order(&m), vec![ids[0], ids[3], ids[1], ids[2]]);
        assert_eq!(parents(&m), vec![None, None, Some(ids[3]), Some(ids[3])]);
        assert!(!m.tabs()[0].is_collapsed());
    }

    #[test]
    fn test_normalize_breaks_cycles() {
        let (mut m, ids) = manager(2);
        m.tabs[0].parent = Some(ids[1]);
        m.tabs[1].parent = Some(ids[0]);
        m.normalize_groups();
        let tops = parents(&m).iter().filter(|p| p.is_none()).count();
        assert!(tops >= 1);
        for index in 0..m.tab_count() {
            if let Some(parent) = m.parent_index(index) {
                assert!(m.tabs()[parent].parent().is_none());
                assert!(parent < index);
            }
        }
    }
}
