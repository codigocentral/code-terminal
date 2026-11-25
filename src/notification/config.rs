//! Notification Configuration
//!
//! Configurações do sistema de notificações.

use super::{NotificationPriority, NotificationType};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Configuração do sistema de notificações
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Habilitar notificações globalmente
    pub enabled: bool,
    /// Habilitar anéis visuais
    pub enable_rings: bool,
    /// Habilitar badges na sidebar
    pub enable_badges: bool,
    /// Habilitar notificações desktop
    pub enable_desktop: bool,
    /// Habilitar painel de notificações
    pub enable_panel: bool,
    /// Habilitar OSC sequences
    pub enable_osc: bool,
    /// Duração padrão das notificações (segundos, None = persistir)
    pub default_duration_seconds: Option<u64>,
    /// Altura do painel em linhas
    pub panel_height: u32,
    /// Mostrar timestamps
    pub show_timestamps: bool,
    /// Ordem de exibição
    pub sort_order: SortOrder,
    /// Máximo de notificações no histórico
    pub max_history: usize,
    /// Regras de filtragem
    pub filter_rules: Vec<FilterRule>,
    /// Configurações específicas por tipo
    pub type_config: NotificationTypeConfig,
    /// Configurações específicas por prioridade
    pub priority_config: NotificationPriorityConfig,
    /// Configurações de desktop
    pub desktop_config: DesktopNotificationConfig,
}

impl NotificationConfig {
    /// Cria configuração padrão
    pub fn default() -> Self {
        Self {
            enabled: true,
            enable_rings: true,
            enable_badges: true,
            enable_desktop: false,
            enable_panel: true,
            enable_osc: true,
            default_duration_seconds: None,
            panel_height: 10,
            show_timestamps: true,
            sort_order: SortOrder::NewestFirst,
            max_history: 1000,
            filter_rules: Vec::new(),
            type_config: NotificationTypeConfig::default(),
            priority_config: NotificationPriorityConfig::default(),
            desktop_config: DesktopNotificationConfig::default(),
        }
    }

    /// Carrega de arquivo
    pub fn load() -> Self {
        // TODO: Implementar carregamento de arquivo
        Self::default()
    }

    /// Salva em arquivo
    pub fn save(&self) {
        // TODO: Implementar salvamento
    }

    /// Define habilitação global
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Retorna duração padrão como Duration
    pub fn default_duration(&self) -> Option<Duration> {
        self.default_duration_seconds.map(Duration::from_secs)
    }

    /// Verifica se uma notificação deve ser mostrada baseada nas regras
    pub fn should_show(
        &self,
        ntype: NotificationType,
        priority: NotificationPriority,
        plugin: Option<&str>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        // Verifica configurações por tipo
        if !self.type_config.is_enabled(ntype) {
            return false;
        }

        // Verifica configurações por prioridade
        if !self.priority_config.is_enabled(priority) {
            return false;
        }

        // Aplica regras de filtro: só Block esconde; Silent ainda mostra no
        // painel (sem anel), conforme documentado em FilterAction::Silent
        self.filter_action(ntype, priority, plugin) != Some(FilterAction::Block)
    }

    /// Ação da primeira regra de filtro que casa com a notificação
    pub fn filter_action(
        &self,
        ntype: NotificationType,
        priority: NotificationPriority,
        plugin: Option<&str>,
    ) -> Option<FilterAction> {
        self.filter_rules
            .iter()
            .find(|rule| rule.matches(ntype, priority, plugin))
            .map(|rule| rule.action)
    }

    /// Adiciona regra de filtro
    pub fn add_filter_rule(&mut self, rule: FilterRule) {
        self.filter_rules.push(rule);
    }

    /// Remove regra de filtro por índice
    pub fn remove_filter_rule(&mut self, index: usize) -> Option<FilterRule> {
        if index < self.filter_rules.len() {
            Some(self.filter_rules.remove(index))
        } else {
            None
        }
    }
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self::default()
    }
}

/// Ordem de exibição
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    /// Mais recentes primeiro
    NewestFirst,
    /// Mais antigas primeiro
    OldestFirst,
    /// Por prioridade (críticas primeiro)
    ByPriority,
    /// Por tipo
    ByType,
}

