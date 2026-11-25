use crate::terminal::buffer::TerminalBuffer;
use crate::terminal::Grid;
use crate::ui::pane_layout::{CellRect, FocusDirection, PaneId, PaneLayout, SplitDirection};
use crate::ui::search_bar::{SearchBar, SearchMatch};
use crate::ui::tab_title::TabTitle;

mod groups;
pub use groups::DropTarget;

static NEXT_PANE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Novo identificador de painel (único no processo).
pub fn allocate_pane_id() -> PaneId {
    NEXT_PANE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Painel que não está em foco.
#[derive(Debug)]
pub struct PaneSlot {
    pub id: PaneId,
    pub buffer: TerminalBuffer,
}

pub type TabId = u32;

static NEXT_TAB_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

fn allocate_tab_id() -> TabId {
    NEXT_TAB_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq)]
pub enum TabState {
    Active,
    Inactive,
    Closing,
}

/// Representa uma tab do terminal
/// Contém seu próprio TerminalBuffer com PTY independente
pub struct Tab {
    pub id: TabId,
    pub title_manager: TabTitle,
    /// Buffer do painel em foco (o que recebe teclado). Os demais painéis
    /// ficam em `other_panes`; trocar o foco troca os buffers de lugar.
    pub buffer: Option<TerminalBuffer>,
    focused_pane: PaneId,
    other_panes: Vec<PaneSlot>,
    layout: PaneLayout,
    pub state: TabState,
    pub scrollback_lines: usize,
    /// Título dado pelo usuário: fica fixo, o programa (OSC 0/2) não troca.
    custom_title: Option<String>,
    /// Tab de cima do grupo, se esta estiver aninhada (um nível só).
    parent: Option<TabId>,
    /// Grupo recolhido na sidebar (só vale para a tab de cima).
    collapsed: bool,
    /// Cor escolhida para a tab na sidebar (RGB).
    pub color: Option<[u8; 3]>,
    /// Fundo do terminal escolhido para esta tab (RGB).
    pub background: Option<[u8; 3]>,
}

impl std::fmt::Debug for Tab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tab")
            .field("id", &self.id)
            .field("title", &self.title())
            .field("state", &self.state)
            .field("scrollback_lines", &self.scrollback_lines)
            .field("parent", &self.parent)
            .field("has_buffer", &self.buffer.is_some())
            .finish()
    }
}

impl Tab {
    /// Título da tab: o dado pelo usuário, senão o que o programa definiu
    /// (OSC 0/2), senão o padrão.
    pub fn title(&self) -> String {
        if let Some(custom) = &self.custom_title {
            return custom.clone();
        }
        self.grid()
            .and_then(|grid| grid.title())
            .map(str::to_string)
            .unwrap_or_else(|| self.title_manager.title().to_string())
    }

    /// Título dado pelo usuário, se houver.
    pub fn custom_title(&self) -> Option<&str> {
        self.custom_title.as_deref()
    }

