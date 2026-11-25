//! Session Groups - Organização de sessões em grupos

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::session::SessionId;
use super::{MultiplexerError, MultiplexerResult};

/// ID de um grupo
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GroupId(pub uuid::Uuid);

impl GroupId {
    /// Cria um novo ID de grupo
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}

impl Default for GroupId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string().split('-').next().unwrap_or(""))
    }
}

/// Um grupo de sessões
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionGroup {
    /// ID do grupo
    pub id: GroupId,
    /// Nome do grupo
    pub name: String,
    /// Descrição
    pub description: Option<String>,
    /// Cor do grupo (hex)
    pub color: Option<String>,
    /// IDs das sessões no grupo
    pub session_ids: HashSet<SessionId>,
    /// Grupo é expandido (UI)
    pub expanded: bool,
    /// Ordem de exibição
    pub sort_order: i32,
    /// Quando foi criado
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Metadados
    pub metadata: HashMap<String, String>,
}

impl SessionGroup {
    /// Cria um novo grupo
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: GroupId::new(),
            name: name.into(),
            description: None,
            color: None,
            session_ids: HashSet::new(),
            expanded: true,
            sort_order: 0,
            created_at: chrono::Utc::now(),
            metadata: HashMap::new(),
        }
    }

    /// Define a descrição
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Define a cor
    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Adiciona uma sessão ao grupo
    pub fn add_session(&mut self, session_id: SessionId) -> bool {
        self.session_ids.insert(session_id)
    }

    /// Remove uma sessão do grupo
    pub fn remove_session(&mut self, session_id: SessionId) -> bool {
        self.session_ids.remove(&session_id)
    }

    /// Verifica se contém uma sessão
    pub fn contains(&self, session_id: SessionId) -> bool {
        self.session_ids.contains(&session_id)
    }

    /// Retorna o número de sessões
    pub fn session_count(&self) -> usize {
        self.session_ids.len()
    }

    /// Verifica se está vazio
    pub fn is_empty(&self) -> bool {
        self.session_ids.is_empty()
    }

    /// Renomeia o grupo
    pub fn rename(&mut self, new_name: impl Into<String>) {
        self.name = new_name.into();
    }

    /// Define a ordem de exibição
    pub fn set_sort_order(&mut self, order: i32) {
        self.sort_order = order;
    }
}

/// Gerenciador de grupos
#[derive(Debug, Default)]
pub struct GroupManager {
    /// Grupos por ID
    groups: HashMap<GroupId, SessionGroup>,
    /// Mapeamento sessão -> grupos
    session_to_groups: HashMap<SessionId, HashSet<GroupId>>,
    /// Grupo padrão (se houver)
    default_group: Option<GroupId>,
}

