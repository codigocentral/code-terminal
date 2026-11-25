//! Pane API - Gerenciamento de panes/divisões via socket

use super::types::*;
use super::{ApiError, ApiResult};
// TabId vem de super::types (crate::types) que é u64, não de crate::ui::tabs
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, warn};

/// Estado de um pane
#[derive(Debug, Clone)]
struct PaneState {
    info: PaneInfo,
}

/// API de panes
pub struct PaneApi {
    panes: Arc<RwLock<HashMap<PaneId, PaneState>>>,
    next_pane_id: Arc<RwLock<PaneId>>,
    active_pane: Arc<RwLock<Option<PaneId>>>,
}

impl PaneApi {
    /// Cria uma nova instância da API
    pub fn new() -> Self {
        Self {
            panes: Arc::new(RwLock::new(HashMap::new())),
            next_pane_id: Arc::new(RwLock::new(1)),
            active_pane: Arc::new(RwLock::new(None)),
        }
    }

    /// Lista todos os panes
    pub fn list(&self, tab_id: Option<TabId>) -> ApiResult<Vec<PaneInfo>> {
        let panes = self
            .panes
            .read()
            .map_err(|_| ApiError::InternalError("Failed to acquire read lock".to_string()))?;

        let active = self.active_pane.read().map_err(|_| {
            ApiError::InternalError("Failed to acquire active pane lock".to_string())
        })?;

        let mut list: Vec<PaneInfo> = panes
            .values()
            .filter(|p| tab_id.is_none_or(|tid| p.info.tab_id == tid))
            .map(|p| {
                let mut info = p.info.clone();
                info.is_active = Some(info.id) == *active;
                info
            })
            .collect();

        // Ordena por índice
        list.sort_by_key(|p| p.index);

        debug!("Listed {} panes", list.len());
        Ok(list)
    }

    /// Retorna informações de um pane específico
    pub fn get(&self, id: PaneId) -> ApiResult<PaneInfo> {
        let panes = self
            .panes
            .read()
            .map_err(|_| ApiError::InternalError("Failed to acquire read lock".to_string()))?;

        let active = self.active_pane.read().map_err(|_| {
            ApiError::InternalError("Failed to acquire active pane lock".to_string())
        })?;

        panes
            .get(&id)
            .map(|p| {
                let mut info = p.info.clone();
                info.is_active = Some(info.id) == *active;
                info
            })
            .ok_or_else(|| ApiError::NotFound(format!("Pane not found: {}", id)))
    }

    /// Retorna informações do pane ativo
    pub fn get_active(&self) -> ApiResult<PaneInfo> {
        let active = self.active_pane.read().map_err(|_| {
            ApiError::InternalError("Failed to acquire active pane lock".to_string())
        })?;

        match *active {
            Some(id) => self.get(id),
            None => Err(ApiError::NotFound("No active pane".to_string())),
        }
    }

    /// Divide um pane (cria um novo)
    pub fn split(&self, params: SplitPaneParams, tab_id: TabId) -> ApiResult<PaneInfo> {
        let mut panes = self
            .panes
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        let mut next_id = self.next_pane_id.write().map_err(|_| {
            ApiError::InternalError("Failed to acquire next pane id lock".to_string())
        })?;

        let id = *next_id;
        *next_id += 1;

        // Calcula índice e posição baseado nos panes existentes da tab
        let existing_count = panes.values().filter(|p| p.info.tab_id == tab_id).count();
        let index = existing_count;

        // Calcula tamanho (simplificado - em implementação real, usaria o layout manager)
        let size_percent = params.size_percent.unwrap_or(50) as f32;
        let (width_percent, height_percent) = match params.direction {
            SplitDirection::Vertical => (size_percent, 100.0),
            SplitDirection::Horizontal => (100.0, size_percent),
        };

        let info = PaneInfo {
            id,
            index,
            tab_id,
            title: format!("Pane {}", id),
            is_active: false,
            working_dir: None,
            size: PaneSize {
                cols: 80,
                rows: 24,
                width_percent,
                height_percent,
            },
            position: PanePosition { x: 0, y: 0 },
        };

        panes.insert(id, PaneState { info: info.clone() });

        // Atualiza pane ativo
        drop(panes);
        let mut active = self.active_pane.write().map_err(|_| {
            ApiError::InternalError("Failed to acquire active pane lock".to_string())
        })?;
        *active = Some(id);

        debug!(
            "Created pane: {} in tab {} with {:?} split",
            id, tab_id, params.direction
        );
        // O pane recém-criado passa a ser o ativo; a resposta precisa refletir isso.
        Ok(PaneInfo {
            is_active: true,
            ..info
        })
    }

