//! Notification System
//!
//! Sistema completo de notificações inspirado no cmux.
//!
//! # Refatoração PaneId (Tarefa 1.5)
//! ✅ `PaneId` importado de `crate::types` (u64 unificado)
//! ✅ `NotificationRing` usa `PaneId` corretamente
//! ✅ `NotificationPanel` não usa `PaneId` diretamente
//!
//! # Features
//! - Anéis visuais ao redor dos painéis
//! - Badges na sidebar vertical
//! - Painel centralizado (Cmd+Shift+U)
//! - Suporte a OSC 9/99/777
//! - Notificações desktop
//!
//! # Componentes
//! - `ring`: Anéis visuais ao redor dos painéis
//! - `panel`: Painel centralizado de notificações
//! - `osc`: Parser de sequences OSC
//! - `config`: Configurações

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::types::PaneId;

mod config;
mod osc;
mod panel;
mod ring;

pub use config::*;
pub use osc::*;
pub use panel::*;
pub use ring::*;

/// ID único de notificação
pub type NotificationId = u64;

/// Tipo de notificação
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    /// Informação geral
    Info,
    /// Aviso
    Warning,
    /// Erro
    Error,
    /// Sucesso
    Success,
    /// Sistema
    System,
    /// AI Agent
    AiAgent,
}

impl NotificationType {
    /// Retorna emoji/icon para o tipo
    pub fn icon(&self) -> &'static str {
        match self {
            NotificationType::Info => "ℹ️",
            NotificationType::Warning => "⚠️",
            NotificationType::Error => "❌",
            NotificationType::Success => "✅",
            NotificationType::System => "⚙️",
            NotificationType::AiAgent => "🤖",
        }
    }

    /// Retorna cor associada (RGB)
    pub fn color(&self) -> (u8, u8, u8) {
        match self {
            NotificationType::Info => (100, 149, 237),    // Azul
            NotificationType::Warning => (255, 165, 0),   // Laranja
            NotificationType::Error => (255, 69, 0),      // Vermelho
            NotificationType::Success => (50, 205, 50),   // Verde
            NotificationType::System => (128, 128, 128),  // Cinza
            NotificationType::AiAgent => (147, 112, 219), // Roxo
        }
    }
}

/// Prioridade da notificação
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPriority {
    /// Baixa
    Low,
    /// Normal
    Normal,
    /// Alta
    High,
    /// Crítica
    Critical,
}

impl NotificationPriority {
    /// Retorna cor associada
    pub fn color(&self) -> (u8, u8, u8) {
        match self {
            NotificationPriority::Low => (128, 128, 128), // Cinza
            NotificationPriority::Normal => (100, 149, 237), // Azul
            NotificationPriority::High => (255, 165, 0),  // Laranja
            NotificationPriority::Critical => (255, 69, 0), // Vermelho
        }
    }

    /// Retorna se deve piscar
    pub fn should_blink(&self) -> bool {
        matches!(
            self,
            NotificationPriority::High | NotificationPriority::Critical
        )
    }
}

/// Notificação individual
#[derive(Debug, Clone)]
pub struct Notification {
    /// ID único
    pub id: NotificationId,
    /// Tipo
    pub notification_type: NotificationType,
    /// Prioridade
    pub priority: NotificationPriority,
    /// Título
    pub title: String,
    /// Mensagem
    pub message: String,
    /// Timestamp
    pub timestamp: SystemTime,
    /// Se foi lida
    pub is_read: bool,
    /// ID do plugin (se aplicável)
    pub plugin: Option<String>,
    /// ID do painel (se aplicável)
    pub pane_id: Option<PaneId>,
    /// Dados adicionais
    pub metadata: HashMap<String, String>,
}

impl Notification {
    /// Cria nova notificação
    pub fn new(
        notification_type: NotificationType,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(1);

        Self {
            id: COUNTER.fetch_add(1, Ordering::SeqCst),
            notification_type,
            priority: NotificationPriority::Normal,
            title: title.into(),
            message: message.into(),
            timestamp: SystemTime::now(),
            is_read: false,
            plugin: None,
            pane_id: None,
            metadata: HashMap::new(),
        }
    }

