//! Input Router - Roteamento de Input para Panes
//!
//! Sistema de roteamento de eventos de input (teclado, mouse, resize)
//! para o pane correto baseado em foco, posição, ou outros critérios.

use super::{PaneContent, PaneError, PaneId, PaneResult, PaneState};
use std::collections::HashMap;

/// Evento de input
#[derive(Debug, Clone)]
pub enum InputEvent {
    /// Evento de teclado
    Key {
        /// Dados brutos da tecla
        data: Vec<u8>,
        /// Código da tecla (opcional)
        key_code: Option<KeyCode>,
        /// Modificadores
        modifiers: KeyModifiers,
    },
    /// Evento de mouse
    Mouse {
        /// Tipo de evento de mouse
        kind: MouseEventKind,
        /// Posição (col, row) em células do terminal
        position: (u16, u16),
        /// Modificadores
        modifiers: KeyModifiers,
    },
    /// Redimensionamento
    Resize {
        /// Novo número de colunas
        cols: u16,
        /// Novo número de linhas  
        rows: u16,
    },
}

/// Código de tecla
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode {
    /// Caractere ASCII/Unicode
    Char(char),
    /// Tecla de função (F1-F24)
    F(u8),
    /// Tecla de seta
    Arrow(ArrowKey),
    /// Enter/Return
    Enter,
    /// Escape
    Escape,
    /// Backspace
    Backspace,
    /// Tab
    Tab,
    /// Delete
    Delete,
    /// Insert
    Insert,
    /// Home
    Home,
    /// End
    End,
    /// Page Up
    PageUp,
    /// Page Down
    PageDown,
}

/// Tecla de seta
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowKey {
    Up,
    Down,
    Left,
    Right,
}

/// Modificadores de teclado
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

/// Tipo de evento de mouse
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventKind {
    /// Botão pressionado
    Down(MouseButton),
    /// Botão liberado
    Up(MouseButton),
    /// Clique (down + up rápido)
    Click(MouseButton),
    /// Duplo clique
    DoubleClick(MouseButton),
    /// Scroll para cima
    ScrollUp,
    /// Scroll para baixo
    ScrollDown,
    /// Movimento do mouse (hover)
    Move,
}

/// Botão do mouse
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Button4,
    Button5,
}

/// Roteador de input
///
/// Responsável por direcionar eventos de input para o pane correto
/// baseado em diferentes estratégias (foco, posição, etc.)
pub struct InputRouter {
    /// Panes registrados (id -> pane)
    panes: HashMap<PaneId, Box<dyn PaneContent>>,
    /// Pane atualmente focado
    focused_pane: Option<PaneId>,
    /// Pane sob o mouse
    hover_pane: Option<PaneId>,
    /// Se o modo de broadcast está ativo (input vai para todos)
    broadcast_mode: bool,
    /// Se o modo de foco automático está ativo
    auto_focus: bool,
    /// Seleção ativa (para arrastar/selecionar)
    active_selection: Option<(PaneId, (u16, u16))>,
}

impl std::fmt::Debug for InputRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InputRouter")
            .field("pane_count", &self.panes.len())
            .field("focused_pane", &self.focused_pane)
            .field("hover_pane", &self.hover_pane)
            .field("broadcast_mode", &self.broadcast_mode)
            .field("auto_focus", &self.auto_focus)
            .finish_non_exhaustive()
    }
}

impl InputRouter {
    /// Cria um novo roteador
    pub fn new() -> Self {
        Self {
            panes: HashMap::new(),
            focused_pane: None,
            hover_pane: None,
            broadcast_mode: false,
            auto_focus: true,
            active_selection: None,
        }
    }

    /// Registra um pane no roteador
    pub fn register_pane(&mut self, pane: Box<dyn PaneContent>) -> PaneId {
        let id = pane.pane_id();
        self.panes.insert(id, pane);

        // Se for o primeiro pane, foca automaticamente (e marca como Active,
        // como em focus_pane)
        if self.focused_pane.is_none() {
            let _ = self.focus_pane(id);
        }

        id
    }

