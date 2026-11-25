//! API para comunicação com agentes

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Requisição para um agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    /// ID da requisição
    pub id: uuid::Uuid,
    /// Tipo de comando
    pub command: AgentCommand,
    /// Timeout para resposta
    #[serde(with = "serde_duration")]
    pub timeout: Duration,
    /// Dados adicionais
    pub data: HashMap<String, serde_json::Value>,
}

impl AgentRequest {
    /// Cria uma nova requisição
    pub fn new(command: AgentCommand) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            command,
            timeout: Duration::from_secs(30),
            data: HashMap::new(),
        }
    }

    /// Define timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Adiciona dado
    pub fn with_data(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }

    /// Cria requisição de execução de comando
    pub fn execute_command(command: impl Into<String>) -> Self {
        Self::new(AgentCommand::ExecuteCommand {
            command: command.into(),
            args: Vec::new(),
            cwd: None,
        })
    }

    /// Cria requisição de sugestão
    pub fn get_suggestion(context: impl Into<String>) -> Self {
        Self::new(AgentCommand::GetSuggestion {
            context: context.into(),
            max_suggestions: 5,
        })
    }

    /// Cria requisição de edição
    pub fn edit_file(path: impl Into<String>, instructions: impl Into<String>) -> Self {
        Self::new(AgentCommand::EditFile {
            path: path.into(),
            instructions: instructions.into(),
        })
    }
}

/// Comandos disponíveis para agentes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "payload")]
pub enum AgentCommand {
    /// Executar comando shell
    ExecuteCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        cwd: Option<String>,
    },
    /// Obter sugestão
    GetSuggestion {
        context: String,
        #[serde(default = "default_max_suggestions")]
        max_suggestions: usize,
    },
    /// Editar arquivo
    EditFile { path: String, instructions: String },
    /// Ler arquivo
    ReadFile {
        path: String,
        #[serde(default)]
        offset: Option<usize>,
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Escrever arquivo
    WriteFile { path: String, content: String },
    /// Buscar código
    SearchCode {
        query: String,
        #[serde(default)]
        language: Option<String>,
        #[serde(default)]
        path: Option<String>,
    },
    /// Explicar erro
    ExplainError {
        error_output: String,
        #[serde(default)]
        command: Option<String>,
    },
    /// Corrigir erro
    FixError {
        error_output: String,
        #[serde(default)]
        auto_apply: bool,
    },
    /// Commit git
    GitCommit {
        message: String,
        #[serde(default)]
        files: Vec<String>,
    },
    /// Analisar código
    AnalyzeCode {
        path: String,
        #[serde(default)]
        analysis_type: AnalysisType,
    },
    /// Gerar testes
    GenerateTests {
        path: String,
        #[serde(default = "default_max_tests")]
        max_tests: usize,
    },
    /// Refatorar código
    Refactor { path: String, instructions: String },
    /// Ping - verificar se agente está vivo
    Ping,
    /// Obter capacidades
    GetCapabilities,
    /// Custom command
    Custom {
        name: String,
        #[serde(default)]
        params: HashMap<String, serde_json::Value>,
    },
}

fn default_max_suggestions() -> usize {
    5
}

fn default_max_tests() -> usize {
    10
}

/// Tipo de análise
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisType {
    #[default]
    General,
    Performance,
    Security,
    Style,
    Complexity,
    Tests,
}

/// Resposta de um agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// ID da requisição (mesmo da request)
    pub request_id: uuid::Uuid,
    /// Sucesso da operação
    pub success: bool,
    /// Dados da resposta
    #[serde(flatten)]
    pub data: ResponseData,
    /// Tempo de processamento
    pub processing_time_ms: u64,
}

impl AgentResponse {
    /// Cria resposta de sucesso
    pub fn success(request_id: uuid::Uuid, data: ResponseData) -> Self {
        Self {
            request_id,
            success: true,
            data,
            processing_time_ms: 0,
        }
    }

    /// Cria resposta de erro
    pub fn error(request_id: uuid::Uuid, message: impl Into<String>) -> Self {
        Self {
            request_id,
            success: false,
            data: ResponseData::Error {
                message: message.into(),
                code: None,
                details: None,
            },
            processing_time_ms: 0,
        }
    }

    /// Define tempo de processamento
    pub fn with_processing_time(mut self, ms: u64) -> Self {
        self.processing_time_ms = ms;
        self
    }
}