impl GroupManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            session_to_groups: HashMap::new(),
            default_group: None,
        }
    }

    /// Cria um grupo padrão
    pub fn with_default_group(mut self, name: impl Into<String>) -> Self {
        let group = SessionGroup::new(name);
        let id = group.id;
        self.groups.insert(id, group);
        self.default_group = Some(id);
        self
    }

    /// Cria um novo grupo
    pub fn create_group(&mut self, name: impl Into<String>) -> GroupId {
        let group = SessionGroup::new(name);
        let id = group.id;
        self.groups.insert(id, group);
        id
    }

    /// Obtém um grupo
    pub fn get_group(&self, id: GroupId) -> Option<&SessionGroup> {
        self.groups.get(&id)
    }

    /// Obtém um grupo mutável
    pub fn get_group_mut(&mut self, id: GroupId) -> Option<&mut SessionGroup> {
        self.groups.get_mut(&id)
    }

    /// Remove um grupo
    pub fn remove_group(&mut self, id: GroupId) -> MultiplexerResult<()> {
        let group = self
            .groups
            .remove(&id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(format!("Group {} not found", id)))?;

        // Remove referências
        for session_id in &group.session_ids {
            if let Some(groups) = self.session_to_groups.get_mut(session_id) {
                groups.remove(&id);
            }
        }

        // Remove default se necessário
        if self.default_group == Some(id) {
            self.default_group = None;
        }

        Ok(())
    }

    /// Adiciona uma sessão a um grupo
    pub fn add_to_group(
        &mut self,
        session_id: SessionId,
        group_id: GroupId,
    ) -> MultiplexerResult<()> {
        let group = self.groups.get_mut(&group_id).ok_or_else(|| {
            MultiplexerError::SessionNotFound(format!("Group {} not found", group_id))
        })?;

        group.add_session(session_id);

        self.session_to_groups
            .entry(session_id)
            .or_default()
            .insert(group_id);

        Ok(())
    }

    /// Remove uma sessão de um grupo
    pub fn remove_from_group(
        &mut self,
        session_id: SessionId,
        group_id: GroupId,
    ) -> MultiplexerResult<()> {
        if let Some(group) = self.groups.get_mut(&group_id) {
            group.remove_session(session_id);
        }

        if let Some(groups) = self.session_to_groups.get_mut(&session_id) {
            groups.remove(&group_id);
        }

        Ok(())
    }

    /// Move uma sessão entre grupos
    pub fn move_session(
        &mut self,
        session_id: SessionId,
        from: GroupId,
        to: GroupId,
    ) -> MultiplexerResult<()> {
        self.remove_from_group(session_id, from)?;
        self.add_to_group(session_id, to)?;
        Ok(())
    }

    /// Retorna grupos de uma sessão
    pub fn get_session_groups(&self, session_id: SessionId) -> Vec<&SessionGroup> {
        self.session_to_groups
            .get(&session_id)
            .map(|ids| ids.iter().filter_map(|id| self.groups.get(id)).collect())
            .unwrap_or_default()
    }

    /// Lista todos os grupos
    pub fn list_groups(&self) -> Vec<&SessionGroup> {
        let mut groups: Vec<_> = self.groups.values().collect();
        groups.sort_by_key(|g| g.sort_order);
        groups
    }

    /// Lista grupos ordenados por nome
    pub fn list_groups_by_name(&self) -> Vec<&SessionGroup> {
        let mut groups: Vec<_> = self.groups.values().collect();
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        groups
    }

    /// Retorna sessões de um grupo
    pub fn get_group_sessions(&self, group_id: GroupId) -> Option<&HashSet<SessionId>> {
        self.groups.get(&group_id).map(|g| &g.session_ids)
    }

    /// Define o grupo padrão
    pub fn set_default_group(&mut self, group_id: Option<GroupId>) {
        self.default_group = group_id;
    }

    /// Retorna o grupo padrão
    pub fn default_group(&self) -> Option<GroupId> {
        self.default_group
    }

    /// Adiciona ao grupo padrão
    pub fn add_to_default(&mut self, session_id: SessionId) -> MultiplexerResult<()> {
        if let Some(default) = self.default_group {
            self.add_to_group(session_id, default)?;
        }
        Ok(())
    }

    /// Renomeia um grupo
    pub fn rename_group(
        &mut self,
        id: GroupId,
        new_name: impl Into<String>,
    ) -> MultiplexerResult<()> {
        let group = self
            .groups
            .get_mut(&id)
            .ok_or_else(|| MultiplexerError::SessionNotFound(format!("Group {} not found", id)))?;

        group.rename(new_name);
        Ok(())
    }

    /// Limpa grupos vazios
    pub fn cleanup_empty_groups(&mut self) -> usize {
        let empty: Vec<_> = self
            .groups
            .iter()
            .filter(|(_, g)| g.is_empty())
            .map(|(id, _)| *id)
            .collect();

        let count = empty.len();
        for id in empty {
            let _ = self.remove_group(id);
        }

        count
    }

    /// Exporta grupos
    pub fn export(&self) -> MultiplexerResult<String> {
        serde_json::to_string_pretty(&self.groups).map_err(MultiplexerError::Serialization)
    }

    /// Importa grupos
    pub fn import(&mut self, json: &str) -> MultiplexerResult<usize> {
        let groups: HashMap<GroupId, SessionGroup> =
            serde_json::from_str(json).map_err(MultiplexerError::Serialization)?;

        let count = groups.len();

        for (id, group) in groups {
            self.groups.insert(id, group);
        }

        // Reconstrói mapeamento
        self.rebuild_session_mapping();

        Ok(count)
    }

    /// Reconstrói mapeamento sessão -> grupos
    fn rebuild_session_mapping(&mut self) {
        self.session_to_groups.clear();

        for (group_id, group) in &self.groups {
            for session_id in &group.session_ids {
                self.session_to_groups
                    .entry(*session_id)
                    .or_default()
                    .insert(*group_id);
            }
        }
    }

    /// Retorna estatísticas
    pub fn stats(&self) -> GroupStats {
        GroupStats {
            total_groups: self.groups.len(),
            total_sessions: self.session_to_groups.len(),
            empty_groups: self.groups.values().filter(|g| g.is_empty()).count(),
        }
    }
}

