//! UnifiedPane - Integração de Pane Multiplexer + UI + Renderer
//!
//! Esta estrutura unifica as diferentes representações de Pane:
//! - Dados de sessão (multiplexer)
//! - Layout e Tabs (ui)
//! - Renderização (renderer)

use super::content::{PaneContent, PaneContentType, PaneState};
use super::{PaneError, PaneId, PaneMetadata, PaneResult, RenderInfo};
use crate::terminal::buffer::TerminalBuffer;
use crate::terminal::grid::Grid;
use crate::ui::layout::Rect;
use crate::ui::tabs::TabManager;
use std::sync::atomic::{AtomicU64, Ordering};

/// Contador global para IDs de pane
static PANE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Gera um novo ID de pane único
fn generate_pane_id() -> PaneId {
    PANE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Pane unificado que integra todas as camadas
///
/// Esta estrutura é usada como a representação principal de um pane
/// no sistema, combinando:
/// - Estado do terminal (TerminalBuffer)
/// - Informações de layout (posição, tamanho)
/// - Metadados de sessão (título, PID, etc)
/// - Estado de renderização
#[derive(Debug)]
pub struct UnifiedPane {
    /// ID único
    pub id: PaneId,

    /// Tipo de conteúdo
    pub content_type: PaneContentType,

    /// Estado atual
    pub state: PaneState,

    /// Buffer do terminal (conteúdo)
    pub buffer: TerminalBuffer,

    /// Gerenciador de tabs (para múltiplos terminais no mesmo pane)
    pub tab_manager: TabManager,

    /// Posição e tamanho na tela (pixels)
    pub rect: Rect,

    /// Posição e tamanho em células do terminal
    pub cell_size: (u16, u16),

    /// Metadados
    pub metadata: PaneMetadata,

    /// Flags de renderização
    pub render_flags: RenderFlags,

    /// Se precisa ser re-renderizado
    pub is_dirty: bool,

    /// Se está em modo de cópia
    pub copy_mode: bool,

    /// Timestamp de criação
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Timestamp da última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
}

/// Flags de renderização
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderFlags {
    /// Mostrar borda
    pub show_border: bool,
    /// Destacar como ativo
    pub highlight_active: bool,
    /// Mostrar barra de título
    pub show_title_bar: bool,
    /// Mostrar scrollbar
    pub show_scrollbar: bool,
    /// Inverter cores (modo highlight)
    pub reverse_video: bool,
}

/// ID de pane unificado
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnifiedPaneId(pub uuid::Uuid);

impl UnifiedPaneId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for UnifiedPaneId {
    fn default() -> Self {
        Self::new()
    }
}

impl UnifiedPane {
    /// Cria um novo pane unificado
    pub fn new(cols: u16, rows: u16) -> PaneResult<Self> {
        let buffer = TerminalBuffer::new(cols, rows)?;
        let id = generate_pane_id();
        let now = chrono::Utc::now();

        Ok(Self {
            id,
            content_type: PaneContentType::Shell,
            state: PaneState::Background,
            buffer,
            tab_manager: TabManager::new(),
            rect: Rect::new(0, 0, 0, 0),
            cell_size: (cols, rows),
            metadata: PaneMetadata::default(),
            render_flags: RenderFlags::default(),
            is_dirty: true,
            copy_mode: false,
            created_at: now,
            last_activity: now,
        })
    }

    /// Cria com título
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.metadata.title = title.into();
        self
    }

    /// Cria com comando
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.metadata.command = Some(command.into());
        self.content_type = PaneContentType::Command;
        self
    }

    /// Cria com tamanho de célula específico
    pub fn with_cell_size(mut self, cols: u16, rows: u16) -> PaneResult<Self> {
        self.resize(cols, rows)?;
        Ok(self)
    }

    /// Ativa o pane
    pub fn activate(&mut self) {
        self.state = PaneState::Active;
        self.render_flags.highlight_active = true;
        self.touch();
        self.mark_dirty();
    }

    /// Desativa o pane
    pub fn deactivate(&mut self) {
        self.state = PaneState::Background;
        self.render_flags.highlight_active = false;
        self.mark_dirty();
    }

    /// Alterna zoom
    pub fn toggle_zoom(&mut self) {
        self.state = match self.state {
            PaneState::Zoomed => PaneState::Active,
            _ => PaneState::Zoomed,
        };
        self.mark_dirty();
    }

    /// Marca como dirty (precisa re-renderizar)
    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }

    /// Marca como limpo (após renderização)
    pub fn mark_clean(&mut self) {
        self.is_dirty = false;
    }

    /// Atualiza timestamp de atividade
    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now();
    }

    /// Retorna o tempo de idle
    pub fn idle_time(&self) -> std::time::Duration {
        let now = chrono::Utc::now();
        now.signed_duration_since(self.last_activity)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(0))
    }

    /// Redimensiona o pane
    pub fn resize(&mut self, cols: u16, rows: u16) -> PaneResult<()> {
        self.buffer.resize(cols, rows)?;
        self.cell_size = (cols, rows);
        self.touch();
        self.mark_dirty();
        Ok(())
    }

    /// Move o pane para nova posição
    pub fn move_to(&mut self, x: u32, y: u32) {
        self.rect.x = x;
        self.rect.y = y;
        self.mark_dirty();
    }

    /// Define o tamanho do pane (em pixels)
    pub fn set_size(&mut self, width: u32, height: u32) {
        self.rect.width = width;
        self.rect.height = height;
        self.mark_dirty();
    }

    /// Processa output do terminal
    pub fn process_output(&mut self) {
        self.buffer.process_output();
        // TODO: Verificar se o grid realmente mudou antes de marcar dirty
        self.mark_dirty();
    }

    /// Escreve dados no terminal
    pub fn write(&mut self, data: &[u8]) -> PaneResult<()> {
        self.buffer.write(data)?;
        self.touch();
        Ok(())
    }

    /// Scroll para cima
    pub fn scroll_up(&mut self, lines: usize) {
        self.buffer.grid_mut().scroll_up_lines(lines);
        self.mark_dirty();
    }

    /// Scroll para baixo
    pub fn scroll_down(&mut self, lines: usize) {
        self.buffer.grid_mut().scroll_down_lines(lines);
        self.mark_dirty();
    }

    /// Scroll para o topo
    pub fn scroll_to_top(&mut self) {
        self.buffer.grid_mut().scroll_to_top();
        self.mark_dirty();
    }

    /// Scroll para o final
    pub fn scroll_to_bottom(&mut self) {
        self.buffer.grid_mut().scroll_to_bottom();
        self.mark_dirty();
    }

    /// Retorna a posição de scroll atual
    pub fn scroll_position(&self) -> usize {
        self.buffer.grid().scroll_offset()
    }

    /// Entra em modo de cópia
    pub fn enter_copy_mode(&mut self) {
        self.copy_mode = true;
        self.scroll_to_top();
        self.mark_dirty();
    }

    /// Sai do modo de cópia
    pub fn exit_copy_mode(&mut self) {
        self.copy_mode = false;
        self.buffer.grid_mut().clear_selection();
        self.scroll_to_bottom();
        self.mark_dirty();
    }

    /// Verifica se um ponto está dentro do pane
    pub fn contains_point(&self, x: u32, y: u32) -> bool {
        self.rect.contains(x, y)
    }

    /// Retorna a área em células
    pub fn area_cells(&self) -> u32 {
        self.cell_size.0 as u32 * self.cell_size.1 as u32
    }

    /// Retorna informações para renderização
    pub fn render_info(&self) -> RenderInfo {
        RenderInfo {
            position: (self.rect.x, self.rect.y),
            size: (self.rect.width, self.rect.height),
            is_active: self.state == PaneState::Active,
            is_dirty: self.is_dirty,
        }
    }

    /// Serializa para persistência de sessão
    pub fn serialize(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id.to_string(),
            "content_type": format!("{:?}", self.content_type),
            "state": format!("{:?}", self.state),
            "cell_size": self.cell_size,
            "rect": {
                "x": self.rect.x,
                "y": self.rect.y,
                "width": self.rect.width,
                "height": self.rect.height,
            },
            "metadata": {
                "title": self.metadata.title,
                "cwd": self.metadata.cwd,
                "command": self.metadata.command,
                "pid": self.metadata.pid,
            },
            "created_at": self.created_at.to_rfc3339(),
        })
    }
}

