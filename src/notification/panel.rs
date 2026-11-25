//! Notification Panel
//!
//! Painel centralizado de notificações.
//! Atalho: Cmd/Ctrl+Shift+U para pular para não lidas

use super::{Notification, NotificationId, NotificationPriority, NotificationType};
use std::collections::HashSet;

/// Estado da visualização do painel
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelView {
    /// Visualização compacta (título apenas)
    Compact,
    /// Visualização completa (título + mensagem)
    Full,
    /// Agrupado por tipo
    Grouped,
    /// Filtrado
    Filtered(NotificationFilter),
}

/// Filtro de notificações
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NotificationFilter {
    /// Filtrar por tipo
    pub types: Option<Vec<NotificationType>>,
    /// Filtrar por prioridade mínima
    pub min_priority: Option<NotificationPriority>,
    /// Apenas não lidas
    pub unread_only: bool,
    /// Apenas por plugin específico
    pub plugin: Option<String>,
}

impl NotificationFilter {
    /// Cria filtro vazio (mostra tudo)
    pub fn empty() -> Self {
        Self::default()
    }

    /// Cria filtro para apenas não lidas
    pub fn unread() -> Self {
        Self {
            unread_only: true,
            ..Default::default()
        }
    }

    /// Cria filtro para prioridade mínima
    pub fn min_priority(priority: NotificationPriority) -> Self {
        Self {
            min_priority: Some(priority),
            ..Default::default()
        }
    }

    /// Cria filtro para tipo específico
    pub fn notification_type(ntype: NotificationType) -> Self {
        Self {
            types: Some(vec![ntype]),
            ..Default::default()
        }
    }

