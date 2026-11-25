//! Notification Rings
//!
//! Implementa anéis visuais ao redor dos painéis quando há notificações.

use super::{Notification, NotificationId, NotificationPriority, PaneId};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Estado de um anel de notificação
#[derive(Debug, Clone)]
pub struct NotificationRing {
    /// ID do painel associado
    pub pane_id: PaneId,
    /// IDs das notificações
    pub notification_ids: Vec<NotificationId>,
    /// Prioridade máxima
    pub max_priority: NotificationPriority,
    /// Cor do anel (RGB)
    pub color: (u8, u8, u8),
    /// Largura do anel em pixels
    pub width: f32,
    /// Opacidade (0.0 - 1.0)
    pub opacity: f32,
    /// Se está piscando
    pub is_blinking: bool,
    /// Taxa de piscada (ms)
    pub blink_rate_ms: u64,
    /// Timestamp de início
    pub started_at: Instant,
    /// Duração total (None = indefinido)
    pub duration: Option<Duration>,
    /// Última atualização do blink
    pub last_blink: Instant,
    /// Estado atual do blink (on/off)
    pub blink_state: bool,
}

impl NotificationRing {
    /// Cria um novo anel de notificação
    pub fn new(pane_id: PaneId) -> Self {
        Self {
            pane_id,
            notification_ids: Vec::new(),
            max_priority: NotificationPriority::Low,
            color: (100, 149, 237), // Azul padrão
            width: 3.0,
            opacity: 1.0,
            is_blinking: false,
            blink_rate_ms: 500,
            started_at: Instant::now(),
            duration: None,
            last_blink: Instant::now(),
            blink_state: true,
        }
    }

    /// Adiciona uma notificação ao anel
    pub fn add_notification(&mut self, notification: &Notification) {
        self.notification_ids.push(notification.id);

        // Atualiza prioridade máxima
        if notification.priority > self.max_priority {
            self.max_priority = notification.priority;
            self.update_appearance();
        }
    }

    /// Remove uma notificação
    pub fn remove_notification(&mut self, id: NotificationId) -> bool {
        self.notification_ids.retain(|&nid| nid != id);

        if self.notification_ids.is_empty() {
            true // Indica que o anel deve ser removido
        } else {
            false
        }
    }

    /// Atualiza aparência baseada na prioridade
    fn update_appearance(&mut self) {
        match self.max_priority {
            NotificationPriority::Critical => {
                self.color = (255, 69, 0); // Vermelho
                self.width = 5.0;
                self.is_blinking = true;
                self.blink_rate_ms = 250;
            }
            NotificationPriority::High => {
                self.color = (255, 165, 0); // Laranja
                self.width = 4.0;
                self.is_blinking = true;
                self.blink_rate_ms = 500;
            }
            NotificationPriority::Normal => {
                self.color = (100, 149, 237); // Azul
                self.width = 3.0;
                self.is_blinking = false;
            }
            NotificationPriority::Low => {
                self.color = (100, 100, 100); // Cinza
                self.width = 2.0;
                self.is_blinking = false;
            }
        }
    }

    /// Atualiza o estado do anel (chamado a cada frame)
    pub fn update(&mut self) {
        if self.is_blinking {
            let elapsed = self.last_blink.elapsed();
            let blink_duration = Duration::from_millis(self.blink_rate_ms);

            if elapsed >= blink_duration {
                self.blink_state = !self.blink_state;
                self.last_blink = Instant::now();
            }
        }
    }

    /// Retorna a opacidade atual considerando o blink
    pub fn current_opacity(&self) -> f32 {
        if self.is_blinking {
            if self.blink_state {
                self.opacity
            } else {
                self.opacity * 0.3 // 30% opacidade quando "off"
            }
        } else {
            self.opacity
        }
    }

    /// Verifica se o anel expirou
    pub fn is_expired(&self) -> bool {
        if let Some(duration) = self.duration {
            self.started_at.elapsed() >= duration
        } else {
            false
        }
    }

    /// Define duração
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = Some(duration);
        self
    }

    /// Define cor customizada
    pub fn with_color(mut self, r: u8, g: u8, b: u8) -> Self {
        self.color = (r, g, b);
        self
    }

    /// Define largura
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

/// Gerenciador de anéis de notificação
pub struct RingManager {
    /// Anéis por painel
    rings: HashMap<PaneId, NotificationRing>,
    /// Anéis globais (não associados a painel)
    global_rings: Vec<NotificationRing>,
    /// Se anéis estão habilitados globalmente
    enabled: bool,
    /// Opacidade padrão
    default_opacity: f32,
}