impl PaneContent for UnifiedPane {
    fn pane_id(&self) -> PaneId {
        self.id
    }

    fn content_type(&self) -> PaneContentType {
        self.content_type
    }

    fn state(&self) -> PaneState {
        self.state
    }

    fn set_state(&mut self, state: PaneState) {
        self.state = state;
        self.render_flags.highlight_active = state == PaneState::Active;
        self.mark_dirty();
    }

    fn metadata(&self) -> &PaneMetadata {
        &self.metadata
    }

    fn set_metadata(&mut self, metadata: PaneMetadata) {
        self.metadata = metadata;
    }

    fn render_info(&self) -> RenderInfo {
        self.render_info()
    }

    fn set_render_info(&mut self, info: RenderInfo) {
        self.rect.x = info.position.0;
        self.rect.y = info.position.1;
        self.rect.width = info.size.0;
        self.rect.height = info.size.1;
        self.is_dirty = info.is_dirty;
    }

    fn mark_dirty(&mut self) {
        UnifiedPane::mark_dirty(self);
    }

    fn mark_clean(&mut self) {
        UnifiedPane::mark_clean(self);
    }

    fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    fn grid(&self) -> Option<&Grid> {
        Some(self.buffer.grid())
    }

    fn grid_mut(&mut self) -> Option<&mut Grid> {
        Some(self.buffer.grid_mut())
    }