    /// Fecha um pane
    pub fn close(&self, id: PaneId) -> ApiResult<()> {
        let mut panes = self
            .panes
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        if panes.remove(&id).is_none() {
            return Err(ApiError::NotFound(format!("Pane not found: {}", id)));
        }

        // Se fechou o pane ativo, limpa
        let mut active = self.active_pane.write().map_err(|_| {
            ApiError::InternalError("Failed to acquire active pane lock".to_string())
        })?;

        if *active == Some(id) {
            // Encontra outro pane para ativar
            *active = panes.keys().next().copied();
        }

        // Reordena índices
        let tab_ids: Vec<_> = panes
            .values()
            .map(|p| p.info.tab_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        for tab_id in tab_ids {
            let mut tab_panes: Vec<_> = panes
                .values_mut()
                .filter(|p| p.info.tab_id == tab_id)
                .collect();
            tab_panes.sort_by_key(|p| p.info.index);
            for (new_index, pane) in tab_panes.iter_mut().enumerate() {
                pane.info.index = new_index;
            }
        }

        debug!("Closed pane: {}", id);
        Ok(())
    }

    /// Foca um pane
    pub fn focus(&self, id: PaneId) -> ApiResult<PaneInfo> {
        let panes = self
            .panes
            .read()
            .map_err(|_| ApiError::InternalError("Failed to acquire read lock".to_string()))?;

        if !panes.contains_key(&id) {
            return Err(ApiError::NotFound(format!("Pane not found: {}", id)));
        }

        drop(panes);

        let mut active = self.active_pane.write().map_err(|_| {
            ApiError::InternalError("Failed to acquire active pane lock".to_string())
        })?;

        *active = Some(id);
        drop(active);

        debug!("Focused pane: {}", id);
        self.get(id)
    }

    /// Foca um pane em uma direção
    pub fn focus_direction(&self, direction: Direction, tab_id: TabId) -> ApiResult<PaneInfo> {
        let panes = self
            .panes
            .read()
            .map_err(|_| ApiError::InternalError("Failed to acquire read lock".to_string()))?;

        let active = self.active_pane.read().map_err(|_| {
            ApiError::InternalError("Failed to acquire active pane lock".to_string())
        })?;

        let current_id = active.ok_or_else(|| ApiError::NotFound("No active pane".to_string()))?;

        let current = panes
            .get(&current_id)
            .ok_or_else(|| ApiError::NotFound(format!("Pane not found: {}", current_id)))?;

        // Encontra pane na direção (simplificado)
        // Ordena por índice: o HashMap não tem ordem, e "próximo"/"anterior"
        // precisam ser o vizinho imediato, não um pane qualquer.
        let mut tab_panes: Vec<_> = panes.values().filter(|p| p.info.tab_id == tab_id).collect();
        tab_panes.sort_by_key(|p| p.info.index);

        let target = match direction {
            Direction::Next => tab_panes
                .iter()
                .find(|p| p.info.index > current.info.index)
                .or_else(|| tab_panes.first()),
            Direction::Previous => tab_panes
                .iter()
                .rev()
                .find(|p| p.info.index < current.info.index)
                .or_else(|| tab_panes.last()),
            _ => {
                // Para Up/Down/Left/Right, precisaríamos de coordenadas espaciais
                warn!("Directional focus not fully implemented");
                None
            }
        };

        match target {
            Some(pane) => {
                let id = pane.info.id;
                drop(panes);
                drop(active);
                self.focus(id)
            }
            None => Err(ApiError::NotFound(format!(
                "No pane found in direction {:?}",
                direction
            ))),
        }
    }

    /// Redimensiona um pane
    pub fn resize(&self, id: PaneId, params: ResizePaneParams) -> ApiResult<PaneInfo> {
        let mut panes = self
            .panes
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        let pane = panes
            .get_mut(&id)
            .ok_or_else(|| ApiError::NotFound(format!("Pane not found: {}", id)))?;

        let amount = params.amount.unwrap_or(5);

        match params.direction {
            Direction::Left | Direction::Right => {
                if let Some(size) = params.absolute_size {
                    pane.info.size.cols = size;
                } else {
                    pane.info.size.cols = pane.info.size.cols.saturating_add(amount);
                }
            }
            Direction::Up | Direction::Down => {
                if let Some(size) = params.absolute_size {
                    pane.info.size.rows = size;
                } else {
                    pane.info.size.rows = pane.info.size.rows.saturating_add(amount);
                }
            }
            _ => {}
        }

        debug!("Resized pane: {} in direction {:?}", id, params.direction);
        Ok(pane.info.clone())
    }

    /// Troca dois panes de posição
    pub fn swap(&self, id1: PaneId, id2: PaneId) -> ApiResult<(PaneInfo, PaneInfo)> {
        let mut panes = self
            .panes
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        let pane1 = panes
            .get(&id1)
            .ok_or_else(|| ApiError::NotFound(format!("Pane not found: {}", id1)))?
            .info
            .clone();

        let pane2 = panes
            .get(&id2)
            .ok_or_else(|| ApiError::NotFound(format!("Pane not found: {}", id2)))?
            .info
            .clone();

        // Troca índices
        if let Some(p1) = panes.get_mut(&id1) {
            p1.info.index = pane2.index;
        }
        if let Some(p2) = panes.get_mut(&id2) {
            p2.info.index = pane1.index;
        }

        debug!("Swapped panes: {} and {}", id1, id2);

        let p1 = panes.get(&id1).unwrap().info.clone();
        let p2 = panes.get(&id2).unwrap().info.clone();

        Ok((p1, p2))
    }

    /// Define o layout dos panes
    pub fn set_layout(&self, layout: PaneLayout, tab_id: TabId) -> ApiResult<Vec<PaneInfo>> {
        let mut panes = self
            .panes
            .write()
            .map_err(|_| ApiError::InternalError("Failed to acquire write lock".to_string()))?;

        // Coleta IDs dos panes da tab
        let pane_ids: Vec<PaneId> = panes
            .values()
            .filter(|p| p.info.tab_id == tab_id)
            .map(|p| p.info.id)
            .collect();

        let count = pane_ids.len();
        if count == 0 {
            return Ok(Vec::new());
        }

        match layout {
            PaneLayout::EvenHorizontal => {
                let width = 100.0 / count as f32;
                for id in &pane_ids {
                    if let Some(pane) = panes.get_mut(id) {
                        pane.info.size.width_percent = width;
                        pane.info.size.height_percent = 100.0;
                    }
                }
            }
            PaneLayout::EvenVertical => {
                let height = 100.0 / count as f32;
                for id in &pane_ids {
                    if let Some(pane) = panes.get_mut(id) {
                        pane.info.size.width_percent = 100.0;
                        pane.info.size.height_percent = height;
                    }
                }
            }
            PaneLayout::MainHorizontal => {
                for (i, id) in pane_ids.iter().enumerate() {
                    if let Some(pane) = panes.get_mut(id) {
                        if i == 0 {
                            pane.info.size.width_percent = 100.0;
                            pane.info.size.height_percent = 70.0;
                        } else {
                            pane.info.size.width_percent = 100.0 / (count - 1) as f32;
                            pane.info.size.height_percent = 30.0;
                        }
                    }
                }
            }
            PaneLayout::MainVertical => {
                for (i, id) in pane_ids.iter().enumerate() {
                    if let Some(pane) = panes.get_mut(id) {
                        if i == 0 {
                            pane.info.size.width_percent = 70.0;
                            pane.info.size.height_percent = 100.0;
                        } else {
                            pane.info.size.width_percent = 30.0;
                            pane.info.size.height_percent = 100.0 / (count - 1) as f32;
                        }
                    }
                }
            }
            PaneLayout::Tiled => {
                // Distribui em grid
                let cols = (count as f32).sqrt().ceil() as usize;
                let width = 100.0 / cols as f32;
                let rows = count.div_ceil(cols);
                let height = 100.0 / rows as f32;

                for id in &pane_ids {
                    if let Some(pane) = panes.get_mut(id) {
                        pane.info.size.width_percent = width;
                        pane.info.size.height_percent = height;
                    }
                }
            }
            PaneLayout::Custom(_) => {
                warn!("Custom layouts not yet implemented");
            }
        }

        debug!("Set layout {:?} for tab {}", layout, tab_id);

        let list: Vec<PaneInfo> = panes
            .values()
            .filter(|p| p.info.tab_id == tab_id)
            .map(|p| p.info.clone())
            .collect();

        Ok(list)
    }

    /// Handler para método JSON-RPC
    pub fn handle_method(&self, method: &str, params: Value) -> ApiResult<Value> {
        match method {
            "pane.list" => {
                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId);
                let panes = self.list(tab_id)?;
                serde_json::to_value(panes).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "pane.get" => {
                if let Some(id) = params.get("id").and_then(|v| v.as_u64()) {
                    let pane = self.get(id as PaneId)?;
                    serde_json::to_value(pane).map_err(|e| ApiError::InternalError(e.to_string()))
                } else {
                    let pane = self.get_active()?;
                    serde_json::to_value(pane).map_err(|e| ApiError::InternalError(e.to_string()))
                }
            }
            "pane.split" => {
                let split_params: SplitPaneParams =
                    serde_json::from_value(params.get("params").cloned().unwrap_or(Value::Null))
                        .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId)
                    .unwrap_or(0);
                let pane = self.split(split_params, tab_id)?;
                serde_json::to_value(pane).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "pane.close" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                self.close(id as PaneId)?;
                Ok(Value::Null)
            }
            "pane.focus" => {
                if let Some(id) = params.get("id").and_then(|v| v.as_u64()) {
                    let pane = self.focus(id as PaneId)?;
                    serde_json::to_value(pane).map_err(|e| ApiError::InternalError(e.to_string()))
                } else if let Some(direction_str) = params.get("direction").and_then(|v| v.as_str())
                {
                    let direction: Direction =
                        serde_json::from_value(Value::String(direction_str.to_string()))
                            .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                    let tab_id = params
                        .get("tab_id")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as TabId)
                        .unwrap_or(0);
                    let pane = self.focus_direction(direction, tab_id)?;
                    serde_json::to_value(pane).map_err(|e| ApiError::InternalError(e.to_string()))
                } else {
                    Err(ApiError::InvalidParams(
                        "Missing 'id' or 'direction' parameter".to_string(),
                    ))
                }
            }
            "pane.resize" => {
                let id = params
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .ok_or_else(|| ApiError::InvalidParams("Missing 'id' parameter".to_string()))?;
                let resize_params: ResizePaneParams =
                    serde_json::from_value(params.get("params").cloned().unwrap_or(Value::Null))
                        .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                let pane = self.resize(id as PaneId, resize_params)?;
                serde_json::to_value(pane).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "pane.swap" => {
                let id1 = params.get("id1").and_then(|v| v.as_u64()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'id1' parameter".to_string())
                })?;
                let id2 = params.get("id2").and_then(|v| v.as_u64()).ok_or_else(|| {
                    ApiError::InvalidParams("Missing 'id2' parameter".to_string())
                })?;
                let (pane1, pane2) = self.swap(id1 as PaneId, id2 as PaneId)?;
                serde_json::to_value(vec![pane1, pane2])
                    .map_err(|e| ApiError::InternalError(e.to_string()))
            }
            "pane.layout" => {
                let layout: PaneLayout = serde_json::from_value(
                    params
                        .get("layout")
                        .cloned()
                        .unwrap_or(Value::String("tiled".to_string())),
                )
                .map_err(|e| ApiError::InvalidParams(e.to_string()))?;
                let tab_id = params
                    .get("tab_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as TabId)
                    .unwrap_or(0);
                let panes = self.set_layout(layout, tab_id)?;
                serde_json::to_value(panes).map_err(|e| ApiError::InternalError(e.to_string()))
            }
            _ => Err(ApiError::MethodNotFound(method.to_string())),
        }
    }
}