/// Regra de filtro
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterRule {
    /// Nome da regra
    pub name: String,
    /// Tipos afetados (None = todos)
    pub types: Option<Vec<NotificationType>>,
    /// Prioridade mínima
    pub min_priority: Option<NotificationPriority>,
    /// Prioridade máxima
    pub max_priority: Option<NotificationPriority>,
    /// Plugin específico
    pub plugin: Option<String>,
    /// Ação
    pub action: FilterAction,
}

impl FilterRule {
    /// Cria nova regra
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            types: None,
            min_priority: None,
            max_priority: None,
            plugin: None,
            action: FilterAction::Allow,
        }
    }

    /// Define tipos
    pub fn with_types(mut self, types: Vec<NotificationType>) -> Self {
        self.types = Some(types);
        self
    }

    /// Define prioridade mínima
    pub fn with_min_priority(mut self, priority: NotificationPriority) -> Self {
        self.min_priority = Some(priority);
        self
    }

    /// Define prioridade máxima
    pub fn with_max_priority(mut self, priority: NotificationPriority) -> Self {
        self.max_priority = Some(priority);
        self
    }

    /// Define plugin
    pub fn with_plugin(mut self, plugin: impl Into<String>) -> Self {
        self.plugin = Some(plugin.into());
        self
    }

    /// Define ação
    pub fn with_action(mut self, action: FilterAction) -> Self {
        self.action = action;
        self
    }

    /// Verifica se uma notificação corresponde à regra
    pub fn matches(
        &self,
        ntype: NotificationType,
        priority: NotificationPriority,
        plugin: Option<&str>,
    ) -> bool {
        // Verifica tipo
        if let Some(ref types) = self.types {
            if !types.contains(&ntype) {
                return false;
            }
        }

        // Verifica prioridade mínima
        if let Some(ref min) = self.min_priority {
            if priority < *min {
                return false;
            }
        }

        // Verifica prioridade máxima
        if let Some(ref max) = self.max_priority {
            if priority > *max {
                return false;
            }
        }

        // Verifica plugin
        if let Some(ref p) = self.plugin {
            if plugin != Some(p.as_str()) {
                return false;
            }
        }

        true
    }
}

/// Ação de filtro
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterAction {
    /// Permitir notificação
    Allow,
    /// Bloquear notificação
    Block,
    /// Permitir apenas silenciosamente (sem desktop/ring)
    Silent,
}

/// Configurações por tipo de notificação
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationTypeConfig {
    /// Info habilitado
    pub info_enabled: bool,
    /// Warning habilitado
    pub warning_enabled: bool,
    /// Error habilitado
    pub error_enabled: bool,
    /// Success habilitado
    pub success_enabled: bool,
    /// System habilitado
    pub system_enabled: bool,
    /// AI Agent habilitado
    pub ai_agent_enabled: bool,
}

impl NotificationTypeConfig {
    /// Cria configuração padrão
    pub fn default() -> Self {
        Self {
            info_enabled: true,
            warning_enabled: true,
            error_enabled: true,
            success_enabled: true,
            system_enabled: true,
            ai_agent_enabled: true,
        }
    }

    /// Verifica se um tipo está habilitado
    pub fn is_enabled(&self, ntype: NotificationType) -> bool {
        match ntype {
            NotificationType::Info => self.info_enabled,
            NotificationType::Warning => self.warning_enabled,
            NotificationType::Error => self.error_enabled,
            NotificationType::Success => self.success_enabled,
            NotificationType::System => self.system_enabled,
            NotificationType::AiAgent => self.ai_agent_enabled,
        }
    }

    /// Define habilitação de um tipo
    pub fn set_enabled(&mut self, ntype: NotificationType, enabled: bool) {
        match ntype {
            NotificationType::Info => self.info_enabled = enabled,
            NotificationType::Warning => self.warning_enabled = enabled,
            NotificationType::Error => self.error_enabled = enabled,
            NotificationType::Success => self.success_enabled = enabled,
            NotificationType::System => self.system_enabled = enabled,
            NotificationType::AiAgent => self.ai_agent_enabled = enabled,
        }
    }
}

impl Default for NotificationTypeConfig {
    fn default() -> Self {
        Self::default()
    }
}

