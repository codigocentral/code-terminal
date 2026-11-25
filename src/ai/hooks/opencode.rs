//! Hooks para OpenCode (SaaS/Freemium AI Agent)

use super::{
    api::{AgentCapabilities, AgentCommand, AgentRequest, AgentResponse, ResponseData},
    error::{HookError, HookErrorExt, HookResult},
    registry::{AgentRegistration, HookHandler},
    types::{AgentInfo, AgentType},
};
use serde::{Deserialize, Serialize};
use tokio::time::{timeout, Duration};
use tracing::{info, instrument};
use url::Url;

/// Hook para OpenCode
#[derive(Debug)]
pub struct OpenCodeHook {
    /// Configuração
    config: OpenCodeConfig,
    /// Cliente HTTP
    client: reqwest::Client,
    /// Estado
    state: OpenCodeState,
}

/// Configuração do OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeConfig {
    /// API endpoint
    pub api_endpoint: String,
    /// API key
    pub api_key: String,
    /// Modelo
    pub model: String,
    /// Organização (opcional)
    pub organization: Option<String>,
    /// Timeout padrão (segundos)
    pub timeout_secs: u64,
    /// Max tokens
    pub max_tokens: u32,
    /// Temperature
    pub temperature: f32,
    /// Habilitar streaming
    pub streaming: bool,
}

impl Default for OpenCodeConfig {
    fn default() -> Self {
        Self {
            api_endpoint: "https://api.opencode.ai/v1".to_string(),
            api_key: String::new(),
            model: "opencode-pro".to_string(),
            organization: None,
            timeout_secs: 60,
            max_tokens: 4096,
            temperature: 0.7,
            streaming: true,
        }
    }
}

/// Estado do OpenCode
#[derive(Debug, Clone, Default)]
struct OpenCodeState {
    /// Taxa de requisições restante
    rate_limit_remaining: Option<u32>,
    /// Reset do rate limit
    rate_limit_reset: Option<u64>,
    /// Última atividade
    last_activity: Option<std::time::SystemTime>,
    /// Tokens usados no mês
    tokens_used_this_month: Option<u64>,
}

/// Request body para API OpenCode
#[derive(Debug, Serialize)]
struct OpenCodeApiRequest {
    model: String,
    messages: Vec<OpenCodeMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    stream: bool,
}

/// Mensagem para API OpenCode
#[derive(Debug, Serialize)]
struct OpenCodeMessage {
    role: String,
    content: String,
}

