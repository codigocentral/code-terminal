//! Registro de hooks e agentes

use super::{
    context::AgentContext,
    error::{HookError, HookErrorExt, HookResult},
    event::AgentEvent,
    types::{AgentInfo, AgentStatus, AgentType},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::SystemTime;
use tracing::{debug, info};

/// Registro de hooks
#[derive(Debug, Clone)]
pub struct HookRegistry {
    /// Agentes registrados
    agents: Arc<RwLock<HashMap<String, AgentRegistration>>>,
    /// Hooks por tipo de evento
    hooks: Arc<RwLock<HashMap<String, Vec<RegisteredHook>>>>,
    /// Contadores estatísticos
    stats: Arc<RwLock<RegistryStats>>,
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRegistry {
    /// Cria um novo registro
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            hooks: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(RegistryStats::default())),
        }
    }

    /// Registra um novo agente
    pub fn register_agent(&self, registration: AgentRegistration) -> HookResult<()> {
        let agent_id = registration.id.clone();
        let agent_name = registration.info.name.clone();

        info!("Registrando agente: {} ({})", agent_name, agent_id);

        let mut agents = self
            .agents
            .write()
            .map_err(|_| HookError::internal_error("Failed to acquire write lock on agents"))?;

        if agents.contains_key(&agent_id) {
            return Err(HookError::AgentAlreadyRegistered(agent_id));
        }

        agents.insert(agent_id.clone(), registration);

        let mut stats = self
            .stats
            .write()
            .map_err(|_| HookError::internal_error("Failed to acquire write lock on stats"))?;
        stats.total_registrations += 1;
        stats.active_agents += 1;

        debug!("Agente {} registrado com sucesso", agent_name);
        Ok(())
    }

    /// Remove um agente
    pub fn unregister_agent(&self, agent_id: &str) -> HookResult<AgentRegistration> {
        info!("Removendo registro do agente: {}", agent_id);

        let mut agents = self
            .agents
            .write()
            .map_err(|_| HookError::internal_error("Failed to acquire write lock on agents"))?;

        let registration = agents
            .remove(agent_id)
            .ok_or_else(|| HookError::agent_not_found(agent_id))?;

        let mut stats = self
            .stats
            .write()
            .map_err(|_| HookError::internal_error("Failed to acquire write lock on stats"))?;
        stats.active_agents = stats.active_agents.saturating_sub(1);

        info!("Agente {} removido", registration.info.name);
        Ok(registration)
    }

    /// Obtém um agente registrado
    pub fn get_agent(&self, agent_id: &str) -> HookResult<AgentRegistration> {
        let agents = self
            .agents
            .read()
            .map_err(|_| HookError::internal_error("Failed to acquire read lock on agents"))?;

        agents
            .get(agent_id)
            .cloned()
            .ok_or_else(|| HookError::agent_not_found(agent_id))
    }

    /// Lista todos os agentes
    pub fn list_agents(&self) -> HookResult<Vec<AgentRegistration>> {
        let agents = self
            .agents
            .read()
            .map_err(|_| HookError::internal_error("Failed to acquire read lock on agents"))?;

        Ok(agents.values().cloned().collect())
    }

    /// Lista agentes por tipo
    pub fn list_agents_by_type(&self, agent_type: AgentType) -> HookResult<Vec<AgentRegistration>> {
        let agents = self
            .agents
            .read()
            .map_err(|_| HookError::internal_error("Failed to acquire read lock on agents"))?;

        Ok(agents
            .values()
            .filter(|r| r.info.agent_type == agent_type)
            .cloned()
            .collect())
    }

    /// Atualiza status de um agente
    pub fn update_agent_status(&self, agent_id: &str, status: AgentStatus) -> HookResult<()> {
        let mut agents = self
            .agents
            .write()
            .map_err(|_| HookError::internal_error("Failed to acquire write lock on agents"))?;

        let registration = agents
            .get_mut(agent_id)
            .ok_or_else(|| HookError::agent_not_found(agent_id))?;

        registration.info.status = status;
        registration.last_seen = SystemTime::now();

        Ok(())
    }

    /// Registra um hook
    pub fn register_hook(
        &self,
        event_type: &str,
        handler: HookHandler,
        priority: super::types::HookPriority,
    ) -> HookResult<String> {
        let hook_id = uuid::Uuid::new_v4().to_string();

        let registered_hook = RegisteredHook {
            id: hook_id.clone(),
            handler,
            priority,
            registered_at: SystemTime::now(),
        };

        let mut hooks = self
            .hooks
            .write()
            .map_err(|_| HookError::internal_error("Failed to acquire write lock on hooks"))?;

        hooks
            .entry(event_type.to_string())
            .or_default()
            .push(registered_hook);

        // Ordenar por prioridade
        if let Some(hook_list) = hooks.get_mut(event_type) {
            hook_list.sort_by_key(|h| h.priority.value());
        }

        debug!("Hook {} registrado para evento {}", hook_id, event_type);
        Ok(hook_id)
    }

    /// Remove um hook
    pub fn unregister_hook(&self, event_type: &str, hook_id: &str) -> HookResult<()> {
        let mut hooks = self
            .hooks
            .write()
            .map_err(|_| HookError::internal_error("Failed to acquire write lock on hooks"))?;

        if let Some(hook_list) = hooks.get_mut(event_type) {
            hook_list.retain(|h| h.id != hook_id);
        }

        Ok(())
    }

    /// Obtém hooks para um tipo de evento
    pub fn get_hooks(&self, event_type: &str) -> HookResult<Vec<RegisteredHook>> {
        let hooks = self
            .hooks
            .read()
            .map_err(|_| HookError::internal_error("Failed to acquire read lock on hooks"))?;

        Ok(hooks.get(event_type).cloned().unwrap_or_default())
    }

    /// Verifica se há hooks para um evento
    pub fn has_hooks(&self, event_type: &str) -> HookResult<bool> {
        let hooks = self
            .hooks
            .read()
            .map_err(|_| HookError::internal_error("Failed to acquire read lock on hooks"))?;

        Ok(hooks
            .get(event_type)
            .map(|h| !h.is_empty())
            .unwrap_or(false))
    }

    /// Detecta agentes disponíveis no ambiente
    pub fn detect_agents(&self) -> Vec<AgentType> {
        let mut detected = Vec::new();

        for agent_type in [AgentType::ClaudeCode, AgentType::OpenCode, AgentType::Aider] {
            if agent_type.detect() {
                detected.push(agent_type);
            }
        }

        detected
    }

    /// Obtém estatísticas
    pub fn stats(&self) -> HookResult<RegistryStats> {
        let stats = self
            .stats
            .read()
            .map_err(|_| HookError::internal_error("Failed to acquire read lock on stats"))?;

        Ok(*stats)
    }

    /// Limpa agentes inativos
    pub fn cleanup_inactive(&self, max_age_secs: u64) -> HookResult<usize> {
        let threshold = SystemTime::now() - std::time::Duration::from_secs(max_age_secs);

        let mut agents = self
            .agents
            .write()
            .map_err(|_| HookError::internal_error("Failed to acquire write lock on agents"))?;

        let before_count = agents.len();
        agents.retain(|_, reg| reg.last_seen > threshold);
        let removed = before_count - agents.len();

        if removed > 0 {
            info!("Removidos {} agentes inativos", removed);
            let mut stats = self
                .stats
                .write()
                .map_err(|_| HookError::internal_error("Failed to acquire write lock on stats"))?;
            stats.active_agents = stats.active_agents.saturating_sub(removed as u32);
        }

        Ok(removed)
    }
}

