//! Session Windows - Sistema de janelas do multiplexador
//!
//! Este módulo implementa o sistema de janelas (windows) similar ao tmux,
//! onde cada sessão pode ter múltiplas janelas, e cada janela contém painéis.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::pane::{Pane, PaneId};
use super::{MultiplexerError, MultiplexerResult};

/// ID único de uma janela
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowId(pub uuid::Uuid);

impl WindowId {
    /// Cria um novo ID de janela
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for WindowId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@{}", self.0.to_string().split('-').next().unwrap_or(""))
    }
}

/// Estado de uma janela
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowState {
    /// Ativa (visível)
    Active,
    /// Em background
    Background,
    /// Em zoom (painel maximizado)
    Zoomed,
    /// Destruída
    Destroyed,
}

impl std::fmt::Display for WindowState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowState::Active => write!(f, "active"),
            WindowState::Background => write!(f, "background"),
            WindowState::Zoomed => write!(f, "zoomed"),
            WindowState::Destroyed => write!(f, "destroyed"),
        }
    }
}

/// Uma janela no multiplexador
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    /// ID único
    pub id: WindowId,
    /// Índice da janela na sessão
    pub index: usize,
    /// Nome da janela
    pub name: String,
    /// Estado atual
    pub state: WindowState,
    /// Painéis na janela
    pub panes: HashMap<PaneId, Pane>,
    /// ID do painel ativo
    pub active_pane: Option<PaneId>,
    /// ID do último painel ativo
    pub last_pane: Option<PaneId>,
    /// Tamanho da janela (cols, rows)
    pub size: (u16, u16),
    /// Layout atual
    pub layout: WindowLayout,
    /// Flags
    pub flags: WindowFlags,
    /// Quando foi criada
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Última atividade
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Shell padrão
    pub shell: String,
    /// Diretório de trabalho
    pub working_dir: String,
    /// Metadados
    pub metadata: HashMap<String, String>,
}

/// Flags de uma janela
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WindowFlags {
    /// Janela está silenciada (não notifica)
    pub silent: bool,
    /// Janela está monitorada para atividade
    pub monitor_activity: bool,
    /// Janela está monitorada para silêncio
    pub monitor_silence: bool,
    /// Janela pode ser destruída automaticamente
    pub auto_destroy: bool,
    /// Janela é protegida
    pub protected: bool,
}

/// Layout de janela
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowLayout {
    /// Layout padrão (livre)
    Free,
    /// Horizontal uniforme
    EvenHorizontal,
    /// Vertical uniforme
    EvenVertical,
    /// Principal na horizontal
    MainHorizontal,
    /// Principal na vertical
    MainVertical,
    /// Em tile
    Tiled,
}

impl std::fmt::Display for WindowLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowLayout::Free => write!(f, "free"),
            WindowLayout::EvenHorizontal => write!(f, "even-horizontal"),
            WindowLayout::EvenVertical => write!(f, "even-vertical"),
            WindowLayout::MainHorizontal => write!(f, "main-horizontal"),
            WindowLayout::MainVertical => write!(f, "main-vertical"),
            WindowLayout::Tiled => write!(f, "tiled"),
        }
    }
}

