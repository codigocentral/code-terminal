//! Gerenciador de hooks

use super::{
    api::{AgentRequest, AgentResponse},
    context::AgentContext,
    error::{HookError, HookErrorExt, HookResult},
    event::{AgentEvent, EventPriority},
    registry::{AgentRegistration, HookRegistry},
    types::{AgentInfo, AgentStatus, AgentType},
};

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::timeout;
use tracing::{error, info, instrument, trace, warn};

/// Gerenciador de hooks
#[derive(Debug, Clone)]
pub struct HookManager {
    /// Registro interno
    registry: Arc<HookRegistry>,
    /// Configuração
    config: Arc<RwLock<HookConfig>>,
    /// Canal de eventos
    event_tx: mpsc::Sender<AgentEvent>,
    /// Handler de comandos
    command_tx: mpsc::Sender<CommandMessage>,
    /// Estatísticas
    stats: Arc<RwLock<HookStatistics>>,
}

/// Mensagem de comando
#[derive(Debug)]
enum CommandMessage {
    /// Processa o evento e, se houver, avisa no canal quando terminar
    ProcessEvent(AgentEvent, Option<oneshot::Sender<()>>),
    SendRequest {
        agent_id: String,
        request: AgentRequest,
        response_tx: oneshot::Sender<HookResult<AgentResponse>>,
    },
    Shutdown,
}