/// Dados da resposta
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", untagged)]
pub enum ResponseData {
    /// Sucesso - sem dados
    Success,
    /// Texto simples
    Text { text: String },
    /// Resultado de execução
    ExecutionResult {
        stdout: String,
        stderr: String,
        exit_code: i32,
    },
    /// Sugestões
    Suggestions { suggestions: Vec<Suggestion> },
    /// Conteúdo de arquivo
    FileContent {
        path: String,
        content: String,
        #[serde(default)]
        encoding: String,
    },
    /// Resultado de busca
    SearchResults {
        results: Vec<SearchResult>,
        total: usize,
    },
    /// Explicação de erro
    ErrorExplanation {
        explanation: String,
        suggestions: Vec<String>,
        #[serde(default)]
        fix_available: bool,
    },
    /// Resultado de análise
    Analysis {
        summary: String,
        issues: Vec<Issue>,
        score: Option<f32>,
    },
    /// Capacidades
    Capabilities {
        capabilities: Vec<AgentCapability>,
        version: String,
    },
    /// Ping response
    Pong { timestamp: u64 },
    /// Erro
    Error {
        message: String,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        details: Option<serde_json::Value>,
    },
}

/// Sugestão
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    /// Texto da sugestão
    pub text: String,
    /// Descrição
    pub description: String,
    /// Confiança (0-1)
    pub confidence: f32,
    /// Ação a ser executada
    pub action: Option<SuggestionAction>,
}

/// Ação de sugestão
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionAction {
    pub action_type: ActionType,
    pub payload: String,
}

/// Tipo de ação
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    InsertText,
    ExecuteCommand,
    OpenFile,
    ShowHelp,
}

/// Resultado de busca
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub text: String,
    pub score: f32,
}

/// Issue encontrada
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub severity: IssueSeverity,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub code: Option<String>,
}

/// Severidade de issue
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

/// Capacidade do agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCapability {
    pub name: String,
    pub description: String,
    pub version: String,
    pub requires_permission: bool,
}

/// API trait para agentes
#[async_trait::async_trait]
pub trait AgentApi: Send + Sync {
    /// Processa uma requisição
    async fn process_request(&self, request: AgentRequest) -> AgentResponse;

    /// Retorna capacidades
    fn capabilities(&self) -> Vec<AgentCapability>;

    /// Verifica se está saudável
    async fn health_check(&self) -> bool;
}

/// Capacidades do agente
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentCapabilities {
    pub can_execute_commands: bool,
    pub can_edit_files: bool,
    pub can_analyze_code: bool,
    pub can_generate_tests: bool,
    pub can_refactor: bool,
    pub supports_streaming: bool,
    pub supports_files: bool,
    pub max_file_size: Option<usize>,
    pub supported_languages: Vec<String>,
}

impl AgentCapabilities {
    /// Cria capacidades básicas
    pub fn basic() -> Self {
        Self {
            can_execute_commands: false,
            can_edit_files: false,
            supports_streaming: false,
            supports_files: true,
            max_file_size: Some(1024 * 1024), // 1MB
            ..Default::default()
        }
    }

    /// Cria capacidades completas
    pub fn full() -> Self {
        Self {
            can_execute_commands: true,
            can_edit_files: true,
            can_analyze_code: true,
            can_generate_tests: true,
            can_refactor: true,
            supports_streaming: true,
            supports_files: true,
            max_file_size: Some(10 * 1024 * 1024), // 10MB
            supported_languages: vec![
                "rust".to_string(),
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
            ],
        }
    }
}

/// Módulo auxiliar para serialização de Duration
mod serde_duration {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_request() {
        let req = AgentRequest::execute_command("ls -la").with_timeout(Duration::from_secs(10));

        assert_eq!(req.timeout, Duration::from_secs(10));
        assert!(matches!(req.command, AgentCommand::ExecuteCommand { .. }));
    }

    #[test]
    fn test_agent_response() {
        let id = uuid::Uuid::new_v4();
        let resp = AgentResponse::success(
            id,
            ResponseData::Text {
                text: "Hello".to_string(),
            },
        );

        assert!(resp.success);
        assert_eq!(resp.request_id, id);
    }

    #[test]
    fn test_agent_response_error() {
        let id = uuid::Uuid::new_v4();
        let resp = AgentResponse::error(id, "Something went wrong");

        assert!(!resp.success);
    }

    #[test]
    fn test_suggestion() {
        let sug = Suggestion {
            text: "cargo build".to_string(),
            description: "Build the project".to_string(),
            confidence: 0.95,
            action: Some(SuggestionAction {
                action_type: ActionType::ExecuteCommand,
                payload: "cargo build".to_string(),
            }),
        };

        assert!(sug.confidence > 0.9);
    }
}