    /// Fixa o título. Texto vazio (ou só espaços) volta ao título automático.
    pub fn set_custom_title(&mut self, title: Option<&str>) {
        self.custom_title = title
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string);
    }

    /// Tab de cima do grupo, se esta estiver aninhada.
    pub fn parent(&self) -> Option<TabId> {
        self.parent
    }

    /// Grupo recolhido (só a tab de cima aparece na sidebar).
    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }

    /// Atualiza o título baseado no processo atual
    pub fn set_process(&mut self, process: &str) {
        self.title_manager.set_process(process);
    }

    /// Limpa o processo atual (volta ao shell)
    pub fn clear_process(&mut self) {
        self.title_manager.clear_process();
    }

    /// Atualiza o diretório de trabalho
    pub fn set_working_dir(&mut self, dir: &str) {
        self.title_manager.set_working_dir(dir);
    }

    /// Cria uma nova tab vazia (sem buffer)
    pub fn new(shell: &str) -> Self {
        Self {
            id: allocate_tab_id(),
            title_manager: TabTitle::new(shell),
            buffer: None,
            focused_pane: 0,
            other_panes: Vec::new(),
            layout: PaneLayout::default(),
            state: TabState::Inactive,
            scrollback_lines: 0,
            custom_title: None,
            parent: None,
            collapsed: false,
            color: None,
            background: None,
        }
    }

    /// Cria uma nova tab com buffer
    pub fn with_buffer(shell: &str, buffer: TerminalBuffer) -> Self {
        Self::with_pane(shell, buffer, allocate_pane_id())
    }

    /// Tab com o primeiro painel já identificado por `pane`.
    pub fn with_pane(shell: &str, buffer: TerminalBuffer, pane: PaneId) -> Self {
        Self {
            id: allocate_tab_id(),
            title_manager: TabTitle::new(shell),
            buffer: Some(buffer),
            focused_pane: pane,
            other_panes: Vec::new(),
            layout: PaneLayout::single(pane),
            state: TabState::Inactive,
            scrollback_lines: 0,
            custom_title: None,
            parent: None,
            collapsed: false,
            color: None,
            background: None,
        }
    }

    /// Tab montada a partir de painéis já criados (restauração de sessão).
    pub fn from_panes(
        shell: &str,
        layout: PaneLayout,
        focused: PaneId,
        mut panes: Vec<(PaneId, TerminalBuffer)>,
    ) -> Self {
        let index = panes.iter().position(|(id, _)| *id == focused).unwrap_or(0);
        let (focused_pane, buffer) = if panes.is_empty() {
            (0, None)
        } else {
            let (id, buffer) = panes.swap_remove(index);
            (id, Some(buffer))
        };
        Self {
            id: allocate_tab_id(),
            title_manager: TabTitle::new(shell),
            buffer,
            focused_pane,
            other_panes: panes
                .into_iter()
                .map(|(id, buffer)| PaneSlot { id, buffer })
                .collect(),
            layout,
            state: TabState::Inactive,
            scrollback_lines: 0,
            custom_title: None,
            parent: None,
            collapsed: false,
            color: None,
            background: None,
        }
    }

    // === Painéis (splits) ===

    /// Painel em foco.
    pub fn focused_pane(&self) -> PaneId {
        self.focused_pane
    }

    /// Quantos painéis a tab tem.
    pub fn pane_count(&self) -> usize {
        self.buffer.is_some() as usize + self.other_panes.len()
    }

    /// Árvore de divisões.
    pub fn layout(&self) -> &PaneLayout {
        &self.layout
    }

    /// Buffer de um painel.
    pub fn pane_buffer(&self, id: PaneId) -> Option<&TerminalBuffer> {
        if id == self.focused_pane {
            return self.buffer.as_ref();
        }
        self.other_panes
            .iter()
            .find(|slot| slot.id == id)
            .map(|slot| &slot.buffer)
    }

    /// Buffer de um painel (mutável).
    pub fn pane_buffer_mut(&mut self, id: PaneId) -> Option<&mut TerminalBuffer> {
        if id == self.focused_pane {
            return self.buffer.as_mut();
        }
        self.other_panes
            .iter_mut()
            .find(|slot| slot.id == id)
            .map(|slot| &mut slot.buffer)
    }

    /// Todos os buffers (o do foco primeiro).
    pub fn pane_buffers_mut(&mut self) -> impl Iterator<Item = &mut TerminalBuffer> {
        self.buffer
            .iter_mut()
            .chain(self.other_panes.iter_mut().map(|slot| &mut slot.buffer))
    }

    /// Identificadores de todos os painéis (o do foco primeiro).
    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.buffer
            .iter()
            .map(|_| self.focused_pane)
            .chain(self.other_panes.iter().map(|slot| slot.id))
            .collect()
    }

    /// PIDs dos processos principais de cada painel.
    pub fn pane_pids(&self) -> Vec<u32> {
        self.buffer
            .iter()
            .chain(self.other_panes.iter().map(|slot| &slot.buffer))
            .filter_map(|buffer| buffer.pty().pid())
            .collect()
    }

    /// Tamanho (colunas, linhas) que um painel novo teria ao dividir o painel
    /// em foco. O PTY já nasce nesse tamanho: programas como `ls` leem a
    /// largura uma vez, logo ao começar.
    pub fn new_pane_size(&self, direction: SplitDirection, area: CellRect) -> (u16, u16) {
        const PREVIEW: PaneId = PaneId::MAX;
        let mut layout = self.layout.clone();
        if layout.is_empty() || !layout.split(self.focused_pane, PREVIEW, direction) {
            return (area.cols.max(1), area.rows.max(1));
        }
        layout
            .content_rects(area)
            .into_iter()
            .find(|(id, _)| *id == PREVIEW)
            .map(|(_, rect)| (rect.cols.max(1), rect.rows.max(1)))
            .unwrap_or((area.cols.max(1), area.rows.max(1)))
    }

    /// Divide o painel em foco; o novo painel (com `buffer`) recebe o foco.
    pub fn split(&mut self, direction: SplitDirection, buffer: TerminalBuffer) -> PaneId {
        self.split_with_id(allocate_pane_id(), direction, buffer)
    }

    /// Como [`Tab::split`], com o id do painel novo já escolhido.
    pub fn split_with_id(
        &mut self,
        new: PaneId,
        direction: SplitDirection,
        buffer: TerminalBuffer,
    ) -> PaneId {
        if self.layout.is_empty() || self.buffer.is_none() {
            self.layout = PaneLayout::single(new);
        } else {
            self.layout.split(self.focused_pane, new, direction);
        }
        if let Some(previous) = self.buffer.replace(buffer) {
            self.other_panes.push(PaneSlot {
                id: self.focused_pane,
                buffer: previous,
            });
        }
        self.focused_pane = new;
        new
    }

    /// Põe o foco em `id`. Devolve `false` se o painel não existe.
    pub fn focus_pane(&mut self, id: PaneId) -> bool {
        if id == self.focused_pane {
            return true;
        }
        let Some(index) = self.other_panes.iter().position(|slot| slot.id == id) else {
            return false;
        };
        let slot = self.other_panes.swap_remove(index);
        if let Some(previous) = self.buffer.replace(slot.buffer) {
            self.other_panes.push(PaneSlot {
                id: self.focused_pane,
                buffer: previous,
            });
        }
        self.focused_pane = id;
        true
    }

    /// Move o foco para o painel vizinho na direção pedida.
    pub fn focus_direction(&mut self, direction: FocusDirection, area: CellRect) -> bool {
        match self.layout.neighbor(self.focused_pane, direction, area) {
            Some(id) => self.focus_pane(id),
            None => false,
        }
    }

    /// Foca o próximo (ou anterior) painel na ordem da árvore.
    pub fn cycle_pane(&mut self, forward: bool, area: CellRect) -> bool {
        let ids: Vec<PaneId> = self
            .layout
            .rects(area)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        let Some(pos) = ids.iter().position(|&id| id == self.focused_pane) else {
            return false;
        };
        if ids.len() < 2 {
            return false;
        }
        let next = if forward {
            (pos + 1) % ids.len()
        } else {
            (pos + ids.len() - 1) % ids.len()
        };
        self.focus_pane(ids[next])
    }

    /// Remove um painel (o PTY é encerrado). Se era o do foco, o foco vai
    /// para outro painel restante. Devolve `true` se a tab ficou vazia.
    pub fn remove_pane(&mut self, id: PaneId) -> bool {
        self.layout.remove(id);
        if id == self.focused_pane {
            self.buffer = None;
            if let Some(slot) = self.other_panes.pop() {
                self.buffer = Some(slot.buffer);
                self.focused_pane = slot.id;
            }
        } else {
            self.other_panes.retain(|slot| slot.id != id);
        }
        self.buffer.is_none()
    }

    /// Remove os painéis cujo processo terminou. Devolve `true` se a tab ficou vazia.
    pub fn remove_exited_panes(&mut self) -> bool {
        let mut exited: Vec<PaneId> = self
            .other_panes
            .iter()
            .filter(|slot| !slot.buffer.pty().is_running())
            .map(|slot| slot.id)
            .collect();
        if self.buffer.as_ref().is_some_and(|b| !b.pty().is_running()) {
            exited.push(self.focused_pane);
        }
        let mut empty = self.buffer.is_none();
        for id in exited {
            empty = self.remove_pane(id);
        }
        empty
    }

    /// Ajusta o tamanho (PTY e grid) de cada painel à sua área.
    pub fn resize_panes(&mut self, area: CellRect) {
        for (id, rect) in self.layout.content_rects(area) {
            if rect.cols == 0 || rect.rows == 0 {
                continue;
            }
            if let Some(buffer) = self.pane_buffer_mut(id) {
                if buffer.content_size() != (rect.cols, rect.rows) {
                    if let Err(e) = buffer.resize(rect.cols, rect.rows) {
                        tracing::warn!(error = %e, pane = id, "Failed to resize pane");
                    }
                }
            }
        }
    }

    /// Aumenta (ou diminui) o painel em foco na direção da divisão mais próxima.
    pub fn resize_focused(&mut self, grow: bool) -> bool {
        let delta = if grow { 0.05 } else { -0.05 };
        let id = self.focused_pane;
        self.layout.resize(id, SplitDirection::Horizontal, delta)
            || self.layout.resize(id, SplitDirection::Vertical, delta)
    }

    pub fn set_active(&mut self, active: bool) {
        self.state = if active {
            TabState::Active
        } else {
            TabState::Inactive
        };
    }

    pub fn is_active(&self) -> bool {
        self.state == TabState::Active
    }

    /// Retorna referência ao grid (se buffer existir)
    pub fn grid(&self) -> Option<&Grid> {
        self.buffer.as_ref().map(|b| b.grid())
    }

    /// Retorna referência mutável ao grid (se buffer existir)
    pub fn grid_mut(&mut self) -> Option<&mut Grid> {
        self.buffer.as_mut().map(|b| b.grid_mut())
    }

    /// Retorna o diretório de trabalho atual
    pub fn working_dir(&self) -> Option<&str> {
        self.title_manager.working_dir()
    }

    /// Retorna o gerenciador de título
    pub fn title_manager(&self) -> &TabTitle {
        &self.title_manager
    }

    /// Verifica se tem processo rodando
    pub fn has_running_process(&self) -> bool {
        self.buffer
            .as_ref()
            .map(|buffer| buffer.pty().is_running())
            .unwrap_or(false)
    }

    /// Programa em primeiro plano em algum painel (além do shell parado).
    pub fn busy_process(&self) -> Option<String> {
        self.buffer
            .iter()
            .chain(self.other_panes.iter().map(|slot| &slot.buffer))
            .find_map(|buffer| buffer.pty().foreground_process())
    }
}

