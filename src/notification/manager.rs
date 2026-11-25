//! Notification Manager
//!
//! Gerencia o ciclo de vida das notificações e coordena os diferentes componentes.

use super::{
    config::NotificationConfig,
    desktop::{DesktopNotificationOptions, DesktopNotifier},
    Notification, NotificationEvent, NotificationHandler, NotificationId, NotificationPriority,
    NotificationType, PaneId,
};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use tracing::warn;

/// Gerenciador central de notificações
pub struct NotificationManager {
    /// Configuração
    config: NotificationConfig,
    /// Notificações ativas (não dismissadas)
    active_notifications: VecDeque<Notification>,
    /// Histórico de notificações
    history: VecDeque<Notification>,
    /// Notificador desktop
    desktop_notifier: DesktopNotifier,
    /// Handlers registrados
    handlers: Vec<Box<dyn NotificationHandler>>,
    /// Canal de eventos
    event_sender: Sender<NotificationEvent>,
    /// Receptor de eventos
    event_receiver: Receiver<NotificationEvent>,
    /// Contador de não lidas
    unread_count: usize,
    /// Notificações por painel
    notifications_by_pane: HashMap<PaneId, Vec<NotificationId>>,
}

impl NotificationManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        let (event_sender, event_receiver) = channel();

        Self {
            config: NotificationConfig::default(),
            active_notifications: VecDeque::new(),
            history: VecDeque::new(),
            desktop_notifier: DesktopNotifier::new(),
            handlers: Vec::new(),
            event_sender,
            event_receiver,
            unread_count: 0,
            notifications_by_pane: HashMap::new(),
        }
    }

    /// Cria com configuração customizada
    pub fn with_config(config: NotificationConfig) -> Self {
        let (event_sender, event_receiver) = channel();

        Self {
            config,
            active_notifications: VecDeque::new(),
            history: VecDeque::new(),
            desktop_notifier: DesktopNotifier::new(),
            handlers: Vec::new(),
            event_sender,
            event_receiver,
            unread_count: 0,
            notifications_by_pane: HashMap::new(),
        }
    }

    /// Adiciona uma notificação
    pub fn notify(&mut self, notification: Notification) -> NotificationId {
        let id = notification.id;
        let priority = notification.priority;
        let pane_id = notification.source_pane;
        let notification_type = notification.notification_type.clone();

        // Verifica se o tipo está habilitado
        if !self
            .config
            .is_type_enabled(&format!("{:?}", notification_type).to_lowercase())
        {
            return id;
        }

        // Desktop notification se configurado
        let priority_config = self.config.priority_behavior.for_priority(priority);
        if self.config.desktop_notifications && priority_config.desktop_notification {
            let options = DesktopNotificationOptions {
                enabled: true,
                sound: self.config.sounds && priority_config.sound,
                ..Default::default()
            };
            self.desktop_notifier.notify(&notification, &options);
        }

        // Adiciona às notificações ativas
        if let Some(pane_id) = pane_id {
            self.notifications_by_pane
                .entry(pane_id)
                .or_default()
                .push(id);
        }

        if !notification.is_read {
            self.unread_count += 1;
        }

        self.active_notifications.push_back(notification.clone());

        // Adiciona ao histórico
        self.history.push_back(notification.clone());

        // Limita tamanho do histórico
        while self.history.len() > self.config.max_history_size {
            self.history.pop_front();
        }

        // Envia evento
        self.emit_event(NotificationEvent::NewNotification(notification));
        self.process_events();

        id
    }

    /// Cria uma notificação simples
    pub fn notify_simple(
        &mut self,
        notification_type: NotificationType,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> NotificationId {
        let notification = Notification::new(notification_type, title, message);
        self.notify(notification)
    }

    /// Marca uma notificação como lida
    pub fn mark_read(&mut self, id: NotificationId) -> bool {
        let mut found = false;

        for notification in self.active_notifications.iter_mut() {
            if notification.id == id && !notification.is_read {
                notification.mark_read();
                self.unread_count = self.unread_count.saturating_sub(1);
                found = true;

                self.emit_event(NotificationEvent::NotificationRead(id));
                break;
            }
        }

        if found {
            self.process_events();
        }

        found
    }

    /// Marca todas como lidas
    pub fn mark_all_read(&mut self) {
        for notification in self.active_notifications.iter_mut() {
            if !notification.is_read {
                notification.mark_read();
            }
        }

        self.unread_count = 0;
        self.emit_event(NotificationEvent::AllNotificationsRead);
        self.process_events();
    }

    /// Dismissa uma notificação
    pub fn dismiss(&mut self, id: NotificationId) -> bool {
        let mut found = false;

        if let Some(pos) = self.active_notifications.iter().position(|n| n.id == id) {
            let Some(notification) = self.active_notifications.get_mut(pos) else {
                return false;
            };

            if !notification.is_read {
                self.unread_count = self.unread_count.saturating_sub(1);
            }

            notification.dismiss();
            found = true;

            // Remove do mapeamento por painel
            if let Some(pane_id) = notification.source_pane {
                if let Some(ids) = self.notifications_by_pane.get_mut(&pane_id) {
                    ids.retain(|&nid| nid != id);
                }
            }

            self.emit_event(NotificationEvent::NotificationDismissed(id));
        }

        if found {
            self.process_events();
        }

        found
    }

    /// Dismissa todas as notificações
    pub fn dismiss_all(&mut self) {
        for notification in self.active_notifications.iter_mut() {
            notification.dismiss();
        }

        self.unread_count = 0;
        self.notifications_by_pane.clear();
        self.emit_event(NotificationEvent::AllNotificationsCleared);
        self.process_events();
    }

    /// Limpa notificações antigas
    pub fn cleanup_old(&mut self, max_age_secs: u64) {
        let now = std::time::SystemTime::now();

        self.active_notifications.retain(|n| {
            let should_keep = !n.is_dismissed
                && now
                    .duration_since(n.created_at)
                    .map(|d| d.as_secs() < max_age_secs)
                    .unwrap_or(true);

            if !should_keep && !n.is_read {
                self.unread_count = self.unread_count.saturating_sub(1);
            }

            should_keep
        });
    }

    /// Retorna todas as notificações ativas
    pub fn active_notifications(&self) -> &[Notification] {
        &self.active_notifications
    }

    /// Retorna notificações ativas como vetor
    pub fn get_active_notifications(&self) -> Vec<Notification> {
        self.active_notifications
            .iter()
            .filter(|n| n.is_active())
            .cloned()
            .collect()
    }

    /// Retorna notificações não lidas
    pub fn unread_notifications(&self) -> Vec<Notification> {
        self.active_notifications
            .iter()
            .filter(|n| !n.is_read && n.is_active())
            .cloned()
            .collect()
    }

    /// Retorna notificações por painel
    pub fn notifications_for_pane(&self, pane_id: PaneId) -> Vec<Notification> {
        self.notifications_by_pane
            .get(&pane_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|&id| self.get_notification(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Retorna uma notificação pelo ID
    pub fn get_notification(&self, id: NotificationId) -> Option<Notification> {
        self.active_notifications
            .iter()
            .find(|n| n.id == id)
            .cloned()
    }

    /// Retorna contagem de não lidas
    pub fn unread_count(&self) -> usize {
        self.unread_count
    }

    /// Retorna contagem por prioridade
    pub fn count_by_priority(&self, priority: NotificationPriority) -> usize {
        self.active_notifications
            .iter()
            .filter(|n| n.priority == priority && !n.is_read && n.is_active())
            .count()
    }

    /// Verifica se há notificações não lidas
    pub fn has_unread(&self) -> bool {
        self.unread_count > 0
    }

    /// Retorna a próxima notificação não lida
    pub fn next_unread(&self) -> Option<Notification> {
        self.active_notifications
            .iter()
            .find(|n| !n.is_read && n.is_active())
            .cloned()
    }

    /// Retorna estatísticas
    pub fn stats(&self) -> NotificationStats {
        NotificationStats {
            total_active: self.active_notifications.len(),
            unread: self.unread_count,
            by_priority: [
                (
                    NotificationPriority::Critical,
                    self.count_by_priority(NotificationPriority::Critical),
                ),
                (
                    NotificationPriority::High,
                    self.count_by_priority(NotificationPriority::High),
                ),
                (
                    NotificationPriority::Normal,
                    self.count_by_priority(NotificationPriority::Normal),
                ),
                (
                    NotificationPriority::Low,
                    self.count_by_priority(NotificationPriority::Low),
                ),
            ]
            .into_iter()
            .collect(),
            by_pane: self.notifications_by_pane.len(),
        }
    }

    /// Registra um handler
    pub fn register_handler(&mut self, handler: Box<dyn NotificationHandler>) {
        self.handlers.push(handler);
    }

    /// Processa eventos pendentes
    fn process_events(&mut self) {
        while let Ok(event) = self.event_receiver.try_recv() {
            for handler in &mut self.handlers {
                handler.on_notification_event(event.clone());
            }
        }
    }

    fn emit_event(&self, event: NotificationEvent) {
        if let Err(error) = self.event_sender.send(event) {
            warn!(error = %error, "Notification event receiver is not available");
        }
    }

    /// Atualiza configuração
    pub fn set_config(&mut self, config: NotificationConfig) {
        self.config = config;
    }

    /// Retorna referência à configuração
    pub fn config(&self) -> &NotificationConfig {
        &self.config
    }

    /// Retorna histórico
    pub fn history(&self) -> &[Notification] {
        &self.history
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Estatísticas de notificações
#[derive(Debug, Clone)]
pub struct NotificationStats {
    /// Total de notificações ativas
    pub total_active: usize,
    /// Não lidas
    pub unread: usize,
    /// Por prioridade
    pub by_priority: HashMap<NotificationPriority, usize>,
    /// Número de painéis com notificações
    pub by_pane: usize,
}

impl NotificationStats {
    /// Retorna total de notificações importantes (High + Critical)
    pub fn important_count(&self) -> usize {
        self.by_priority
            .get(&NotificationPriority::High)
            .copied()
            .unwrap_or(0)
            + self
                .by_priority
                .get(&NotificationPriority::Critical)
                .copied()
                .unwrap_or(0)
    }

    /// Retorna se há notificações críticas
    pub fn has_critical(&self) -> bool {
        self.by_priority
            .get(&NotificationPriority::Critical)
            .copied()
            .unwrap_or(0)
            > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_creation() {
        let manager = NotificationManager::new();
        assert_eq!(manager.unread_count(), 0);
        assert!(!manager.has_unread());
    }

    #[test]
    fn test_notify() {
        let mut manager = NotificationManager::new();
        let id = manager.notify_simple(NotificationType::Info, "Test", "Test message");

        assert_eq!(manager.unread_count(), 1);
        assert!(manager.has_unread());
        assert!(manager.get_notification(id).is_some());
    }

    #[test]
    fn test_mark_read() {
        let mut manager = NotificationManager::new();
        let id = manager.notify_simple(NotificationType::Info, "Test", "Test message");

        assert_eq!(manager.unread_count(), 1);
        assert!(manager.mark_read(id));
        assert_eq!(manager.unread_count(), 0);
    }

    #[test]
    fn test_dismiss() {
        let mut manager = NotificationManager::new();
        let id = manager.notify_simple(NotificationType::Info, "Test", "Test message");

        assert!(manager.dismiss(id));
        assert!(!manager.get_notification(id).unwrap().is_active());
    }

    #[test]
    fn test_mark_all_read() {
        let mut manager = NotificationManager::new();

        manager.notify_simple(NotificationType::Info, "Test 1", "Message 1");
        manager.notify_simple(NotificationType::Info, "Test 2", "Message 2");
        manager.notify_simple(NotificationType::Info, "Test 3", "Message 3");

        assert_eq!(manager.unread_count(), 3);
        manager.mark_all_read();
        assert_eq!(manager.unread_count(), 0);
    }

    #[test]
    fn test_next_unread() {
        let mut manager = NotificationManager::new();

        manager.notify_simple(NotificationType::Info, "First", "First message");
        manager.notify_simple(NotificationType::Info, "Second", "Second message");

        let next = manager.next_unread();
        assert!(next.is_some());
        assert_eq!(next.unwrap().title, "First");
    }

    #[test]
    fn test_stats() {
        let mut manager = NotificationManager::new();

        let notif1 = Notification::new(NotificationType::Error, "Error", "Error message")
            .with_priority(NotificationPriority::High);
        let notif2 = Notification::new(NotificationType::Info, "Info", "Info message")
            .with_priority(NotificationPriority::Normal);

        manager.notify(notif1);
        manager.notify(notif2);

        let stats = manager.stats();
        assert_eq!(stats.total_active, 2);
        assert_eq!(stats.unread, 2);
        assert_eq!(stats.important_count(), 1);
    }

    #[test]
    fn test_notifications_for_pane() {
        let mut manager = NotificationManager::new();

        let notif1 =
            Notification::new(NotificationType::Info, "Pane 1", "Message").with_source_pane(1);
        let notif2 =
            Notification::new(NotificationType::Info, "Pane 2", "Message").with_source_pane(2);

        manager.notify(notif1);
        manager.notify(notif2);

        let pane1_notifications = manager.notifications_for_pane(1);
        assert_eq!(pane1_notifications.len(), 1);
        assert_eq!(pane1_notifications[0].title, "Pane 1");
    }

    #[test]
    fn test_cleanup_old() {
        let mut manager = NotificationManager::new();

        manager.notify_simple(NotificationType::Info, "Test", "Test message");

        // Cleanup com tempo muito curto (0 segundos)
        manager.cleanup_old(0);

        // A notificação deve ser removida
        assert_eq!(manager.active_notifications().len(), 0);
    }
}