/// Estatísticas de grupos
#[derive(Debug, Clone)]
pub struct GroupStats {
    pub total_groups: usize,
    pub total_sessions: usize,
    pub empty_groups: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_creation() {
        let group = SessionGroup::new("Test Group");
        assert_eq!(group.name, "Test Group");
        assert!(group.is_empty());
        assert!(group.expanded);
    }

    #[test]
    fn test_add_remove_session() {
        let mut group = SessionGroup::new("Test");
        let session_id = SessionId::new();

        assert!(group.add_session(session_id));
        assert_eq!(group.session_count(), 1);
        assert!(group.contains(session_id));

        assert!(group.remove_session(session_id));
        assert!(group.is_empty());
    }

    #[test]
    fn test_group_manager() {
        let mut manager = GroupManager::new();
        let group_id = manager.create_group("My Group");

        let session_id = SessionId::new();
        manager.add_to_group(session_id, group_id).unwrap();

        let groups = manager.get_session_groups(session_id);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "My Group");
    }

    #[test]
    fn test_move_session() {
        let mut manager = GroupManager::new();
        let group1 = manager.create_group("Group 1");
        let group2 = manager.create_group("Group 2");

        let session_id = SessionId::new();
        manager.add_to_group(session_id, group1).unwrap();
        manager.move_session(session_id, group1, group2).unwrap();

        assert!(!manager.get_group(group1).unwrap().contains(session_id));
        assert!(manager.get_group(group2).unwrap().contains(session_id));
    }

    #[test]
    fn test_default_group() {
        let mut manager = GroupManager::with_default_group(GroupManager::new(), "Default");

        let session_id = SessionId::new();
        manager.add_to_default(session_id).unwrap();

        let groups = manager.get_session_groups(session_id);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Default");
    }

    #[test]
    fn test_cleanup_empty_groups() {
        let mut manager = GroupManager::new();
        let _ = manager.create_group("Empty 1");
        let _ = manager.create_group("Empty 2");

        let group3 = manager.create_group("With Session");
        let session_id = SessionId::new();
        manager.add_to_group(session_id, group3).unwrap();

        let removed = manager.cleanup_empty_groups();
        assert_eq!(removed, 2);
        assert_eq!(manager.list_groups().len(), 1);
    }

    #[test]
    fn test_import_export() {
        let mut manager1 = GroupManager::new();
        let group = manager1.create_group("Export Test");
        let session_id = SessionId::new();
        manager1.add_to_group(session_id, group).unwrap();

        let json = manager1.export().unwrap();

        let mut manager2 = GroupManager::new();
        let imported = manager2.import(&json).unwrap();
        assert_eq!(imported, 1);

        let groups = manager2.list_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Export Test");
    }

    #[test]
    fn test_stats() {
        let mut manager = GroupManager::new();
        let _ = manager.create_group("Empty");
        let g2 = manager.create_group("With Sessions");

        manager.add_to_group(SessionId::new(), g2).unwrap();
        manager.add_to_group(SessionId::new(), g2).unwrap();

        let stats = manager.stats();
        assert_eq!(stats.total_groups, 2);
        assert_eq!(stats.empty_groups, 1);
        assert_eq!(stats.total_sessions, 2);
    }
}
