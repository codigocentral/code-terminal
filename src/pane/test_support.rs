//! Pane falso para testar roteamento e gerenciamento sem PTY.

use super::input::InputEvent;
use super::{
    generate_pane_id, PaneContent, PaneContentType, PaneError, PaneId, PaneMetadata, PaneResult,
    PaneState, RenderInfo,
};
use std::sync::{Arc, Mutex};

/// Registro do que o [`FakePane`] recebeu.
#[derive(Debug, Default)]
pub struct FakeLog {
    pub inputs: Vec<InputEvent>,
    pub scroll: i64,
    pub selection: Option<((u16, u16), (u16, u16))>,
    pub cleared: usize,
    pub resized: Vec<(u16, u16)>,
    pub updates: usize,
}

/// Pane em memória: consome input conforme `consume`, guarda o que recebe
/// em `log` (compartilhado, para inspecionar depois de entregar o Box).
#[derive(Debug)]
pub struct FakePane {
    pub id: PaneId,
    pub state: PaneState,
    pub metadata: PaneMetadata,
    pub render_info: RenderInfo,
    pub dirty: bool,
    pub consume: bool,
    pub fail: bool,
    pub size: (u16, u16),
    pub log: Arc<Mutex<FakeLog>>,
}

impl FakePane {
    /// Pane ocupando o retângulo dado (em pixels).
    pub fn at(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self {
            id: generate_pane_id(),
            state: PaneState::Background,
            metadata: PaneMetadata::default(),
            render_info: RenderInfo::new(x, y, w, h),
            dirty: false,
            consume: true,
            fail: false,
            size: (80, 24),
            log: Arc::default(),
        }
    }

    fn check(&self) -> PaneResult<()> {
        if self.fail {
            Err(PaneError::InvalidState("falha simulada".into()))
        } else {
            Ok(())
        }
    }
}

impl PaneContent for FakePane {
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
        self.state = state;
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
    fn process_input(&mut self, input: &InputEvent) -> PaneResult<bool> {
        self.check()?;
        self.log.lock().unwrap().inputs.push(input.clone());
        Ok(self.consume)
    }
    fn update(&mut self) -> PaneResult<()> {
        self.check()?;
        self.log.lock().unwrap().updates += 1;
        Ok(())
    }
    fn get_content_text(&self) -> String {
        format!("pane {}", self.id)
    }
    fn set_selection(&mut self, start: (u16, u16), end: (u16, u16)) {
        self.log.lock().unwrap().selection = Some((start, end));
    }
    fn clear_selection(&mut self) {
        self.log.lock().unwrap().cleared += 1;
    }
    fn content_size(&self) -> (u16, u16) {
        self.size
    }
    fn resize(&mut self, cols: u16, rows: u16) -> PaneResult<()> {
        self.check()?;
        self.size = (cols, rows);
        self.log.lock().unwrap().resized.push((cols, rows));
        Ok(())
    }
    fn scroll_up(&mut self, lines: usize) {
        self.log.lock().unwrap().scroll += lines as i64;
    }
    fn scroll_down(&mut self, lines: usize) {
        self.log.lock().unwrap().scroll -= lines as i64;
    }
    fn scroll_to_top(&mut self) {}
    fn scroll_to_bottom(&mut self) {}
}
