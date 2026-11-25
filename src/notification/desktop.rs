//! Desktop Notifications
//!
//! Implementa integração com o sistema de notificações do OS.

use super::{Notification, NotificationPriority};
use std::process::Command;

/// Interface para notificações do desktop
pub struct DesktopNotifier;

impl DesktopNotifier {
    /// Cria um novo notificador
    pub fn new() -> Self {
        Self
    }

    /// Envia uma notificação para o desktop
    pub fn notify(&self, notification: &Notification, options: &DesktopNotificationOptions) -> bool {
        if !options.enabled {
            return false;
        }

        let title = &notification.title;
        let message = &notification.message;
        
        #[cfg(target_os = "macos")]
        return self.notify_macos(title, message, notification.priority, options);
        
        #[cfg(target_os = "linux")]
        return self.notify_linux(title, message, notification.priority, options);
        
        #[cfg(target_os = "windows")]
        return self.notify_windows(title, message, notification.priority, options);
        
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        return false;
    }

    /// Notificação no macOS usando osascript
    #[cfg(target_os = "macos")]
    fn notify_macos(
        &self,
        title: &str,
        message: &str,
        priority: NotificationPriority,
        _options: &DesktopNotificationOptions,
    ) -> bool {
        let sound = match priority {
            NotificationPriority::Critical => "Basso",
            NotificationPriority::High => "Glass",
            _ => "",
        };

        let script = if sound.is_empty() {
            format!(
                r#"display notification "{}" with title "{}""#,
                Self::escape_applescript(message),
                Self::escape_applescript(title)
            )
        } else {
            format!(
                r#"display notification "{}" with title "{}" sound name "{}""#,
                Self::escape_applescript(message),
                Self::escape_applescript(title),
                sound
            )
        };

        Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Notificação no Linux usando notify-send
    #[cfg(target_os = "linux")]
    fn notify_linux(
        &self,
        title: &str,
        message: &str,
        priority: NotificationPriority,
        _options: &DesktopNotificationOptions,
    ) -> bool {
        let urgency = match priority {
            NotificationPriority::Low => "low",
            NotificationPriority::Normal => "normal",
            NotificationPriority::High => "critical",
            NotificationPriority::Critical => "critical",
        };

        Command::new("notify-send")
            .arg("-u")
            .arg(urgency)
            .arg("-a")
            .arg("Codeterm")
            .arg(title)
            .arg(message)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Notificação no Windows usando PowerShell
    #[cfg(target_os = "windows")]
    fn notify_windows(
        &self,
        title: &str,
        message: &str,
        _priority: NotificationPriority,
        _options: &DesktopNotificationOptions,
    ) -> bool {
        let ps_script = format!(
            r#"Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show("{}", "{}")"#,
            Self::escape_powershell(message),
            Self::escape_powershell(title)
        );

        Command::new("powershell.exe")
            .arg("-Command")
            .arg(&ps_script)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    /// Escapa string para AppleScript
    #[cfg(target_os = "macos")]
    fn escape_applescript(s: &str) -> String {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }

    /// Escapa string para PowerShell
    #[cfg(target_os = "windows")]
    fn escape_powershell(s: &str) -> String {
        s.replace('"', "`\"").replace('\n', "`n")
    }

    /// Verifica se notificações desktop estão disponíveis
    pub fn is_available(&self) -> bool {
        #[cfg(target_os = "macos")]
        return Command::new("which")
            .arg("osascript")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);

        #[cfg(target_os = "linux")]
        return Command::new("which")
            .arg("notify-send")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);

        #[cfg(target_os = "windows")]
        return true; // PowerShell está sempre disponível no Windows

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        return false;
    }
}

impl Default for DesktopNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Opções para notificações desktop
#[derive(Debug, Clone)]
pub struct DesktopNotificationOptions {
    /// Habilitar notificações desktop
    pub enabled: bool,
    /// Mostrar ícone
    pub show_icon: bool,
    /// Tempo de exibição em segundos
    pub timeout_secs: u64,
    /// Som de notificação
    pub sound: bool,
}

impl Default for DesktopNotificationOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            show_icon: true,
            timeout_secs: 5,
            sound: false,
        }
    }
}

impl DesktopNotificationOptions {
    /// Cria opções com notificações desabilitadas
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Habilita notificações
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }
}

/// Builder para opções de notificação
pub struct DesktopNotificationOptionsBuilder {
    options: DesktopNotificationOptions,
}

impl DesktopNotificationOptionsBuilder {
    /// Cria um novo builder
    pub fn new() -> Self {
        Self {
            options: DesktopNotificationOptions::default(),
        }
    }

    /// Define se está habilitado
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.options.enabled = enabled;
        self
    }

    /// Define se mostra ícone
    pub fn show_icon(mut self, show: bool) -> Self {
        self.options.show_icon = show;
        self
    }

    /// Define timeout
    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.options.timeout_secs = secs;
        self
    }

    /// Define se toca som
    pub fn sound(mut self, sound: bool) -> Self {
        self.options.sound = sound;
        self
    }

    /// Constrói as opções
    pub fn build(self) -> DesktopNotificationOptions {
        self.options
    }
}

impl Default for DesktopNotificationOptionsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::{Notification, NotificationType};

    #[test]
    fn test_desktop_notifier_creation() {
        let notifier = DesktopNotifier::new();
        assert!(notifier.is_available() || !notifier.is_available()); // Verifica que não panica
    }

    #[test]
    fn test_desktop_notification_options_default() {
        let options = DesktopNotificationOptions::default();
        assert!(options.enabled);
        assert!(options.show_icon);
        assert_eq!(options.timeout_secs, 5);
        assert!(!options.sound);
    }

    #[test]
    fn test_desktop_notification_options_disabled() {
        let options = DesktopNotificationOptions::disabled();
        assert!(!options.enabled);
    }

    #[test]
    fn test_options_builder() {
        let options = DesktopNotificationOptionsBuilder::new()
            .enabled(true)
            .sound(true)
            .timeout_secs(10)
            .build();

        assert!(options.enabled);
        assert!(options.sound);
        assert_eq!(options.timeout_secs, 10);
    }

    #[test]
    fn test_notify_disabled() {
        let notifier = DesktopNotifier::new();
        let notification = Notification::new(
            NotificationType::Info,
            "Test",
            "Test message",
        );
        let options = DesktopNotificationOptions::disabled();

        // Deve retornar false quando desabilitado
        assert!(!notifier.notify(&notification, &options));
    }
}