    fn process_input(&mut self, input: &super::input::InputEvent) -> PaneResult<bool> {
        use super::input::InputEvent;

        match input {
            InputEvent::Key { data, .. } => {
                self.write(data)?;
                Ok(true)
            }
            InputEvent::Mouse { kind, position, .. } => {
                // TODO: Implementar seleção com mouse
                let _ = (kind, position);
                Ok(false)
            }
            InputEvent::Resize { cols, rows } => {
                self.resize(*cols, *rows)?;
                Ok(true)
            }
        }
    }

    fn update(&mut self) -> PaneResult<()> {
        self.process_output();
        Ok(())
    }

    fn get_content_text(&self) -> String {
        super::grid_text(self.buffer.grid())
    }

    fn get_selection_text(&self) -> Option<String> {
        let text = self.buffer.grid().get_selected_text();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn set_selection(&mut self, start: (u16, u16), end: (u16, u16)) {
        use crate::terminal::grid::SelectionKind;
        let grid = self.buffer.grid_mut();
        grid.start_selection(start.1 as usize, start.0 as usize, SelectionKind::Normal);
        grid.update_selection(end.1 as usize, end.0 as usize);
        self.mark_dirty();
    }

    fn clear_selection(&mut self) {
        self.buffer.grid_mut().clear_selection();
        self.mark_dirty();
    }

    fn cursor_position(&self) -> (u16, u16) {
        let (row, col) = self.buffer.grid().cursor_position();
        (col as u16, row as u16)
    }

    fn content_size(&self) -> (u16, u16) {
        self.cell_size
    }

    fn resize(&mut self, cols: u16, rows: u16) -> PaneResult<()> {
        UnifiedPane::resize(self, cols, rows)
    }

    fn scroll_up(&mut self, lines: usize) {
        UnifiedPane::scroll_up(self, lines);
    }

    fn scroll_down(&mut self, lines: usize) {
        UnifiedPane::scroll_down(self, lines);
    }

    fn scroll_to_top(&mut self) {
        UnifiedPane::scroll_to_top(self);
    }

    fn scroll_to_bottom(&mut self) {
        UnifiedPane::scroll_to_bottom(self);
    }

    fn scroll_position(&self) -> usize {
        UnifiedPane::scroll_position(self)
    }

    fn enter_copy_mode(&mut self) {
        UnifiedPane::enter_copy_mode(self);
    }

    fn exit_copy_mode(&mut self) {
        UnifiedPane::exit_copy_mode(self);
    }

    fn is_in_copy_mode(&self) -> bool {
        self.copy_mode
    }
}

/// Gerenciador de panes unificados
#[derive(Debug)]
pub struct UnifiedPaneManager {
    panes: HashMap<PaneId, UnifiedPane>,
    active_pane: Option<PaneId>,
    last_active: Option<PaneId>,
    window_size: (u32, u32),
}

impl UnifiedPaneManager {
    pub fn new() -> Self {
        Self {
            panes: HashMap::new(),
            active_pane: None,
            last_active: None,
            window_size: (0, 0),
        }
    }

    pub fn with_window_size(mut self, width: u32, height: u32) -> Self {
        self.window_size = (width, height);
        self
    }