    /// Remove um pane do roteador
    pub fn unregister_pane(&mut self, id: PaneId) -> Option<Box<dyn PaneContent>> {
        let pane = self.panes.remove(&id);

        if self.focused_pane == Some(id) {
            // O novo foco precisa ficar Active (antes só o ID mudava e o pane
            // continuava em Background)
            self.focused_pane = None;
            if let Some(next) = self.panes.keys().next().copied() {
                let _ = self.focus_pane(next);
            }
        }

        if self.hover_pane == Some(id) {
            self.hover_pane = None;
        }

        pane
    }

    /// Retorna o número de panes registrados
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Foca um pane específico
    pub fn focus_pane(&mut self, id: PaneId) -> PaneResult<()> {
        if !self.panes.contains_key(&id) {
            return Err(PaneError::NotFound(id));
        }

        // Desfoca o pane anterior
        if let Some(old_id) = self.focused_pane {
            if let Some(pane) = self.panes.get_mut(&old_id) {
                pane.set_state(PaneState::Background);
            }
        }

        // Foca o novo pane
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.set_state(PaneState::Active);
        }

        self.focused_pane = Some(id);
        Ok(())
    }

    /// Retorna o ID do pane focado
    pub fn focused_pane(&self) -> Option<PaneId> {
        self.focused_pane
    }

    /// Foca o próximo pane
    pub fn focus_next(&mut self) {
        let mut ids: Vec<PaneId> = self.panes.keys().copied().collect();
        ids.sort_unstable();
        if ids.is_empty() {
            return;
        }

        let current_idx = self
            .focused_pane
            .and_then(|id| ids.iter().position(|&x| x == id))
            .unwrap_or(0);

        let next_idx = (current_idx + 1) % ids.len();
        let _ = self.focus_pane(ids[next_idx]);
    }

    /// Foca o pane anterior
    pub fn focus_previous(&mut self) {
        let mut ids: Vec<PaneId> = self.panes.keys().copied().collect();
        ids.sort_unstable();
        if ids.is_empty() {
            return;
        }

        let current_idx = self
            .focused_pane
            .and_then(|id| ids.iter().position(|&x| x == id))
            .unwrap_or(0);

        let prev_idx = if current_idx == 0 {
            ids.len() - 1
        } else {
            current_idx - 1
        };

        let _ = self.focus_pane(ids[prev_idx]);
    }

    /// Encontra o pane na posição especificada (em pixels)
    pub fn pane_at_position(&self, x: u32, y: u32) -> Option<PaneId> {
        self.panes
            .iter()
            .find(|(_, pane)| {
                let info = pane.render_info();
                let rect = info.rect();
                rect.contains(x, y)
            })
            .map(|(id, _)| *id)
    }

    /// Processa um evento de input
    pub fn process_input(&mut self, event: InputEvent) -> PaneResult<bool> {
        match event {
            InputEvent::Key { .. } => self.process_key_input(event),
            InputEvent::Mouse { position, kind, .. } => {
                self.process_mouse_input(event, position, kind)
            }
            InputEvent::Resize { cols, rows } => self.process_resize(cols, rows),
        }
    }

    /// Processa input de teclado
    fn process_key_input(&mut self, event: InputEvent) -> PaneResult<bool> {
        if self.broadcast_mode {
            // Envia para todos os panes ativos
            let mut consumed = false;
            for pane in self.panes.values_mut() {
                if (pane.state() == PaneState::Active || pane.state() == PaneState::Background)
                    && pane.process_input(&event)?
                {
                    consumed = true;
                }
            }
            Ok(consumed)
        } else if let Some(id) = self.focused_pane {
            // Envia apenas para o pane focado
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.process_input(&event)
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    /// Processa input de mouse
    fn process_mouse_input(
        &mut self,
        event: InputEvent,
        position: (u16, u16),
        kind: MouseEventKind,
    ) -> PaneResult<bool> {
        // Converte posição de células para pixels (aproximado)
        // TODO: Usar dimensões reais de célula
        let cell_width = 8;
        let cell_height = 16;
        let pixel_x = position.0 as u32 * cell_width;
        let pixel_y = position.1 as u32 * cell_height;

        // Encontra o pane na posição
        let target_pane = self.pane_at_position(pixel_x, pixel_y);

        // Atualiza hover
        if self.hover_pane != target_pane {
            self.hover_pane = target_pane;
        }

        // Auto-focus em clique
        if matches!(kind, MouseEventKind::Click(_) | MouseEventKind::Down(_)) && self.auto_focus {
            if let Some(id) = target_pane {
                self.focus_pane(id)?;
            }
        }

        // Seleção com mouse
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(id) = target_pane {
                    self.active_selection = Some((id, position));
                    // Limpa seleção anterior
                    if let Some(pane) = self.panes.get_mut(&id) {
                        pane.clear_selection();
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some((id, start)) = self.active_selection.take() {
                    if let Some(pane) = self.panes.get_mut(&id) {
                        pane.set_selection(start, position);
                    }
                }
            }
            MouseEventKind::Move => {
                if let Some((id, start)) = self.active_selection {
                    if let Some(pane) = self.panes.get_mut(&id) {
                        pane.set_selection(start, position);
                    }
                }
            }
            _ => {}
        }

        // Scroll em hover
        if matches!(kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
            if let Some(id) = target_pane.or(self.focused_pane) {
                if let Some(pane) = self.panes.get_mut(&id) {
                    let lines = 3; // Scroll padrão
                    match kind {
                        MouseEventKind::ScrollUp => pane.scroll_up(lines),
                        MouseEventKind::ScrollDown => pane.scroll_down(lines),
                        _ => {}
                    }
                    return Ok(true);
                }
            }
        }

        // Envia o evento para o pane alvo
        if let Some(id) = target_pane {
            if let Some(pane) = self.panes.get_mut(&id) {
                return pane.process_input(&event);
            }
        }

        Ok(false)
    }

    /// Processa redimensionamento
    fn process_resize(&mut self, cols: u16, rows: u16) -> PaneResult<bool> {
        // Redimensiona todos os panes proporcionalmente
        let count = self.panes.len() as u16;
        if count == 0 {
            return Ok(false);
        }

        // Divide o espaço igualmente (simplificado)
        let pane_cols = cols / count;
        let pane_rows = rows;

        for pane in self.panes.values_mut() {
            pane.resize(pane_cols, pane_rows)?;
        }

        Ok(true)
    }

    /// Atualiza todos os panes
    pub fn update_all(&mut self) -> PaneResult<()> {
        for pane in self.panes.values_mut() {
            pane.update()?;
        }
        Ok(())
    }

    /// Retorna referência a um pane
    pub fn get_pane(&self, id: PaneId) -> Option<&dyn PaneContent> {
        self.panes.get(&id).map(|b| b.as_ref())
    }

    /// Retorna referência mutável a um pane
    /// Itera sobre todos os panes
    pub fn iter_panes(&self) -> impl Iterator<Item = (PaneId, &dyn PaneContent)> {
        self.panes.iter().map(|(id, pane)| (*id, pane.as_ref()))
    }

    /// Habilita/desabilita modo de broadcast
    pub fn set_broadcast_mode(&mut self, enabled: bool) {
        self.broadcast_mode = enabled;
    }

    /// Verifica se está em modo de broadcast
    pub fn is_broadcast_mode(&self) -> bool {
        self.broadcast_mode
    }

    /// Habilita/desabilita auto-focus
    pub fn set_auto_focus(&mut self, enabled: bool) {
        self.auto_focus = enabled;
    }

    /// Alterna o modo de broadcast
    pub fn toggle_broadcast(&mut self) {
        self.broadcast_mode = !self.broadcast_mode;
    }

    /// Envia input para um pane específico (bypass do roteamento)
    pub fn send_to_pane(&mut self, id: PaneId, event: InputEvent) -> PaneResult<bool> {
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.process_input(&event)
        } else {
            Err(PaneError::NotFound(id))
        }
    }

    /// Retorna os IDs de todos os panes
    pub fn pane_ids(&self) -> Vec<PaneId> {
        self.panes.keys().copied().collect()
    }

    /// Limpa todos os panes
    pub fn clear(&mut self) {
        self.panes.clear();
        self.focused_pane = None;
        self.hover_pane = None;
        self.active_selection = None;
    }
}

