//! Integração com hooks de agentes de código (Claude Code e compatíveis).
//!
//! O Claude Code chama um comando a cada evento e manda um JSON pela entrada
//! padrão (`hook_event_name`, `message`...). O `code-terminal-cli agent-hook`
//! transforma os eventos que pedem atenção em notificação no painel onde o
//! agente roda: o painel ganha o anel, a tab ganha o contador e, se você não
//! estiver olhando, sai uma notificação do desktop.

use serde_json::{json, Value};

/// Aviso a mandar para o terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookNotification {
    pub title: String,
    pub body: String,
}

/// Converte o JSON de um hook em aviso. Eventos que não pedem atenção
/// (antes/depois de ferramentas, prompt enviado...) devolvem `None`.
pub fn notification_from_hook(agent: &str, payload: &Value) -> Option<HookNotification> {
    let event = payload
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("Notification");
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty());
    let body = match event {
        "Notification" => message.unwrap_or("needs your attention").to_string(),
        "Stop" => message.unwrap_or("finished").to_string(),
        "SubagentStop" => message.unwrap_or("subagent finished").to_string(),
        _ => return None,
    };
    let project = payload
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(|cwd| std::path::Path::new(cwd).file_name())
        .and_then(|name| name.to_str());
    let title = match project {
        Some(project) => format!("{agent} · {project}"),
        None => agent.to_string(),
    };
    Some(HookNotification { title, body })
}

/// O que um hook conta ao daemon das abas persistentes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentReport {
    /// `claude` para o Claude Code (o que sabe retomar por id).
    pub kind: String,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    /// `false` no fim da conversa (`SessionEnd`).
    pub active: bool,
}

/// Conversa do agente vista num hook (qualquer evento com `session_id`).
pub fn agent_report(agent: &str, payload: &Value) -> Option<AgentReport> {
    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())?;
    let event = payload
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let kind = if agent.to_lowercase().contains("claude") {
        "claude".to_string()
    } else {
        agent.to_lowercase()
    };
    Some(AgentReport {
        kind,
        session_id: Some(session_id.to_string()),
        cwd: payload
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_string),
        active: event != "SessionEnd",
    })
}

/// Trecho de `~/.claude/settings.json` que liga os hooks ao Code Terminal.
pub fn claude_settings_snippet() -> Value {
    let hook = |event: &str| {
        json!({
            event: [{
                "hooks": [{
                    "type": "command",
                    "command": "code-terminal-cli agent-hook --agent 'Claude Code'"
                }]
            }]
        })
    };
    let mut hooks = serde_json::Map::new();
    // SessionStart/SessionEnd: a aba persistente guarda a conversa para
    // oferecer `claude --resume` depois de reiniciar a máquina.
    for event in [
        "Notification",
        "Stop",
        "SubagentStop",
        "SessionStart",
        "SessionEnd",
    ] {
        if let Value::Object(entry) = hook(event) {
            hooks.extend(entry);
        }
    }
    json!({ "hooks": hooks })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_report_from_any_event_with_session() {
        let start = json!({
            "hook_event_name": "SessionStart",
            "session_id": "abc-123",
            "cwd": "/home/eu/proj"
        });
        let report = agent_report("Claude Code", &start).unwrap();
        assert_eq!(report.kind, "claude");
        assert_eq!(report.session_id.as_deref(), Some("abc-123"));
        assert_eq!(report.cwd.as_deref(), Some("/home/eu/proj"));
        assert!(report.active);
        let end = json!({"hook_event_name": "SessionEnd", "session_id": "abc-123"});
        assert!(!agent_report("Claude Code", &end).unwrap().active);
        assert_eq!(
            agent_report("Claude Code", &json!({"hook_event_name": "Stop"})),
            None
        );
        assert!(notification_from_hook("Claude Code", &start).is_none());
        let snippet = claude_settings_snippet().to_string();
        assert!(snippet.contains("SessionStart") && snippet.contains("SessionEnd"));
    }

    #[test]
    fn test_notification_events_become_notifications() {
        let payload = json!({
            "hook_event_name": "Notification",
            "message": "Claude needs your permission to use Bash",
            "cwd": "/home/example/projetos/codeterminal"
        });
        assert_eq!(
            notification_from_hook("Claude Code", &payload),
            Some(HookNotification {
                title: "Claude Code · codeterminal".into(),
                body: "Claude needs your permission to use Bash".into(),
            })
        );
        let stop = json!({"hook_event_name": "Stop"});
        assert_eq!(
            notification_from_hook("Claude Code", &stop).unwrap().body,
            "finished"
        );
        let sub = json!({"hook_event_name": "SubagentStop", "message": "  "});
        assert_eq!(
            notification_from_hook("Agent", &sub).unwrap(),
            HookNotification {
                title: "Agent".into(),
                body: "subagent finished".into()
            }
        );
    }

    #[test]
    fn test_other_events_are_ignored() {
        for event in [
            "PreToolUse",
            "PostToolUse",
            "UserPromptSubmit",
            "SessionStart",
        ] {
            let payload = json!({ "hook_event_name": event, "message": "x" });
            assert_eq!(
                notification_from_hook("Claude Code", &payload),
                None,
                "{event}"
            );
        }
    }

    #[test]
    fn test_settings_snippet_covers_attention_events() {
        let snippet = claude_settings_snippet();
        // SessionStart/SessionEnd: a aba persistente guarda a conversa para
        // oferecer `claude --resume` depois de reiniciar a máquina.
        for event in [
            "Notification",
            "Stop",
            "SubagentStop",
            "SessionStart",
            "SessionEnd",
        ] {
            let command = &snippet["hooks"][event][0]["hooks"][0]["command"];
            assert!(
                command
                    .as_str()
                    .unwrap()
                    .starts_with("code-terminal-cli agent-hook"),
                "{event}"
            );
        }
    }
}