/// Resultado da tentativa de fechar uma tab
#[derive(Debug, Clone, PartialEq)]
pub enum TabCloseResult {
    /// Tab fechada com sucesso
    Closed,
    /// Todas as tabs foram fechadas
    AllTabsClosed,
    /// Não há tabs para fechar
    NoTabs,
    /// Precisa de confirmação do usuário (tem processo rodando)
    NeedsConfirmation(usize),
}

#[derive(Debug)]
pub struct TabManager {
    tabs: Vec<Tab>,
    active_tab_index: usize,
    search_bar: SearchBar,
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active_tab_index: 0,
            search_bar: SearchBar::new(),
        }
    }

    /// Cria uma nova tab vazia
    pub fn create_tab(&mut self, shell: &str) -> TabId {
        let mut tab = Tab::new(shell);
        let id = tab.id;

        if self.tabs.is_empty() {
            tab.set_active(true);
            self.active_tab_index = 0;
        }

        self.tabs.push(tab);
        id
    }

    /// Cria uma nova tab com buffer
    pub fn create_tab_with_buffer(&mut self, shell: &str, buffer: TerminalBuffer) -> TabId {
        self.create_tab_with_pane(shell, buffer, allocate_pane_id())
    }

    /// Acrescenta uma tab já montada.
    pub fn add_tab(&mut self, mut tab: Tab) -> TabId {
        let id = tab.id;
        if self.tabs.is_empty() {
            tab.set_active(true);
            self.active_tab_index = 0;
        }
        self.tabs.push(tab);
        id
    }

    /// Cria uma tab cujo primeiro painel tem o id `pane`.
    pub fn create_tab_with_pane(
        &mut self,
        shell: &str,
        buffer: TerminalBuffer,
        pane: PaneId,
    ) -> TabId {
        let mut tab = Tab::with_pane(shell, buffer, pane);
        let id = tab.id;

        if self.tabs.is_empty() {
            tab.set_active(true);
            self.active_tab_index = 0;
        }

        self.tabs.push(tab);
        id
    }

    pub fn close_tab(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }

        // Cleanup: remove o buffer da tab (Drop será chamado)
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.state = TabState::Closing;
            tab.buffer = None; // Isso faz com que o PTY seja dropped e cleanup seja feito
        }

        // Fechar a tab de cima de um grupo não fecha as de dentro: elas sobem
        // um nível e ficam no mesmo lugar.
        let closed = self.tabs[index].id;
        for tab in &mut self.tabs {
            if tab.parent == Some(closed) {
                tab.parent = None;
            }
        }
        self.tabs.remove(index);

        if self.tabs.is_empty() {
            self.active_tab_index = 0;
            return true;
        }

        let new_active = if index == self.active_tab_index {
            index.saturating_sub(1).min(self.tabs.len() - 1)
        } else if index < self.active_tab_index {
            self.active_tab_index - 1
        } else {
            self.active_tab_index.min(self.tabs.len() - 1)
        };

        self.set_active_tab(new_active);

        true
    }

    /// Verifica se a tab tem processo rodando
    pub fn has_running_process(&self, index: usize) -> bool {
        if let Some(tab) = self.tabs.get(index) {
            return tab.has_running_process();
        }
        false
    }

    /// Verifica se a tab ativa tem processo rodando
    pub fn active_tab_has_running_process(&self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        self.has_running_process(self.active_tab_index)
    }

    /// Fecha a tab ativa com verificação de processo rodando
    /// Retorna true se a tab foi fechada, false se precisa de confirmação
    pub fn try_close_active_tab(&mut self) -> TabCloseResult {
        if self.tabs.is_empty() {
            return TabCloseResult::NoTabs;
        }

        // Só pede confirmação se fechar mataria um programa em primeiro plano.
        if self.tabs[self.active_tab_index].busy_process().is_some() {
            return TabCloseResult::NeedsConfirmation(self.active_tab_index);
        }

        self.close_tab(self.active_tab_index);

        if self.tabs.is_empty() {
            TabCloseResult::AllTabsClosed
        } else {
            TabCloseResult::Closed
        }
    }

    /// Fecha uma tab específica com verificação de processo rodando
    pub fn try_close_tab(&mut self, index: usize) -> TabCloseResult {
        if index >= self.tabs.len() {
            return TabCloseResult::NoTabs;
        }

        // Só pede confirmação se fechar mataria um programa (vim, build...);
        // o shell parado sempre "está rodando" e não conta.
        if self.tabs[index].busy_process().is_some() {
            return TabCloseResult::NeedsConfirmation(index);
        }

        self.close_tab(index);

        if self.tabs.is_empty() {
            TabCloseResult::AllTabsClosed
        } else {
            TabCloseResult::Closed
        }
    }

    pub fn close_active_tab(&mut self) -> bool {
        if self.tabs.is_empty() {
            return false;
        }
        self.close_tab(self.active_tab_index)
    }

    pub fn switch_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.set_active_tab(index);
        }
    }

    /// Próxima tab na ordem da sidebar (pula as de grupos recolhidos).
    pub fn next_tab(&mut self) {
        if let Some(next) = self.cycle_visible(true) {
            self.set_active_tab(next);
        }
    }

    /// Tab anterior na ordem da sidebar (pula as de grupos recolhidos).
    pub fn previous_tab(&mut self) {
        if let Some(prev) = self.cycle_visible(false) {
            self.set_active_tab(prev);
        }
    }

    /// Ativa a tab. Se ela estiver dentro de um grupo recolhido, o grupo abre
    /// (senão a tab ativa sumiria da sidebar).
    pub fn set_active_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.expand_group_of(index);

        for (i, tab) in self.tabs.iter_mut().enumerate() {
            tab.set_active(i == index);
        }

        self.active_tab_index = index;
    }

    pub fn get_active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab_index)
    }

    pub fn get_active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active_tab_index)
    }

    pub fn get_tab(&self, index: usize) -> Option<&Tab> {
        self.tabs.get(index)
    }

    pub fn get_tab_mut(&mut self, index: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(index)
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_tab_index(&self) -> usize {
        self.active_tab_index
    }

    /// Atualiza o título de uma tab (define processo atual)
    pub fn update_title(&mut self, index: usize, process: &str) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.set_process(process);
        }
    }

    /// Atualiza o diretório de trabalho de uma tab
    pub fn update_working_dir(&mut self, index: usize, dir: &str) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.set_working_dir(dir);
        }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn tabs_mut(&mut self) -> &mut [Tab] {
        &mut self.tabs
    }

    pub fn search_bar(&self) -> &SearchBar {
        &self.search_bar
    }

    pub fn search_bar_mut(&mut self) -> &mut SearchBar {
        &mut self.search_bar
    }

    pub fn toggle_search(&mut self) {
        if self.search_bar.is_visible() {
            self.search_bar.hide();
        } else {
            self.search_bar.show();
        }
    }

    pub fn start_search(&mut self) {
        self.search_bar.show();
        self.search_bar.activate();
    }

    pub fn stop_search(&mut self) {
        self.search_bar.hide();

        if let Some(ref mut active_tab) = self.get_active_tab_mut() {
            if let Some(ref mut grid) = active_tab.grid_mut() {
                grid.clear_highlights();
            }
        }
    }

    /// Busca no histórico e na tela da tab ativa (#34). As linhas usam a
    /// numeração do grid: 0..len é o histórico (a mais antiga primeiro) e
    /// len.. é a tela. O histórico é aberto uma linha por vez num buffer só,
    /// sem expandir tudo; só os resultados ficam guardados. O resultado
    /// atual começa no primeiro que aparece na tela (rolagem atual).
    pub fn update_search_query(&mut self, query: &str) {
        self.search_bar.query = query.to_string();
        if let Some(grid) = self.get_active_tab().and_then(|tab| tab.grid()) {
            let history = grid.scrollback_lines();
            let mut row = Vec::new();
            let history_lines = (0..history.len()).map(|index| {
                history.decode_into(index, &mut row);
                row_text(&row)
            });
            let screen_lines = grid.get_cells().iter().map(|cells| row_text(cells));
            let mut result = self
                .search_bar
                .search_in_line_iter(history_lines.chain(screen_lines));
            let top = grid.scrollback_len().saturating_sub(grid.scroll_offset());
            result.current_match_index = result
                .matches
                .iter()
                .position(|m| m.line >= top)
                .unwrap_or_else(|| result.matches.len().saturating_sub(1));
            self.search_bar.result = Some(result);
        }

        self.apply_search_highlights();
    }

    pub fn apply_search_highlights(&mut self) {
        let matches: Vec<SearchMatch> = self
            .search_bar
            .result
            .as_ref()
            .map(|r| r.matches.clone())
            .unwrap_or_default();

        if let Some(ref mut active_tab) = self.get_active_tab_mut() {
            if let Some(ref mut grid) = active_tab.grid_mut() {
                if !matches.is_empty() {
                    grid.apply_search_highlights(&matches);
                } else {
                    grid.clear_highlights();
                }
            }
        }
    }

    pub fn next_search_match(&mut self) -> bool {
        let target_line: Option<usize> = {
            if let Some(ref mut result) = self.search_bar.result.as_mut() {
                let has_next = result.next_match();
                if has_next {
                    result.current_match().map(|m| m.line)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(line) = target_line {
            if let Some(grid) = self.get_active_tab_mut().and_then(|tab| tab.grid_mut()) {
                reveal_line(grid, line);
            }
            true
        } else {
            false
        }
    }

    pub fn previous_search_match(&mut self) -> bool {
        let target_line: Option<usize> = {
            if let Some(ref mut result) = self.search_bar.result.as_mut() {
                let has_prev = result.previous_match();
                if has_prev {
                    result.current_match().map(|m| m.line)
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(line) = target_line {
            if let Some(grid) = self.get_active_tab_mut().and_then(|tab| tab.grid_mut()) {
                reveal_line(grid, line);
            }
            true
        } else {
            false
        }
    }

    pub fn get_current_match_position(&self) -> Option<(usize, usize, usize)> {
        self.search_bar.get_current_match_position()
    }

    pub fn is_search_visible(&self) -> bool {
        self.search_bar.is_visible()
    }

    pub fn is_search_active(&self) -> bool {
        self.search_bar.is_active()
    }

    pub fn get_search_status(&self) -> String {
        self.search_bar.get_status_text()
    }

    pub fn toggle_case_sensitive(&mut self) {
        self.search_bar.toggle_case_sensitive();
    }

    pub fn toggle_whole_word(&mut self) {
        self.search_bar.toggle_whole_word();
    }

    pub fn toggle_regex(&mut self) {
        self.search_bar.toggle_regex();
    }

    pub fn get_search_query(&self) -> String {
        self.search_bar.query.clone()
    }
}

impl Default for TabManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Texto de uma linha de células, sem os espaços do fim (a busca conta
/// colunas por caractere, como o grid).
fn row_text(cells: &[crate::terminal::Cell]) -> String {
    let line: String = cells
        .iter()
        .map(|cell| if cell.char == '\0' { ' ' } else { cell.char })
        .collect();
    line.trim_end().to_string()
}

/// Rola até a linha `line` (numeração do grid) se ela não está na tela:
/// linha do histórico fica no meio da tela; linha da tela volta ao fim.
fn reveal_line(grid: &mut Grid, line: usize) {
    let history = grid.scrollback_len();
    let top = history.saturating_sub(grid.scroll_offset());
    if (top..top + grid.rows()).contains(&line) {
        return;
    }
    if line >= history {
        grid.scroll_to_bottom();
    } else {
        grid.scroll_to_line(line.saturating_sub(grid.rows() / 2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_tab() {
        let mut manager = TabManager::new();
        let id = manager.create_tab("bash");
        assert_eq!(manager.tab_count(), 1);
        assert_eq!(manager.get_active_tab().unwrap().id, id);
    }

    #[test]
    fn test_switch_tab() {
        let mut manager = TabManager::new();
        manager.create_tab("bash");
        manager.create_tab("bash");

        manager.switch_tab(1);
        assert_eq!(manager.active_tab_index(), 1);
        assert!(manager.get_active_tab().unwrap().is_active());
    }

    #[test]
    fn test_close_tab() {
        let mut manager = TabManager::new();
        manager.create_tab("bash");
        manager.create_tab("bash");

        assert!(manager.close_tab(0));
        assert_eq!(manager.tab_count(), 1);
    }

    #[test]
    fn test_close_tab_before_active_preserves_active_tab() {
        let mut manager = TabManager::new();
        let _id1 = manager.create_tab("bash");
        let _id2 = manager.create_tab("bash");
        let id3 = manager.create_tab("bash");
        let id4 = manager.create_tab("bash");

        manager.switch_tab(2);
        assert!(manager.close_tab(0));

        assert_eq!(manager.active_tab_index(), 1);
        assert_eq!(manager.get_active_tab().unwrap().id, id3);
        assert!(manager.get_active_tab().unwrap().is_active());
        assert!(!manager.get_tab(0).unwrap().is_active());
        assert!(!manager.get_tab(2).unwrap().is_active());
        assert_eq!(manager.get_tab(2).unwrap().id, id4);
    }

    #[test]
    fn test_direct_tab_constructors_share_id_sequence() {
        let first = Tab::new("bash");
        let second = Tab::new("bash");
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn test_next_previous_tab() {
        let mut manager = TabManager::new();
        manager.create_tab("bash");
        manager.create_tab("bash");
        manager.create_tab("bash");

        manager.next_tab();
        assert_eq!(manager.active_tab_index(), 1);

        manager.next_tab();
        assert_eq!(manager.active_tab_index(), 2);

        manager.next_tab(); // Wrap around
        assert_eq!(manager.active_tab_index(), 0);

        manager.previous_tab(); // Wrap around backwards
        assert_eq!(manager.active_tab_index(), 2);
    }

    #[test]
    fn test_multiple_tabs() {
        let mut manager = TabManager::new();

        let id1 = manager.create_tab("bash");
        let id2 = manager.create_tab("bash");
        let id3 = manager.create_tab("bash");

        assert_eq!(manager.tab_count(), 3);
        assert_eq!(manager.get_tab(0).unwrap().id, id1);
        assert_eq!(manager.get_tab(1).unwrap().id, id2);
        assert_eq!(manager.get_tab(2).unwrap().id, id3);
    }

    fn manager_with_text(text: &[u8]) -> TabManager {
        let mut tm = TabManager::default();
        let buffer = TerminalBuffer::new(30, 6).unwrap();
        tm.create_tab_with_buffer("bash", buffer);
        let grid = tm.get_active_tab_mut().unwrap().grid_mut().unwrap();
        crate::terminal::parser::Parser::new(grid).process(text);
        tm
    }

    fn highlighted(tm: &TabManager) -> usize {
        tm.get_active_tab()
            .unwrap()
            .grid()
            .unwrap()
            .visible_lines()
            .iter()
            .flat_map(|row| row.iter())
            .filter(|c| c.highlight)
            .count()
    }

    #[test]
    fn test_search_finds_and_highlights_matches() {
        let mut tm = manager_with_text(b"erro aqui\r\nnada\r\noutro erro\r\n");
        assert!(!tm.is_search_visible());
        tm.start_search();
        assert!(tm.is_search_visible() && tm.is_search_active());
        assert_eq!(tm.get_search_status(), "Search");

        tm.update_search_query("erro");
        assert_eq!(tm.get_search_query(), "erro");
        assert_eq!(tm.get_search_status(), "1/2 matches for \"erro\"");
        assert_eq!(tm.get_current_match_position(), Some((0, 0, 4)));
        assert_eq!(highlighted(&tm), 8);

        assert!(tm.next_search_match());
        assert_eq!(tm.get_current_match_position().unwrap().0, 2);
        assert!(tm.previous_search_match());
        assert_eq!(tm.get_current_match_position().unwrap().0, 0);

        // sem resultados os destaques somem
        tm.update_search_query("zzz");
        assert_eq!(tm.get_search_status(), "No results for \"zzz\"");
        assert_eq!(highlighted(&tm), 0);

        tm.update_search_query("erro");
        assert!(highlighted(&tm) > 0);
        tm.stop_search();
        assert!(!tm.is_search_visible());
        assert_eq!(highlighted(&tm), 0);

        tm.toggle_search();
        assert!(tm.is_search_visible());
        tm.toggle_search();
        assert!(!tm.is_search_visible());

        tm.toggle_case_sensitive();
        tm.toggle_whole_word();
        tm.toggle_regex();
        let bar = tm.search_bar();
        assert!(bar.options.case_sensitive && bar.options.whole_word && bar.options.regex);
        tm.search_bar_mut().options.regex = false;
        assert!(!tm.search_bar().options.regex);
    }

    #[test]
    fn test_search_match_lines_follow_grid_rows_with_blank_lines() {
        // Regressão: linhas vazias eram descartadas antes da busca e o match
        // da linha 2 virava linha 1 (destaque na linha em branco)
        let mut tm = manager_with_text(b"a\r\n\r\nerro\r\n");
        tm.update_search_query("erro");
        assert_eq!(tm.get_current_match_position(), Some((2, 0, 4)));
        let grid = tm.get_active_tab().unwrap().grid().unwrap();
        let rows = grid.visible_lines();
        assert!(rows[2].iter().take(4).all(|c| c.highlight));
        assert!(rows[1].iter().all(|c| !c.highlight));
    }

    #[test]
    fn test_search_navigation_without_results_or_buffer() {
        let mut tm = TabManager::new();
        assert!(!tm.next_search_match());
        assert!(!tm.previous_search_match());
        assert!(tm.get_current_match_position().is_none());
        tm.create_tab("sh");
        // tab sem buffer: a busca não tem onde procurar
        tm.update_search_query("x");
        assert!(!tm.next_search_match());
        tm.stop_search();
        assert!(tm.get_active_tab_mut().unwrap().grid_mut().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_close_with_running_process_needs_confirmation() {
        // Um programa em primeiro plano (não o shell parado) pede confirmação.
        let mut tm = TabManager::default();
        let mut busy = TerminalBuffer::with_command(
            30,
            6,
            "/bin/bash",
            &["--norc".to_string(), "-i".to_string()],
            &Default::default(),
        )
        .unwrap();
        busy.write_str("sleep 30\n").unwrap();
        tm.create_tab_with_buffer("bash", busy);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while tm.get_tab(0).unwrap().busy_process().is_none()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(tm.active_tab_has_running_process());
        assert_eq!(
            tm.try_close_active_tab(),
            TabCloseResult::NeedsConfirmation(0)
        );
        assert_eq!(tm.try_close_tab(0), TabCloseResult::NeedsConfirmation(0));
        assert_eq!(tm.try_close_tab(5), TabCloseResult::NoTabs);
        assert!(!tm.has_running_process(5));

        tm.create_tab("zsh");
        // a tab sem PTY fecha direto
        assert_eq!(tm.try_close_tab(1), TabCloseResult::Closed);
        assert!(tm.close_active_tab());
        assert_eq!(tm.try_close_active_tab(), TabCloseResult::NoTabs);
        assert!(!tm.active_tab_has_running_process());

        tm.create_tab("a");
        assert_eq!(tm.try_close_tab(0), TabCloseResult::AllTabsClosed);
        tm.create_tab("a");
        assert_eq!(tm.try_close_active_tab(), TabCloseResult::AllTabsClosed);
        assert!(!tm.close_active_tab());
    }

    #[test]
    fn test_tab_title_helpers_and_debug() {
        let mut tab = Tab::new("bash");
        tab.set_working_dir("/srv/app");
        assert_eq!(tab.working_dir(), Some("/srv/app"));
        tab.set_process("vim");
        assert!(tab.title().contains("vim"));
        tab.clear_process();
        assert!(!tab.title().contains("vim"));
        assert_eq!(tab.title_manager().title(), tab.title());
        let dbg = format!("{:?}", tab);
        assert!(dbg.contains("has_buffer: false"));
        assert!(!tab.has_running_process());
    }
}