/// Registro de um agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRegistration {
    /// ID único
    pub id: String,
    /// Informações do agente
    pub info: AgentInfo,
    /// Endpoint para comunicação (se aplicável)
    pub endpoint: Option<String>,
    /// Chave de API (encriptada em produção)
    pub api_key: Option<String>,
    /// Timestamp de registro
    pub registered_at: SystemTime,
    /// Última vez visto
    pub last_seen: SystemTime,
    /// Configurações
    pub config: HashMap<String, serde_json::Value>,
    /// Permissões
    pub permissions: Vec<String>,
    /// Contexto atual
    #[serde(skip)]
    pub context: Option<AgentContext>,
}

impl AgentRegistration {
    /// Cria um novo registro
    pub fn new(info: AgentInfo) -> Self {
        let now = SystemTime::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            info,
            endpoint: None,
            api_key: None,
            registered_at: now,
            last_seen: now,
            config: HashMap::new(),
            permissions: Vec::new(),
            context: None,
        }
    }

    /// Define endpoint
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Adiciona permissão
    pub fn with_permission(mut self, permission: impl Into<String>) -> Self {
        self.permissions.push(permission.into());
        self
    }

    /// Verifica se tem permissão
    pub fn has_permission(&self, permission: &str) -> bool {
        self.permissions.contains(&permission.to_string())
    }

    /// Atualiza timestamp de last_seen
    pub fn touch(&mut self) {
        self.last_seen = SystemTime::now();
    }
}

