//! Eventos do sistema de hooks

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Tipos de eventos que podem ser emitidos
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "data")]
pub enum EventType {
    /// Terminal iniciou
    TerminalStart,
    /// Terminal vai fechar
    TerminalExit { reason: ExitReason },
    /// Nova aba criada
    TabCreated { tab_id: u64, title: String },
    /// Aba fechada
    TabClosed { tab_id: u64 },
    /// Aba focada
    TabFocused { tab_id: u64 },
    /// Conteúdo do terminal mudou
    ContentChanged {
        tab_id: u64,
        changes: Vec<ContentChange>,
    },
    /// Comando executado
    CommandExecuted { tab_id: u64, command: String },
    /// Comando completou
    CommandCompleted {
        tab_id: u64,
        command: String,
        exit_code: Option<i32>,
        duration_ms: u64,
    },
    /// Erro detectado
    ErrorDetected {
        tab_id: u64,
        error_type: String,
        message: String,
        severity: ErrorSeverity,
    },
    /// Sugestão solicitada
    SuggestionRequested { tab_id: u64, context: String },
    /// Intervalo tick
    IntervalTick { count: u64 },
    /// Input do usuário
    UserInput { tab_id: u64, input: String },
    /// Seleção de texto
    TextSelected { tab_id: u64, text: String },
    /// Arquivo modificado
    FileModified { path: String, content_hash: String },
    /// Git status mudou
    GitStatusChanged { branch: String, status: GitStatus },
    /// Workspace mudou
    WorkspaceChanged { old_dir: String, new_dir: String },
    /// Agente conectou
    AgentConnected {
        agent_name: String,
        capabilities: Vec<String>,
    },
    /// Agente desconectou
    AgentDisconnected {
        agent_name: String,
        reason: DisconnectReason,
    },
    /// Custom event
    Custom {
        name: String,
        payload: serde_json::Value,
    },
}

/// Razão de saída
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    /// Usuario solicitou
    UserRequest,
    /// Sinal recebido
    Signal { signal: String },
    /// Erro
    Error { code: i32 },
    /// Atualização necessária
    UpdateRequired,
}

/// Severidade de erro
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSeverity {
    /// Informação
    Info,
    /// Aviso
    Warning,
    /// Erro
    Error,
    /// Fatal
    Fatal,
}

/// Status do git
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GitStatus {
    pub ahead: u32,
    pub behind: u32,
    pub modified: u32,
    pub staged: u32,
    pub untracked: u32,
    pub conflicted: u32,
}

/// Razão de desconexão
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectReason {
    /// Solicitado pelo usuário
    UserRequest,
    /// Timeout
    Timeout,
    /// Erro de comunicação
    CommunicationError,
    /// Agente crashou
    Crash,
    /// Versão incompatível
    IncompatibleVersion,
}

/// Mudança de conteúdo
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentChange {
    pub row: u32,
    pub col: u32,
    pub old_text: String,
    pub new_text: String,
}

/// Prioridade de evento
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum EventPriority {
    /// Crítico - processar imediatamente
    Critical = 0,
    /// Alta prioridade
    High = 1,
    /// Normal
    #[default]
    Normal = 2,
    /// Baixa
    Low = 3,
    /// Background
    Background = 4,
}

impl EventPriority {
    /// Retorna o valor numérico da prioridade
    pub fn value(&self) -> u8 {
        match self {
            EventPriority::Critical => 0,
            EventPriority::High => 1,
            EventPriority::Normal => 2,
            EventPriority::Low => 3,
            EventPriority::Background => 4,
        }
    }
}

/// Evento completo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    /// ID único do evento
    pub id: uuid::Uuid,
    /// Timestamp do evento
    pub timestamp: SystemTime,
    /// Tipo do evento
    pub event_type: EventType,
    /// Prioridade
    pub priority: EventPriority,
    /// Dados adicionais
    pub data: EventData,
    /// ID da aba (se aplicável)
    pub tab_id: Option<u64>,
    /// Metadados
    pub metadata: HashMap<String, serde_json::Value>,
}