    /// Cria um novo pane
    pub fn create_pane(&mut self, cols: u16, rows: u16) -> PaneResult<PaneId> {
        let pane = UnifiedPane::new(cols, rows)?;
        let id = pane.id;

        self.panes.insert(id, pane);

        // Se for o primeiro, ativa
        if self.active_pane.is_none() {
            self.set_active_pane(id)?;
        }

        Ok(id)
    }

    /// Remove um pane
    pub fn remove_pane(&mut self, id: PaneId) -> PaneResult<UnifiedPane> {
        let pane = self
            .panes
            .remove(&id)
            .ok_or_else(|| PaneError::NotFound(id))?;

        if self.active_pane == Some(id) {
            let next = self
                .last_active
                .take()
                .filter(|last_id| self.panes.contains_key(last_id))
                .or_else(|| self.panes.keys().next().copied());
            // O substituto precisa ser ativado (antes só o ID mudava)
            self.active_pane = None;
            if let Some(next) = next {
                self.set_active_pane(next)?;
            }
        }

        Ok(pane)
    }

    /// Define o pane ativo
    pub fn set_active_pane(&mut self, id: PaneId) -> PaneResult<()> {
        if !self.panes.contains_key(&id) {
            return Err(PaneError::NotFound(id));
        }

        // Desativa o anterior
        if let Some(old_id) = self.active_pane {
            self.last_active = Some(old_id);
            if let Some(pane) = self.panes.get_mut(&old_id) {
                pane.deactivate();
            }
        }

        // Ativa o novo
        if let Some(pane) = self.panes.get_mut(&id) {
            pane.activate();
        }

        self.active_pane = Some(id);
        Ok(())
    }

    /// Retorna o pane ativo
    pub fn active_pane(&self) -> Option<&UnifiedPane> {
        self.active_pane.and_then(|id| self.panes.get(&id))
    }

    /// Retorna o pane ativo mutável
    pub fn active_pane_mut(&mut self) -> Option<&mut UnifiedPane> {
        self.active_pane.and_then(move |id| self.panes.get_mut(&id))
    }

    /// Retorna um pane pelo ID
    pub fn get_pane(&self, id: PaneId) -> Option<&UnifiedPane> {
        self.panes.get(&id)
    }

    /// Retorna um pane mutável pelo ID
    pub fn get_pane_mut(&mut self, id: PaneId) -> Option<&mut UnifiedPane> {
        self.panes.get_mut(&id)
    }

    /// Lista todos os panes
    pub fn list_panes(&self) -> Vec<&UnifiedPane> {
        self.panes.values().collect()
    }

    /// Encontra pane na posição
    pub fn pane_at_position(&self, x: u32, y: u32) -> Option<&UnifiedPane> {
        self.panes.values().find(|p| p.contains_point(x, y))
    }

    /// Retorna o número de panes
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Atualiza todos os panes
    pub fn update_all(&mut self) {
        for pane in self.panes.values_mut() {
            let _ = pane.update();
        }
    }

    /// Redimensiona todos os panes proporcionalmente
    pub fn resize_all(&mut self, new_width: u32, new_height: u32) {
        if self.window_size.0 == 0 || self.window_size.1 == 0 {
            self.window_size = (new_width, new_height);
            return;
        }

        let ratio_x = new_width as f32 / self.window_size.0 as f32;
        let ratio_y = new_height as f32 / self.window_size.1 as f32;

        for pane in self.panes.values_mut() {
            let new_x = (pane.rect.x as f32 * ratio_x) as u32;
            let new_y = (pane.rect.y as f32 * ratio_y) as u32;
            let new_w = (pane.rect.width as f32 * ratio_x) as u32;
            let new_h = (pane.rect.height as f32 * ratio_y) as u32;

            pane.move_to(new_x, new_y);
            pane.set_size(new_w, new_h);
        }

        self.window_size = (new_width, new_height);
    }

    /// Serializa todos os panes para persistência
    pub fn serialize_session(&self) -> serde_json::Value {
        serde_json::json!({
            "panes": self.panes.values().map(|p| p.serialize()).collect::<Vec<_>>(),
            "active_pane": self.active_pane.map(|id| id.to_string()),
            "window_size": self.window_size,
        })
    }

