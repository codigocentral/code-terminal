//! TerminalPane - Implementação completa de Pane para terminal
//!
//! Este módulo fornece uma implementação completa do trait `PaneContent`
//! para terminais, integrando:
//! - TerminalBuffer (grid + parser + scrollback)
//! - PTY (comunicação com processo shell)
//! - Input routing (teclado e mouse)
//! - Renderização de conteúdo

use super::{
    content::{PaneContent, PaneContentType, PaneState},
    generate_pane_id,
    input::{InputEvent, KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    PaneError, PaneId, PaneMetadata, PaneResult, RenderInfo,
};
use crate::terminal::buffer::TerminalBuffer;
use crate::terminal::grid::{Grid, SelectionKind};
use crate::terminal::modes::MouseMode;
use tracing::{debug, trace};

/// TerminalPane - Implementação completa de pane de terminal
///
/// Integra TerminalBuffer (com PTY) ao sistema de panes,
/// implementando o trait `PaneContent` para permitir:
/// - Renderização no sistema de UI
/// - Roteamento de input
/// - Gestão de estado (foco, scroll, seleção)
#[derive(Debug)]
pub struct TerminalPane {
    /// ID único do pane
    id: PaneId,
    /// Buffer do terminal (integra Grid + Parser + PTY)
    buffer: TerminalBuffer,
    /// Metadados do pane (título, cwd, pid, etc)
    metadata: PaneMetadata,
    /// Informações de renderização (posição, tamanho, estado)
    render_info: RenderInfo,
    /// Estado atual do pane
    state: PaneState,
    /// Flag indicando se precisa ser re-renderizado
    dirty: bool,
    /// Modo de cópia ativo
    copy_mode: bool,
    /// Posição inicial da seleção (para modo cópia)
    selection_start: Option<(u16, u16)>,
    /// Última atividade (para timeouts)
    last_activity: std::time::Instant,
}

impl TerminalPane {
    /// Cria um novo TerminalPane com as dimensões especificadas
    ///
    /// # Arguments
    /// * `cols` - Número de colunas
    /// * `rows` - Número de linhas
    ///
    /// # Example
    /// ```
    /// use code_terminal::pane::TerminalPane;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let pane = TerminalPane::new(80, 24)?;
    /// # let _ = pane;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(cols: u16, rows: u16) -> PaneResult<Self> {
        let buffer = TerminalBuffer::new(cols, rows).map_err(PaneError::Internal)?;

        let mut metadata = PaneMetadata::default();
        metadata.title = "Terminal".to_string();

        Ok(Self {
            id: generate_pane_id(),
            buffer,
            metadata,
            render_info: RenderInfo::new(0, 0, 0, 0),
            state: PaneState::Background,
            dirty: true,
            copy_mode: false,
            selection_start: None,
            last_activity: std::time::Instant::now(),
        })
    }

    /// Cria um novo TerminalPane com título personalizado
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.metadata.title = title.into();
        self
    }

    /// Cria um novo TerminalPane com diretório de trabalho inicial
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.metadata.cwd = Some(cwd.into());
        self
    }

    /// Cria um novo TerminalPane com comando específico
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        let command = command.into();
        self.metadata.command = Some(command.clone());
        self.metadata.title = command;
        self
    }

    /// Retorna referência ao buffer do terminal
    pub fn buffer(&self) -> &TerminalBuffer {
        &self.buffer
    }

    /// Retorna referência mutável ao buffer do terminal
    pub fn buffer_mut(&mut self) -> &mut TerminalBuffer {
        &mut self.buffer
    }

    /// Retorna o PID do processo do PTY
    pub fn pid(&self) -> Option<u32> {
        self.buffer.pty().pid()
    }

    /// Verifica se o processo do PTY está rodando
    pub fn is_process_running(&self) -> bool {
        self.buffer.pty().is_running()
    }

    /// Envia dados brutos para o PTY
    pub fn send_to_pty(&mut self, data: &[u8]) -> PaneResult<()> {
        self.buffer
            .write(data)
            .map_err(|e| PaneError::Pty(e.to_string()))?;
        self.last_activity = std::time::Instant::now();
        Ok(())
    }

    /// Envia uma string para o PTY
    pub fn send_str_to_pty(&mut self, s: &str) -> PaneResult<()> {
        self.buffer
            .write_str(s)
            .map_err(|e| PaneError::Pty(e.to_string()))?;
        self.last_activity = std::time::Instant::now();
        Ok(())
    }

    /// Envia Ctrl+C (SIGINT) para o PTY
    pub fn send_interrupt(&mut self) -> PaneResult<()> {
        // Ctrl+C é enviado como byte 0x03 (ETX - End of Text)
        self.send_to_pty(&[0x03])?;
        self.last_activity = std::time::Instant::now();
        Ok(())
    }

    /// Envia Ctrl+D (EOF) para o PTY
    pub fn send_eof(&mut self) -> PaneResult<()> {
        // Ctrl+D é enviado como byte 0x04 (EOT - End of Transmission)
        self.send_to_pty(&[0x04])?;
        Ok(())
    }

    /// Retorna o tempo desde a última atividade
    pub fn idle_time(&self) -> std::time::Duration {
        self.last_activity.elapsed()
    }

    /// Processa output pendente do PTY
    /// Deve ser chamado regularmente (a cada frame)
    pub fn process_output(&mut self) {
        self.buffer.process_output();
        // Verifica se houve mudanças no grid
        if self.buffer.grid().scroll_offset() != self.scroll_position() {
            self.dirty = true;
        }
    }

    /// Converte InputEvent em sequência de bytes para o PTY
    fn input_event_to_bytes(&self, event: &InputEvent) -> Option<Vec<u8>> {
        match event {
            InputEvent::Key { data, .. } => Some(data.clone()),
            InputEvent::Mouse { kind, position, .. } => {
                // Gera sequências de mouse para protocolo xterm
                self.mouse_event_to_sequence(*kind, *position)
            }
            InputEvent::Resize { cols, rows } => {
                // Resize é tratado separadamente via resize()
                let _ = (*cols, *rows);
                None
            }
        }
    }

    /// Converte evento de mouse em sequência de escape
    fn mouse_event_to_sequence(
        &self,
        kind: MouseEventKind,
        position: (u16, u16),
    ) -> Option<Vec<u8>> {
        // Simplified mouse protocol - só envia se o grid suportar
        let grid = self.buffer.grid();
        if grid.modes().mouse_mode == MouseMode::None {
            return None;
        }

        let (col, row) = position;
        let (max_col, max_row) = self.buffer.content_size();

        if col >= max_col || row >= max_row {
            return None;
        }

        // Converte para formato X10/X11 mouse reporting
        let cb = match kind {
            MouseEventKind::Down(MouseButton::Left) => 0,
            MouseEventKind::Down(MouseButton::Middle) => 1,
            MouseEventKind::Down(MouseButton::Right) => 2,
            MouseEventKind::Up(_) => 3,
            MouseEventKind::ScrollUp => 64,
            MouseEventKind::ScrollDown => 65,
            _ => return None,
        };

        let cx = (col + 1).min(223) as u8; // Limita ao range válido
        let cy = (row + 1).min(223) as u8;

        Some(vec![0x1b, b'[', b'M', 32 + cb, 32 + cx, 32 + cy])
    }

    /// Processa input de teclado especial (navegação, cópia, etc)
    fn process_special_key(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        match (key, modifiers) {
            // Modo cópia - navegação
            (KeyCode::Arrow(arrow), _) if self.copy_mode => {
                match arrow {
                    super::input::ArrowKey::Up => self.scroll_up(1),
                    super::input::ArrowKey::Down => self.scroll_down(1),
                    super::input::ArrowKey::Left => self.scroll_up(self.buffer.grid().size().1 / 2),
                    super::input::ArrowKey::Right => {
                        self.scroll_down(self.buffer.grid().size().1 / 2)
                    }
                }
                true
            }
            // Sair do modo cópia com Escape ou 'q'
            (KeyCode::Escape, _) if self.copy_mode => {
                self.exit_copy_mode();
                true
            }
            (
                KeyCode::Char('q'),
                KeyModifiers {
                    ctrl: false,
                    alt: false,
                    shift: false,
                    ..
                },
            ) if self.copy_mode => {
                self.exit_copy_mode();
                true
            }
            // Enter no modo cópia copia seleção
            (KeyCode::Enter, _) if self.copy_mode => {
                if let Some(text) = self.get_selection_text() {
                    debug!(len = text.len(), "Copiando seleção para clipboard");
                    // TODO: Integrar com clipboard do sistema
                    let _ = text;
                }
                self.exit_copy_mode();
                true
            }
            // Scroll com Shift+PageUp/PageDown
            (KeyCode::PageUp, KeyModifiers { shift: true, .. }) => {
                self.scroll_up(self.buffer.grid().size().1 / 2);
                true
            }
            (KeyCode::PageDown, KeyModifiers { shift: true, .. }) => {
                self.scroll_down(self.buffer.grid().size().1 / 2);
                true
            }
            // Ctrl+C - copiar seleção ou enviar SIGINT
            (KeyCode::Char('c'), KeyModifiers { ctrl: true, .. }) => {
                if let Some(text) = self.get_selection_text() {
                    if !text.is_empty() {
                        debug!(len = text.len(), "Copiando seleção");
                        // TODO: Integrar com clipboard
                        let _ = text;
                        return true;
                    }
                }
                // Se não houver seleção, envia SIGINT
                let _ = self.send_interrupt();
                true
            }
            // Ctrl+Shift+C - copiar sempre
            (
                KeyCode::Char('C'),
                KeyModifiers {
                    ctrl: true,
                    shift: true,
                    ..
                },
            ) => {
                let text = self.get_content_text();
                debug!(len = text.len(), "Copiando todo conteúdo");
                // TODO: Integrar com clipboard
                let _ = text;
                true
            }
            _ => false,
        }
    }
}