impl Window {
    /// Cria uma nova janela
    pub fn new(name: Option<String>, index: usize, size: (u16, u16)) -> Self {
        let id = WindowId::new();
        let name = name.unwrap_or_else(|| format!("window-{}", index));

        Self {
            id,
            index,
            name,
            state: WindowState::Background,
            panes: HashMap::new(),
            active_pane: None,
            last_pane: None,
            size,
            layout: WindowLayout::Free,
            flags: WindowFlags::default(),
            created_at: chrono::Utc::now(),
            last_activity: chrono::Utc::now(),
            shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string()),
            working_dir: std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            metadata: HashMap::new(),
        }
    }

    /// Cria uma janela com shell padrão
    pub fn with_shell(mut self, shell: impl Into<String>) -> Self {
        self.shell = shell.into();
        self
    }

    /// Define o diretório de trabalho
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = dir.into();
        self
    }

    /// Adiciona um painel à janela
    pub fn add_pane(&mut self, pane: Pane) -> PaneId {
        let id = pane.id;

        // Atualiza last_pane se houver um ativo
        if let Some(active) = self.active_pane {
            self.last_pane = Some(active);
        }

        self.active_pane = Some(id);
        self.panes.insert(id, pane);
        self.touch();

        id
    }

    /// Remove um painel da janela
    pub fn remove_pane(&mut self, pane_id: PaneId) -> MultiplexerResult<Pane> {
        let pane = self
            .panes
            .remove(&pane_id)
            .ok_or_else(|| MultiplexerError::ServerError(format!("Pane {} not found", pane_id)))?;

        // Atualiza active_pane se necessário
        if self.active_pane == Some(pane_id) {
            self.active_pane = self
                .last_pane
                .take()
                .or_else(|| self.panes.keys().next().copied());
        }

        // Se removeu o last_pane, limpa
        if self.last_pane == Some(pane_id) {
            self.last_pane = None;
        }

        self.touch();
        Ok(pane)
    }

    /// Obtém um painel
    pub fn get_pane(&self, pane_id: PaneId) -> Option<&Pane> {
        self.panes.get(&pane_id)
    }

    /// Obtém um painel mutável
    pub fn get_pane_mut(&mut self, pane_id: PaneId) -> Option<&mut Pane> {
        self.panes.get_mut(&pane_id)
    }

    /// Seleciona um painel como ativo
    pub fn select_pane(&mut self, pane_id: PaneId) -> MultiplexerResult<()> {
        if !self.panes.contains_key(&pane_id) {
            return Err(MultiplexerError::ServerError(format!(
                "Pane {} not found",
                pane_id
            )));
        }

        if let Some(current) = self.active_pane {
            self.last_pane = Some(current);
        }

        self.active_pane = Some(pane_id);
        self.touch();
        Ok(())
    }

    /// Seleciona o próximo painel
    pub fn select_next_pane(&mut self) {
        let panes: Vec<_> = self.panes.keys().copied().collect();
        if panes.len() <= 1 {
            return;
        }

        if let Some(current) = self.active_pane {
            if let Some(pos) = panes.iter().position(|&id| id == current) {
                let next_pos = (pos + 1) % panes.len();
                self.last_pane = Some(current);
                self.active_pane = Some(panes[next_pos]);
            }
        }
        self.touch();
    }

    /// Seleciona o painel anterior
    pub fn select_previous_pane(&mut self) {
        let panes: Vec<_> = self.panes.keys().copied().collect();
        if panes.len() <= 1 {
            return;
        }

        if let Some(current) = self.active_pane {
            if let Some(pos) = panes.iter().position(|&id| id == current) {
                let prev_pos = if pos == 0 { panes.len() - 1 } else { pos - 1 };
                self.last_pane = Some(current);
                self.active_pane = Some(panes[prev_pos]);
            }
        }
        self.touch();
    }

    /// Seleciona o último painel ativo
    pub fn select_last_pane(&mut self) {
        if let Some(last) = self.last_pane {
            if self.panes.contains_key(&last) {
                self.active_pane = Some(last);
                self.last_pane = None;
                self.touch();
            }
        }
    }

    /// Retorna o painel ativo
    pub fn active_pane(&self) -> Option<&Pane> {
        self.active_pane.and_then(|id| self.panes.get(&id))
    }

    /// Retorna o painel ativo mutável
    pub fn active_pane_mut(&mut self) -> Option<&mut Pane> {
        self.active_pane.and_then(move |id| self.panes.get_mut(&id))
    }

    /// Retorna o número de painéis
    pub fn pane_count(&self) -> usize {
        self.panes.len()
    }

    /// Retorna se a janela está vazia
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    /// Redimensiona a janela
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = (cols, rows);
        self.apply_layout();
        self.touch();
    }

    /// Define o layout
    pub fn set_layout(&mut self, layout: WindowLayout) {
        self.layout = layout;
        self.apply_layout();
        self.touch();
    }

    /// Aplica o layout atual aos painéis
    fn apply_layout(&mut self) {
        if self.panes.is_empty() {
            return;
        }

        let (cols, rows) = self.size;
        let pane_count = self.panes.len() as u16;

        match self.layout {
            WindowLayout::Free => {
                // Não faz nada, painéis ficam onde estão
            }
            WindowLayout::EvenHorizontal => {
                let height = rows / pane_count;
                for (i, pane) in self.panes.values_mut().enumerate() {
                    pane.position = (0, i as u16 * height);
                    pane.size = (cols, height);
                }
            }
            WindowLayout::EvenVertical => {
                let width = cols / pane_count;
                for (i, pane) in self.panes.values_mut().enumerate() {
                    pane.position = (i as u16 * width, 0);
                    pane.size = (width, rows);
                }
            }
            WindowLayout::MainHorizontal => {
                // Primeiro painel ocupa metade, restante divide o resto
                if let Some(first_id) = self.panes.keys().copied().next() {
                    let main_height = rows / 2;
                    if let Some(pane) = self.panes.get_mut(&first_id) {
                        pane.position = (0, 0);
                        pane.size = (cols, main_height);
                    }

                    let remaining = pane_count - 1;
                    if let Some(sub_height) = main_height.checked_div(remaining) {
                        // Os painéis secundários começam logo abaixo do principal
                        // (índice 1 → posição 0 da área restante).
                        for (i, (_, pane)) in self.panes.iter_mut().enumerate().skip(1) {
                            pane.position = (0, main_height + (i as u16 - 1) * sub_height);
                            pane.size = (cols, sub_height);
                        }
                    }
                }
            }
            WindowLayout::MainVertical => {
                // Primeiro painel ocupa metade, restante divide o resto
                if let Some(first_id) = self.panes.keys().copied().next() {
                    let main_width = cols / 2;
                    if let Some(pane) = self.panes.get_mut(&first_id) {
                        pane.position = (0, 0);
                        pane.size = (main_width, rows);
                    }

                    let remaining = pane_count - 1;
                    if let Some(sub_width) = main_width.checked_div(remaining) {
                        for (i, (_, pane)) in self.panes.iter_mut().enumerate().skip(1) {
                            pane.position = (main_width + (i as u16 - 1) * sub_width, 0);
                            pane.size = (sub_width, rows);
                        }
                    }
                }
            }
            WindowLayout::Tiled => {
                // Layout em grid
                let cols_count = (pane_count as f64).sqrt().ceil() as u16;
                let rows_count = pane_count.div_ceil(cols_count);

                let pane_width = cols / cols_count;
                let pane_height = rows / rows_count;

                for (i, pane) in self.panes.values_mut().enumerate() {
                    let col = i as u16 % cols_count;
                    let row = i as u16 / cols_count;
                    pane.position = (col * pane_width, row * pane_height);
                    pane.size = (pane_width, pane_height);
                }
            }
        }
    }

    /// Alterna o zoom do painel ativo
    pub fn toggle_zoom(&mut self) {
        self.state = match self.state {
            WindowState::Zoomed => WindowState::Active,
            _ => WindowState::Zoomed,
        };
        self.touch();
    }

    /// Ativa a janela
    pub fn activate(&mut self) {
        if self.state != WindowState::Destroyed {
            self.state = WindowState::Active;
            self.touch();
        }
    }

    /// Coloca em background
    pub fn deactivate(&mut self) {
        if self.state == WindowState::Active {
            self.state = WindowState::Background;
        }
    }

    /// Renomeia a janela
    pub fn rename(&mut self, new_name: impl Into<String>) {
        self.name = new_name.into();
        self.touch();
    }

    /// Atualiza a última atividade
    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now();
    }

    /// Retorna o tempo de vida da janela
    pub fn age(&self) -> std::time::Duration {
        let now = chrono::Utc::now();
        now.signed_duration_since(self.created_at)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(0))
    }

    /// Retorna o tempo desde a última atividade
    pub fn idle_time(&self) -> std::time::Duration {
        let now = chrono::Utc::now();
        now.signed_duration_since(self.last_activity)
            .to_std()
            .unwrap_or(std::time::Duration::from_secs(0))
    }

    /// Troca dois painéis de posição
    pub fn swap_panes(&mut self, pane1: PaneId, pane2: PaneId) -> MultiplexerResult<()> {
        let p1 = self
            .panes
            .get(&pane1)
            .ok_or_else(|| MultiplexerError::ServerError(format!("Pane {} not found", pane1)))?;
        let p2 = self
            .panes
            .get(&pane2)
            .ok_or_else(|| MultiplexerError::ServerError(format!("Pane {} not found", pane2)))?;

        let pos1 = p1.position;
        let size1 = p1.size;
        let pos2 = p2.position;
        let size2 = p2.size;

        // Troca posições e tamanhos
        if let Some(p1) = self.panes.get_mut(&pane1) {
            p1.position = pos2;
            p1.size = size2;
        }
        if let Some(p2) = self.panes.get_mut(&pane2) {
            p2.position = pos1;
            p2.size = size1;
        }

        self.touch();
        Ok(())
    }

    /// Divide o painel ativo
    pub fn split_active_pane(
        &mut self,
        vertical: bool,
        size_percent: Option<u8>,
    ) -> MultiplexerResult<PaneId> {
        let active_id = self
            .active_pane
            .ok_or_else(|| MultiplexerError::ServerError("No active pane".to_string()))?;

        let active_pane = self
            .panes
            .get(&active_id)
            .ok_or_else(|| MultiplexerError::ServerError("Active pane not found".to_string()))?;

        let (cols, rows) = active_pane.size;
        let (x, y) = active_pane.position;

        // Calcula o tamanho do novo painel
        let split_percent = size_percent.unwrap_or(50) as f32 / 100.0;

        let (new_pane, updated_size) = if vertical {
            // Divisão vertical (lado a lado)
            let new_width = (cols as f32 * split_percent) as u16;
            let remaining_width = cols - new_width;

            let new_pane = Pane::new((new_width, rows), (x, y));
            (new_pane, (remaining_width, rows))
        } else {
            // Divisão horizontal (em cima/embaxo)
            let new_height = (rows as f32 * split_percent) as u16;
            let remaining_height = rows - new_height;

            let new_pane = Pane::new((cols, new_height), (x, y));
            (new_pane, (cols, remaining_height))
        };

        // Atualiza o painel existente
        if let Some(pane) = self.panes.get_mut(&active_id) {
            pane.size = updated_size;
            if vertical {
                pane.position = (x + cols - updated_size.0, y);
            } else {
                pane.position = (x, y + rows - updated_size.1);
            }
        }

        let new_id = new_pane.id;
        self.add_pane(new_pane);

        Ok(new_id)
    }
}