/// Resposta da API OpenCode
#[derive(Debug, Deserialize)]
struct OpenCodeApiResponse {
    id: String,
    choices: Vec<OpenCodeChoice>,
    usage: Option<OpenCodeUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenCodeChoice {
    message: OpenCodeResponseMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenCodeResponseMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenCodeUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

impl OpenCodeHook {
    /// Cria um novo hook
    pub fn new(mut config: OpenCodeConfig) -> HookResult<Self> {
        if config.api_key.is_empty() {
            return Err(HookError::ConfigError(
                "OpenCode API key is required".to_string(),
            ));
        }

        config.api_endpoint = Self::validate_api_endpoint(&config.api_endpoint)?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| {
                HookError::internal_error(format!("Failed to create HTTP client: {}", e))
            })?;

        Ok(Self {
            config,
            client,
            state: OpenCodeState::default(),
        })
    }

    /// Cria a partir de variáveis de ambiente
    pub fn from_env() -> HookResult<Self> {
        let api_key = std::env::var("OPENCODE_API_KEY")
            .map_err(|_| HookError::ConfigError("OPENCODE_API_KEY not set".to_string()))?;

        let mut config = OpenCodeConfig::default();
        config.api_key = api_key;

        if let Ok(endpoint) = std::env::var("OPENCODE_API_ENDPOINT") {
            config.api_endpoint = endpoint;
        }

        if let Ok(model) = std::env::var("OPENCODE_MODEL") {
            config.model = model;
        }

        Self::new(config)
    }

    /// Valida e normaliza o endpoint da API.
    fn validate_api_endpoint(endpoint: &str) -> HookResult<String> {
        let parsed = Url::parse(endpoint)
            .map_err(|_| HookError::ConfigError("OpenCode API endpoint is invalid".to_string()))?;

        match parsed.scheme() {
            "https" | "http" => {}
            _ => {
                return Err(HookError::ConfigError(
                    "OpenCode API endpoint must use http or https".to_string(),
                ));
            }
        }

        if parsed.host_str().is_none() {
            return Err(HookError::ConfigError(
                "OpenCode API endpoint must include a host".to_string(),
            ));
        }

        Ok(endpoint.trim_end_matches('/').to_string())
    }

    /// Detecta se OpenCode está configurado
    pub fn detect() -> bool {
        AgentType::OpenCode.detect()
    }

    /// Verifica se a API key é válida
    #[instrument(skip(self))]
    pub async fn validate_api_key(&mut self) -> HookResult<bool> {
        let response = self.make_request("Test", 10).await;

        match response {
            Ok(_) => Ok(true),
            Err(HookError::AgentError { message, .. }) if message.contains("401") => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Faz requisição para API
    #[instrument(skip(self, prompt))]
    async fn make_request(&mut self, prompt: &str, max_tokens: u32) -> HookResult<String> {
        let url = format!("{}/chat/completions", self.config.api_endpoint);

        let request_body = OpenCodeApiRequest {
            model: self.config.model.clone(),
            messages: vec![OpenCodeMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            max_tokens: Some(max_tokens),
            temperature: Some(self.config.temperature),
            stream: false,
        };

        let mut request = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json");

        if let Some(org) = &self.config.organization {
            request = request.header("OpenCode-Organization", org);
        }

        let response = timeout(
            Duration::from_secs(self.config.timeout_secs),
            request.json(&request_body).send(),
        )
        .await
        .map_err(|_| HookError::timeout(self.config.timeout_secs * 1000))?
        .map_err(|e| HookError::CommunicationError(format!("HTTP request failed: {}", e)))?;

        // Atualizar rate limits dos headers
        if let Some(remaining) = response.headers().get("x-ratelimit-remaining") {
            if let Ok(val) = remaining.to_str() {
                self.state.rate_limit_remaining = val.parse().ok();
            }
        }
        if let Some(reset) = response.headers().get("x-ratelimit-reset") {
            if let Ok(val) = reset.to_str() {
                self.state.rate_limit_reset = val.parse().ok();
            }
        }
        self.state.last_activity = Some(std::time::SystemTime::now());

        let status = response.status();
        let body = response.text().await.map_err(|e| {
            HookError::CommunicationError(format!("Failed to read response: {}", e))
        })?;

        if !status.is_success() {
            return Err(HookError::AgentError {
                agent: "opencode".to_string(),
                message: format!("API error {}: {}", status, body),
            });
        }

        let api_response: OpenCodeApiResponse = serde_json::from_str(&body).map_err(|e| {
            HookError::SerializationError(format!("Failed to parse response: {}", e))
        })?;
        if let Some(usage) = &api_response.usage {
            self.state.tokens_used_this_month = Some(
                self.state
                    .tokens_used_this_month
                    .unwrap_or_default()
                    .saturating_add(usage.total_tokens as u64),
            );
            info!(
                response_id = %api_response.id,
                prompt_tokens = usage.prompt_tokens,
                completion_tokens = usage.completion_tokens,
                total_tokens = usage.total_tokens,
                "OpenCode usage recorded"
            );
        }

        if let Some(choice) = api_response.choices.first() {
            info!(
                response_id = %api_response.id,
                role = %choice.message.role,
                finish_reason = ?choice.finish_reason,
                "OpenCode response received"
            );
            Ok(choice.message.content.clone())
        } else {
            Err(HookError::AgentError {
                agent: "opencode".to_string(),
                message: "No response from API".to_string(),
            })
        }
    }

    /// Envia prompt para OpenCode
    #[instrument(skip(self, prompt))]
    pub async fn send_prompt(&mut self, prompt: &str) -> HookResult<String> {
        self.make_request(prompt, self.config.max_tokens).await
    }

    /// Solicita sugestões
    #[instrument(skip(self, context))]
    pub async fn get_suggestions(
        &mut self,
        context: &str,
        max_suggestions: usize,
    ) -> HookResult<Vec<String>> {
        let prompt = format!(
            "Based on this terminal context, suggest {} commands or actions:\n\n{}",
            max_suggestions, context
        );

        let response = self.send_prompt(&prompt).await?;

        // Parsear resposta em sugestões
        let suggestions: Vec<String> = response
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(max_suggestions)
            .map(|l| l.trim().to_string())
            .collect();

        Ok(suggestions)
    }

    /// Explica erro
    #[instrument(skip(self, error_output))]
    pub async fn explain_error(&mut self, error_output: &str) -> HookResult<String> {
        let prompt = format!(
            "Explain this error in simple terms and suggest how to fix it:\n\n```\n{}\n```",
            error_output
        );
        self.send_prompt(&prompt).await
    }

    /// Analisa código
    #[instrument(skip(self))]
    pub async fn analyze_code(&mut self, code: &str, language: &str) -> HookResult<String> {
        let prompt = format!(
            "Analyze this {} code and provide feedback on quality, issues, and improvements:\n\n```{}\n{}\n```",
            language, language, code
        );
        self.send_prompt(&prompt).await
    }

    /// Cria agent registration
    pub fn create_registration(&self) -> AgentRegistration {
        let info = AgentInfo::new(AgentType::OpenCode, "OpenCode")
            .with_version("api")
            .with_capability("code_suggest")
            .with_capability("error_explain")
            .with_capability("code_analyze")
            .with_capability("chat")
            .with_metadata("model", self.config.model.clone())
            .with_metadata("streaming", self.config.streaming);

        AgentRegistration::new(info).with_endpoint(&self.config.api_endpoint)
    }

    /// Processa uma requisição
    #[instrument(skip(self, request))]
    pub async fn process_request(&mut self, request: AgentRequest) -> HookResult<AgentResponse> {
        let start = std::time::Instant::now();

        let result = match request.command {
            AgentCommand::GetSuggestion {
                context,
                max_suggestions,
            } => {
                let suggestions = self.get_suggestions(&context, max_suggestions).await?;
                Ok(ResponseData::Text {
                    text: suggestions.join("\n"),
                })
            }
            AgentCommand::ExplainError { error_output, .. } => {
                let explanation = self.explain_error(&error_output).await?;
                Ok(ResponseData::ErrorExplanation {
                    explanation,
                    suggestions: Vec::new(),
                    fix_available: false,
                })
            }
            AgentCommand::AnalyzeCode { path, .. } => {
                // Ler arquivo e analisar
                let code = tokio::fs::read_to_string(&path).await.map_err(|e| {
                    HookError::internal_error(format!("Failed to read file: {}", e))
                })?;
                let language = detect_language(&path);
                let analysis = self.analyze_code(&code, &language).await?;
                Ok(ResponseData::Text { text: analysis })
            }
            AgentCommand::Ping => Ok(ResponseData::Pong {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            }),
            AgentCommand::GetCapabilities => {
                let _caps = self.capabilities();
                Ok(ResponseData::Capabilities {
                    capabilities: vec![
                        super::api::AgentCapability {
                            name: "chat".to_string(),
                            description: "General chat and Q&A".to_string(),
                            version: "1.0".to_string(),
                            requires_permission: false,
                        },
                        super::api::AgentCapability {
                            name: "code_suggest".to_string(),
                            description: "Suggest code improvements".to_string(),
                            version: "1.0".to_string(),
                            requires_permission: false,
                        },
                    ],
                    version: "1.0".to_string(),
                })
            }
            _ => Err(HookError::UnsupportedHook {
                hook_type: format!("{:?}", request.command),
            }),
        };

        let processing_time = start.elapsed().as_millis() as u64;

        match result {
            Ok(data) => {
                Ok(AgentResponse::success(request.id, data).with_processing_time(processing_time))
            }
            Err(e) => Ok(AgentResponse::error(request.id, e.to_string())
                .with_processing_time(processing_time)),
        }
    }

    /// Retorna capacidades
    pub fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            can_execute_commands: false,
            can_edit_files: false,
            can_analyze_code: true,
            can_generate_tests: false,
            can_refactor: false,
            supports_streaming: self.config.streaming,
            supports_files: true,
            max_file_size: Some(1024 * 1024), // 1MB
            supported_languages: vec![
                "rust".to_string(),
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
                "go".to_string(),
                "java".to_string(),
                "cpp".to_string(),
                "c".to_string(),
            ],
        }
    }

    /// Cria handler de hook
    pub fn create_handler(&self) -> HookHandler {
        HookHandler::new(|event, _context| {
            info!("OpenCode hook received event: {:?}", event.event_type);
            Ok(())
        })
    }

    /// Obtém uso atual
    pub fn get_usage(&self) -> Option<u64> {
        self.state.tokens_used_this_month
    }

    /// Obtém rate limit restante
    pub fn get_rate_limit(&self) -> Option<u32> {
        self.state.rate_limit_remaining
    }

    /// Obtém timestamp de reset do rate limit.
    pub fn get_rate_limit_reset(&self) -> Option<u64> {
        self.state.rate_limit_reset
    }

    /// Obtém última atividade registrada.
    pub fn last_activity(&self) -> Option<std::time::SystemTime> {
        self.state.last_activity
    }
}

/// Detecta linguagem pelo path
fn detect_language(path: &str) -> String {
    if path.ends_with(".rs") {
        "rust".to_string()
    } else if path.ends_with(".py") {
        "python".to_string()
    } else if path.ends_with(".js") {
        "javascript".to_string()
    } else if path.ends_with(".ts") {
        "typescript".to_string()
    } else if path.ends_with(".go") {
        "go".to_string()
    } else if path.ends_with(".java") {
        "java".to_string()
    } else if path.ends_with(".cpp") || path.ends_with(".cc") {
        "cpp".to_string()
    } else if path.ends_with(".c") {
        "c".to_string()
    } else {
        "text".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opencode_config_default() {
        let config = OpenCodeConfig::default();
        assert_eq!(config.api_endpoint, "https://api.opencode.ai/v1");
        assert_eq!(config.model, "opencode-pro");
        assert!(config.streaming);
    }

    #[test]
    fn test_opencode_hook_creation() {
        let config = OpenCodeConfig {
            api_key: "test-key".to_string(),
            ..Default::default()
        };
        let hook = OpenCodeHook::new(config).unwrap();
        assert_eq!(hook.config.model, "opencode-pro");
    }

    #[test]
    fn test_opencode_hook_requires_api_key() {
        let config = OpenCodeConfig::default();
        let result = OpenCodeHook::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_opencode_hook_rejects_invalid_endpoint() {
        let config = OpenCodeConfig {
            api_key: "test-key".to_string(),
            api_endpoint: "not a url".to_string(),
            ..Default::default()
        };

        let result = OpenCodeHook::new(config);
        assert!(matches!(result, Err(HookError::ConfigError(_))));
    }

    #[test]
    fn test_opencode_hook_rejects_unsafe_endpoint_scheme() {
        let config = OpenCodeConfig {
            api_key: "test-key".to_string(),
            api_endpoint: "file:///etc/passwd".to_string(),
            ..Default::default()
        };

        let result = OpenCodeHook::new(config);
        assert!(matches!(result, Err(HookError::ConfigError(_))));
    }

    #[test]
    fn test_opencode_hook_normalizes_endpoint() {
        let config = OpenCodeConfig {
            api_key: "test-key".to_string(),
            api_endpoint: "https://api.example.test/v1/".to_string(),
            ..Default::default()
        };

        let hook = OpenCodeHook::new(config).unwrap();
        assert_eq!(hook.config.api_endpoint, "https://api.example.test/v1");
    }

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("test.rs"), "rust");
        assert_eq!(detect_language("test.py"), "python");
        assert_eq!(detect_language("test.js"), "javascript");
        assert_eq!(detect_language("test.ts"), "typescript");
        assert_eq!(detect_language("test.go"), "go");
        assert_eq!(detect_language("test.unknown"), "text");
    }

    #[test]
    fn test_capabilities() {
        let config = OpenCodeConfig {
            api_key: "test".to_string(),
            ..Default::default()
        };
        let hook = OpenCodeHook::new(config).unwrap();
        let caps = hook.capabilities();

        assert!(caps.can_analyze_code);
        assert!(!caps.can_execute_commands);
        assert!(caps.supports_streaming);
    }

    #[test]
    fn test_create_registration() {
        let config = OpenCodeConfig {
            api_key: "test".to_string(),
            ..Default::default()
        };
        let hook = OpenCodeHook::new(config).unwrap();
        let reg = hook.create_registration();

        assert_eq!(reg.info.agent_type, AgentType::OpenCode);
        assert_eq!(reg.info.name, "OpenCode");
    }

    // Testes que requerem API key válida são ignorados
    #[tokio::test]
    #[ignore = "Requires valid OpenCode API key"]
    async fn test_validate_api_key() {
        let mut hook = OpenCodeHook::from_env().unwrap();
        let valid = hook.validate_api_key().await.unwrap();
        info!(valid, "OpenCode API key validation completed");
    }

    use crate::ai::test_support::FakeOpenAi;

    fn hook_for(server: &FakeOpenAi, org: Option<&str>) -> OpenCodeHook {
        OpenCodeHook::new(OpenCodeConfig {
            api_key: "chave".into(),
            api_endpoint: format!("{}/", server.base_url),
            organization: org.map(String::from),
            max_tokens: 321,
            timeout_secs: 5,
            ..Default::default()
        })
        .unwrap()
    }

    async fn replying_with_limits(content: &str) -> FakeOpenAi {
        let body = serde_json::json!({
            "id": "r1",
            "choices": [{"message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
        });
        FakeOpenAi::start_with_headers(
            200,
            body.to_string(),
            vec![
                ("x-ratelimit-remaining", "41".to_string()),
                ("x-ratelimit-reset", "1700000000".to_string()),
            ],
        )
        .await
    }

    #[tokio::test]
    async fn test_send_prompt_updates_usage_and_rate_limits() {
        let server = replying_with_limits("resposta").await;
        let mut hook = hook_for(&server, Some("org-1"));
        assert!(hook.last_activity().is_none());
        assert_eq!(hook.send_prompt("olá").await.unwrap(), "resposta");
        assert_eq!(hook.get_usage(), Some(7));
        assert_eq!(hook.get_rate_limit(), Some(41));
        assert_eq!(hook.get_rate_limit_reset(), Some(1_700_000_000));
        assert!(hook.last_activity().is_some());
        // uso acumula
        hook.send_prompt("de novo").await.unwrap();
        assert_eq!(hook.get_usage(), Some(14));

        let body = &server.bodies()[0];
        assert_eq!(body["model"], "opencode-pro");
        assert_eq!(body["max_tokens"], 321);
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["content"], "olá");
        let headers = body["__headers"].as_str().unwrap();
        assert!(headers.contains("authorization: bearer chave"));
        assert!(headers.contains("opencode-organization: org-1"));
    }

    #[tokio::test]
    async fn test_helpers_build_prompts() {
        let server = replying_with_limits("ls\n\n  git status  \npwd\nexit").await;
        let mut hook = hook_for(&server, None);
        let sugg = hook.get_suggestions("contexto", 3).await.unwrap();
        assert_eq!(sugg, vec!["ls", "git status", "pwd"]);
        hook.explain_error("panic").await.unwrap();
        hook.analyze_code("fn main(){}", "rust").await.unwrap();
        let bodies = server.bodies();
        let prompt = |i: usize| {
            bodies[i]["messages"][0]["content"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert!(prompt(0).starts_with("Based on this terminal context, suggest 3 commands"));
        assert!(prompt(1).contains("```\npanic\n```"));
        assert!(prompt(2).contains("```rust\nfn main(){}\n```"));
        assert!(!bodies[0]["__headers"]
            .as_str()
            .unwrap()
            .contains("opencode-organization"));
    }

    #[tokio::test]
    async fn test_api_errors_and_key_validation() {
        let server = FakeOpenAi::start(401, r#"{"error":"nao autorizado"}"#.into()).await;
        let mut hook = hook_for(&server, None);
        assert!(!hook.validate_api_key().await.unwrap());
        assert!(matches!(
            hook.send_prompt("x").await,
            Err(HookError::AgentError { message, .. }) if message.contains("401") && message.contains("nao autorizado")
        ));

        let server = FakeOpenAi::start(500, "falhou".into()).await;
        let mut hook = hook_for(&server, None);
        assert!(matches!(
            hook.validate_api_key().await,
            Err(HookError::AgentError { .. })
        ));

        let server = replying_with_limits("ok").await;
        let mut hook = hook_for(&server, None);
        assert!(hook.validate_api_key().await.unwrap());

        let server = FakeOpenAi::start(200, "nao json".into()).await;
        let mut hook = hook_for(&server, None);
        assert!(matches!(
            hook.send_prompt("x").await,
            Err(HookError::SerializationError(_))
        ));

        let server = FakeOpenAi::start(200, r#"{"id":"v","choices":[]}"#.into()).await;
        let mut hook = hook_for(&server, None);
        assert!(matches!(
            hook.send_prompt("x").await,
            Err(HookError::AgentError { message, .. }) if message == "No response from API"
        ));
        // sem usage: contador continua vazio
        assert_eq!(hook.get_usage(), None);

        // ninguém escutando na porta
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut hook = OpenCodeHook::new(OpenCodeConfig {
            api_key: "k".into(),
            api_endpoint: format!("http://127.0.0.1:{port}/v1"),
            ..Default::default()
        })
        .unwrap();
        assert!(matches!(
            hook.send_prompt("x").await,
            Err(HookError::CommunicationError(_))
        ));
    }

    #[tokio::test]
    async fn test_process_request_variants() {
        use crate::ai::hooks::api::AnalysisType;
        let server = replying_with_limits("a\nb").await;
        let mut hook = hook_for(&server, None);

        let r = hook
            .process_request(AgentRequest::new(AgentCommand::GetSuggestion {
                context: "c".into(),
                max_suggestions: 5,
            }))
            .await
            .unwrap();
        assert!(matches!(r.data, ResponseData::Text { ref text } if text == "a\nb"));

        let r = hook
            .process_request(AgentRequest::new(AgentCommand::ExplainError {
                error_output: "e".into(),
                command: None,
            }))
            .await
            .unwrap();
        assert!(matches!(
            r.data,
            ResponseData::ErrorExplanation {
                fix_available: false,
                ..
            }
        ));

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("x.py");
        std::fs::write(&file, "print(1)").unwrap();
        let r = hook
            .process_request(AgentRequest::new(AgentCommand::AnalyzeCode {
                path: file.to_string_lossy().to_string(),
                analysis_type: AnalysisType::default(),
            }))
            .await
            .unwrap();
        assert!(r.success);
        let last = server.bodies().last().unwrap()["messages"][0]["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(last.contains("```python\nprint(1)\n```"));
        assert!(hook
            .process_request(AgentRequest::new(AgentCommand::AnalyzeCode {
                path: "/nao/existe.rs".into(),
                analysis_type: AnalysisType::default(),
            }))
            .await
            .is_err());

        let r = hook
            .process_request(AgentRequest::new(AgentCommand::Ping))
            .await
            .unwrap();
        assert!(matches!(r.data, ResponseData::Pong { .. }));
        let r = hook
            .process_request(AgentRequest::new(AgentCommand::GetCapabilities))
            .await
            .unwrap();
        assert!(
            matches!(r.data, ResponseData::Capabilities { ref version, .. } if version == "1.0")
        );
        let r = hook
            .process_request(AgentRequest::execute_command("ls"))
            .await
            .unwrap();
        assert!(!r.success);

        hook.create_handler()
            .handle(
                &crate::ai::hooks::event::AgentEvent::new(
                    crate::ai::hooks::event::EventType::TerminalStart,
                ),
                &crate::ai::hooks::context::AgentContext::new("s", 0),
            )
            .unwrap();
        let reg = hook.create_registration();
        assert_eq!(reg.endpoint.as_deref(), Some(server.base_url.as_str()));
    }

    #[test]
    fn test_endpoint_validation_and_languages() {
        assert!(matches!(
            OpenCodeHook::validate_api_endpoint("ftp://host/v1"),
            Err(HookError::ConfigError(m)) if m.contains("http or https")
        ));
        assert_eq!(
            OpenCodeHook::validate_api_endpoint("http://h:8080/v1//").unwrap(),
            "http://h:8080/v1"
        );
        for (p, l) in [
            ("a.java", "java"),
            ("a.cpp", "cpp"),
            ("a.cc", "cpp"),
            ("a.c", "c"),
        ] {
            assert_eq!(detect_language(p), l);
        }
        let _ = OpenCodeHook::detect();
    }
}