impl RingManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            rings: HashMap::new(),
            global_rings: Vec::new(),
            enabled: true,
            default_opacity: 1.0,
        }
    }

    /// Habilita/desabilita anéis
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Verifica se está habilitado
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Define a opacidade padrão para novos anéis.
    pub fn set_default_opacity(&mut self, opacity: f32) {
        self.default_opacity = opacity.clamp(0.0, 1.0);
    }

    /// Retorna a opacidade padrão para novos anéis.
    pub fn default_opacity(&self) -> f32 {
        self.default_opacity
    }

    /// Adiciona ou atualiza um anel para um painel
    pub fn notify(&mut self, pane_id: PaneId, notification: &Notification) {
        if !self.enabled {
            return;
        }

        let default_opacity = self.default_opacity;
        let ring = self.rings.entry(pane_id).or_insert_with(|| {
            let mut ring = NotificationRing::new(pane_id);
            ring.opacity = default_opacity;
            ring
        });

        ring.add_notification(notification);
    }

    /// Remove uma notificação de um anel
    pub fn dismiss(&mut self, pane_id: PaneId, notification_id: NotificationId) {
        if let Some(ring) = self.rings.get_mut(&pane_id) {
            if ring.remove_notification(notification_id) {
                // Remove o anel se não há mais notificações
                self.rings.remove(&pane_id);
            }
        }
    }

    /// Marca todas as notificações de um painel como lidas
    pub fn clear_pane(&mut self, pane_id: PaneId) {
        self.rings.remove(&pane_id);
    }

    /// Limpa todos os anéis
    pub fn clear_all(&mut self) {
        self.rings.clear();
        self.global_rings.clear();
    }

    /// Retorna anel para um painel
    pub fn get_ring(&self, pane_id: PaneId) -> Option<&NotificationRing> {
        self.rings.get(&pane_id)
    }

    /// Retorna todos os anéis ativos
    pub fn active_rings(&self) -> Vec<&NotificationRing> {
        if !self.enabled {
            return Vec::new();
        }

        self.rings.values().collect()
    }

    /// Atualiza todos os anéis (chamado a cada frame)
    pub fn update(&mut self) {
        if !self.enabled {
            return;
        }

        // Atualiza anéis e remove os expirados
        let expired_panes: Vec<PaneId> = self
            .rings
            .iter_mut()
            .map(|(id, ring)| {
                ring.update();
                (*id, ring.is_expired())
            })
            .filter(|(_, expired)| *expired)
            .map(|(id, _)| id)
            .collect();

        for pane_id in expired_panes {
            self.rings.remove(&pane_id);
        }
    }

    /// Verifica se um painel tem anel ativo
    pub fn has_ring(&self, pane_id: PaneId) -> bool {
        self.enabled && self.rings.contains_key(&pane_id)
    }

    /// Retorna contagem de anéis ativos
    pub fn active_count(&self) -> usize {
        if self.enabled {
            self.rings.len()
        } else {
            0
        }
    }

    /// Retorna estatísticas
    pub fn stats(&self) -> RingStats {
        let mut critical = 0;
        let mut high = 0;
        let mut normal = 0;
        let mut low = 0;

        for ring in self.rings.values() {
            match ring.max_priority {
                NotificationPriority::Critical => critical += 1,
                NotificationPriority::High => high += 1,
                NotificationPriority::Normal => normal += 1,
                NotificationPriority::Low => low += 1,
            }
        }

        RingStats {
            total: self.rings.len(),
            critical,
            high,
            normal,
            low,
        }
    }
}

impl Default for RingManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Estatísticas de anéis
#[derive(Debug, Clone, Copy)]
pub struct RingStats {
    /// Total de anéis
    pub total: usize,
    /// Anéis críticos
    pub critical: usize,
    /// Anéis altos
    pub high: usize,
    /// Anéis normais
    pub normal: usize,
    /// Anéis baixos
    pub low: usize,
}

impl RingStats {
    /// Retorna total de anéis importantes
    pub fn important_count(&self) -> usize {
        self.critical + self.high
    }

    /// Retorna se há anéis críticos
    pub fn has_critical(&self) -> bool {
        self.critical > 0
    }
}

/// Posição do anel em relação ao painel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingPosition {
    /// Interno (dentro do painel)
    Inner,
    /// Externo (fora do painel)
    Outer,
    /// Sobreposto (na borda)
    Overlay,
}

