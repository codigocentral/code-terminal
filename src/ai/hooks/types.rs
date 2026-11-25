//! Tipos e estruturas fundamentais para o sistema de hooks

use serde::{Deserialize, Serialize};
use std::fmt;

/// Tipo de agente AI suportado
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AgentType {
    /// Claude Code (Anthropic)
    ClaudeCode,
    /// OpenCode (SaaS/Freemium)
    OpenCode,
    /// Aider (Open source)
    Aider,
    /// Agente genérico
    #[default]
    Generic,
}

impl AgentType {
    /// Retorna o nome legível do agente
    pub fn display_name(&self) -> &'static str {
        match self {
            AgentType::ClaudeCode => "Claude Code",
            AgentType::OpenCode => "OpenCode",
            AgentType::Aider => "Aider",
            AgentType::Generic => "Generic AI Agent",
        }
    }

    /// Retorna identificadores de processo típicos para este agente
    pub fn process_identifiers(&self) -> &'static [&'static str] {
        match self {
            AgentType::ClaudeCode => &["claude", "claude-code", "anthropic"],
            AgentType::OpenCode => &["opencode", "opencode-agent"],
            AgentType::Aider => &["aider", "aider-chat"],
            AgentType::Generic => &["ai-agent", "assistant"],
        }
    }

    /// Retorna variáveis de ambiente típicas para este agente
    pub fn env_identifiers(&self) -> &'static [&'static str] {
        match self {
            AgentType::ClaudeCode => &["CLAUDE_CODE", "ANTHROPIC_API_KEY"],
            AgentType::OpenCode => &["OPENCODE_API_KEY", "OPENCODE_TOKEN"],
            AgentType::Aider => &["AIDER_MODEL", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"],
            AgentType::Generic => &["AI_AGENT", "ASSISTANT_ENABLED"],
        }
    }

    /// Detecta se este agente está ativo no ambiente
    pub fn detect(&self) -> bool {
        use std::env;

        // Verificar variáveis de ambiente
        for var in self.env_identifiers() {
            if env::var(var).is_ok() {
                return true;
            }
        }

        // Verificar processos (implementação específica por plataforma)
        self.detect_process()
    }

    #[cfg(target_os = "windows")]
    fn detect_process(&self) -> bool {
        // Implementação Windows: verificar via tasklist ou processos
        use std::process::Command;

        for identifier in self.process_identifiers() {
            if let Ok(output) = Command::new("tasklist")
                .args(["/FI", &format!("IMAGENAME eq {}*", identifier)])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.contains(identifier) {
                    return true;
                }
            }
        }
        false
    }

    #[cfg(not(target_os = "windows"))]
    fn detect_process(&self) -> bool {
        // Implementação Unix: verificar via pgrep ou /proc
        use std::process::Command;

        for identifier in self.process_identifiers() {
            if Command::new("pgrep")
                .arg(identifier)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return true;
            }
        }
        false
    }
}

impl fmt::Display for AgentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Status de um agente
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Agente não detectado
    NotDetected,
    /// Agente detectado mas não ativo
    Inactive,
    /// Agente ativo e pronto
    Active,
    /// Agente ocupado processando
    Busy,
    /// Agente em erro
    Error,
    /// Agente desconectado
    Disconnected,
}

impl AgentStatus {
    /// Verifica se o agente está disponível para uso
    pub fn is_available(&self) -> bool {
        matches!(self, AgentStatus::Active | AgentStatus::Inactive)
    }

    /// Verifica se o agente está em operação
    pub fn is_operational(&self) -> bool {
        matches!(self, AgentStatus::Active | AgentStatus::Busy)
    }
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentStatus::NotDetected => write!(f, "not_detected"),
            AgentStatus::Inactive => write!(f, "inactive"),
            AgentStatus::Active => write!(f, "active"),
            AgentStatus::Busy => write!(f, "busy"),
            AgentStatus::Error => write!(f, "error"),
            AgentStatus::Disconnected => write!(f, "disconnected"),
        }
    }
}