/// Configurações por prioridade
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationPriorityConfig {
    /// Low habilitado
    pub low_enabled: bool,
    /// Normal habilitado
    pub normal_enabled: bool,
    /// High habilitado
    pub high_enabled: bool,
    /// Critical habilitado
    pub critical_enabled: bool,
}

impl NotificationPriorityConfig {
    /// Cria configuração padrão
    pub fn default() -> Self {
        Self {
            low_enabled: true,
            normal_enabled: true,
            high_enabled: true,
            critical_enabled: true,
        }
    }

    /// Verifica se uma prioridade está habilitada
    pub fn is_enabled(&self, priority: NotificationPriority) -> bool {
        match priority {
            NotificationPriority::Low => self.low_enabled,
            NotificationPriority::Normal => self.normal_enabled,
            NotificationPriority::High => self.high_enabled,
            NotificationPriority::Critical => self.critical_enabled,
        }
    }

    /// Define habilitação de uma prioridade
    pub fn set_enabled(&mut self, priority: NotificationPriority, enabled: bool) {
        match priority {
            NotificationPriority::Low => self.low_enabled = enabled,
            NotificationPriority::Normal => self.normal_enabled = enabled,
            NotificationPriority::High => self.high_enabled = enabled,
            NotificationPriority::Critical => self.critical_enabled = enabled,
        }
    }
}

impl Default for NotificationPriorityConfig {
    fn default() -> Self {
        Self::default()
    }
}

/// Configurações de notificações desktop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopNotificationConfig {
    /// Habilitar desktop notifications
    pub enabled: bool,
    /// Mostrar apenas para prioridades >=
    pub min_priority: NotificationPriority,
    /// Timeout em segundos (None = padrão do sistema)
    pub timeout_seconds: Option<u64>,
    /// Ícone padrão
    pub default_icon: Option<String>,
    /// Som de notificação
    pub play_sound: bool,
    /// Som customizado (None = som padrão)
    pub sound_file: Option<String>,
    /// Rate limiting (máximo por minuto)
    pub rate_limit_per_minute: u32,
}

impl DesktopNotificationConfig {
    /// Cria configuração padrão
    pub fn default() -> Self {
        Self {
            enabled: false,
            min_priority: NotificationPriority::High,
            timeout_seconds: Some(5),
            default_icon: None,
            play_sound: false,
            sound_file: None,
            rate_limit_per_minute: 10,
        }
    }

    /// Define timeout
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout_seconds.map(Duration::from_secs)
    }
}

impl Default for DesktopNotificationConfig {
    fn default() -> Self {
        Self::default()
    }
}

/// Configurações de atalhos de teclado
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationKeybindings {
    /// Abrir/fechar painel
    pub toggle_panel: String,
    /// Pular para não lida
    pub jump_to_unread: String,
    /// Próxima notificação
    pub next: String,
    /// Notificação anterior
    pub previous: String,
    /// Marcar como lida
    pub mark_read: String,
    /// Marcar todas como lidas
    pub mark_all_read: String,
    /// Fechar painel
    pub close: String,
}

impl NotificationKeybindings {
    /// Cria configuração padrão
    pub fn default() -> Self {
        Self {
            toggle_panel: "Ctrl+Shift+N".to_string(),
            jump_to_unread: "Ctrl+Shift+U".to_string(),
            next: "j".to_string(),
            previous: "k".to_string(),
            mark_read: "Enter".to_string(),
            mark_all_read: "Ctrl+A".to_string(),
            close: "Esc".to_string(),
        }
    }
}