    /// Aplica filtro a uma lista de notificações
    pub fn apply(&self, notifications: &[Notification]) -> Vec<Notification> {
        notifications
            .iter()
            .filter(|n| {
                // Filtro de não lidas
                if self.unread_only && n.is_read {
                    return false;
                }

                // Filtro de tipo
                if let Some(ref types) = self.types {
                    if !types.contains(&n.notification_type) {
                        return false;
                    }
                }

                // Filtro de prioridade
                if let Some(ref min_priority) = self.min_priority {
                    if n.priority < *min_priority {
                        return false;
                    }
                }

                // Filtro de plugin
                if let Some(ref plugin) = self.plugin {
                    if n.plugin.as_ref() != Some(plugin) {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect()
    }
}

/// Painel de notificações
pub struct NotificationPanel {
    /// Todas as notificações
    notifications: Vec<Notification>,
    /// Visualização atual
    view: PanelView,
    /// Índice selecionado
    selected_index: Option<usize>,
    /// Visível
    visible: bool,
    /// Altura do painel (em linhas ou pixels)
    height: u32,
    /// Mostrar timestamps
    show_timestamps: bool,
    /// IDs selecionados para ações em massa
    selected_ids: HashSet<NotificationId>,
    /// Modo de seleção múltipla
    multi_select_mode: bool,
}

impl NotificationPanel {
    /// Cria um novo painel
    pub fn new() -> Self {
        Self {
            notifications: Vec::new(),
            view: PanelView::Full,
            selected_index: None,
            visible: false,
            height: 10,
            show_timestamps: true,
            selected_ids: HashSet::new(),
            multi_select_mode: false,
        }
    }

    /// Mostra o painel
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Esconde o painel
    pub fn hide(&mut self) {
        self.visible = false;
        self.selected_index = None;
        self.selected_ids.clear();
        self.multi_select_mode = false;
    }

    /// Toggle visibilidade
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    /// Verifica se está visível
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Adiciona notificação ao painel
    pub fn add(&mut self, notification: Notification) {
        self.notifications.push(notification);

        // Auto-seleciona a primeira notificação se nenhuma selecionada
        if self.selected_index.is_none() && self.visible {
            self.selected_index = Some(0);
        }
    }

    /// Remove notificação
    pub fn remove(&mut self, id: NotificationId) -> Option<Notification> {
        if let Some(index) = self.notifications.iter().position(|n| n.id == id) {
            // Ajusta seleção se necessário
            if let Some(selected) = self.selected_index {
                if index == selected {
                    // Seleciona próxima ou anterior
                    if index < self.notifications.len() - 1 {
                        self.selected_index = Some(index);
                    } else if index > 0 {
                        self.selected_index = Some(index - 1);
                    } else {
                        self.selected_index = None;
                    }
                } else if index < selected {
                    self.selected_index = Some(selected - 1);
                }
            }

            self.selected_ids.remove(&id);
            Some(self.notifications.remove(index))
        } else {
            None
        }
    }

    /// Marca notificação como lida
    pub fn mark_as_read(&mut self, id: NotificationId) -> bool {
        if let Some(notification) = self.notifications.iter_mut().find(|n| n.id == id) {
            notification.mark_as_read();
            true
        } else {
            false
        }
    }

    /// Marca todas como lidas
    pub fn mark_all_as_read(&mut self) {
        for notification in &mut self.notifications {
            notification.mark_as_read();
        }
        self.selected_ids.clear();
    }

    /// Marca selecionadas como lidas
    pub fn mark_selected_as_read(&mut self) {
        for id in &self.selected_ids {
            if let Some(n) = self.notifications.iter_mut().find(|n| n.id == *id) {
                n.mark_as_read();
            }
        }
        self.selected_ids.clear();
    }

    /// Limpa todas as notificações lidas
    pub fn clear_read(&mut self) {
        self.notifications.retain(|n| !n.is_read);
        self.selected_index = self.selected_index.and_then(|idx| {
            if idx < self.notifications.len() {
                Some(idx)
            } else if !self.notifications.is_empty() {
                Some(self.notifications.len() - 1)
            } else {
                None
            }
        });
    }

    /// Limpa todas
    pub fn clear_all(&mut self) {
        self.notifications.clear();
        self.selected_index = None;
        self.selected_ids.clear();
    }

    /// Pula para próxima não lida (Cmd/Ctrl+Shift+U)
    pub fn jump_to_next_unread(&mut self) -> Option<&Notification> {
        let start = self.selected_index.map(|i| i + 1).unwrap_or(0);

        for (i, notification) in self.notifications.iter().enumerate().skip(start) {
            if !notification.is_read {
                self.selected_index = Some(i);
                return Some(notification);
            }
        }

        // Volta ao início
        for (i, notification) in self.notifications.iter().enumerate().take(start) {
            if !notification.is_read {
                self.selected_index = Some(i);
                return Some(notification);
            }
        }

        None
    }

    /// Navegação: próxima
    pub fn next(&mut self) {
        if self.notifications.is_empty() {
            return;
        }

        self.selected_index = Some(match self.selected_index {
            Some(idx) => (idx + 1) % self.notifications.len(),
            None => 0,
        });
    }

    /// Navegação: anterior
    pub fn previous(&mut self) {
        if self.notifications.is_empty() {
            return;
        }

        self.selected_index = Some(match self.selected_index {
            Some(idx) => {
                if idx == 0 {
                    self.notifications.len() - 1
                } else {
                    idx - 1
                }
            }
            None => self.notifications.len() - 1,
        });
    }

    /// Seleciona notificação atual
    pub fn select_current(&mut self) {
        if self.multi_select_mode {
            if let Some(idx) = self.selected_index {
                if let Some(notification) = self.notifications.get(idx) {
                    let id = notification.id;
                    if self.selected_ids.contains(&id) {
                        self.selected_ids.remove(&id);
                    } else {
                        self.selected_ids.insert(id);
                    }
                }
            }
        }
    }

    /// Alterna modo de seleção múltipla
    pub fn toggle_multi_select(&mut self) {
        self.multi_select_mode = !self.multi_select_mode;
        if !self.multi_select_mode {
            self.selected_ids.clear();
        }
    }

    /// Define visualização
    pub fn set_view(&mut self, view: PanelView) {
        self.view = view;
    }

    /// Retorna visualização atual
    pub fn view(&self) -> &PanelView {
        &self.view
    }

    /// Retorna notificações filtradas para exibição
    pub fn displayed_notifications(&self) -> Vec<Notification> {
        match self.view {
            PanelView::Compact | PanelView::Full => self.notifications.clone(),
            PanelView::Grouped => self.grouped_notifications(),
            PanelView::Filtered(ref filter) => filter.apply(&self.notifications),
        }
    }

    /// Agrupa notificações por tipo
    fn grouped_notifications(&self) -> Vec<Notification> {
        let mut result = self.notifications.clone();
        result.sort_by(|a, b| {
            a.notification_type
                .cmp(&b.notification_type)
                .then_with(|| b.timestamp.cmp(&a.timestamp))
        });
        result
    }

    /// Retorna notificação selecionada
    pub fn selected_notification(&self) -> Option<&Notification> {
        self.selected_index
            .and_then(|idx| self.notifications.get(idx))
    }

    /// Retorna índice selecionado
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Contagem total
    pub fn total_count(&self) -> usize {
        self.notifications.len()
    }

    /// Contagem de não lidas
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.is_read).count()
    }

    /// Contagem por tipo
    pub fn count_by_type(&self, ntype: NotificationType) -> usize {
        self.notifications
            .iter()
            .filter(|n| n.notification_type == ntype)
            .count()
    }

    /// Contagem por prioridade
    pub fn count_by_priority(&self, priority: NotificationPriority) -> usize {
        self.notifications
            .iter()
            .filter(|n| n.priority == priority)
            .count()
    }

    /// Verifica se há notificações não lidas
    pub fn has_unread(&self) -> bool {
        self.notifications.iter().any(|n| !n.is_read)
    }

    /// Verifica se há notificações críticas
    pub fn has_critical(&self) -> bool {
        self.notifications
            .iter()
            .any(|n| n.priority == NotificationPriority::Critical && !n.is_read)
    }

    /// Retorna estatísticas
    pub fn stats(&self) -> PanelStats {
        PanelStats {
            total: self.total_count(),
            unread: self.unread_count(),
            critical: self.count_by_priority(NotificationPriority::Critical),
            high: self.count_by_priority(NotificationPriority::High),
        }
    }

    /// Define altura
    pub fn set_height(&mut self, height: u32) {
        self.height = height.max(3);
    }

    /// Retorna altura
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Toggle timestamps
    pub fn toggle_timestamps(&mut self) {
        self.show_timestamps = !self.show_timestamps;
    }

    /// Retorna todas as notificações
    pub fn all_notifications(&self) -> &[Notification] {
        &self.notifications
    }

    /// Retorna IDs selecionados
    pub fn selected_ids(&self) -> &HashSet<NotificationId> {
        &self.selected_ids
    }

    /// Verifica se está em modo multi-select
    pub fn is_multi_select(&self) -> bool {
        self.multi_select_mode
    }
}

impl Default for NotificationPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// Estatísticas do painel
#[derive(Debug, Clone, Copy)]
pub struct PanelStats {
    /// Total
    pub total: usize,
    /// Não lidas
    pub unread: usize,
    /// Críticas
    pub critical: usize,
    /// Altas
    pub high: usize,
}

impl PanelStats {
    /// Retorna total de importantes
    pub fn important_unread(&self) -> usize {
        self.critical + self.high
    }
}

/// Ações do painel
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelAction {
    /// Mostrar/esconder
    Toggle,
    /// Próxima
    Next,
    /// Anterior
    Previous,
    /// Marcar como lida
    MarkRead,
    /// Marcar todas como lidas
    MarkAllRead,
    /// Limpar lidas
    ClearRead,
    /// Pular para não lida
    JumpToUnread,
    /// Alternar modo seleção
    ToggleMultiSelect,
    /// Selecionar atual
    Select,
    /// Fechar
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_creation() {
        let panel = NotificationPanel::new();
        assert!(!panel.is_visible());
        assert_eq!(panel.total_count(), 0);
        assert_eq!(panel.unread_count(), 0);
    }

    #[test]
    fn test_panel_toggle() {
        let mut panel = NotificationPanel::new();
        assert!(!panel.is_visible());

        panel.toggle();
        assert!(panel.is_visible());

        panel.toggle();
        assert!(!panel.is_visible());
    }

    #[test]
    fn test_add_notification() {
        let mut panel = NotificationPanel::new();
        let notification = Notification::new(NotificationType::Info, "Test", "Test message");

        panel.add(notification);
        assert_eq!(panel.total_count(), 1);
        assert_eq!(panel.unread_count(), 1);
    }

    #[test]
    fn test_mark_as_read() {
        let mut panel = NotificationPanel::new();
        let notification = Notification::new(NotificationType::Info, "Test", "Test");
        let id = notification.id;

        panel.add(notification);
        assert_eq!(panel.unread_count(), 1);

        panel.mark_as_read(id);
        assert_eq!(panel.unread_count(), 0);
    }

    #[test]
    fn test_jump_to_unread() {
        let mut panel = NotificationPanel::new();

        // Adiciona notificações
        let n1 = Notification::new(NotificationType::Info, "1", "1");
        let n2 = Notification::new(NotificationType::Info, "2", "2");
        let n3 = Notification::new(NotificationType::Info, "3", "3");

        panel.add(n1.clone());
        panel.add(n2.clone());
        panel.add(n3.clone());

        // Marca a primeira como lida
        panel.mark_as_read(n1.id);

        // Primeira chamada deve ir para a segunda (índice 1, pois a primeira está lida)
        let unread = panel.jump_to_next_unread();
        assert!(unread.is_some());
        assert_eq!(unread.unwrap().title, "2");
        assert_eq!(panel.selected_index(), Some(1));

        // Marca a segunda como lida
        panel.mark_as_read(n2.id);

        // Próxima chamada deve ir para a terceira
        let unread = panel.jump_to_next_unread();
        assert!(unread.is_some());
        assert_eq!(unread.unwrap().title, "3");
        assert_eq!(panel.selected_index(), Some(2));
    }

    #[test]
    fn test_navigation() {
        let mut panel = NotificationPanel::new();

        panel.add(Notification::new(NotificationType::Info, "1", "1"));
        panel.add(Notification::new(NotificationType::Info, "2", "2"));
        panel.add(Notification::new(NotificationType::Info, "3", "3"));

        assert!(panel.selected_index().is_none());

        panel.next();
        assert_eq!(panel.selected_index(), Some(0));

        panel.next();
        assert_eq!(panel.selected_index(), Some(1));

        panel.previous();
        assert_eq!(panel.selected_index(), Some(0));

        // Testa wrap-around
        panel.previous();
        assert_eq!(panel.selected_index(), Some(2));
    }

    #[test]
    fn test_filter() {
        let filter = NotificationFilter::min_priority(NotificationPriority::High);

        let notifications = vec![
            Notification::new(NotificationType::Info, "Low", "Low")
                .with_priority(NotificationPriority::Low),
            Notification::new(NotificationType::Info, "High", "High")
                .with_priority(NotificationPriority::High),
            Notification::new(NotificationType::Info, "Critical", "Critical")
                .with_priority(NotificationPriority::Critical),
        ];

        let filtered = filter.apply(&notifications);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].title, "High");
        assert_eq!(filtered[1].title, "Critical");
    }