impl HookManager {
    /// Cria um novo gerenciador
    pub fn new(config: HookConfig) -> Self {
        let (event_tx, mut event_rx) = mpsc::channel::<AgentEvent>(1000);
        let (command_tx, mut command_rx) = mpsc::channel::<CommandMessage>(100);

        let registry = Arc::new(HookRegistry::new());
        let config = Arc::new(RwLock::new(config));
        let stats = Arc::new(RwLock::new(HookStatistics::default()));

        let registry_clone = Arc::clone(&registry);
        let config_clone = Arc::clone(&config);
        let stats_clone = Arc::clone(&stats);

        // Spawn task de processamento
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(event) = event_rx.recv() => {
                        Self::process_event_internal(
                            &registry_clone,
                            &config_clone,
                            &stats_clone,
                            event,
                        ).await;
                    }
                    Some(cmd) = command_rx.recv() => {
                        match cmd {
                            CommandMessage::ProcessEvent(event, done) => {
                                Self::process_event_internal(
                                    &registry_clone,
                                    &config_clone,
                                    &stats_clone,
                                    event,
                                ).await;
                                if let Some(done) = done {
                                    let _ = done.send(());
                                }
                            }
                            CommandMessage::SendRequest { agent_id, request, response_tx } => {
                                let result = Self::send_request_internal(
                                    &registry_clone,
                                    &agent_id,
                                    request,
                                ).await;
                                if response_tx.send(result).is_err() {
                                    warn!(agent_id = %agent_id, "Agent request receiver was dropped before response");
                                }
                            }
                            CommandMessage::Shutdown => {
                                info!("Hook manager shutting down");
                                break;
                            }
                        }
                    }
                    else => break,
                }
            }
        });

        Self {
            registry,
            config,
            event_tx,
            command_tx,
            stats,
        }
    }

    /// Retorna uma cópia da configuração atual.
    pub async fn config(&self) -> HookConfig {
        self.config.read().await.clone()
    }

    /// Cria com configuração padrão
    pub fn default() -> Self {
        Self::new(HookConfig::default())
    }

    /// Registra um agente
    #[instrument(skip(self, registration))]
    pub async fn register_agent(&self, registration: AgentRegistration) -> HookResult<()> {
        info!("Registering agent: {}", registration.info.name);
        self.registry.register_agent(registration)?;

        let mut stats = self.stats.write().await;
        stats.total_agents += 1;

        Ok(())
    }

    /// Remove um agente
    #[instrument(skip(self))]
    pub async fn unregister_agent(&self, agent_id: &str) -> HookResult<AgentRegistration> {
        info!("Unregistering agent: {}", agent_id);
        let reg = self.registry.unregister_agent(agent_id)?;

        let mut stats = self.stats.write().await;
        stats.total_agents = stats.total_agents.saturating_sub(1);

        Ok(reg)
    }

    /// Lista agentes
    pub async fn list_agents(&self) -> HookResult<Vec<AgentRegistration>> {
        self.registry.list_agents()
    }

    /// Obtém agente por tipo
    pub async fn get_agent_by_type(
        &self,
        agent_type: AgentType,
    ) -> HookResult<Option<AgentRegistration>> {
        let agents = self.registry.list_agents_by_type(agent_type)?;
        Ok(agents.into_iter().next())
    }

    /// Atualiza status do agente
    pub async fn update_agent_status(&self, agent_id: &str, status: AgentStatus) -> HookResult<()> {
        self.registry.update_agent_status(agent_id, status)
    }

    /// Emite um evento
    #[instrument(skip(self))]
    pub async fn emit_event(&self, event: AgentEvent) -> HookResult<()> {
        trace!("Emitting event: {:?}", event.event_type);

        self.event_tx
            .send(event)
            .await
            .map_err(|_| HookError::internal_error("Failed to send event"))?;

        Ok(())
    }

    /// Emite evento e aguarda processamento
    pub async fn emit_event_sync(&self, event: AgentEvent) -> HookResult<()> {
        // O sender vai junto com o comando; antes ele ficava preso aqui e a
        // espera sempre durava os 5s inteiros do timeout
        let (tx, rx) = oneshot::channel::<()>();

        self.command_tx
            .send(CommandMessage::ProcessEvent(event, Some(tx)))
            .await
            .map_err(|_| HookError::internal_error("Failed to send command"))?;

        // Aguardar confirmação
        let _ = timeout(Duration::from_secs(5), rx).await;
        Ok(())
    }

    /// Envia requisição para agente
    #[instrument(skip(self, request))]
    pub async fn send_request(
        &self,
        agent_id: &str,
        request: AgentRequest,
    ) -> HookResult<AgentResponse> {
        let (response_tx, response_rx) = oneshot::channel();

        self.command_tx
            .send(CommandMessage::SendRequest {
                agent_id: agent_id.to_string(),
                request,
                response_tx,
            })
            .await
            .map_err(|_| HookError::internal_error("Failed to send command"))?;

        let result = timeout(Duration::from_secs(30), response_rx)
            .await
            .map_err(|_| HookError::timeout(30000))?
            .map_err(|_| HookError::internal_error("Response channel closed"))?;

        result
    }

    /// Envia requisição para agente por tipo
    pub async fn send_request_to_type(
        &self,
        agent_type: AgentType,
        request: AgentRequest,
    ) -> HookResult<AgentResponse> {
        let agent = self.get_agent_by_type(agent_type).await?;

        match agent {
            Some(reg) => self.send_request(&reg.id, request).await,
            None => Err(HookError::AgentNotFound(format!(
                "No agent of type {:?} found",
                agent_type
            ))),
        }
    }

    /// Detecta e registra agentes automaticamente
    pub async fn auto_detect_agents(&self) -> HookResult<Vec<AgentType>> {
        let detected = self.registry.detect_agents();

        for agent_type in &detected {
            // Verificar se já está registrado
            let existing = self.get_agent_by_type(*agent_type).await?;
            if existing.is_none() {
                info!("Auto-detected agent: {:?}", agent_type);

                let info = AgentInfo::new(*agent_type, agent_type.display_name())
                    .with_capability("auto_detected");

                let reg = AgentRegistration::new(info);
                self.register_agent(reg).await?;
            }
        }

        Ok(detected)
    }

    /// Obtém estatísticas
    pub async fn statistics(&self) -> HookStatistics {
        *self.stats.read().await
    }

    /// Limpa agentes inativos
    pub async fn cleanup(&self, max_age_secs: u64) -> HookResult<usize> {
        self.registry.cleanup_inactive(max_age_secs)
    }

    /// Desliga o gerenciador
    pub async fn shutdown(&self) -> HookResult<()> {
        self.command_tx
            .send(CommandMessage::Shutdown)
            .await
            .map_err(|_| HookError::internal_error("Failed to send shutdown"))?;
        Ok(())
    }

    /// Processa evento internamente
    async fn process_event_internal(
        registry: &Arc<HookRegistry>,
        config: &Arc<RwLock<HookConfig>>,
        stats: &Arc<RwLock<HookStatistics>>,
        event: AgentEvent,
    ) {
        let event_type = event_key(&event.event_type);

        // Verificar se há hooks
        let has_hooks = match registry.has_hooks(&event_type) {
            Ok(h) => h,
            Err(e) => {
                error!("Failed to check hooks: {}", e);
                return;
            }
        };

        if !has_hooks {
            trace!("No hooks for event type: {}", event_type);
            return;
        }

        // Obter hooks
        let hooks = match registry.get_hooks(&event_type) {
            Ok(h) => h,
            Err(e) => {
                error!("Failed to get hooks: {}", e);
                return;
            }
        };

        let cfg = config.read().await;

        // Executar hooks
        for hook in hooks {
            // Valor menor = mais prioritário: ignora o que for menos
            // importante que o mínimo (antes a comparação estava invertida e
            // descartava eventos Critical/High)
            if event.priority.value() > cfg.min_priority.value() {
                continue;
            }

            // Criar contexto padrão
            let context = AgentContext::new("default", event.tab_id.unwrap_or(0));

            if let Err(e) = hook.handler.handle(&event, &context) {
                warn!("Hook {} failed: {}", hook.id, e);

                let mut s = stats.write().await;
                s.errors += 1;
            }
        }

        let mut s = stats.write().await;
        s.events_processed += 1;
    }

    /// Envia requisição internamente
    async fn send_request_internal(
        _registry: &Arc<HookRegistry>,
        agent_id: &str,
        _request: AgentRequest,
    ) -> HookResult<AgentResponse> {
        // Implementação real dependeria do tipo de agente e protocolo de comunicação
        // Por enquanto, retorna um mock
        warn!("Request to {} not implemented yet", agent_id);
        Err(HookError::internal_error("Not implemented"))
    }
}

