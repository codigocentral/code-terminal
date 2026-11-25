//! OSC Notification Handler
//!
//! Processa sequences OSC 9/99/777 para notificações.
//!
//! # OSC 9 - iTerm2 Growl notifications
//! Format: `\x1b]9;Message\x07`
//!
//! # OSC 99 - Notification
//! Format: `\x1b]99;key=value:key2=value2\x07`
//!
//! # OSC 777 - rxvt-unicode / urxvt
//! Format: `\x1b]777;notify;title;body\x07`

use super::{Notification, NotificationPriority, NotificationType};

/// Parser e handler de OSC notifications
pub struct OscNotificationHandler;

impl OscNotificationHandler {
    /// Cria um novo handler
    pub fn new() -> Self {
        Self
    }

    /// Parseia uma sequence OSC e retorna notificação se aplicável
    pub fn parse(&self, osc_sequence: &str) -> Option<Notification> {
        // Remove ESC] ou OSC prefix
        let content = if let Some(stripped) = osc_sequence.strip_prefix("\x1b]") {
            stripped
        } else {
            osc_sequence
        };

        // Remove terminator (BEL ou ESC\\)
        let content = if let Some(stripped) = content.strip_suffix('\x07') {
            stripped
        } else if let Some(stripped) = content.strip_suffix("\x1b\\") {
            stripped
        } else {
            content
        };

        // Identifica o tipo de OSC
        if let Some(stripped) = content.strip_prefix("9;") {
            self.parse_osc_9(stripped)
        } else if let Some(stripped) = content.strip_prefix("99;") {
            self.parse_osc_99(stripped)
        } else if let Some(stripped) = content.strip_prefix("777;") {
            self.parse_osc_777(stripped)
        } else {
            None
        }
    }

    /// Parseia OSC 9 (iTerm2 Growl)
    /// Format: `9;Message` ou `9;Title|Message`
    fn parse_osc_9(&self, content: &str) -> Option<Notification> {
        if content.is_empty() {
            return None;
        }

        let parts: Vec<&str> = content.split('|').collect();

        let (title, message) = if parts.len() >= 2 {
            (parts[0].to_string(), parts[1..].join("|"))
        } else {
            ("Notification".to_string(), content.to_string())
        };

        Some(
            Notification::new(NotificationType::System, title, message)
                .with_priority(NotificationPriority::Normal),
        )
    }

    /// Parseia OSC 99 (Extended notification)
    /// Format: `99;key=value:key2=value2`
    /// Keys: title, body, type, priority
    fn parse_osc_99(&self, content: &str) -> Option<Notification> {
        if content.is_empty() {
            return None;
        }

        let mut title = None;
        let mut body = None;
        let mut notification_type = NotificationType::System;
        let mut priority = NotificationPriority::Normal;

        // Parse key=value pairs separados por :
        for pair in content.split(':') {
            let kv: Vec<&str> = pair.splitn(2, '=').collect();
            if kv.len() != 2 {
                continue;
            }

            let key = kv[0].trim().to_lowercase();
            let value = kv[1].trim();

            match key.as_str() {
                "title" => title = Some(value.to_string()),
                "body" | "message" | "msg" => body = Some(value.to_string()),
                "type" | "kind" => {
                    notification_type = match value.to_lowercase().as_str() {
                        "info" => NotificationType::Info,
                        "warning" | "warn" => NotificationType::Warning,
                        "error" | "err" => NotificationType::Error,
                        "success" => NotificationType::Success,
                        "ai" | "agent" | "aiagent" => NotificationType::AiAgent,
                        _ => NotificationType::System,
                    };
                }
                "priority" | "prio" => {
                    priority = match value.to_lowercase().as_str() {
                        "low" => NotificationPriority::Low,
                        "normal" | "med" => NotificationPriority::Normal,
                        "high" => NotificationPriority::High,
                        "critical" | "crit" => NotificationPriority::Critical,
                        _ => NotificationPriority::Normal,
                    };
                }
                _ => {}
            }
        }

        let title = title.unwrap_or_else(|| "Notification".to_string());
        let body = body.unwrap_or_else(|| content.to_string());

        Some(Notification::new(notification_type, title, body).with_priority(priority))
    }

    /// Parseia OSC 777 (rxvt-unicode)
    /// Format: `777;notify;title;body`
    fn parse_osc_777(&self, content: &str) -> Option<Notification> {
        let parts: Vec<&str> = content.split(';').collect();

        if parts.len() < 3 || parts[0] != "notify" {
            return None;
        }

        let title = parts[1].to_string();
        let body = parts[2..].join(";");

        Some(
            Notification::new(NotificationType::System, title, body)
                .with_priority(NotificationPriority::Normal),
        )
    }

    /// Verifica se uma string contém OSC notification
    pub fn contains_osc(&self, data: &str) -> bool {
        data.contains("\x1b]9;") || data.contains("\x1b]99;") || data.contains("\x1b]777;")
    }