impl Default for InputRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder para configuração do InputRouter
pub struct InputRouterBuilder {
    router: InputRouter,
}

impl InputRouterBuilder {
    pub fn new() -> Self {
        Self {
            router: InputRouter::new(),
        }
    }

    pub fn with_auto_focus(mut self, enabled: bool) -> Self {
        self.router.auto_focus = enabled;
        self
    }

    pub fn with_broadcast_mode(mut self, enabled: bool) -> Self {
        self.router.broadcast_mode = enabled;
        self
    }

    pub fn build(self) -> InputRouter {
        self.router
    }
}

impl Default for InputRouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::terminal_pane::TerminalPane;

    #[test]
    fn test_input_router_creation() {
        let router = InputRouter::new();
        assert_eq!(router.pane_count(), 0);
        assert!(router.focused_pane().is_none());
    }

    #[test]
    fn test_register_and_focus_pane() {
        let mut router = InputRouter::new();

        let pane = TerminalPane::new(80, 24).unwrap();
        let id = pane.pane_id();

        router.register_pane(Box::new(pane));
        assert_eq!(router.pane_count(), 1);

        // Deve focar automaticamente
        assert_eq!(router.focused_pane(), Some(id));
    }

    #[test]
    fn test_focus_next_previous() {
        let mut router = InputRouter::new();

        let pane1 = TerminalPane::new(80, 24).unwrap();
        let pane2 = TerminalPane::new(80, 24).unwrap();
        let pane3 = TerminalPane::new(80, 24).unwrap();

        let id1 = pane1.pane_id();
        let id2 = pane2.pane_id();

        router.register_pane(Box::new(pane1));
        router.register_pane(Box::new(pane2));
        router.register_pane(Box::new(pane3));

        assert_eq!(router.focused_pane(), Some(id1));

        router.focus_next();
        assert_eq!(router.focused_pane(), Some(id2));

        router.focus_previous();
        assert_eq!(router.focused_pane(), Some(id1));
    }

    #[test]
    fn test_broadcast_mode() {
        let mut router = InputRouter::new();

        assert!(!router.is_broadcast_mode());

        router.toggle_broadcast();
        assert!(router.is_broadcast_mode());

        router.set_broadcast_mode(false);
        assert!(!router.is_broadcast_mode());
    }

    #[test]
    fn test_key_modifiers() {
        let mods = KeyModifiers {
            ctrl: true,
            alt: false,
            shift: true,
            meta: false,
        };

        assert!(mods.ctrl);
        assert!(!mods.alt);
        assert!(mods.shift);
    }

    #[test]
    fn test_mouse_event_kind() {
        let kind = MouseEventKind::Click(MouseButton::Left);
        assert!(matches!(kind, MouseEventKind::Click(MouseButton::Left)));
    }

    use crate::pane::test_support::{FakeLog, FakePane};
    use std::sync::{Arc, Mutex};

    fn key(c: char) -> InputEvent {
        InputEvent::Key {
            data: vec![c as u8],
            key_code: Some(KeyCode::Char(c)),
            modifiers: KeyModifiers::default(),
        }
    }

    fn mouse(kind: MouseEventKind, col: u16, row: u16) -> InputEvent {
        InputEvent::Mouse {
            kind,
            position: (col, row),
            modifiers: KeyModifiers::default(),
        }
    }

    /// Dois panes lado a lado: esquerdo em x 0..400, direito em x 400..800
    /// (células de 8x16 px: colunas 0..49 e 50..99).
    fn two_panes() -> (
        InputRouter,
        PaneId,
        PaneId,
        Arc<Mutex<FakeLog>>,
        Arc<Mutex<FakeLog>>,
    ) {
        let mut router = InputRouter::default();
        let left = FakePane::at(0, 0, 400, 400);
        let right = FakePane::at(400, 0, 400, 400);
        let (lid, rid) = (left.id, right.id);
        let (llog, rlog) = (left.log.clone(), right.log.clone());
        router.register_pane(Box::new(left));
        router.register_pane(Box::new(right));
        (router, lid, rid, llog, rlog)
    }

    #[test]
    fn test_keys_go_to_focused_pane_or_broadcast() {
        let (mut router, lid, rid, llog, rlog) = two_panes();
        assert_eq!(router.focused_pane(), Some(lid));
        // o primeiro pane registrado já fica Active
        assert_eq!(router.get_pane(lid).unwrap().state(), PaneState::Active);
        assert_eq!(router.get_pane(rid).unwrap().state(), PaneState::Background);
        assert!(router.process_input(key('a')).unwrap());
        assert_eq!(llog.lock().unwrap().inputs.len(), 1);
        assert!(rlog.lock().unwrap().inputs.is_empty());

        router.focus_pane(rid).unwrap();
        assert_eq!(router.get_pane(rid).unwrap().state(), PaneState::Active);
        assert_eq!(router.get_pane(lid).unwrap().state(), PaneState::Background);
        router.process_input(key('b')).unwrap();
        assert_eq!(rlog.lock().unwrap().inputs.len(), 1);

        router.set_broadcast_mode(true);
        assert!(router.process_input(key('c')).unwrap());
        assert_eq!(llog.lock().unwrap().inputs.len(), 2);
        assert_eq!(rlog.lock().unwrap().inputs.len(), 2);

        assert!(matches!(
            router.focus_pane(9_999_999),
            Err(PaneError::NotFound(_))
        ));
        assert!(router.send_to_pane(lid, key('d')).unwrap());
        assert!(matches!(
            router.send_to_pane(9_999_999, key('d')),
            Err(PaneError::NotFound(_))
        ));
    }

    #[test]
    fn test_key_without_focus_is_not_consumed() {
        let mut router = InputRouter::new();
        assert!(!router.process_input(key('x')).unwrap());
        let mut lonely = InputRouterBuilder::new().with_broadcast_mode(true).build();
        assert!(!lonely.process_input(key('x')).unwrap());
    }

    #[test]
    fn test_mouse_click_focuses_and_selects() {
        let (mut router, lid, rid, llog, rlog) = two_panes();
        // clique no pane direito (coluna 60 -> x = 480)
        router
            .process_input(mouse(MouseEventKind::Down(MouseButton::Left), 60, 2))
            .unwrap();
        assert_eq!(router.focused_pane(), Some(rid));
        assert_eq!(rlog.lock().unwrap().cleared, 1);
        router
            .process_input(mouse(MouseEventKind::Move, 65, 3))
            .unwrap();
        assert_eq!(rlog.lock().unwrap().selection, Some(((60, 2), (65, 3))));
        router
            .process_input(mouse(MouseEventKind::Up(MouseButton::Left), 70, 4))
            .unwrap();
        assert_eq!(rlog.lock().unwrap().selection, Some(((60, 2), (70, 4))));
        // depois do Up, mover não altera a seleção
        router
            .process_input(mouse(MouseEventKind::Move, 1, 1))
            .unwrap();
        assert_eq!(rlog.lock().unwrap().selection, Some(((60, 2), (70, 4))));
        assert!(llog.lock().unwrap().selection.is_none());

        // sem auto-focus o clique não troca o foco
        router.set_auto_focus(false);
        router
            .process_input(mouse(MouseEventKind::Click(MouseButton::Left), 1, 1))
            .unwrap();
        assert_eq!(router.focused_pane(), Some(rid));
        assert_eq!(router.pane_at_position(10, 10), Some(lid));
        assert_eq!(router.pane_at_position(5000, 10), None);
    }

    #[test]
    fn test_mouse_scroll_uses_hover_or_focus() {
        let (mut router, _lid, _rid, llog, rlog) = two_panes();
        assert!(router
            .process_input(mouse(MouseEventKind::ScrollUp, 60, 1))
            .unwrap());
        assert_eq!(rlog.lock().unwrap().scroll, 3);
        // fora de qualquer pane: usa o focado (esquerdo)
        assert!(router
            .process_input(mouse(MouseEventKind::ScrollDown, 500, 500))
            .unwrap());
        assert_eq!(llog.lock().unwrap().scroll, -3);
        // clique fora: nada consome
        assert!(!router
            .process_input(mouse(MouseEventKind::Down(MouseButton::Right), 500, 500))
            .unwrap());
    }

    #[test]
    fn test_resize_update_unregister_and_clear() {
        let (mut router, lid, rid, llog, rlog) = two_panes();
        assert!(router
            .process_input(InputEvent::Resize {
                cols: 100,
                rows: 30
            })
            .unwrap());
        assert_eq!(llog.lock().unwrap().resized, vec![(50, 30)]);
        assert_eq!(rlog.lock().unwrap().resized, vec![(50, 30)]);
        router.update_all().unwrap();
        assert_eq!(llog.lock().unwrap().updates, 1);
        let mut ids = router.pane_ids();
        ids.sort();
        assert_eq!(ids, {
            let mut v = vec![lid, rid];
            v.sort();
            v
        });
        assert_eq!(router.iter_panes().count(), 2);
        assert!(format!("{:?}", router).contains("pane_count: 2"));

        // hover no direito, depois remove o focado (esquerdo)
        router
            .process_input(mouse(MouseEventKind::Move, 60, 1))
            .unwrap();
        let removed = router.unregister_pane(lid).unwrap();
        assert_eq!(removed.pane_id(), lid);
        // Regressão: o novo foco fica Active
        assert_eq!(router.focused_pane(), Some(rid));
        assert_eq!(router.get_pane(rid).unwrap().state(), PaneState::Active);
        assert!(router.unregister_pane(rid).is_some());
        assert_eq!(router.focused_pane(), None);
        assert!(router.unregister_pane(rid).is_none());

        let (mut router, ..) = two_panes();
        router.clear();
        assert_eq!(router.pane_count(), 0);
        assert!(router.focused_pane().is_none());
        assert!(!router
            .process_input(InputEvent::Resize { cols: 1, rows: 1 })
            .unwrap());
        router.focus_next();
        router.focus_previous();
        assert!(router.focused_pane().is_none());
    }

    #[test]
    fn test_failing_pane_errors_propagate() {
        let mut router = InputRouterBuilder::default().with_auto_focus(false).build();
        let mut pane = FakePane::at(0, 0, 100, 100);
        pane.fail = true;
        let id = router.register_pane(Box::new(pane));
        assert!(router.process_input(key('x')).is_err());
        assert!(router.update_all().is_err());
        assert!(router
            .process_input(InputEvent::Resize { cols: 10, rows: 10 })
            .is_err());
        assert!(router.send_to_pane(id, key('x')).is_err());
    }

    #[test]
    fn test_focus_previous_wraps() {
        let (mut router, lid, rid, ..) = two_panes();
        let (first, last) = if lid < rid { (lid, rid) } else { (rid, lid) };
        router.focus_pane(first).unwrap();
        router.focus_previous();
        assert_eq!(router.focused_pane(), Some(last));
        router.focus_next();
        assert_eq!(router.focused_pane(), Some(first));
    }
}