    /// Define prioridade
    pub fn with_priority(mut self, priority: NotificationPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Define plugin
    pub fn with_plugin(mut self, plugin: impl Into<String>) -> Self {
        self.plugin = Some(plugin.into());
        self
    }

    /// Define painel
    pub fn with_pane(mut self, pane_id: PaneId) -> Self {
        self.pane_id = Some(pane_id);
        self
    }

    /// Adiciona metadado
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Marca como lida
    pub fn mark_as_read(&mut self) {
        self.is_read = true;
    }

    /// Marca como não lida
    pub fn mark_as_unread(&mut self) {
        self.is_read = false;
    }

    /// Retorna tempo decorrido desde criação
    pub fn elapsed(&self) -> Duration {
        self.timestamp.elapsed().unwrap_or(Duration::ZERO)
    }
}

/// Gerenciador central de notificações
pub struct NotificationManager {
    /// Configuração
    config: NotificationConfig,
    /// Gerenciador de anéis
    ring_manager: RingManager,
    /// Painel
    panel: NotificationPanel,
    /// Parser OSC
    osc_handler: OscNotificationHandler,
    /// Contador para ID único
    next_id: NotificationId,
    /// Callbacks para notificações
    callbacks: Vec<Box<dyn Fn(&Notification) + Send + Sync>>,
}

impl NotificationManager {
    /// Cria novo gerenciador
    pub fn new() -> Self {
        Self {
            config: NotificationConfig::default(),
            ring_manager: RingManager::new(),
            panel: NotificationPanel::new(),
            osc_handler: OscNotificationHandler::new(),
            next_id: 1,
            callbacks: Vec::new(),
        }
    }

    /// Cria com configuração customizada
    pub fn with_config(config: NotificationConfig) -> Self {
        let enable_rings = config.enable_rings;
        let panel_height = config.panel_height;
        let _show_timestamps = config.show_timestamps;

        let mut manager = Self {
            config,
            ring_manager: RingManager::new(),
            panel: NotificationPanel::new(),
            osc_handler: OscNotificationHandler::new(),
            next_id: 1,
            callbacks: Vec::new(),
        };

        manager.ring_manager.set_enabled(enable_rings);
        manager.panel.set_height(panel_height);

        manager
    }

    /// Gera novo ID
    fn next_id(&mut self) -> NotificationId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Envia notificação
    pub fn notify(&mut self, mut notification: Notification) -> NotificationId {
        // Gera ID se necessário
        if notification.id == 0 {
            notification.id = self.next_id();
        }

        // Verifica configurações
        if !self.config.should_show(
            notification.notification_type,
            notification.priority,
            notification.plugin.as_deref(),
        ) {
            return notification.id;
        }

        // Adiciona ao painel
        if self.config.enable_panel {
            self.panel.add(notification.clone());
        }

        let silent = self.config.filter_action(
            notification.notification_type,
            notification.priority,
            notification.plugin.as_deref(),
        ) == Some(FilterAction::Silent);

        // Adiciona anel ao painel se aplicável (regras Silent não acendem anel)
        if self.config.enable_rings && !silent {
            if let Some(pane_id) = notification.pane_id {
                self.ring_manager.notify(pane_id, &notification);
            }
        }

        // Executa callbacks
        for callback in &self.callbacks {
            callback(&notification);
        }

        notification.id
    }