impl Default for NotificationKeybindings {
    fn default() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NotificationConfig::default();
        assert!(config.enabled);
        assert!(config.enable_rings);
        assert!(config.enable_badges);
        assert!(!config.enable_desktop);
        assert!(config.enable_osc);
        assert_eq!(config.panel_height, 10);
    }

    #[test]
    fn test_filter_rule() {
        let rule = FilterRule::new("Block Low Priority")
            .with_max_priority(NotificationPriority::Low)
            .with_action(FilterAction::Block);

        assert!(rule.matches(NotificationType::Info, NotificationPriority::Low, None));
        assert!(!rule.matches(NotificationType::Info, NotificationPriority::High, None));
    }

    #[test]
    fn test_filter_rule_with_types() {
        let rule = FilterRule::new("Block Info and Success")
            .with_types(vec![NotificationType::Info, NotificationType::Success])
            .with_action(FilterAction::Block);

        assert!(rule.matches(NotificationType::Info, NotificationPriority::Normal, None));
        assert!(rule.matches(
            NotificationType::Success,
            NotificationPriority::Normal,
            None
        ));
        assert!(!rule.matches(NotificationType::Error, NotificationPriority::Normal, None));
    }

    #[test]
    fn test_type_config() {
        let mut config = NotificationTypeConfig::default();

        assert!(config.is_enabled(NotificationType::Info));

        config.set_enabled(NotificationType::Info, false);
        assert!(!config.is_enabled(NotificationType::Info));

        // Outros tipos não devem ser afetados
        assert!(config.is_enabled(NotificationType::Error));
    }

    #[test]
    fn test_priority_config() {
        let mut config = NotificationPriorityConfig::default();

        assert!(config.is_enabled(NotificationPriority::Critical));

        config.set_enabled(NotificationPriority::Critical, false);
        assert!(!config.is_enabled(NotificationPriority::Critical));
    }

    #[test]
    fn test_should_show() {
        let mut config = NotificationConfig::default();

        // Por padrão tudo deve ser permitido
        assert!(config.should_show(NotificationType::Info, NotificationPriority::Low, None));

        // Desabilita tipo
        config
            .type_config
            .set_enabled(NotificationType::Info, false);
        assert!(!config.should_show(NotificationType::Info, NotificationPriority::Critical, None));
        assert!(config.should_show(
            NotificationType::Error,
            NotificationPriority::Critical,
            None
        ));

        // Regra de bloqueio
        config.add_filter_rule(
            FilterRule::new("Block All Low")
                .with_max_priority(NotificationPriority::Low)
                .with_action(FilterAction::Block),
        );

        // Low deve ser bloqueado
        assert!(!config.should_show(NotificationType::Error, NotificationPriority::Low, None));
        // Normal deve passar
        assert!(config.should_show(NotificationType::Error, NotificationPriority::Normal, None));
    }

    #[test]
    fn test_filter_rule_plugin() {
        let rule = FilterRule::new("Plugin Filter")
            .with_plugin("test-plugin")
            .with_action(FilterAction::Block);

        assert!(rule.matches(
            NotificationType::Info,
            NotificationPriority::Normal,
            Some("test-plugin")
        ));
        assert!(!rule.matches(
            NotificationType::Info,
            NotificationPriority::Normal,
            Some("other-plugin")
        ));
        assert!(!rule.matches(NotificationType::Info, NotificationPriority::Normal, None));
    }

    #[test]
    fn test_desktop_config() {
        let config = DesktopNotificationConfig::default();

        assert!(!config.enabled);
        assert_eq!(config.min_priority, NotificationPriority::High);
        assert_eq!(config.timeout_seconds, Some(5));
        assert_eq!(config.rate_limit_per_minute, 10);
    }

    #[test]
    fn test_duration() {
        let config = NotificationConfig {
            default_duration_seconds: Some(30),
            ..NotificationConfig::default()
        };

        assert_eq!(config.default_duration(), Some(Duration::from_secs(30)));

        let config_none = NotificationConfig {
            default_duration_seconds: None,
            ..NotificationConfig::default()
        };

        assert_eq!(config_none.default_duration(), None);
    }

    #[test]
    fn test_filter_rules_first_match_wins() {
        let mut config = NotificationConfig::default();
        config.add_filter_rule(
            FilterRule::new("bloqueia plugin x")
                .with_plugin("x")
                .with_action(FilterAction::Block),
        );
        config.add_filter_rule(FilterRule::new("permite tudo"));

        assert!(!config.should_show(
            NotificationType::Info,
            NotificationPriority::Normal,
            Some("x")
        ));
        assert!(config.should_show(
            NotificationType::Info,
            NotificationPriority::Normal,
            Some("y")
        ));
        assert_eq!(
            config.filter_action(NotificationType::Info, NotificationPriority::Low, None),
            Some(FilterAction::Allow)
        );

        let removed = config.remove_filter_rule(0).unwrap();
        assert_eq!(removed.name, "bloqueia plugin x");
        assert!(config.remove_filter_rule(5).is_none());
        assert!(config.should_show(
            NotificationType::Info,
            NotificationPriority::Normal,
            Some("x")
        ));
    }

    /// Regressão: Silent era tratado como Block e a notificação sumia.
    #[test]
    fn test_silent_rule_still_shows() {
        let mut config = NotificationConfig::default();
        config.add_filter_rule(
            FilterRule::new("silencia baixas")
                .with_max_priority(NotificationPriority::Low)
                .with_action(FilterAction::Silent),
        );
        assert!(config.should_show(NotificationType::Info, NotificationPriority::Low, None));
        assert_eq!(
            config.filter_action(NotificationType::Info, NotificationPriority::Low, None),
            Some(FilterAction::Silent)
        );
        assert_eq!(
            config.filter_action(NotificationType::Info, NotificationPriority::High, None),
            None
        );
    }

    #[test]
    fn test_filter_rule_matching() {
        let rule = FilterRule::new("r")
            .with_types(vec![NotificationType::Error, NotificationType::Warning])
            .with_min_priority(NotificationPriority::Normal)
            .with_max_priority(NotificationPriority::High)
            .with_plugin("git");
        assert!(rule.matches(
            NotificationType::Error,
            NotificationPriority::High,
            Some("git")
        ));
        assert!(!rule.matches(
            NotificationType::Info,
            NotificationPriority::High,
            Some("git")
        ));
        assert!(!rule.matches(
            NotificationType::Error,
            NotificationPriority::Low,
            Some("git")
        ));
        assert!(!rule.matches(
            NotificationType::Error,
            NotificationPriority::Critical,
            Some("git")
        ));
        assert!(!rule.matches(NotificationType::Error, NotificationPriority::High, None));
    }

    #[test]
    fn test_global_type_and_priority_switches() {
        let mut config = NotificationConfig::load();
        config.save();
        config.set_enabled(false);
        assert!(!config.should_show(NotificationType::Info, NotificationPriority::High, None));
        config.set_enabled(true);

        let types = [
            NotificationType::Info,
            NotificationType::Warning,
            NotificationType::Error,
            NotificationType::Success,
            NotificationType::System,
            NotificationType::AiAgent,
        ];
        for t in types {
            config.type_config.set_enabled(t, false);
            assert!(!config.type_config.is_enabled(t));
            assert!(!config.should_show(t, NotificationPriority::Normal, None));
            config.type_config.set_enabled(t, true);
        }

        let priorities = [
            NotificationPriority::Low,
            NotificationPriority::Normal,
            NotificationPriority::High,
            NotificationPriority::Critical,
        ];
        for p in priorities {
            config.priority_config.set_enabled(p, false);
            assert!(!config.priority_config.is_enabled(p));
            assert!(!config.should_show(NotificationType::Info, p, None));
            config.priority_config.set_enabled(p, true);
        }
    }

    #[test]
    fn test_durations_and_defaults() {
        let mut config = NotificationConfig::default();
        assert_eq!(config.default_duration(), None);
        config.default_duration_seconds = Some(3);
        assert_eq!(config.default_duration(), Some(Duration::from_secs(3)));

        let desktop = DesktopNotificationConfig::default();
        assert_eq!(desktop.timeout(), Some(Duration::from_secs(5)));
        assert_eq!(desktop.min_priority, NotificationPriority::High);

        let keys = NotificationKeybindings::default();
        assert_eq!(keys.jump_to_unread, "Ctrl+Shift+U");
        assert_eq!(<NotificationKeybindings as Default>::default().close, "Esc");
        assert!(<NotificationTypeConfig as Default>::default().info_enabled);
        assert!(<NotificationPriorityConfig as Default>::default().critical_enabled);
        assert!(!<DesktopNotificationConfig as Default>::default().enabled);

        // serde ida e volta
        let json = serde_json::to_string(&config).unwrap();
        let back: NotificationConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.default_duration_seconds, Some(3));
        assert_eq!(back.sort_order, SortOrder::NewestFirst);
    }
}