impl PaneContent for TerminalPane {
    fn pane_id(&self) -> PaneId {
        self.id
    }

    fn content_type(&self) -> PaneContentType {
        PaneContentType::Shell
    }

    fn state(&self) -> PaneState {
        self.state
    }

    fn set_state(&mut self, state: PaneState) {
        if self.state != state {
            debug!(pane_id = self.id, ?state, "Mudando estado do pane");
            self.state = state;
            self.dirty = true;
        }
    }

    fn metadata(&self) -> &PaneMetadata {
        &self.metadata
    }

    fn set_metadata(&mut self, metadata: PaneMetadata) {
        self.metadata = metadata;
    }

    fn render_info(&self) -> RenderInfo {
        self.render_info
    }

    fn set_render_info(&mut self, info: RenderInfo) {
        self.render_info = info;
        self.dirty = true;
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn mark_clean(&mut self) {
        self.dirty = false;
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn grid(&self) -> Option<&Grid> {
        Some(self.buffer.grid())
    }

    fn grid_mut(&mut self) -> Option<&mut Grid> {
        Some(self.buffer.grid_mut())
    }

    fn process_input(&mut self, event: &InputEvent) -> PaneResult<bool> {
        trace!(?event, pane_id = self.id, "Processando input");

        match event {
            InputEvent::Key {
                key_code,
                modifiers,
                ..
            } => {
                // Primeiro tenta processar teclas especiais
                if let Some(key) = key_code {
                    if self.process_special_key(*key, *modifiers) {
                        return Ok(true);
                    }
                }

                // Envia para o PTY se estiver ativo e não em modo cópia
                if self.state == PaneState::Active && !self.copy_mode {
                    if let Some(data) = self.input_event_to_bytes(event) {
                        self.send_to_pty(&data)?;
                        self.mark_dirty();
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                } else {
                    Ok(false)
                }
            }
            InputEvent::Mouse { kind, position, .. } => {
                // Processa eventos de mouse
                match kind {
                    MouseEventKind::Click(MouseButton::Left) => {
                        if self.copy_mode {
                            // Inicia seleção no modo cópia
                            self.selection_start = Some(*position);
                            self.set_selection(*position, *position);
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        if self.copy_mode {
                            if let Some(start) = self.selection_start {
                                self.set_selection(start, *position);
                                self.selection_start = None;
                            }
                        }
                    }
                    MouseEventKind::ScrollUp => {
                        self.scroll_up(3);
                        return Ok(true);
                    }
                    MouseEventKind::ScrollDown => {
                        self.scroll_down(3);
                        return Ok(true);
                    }
                    _ => {}
                }

                // Envia para o PTY se tiver mouse tracking habilitado
                if let Some(seq) = self.input_event_to_bytes(event) {
                    self.send_to_pty(&seq)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            InputEvent::Resize { cols, rows } => {
                self.resize(*cols, *rows)?;
                Ok(true)
            }
        }
    }

    fn update(&mut self) -> PaneResult<()> {
        // Processa output do PTY
        self.process_output();

        // Atualiza metadados do processo
        if let Some(pid) = self.pid() {
            self.metadata.pid = Some(pid);
        }

        // Verifica se o processo terminou
        if !self.is_process_running() {
            // TODO: Notificar UI que o processo terminou
            trace!(pane_id = self.id, "Processo do PTY terminou");
        }

        Ok(())
    }

    fn get_content_text(&self) -> String {
        // TerminalBuffer::get_content_text devolve só a seleção; aqui o
        // contrato é o conteúdo inteiro (usado pelo Ctrl+Shift+C)
        super::grid_text(self.buffer.grid())
    }

    fn get_selection_text(&self) -> Option<String> {
        let grid = self.buffer.grid();
        let text = grid.get_selected_text();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn set_selection(&mut self, start: (u16, u16), end: (u16, u16)) {
        let grid = self.buffer.grid_mut();
        grid.start_selection(start.1 as usize, start.0 as usize, SelectionKind::Normal);
        grid.update_selection(end.1 as usize, end.0 as usize);
        self.dirty = true;
    }

    fn clear_selection(&mut self) {
        let grid = self.buffer.grid_mut();
        grid.clear_selection();
        self.selection_start = None;
        self.dirty = true;
    }

    fn cursor_position(&self) -> (u16, u16) {
        let (row, col) = self.buffer.grid().cursor_position();
        (col as u16, row as u16)
    }

    fn set_cursor_position(&mut self, col: u16, row: u16) {
        // TerminalPane não permite posicionar cursor arbitrariamente
        // (o cursor é controlado pelo processo do PTY)
        let _ = (col, row);
    }

    fn content_size(&self) -> (u16, u16) {
        self.buffer.content_size()
    }

    fn resize(&mut self, cols: u16, rows: u16) -> PaneResult<()> {
        debug!(pane_id = self.id, cols, rows, "Redimensionando pane");
        self.buffer
            .resize(cols, rows)
            .map_err(PaneError::Internal)?;
        self.dirty = true;
        Ok(())
    }

    fn scroll_up(&mut self, lines: usize) {
        self.buffer.scroll_up(lines);
        self.dirty = true;
    }

    fn scroll_down(&mut self, lines: usize) {
        self.buffer.scroll_down(lines);
        self.dirty = true;
    }

    fn scroll_to_top(&mut self) {
        self.buffer.scroll_to_top();
        self.dirty = true;
    }

    fn scroll_to_bottom(&mut self) {
        self.buffer.scroll_to_bottom();
        self.dirty = true;
    }

    fn scroll_position(&self) -> usize {
        self.buffer.scroll_position()
    }

    fn enter_copy_mode(&mut self) {
        debug!(pane_id = self.id, "Entrando em modo cópia");
        self.copy_mode = true;
        self.scroll_to_top();
    }

    fn exit_copy_mode(&mut self) {
        debug!(pane_id = self.id, "Saindo do modo cópia");
        self.copy_mode = false;
        self.clear_selection();
        self.scroll_to_bottom();
    }

    fn is_in_copy_mode(&self) -> bool {
        self.copy_mode
    }

    fn serialize(&self) -> serde_json::Value {
        serde_json::json!({
            "pane_id": self.id,
            "content_type": "Shell",
            "state": format!("{:?}", self.state),
            "title": self.metadata.title,
            "cwd": self.metadata.cwd,
            "command": self.metadata.command,
            "pid": self.metadata.pid,
            "cell_size": self.content_size(),
            "scroll_position": self.scroll_position(),
            "copy_mode": self.copy_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_pane_creation() {
        let pane = TerminalPane::new(80, 24).unwrap();
        assert_eq!(pane.content_size(), (80, 24));
        assert!(pane.is_dirty());
        assert_eq!(pane.state(), PaneState::Background);
        assert_eq!(pane.content_type(), PaneContentType::Shell);
    }

    #[test]
    fn test_terminal_pane_with_title() {
        let pane = TerminalPane::new(80, 24)
            .unwrap()
            .with_title("Test Terminal");
        assert_eq!(pane.metadata().title, "Test Terminal");
    }

    #[test]
    fn test_terminal_pane_with_cwd() {
        let pane = TerminalPane::new(80, 24).unwrap().with_cwd("/home/user");
        assert_eq!(pane.metadata().cwd, Some("/home/user".to_string()));
    }

    #[test]
    fn test_terminal_pane_state_transitions() {
        let mut pane = TerminalPane::new(80, 24).unwrap();

        assert_eq!(pane.state(), PaneState::Background);
        assert!(pane.is_dirty());

        pane.mark_clean();
        assert!(!pane.is_dirty());

        pane.set_state(PaneState::Active);
        assert_eq!(pane.state(), PaneState::Active);
        assert!(pane.is_dirty());

        pane.mark_clean();
        pane.set_state(PaneState::Zoomed);
        assert_eq!(pane.state(), PaneState::Zoomed);
        assert!(pane.is_dirty());
    }

    #[test]
    fn test_terminal_pane_scroll() {
        let mut pane = TerminalPane::new(80, 24).unwrap();
        pane.mark_clean();

        assert_eq!(pane.scroll_position(), 0);

        pane.scroll_up(5);
        assert!(pane.is_dirty());
        assert_eq!(pane.scroll_position(), 0);

        pane.mark_clean();
        pane.scroll_down(3);
        assert_eq!(pane.scroll_position(), 0);
    }

    #[test]
    fn test_terminal_pane_copy_mode() {
        let mut pane = TerminalPane::new(80, 24).unwrap();

        assert!(!pane.is_in_copy_mode());

        pane.enter_copy_mode();
        assert!(pane.is_in_copy_mode());

        pane.exit_copy_mode();
        assert!(!pane.is_in_copy_mode());
    }

    #[test]
    fn test_terminal_pane_resize() {
        let mut pane = TerminalPane::new(80, 24).unwrap();
        pane.mark_clean();

        pane.resize(100, 30).unwrap();
        assert_eq!(pane.content_size(), (100, 30));
        assert!(pane.is_dirty());
    }

    #[test]
    fn test_terminal_pane_pid() {
        let pane = TerminalPane::new(80, 24).unwrap();
        // O pane deve ter um PID pois criou um PTY
        assert!(pane.pid().is_some());
        assert!(pane.is_process_running());
    }

    #[test]
    fn test_terminal_pane_serialize() {
        let pane = TerminalPane::new(80, 24).unwrap().with_title("Test");

        let json = pane.serialize();
        assert_eq!(json["content_type"], "Shell");
        assert_eq!(json["title"], "Test");
        assert!(json["cell_size"].is_array());
    }

    #[test]
    fn test_input_router_integration() {
        use super::super::input::InputRouter;

        let mut router = InputRouter::new();
        let pane = TerminalPane::new(80, 24).unwrap();
        let id = pane.pane_id();

        router.register_pane(Box::new(pane));
        assert_eq!(router.pane_count(), 1);
        assert_eq!(router.focused_pane(), Some(id));
    }

    #[test]
    fn test_process_resize_event() {
        let mut pane = TerminalPane::new(80, 24).unwrap();
        pane.mark_clean();

        let event = InputEvent::Resize {
            cols: 100,
            rows: 30,
        };
        let result = pane.process_input(&event);

        assert!(result.unwrap());
        assert_eq!(pane.content_size(), (100, 30));
        assert!(pane.is_dirty());
    }

    #[test]
    fn test_process_key_in_copy_mode() {
        let mut pane = TerminalPane::new(80, 24).unwrap();
        pane.enter_copy_mode();

        // Tecla 'q' deve sair do modo cópia
        let event = InputEvent::Key {
            data: vec![b'q'],
            key_code: Some(KeyCode::Char('q')),
            modifiers: KeyModifiers::default(),
        };

        let result = pane.process_input(&event).unwrap();
        assert!(result);
        assert!(!pane.is_in_copy_mode());
    }

    #[test]
    fn test_scroll_to_top_bottom() {
        let mut pane = TerminalPane::new(80, 24).unwrap();

        // Primeiro scrolla para cima
        pane.scroll_up(100);
        let pos_up = pane.scroll_position();

        // Scroll to bottom deve zerar
        pane.scroll_to_bottom();
        assert_eq!(pane.scroll_position(), 0);

        // Scroll to top deve ir para o máximo
        pane.scroll_to_top();
        assert_eq!(pane.scroll_position(), pos_up);
    }

    use crate::pane::input::ArrowKey;

    fn key_event(code: KeyCode, mods: KeyModifiers, data: &[u8]) -> InputEvent {
        InputEvent::Key {
            data: data.to_vec(),
            key_code: Some(code),
            modifiers: mods,
        }
    }

    fn mouse_event(kind: MouseEventKind, col: u16, row: u16) -> InputEvent {
        InputEvent::Mouse {
            kind,
            position: (col, row),
            modifiers: KeyModifiers::default(),
        }
    }

    const CTRL: KeyModifiers = KeyModifiers {
        ctrl: true,
        alt: false,
        shift: false,
        meta: false,
    };
    const SHIFT: KeyModifiers = KeyModifiers {
        ctrl: false,
        alt: false,
        shift: true,
        meta: false,
    };
    const NONE: KeyModifiers = KeyModifiers {
        ctrl: false,
        alt: false,
        shift: false,
        meta: false,
    };

    #[test]
    fn test_mouse_sequences_follow_tracking_mode() {
        let mut pane = TerminalPane::new(20, 10).unwrap();
        let down = mouse_event(MouseEventKind::Down(MouseButton::Left), 0, 0);
        // sem tracking não há sequência
        assert!(pane.input_event_to_bytes(&down).is_none());

        pane.grid_mut().unwrap().modes_mut().mouse_mode = MouseMode::X10;
        let seq = |p: &TerminalPane, kind, c, r| p.input_event_to_bytes(&mouse_event(kind, c, r));
        assert_eq!(
            seq(&pane, MouseEventKind::Down(MouseButton::Left), 0, 0),
            Some(vec![0x1b, b'[', b'M', 32, 33, 33])
        );
        assert_eq!(
            seq(&pane, MouseEventKind::Down(MouseButton::Middle), 4, 2).unwrap()[3..],
            [33, 37, 35]
        );
        assert_eq!(
            seq(&pane, MouseEventKind::Down(MouseButton::Right), 0, 0).unwrap()[3],
            34
        );
        assert_eq!(
            seq(&pane, MouseEventKind::Up(MouseButton::Left), 0, 0).unwrap()[3],
            35
        );
        assert_eq!(seq(&pane, MouseEventKind::ScrollUp, 0, 0).unwrap()[3], 96);
        assert_eq!(seq(&pane, MouseEventKind::ScrollDown, 0, 0).unwrap()[3], 97);
        assert!(seq(&pane, MouseEventKind::Move, 0, 0).is_none());
        // fora da área do terminal
        assert!(seq(&pane, MouseEventKind::Down(MouseButton::Left), 20, 0).is_none());
        assert!(seq(&pane, MouseEventKind::Down(MouseButton::Left), 0, 10).is_none());

        assert_eq!(
            pane.input_event_to_bytes(&key_event(KeyCode::Char('a'), NONE, b"a")),
            Some(b"a".to_vec())
        );
        assert!(pane
            .input_event_to_bytes(&InputEvent::Resize { cols: 1, rows: 1 })
            .is_none());

        // com tracking o clique vai para o PTY
        pane.set_state(PaneState::Active);
        assert!(pane.process_input(&down).unwrap());
        // e o movimento, que não gera sequência, não é consumido
        assert!(!pane
            .process_input(&mouse_event(MouseEventKind::Move, 1, 1))
            .unwrap());
    }

    #[test]
    fn test_copy_mode_keys() {
        let mut pane = TerminalPane::new(20, 10).unwrap();
        pane.enter_copy_mode();
        for arrow in [
            ArrowKey::Up,
            ArrowKey::Down,
            ArrowKey::Left,
            ArrowKey::Right,
        ] {
            assert!(pane.process_special_key(KeyCode::Arrow(arrow), NONE));
            assert!(pane.is_in_copy_mode());
        }
        assert!(pane.process_special_key(KeyCode::Escape, NONE));
        assert!(!pane.is_in_copy_mode());

        pane.enter_copy_mode();
        // 'q' com modificador não sai
        assert!(!pane.process_special_key(KeyCode::Char('q'), CTRL));
        assert!(pane.process_special_key(KeyCode::Char('q'), NONE));
        assert!(!pane.is_in_copy_mode());

        pane.enter_copy_mode();
        pane.set_selection((0, 0), (3, 0));
        assert!(pane.process_special_key(KeyCode::Enter, NONE));
        assert!(!pane.is_in_copy_mode());
        // sair do modo cópia limpa a seleção
        assert!(pane.get_selection_text().is_none());

        // fora do modo cópia setas e Escape não são especiais
        assert!(!pane.process_special_key(KeyCode::Arrow(ArrowKey::Up), NONE));
        assert!(!pane.process_special_key(KeyCode::Escape, NONE));
        assert!(!pane.process_special_key(KeyCode::Enter, NONE));
        assert!(pane.process_special_key(KeyCode::PageUp, SHIFT));
        assert!(pane.process_special_key(KeyCode::PageDown, SHIFT));
        assert!(!pane.process_special_key(KeyCode::PageUp, NONE));
    }

    #[test]
    fn test_ctrl_c_copies_selection_or_interrupts() {
        let mut pane = TerminalPane::new(20, 10).unwrap();
        pane.set_selection((0, 0), (5, 0));
        assert!(pane.get_selection_text().is_some());
        assert!(pane.process_special_key(KeyCode::Char('c'), CTRL));
        pane.clear_selection();
        assert!(pane.get_selection_text().is_none());
        // sem seleção manda SIGINT (Ctrl+C) para o PTY
        assert!(pane.process_special_key(KeyCode::Char('c'), CTRL));
        let ctrl_shift = KeyModifiers {
            ctrl: true,
            shift: true,
            ..NONE
        };
        assert!(pane.process_special_key(KeyCode::Char('C'), ctrl_shift));
        assert!(!pane.process_special_key(KeyCode::Char('x'), CTRL));
    }

    #[test]
    fn test_process_input_keys_depend_on_state() {
        let mut pane = TerminalPane::new(20, 10).unwrap();
        let a = key_event(KeyCode::Char('a'), NONE, b"a");
        // em background a tecla não vai para o PTY
        assert!(!pane.process_input(&a).unwrap());
        pane.set_state(PaneState::Active);
        pane.mark_clean();
        assert!(pane.process_input(&a).unwrap());
        assert!(pane.is_dirty());
        // tecla especial é tratada antes do PTY
        assert!(pane
            .process_input(&key_event(KeyCode::PageUp, SHIFT, b""))
            .unwrap());
        // no modo cópia teclas comuns não são enviadas
        pane.enter_copy_mode();
        assert!(!pane.process_input(&a).unwrap());
        pane.exit_copy_mode();
        // sem key_code: usa os bytes crus
        let raw = InputEvent::Key {
            data: b"ls\r".to_vec(),
            key_code: None,
            modifiers: NONE,
        };
        assert!(pane.process_input(&raw).unwrap());
    }

    #[test]
    fn test_mouse_selection_and_scroll_in_copy_mode() {
        let mut pane = TerminalPane::new(20, 10).unwrap();
        pane.enter_copy_mode();
        assert!(!pane
            .process_input(&mouse_event(MouseEventKind::Click(MouseButton::Left), 1, 0))
            .unwrap());
        assert_eq!(pane.selection_start, Some((1, 0)));
        pane.process_input(&mouse_event(MouseEventKind::Up(MouseButton::Left), 4, 0))
            .unwrap();
        assert_eq!(pane.selection_start, None);
        assert!(pane.get_selection_text().is_some());

        assert!(pane
            .process_input(&mouse_event(MouseEventKind::ScrollUp, 0, 0))
            .unwrap());
        assert!(pane
            .process_input(&mouse_event(MouseEventKind::ScrollDown, 0, 0))
            .unwrap());

        // fora do modo cópia clique não seleciona
        pane.exit_copy_mode();
        pane.process_input(&mouse_event(MouseEventKind::Click(MouseButton::Left), 2, 2))
            .unwrap();
        assert_eq!(pane.selection_start, None);
        pane.process_input(&mouse_event(MouseEventKind::Up(MouseButton::Left), 2, 2))
            .unwrap();
    }

    #[test]
    fn test_pty_io_update_and_metadata() {
        let mut pane = TerminalPane::new(30, 8).unwrap().with_command("htop -d 10");
        assert_eq!(pane.metadata().title, "htop -d 10");
        assert_eq!(pane.metadata().command.as_deref(), Some("htop -d 10"));

        pane.send_to_pty(b"true\r").unwrap();
        pane.send_str_to_pty("echo ok\r").unwrap();
        assert!(pane.idle_time() < std::time::Duration::from_secs(5));
        assert!(pane
            .process_input(&InputEvent::Resize { cols: 40, rows: 12 })
            .unwrap());
        assert_eq!(pane.content_size(), (40, 12));
        assert_eq!(pane.buffer().content_size(), (40, 12));
        assert_eq!(pane.buffer_mut().content_size(), (40, 12));

        pane.update().unwrap();
        assert_eq!(pane.metadata().pid, pane.pid());
        pane.process_output();

        let mut meta = pane.metadata().clone();
        meta.title = "novo".into();
        pane.set_metadata(meta);
        assert_eq!(pane.metadata().title, "novo");

        pane.mark_clean();
        let info = RenderInfo::new(1, 2, 3, 4);
        pane.set_render_info(info);
        assert_eq!(pane.render_info(), info);
        assert!(pane.is_dirty());

        assert!(pane.cursor_position().1 < 12);
        pane.set_cursor_position(5, 5);
        let v = pane.serialize();
        assert_eq!(v["title"], "novo");
        assert_eq!(v["cell_size"], serde_json::json!([40, 12]));
        assert_eq!(v["copy_mode"], false);
    }

    #[test]
    fn test_content_text_is_whole_grid_not_selection() {
        // Regressão: devolvia só o texto selecionado (vazio sem seleção)
        let mut pane = TerminalPane::new(5, 2).unwrap();
        crate::terminal::parser::Parser::new(pane.grid_mut().unwrap()).process(b"oi\r\ntudo");
        assert_eq!(pane.get_content_text(), "oi   \ntudo ");
        assert!(pane.get_selection_text().is_none());
    }

    #[test]
    fn test_control_bytes_reach_the_pty() {
        // cada um num pane novo: o shell pode encerrar depois de ^C/^D
        let mut pane = TerminalPane::new(10, 4).unwrap();
        pane.send_interrupt().unwrap();
        let mut pane = TerminalPane::new(10, 4).unwrap();
        pane.send_eof().unwrap();
    }
}