/// Handler de hook
#[derive(Clone)]
pub struct HookHandler {
    pub callback: Arc<dyn Fn(&AgentEvent, &AgentContext) -> HookResult<()> + Send + Sync>,
}

impl std::fmt::Debug for HookHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookHandler").finish()
    }
}

impl HookHandler {
    /// Cria um novo handler
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn(&AgentEvent, &AgentContext) -> HookResult<()> + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Executa o handler
    pub fn handle(&self, event: &AgentEvent, context: &AgentContext) -> HookResult<()> {
        (self.callback)(event, context)
    }
}

/// Hook registrado
#[derive(Clone)]
pub struct RegisteredHook {
    /// ID do hook
    pub id: String,
    /// Handler
    pub handler: HookHandler,
    /// Prioridade
    pub priority: super::types::HookPriority,
    /// Timestamp de registro
    pub registered_at: SystemTime,
}

impl std::fmt::Debug for RegisteredHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RegisteredHook")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("registered_at", &self.registered_at)
            .finish()
    }
}

/// Estatísticas do registro
#[derive(Debug, Clone, Copy, Default)]
pub struct RegistryStats {
    /// Total de registros (incluindo removidos)
    pub total_registrations: u64,
    /// Agentes ativos
    pub active_agents: u32,
    /// Hooks registrados
    pub total_hooks: u32,
    /// Eventos processados
    pub events_processed: u64,
    /// Erros
    pub errors: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_registration() {
        let info = AgentInfo::new(AgentType::ClaudeCode, "Test Claude");
        let reg = AgentRegistration::new(info)
            .with_endpoint("http://localhost:8080")
            .with_permission("read")
            .with_permission("write");

        assert!(reg.has_permission("read"));
        assert!(!reg.has_permission("execute"));
        assert!(reg.endpoint.is_some());
    }

    #[test]
    fn test_hook_registry() {
        let registry = HookRegistry::new();

        // Registrar agente
        let info = AgentInfo::new(AgentType::Generic, "Test Agent");
        let reg = AgentRegistration::new(info);
        registry.register_agent(reg.clone()).unwrap();

        // Verificar registro
        let agents = registry.list_agents().unwrap();
        assert_eq!(agents.len(), 1);

        // Atualizar status
        registry
            .update_agent_status(&reg.id, AgentStatus::Active)
            .unwrap();

        let agent = registry.get_agent(&reg.id).unwrap();
        assert_eq!(agent.info.status, AgentStatus::Active);

        // Remover
        registry.unregister_agent(&reg.id).unwrap();
        let agents = registry.list_agents().unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn test_hook_registration() {
        let registry = HookRegistry::new();

        let handler = HookHandler::new(|_event, _context| Ok(()));
        let hook_id = registry
            .register_hook(
                "test_event",
                handler,
                super::super::types::HookPriority::Normal,
            )
            .unwrap();

        let hooks = registry.get_hooks("test_event").unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].id, hook_id);

        // Remover
        registry.unregister_hook("test_event", &hook_id).unwrap();
        let hooks = registry.get_hooks("test_event").unwrap();
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_list_by_type() {
        let registry = HookRegistry::new();

        let claude = AgentRegistration::new(AgentInfo::new(AgentType::ClaudeCode, "Claude"));
        let aider = AgentRegistration::new(AgentInfo::new(AgentType::Aider, "Aider"));

        registry.register_agent(claude).unwrap();
        registry.register_agent(aider).unwrap();

        let claude_agents = registry.list_agents_by_type(AgentType::ClaudeCode).unwrap();
        assert_eq!(claude_agents.len(), 1);
        assert_eq!(claude_agents[0].info.agent_type, AgentType::ClaudeCode);
    }
}