    /// Extrai e processa todas as OSC notifications de uma string
    pub fn extract_all(&self, data: &str) -> (String, Vec<Notification>) {
        let mut notifications = Vec::new();
        let mut result = String::with_capacity(data.len());

        let mut i = 0;
        while i < data.len() {
            // Procura por OSC
            if let Some(pos) = data[i..].find("\x1b]") {
                let start = i + pos;

                // Copia conteúdo antes da OSC
                result.push_str(&data[i..start]);

                // Encontra o fim da OSC
                if let Some(end) = self.find_osc_end(&data[start..]) {
                    let osc_sequence = &data[start..start + end];

                    // Tenta parsear; OSC que não é notificação (título da
                    // janela, hyperlink, cwd...) segue intacta para o terminal
                    match self.parse(osc_sequence) {
                        Some(notification) => notifications.push(notification),
                        None => result.push_str(osc_sequence),
                    }

                    i = start + end;
                } else {
                    // OSC malformada, copia e continua
                    result.push_str(&data[start..start + 2]);
                    i = start + 2;
                }
            } else {
                // Não há mais OSC
                result.push_str(&data[i..]);
                break;
            }
        }

        (result, notifications)
    }

    /// Encontra o fim de uma sequence OSC
    fn find_osc_end(&self, data: &str) -> Option<usize> {
        for (i, c) in data.char_indices() {
            if c == '\x07' {
                return Some(i + 1);
            }
            if data[i..].starts_with("\x1b\\") {
                return Some(i + 2);
            }
        }
        None
    }
}

impl Default for OscNotificationHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Gera sequences OSC para notificações
pub struct OscNotificationGenerator;

impl OscNotificationGenerator {
    /// Gera OSC 9
    pub fn osc_9(title: &str, message: &str) -> String {
        format!("\x1b]9;{}|{}\x07", title, message)
    }

    /// Gera OSC 99
    pub fn osc_99(
        title: &str,
        message: &str,
        notification_type: Option<NotificationType>,
        priority: Option<NotificationPriority>,
    ) -> String {
        let mut params = vec![format!("title={}", title), format!("body={}", message)];

        if let Some(nt) = notification_type {
            params.push(format!("type={:?}", nt).to_lowercase());
        }

        if let Some(p) = priority {
            params.push(format!("priority={:?}", p).to_lowercase());
        }

        format!("\x1b]99;{}\x07", params.join(":"))
    }

    /// Gera OSC 777
    pub fn osc_777(title: &str, message: &str) -> String {
        format!("\x1b]777;notify;{};{}\x07", title, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_osc_9_simple() {
        let handler = OscNotificationHandler::new();
        let notif = handler.parse_osc_9("Hello World").unwrap();

        assert_eq!(notif.title, "Notification");
        assert_eq!(notif.message, "Hello World");
    }

    #[test]
    fn test_parse_osc_9_with_title() {
        let handler = OscNotificationHandler::new();
        let notif = handler
            .parse_osc_9("Build Complete|Your build finished successfully")
            .unwrap();

        assert_eq!(notif.title, "Build Complete");
        assert_eq!(notif.message, "Your build finished successfully");
    }

    #[test]
    fn test_parse_osc_99_basic() {
        let handler = OscNotificationHandler::new();
        let osc = "title=Test:title=Test:message=Hello World";
        let notif = handler.parse_osc_99(osc).unwrap();

        assert_eq!(notif.title, "Test");
    }

    #[test]
    fn test_parse_osc_99_full() {
        let handler = OscNotificationHandler::new();
        let osc = "title=Build Error:body=Compilation failed:type=error:priority=high";
        let notif = handler.parse_osc_99(osc).unwrap();

        assert_eq!(notif.title, "Build Error");
        assert_eq!(notif.message, "Compilation failed");
        assert_eq!(notif.notification_type, NotificationType::Error);
        assert_eq!(notif.priority, NotificationPriority::High);
    }

    #[test]
    fn test_parse_osc_777() {
        let handler = OscNotificationHandler::new();
        let osc = "notify;Test Title;Test body here";
        let notif = handler.parse_osc_777(osc).unwrap();

        assert_eq!(notif.title, "Test Title");
        assert_eq!(notif.message, "Test body here");
    }

    #[test]
    fn test_full_parse_osc_9() {
        let handler = OscNotificationHandler::new();
        let osc = "\x1b]9;Test Title|Test message\x07";
        let notif = handler.parse(osc).unwrap();

        assert_eq!(notif.title, "Test Title");
        assert_eq!(notif.message, "Test message");
    }

    #[test]
    fn test_contains_osc() {
        let handler = OscNotificationHandler::new();

        assert!(handler.contains_osc("\x1b]9;test\x07"));
        assert!(handler.contains_osc("\x1b]99;test\x07"));
        assert!(handler.contains_osc("\x1b]777;test\x07"));
        assert!(!handler.contains_osc("Hello world"));
    }

    #[test]
    fn test_extract_all() {
        let handler = OscNotificationHandler::new();
        let input = "Hello \x1b]9;Title|Message\x07 World";
        let (cleaned, notifications) = handler.extract_all(input);

        assert_eq!(cleaned, "Hello  World");
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].title, "Title");
    }