/// Estado visual do anel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingState {
    /// Estático
    Static,
    /// Piscando
    Blinking,
    /// Pulsando
    Pulsing,
    /// Desaparecendo
    Fading,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_ring_creation() {
        let ring = NotificationRing::new(1);
        assert_eq!(ring.pane_id, 1);
        assert!(ring.notification_ids.is_empty());
        assert_eq!(ring.width, 3.0);
    }

    #[test]
    fn test_add_notification() {
        let mut ring = NotificationRing::new(1);
        let notification =
            Notification::new(super::super::NotificationType::Info, "Test", "Test message")
                .with_priority(NotificationPriority::High);

        ring.add_notification(&notification);

        assert_eq!(ring.notification_ids.len(), 1);
        assert_eq!(ring.max_priority, NotificationPriority::High);
        assert!(ring.is_blinking);
    }

    #[test]
    fn test_remove_notification() {
        let mut ring = NotificationRing::new(1);
        let notification = Notification::new(super::super::NotificationType::Info, "Test", "Test");

        ring.add_notification(&notification);
        assert_eq!(ring.notification_ids.len(), 1);

        // Tentar remover com ID diferente não faz nada
        assert!(!ring.remove_notification(999));
        assert_eq!(ring.notification_ids.len(), 1);

        // Remover com ID correto esvazia o ring
        assert!(ring.remove_notification(notification.id));
        assert!(ring.notification_ids.is_empty());
    }

    #[test]
    fn test_priority_update() {
        let mut ring = NotificationRing::new(1);

        let low = Notification::new(super::super::NotificationType::Info, "Low", "Low priority")
            .with_priority(NotificationPriority::Low);

        let critical = Notification::new(
            super::super::NotificationType::Error,
            "Critical",
            "Critical priority",
        )
        .with_priority(NotificationPriority::Critical);

        ring.add_notification(&low);
        assert_eq!(ring.max_priority, NotificationPriority::Low);
        assert!(!ring.is_blinking);

        ring.add_notification(&critical);
        assert_eq!(ring.max_priority, NotificationPriority::Critical);
        assert!(ring.is_blinking);
    }

    #[test]
    fn test_ring_manager() {
        let mut manager = RingManager::new();
        let notification = Notification::new(super::super::NotificationType::Info, "Test", "Test");

        manager.notify(1, &notification);
        assert!(manager.has_ring(1));
        assert_eq!(manager.active_count(), 1);

        manager.dismiss(1, notification.id);
        assert!(!manager.has_ring(1));
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_ring_manager_disabled() {
        let mut manager = RingManager::new();
        manager.set_enabled(false);

        let notification = Notification::new(super::super::NotificationType::Info, "Test", "Test");

        manager.notify(1, &notification);
        assert!(!manager.has_ring(1));
        assert_eq!(manager.active_count(), 0);
    }

    #[test]
    fn test_ring_manager_default_opacity() {
        let mut manager = RingManager::new();
        manager.set_default_opacity(0.5);

        let notification = Notification::new(super::super::NotificationType::Info, "Test", "Test");

        manager.notify(1, &notification);

        assert_eq!(manager.default_opacity(), 0.5);
        assert_eq!(manager.get_ring(1).unwrap().opacity, 0.5);
    }

    #[test]
    fn test_ring_stats() {
        let mut manager = RingManager::new();

        let critical = Notification::new(
            super::super::NotificationType::Error,
            "Critical",
            "Critical",
        )
        .with_priority(NotificationPriority::Critical);

        let high = Notification::new(super::super::NotificationType::Warning, "High", "High")
            .with_priority(NotificationPriority::High);

        manager.notify(1, &critical);
        manager.notify(2, &high);

        let stats = manager.stats();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.critical, 1);
        assert_eq!(stats.high, 1);
        assert!(stats.has_critical());
        assert_eq!(stats.important_count(), 2);
    }

    #[test]
    fn test_current_opacity() {
        let mut ring = NotificationRing::new(1);
        ring.opacity = 1.0;
        ring.is_blinking = false;

        // Sem blink, deve retornar opacidade total
        assert_eq!(ring.current_opacity(), 1.0);

        // Com blink
        ring.is_blinking = true;
        ring.blink_state = true;
        assert_eq!(ring.current_opacity(), 1.0);

        ring.blink_state = false;
        assert_eq!(ring.current_opacity(), 0.3);
    }

    use super::super::NotificationType;

    fn with_priority(priority: NotificationPriority) -> Notification {
        Notification::new(NotificationType::Info, "t", "m").with_priority(priority)
    }

    #[test]
    fn test_appearance_per_priority() {
        let mut ring = NotificationRing::new(1);
        ring.add_notification(&with_priority(NotificationPriority::Normal));
        assert_eq!(
            (ring.color, ring.width, ring.is_blinking),
            ((100, 149, 237), 3.0, false)
        );
        ring.add_notification(&with_priority(NotificationPriority::High));
        assert_eq!(ring.color, (255, 165, 0));
        assert_eq!(ring.blink_rate_ms, 500);
        assert!(ring.is_blinking);
        ring.add_notification(&with_priority(NotificationPriority::Critical));
        assert_eq!(
            (ring.color, ring.width, ring.blink_rate_ms),
            ((255, 69, 0), 5.0, 250)
        );
        // Prioridade menor não rebaixa o anel
        ring.add_notification(&with_priority(NotificationPriority::Low));
        assert_eq!(ring.max_priority, NotificationPriority::Critical);

        // Aparência Low (aplicada diretamente)
        let mut low = NotificationRing::new(2);
        low.max_priority = NotificationPriority::Low;
        low.update_appearance();
        assert_eq!((low.color, low.width), ((100, 100, 100), 2.0));
    }

    #[test]
    fn test_blinking_toggles_and_dims_opacity() {
        let mut ring = NotificationRing::new(1);
        ring.add_notification(&with_priority(NotificationPriority::Critical));
        ring.blink_rate_ms = 0;
        assert!(ring.blink_state);
        ring.update();
        assert!(!ring.blink_state);
        assert!((ring.current_opacity() - 0.3).abs() < 1e-6);
        ring.update();
        assert_eq!(ring.current_opacity(), 1.0);

        // Anel sem piscar ignora update
        let mut calm = NotificationRing::new(2);
        calm.update();
        assert!(calm.blink_state);
    }

    #[test]
    fn test_builders_and_expiration() {
        let ring = NotificationRing::new(1)
            .with_color(1, 2, 3)
            .with_width(9.0)
            .with_duration(Duration::ZERO);
        assert_eq!(ring.color, (1, 2, 3));
        assert_eq!(ring.width, 9.0);
        assert!(ring.is_expired());
        assert!(!NotificationRing::new(2).is_expired());
        assert!(!NotificationRing::new(3)
            .with_duration(Duration::from_secs(3600))
            .is_expired());
    }

    #[test]
    fn test_manager_update_removes_expired_rings() {
        let mut manager = RingManager::default();
        manager.notify(1, &with_priority(NotificationPriority::Normal));
        manager.notify(2, &with_priority(NotificationPriority::Normal));
        manager.rings.get_mut(&1).unwrap().duration = Some(Duration::ZERO);
        manager.update();
        assert!(!manager.has_ring(1));
        assert!(manager.has_ring(2));
        assert_eq!(manager.active_count(), 1);

        // Desabilitado: update não faz nada e nada aparece como ativo
        manager.set_enabled(false);
        manager.rings.get_mut(&2).unwrap().duration = Some(Duration::ZERO);
        manager.update();
        assert_eq!(manager.rings.len(), 1);
        assert_eq!(manager.active_count(), 0);
        assert!(!manager.has_ring(2));
    }

    #[test]
    fn test_manager_dismiss_clear_and_stats() {
        let mut manager = RingManager::new();
        let a = with_priority(NotificationPriority::High);
        let b = with_priority(NotificationPriority::Low);
        manager.notify(1, &a);
        manager.notify(1, &b);
        manager.notify(2, &with_priority(NotificationPriority::Low));
        manager.notify(3, &with_priority(NotificationPriority::Normal));

        // Remover uma de duas mantém o anel
        manager.dismiss(1, a.id);
        assert!(manager.get_ring(1).is_some());
        manager.dismiss(1, b.id);
        assert!(manager.get_ring(1).is_none());
        manager.dismiss(99, 1); // painel sem anel

        let stats = manager.stats();
        assert_eq!((stats.total, stats.low, stats.normal), (2, 1, 1));
        // Anel com notificação Low começa com max_priority Low
        assert_eq!(
            manager.get_ring(2).unwrap().max_priority,
            NotificationPriority::Low
        );

        manager.clear_pane(2);
        assert!(manager.get_ring(2).is_none());
        manager.clear_all();
        assert!(manager.active_rings().is_empty());
        assert_ne!(RingPosition::Inner, RingPosition::Outer);
        assert_ne!(RingState::Static, RingState::Fading);
    }
}