/// Chave usada para registrar hooks: o nome da variante do evento.
///
/// Antes a chave era o `Debug` completo, com os campos
/// (`CommandExecuted { tab_id: 1, command: "ls" }`), e nenhum hook registrado
/// para "CommandExecuted" casava com eventos que carregam dados.
fn event_key(event_type: &super::event::EventType) -> String {
    let debug = format!("{:?}", event_type);
    debug
        .split([' ', '{', '('])
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Configuração do hook manager
#[derive(Debug, Clone)]
pub struct HookConfig {
    /// Prioridade mínima de evento a processar
    pub min_priority: EventPriority,
    /// Timeout padrão para requisições
    pub default_timeout: Duration,
    /// Max idade para cleanup
    pub max_inactive_age: Duration,
    /// Habilitar auto-detecção
    pub auto_detect: bool,
    /// Eventos a ignorar
    pub ignored_events: Vec<String>,
}

impl Default for HookConfig {
    fn default() -> Self {
        Self {
            min_priority: EventPriority::Normal,
            default_timeout: Duration::from_secs(30),
            max_inactive_age: Duration::from_secs(3600), // 1 hora
            auto_detect: true,
            ignored_events: Vec::new(),
        }
    }
}

/// Estatísticas do hook manager
#[derive(Debug, Clone, Copy, Default)]
pub struct HookStatistics {
    /// Total de agentes registrados
    pub total_agents: u32,
    /// Eventos processados
    pub events_processed: u64,
    /// Requisições enviadas
    pub requests_sent: u64,
    /// Respostas recebidas
    pub responses_received: u64,
    /// Erros
    pub errors: u32,
    /// Tempo médio de processamento (ms)
    pub avg_processing_time_ms: u64,
}

impl HookStatistics {
    /// Retorna taxa de sucesso
    pub fn success_rate(&self) -> f64 {
        let total = self.requests_sent as f64;
        if total == 0.0 {
            return 100.0;
        }
        (self.responses_received as f64 / total) * 100.0
    }

    /// Retorna taxa de erro
    pub fn error_rate(&self) -> f64 {
        if self.events_processed == 0 {
            return 0.0;
        }
        (self.errors as f64 / self.events_processed as f64) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::hooks::event::EventType;

    #[tokio::test]
    async fn test_hook_manager_creation() {
        let manager = HookManager::default();
        let stats = manager.statistics().await;
        assert_eq!(stats.total_agents, 0);
    }

    #[tokio::test]
    async fn test_agent_registration() {
        let manager = HookManager::default();

        let info = AgentInfo::new(AgentType::Generic, "Test Agent");
        let reg = AgentRegistration::new(info);

        manager.register_agent(reg).await.unwrap();

        let agents = manager.list_agents().await.unwrap();
        assert_eq!(agents.len(), 1);

        let stats = manager.statistics().await;
        assert_eq!(stats.total_agents, 1);
    }

    #[tokio::test]
    async fn test_event_emission() {
        let manager = HookManager::default();

        let event = AgentEvent::new(EventType::TerminalStart);
        manager.emit_event(event).await.unwrap();

        // Pequena pausa para processamento
        tokio::time::sleep(Duration::from_millis(10)).await;

        let stats = manager.statistics().await;
        // Evento pode ou não ser processado dependendo de hooks registrados
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn test_hook_statistics() {
        let stats = HookStatistics {
            requests_sent: 100,
            responses_received: 95,
            errors: 5,
            events_processed: 1000,
            ..Default::default()
        };

        assert_eq!(stats.success_rate(), 95.0);
        assert_eq!(stats.error_rate(), 0.5);
    }

    #[test]
    fn test_hook_config_default() {
        let config = HookConfig::default();
        assert_eq!(config.min_priority, EventPriority::Normal);
        assert_eq!(config.default_timeout, Duration::from_secs(30));
        assert!(config.auto_detect);
    }

    use crate::ai::hooks::registry::HookHandler;
    use crate::ai::hooks::types::HookPriority;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn cmd_event() -> AgentEvent {
        AgentEvent::new(EventType::CommandExecuted {
            tab_id: 7,
            command: "ls".into(),
        })
    }

    #[test]
    fn test_event_key_is_variant_name() {
        // Regressão: a chave incluía os campos do evento
        assert_eq!(event_key(&EventType::TerminalStart), "TerminalStart");
        assert_eq!(event_key(&cmd_event().event_type), "CommandExecuted");
        assert_eq!(event_key(&EventType::TabClosed { tab_id: 3 }), "TabClosed");
    }

    fn counting_hook(manager: &HookManager, event_type: &str, fail: bool) -> Arc<AtomicUsize> {
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        manager
            .registry
            .register_hook(
                event_type,
                HookHandler::new(move |_, ctx| {
                    assert_eq!(ctx.current_tab_id, 7);
                    c.fetch_add(1, Ordering::SeqCst);
                    if fail {
                        Err(HookError::internal_error("hook falhou"))
                    } else {
                        Ok(())
                    }
                }),
                HookPriority::Normal,
            )
            .unwrap();
        count
    }

    #[tokio::test]
    async fn test_emit_event_sync_returns_after_processing() {
        // Regressão: o sender do oneshot ficava preso na função e a espera
        // sempre durava os 5s do timeout
        let manager = HookManager::default();
        let count = counting_hook(&manager, "CommandExecuted", false);
        let started = std::time::Instant::now();
        manager
            .emit_event_sync(cmd_event().with_tab_id(7))
            .await
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(2));
        // já processado quando retorna
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(manager.statistics().await.events_processed, 1);
    }

    #[tokio::test]
    async fn test_priority_filter_keeps_important_events() {
        // Regressão: a comparação invertida descartava Critical/High e
        // processava Low/Background
        let manager = HookManager::new(HookConfig {
            min_priority: EventPriority::Normal,
            ..Default::default()
        });
        let count = counting_hook(&manager, "CommandExecuted", false);
        for (priority, expected) in [
            (EventPriority::Critical, 1),
            (EventPriority::High, 2),
            (EventPriority::Normal, 3),
            (EventPriority::Low, 3),
            (EventPriority::Background, 3),
        ] {
            let ev = cmd_event().with_priority(priority).with_tab_id(7);
            manager.emit_event_sync(ev).await.unwrap();
            assert_eq!(count.load(Ordering::SeqCst), expected, "{priority:?}");
        }
    }

    #[tokio::test]
    async fn test_failing_hook_counts_errors_and_async_emit() {
        let manager = HookManager::default();
        let count = counting_hook(&manager, "CommandCompleted", true);
        manager
            .emit_event(
                AgentEvent::new(EventType::CommandCompleted {
                    tab_id: 7,
                    command: "make".into(),
                    exit_code: Some(2),
                    duration_ms: 10,
                })
                .with_tab_id(7),
            )
            .await
            .unwrap();
        for _ in 0..200 {
            if manager.statistics().await.events_processed == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let stats = manager.statistics().await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(stats.events_processed, 1);
        assert_eq!(stats.errors, 1);
        assert_eq!(stats.error_rate(), 100.0);

        // evento sem hooks não conta como processado
        manager
            .emit_event_sync(AgentEvent::new(EventType::TerminalStart))
            .await
            .unwrap();
        assert_eq!(manager.statistics().await.events_processed, 1);
    }

    #[tokio::test]
    async fn test_agent_lifecycle_and_requests() {
        let manager = HookManager::default();
        assert!(manager.config().await.auto_detect);

        let reg = AgentRegistration::new(AgentInfo::new(AgentType::Aider, "Aider"));
        let id = reg.id.clone();
        manager.register_agent(reg).await.unwrap();
        let found = manager.get_agent_by_type(AgentType::Aider).await.unwrap();
        assert_eq!(found.unwrap().id, id);
        assert!(manager
            .get_agent_by_type(AgentType::OpenCode)
            .await
            .unwrap()
            .is_none());
        manager
            .update_agent_status(&id, AgentStatus::Busy)
            .await
            .unwrap();
        assert_eq!(
            manager.list_agents().await.unwrap()[0].info.status,
            AgentStatus::Busy
        );

        // o envio ainda não é implementado: o erro volta pelo canal
        let err = manager
            .send_request_to_type(AgentType::Aider, AgentRequest::execute_command("ls"))
            .await
            .unwrap_err();
        assert!(matches!(err, HookError::InternalError(m) if m == "Not implemented"));
        let err = manager
            .send_request_to_type(AgentType::ClaudeCode, AgentRequest::execute_command("ls"))
            .await
            .unwrap_err();
        assert!(matches!(err, HookError::AgentNotFound(_)));

        // agentes recém-registrados não são limpos
        assert_eq!(manager.cleanup(3600).await.unwrap(), 0);

        let removed = manager.unregister_agent(&id).await.unwrap();
        assert_eq!(removed.id, id);
        assert_eq!(manager.statistics().await.total_agents, 0);
        assert!(manager.unregister_agent(&id).await.is_err());
    }

    #[tokio::test]
    async fn test_auto_detect_registers_each_type_once() {
        let manager = HookManager::default();
        let detected = manager.auto_detect_agents().await.unwrap();
        let agents = manager.list_agents().await.unwrap();
        assert_eq!(agents.len(), detected.len());
        // rodar de novo não duplica
        manager.auto_detect_agents().await.unwrap();
        assert_eq!(manager.list_agents().await.unwrap().len(), detected.len());
    }

    #[tokio::test]
    async fn test_shutdown_stops_processing() {
        let manager = HookManager::default();
        manager.shutdown().await.unwrap();
        // a task terminou: os canais fecham e os envios falham
        for _ in 0..200 {
            if manager.shutdown().await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(manager.shutdown().await.is_err());
        assert!(manager
            .emit_event(AgentEvent::new(EventType::TerminalStart))
            .await
            .is_err());
        assert!(manager
            .emit_event_sync(AgentEvent::new(EventType::TerminalStart))
            .await
            .is_err());
        assert!(matches!(
            manager
                .send_request("x", AgentRequest::execute_command("ls"))
                .await,
            Err(HookError::InternalError(_))
        ));
    }

    #[test]
    fn test_statistics_rates_without_data() {
        let stats = HookStatistics::default();
        assert_eq!(stats.success_rate(), 100.0);
        assert_eq!(stats.error_rate(), 0.0);
    }
}