    #[test]
    fn test_filter_unread() {
        let filter = NotificationFilter::unread();

        let mut n1 = Notification::new(NotificationType::Info, "Read", "Read");
        n1.mark_as_read();

        let n2 = Notification::new(NotificationType::Info, "Unread", "Unread");

        let notifications = vec![n1, n2];
        let filtered = filter.apply(&notifications);

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].title, "Unread");
    }

    #[test]
    fn test_clear_read() {
        let mut panel = NotificationPanel::new();

        let mut n1 = Notification::new(NotificationType::Info, "Read", "Read");
        n1.mark_as_read();

        let n2 = Notification::new(NotificationType::Info, "Unread", "Unread");

        panel.add(n1);
        panel.add(n2);

        assert_eq!(panel.total_count(), 2);

        panel.clear_read();
        assert_eq!(panel.total_count(), 1);
        assert_eq!(panel.all_notifications()[0].title, "Unread");
    }

    #[test]
    fn test_multi_select() {
        let mut panel = NotificationPanel::new();

        panel.add(Notification::new(NotificationType::Info, "1", "1"));
        panel.add(Notification::new(NotificationType::Info, "2", "2"));

        panel.toggle_multi_select();
        assert!(panel.is_multi_select());

        panel.next(); // seleciona índice 0
        panel.select_current();
        assert_eq!(panel.selected_ids().len(), 1);

        panel.next(); // seleciona índice 1
        panel.select_current();
        assert_eq!(panel.selected_ids().len(), 2);

        // Toggle multi-select off limpa seleção
        panel.toggle_multi_select();
        assert!(!panel.is_multi_select());
        assert!(panel.selected_ids().is_empty());
    }

    #[test]
    fn test_stats() {
        let mut panel = NotificationPanel::new();

        panel.add(
            Notification::new(NotificationType::Error, "Critical", "Critical")
                .with_priority(NotificationPriority::Critical),
        );
        panel.add(
            Notification::new(NotificationType::Warning, "High", "High")
                .with_priority(NotificationPriority::High),
        );
        panel.add(Notification::new(
            NotificationType::Info,
            "Normal",
            "Normal",
        ));

        let stats = panel.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.unread, 3);
        assert_eq!(stats.critical, 1);
        assert_eq!(stats.high, 1);
        assert_eq!(stats.important_unread(), 2);
    }

    fn note(t: NotificationType, title: &str) -> Notification {
        Notification::new(t, title, "msg")
    }

    fn panel_with(titles: &[&str]) -> (NotificationPanel, Vec<NotificationId>) {
        let mut panel = NotificationPanel::default();
        let mut ids = Vec::new();
        for title in titles {
            let n = note(NotificationType::Info, title);
            ids.push(n.id);
            panel.add(n);
        }
        (panel, ids)
    }

    fn titles(panel: &NotificationPanel) -> Vec<String> {
        panel
            .all_notifications()
            .iter()
            .map(|n| n.title.clone())
            .collect()
    }

    #[test]
    fn test_add_auto_selects_only_when_visible() {
        let (mut panel, _) = panel_with(&["a"]);
        assert_eq!(panel.selected_index(), None);
        panel.show();
        panel.add(note(NotificationType::Info, "b"));
        assert_eq!(panel.selected_index(), Some(0));
        assert_eq!(panel.selected_notification().unwrap().title, "a");
    }

    #[test]
    fn test_remove_adjusts_selection() {
        // Remove a selecionada no meio: seleção fica no mesmo índice (próxima)
        let (mut panel, ids) = panel_with(&["a", "b", "c"]);
        panel.next();
        panel.next(); // índice 1
        assert_eq!(panel.remove(ids[1]).unwrap().title, "b");
        assert_eq!(panel.selected_index(), Some(1));
        assert_eq!(panel.selected_notification().unwrap().title, "c");

        // Remove a selecionada no fim: volta uma
        assert!(panel.remove(ids[2]).is_some());
        assert_eq!(panel.selected_index(), Some(0));

        // Remove a única: sem seleção
        assert!(panel.remove(ids[0]).is_some());
        assert_eq!(panel.selected_index(), None);
        assert!(panel.remove(ids[0]).is_none());

        // Remover antes da seleção desloca o índice
        let (mut panel, ids) = panel_with(&["a", "b", "c"]);
        panel.previous(); // None -> último (2)
        panel.remove(ids[0]);
        assert_eq!(panel.selected_index(), Some(1));
        assert_eq!(panel.selected_notification().unwrap().title, "c");

        // Remover depois da seleção não mexe
        panel.previous(); // 0
        panel.remove(ids[2]);
        assert_eq!(panel.selected_index(), Some(0));
    }

    #[test]
    fn test_mark_selected_and_unknown() {
        let (mut panel, ids) = panel_with(&["a", "b"]);
        assert!(!panel.mark_as_read(9_999_999));
        panel.toggle_multi_select();
        assert!(panel.is_multi_select());
        panel.next();
        panel.select_current();
        assert!(panel.selected_ids().contains(&ids[0]));
        panel.mark_selected_as_read();
        assert!(panel.selected_ids().is_empty());
        assert_eq!(panel.unread_count(), 1);

        // Selecionar duas vezes desmarca
        panel.next();
        panel.select_current();
        panel.select_current();
        assert!(panel.selected_ids().is_empty());

        // Fora do modo multi-seleção, select_current não faz nada
        panel.toggle_multi_select();
        panel.select_current();
        assert!(panel.selected_ids().is_empty());

        panel.mark_all_as_read();
        assert!(!panel.has_unread());
    }

    #[test]
    fn test_clear_read_keeps_selection_in_bounds() {
        let (mut panel, ids) = panel_with(&["a", "b", "c"]);
        panel.previous(); // seleciona o último
        panel.mark_as_read(ids[1]);
        panel.mark_as_read(ids[2]);
        panel.clear_read();
        assert_eq!(titles(&panel), vec!["a"]);
        assert_eq!(panel.selected_index(), Some(0));

        panel.mark_all_as_read();
        panel.clear_read();
        assert_eq!(panel.selected_index(), None);
        panel.clear_all();
        assert_eq!(panel.total_count(), 0);
    }

    #[test]
    fn test_navigation_on_empty_panel_and_jump() {
        let mut panel = NotificationPanel::new();
        panel.next();
        panel.previous();
        assert_eq!(panel.selected_index(), None);
        assert!(panel.jump_to_next_unread().is_none());

        // Sem não lidas: None mesmo com itens
        let (mut panel, ids) = panel_with(&["a"]);
        panel.mark_as_read(ids[0]);
        assert!(panel.jump_to_next_unread().is_none());
    }

    #[test]
    fn test_views_and_grouping() {
        let mut panel = NotificationPanel::new();
        panel.add(note(NotificationType::Error, "erro"));
        panel.add(note(NotificationType::Info, "info"));
        panel.add(
            note(NotificationType::Warning, "aviso")
                .with_priority(NotificationPriority::High)
                .with_plugin("git"),
        );

        panel.set_view(PanelView::Compact);
        assert_eq!(panel.view(), &PanelView::Compact);
        assert_eq!(panel.displayed_notifications().len(), 3);

        panel.set_view(PanelView::Grouped);
        let grouped: Vec<_> = panel
            .displayed_notifications()
            .into_iter()
            .map(|n| n.notification_type)
            .collect();
        assert_eq!(
            grouped,
            vec![
                NotificationType::Info,
                NotificationType::Warning,
                NotificationType::Error
            ]
        );

        panel.set_view(PanelView::Filtered(NotificationFilter::notification_type(
            NotificationType::Error,
        )));
        assert_eq!(panel.displayed_notifications()[0].title, "erro");

        panel.set_view(PanelView::Filtered(NotificationFilter::min_priority(
            NotificationPriority::High,
        )));
        assert_eq!(panel.displayed_notifications()[0].title, "aviso");

        let by_plugin = NotificationFilter {
            plugin: Some("git".into()),
            ..NotificationFilter::empty()
        };
        assert_eq!(by_plugin.apply(panel.all_notifications()).len(), 1);
        assert_eq!(
            NotificationFilter::empty()
                .apply(panel.all_notifications())
                .len(),
            3
        );

        assert_eq!(panel.count_by_type(NotificationType::Info), 1);
        assert_eq!(panel.count_by_priority(NotificationPriority::High), 1);
        assert_eq!(panel.stats().important_unread(), 1);
        assert!(!panel.has_critical());

        panel.toggle_timestamps();
        panel.set_height(1);
        assert_eq!(panel.height(), 3);
        assert_ne!(PanelAction::Next, PanelAction::Close);
    }
}