/// Informações sobre um agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Tipo do agente
    pub agent_type: AgentType,
    /// Nome do agente
    pub name: String,
    /// Versão do agente (se disponível)
    pub version: Option<String>,
    /// Status atual
    pub status: AgentStatus,
    /// Capacidades do agente
    pub capabilities: Vec<String>,
    /// Metadados adicionais
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl AgentInfo {
    /// Cria informações básicas de um agente
    pub fn new(agent_type: AgentType, name: impl Into<String>) -> Self {
        Self {
            agent_type,
            name: name.into(),
            version: None,
            status: AgentStatus::NotDetected,
            capabilities: Vec::new(),
            metadata: serde_json::Map::new(),
        }
    }

    /// Define a versão do agente
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Adiciona uma capacidade
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self
    }

    /// Adiciona metadado
    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// Tipo de hook
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookType {
    /// Hook chamado quando o terminal inicia
    OnTerminalStart,
    /// Hook chamado quando o terminal fecha
    OnTerminalExit,
    /// Hook chamado quando uma aba é criada
    OnTabCreated,
    /// Hook chamado quando uma aba é fechada
    OnTabClosed,
    /// Hook chamado quando o conteúdo muda
    OnContentChange,
    /// Hook chamado em intervalos regulares
    OnInterval,
    /// Hook chamado quando um comando é executado
    OnCommandExecute,
    /// Hook chamado quando um comando termina
    OnCommandComplete,
    /// Hook chamado em caso de erro
    OnError,
    /// Hook chamado para solicitar sugestão
    OnSuggestionRequest,
}

/// Prioridade de execução de um hook
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum HookPriority {
    /// Prioridade mais alta - executado primeiro
    Critical = 0,
    /// Prioridade alta
    High = 1,
    /// Prioridade normal (padrão)
    #[default]
    Normal = 2,
    /// Prioridade baixa
    Low = 3,
    /// Prioridade mais baixa - executado por último
    Background = 4,
}

impl HookPriority {
    /// Retorna o valor numérico da prioridade
    pub fn value(&self) -> u8 {
        *self as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_type_display() {
        assert_eq!(AgentType::ClaudeCode.display_name(), "Claude Code");
        assert_eq!(AgentType::OpenCode.display_name(), "OpenCode");
        assert_eq!(AgentType::Aider.display_name(), "Aider");
        assert_eq!(AgentType::Generic.display_name(), "Generic AI Agent");
    }

    #[test]
    fn test_agent_type_process_identifiers() {
        assert!(AgentType::ClaudeCode
            .process_identifiers()
            .contains(&"claude"));
        assert!(AgentType::OpenCode
            .process_identifiers()
            .contains(&"opencode"));
        assert!(AgentType::Aider.process_identifiers().contains(&"aider"));
    }

    #[test]
    fn test_agent_status_availability() {
        assert!(AgentStatus::Active.is_available());
        assert!(AgentStatus::Inactive.is_available());
        assert!(!AgentStatus::Busy.is_available());
        assert!(!AgentStatus::Error.is_available());
    }

    #[test]
    fn test_agent_status_operational() {
        assert!(AgentStatus::Active.is_operational());
        assert!(AgentStatus::Busy.is_operational());
        assert!(!AgentStatus::Inactive.is_operational());
        assert!(!AgentStatus::Error.is_operational());
    }

    #[test]
    fn test_hook_priority_ordering() {
        assert!(HookPriority::Critical < HookPriority::Normal);
        assert!(HookPriority::Normal < HookPriority::Background);
        assert_eq!(HookPriority::High.value(), 1);
        assert_eq!(HookPriority::default().value(), 2);
    }

    #[test]
    fn test_agent_info_builder() {
        let info = AgentInfo::new(AgentType::ClaudeCode, "My Claude")
            .with_version("1.0.0")
            .with_capability("code_edit")
            .with_metadata("model", "claude-3-sonnet");

        assert_eq!(info.name, "My Claude");
        assert_eq!(info.version, Some("1.0.0".to_string()));
        assert!(info.capabilities.contains(&"code_edit".to_string()));
        assert!(info.metadata.contains_key("model"));
    }
}