    /// Envia notificação simples
    pub fn notify_simple(
        &mut self,
        ntype: NotificationType,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> NotificationId {
        let notification = Notification::new(ntype, title, message);
        self.notify(notification)
    }

    /// Processa dados de saída do terminal (para OSC)
    pub fn process_terminal_output(&mut self, data: &str) -> String {
        if !self.config.enable_osc {
            return data.to_string();
        }

        let (cleaned, notifications) = self.osc_handler.extract_all(data);

        // As notificações já vêm com ID único (Notification::new); sobrescrever
        // com o contador do manager colidia com IDs já presentes no painel
        for notification in notifications {
            self.notify(notification);
        }

        cleaned
    }

    /// Marca notificação como lida
    pub fn mark_as_read(&mut self, id: NotificationId) {
        self.panel.mark_as_read(id);

        // Também atualiza o ring se necessário
        // (mantém o ring até todas as notificações serem lidas)
    }

    /// Marca todas como lidas
    pub fn mark_all_as_read(&mut self) {
        self.panel.mark_all_as_read();
    }

    /// Limpa notificações lidas
    pub fn clear_read(&mut self) {
        self.panel.clear_read();
    }

    /// Dismiss (remove) notificação específica
    pub fn dismiss(&mut self, id: NotificationId) {
        if let Some(notification) = self.panel.remove(id) {
            if let Some(pane_id) = notification.pane_id {
                self.ring_manager.dismiss(pane_id, id);
            }
        }
    }

    /// Limpa tudo
    pub fn clear_all(&mut self) {
        self.panel.clear_all();
        self.ring_manager.clear_all();
    }

    /// Alterna visibilidade do painel
    pub fn toggle_panel(&mut self) {
        self.panel.toggle();
    }

    /// Mostra painel
    pub fn show_panel(&mut self) {
        self.panel.show();
    }

    /// Esconde painel
    pub fn hide_panel(&mut self) {
        self.panel.hide();
    }

    /// Pula para próxima não lida
    pub fn jump_to_unread(&mut self) -> Option<Notification> {
        self.panel.jump_to_next_unread().cloned()
    }

    /// Navegação: próxima
    pub fn next_notification(&mut self) {
        self.panel.next();
    }

    /// Navegação: anterior
    pub fn previous_notification(&mut self) {
        self.panel.previous();
    }

    /// Retorna notificações do painel
    pub fn panel_notifications(&self) -> Vec<Notification> {
        self.panel.displayed_notifications()
    }

    /// Retorna anéis ativos
    pub fn active_rings(&self) -> Vec<&NotificationRing> {
        self.ring_manager.active_rings()
    }

    /// Retorna contagem de não lidas
    pub fn unread_count(&self) -> usize {
        self.panel.unread_count()
    }

    /// Verifica se há notificações não lidas
    pub fn has_unread(&self) -> bool {
        self.panel.has_unread()
    }

    /// Verifica se há notificações críticas
    pub fn has_critical(&self) -> bool {
        self.panel.has_critical()
    }

    /// Retorna estatísticas
    pub fn stats(&self) -> NotificationStats {
        let panel_stats = self.panel.stats();
        let ring_stats = self.ring_manager.stats();

        NotificationStats {
            total: panel_stats.total,
            unread: panel_stats.unread,
            critical: panel_stats.critical,
            high: panel_stats.high,
            active_rings: ring_stats.total,
        }
    }

    /// Atualiza configuração
    pub fn set_config(&mut self, config: NotificationConfig) {
        self.ring_manager.set_enabled(config.enable_rings);
        self.panel.set_height(config.panel_height);
        self.config = config;
    }

    /// Retorna configuração
    pub fn config(&self) -> &NotificationConfig {
        &self.config
    }

    /// Habilita/desabilita anéis
    pub fn set_rings_enabled(&mut self, enabled: bool) {
        self.config.enable_rings = enabled;
        self.ring_manager.set_enabled(enabled);
    }

    /// Adiciona callback
    pub fn on_notification<F>(&mut self, callback: F)
    where
        F: Fn(&Notification) + Send + Sync + 'static,
    {
        self.callbacks.push(Box::new(callback));
    }

    /// Retorna referência ao painel
    pub fn panel(&self) -> &NotificationPanel {
        &self.panel
    }

    /// Retorna referência mutável ao painel
    pub fn panel_mut(&mut self) -> &mut NotificationPanel {
        &mut self.panel
    }

    /// Retorna referência ao ring manager
    pub fn ring_manager(&self) -> &RingManager {
        &self.ring_manager
    }

    /// Atualiza estado (chamar a cada frame)
    pub fn update(&mut self) {
        self.ring_manager.update();
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Estatísticas do sistema de notificações
#[derive(Debug, Clone, Copy)]
pub struct NotificationStats {
    /// Total
    pub total: usize,
    /// Não lidas
    pub unread: usize,
    /// Críticas
    pub critical: usize,
    /// Altas
    pub high: usize,
    /// Anéis ativos
    pub active_rings: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_creation() {
        let notification = Notification::new(NotificationType::Info, "Test", "Test message");

        assert_eq!(notification.title, "Test");
        assert_eq!(notification.message, "Test message");
        assert!(!notification.is_read);
        assert_eq!(notification.priority, NotificationPriority::Normal);
    }

    #[test]
    fn test_notification_builder() {
        let notification =
            Notification::new(NotificationType::Error, "Error", "Something went wrong")
                .with_priority(NotificationPriority::High)
                .with_plugin("test-plugin")
                .with_pane(1)
                .with_metadata("key", "value");

        assert_eq!(notification.priority, NotificationPriority::High);
        assert_eq!(notification.plugin, Some("test-plugin".to_string()));
        assert_eq!(notification.pane_id, Some(1));
        assert_eq!(notification.metadata.get("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_notification_mark_as_read() {
        let mut notification = Notification::new(NotificationType::Info, "Test", "Test");
        assert!(!notification.is_read);

        notification.mark_as_read();
        assert!(notification.is_read);

        notification.mark_as_unread();
        assert!(!notification.is_read);
    }

    #[test]
    fn test_notification_type_icon() {
        assert_eq!(NotificationType::Info.icon(), "ℹ️");
        assert_eq!(NotificationType::Warning.icon(), "⚠️");
        assert_eq!(NotificationType::Error.icon(), "❌");
        assert_eq!(NotificationType::Success.icon(), "✅");
        assert_eq!(NotificationType::AiAgent.icon(), "🤖");
    }

    #[test]
    fn test_notification_priority_should_blink() {
        assert!(!NotificationPriority::Low.should_blink());
        assert!(!NotificationPriority::Normal.should_blink());
        assert!(NotificationPriority::High.should_blink());
        assert!(NotificationPriority::Critical.should_blink());
    }

    #[test]
    fn test_manager_notify() {
        let mut manager = NotificationManager::new();

        let id = manager.notify_simple(NotificationType::Info, "Test", "Test message");
        assert!(id > 0);

        assert_eq!(manager.panel.total_count(), 1);
        assert_eq!(manager.unread_count(), 1);
    }

    #[test]
    fn test_manager_mark_as_read() {
        let mut manager = NotificationManager::new();

        let id = manager.notify_simple(NotificationType::Info, "Test", "Test");
        assert_eq!(manager.unread_count(), 1);

        manager.mark_as_read(id);
        assert_eq!(manager.unread_count(), 0);
    }

    #[test]
    fn test_manager_panel_toggle() {
        let mut manager = NotificationManager::new();

        assert!(!manager.panel().is_visible());

        manager.toggle_panel();
        assert!(manager.panel().is_visible());

        manager.hide_panel();
        assert!(!manager.panel().is_visible());
    }

    #[test]
    fn test_manager_process_osc() {
        let mut manager = NotificationManager::new();

        let output = manager.process_terminal_output("\x1b]9;Title|Message\x07");

        // Output deve estar limpo
        assert_eq!(output, "");

        // Notificação deve ter sido criada
        assert_eq!(manager.panel.total_count(), 1);
    }

    #[test]
    fn test_manager_callback() {
        let mut manager = NotificationManager::new();
        let received = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let received_clone = received.clone();

        manager.on_notification(move |_| {
            received_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        manager.notify_simple(NotificationType::Info, "Test", "Test");

        assert!(received.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_manager_stats() {
        let mut manager = NotificationManager::new();

        let notification = Notification::new(NotificationType::Error, "Critical", "Critical")
            .with_priority(NotificationPriority::Critical);

        manager.notify(notification);

        let stats = manager.stats();
        assert_eq!(stats.total, 1);
        assert_eq!(stats.critical, 1);
    }

    #[test]
    fn test_type_and_priority_visuals() {
        let types = [
            (NotificationType::Info, "ℹ️", (100, 149, 237)),
            (NotificationType::Warning, "⚠️", (255, 165, 0)),
            (NotificationType::Error, "❌", (255, 69, 0)),
            (NotificationType::Success, "✅", (50, 205, 50)),
            (NotificationType::System, "⚙️", (128, 128, 128)),
            (NotificationType::AiAgent, "🤖", (147, 112, 219)),
        ];
        for (t, icon, color) in types {
            assert_eq!(t.icon(), icon);
            assert_eq!(t.color(), color);
        }
        assert_eq!(NotificationPriority::Low.color(), (128, 128, 128));
        assert_eq!(NotificationPriority::Normal.color(), (100, 149, 237));
        assert_eq!(NotificationPriority::High.color(), (255, 165, 0));
        assert_eq!(NotificationPriority::Critical.color(), (255, 69, 0));
        assert!(!NotificationPriority::Normal.should_blink());

        let mut n = Notification::new(NotificationType::Info, "t", "m");
        n.mark_as_read();
        n.mark_as_unread();
        assert!(!n.is_read);
        assert!(n.elapsed() < Duration::from_secs(5));
    }

    /// Regressão: notificações OSC recebiam o contador do manager (que começa
    /// em 1) e colidiam com IDs já usados no painel.
    #[test]
    fn test_osc_notifications_get_unique_ids() {
        let mut manager = NotificationManager::new();
        let mut first = Notification::new(NotificationType::Info, "a", "b");
        first.id = 1;
        manager.notify(first);
        manager.process_terminal_output("\x1b]9;osc\x07");

        let ids: Vec<_> = manager
            .panel()
            .all_notifications()
            .iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "IDs duplicados: {ids:?}");
    }

    #[test]
    fn test_notify_assigns_id_and_respects_config() {
        let mut manager = NotificationManager::default();
        let mut n = Notification::new(NotificationType::Info, "t", "m");
        n.id = 0;
        let id = manager.notify(n);
        assert_eq!(id, 1);

        // Bloqueada pela configuração: não entra no painel
        let mut config = NotificationConfig::default();
        config
            .type_config
            .set_enabled(NotificationType::Error, false);
        config.enable_rings = false;
        config.panel_height = 1;
        manager.set_config(config);
        assert!(!manager.ring_manager().is_enabled());
        assert_eq!(manager.panel().height(), 3); // mínimo do painel
        manager.notify_simple(NotificationType::Error, "e", "m");
        assert_eq!(manager.panel().total_count(), 1);

        // Painel desabilitado
        let mut config = manager.config().clone();
        config.enable_panel = false;
        manager.set_config(config);
        manager.notify_simple(NotificationType::Info, "x", "y");
        assert_eq!(manager.panel().total_count(), 1);
    }

    #[test]
    fn test_rings_follow_pane_notifications() {
        let mut manager = NotificationManager::with_config(NotificationConfig::default());
        let id = manager.notify(
            Notification::new(NotificationType::Error, "falhou", "build")
                .with_priority(NotificationPriority::Critical)
                .with_pane(7),
        );
        assert_eq!(manager.active_rings().len(), 1);
        assert!(manager.has_critical());
        let stats = manager.stats();
        assert_eq!(stats.active_rings, 1);
        assert_eq!(stats.critical, 1);

        manager.update();
        manager.dismiss(id);
        assert!(manager.active_rings().is_empty());
        assert_eq!(manager.panel().total_count(), 0);

        // Desligar anéis em tempo de execução
        manager.set_rings_enabled(false);
        manager.notify(Notification::new(NotificationType::Info, "a", "b").with_pane(8));
        assert!(manager.active_rings().is_empty());
        assert!(!manager.config().enable_rings);
    }

    #[test]
    fn test_silent_rule_shows_in_panel_without_ring() {
        let mut config = NotificationConfig::default();
        config.add_filter_rule(
            FilterRule::new("silencio")
                .with_types(vec![NotificationType::Info])
                .with_action(FilterAction::Silent),
        );
        let mut manager = NotificationManager::with_config(config);
        manager.notify(Notification::new(NotificationType::Info, "a", "b").with_pane(1));
        assert_eq!(manager.panel().total_count(), 1);
        assert!(manager.active_rings().is_empty());
    }

    #[test]
    fn test_osc_disabled_passes_output_through() {
        let mut config = NotificationConfig::default();
        config.enable_osc = false;
        let mut manager = NotificationManager::with_config(config);
        let data = "\x1b]9;oi\x07";
        assert_eq!(manager.process_terminal_output(data), data);
        assert_eq!(manager.unread_count(), 0);
    }

    #[test]
    fn test_manager_panel_navigation_and_cleanup() {
        let mut manager = NotificationManager::new();
        let a = manager.notify_simple(NotificationType::Info, "a", "1");
        let b = manager.notify_simple(NotificationType::Info, "b", "2");
        manager.show_panel();
        assert!(manager.panel().is_visible());

        manager.next_notification();
        assert_eq!(manager.panel().selected_index(), Some(0));
        manager.previous_notification();
        assert_eq!(manager.panel().selected_index(), Some(1));

        manager.mark_as_read(a);
        let unread = manager.jump_to_unread().unwrap();
        assert_eq!(unread.id, b);

        manager.clear_read();
        assert_eq!(manager.panel().total_count(), 1);
        manager.mark_all_as_read();
        assert!(!manager.has_unread());

        manager.panel_mut().set_height(20);
        assert_eq!(manager.panel().height(), 20);
        assert_eq!(manager.panel_notifications().len(), 1);

        manager.hide_panel();
        assert!(!manager.panel().is_visible());
        manager.clear_all();
        assert_eq!(manager.stats().total, 0);
    }
}