impl AgentEvent {
    /// Cria um novo evento
    pub fn new(event_type: EventType) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            timestamp: SystemTime::now(),
            priority: EventPriority::Normal,
            event_type,
            data: EventData::default(),
            tab_id: None,
            metadata: HashMap::new(),
        }
    }

    /// Define a prioridade
    pub fn with_priority(mut self, priority: EventPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Define o ID da aba
    pub fn with_tab_id(mut self, tab_id: u64) -> Self {
        self.tab_id = Some(tab_id);
        self
    }

    /// Adiciona dado
    pub fn with_data(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.data.0.insert(key.into(), value.into());
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

    /// Cria evento de terminal iniciado
    pub fn terminal_start() -> Self {
        Self::new(EventType::TerminalStart).with_priority(EventPriority::High)
    }

    /// Cria evento de comando executado
    pub fn command_executed(tab_id: u64, command: impl Into<String>) -> Self {
        Self::new(EventType::CommandExecuted {
            tab_id,
            command: command.into(),
        })
        .with_tab_id(tab_id)
    }

    /// Cria evento de comando completado
    pub fn command_completed(
        tab_id: u64,
        command: impl Into<String>,
        exit_code: Option<i32>,
        duration: Duration,
    ) -> Self {
        Self::new(EventType::CommandCompleted {
            tab_id,
            command: command.into(),
            exit_code,
            duration_ms: duration.as_millis() as u64,
        })
        .with_tab_id(tab_id)
    }

    /// Cria evento de erro
    pub fn error_detected(
        tab_id: u64,
        error_type: impl Into<String>,
        message: impl Into<String>,
        severity: ErrorSeverity,
    ) -> Self {
        Self::new(EventType::ErrorDetected {
            tab_id,
            error_type: error_type.into(),
            message: message.into(),
            severity,
        })
        .with_tab_id(tab_id)
        .with_priority(EventPriority::High)
    }

    /// Retorna a idade do evento
    pub fn age(&self) -> Duration {
        self.timestamp.elapsed().unwrap_or(Duration::from_secs(0))
    }

    /// Verifica se o evento é de erro
    pub fn is_error(&self) -> bool {
        matches!(self.event_type, EventType::ErrorDetected { .. })
    }
}

/// Dados adicionais do evento
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventData(pub HashMap<String, serde_json::Value>);

impl EventData {
    /// Cria dados vazios
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    /// Adiciona um valor
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<serde_json::Value>) {
        self.0.insert(key.into(), value.into());
    }

    /// Obtém um valor
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.0.get(key)
    }

    /// Verifica se contém uma chave
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.contains_key(key)
    }
}

/// Filtro de eventos
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventFilter {
    /// Tipos de evento a incluir (None = todos)
    pub include_types: Option<Vec<EventType>>,
    /// Tipos de evento a excluir
    pub exclude_types: Vec<EventType>,
    /// Prioridade mínima
    pub min_priority: Option<EventPriority>,
    /// ID da aba específica
    pub tab_id: Option<u64>,
    /// Idade máxima
    pub max_age: Option<Duration>,
}

impl EventFilter {
    /// Aceita todos os eventos
    pub fn all() -> Self {
        Self::default()
    }

    /// Aceita apenas tipos específicos
    pub fn types(types: Vec<EventType>) -> Self {
        Self {
            include_types: Some(types),
            ..Default::default()
        }
    }

    /// Aceita apenas eventos de prioridade mínima
    pub fn min_priority(priority: EventPriority) -> Self {
        Self {
            min_priority: Some(priority),
            ..Default::default()
        }
    }

    /// Apenas eventos de uma aba específica
    pub fn tab(tab_id: u64) -> Self {
        Self {
            tab_id: Some(tab_id),
            ..Default::default()
        }
    }

    /// Verifica se um evento passa no filtro
    pub fn matches(&self, event: &AgentEvent) -> bool {
        // Verificar tipo
        if let Some(ref include) = self.include_types {
            if !include.contains(&event.event_type) {
                return false;
            }
        }
        if self.exclude_types.contains(&event.event_type) {
            return false;
        }

        // Verificar prioridade
        if let Some(min) = self.min_priority {
            if event.priority > min {
                return false;
            }
        }

        // Verificar aba
        if let Some(tab_id) = self.tab_id {
            if event.tab_id != Some(tab_id) {
                return false;
            }
        }

        // Verificar idade
        if let Some(max_age) = self.max_age {
            if event.age() > max_age {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_creation() {
        let event = AgentEvent::terminal_start();
        assert_eq!(event.priority, EventPriority::High);
        assert!(matches!(event.event_type, EventType::TerminalStart));
    }

    #[test]
    fn test_command_events() {
        let event = AgentEvent::command_executed(1, "ls -la");
        assert_eq!(event.tab_id, Some(1));

        let event = AgentEvent::command_completed(1, "ls", Some(0), Duration::from_secs(1));
        assert!(matches!(
            event.event_type,
            EventType::CommandCompleted { .. }
        ));
    }

    #[test]
    fn test_error_event() {
        let event =
            AgentEvent::error_detected(1, "CompileError", "syntax error", ErrorSeverity::Error);
        assert!(event.is_error());
        assert_eq!(event.priority, EventPriority::High);
    }

    #[test]
    fn test_event_filter() {
        let filter = EventFilter::min_priority(EventPriority::High);

        let high_event = AgentEvent::terminal_start();
        let low_event = AgentEvent::new(EventType::IntervalTick { count: 1 })
            .with_priority(EventPriority::Background);

        assert!(filter.matches(&high_event));
        assert!(!filter.matches(&low_event));
    }

    #[test]
    fn test_event_filter_tab() {
        let filter = EventFilter::tab(1);

        let event1 = AgentEvent::command_executed(1, "ls");
        let event2 = AgentEvent::command_executed(2, "pwd");

        assert!(filter.matches(&event1));
        assert!(!filter.matches(&event2));
    }
}