    /// Limpa todos os panes
    pub fn clear(&mut self) {
        self.panes.clear();
        self.active_pane = None;
        self.last_active = None;
    }
}

impl Default for UnifiedPaneManager {
    fn default() -> Self {
        Self::new()
    }
}

use std::collections::HashMap;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_pane_creation() {
        let pane = UnifiedPane::new(80, 24).unwrap();
        assert_eq!(pane.cell_size, (80, 24));
        assert_eq!(pane.state, PaneState::Background);
        assert!(pane.is_dirty);
    }

    #[test]
    fn test_unified_pane_activation() {
        let mut pane = UnifiedPane::new(80, 24).unwrap();

        pane.activate();
        assert_eq!(pane.state, PaneState::Active);
        assert!(pane.render_flags.highlight_active);

        pane.deactivate();
        assert_eq!(pane.state, PaneState::Background);
        assert!(!pane.render_flags.highlight_active);
    }

    #[test]
    fn test_unified_pane_manager() {
        let mut manager = UnifiedPaneManager::new();

        let id1 = manager.create_pane(80, 24).unwrap();
        let id2 = manager.create_pane(80, 24).unwrap();

        assert_eq!(manager.pane_count(), 2);
        assert_eq!(manager.active_pane().unwrap().id, id1);

        manager.set_active_pane(id2).unwrap();
        assert_eq!(manager.active_pane().unwrap().id, id2);

        // id1 deve estar em background
        assert_eq!(manager.get_pane(id1).unwrap().state, PaneState::Background);
        // id2 deve estar ativo
        assert_eq!(manager.get_pane(id2).unwrap().state, PaneState::Active);
    }

    #[test]
    fn test_pane_at_position() {
        let mut manager = UnifiedPaneManager::new();

        let id = manager.create_pane(80, 24).unwrap();
        {
            let pane = manager.get_pane_mut(id).unwrap();
            pane.move_to(10, 10);
            pane.set_size(100, 100);
        }

        let found = manager.pane_at_position(50, 50);
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, id);