    #[test]
    fn test_generate_osc_9() {
        let osc = OscNotificationGenerator::osc_9("Title", "Message");
        assert!(osc.contains("\x1b]9;"));
        assert!(osc.contains("Title|Message"));
        assert!(osc.contains("\x07"));
    }

    #[test]
    fn test_generate_osc_99() {
        let osc = OscNotificationGenerator::osc_99(
            "Title",
            "Message",
            Some(NotificationType::Error),
            Some(NotificationPriority::High),
        );
        assert!(osc.contains("\x1b]99;"));
        assert!(osc.contains("title=Title"));
        assert!(osc.contains("body=Message"));
    }

    /// Regressão: OSC que não é notificação era removida da saída, quebrando
    /// título da janela (OSC 0/2), hyperlinks (OSC 8) etc.
    #[test]
    fn test_extract_all_keeps_non_notification_osc() {
        let handler = OscNotificationHandler;
        let data = "a\x1b]0;meu titulo\x07b\x1b]9;oi\x07c\x1b]8;;http://x\x1b\\d";
        let (cleaned, notifications) = handler.extract_all(data);
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].message, "oi");
        assert_eq!(cleaned, "a\x1b]0;meu titulo\x07bc\x1b]8;;http://x\x1b\\d");
    }

    #[test]
    fn test_extract_all_handles_unterminated_and_st_terminator() {
        let handler = OscNotificationHandler::new();
        // Sem terminador: mantém o texto
        let (cleaned, notifications) = handler.extract_all("x\x1b]9;sem fim");
        assert!(notifications.is_empty());
        assert_eq!(cleaned, "x\x1b]9;sem fim");

        // Terminador ST (ESC \\)
        let (cleaned, notifications) =
            handler.extract_all("\x1b]777;notify;T;corpo;extra\x1b\\fim");
        assert_eq!(cleaned, "fim");
        assert_eq!(notifications[0].title, "T");
        assert_eq!(notifications[0].message, "corpo;extra");
    }

    #[test]
    fn test_parse_variants_and_rejections() {
        let handler = OscNotificationHandler::new();
        assert!(handler.parse("\x1b]9;\x07").is_none());
        assert!(handler.parse("\x1b]99;\x07").is_none());
        assert!(handler.parse("\x1b]777;outro;a;b\x07").is_none());
        assert!(handler.parse("\x1b]777;notify;so\x07").is_none());
        assert!(handler.parse("\x1b]52;c;abc\x07").is_none());

        // Sem prefixo/terminador também é aceito
        let n = handler.parse("9;Titulo|Msg|com barra").unwrap();
        assert_eq!(n.title, "Titulo");
        assert_eq!(n.message, "Msg|com barra");

        let n = handler
            .parse("\x1b]99;msg=corpo:kind=err:prio=crit:x=y:semvalor\x07")
            .unwrap();
        assert_eq!(n.title, "Notification");
        assert_eq!(n.message, "corpo");
        assert_eq!(n.notification_type, NotificationType::Error);
        assert_eq!(n.priority, NotificationPriority::Critical);

        // Sem body: usa o conteúdo bruto
        let n = handler.parse("99;title=T").unwrap();
        assert_eq!(n.message, "title=T");

        for (value, expected) in [
            ("info", NotificationType::Info),
            ("warn", NotificationType::Warning),
            ("success", NotificationType::Success),
            ("agent", NotificationType::AiAgent),
            ("outro", NotificationType::System),
        ] {
            let n = handler.parse(&format!("99;type={value}")).unwrap();
            assert_eq!(n.notification_type, expected, "{value}");
        }
        for (value, expected) in [
            ("low", NotificationPriority::Low),
            ("med", NotificationPriority::Normal),
            ("high", NotificationPriority::High),
            ("??", NotificationPriority::Normal),
        ] {
            let n = handler.parse(&format!("99;priority={value}")).unwrap();
            assert_eq!(n.priority, expected, "{value}");
        }
    }

    #[test]
    fn test_generators_roundtrip_through_parser() {
        let handler = OscNotificationHandler::new();
        assert!(handler.contains_osc(&OscNotificationGenerator::osc_9("a", "b")));
        assert!(!handler.contains_osc("texto comum"));

        let n = handler
            .parse(&OscNotificationGenerator::osc_99(
                "T",
                "B",
                Some(NotificationType::AiAgent),
                Some(NotificationPriority::High),
            ))
            .unwrap();
        assert_eq!((n.title.as_str(), n.message.as_str()), ("T", "B"));
        assert_eq!(n.notification_type, NotificationType::AiAgent);
        assert_eq!(n.priority, NotificationPriority::High);

        let n = handler
            .parse(&OscNotificationGenerator::osc_777("T", "B"))
            .unwrap();
        assert_eq!((n.title.as_str(), n.message.as_str()), ("T", "B"));
    }
}