/// Gerenciador de janelas
#[derive(Debug, Default)]
pub struct WindowManager {
    /// Janelas por ID
    windows: HashMap<WindowId, Window>,
    /// Índice máximo usado
    max_index: usize,
    /// Janela ativa
    active_window: Option<WindowId>,
    /// Última janela ativa
    last_window: Option<WindowId>,
}

impl WindowManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            max_index: 0,
            active_window: None,
            last_window: None,
        }
    }

    /// Cria uma nova janela
    pub fn create_window(&mut self, name: Option<String>, size: (u16, u16)) -> WindowId {
        let index = self.max_index;
        self.max_index += 1;

        let window = Window::new(name, index, size);
        let id = window.id;

        self.windows.insert(id, window);

        // Se for a primeira janela, ativa
        if self.active_window.is_none() {
            self.active_window = Some(id);
            if let Some(w) = self.windows.get_mut(&id) {
                w.activate();
            }
        }

        id
    }

    /// Obtém uma janela
    pub fn get_window(&self, id: WindowId) -> Option<&Window> {
        self.windows.get(&id)
    }

    /// Obtém uma janela mutável
    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        self.windows.get_mut(&id)
    }

    /// Remove uma janela
    pub fn remove_window(&mut self, id: WindowId) -> MultiplexerResult<Window> {
        let window = self
            .windows
            .remove(&id)
            .ok_or_else(|| MultiplexerError::ServerError(format!("Window {} not found", id)))?;

        // Atualiza active_window se necessário
        if self.active_window == Some(id) {
            self.active_window = self
                .last_window
                .take()
                .or_else(|| self.windows.keys().next().copied());

            if let Some(active_id) = self.active_window {
                if let Some(w) = self.windows.get_mut(&active_id) {
                    w.activate();
                }
            }
        }

        if self.last_window == Some(id) {
            self.last_window = None;
        }

        Ok(window)
    }

    /// Lista todas as janelas
    pub fn list_windows(&self) -> Vec<&Window> {
        let mut windows: Vec<_> = self.windows.values().collect();
        windows.sort_by_key(|w| w.index);
        windows
    }

    /// Retorna a janela ativa
    pub fn active_window(&self) -> Option<&Window> {
        self.active_window.and_then(|id| self.windows.get(&id))
    }

    /// Retorna a janela ativa mutável
    pub fn active_window_mut(&mut self) -> Option<&mut Window> {
        self.active_window
            .and_then(move |id| self.windows.get_mut(&id))
    }

    /// Seleciona uma janela
    pub fn select_window(&mut self, id: WindowId) -> MultiplexerResult<()> {
        if !self.windows.contains_key(&id) {
            return Err(MultiplexerError::ServerError(format!(
                "Window {} not found",
                id
            )));
        }

        // Desativa a janela atual
        if let Some(current_id) = self.active_window {
            self.last_window = Some(current_id);
            if let Some(w) = self.windows.get_mut(&current_id) {
                w.deactivate();
            }
        }

        // Ativa a nova janela
        self.active_window = Some(id);
        if let Some(w) = self.windows.get_mut(&id) {
            w.activate();
        }

        Ok(())
    }

    /// Seleciona a próxima janela
    pub fn select_next(&mut self) {
        let windows: Vec<_> = self.list_windows();
        if windows.len() <= 1 {
            return;
        }

        if let Some(current_id) = self.active_window {
            if let Some(pos) = windows.iter().position(|w| w.id == current_id) {
                let next_pos = (pos + 1) % windows.len();
                let next_id = windows[next_pos].id;
                let _ = self.select_window(next_id);
            }
        }
    }

    /// Seleciona a janela anterior
    pub fn select_previous(&mut self) {
        let windows: Vec<_> = self.list_windows();
        if windows.len() <= 1 {
            return;
        }

        if let Some(current_id) = self.active_window {
            if let Some(pos) = windows.iter().position(|w| w.id == current_id) {
                let prev_pos = if pos == 0 { windows.len() - 1 } else { pos - 1 };
                let prev_id = windows[prev_pos].id;
                let _ = self.select_window(prev_id);
            }
        }
    }

    /// Seleciona a última janela
    pub fn select_last(&mut self) {
        if let Some(last_id) = self.last_window {
            let _ = self.select_window(last_id);
        }
    }

    /// Encontra janela por índice
    pub fn find_by_index(&self, index: usize) -> Option<&Window> {
        self.windows.values().find(|w| w.index == index)
    }

    /// Encontra janela por nome
    pub fn find_by_name(&self, name: &str) -> Option<&Window> {
        self.windows.values().find(|w| w.name == name)
    }

    /// Troca duas janelas de posição (índice)
    pub fn swap_windows(&mut self, id1: WindowId, id2: WindowId) -> MultiplexerResult<()> {
        let w1 = self
            .windows
            .get(&id1)
            .ok_or_else(|| MultiplexerError::ServerError(format!("Window {} not found", id1)))?;
        let w2 = self
            .windows
            .get(&id2)
            .ok_or_else(|| MultiplexerError::ServerError(format!("Window {} not found", id2)))?;

        let idx1 = w1.index;
        let idx2 = w2.index;

        if let Some(w) = self.windows.get_mut(&id1) {
            w.index = idx2;
        }
        if let Some(w) = self.windows.get_mut(&id2) {
            w.index = idx1;
        }

        Ok(())
    }

    /// Renomeia uma janela
    pub fn rename_window(
        &mut self,
        id: WindowId,
        new_name: impl Into<String>,
    ) -> MultiplexerResult<()> {
        if let Some(w) = self.windows.get_mut(&id) {
            w.rename(new_name);
            Ok(())
        } else {
            Err(MultiplexerError::ServerError(format!(
                "Window {} not found",
                id
            )))
        }
    }

    /// Retorna o número de janelas
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_creation() {
        let window = Window::new(Some("test".to_string()), 0, (80, 24));
        assert_eq!(window.name, "test");
        assert_eq!(window.index, 0);
        assert_eq!(window.size, (80, 24));
        assert!(window.is_empty());
    }

    #[test]
    fn test_window_manager() {
        let mut manager = WindowManager::new();

        let id1 = manager.create_window(Some("window1".to_string()), (80, 24));
        let id2 = manager.create_window(Some("window2".to_string()), (80, 24));

        assert_eq!(manager.window_count(), 2);

        // A primeira janela criada deve estar ativa
        assert_eq!(manager.active_window().unwrap().id, id1);

        // Seleciona a segunda
        manager.select_window(id2).unwrap();
        assert_eq!(manager.active_window().unwrap().id, id2);

        // Testa select_previous
        manager.select_previous();
        assert_eq!(manager.active_window().unwrap().id, id1);

        // Testa select_next
        manager.select_next();
        assert_eq!(manager.active_window().unwrap().id, id2);
    }

    #[test]
    fn test_find_by_index() {
        let mut manager = WindowManager::new();
        let id = manager.create_window(None, (80, 24));

        let found = manager.find_by_index(0);
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, id);
    }

    #[test]
    fn test_swap_windows() {
        let mut manager = WindowManager::new();
        let id1 = manager.create_window(None, (80, 24));
        let id2 = manager.create_window(None, (80, 24));

        manager.swap_windows(id1, id2).unwrap();

        assert_eq!(manager.get_window(id1).unwrap().index, 1);
        assert_eq!(manager.get_window(id2).unwrap().index, 0);
    }

    fn window_with_panes(count: usize, size: (u16, u16)) -> (Window, Vec<PaneId>) {
        let mut window = Window::new(Some("w".to_string()), 0, size);
        let ids = (0..count)
            .map(|_| window.add_pane(Pane::new(size, (0, 0))))
            .collect();
        (window, ids)
    }

    /// Posição e tamanho dos painéis na ordem em que o layout os percorre.
    fn layout_of(window: &Window) -> Vec<((u16, u16), (u16, u16))> {
        window
            .panes
            .values()
            .map(|p| (p.position, p.size))
            .collect()
    }

    #[test]
    fn test_display_impls() {
        let id = WindowId::default();
        let shown = id.to_string();
        assert!(shown.starts_with('@'));
        assert_eq!(shown.len(), 9, "{shown}");

        assert_eq!(WindowState::Active.to_string(), "active");
        assert_eq!(WindowState::Background.to_string(), "background");
        assert_eq!(WindowState::Zoomed.to_string(), "zoomed");
        assert_eq!(WindowState::Destroyed.to_string(), "destroyed");

        let layouts = [
            (WindowLayout::Free, "free"),
            (WindowLayout::EvenHorizontal, "even-horizontal"),
            (WindowLayout::EvenVertical, "even-vertical"),
            (WindowLayout::MainHorizontal, "main-horizontal"),
            (WindowLayout::MainVertical, "main-vertical"),
            (WindowLayout::Tiled, "tiled"),
        ];
        for (layout, name) in layouts {
            assert_eq!(layout.to_string(), name);
        }
    }

    #[test]
    fn test_builders_and_default_name() {
        let window = Window::new(None, 3, (80, 24))
            .with_shell("/bin/zsh")
            .with_working_dir("/tmp");
        assert_eq!(window.name, "window-3");
        assert_eq!(window.shell, "/bin/zsh");
        assert_eq!(window.working_dir, "/tmp");
    }

    #[test]
    fn test_remove_pane_restores_last_and_clears_references() {
        let (mut window, ids) = window_with_panes(3, (80, 24));
        // add_pane deixa o último ativo e o anterior como "last"
        assert_eq!(window.active_pane, Some(ids[2]));
        assert_eq!(window.last_pane, Some(ids[1]));

        let removed = window.remove_pane(ids[2]).unwrap();
        assert_eq!(removed.id, ids[2]);
        assert_eq!(window.active_pane, Some(ids[1]));
        assert_eq!(window.last_pane, None);

        // Remover o ativo sem "last" escolhe qualquer painel restante
        window.remove_pane(ids[1]).unwrap();
        assert_eq!(window.active_pane, Some(ids[0]));

        // Remover o painel que era "last" limpa a referência
        let extra = window.add_pane(Pane::new((80, 24), (0, 0)));
        assert_eq!(window.last_pane, Some(ids[0]));
        window.remove_pane(ids[0]).unwrap();
        assert_eq!(window.last_pane, None);
        assert_eq!(window.active_pane, Some(extra));

        let error = window.remove_pane(ids[0]).unwrap_err();
        assert!(error.to_string().contains("not found"), "{error}");
    }

    #[test]
    fn test_pane_lookup_and_selection() {
        let (mut window, ids) = window_with_panes(2, (80, 24));
        assert!(window.get_pane(ids[0]).is_some());
        window.get_pane_mut(ids[0]).unwrap().title = "editado".to_string();
        assert_eq!(window.get_pane(ids[0]).unwrap().title, "editado");

        window.select_pane(ids[0]).unwrap();
        assert_eq!(window.active_pane().unwrap().id, ids[0]);
        assert_eq!(window.last_pane, Some(ids[1]));
        window.active_pane_mut().unwrap().title = "ativo".to_string();
        assert_eq!(window.get_pane(ids[0]).unwrap().title, "ativo");

        window.select_last_pane();
        assert_eq!(window.active_pane, Some(ids[1]));
        assert_eq!(window.last_pane, None);
        // Sem "last" não muda nada
        window.select_last_pane();
        assert_eq!(window.active_pane, Some(ids[1]));

        let missing = Pane::new((1, 1), (0, 0)).id;
        assert!(window.select_pane(missing).is_err());
        assert!(window.get_pane(missing).is_none());
    }

    #[test]
    fn test_select_next_and_previous_pane_cycle() {
        let (mut window, ids) = window_with_panes(3, (80, 24));
        let order: Vec<PaneId> = window.panes.keys().copied().collect();
        window.select_pane(order[2]).unwrap();

        window.select_next_pane();
        assert_eq!(window.active_pane, Some(order[0]));
        assert_eq!(window.last_pane, Some(order[2]));
        window.select_previous_pane();
        assert_eq!(window.active_pane, Some(order[2]));
        window.select_previous_pane();
        assert_eq!(window.active_pane, Some(order[1]));

        // Com um painel só, navegar não faz nada
        let (mut single, single_ids) = window_with_panes(1, (80, 24));
        single.select_next_pane();
        single.select_previous_pane();
        assert_eq!(single.active_pane, Some(single_ids[0]));
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn test_even_layouts_split_space_uniformly() {
        let (mut window, _) = window_with_panes(2, (80, 24));
        window.set_layout(WindowLayout::EvenHorizontal);
        let mut layout = layout_of(&window);
        layout.sort();
        assert_eq!(layout, vec![((0, 0), (80, 12)), ((0, 12), (80, 12))]);

        window.set_layout(WindowLayout::EvenVertical);
        let mut layout = layout_of(&window);
        layout.sort();
        assert_eq!(layout, vec![((0, 0), (40, 24)), ((40, 0), (40, 24))]);

        // resize reaplica o layout com o novo tamanho
        window.resize(100, 30);
        assert_eq!(window.size, (100, 30));
        let mut layout = layout_of(&window);
        layout.sort();
        assert_eq!(layout, vec![((0, 0), (50, 30)), ((50, 0), (50, 30))]);
    }

    /// Regressão: o primeiro painel secundário era posicionado uma faixa
    /// abaixo/à direita do esperado (com 2 painéis, fora da janela).
    #[test]
    fn test_main_layouts_place_secondary_panes_inside_window() {
        let (mut window, _) = window_with_panes(3, (80, 24));
        window.set_layout(WindowLayout::MainHorizontal);
        let layout = layout_of(&window);
        assert_eq!(layout[0], ((0, 0), (80, 12)));
        let mut secondary = layout[1..].to_vec();
        secondary.sort();
        assert_eq!(secondary, vec![((0, 12), (80, 6)), ((0, 18), (80, 6))]);

        window.set_layout(WindowLayout::MainVertical);
        let layout = layout_of(&window);
        assert_eq!(layout[0], ((0, 0), (40, 24)));
        let mut secondary = layout[1..].to_vec();
        secondary.sort();
        assert_eq!(secondary, vec![((40, 0), (20, 24)), ((60, 0), (20, 24))]);

        for ((x, y), (w, h)) in layout {
            assert!(x + w <= 80 && y + h <= 24);
        }
    }

    #[test]
    fn test_main_layout_with_single_pane_keeps_only_main() {
        let (mut window, ids) = window_with_panes(1, (80, 24));
        window.set_layout(WindowLayout::MainHorizontal);
        assert_eq!(window.get_pane(ids[0]).unwrap().size, (80, 12));
        window.set_layout(WindowLayout::MainVertical);
        assert_eq!(window.get_pane(ids[0]).unwrap().size, (40, 24));
    }

    #[test]
    fn test_tiled_and_free_layouts() {
        let (mut window, _) = window_with_panes(3, (80, 24));
        window.set_layout(WindowLayout::Tiled);
        let mut layout = layout_of(&window);
        layout.sort();
        assert_eq!(
            layout,
            vec![((0, 0), (40, 12)), ((0, 12), (40, 12)), ((40, 0), (40, 12))]
        );

        // Free mantém as posições atuais
        window.set_layout(WindowLayout::Free);
        let mut free = layout_of(&window);
        free.sort();
        assert_eq!(free, layout);

        // Janela vazia: aplicar layout não faz nada
        let mut empty = Window::new(None, 0, (80, 24));
        empty.set_layout(WindowLayout::Tiled);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_zoom_activate_deactivate_and_rename() {
        let mut window = Window::new(None, 0, (80, 24));
        assert_eq!(window.state, WindowState::Background);
        window.toggle_zoom();
        assert_eq!(window.state, WindowState::Zoomed);
        window.toggle_zoom();
        assert_eq!(window.state, WindowState::Active);
        window.deactivate();
        assert_eq!(window.state, WindowState::Background);
        // deactivate só afeta janelas ativas
        window.state = WindowState::Zoomed;
        window.deactivate();
        assert_eq!(window.state, WindowState::Zoomed);

        window.state = WindowState::Destroyed;
        window.activate();
        assert_eq!(window.state, WindowState::Destroyed);

        window.rename("novo");
        assert_eq!(window.name, "novo");
    }

    #[test]
    fn test_age_and_idle_time_are_clamped() {
        let mut window = Window::new(None, 0, (80, 24));
        let future = chrono::Utc::now() + chrono::Duration::hours(1);
        window.created_at = future;
        window.last_activity = future;
        assert_eq!(window.age(), std::time::Duration::ZERO);
        assert_eq!(window.idle_time(), std::time::Duration::ZERO);

        let past = chrono::Utc::now() - chrono::Duration::seconds(30);
        window.created_at = past;
        window.last_activity = past;
        assert!(window.age() >= std::time::Duration::from_secs(30));
        assert!(window.idle_time() >= std::time::Duration::from_secs(30));
    }

    #[test]
    fn test_window_manager_remove_window_updates_active_and_last() {
        let mut manager = WindowManager::new();
        let a = manager.create_window(Some("a".to_string()), (80, 24));
        let b = manager.create_window(Some("b".to_string()), (80, 24));
        let c = manager.create_window(Some("c".to_string()), (80, 24));

        manager.select_window(b).unwrap();
        manager.select_window(c).unwrap();
        assert_eq!(
            manager.get_window(b).unwrap().state,
            WindowState::Background
        );

        // Remover a ativa volta para a última
        manager.remove_window(c).unwrap();
        assert_eq!(manager.active_window().unwrap().id, b);
        assert_eq!(manager.get_window(b).unwrap().state, WindowState::Active);

        // Remover a "last" (não ativa) só limpa a referência
        manager.select_window(a).unwrap();
        manager.remove_window(b).unwrap();
        manager.select_last();
        assert_eq!(manager.active_window().unwrap().id, a);

        // Remover a ativa sem "last" escolhe qualquer restante (ou nenhuma)
        manager.remove_window(a).unwrap();
        assert!(manager.active_window().is_none());
        assert_eq!(manager.window_count(), 0);

        let error = manager.remove_window(a).unwrap_err();
        assert!(error.to_string().contains("not found"), "{error}");
    }

    #[test]
    fn test_window_manager_remove_active_falls_back_to_any_window() {
        let mut manager = WindowManager::new();
        let a = manager.create_window(None, (80, 24));
        let b = manager.create_window(None, (80, 24));
        manager.remove_window(a).unwrap();
        assert_eq!(manager.active_window().unwrap().id, b);
        assert_eq!(manager.get_window(b).unwrap().state, WindowState::Active);
    }

    #[test]
    fn test_window_manager_lookup_select_and_rename() {
        let mut manager = WindowManager::new();
        assert!(manager.active_window().is_none());
        assert!(manager.active_window_mut().is_none());
        // Sem janelas, navegar é no-op
        manager.select_next();
        manager.select_previous();
        manager.select_last();

        let a = manager.create_window(Some("a".to_string()), (80, 24));
        let b = manager.create_window(Some("b".to_string()), (80, 24));
        manager.get_window_mut(b).unwrap().flags.silent = true;
        assert!(manager.get_window(b).unwrap().flags.silent);

        manager.active_window_mut().unwrap().rename("primeira");
        assert_eq!(manager.find_by_name("primeira").unwrap().id, a);
        assert!(manager.find_by_name("inexistente").is_none());

        manager.rename_window(b, "segunda").unwrap();
        assert_eq!(manager.find_by_name("segunda").unwrap().id, b);
        let ghost = WindowId::new();
        assert!(manager.rename_window(ghost, "x").is_err());
        assert!(manager.select_window(ghost).is_err());
        assert!(manager.swap_windows(a, ghost).is_err());
        assert!(manager.swap_windows(ghost, a).is_err());

        let names: Vec<_> = manager.list_windows().iter().map(|w| w.index).collect();
        assert_eq!(names, vec![0, 1]);

        manager.select_window(b).unwrap();
        manager.select_last();
        assert_eq!(manager.active_window().unwrap().id, a);
        manager.select_next();
        assert_eq!(manager.active_window().unwrap().id, b);
        manager.select_next();
        assert_eq!(manager.active_window().unwrap().id, a);
    }
}