        let not_found = manager.pane_at_position(200, 200);
        assert!(not_found.is_none());
    }

    #[test]
    fn test_remove_pane() {
        let mut manager = UnifiedPaneManager::new();

        let id1 = manager.create_pane(80, 24).unwrap();
        let id2 = manager.create_pane(80, 24).unwrap();

        manager.set_active_pane(id2).unwrap();

        manager.remove_pane(id2).unwrap();
        assert_eq!(manager.pane_count(), 1);

        // Deve voltar para id1
        assert_eq!(manager.active_pane().unwrap().id, id1);
    }

    #[test]
    fn test_serialize_session() {
        let mut manager = UnifiedPaneManager::new();
        manager.create_pane(80, 24).unwrap();

        let json = manager.serialize_session();
        assert!(json.get("panes").is_some());
        assert!(json.get("active_pane").is_some());
    }

    use crate::pane::input::{InputEvent, KeyModifiers, MouseEventKind};

    #[test]
    fn test_unified_pane_id_is_unique() {
        assert_ne!(UnifiedPaneId::new(), UnifiedPaneId::default());
    }

    #[test]
    fn test_pane_content_trait_methods() {
        let mut pane = UnifiedPane::new(6, 2)
            .unwrap()
            .with_title("t")
            .with_command("make")
            .with_cell_size(8, 3)
            .unwrap();
        assert_eq!(pane.content_type, PaneContentType::Command);
        assert_eq!(pane.metadata.command.as_deref(), Some("make"));
        assert_eq!(PaneContent::content_size(&pane), (8, 3));
        assert_eq!(pane.area_cells(), 24);
        let id = pane.id;
        let dyn_pane: &mut dyn PaneContent = &mut pane;
        assert_eq!(dyn_pane.pane_id(), id);

        dyn_pane.set_render_info(RenderInfo {
            position: (10, 20),
            size: (100, 50),
            is_active: false,
            is_dirty: false,
        });
        assert!(!dyn_pane.is_dirty());
        let info = dyn_pane.render_info();
        assert_eq!((info.position, info.size), ((10, 20), (100, 50)));
        dyn_pane.mark_dirty();
        assert!(dyn_pane.is_dirty());
        dyn_pane.mark_clean();
        assert!(!dyn_pane.is_dirty());

        let mut meta = dyn_pane.metadata().clone();
        meta.title = "novo".into();
        dyn_pane.set_metadata(meta);
        assert_eq!(dyn_pane.metadata().title, "novo");

        assert!(dyn_pane.grid().is_some());
        crate::terminal::parser::Parser::new(dyn_pane.grid_mut().unwrap()).process(b"ab\r\ncd");
        assert_eq!(dyn_pane.get_content_text(), "ab      \ncd      \n        ");
        assert_eq!(dyn_pane.cursor_position(), (2, 1));

        assert!(dyn_pane.get_selection_text().is_none());
        dyn_pane.set_selection((0, 0), (1, 0));
        assert_eq!(dyn_pane.get_selection_text().as_deref(), Some("ab"));
        dyn_pane.clear_selection();
        assert!(dyn_pane.get_selection_text().is_none());

        dyn_pane.scroll_up(2);
        dyn_pane.scroll_down(1);
        dyn_pane.scroll_to_top();
        dyn_pane.scroll_to_bottom();
        assert_eq!(dyn_pane.scroll_position(), 0);
        dyn_pane.enter_copy_mode();
        assert!(dyn_pane.is_in_copy_mode());
        dyn_pane.exit_copy_mode();
        assert!(!dyn_pane.is_in_copy_mode());

        // input: tecla vai para o PTY, mouse é ignorado, resize redimensiona
        let key = InputEvent::Key {
            data: b"true\r".to_vec(),
            key_code: None,
            modifiers: KeyModifiers::default(),
        };
        assert!(dyn_pane.process_input(&key).unwrap());
        let mouse = InputEvent::Mouse {
            kind: MouseEventKind::Move,
            position: (0, 0),
            modifiers: KeyModifiers::default(),
        };
        assert!(!dyn_pane.process_input(&mouse).unwrap());
        assert!(dyn_pane
            .process_input(&InputEvent::Resize { cols: 12, rows: 4 })
            .unwrap());
        assert_eq!(dyn_pane.content_size(), (12, 4));
        dyn_pane.resize(10, 3).unwrap();
        assert_eq!(dyn_pane.content_size(), (10, 3));
        assert!(pane.idle_time() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn test_manager_remove_activates_replacement() {
        let mut m = UnifiedPaneManager::default().with_window_size(800, 600);
        let a = m.create_pane(10, 3).unwrap();
        let b = m.create_pane(10, 3).unwrap();
        let c = m.create_pane(10, 3).unwrap();
        m.set_active_pane(b).unwrap();
        m.set_active_pane(c).unwrap();
        assert!(matches!(
            m.set_active_pane(9_999_999),
            Err(PaneError::NotFound(_))
        ));

        // removendo o ativo volta para o último ativo, que fica Active
        m.remove_pane(c).unwrap();
        assert_eq!(m.active_pane().unwrap().id, b);
        assert_eq!(m.get_pane(b).unwrap().state, PaneState::Active);
        // Regressão: sem histórico, o substituto também precisa ficar Active
        m.remove_pane(b).unwrap();
        assert_eq!(m.active_pane().unwrap().id, a);
        assert_eq!(m.get_pane(a).unwrap().state, PaneState::Active);
        assert!(matches!(m.remove_pane(b), Err(PaneError::NotFound(_))));

        m.active_pane_mut().unwrap().toggle_zoom();
        assert_eq!(m.get_pane(a).unwrap().state, PaneState::Zoomed);
        m.remove_pane(a).unwrap();
        assert!(m.active_pane().is_none());
    }

    #[test]
    fn test_manager_resize_all_scales_rects() {
        let mut m = UnifiedPaneManager::new();
        // sem tamanho de janela só registra o tamanho
        m.resize_all(400, 300);
        let id = m.create_pane(10, 3).unwrap();
        {
            let p = m.get_pane_mut(id).unwrap();
            p.move_to(100, 50);
            p.set_size(200, 100);
        }
        m.resize_all(800, 600);
        let p = m.get_pane(id).unwrap();
        assert_eq!(
            (p.rect.x, p.rect.y, p.rect.width, p.rect.height),
            (200, 100, 400, 200)
        );
        assert!(m.pane_at_position(300, 150).is_some());
        assert!(m.pane_at_position(10, 10).is_none());
        m.update_all();
        let v = m.serialize_session();
        assert_eq!(v["window_size"], serde_json::json!([800, 600]));
        assert_eq!(v["panes"].as_array().unwrap().len(), 1);
        m.clear();
        assert_eq!(m.pane_count(), 0);
    }
}