impl Default for PaneApi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pane_create() {
        let api = PaneApi::new();
        let pane = api
            .split(
                SplitPaneParams {
                    direction: SplitDirection::Vertical,
                    size_percent: Some(50),
                    command: None,
                },
                1,
            )
            .unwrap();

        assert_eq!(pane.index, 0);
        assert!(!pane.title.is_empty());
    }

    #[test]
    fn test_pane_list() {
        let api = PaneApi::new();

        api.split(
            SplitPaneParams {
                direction: SplitDirection::Vertical,
                size_percent: None,
                command: None,
            },
            1,
        )
        .unwrap();

        api.split(
            SplitPaneParams {
                direction: SplitDirection::Horizontal,
                size_percent: None,
                command: None,
            },
            1,
        )
        .unwrap();

        let panes = api.list(Some(1)).unwrap();
        assert_eq!(panes.len(), 2);
    }

    #[test]
    fn test_pane_focus() {
        let api = PaneApi::new();

        let pane1 = api
            .split(
                SplitPaneParams {
                    direction: SplitDirection::Vertical,
                    size_percent: None,
                    command: None,
                },
                1,
            )
            .unwrap();

        let pane2 = api
            .split(
                SplitPaneParams {
                    direction: SplitDirection::Vertical,
                    size_percent: None,
                    command: None,
                },
                1,
            )
            .unwrap();

        api.focus(pane1.id).unwrap();
        let active = api.get_active().unwrap();
        assert_eq!(active.id, pane1.id);

        api.focus(pane2.id).unwrap();
        let active = api.get_active().unwrap();
        assert_eq!(active.id, pane2.id);
    }

    #[test]
    fn test_pane_resize() {
        let api = PaneApi::new();

        let pane = api
            .split(
                SplitPaneParams {
                    direction: SplitDirection::Vertical,
                    size_percent: None,
                    command: None,
                },
                1,
            )
            .unwrap();

        let resized = api
            .resize(
                pane.id,
                ResizePaneParams {
                    direction: Direction::Right,
                    amount: Some(10),
                    absolute_size: None,
                },
            )
            .unwrap();

        assert_eq!(resized.size.cols, 90); // 80 + 10
    }

    #[test]
    fn test_pane_layout() {
        let api = PaneApi::new();

        // Cria alguns panes
        for _ in 0..3 {
            api.split(
                SplitPaneParams {
                    direction: SplitDirection::Vertical,
                    size_percent: None,
                    command: None,
                },
                1,
            )
            .unwrap();
        }

        let panes = api.set_layout(PaneLayout::EvenHorizontal, 1).unwrap();
        assert_eq!(panes.len(), 3);

        // Cada pane deve ter ~33.33% de largura
        for pane in &panes {
            assert!((pane.size.width_percent - 33.33).abs() < 1.0);
        }
    }

    #[test]
    fn test_pane_not_found() {
        let api = PaneApi::new();
        let result = api.get(99999);

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApiError::NotFound(_)));
    }

    fn split_n(api: &PaneApi, tab_id: TabId, n: usize) -> Vec<PaneId> {
        (0..n)
            .map(|_| {
                api.split(
                    SplitPaneParams {
                        direction: SplitDirection::Vertical,
                        size_percent: None,
                        command: None,
                    },
                    tab_id,
                )
                .unwrap()
                .id
            })
            .collect()
    }

    fn call(api: &PaneApi, method: &str, params: Value) -> ApiResult<Value> {
        api.handle_method(method, params)
    }

    #[test]
    fn test_split_horizontal_uses_height_percent() {
        let api = PaneApi::default();
        let pane = api
            .split(
                SplitPaneParams {
                    direction: SplitDirection::Horizontal,
                    size_percent: Some(30),
                    command: None,
                },
                2,
            )
            .unwrap();
        assert_eq!(pane.size.width_percent, 100.0);
        assert_eq!(pane.size.height_percent, 30.0);
        assert_eq!(pane.tab_id, 2);
        assert!(pane.is_active);
        assert_eq!(api.get_active().unwrap().id, pane.id);
    }

    #[test]
    fn test_get_active_without_panes() {
        let api = PaneApi::new();
        assert!(matches!(api.get_active(), Err(ApiError::NotFound(_))));
    }

    #[test]
    fn test_close_reindexes_and_moves_focus() {
        let api = PaneApi::new();
        let ids = split_n(&api, 1, 3);
        let other_tab = split_n(&api, 2, 1)[0];

        api.focus(ids[1]).unwrap();
        api.close(ids[1]).unwrap();

        let tab1 = api.list(Some(1)).unwrap();
        assert_eq!(
            tab1.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![ids[0], ids[2]]
        );
        assert_eq!(tab1.iter().map(|p| p.index).collect::<Vec<_>>(), vec![0, 1]);
        // O foco vai para algum pane que ainda existe
        let active = api.get_active().unwrap().id;
        assert!([ids[0], ids[2], other_tab].contains(&active));

        assert!(matches!(api.close(ids[1]), Err(ApiError::NotFound(_))));

        // Fechar um pane que não está ativo não muda o foco
        api.focus(ids[0]).unwrap();
        api.close(other_tab).unwrap();
        assert_eq!(api.get_active().unwrap().id, ids[0]);
        assert_eq!(api.list(None).unwrap().len(), 2);
    }

    #[test]
    fn test_focus_unknown_pane() {
        let api = PaneApi::new();
        assert!(matches!(api.focus(42), Err(ApiError::NotFound(_))));
    }

    /// Regressão: com os panes vindo de um HashMap, "next"/"previous" podiam
    /// pular para qualquer pane com índice maior/menor, não o vizinho.
    #[test]
    fn test_focus_direction_moves_to_adjacent_pane() {
        let api = PaneApi::new();
        let ids = split_n(&api, 1, 6);

        api.focus(ids[2]).unwrap();
        assert_eq!(api.focus_direction(Direction::Next, 1).unwrap().id, ids[3]);
        assert_eq!(api.focus_direction(Direction::Next, 1).unwrap().id, ids[4]);
        assert_eq!(
            api.focus_direction(Direction::Previous, 1).unwrap().id,
            ids[3]
        );
        assert_eq!(
            api.focus_direction(Direction::Previous, 1).unwrap().id,
            ids[2]
        );

        // Nas pontas, dá a volta
        api.focus(ids[5]).unwrap();
        assert_eq!(api.focus_direction(Direction::Next, 1).unwrap().id, ids[0]);
        assert_eq!(
            api.focus_direction(Direction::Previous, 1).unwrap().id,
            ids[5]
        );

        // Direções espaciais ainda não são suportadas
        assert!(matches!(
            api.focus_direction(Direction::Left, 1),
            Err(ApiError::NotFound(_))
        ));
        // Tab sem panes
        assert!(matches!(
            api.focus_direction(Direction::Next, 99),
            Err(ApiError::NotFound(_))
        ));
    }

    #[test]
    fn test_focus_direction_requires_active_pane() {
        let api = PaneApi::new();
        assert!(matches!(
            api.focus_direction(Direction::Next, 1),
            Err(ApiError::NotFound(message)) if message == "No active pane"
        ));
    }

    #[test]
    fn test_resize_vertical_and_absolute() {
        let api = PaneApi::new();
        let id = split_n(&api, 1, 1)[0];
        let pane = api
            .resize(
                id,
                ResizePaneParams {
                    direction: Direction::Down,
                    amount: None,
                    absolute_size: None,
                },
            )
            .unwrap();
        assert_eq!(pane.size.rows, 29);
        let pane = api
            .resize(
                id,
                ResizePaneParams {
                    direction: Direction::Up,
                    amount: None,
                    absolute_size: Some(10),
                },
            )
            .unwrap();
        assert_eq!(pane.size.rows, 10);
        let pane = api
            .resize(
                id,
                ResizePaneParams {
                    direction: Direction::Left,
                    amount: None,
                    absolute_size: Some(40),
                },
            )
            .unwrap();
        assert_eq!(pane.size.cols, 40);
        // Next/Previous não alteram tamanho
        let pane = api
            .resize(
                id,
                ResizePaneParams {
                    direction: Direction::Next,
                    amount: Some(3),
                    absolute_size: None,
                },
            )
            .unwrap();
        assert_eq!((pane.size.cols, pane.size.rows), (40, 10));
        assert!(api
            .resize(
                999,
                ResizePaneParams {
                    direction: Direction::Up,
                    amount: None,
                    absolute_size: None,
                },
            )
            .is_err());
    }

    #[test]
    fn test_swap_exchanges_indexes() {
        let api = PaneApi::new();
        let ids = split_n(&api, 1, 2);
        let (a, b) = api.swap(ids[0], ids[1]).unwrap();
        assert_eq!((a.id, a.index), (ids[0], 1));
        assert_eq!((b.id, b.index), (ids[1], 0));
        assert!(matches!(api.swap(ids[0], 999), Err(ApiError::NotFound(_))));
        assert!(matches!(api.swap(999, ids[0]), Err(ApiError::NotFound(_))));
    }

    #[test]
    fn test_set_layout_variants() {
        let api = PaneApi::new();
        assert!(api.set_layout(PaneLayout::Tiled, 1).unwrap().is_empty());
        split_n(&api, 1, 3);

        let percents = |panes: Vec<PaneInfo>| -> Vec<(f32, f32)> {
            panes
                .iter()
                .map(|p| (p.size.width_percent, p.size.height_percent))
                .collect()
        };

        let vertical = percents(api.set_layout(PaneLayout::EvenVertical, 1).unwrap());
        assert!(vertical
            .iter()
            .all(|&(w, h)| w == 100.0 && (h - 100.0 / 3.0).abs() < 0.01));

        let tiled = percents(api.set_layout(PaneLayout::Tiled, 1).unwrap());
        assert!(
            tiled.iter().all(|&(w, h)| w == 50.0 && h == 50.0),
            "{tiled:?}"
        );

        // Main*: o "primeiro" pane depende da ordem do HashMap, então contamos
        let horizontal = percents(api.set_layout(PaneLayout::MainHorizontal, 1).unwrap());
        assert_eq!(
            horizontal.iter().filter(|&&p| p == (100.0, 70.0)).count(),
            1
        );
        assert_eq!(horizontal.iter().filter(|&&p| p == (50.0, 30.0)).count(), 2);

        let main_vertical = percents(api.set_layout(PaneLayout::MainVertical, 1).unwrap());
        assert_eq!(
            main_vertical
                .iter()
                .filter(|&&p| p == (70.0, 100.0))
                .count(),
            1
        );
        assert_eq!(
            main_vertical.iter().filter(|&&p| p == (30.0, 50.0)).count(),
            2
        );

        // Custom não muda nada
        let custom = percents(
            api.set_layout(PaneLayout::Custom("x".to_string()), 1)
                .unwrap(),
        );
        assert_eq!(custom, main_vertical);
    }

    #[test]
    fn test_handle_method_routes_all_pane_methods() {
        let api = PaneApi::new();
        let ids = split_n(&api, 1, 2);

        let list = call(&api, "pane.list", serde_json::json!({"tab_id": 1})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 2);
        assert_eq!(
            call(&api, "pane.list", serde_json::json!({"tab_id": 7}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            0
        );

        let got = call(&api, "pane.get", serde_json::json!({"id": ids[0]})).unwrap();
        assert_eq!(got["id"], ids[0]);
        let active = call(&api, "pane.get", serde_json::json!({})).unwrap();
        assert_eq!(active["id"], ids[1]);

        let split = call(
            &api,
            "pane.split",
            serde_json::json!({"params": {"direction": "horizontal"}, "tab_id": 1}),
        )
        .unwrap();
        let third = split["id"].as_u64().unwrap() as PaneId;

        let focused = call(&api, "pane.focus", serde_json::json!({"id": ids[0]})).unwrap();
        assert_eq!(focused["id"], ids[0]);
        let next = call(
            &api,
            "pane.focus",
            serde_json::json!({"direction": "next", "tab_id": 1}),
        )
        .unwrap();
        assert_eq!(next["id"], ids[1]);
        assert!(matches!(
            call(
                &api,
                "pane.focus",
                serde_json::json!({"direction": "sideways"})
            ),
            Err(ApiError::InvalidParams(_))
        ));
        assert!(matches!(
            call(&api, "pane.focus", serde_json::json!({})),
            Err(ApiError::InvalidParams(_))
        ));

        let resized = call(
            &api,
            "pane.resize",
            serde_json::json!({"id": ids[0], "params": {"direction": "right", "amount": 2}}),
        )
        .unwrap();
        assert_eq!(resized["size"]["cols"], 82);
        assert!(matches!(
            call(&api, "pane.resize", serde_json::json!({"params": {}})),
            Err(ApiError::InvalidParams(_))
        ));
        assert!(matches!(
            call(&api, "pane.resize", serde_json::json!({"id": ids[0]})),
            Err(ApiError::InvalidParams(_))
        ));

        let swapped = call(
            &api,
            "pane.swap",
            serde_json::json!({"id1": ids[0], "id2": ids[1]}),
        )
        .unwrap();
        assert_eq!(swapped.as_array().unwrap().len(), 2);
        assert!(matches!(
            call(&api, "pane.swap", serde_json::json!({"id2": 1})),
            Err(ApiError::InvalidParams(_))
        ));
        assert!(matches!(
            call(&api, "pane.swap", serde_json::json!({"id1": 1})),
            Err(ApiError::InvalidParams(_))
        ));

        let layout = call(&api, "pane.layout", serde_json::json!({"tab_id": 1})).unwrap();
        assert_eq!(layout.as_array().unwrap().len(), 3);
        let layout = call(
            &api,
            "pane.layout",
            serde_json::json!({"layout": "even-horizontal", "tab_id": 1}),
        )
        .unwrap();
        assert!(layout
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["size"]["height_percent"] == 100.0));
        assert!(matches!(
            call(&api, "pane.layout", serde_json::json!({"layout": "spiral"})),
            Err(ApiError::InvalidParams(_))
        ));

        call(&api, "pane.close", serde_json::json!({"id": third})).unwrap();
        assert!(matches!(
            call(&api, "pane.close", serde_json::json!({})),
            Err(ApiError::InvalidParams(_))
        ));
    }

    /// Regressão: o método já chega completo ("pane.x"); a mensagem repetia o prefixo.
    #[test]
    fn test_unknown_method_message_is_not_double_prefixed() {
        let api = PaneApi::new();
        match call(&api, "pane.explode", Value::Null) {
            Err(ApiError::MethodNotFound(method)) => assert_eq!(method, "pane.explode"),
            other => panic!("esperava MethodNotFound, veio {:?}", other),
        }
    }
}
