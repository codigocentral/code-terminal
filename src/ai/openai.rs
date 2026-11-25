use async_openai::{
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
        ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
        CreateChatCompletionRequestArgs,
    },
    Client,
};
use async_trait::async_trait;
use tracing::{debug, error, info, warn};

use super::config::AiConfig;
use super::error::{AiError, AiResult};
use super::provider::{
    AIProvider, ChatMessage, ChatRequest, ChatResponse, CommandSuggestion, TerminalContext,
};

pub struct OpenAIProvider {
    client: Client<async_openai::config::OpenAIConfig>,
    config: AiConfig,
    model: String,
}

impl OpenAIProvider {
    pub fn new(config: AiConfig) -> AiResult<Self> {
        let api_key = config
            .get_api_key()
            .ok_or(AiError::ConfigError("API key not provided".to_string()))?
            .to_string();

        let openai_config = if let Some(base_url) = &config.base_url {
            async_openai::config::OpenAIConfig::new()
                .with_api_key(api_key)
                .with_api_base(base_url)
        } else {
            async_openai::config::OpenAIConfig::new().with_api_key(api_key)
        };

        let client = Client::with_config(openai_config);
        let model = config.model.clone();

        info!("OpenAI provider initialized with model: {}", model);

        Ok(Self {
            client,
            config,
            model,
        })
    }

    fn convert_messages(
        &self,
        messages: Vec<ChatMessage>,
    ) -> AiResult<Vec<ChatCompletionRequestMessage>> {
        let build_err = |e: async_openai::error::OpenAIError| {
            AiError::ApiError(format!("Failed to build message: {}", e))
        };
        messages
            .into_iter()
            .map(|msg| {
                Ok(match msg.role {
                    super::provider::MessageRole::System => ChatCompletionRequestMessage::System(
                        ChatCompletionRequestSystemMessageArgs::default()
                            .content(msg.content)
                            .build()
                            .map_err(build_err)?,
                    ),
                    super::provider::MessageRole::User => ChatCompletionRequestMessage::User(
                        ChatCompletionRequestUserMessageArgs::default()
                            .content(msg.content)
                            .build()
                            .map_err(build_err)?,
                    ),
                    // Antes as mensagens do assistente iam com papel "user"
                    super::provider::MessageRole::Assistant => {
                        ChatCompletionRequestMessage::Assistant(
                            ChatCompletionRequestAssistantMessageArgs::default()
                                .content(msg.content)
                                .build()
                                .map_err(build_err)?,
                        )
                    }
                })
            })
            .collect()
    }

    fn build_system_prompt_for_suggestions() -> String {
        r#"You are a terminal command assistant. Your role is to suggest appropriate shell commands based on the user's context and input.

Rules:
1. Provide only the exact command to run, no explanations in the command field
2. Provide a brief, helpful explanation in the explanation field
3. If you're uncertain, suggest the most common/safe option
4. Consider the user's shell type (bash, zsh, fish, powershell, etc.)
5. Take into account the current directory and recent commands
6. Suggest alternatives when appropriate
7. Always prioritize safety - warn about destructive operations
8. Format your response as JSON with: command, explanation, alternatives (optional), confidence (0-1)

Example response format:
{
  "command": "ls -la",
  "explanation": "List all files with detailed information",
  "alternatives": ["ls -lh", "dir"],
  "confidence": 0.95
}"#
            .to_string()
    }

    fn build_system_prompt_for_errors() -> String {
        r#"You are a terminal error diagnosis assistant. Your role is to explain error messages and suggest solutions.

Rules:
1. Explain what the error means in simple terms
2. Identify the most likely cause
3. Suggest concrete steps to fix it
4. Consider the context (command that failed, directory, etc.)
5. Be concise but thorough
6. Include relevant documentation links when helpful"#
            .to_string()
    }

    fn redact_sensitive_info(&self, text: &str) -> String {
        let mut result = text.to_string();

        for pattern in &self.config.privacy.redact_patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                result = re.replace_all(&result, "[REDACTED]").to_string();
            }
        }

        result
    }

    fn format_context(&self, context: &TerminalContext) -> String {
        let mut parts = Vec::new();

        if let Some(dir) = &context.current_directory {
            if self.config.privacy.send_file_paths {
                parts.push(format!("Current directory: {}", dir));
            } else {
                parts.push("Current directory: [REDACTED]".to_string());
            }
        }

        if let Some(shell) = &context.shell_type {
            parts.push(format!("Shell: {}", shell));
        }

        if !context.recent_commands.is_empty() && self.config.privacy.send_shell_history {
            parts.push("Recent commands:".to_string());
            for cmd in context.recent_commands.iter().take(5) {
                let redacted = self.redact_sensitive_info(cmd);
                parts.push(format!("  - {}", redacted));
            }
        }

        if let Some(output) = &context.last_output {
            let redacted = self.redact_sensitive_info(output);
            parts.push(format!("Last output:\n{}", redacted));
        }

        parts.join("\n")
    }
}

#[async_trait]
impl AIProvider for OpenAIProvider {
    async fn chat(&self, request: ChatRequest) -> AiResult<ChatResponse> {
        if !self.config.enabled {
            return Err(AiError::ConfigError("AI is disabled".to_string()));
        }

        debug!(
            "Sending chat request with {} messages",
            request.messages.len()
        );

        let messages = self.convert_messages(request.messages)?;

        let max_tokens = request.max_tokens.unwrap_or(self.config.max_tokens);
        let temperature = request.temperature.unwrap_or(self.config.temperature);

        let req = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(messages)
            .max_tokens(max_tokens)
            .temperature(temperature)
            .build()
            .map_err(|e| AiError::ApiError(format!("Failed to build request: {}", e)))?;

        let response = self.client.chat().create(req).await.map_err(|e| {
            error!("OpenAI API error: {}", e);
            AiError::ApiError(format!("OpenAI API error: {}", e))
        })?;

        let choice = response
            .choices
            .first()
            .ok_or_else(|| AiError::InvalidResponse("No choices in response".to_string()))?;

        let content = choice.message.content.clone().unwrap_or_default();

        if let Some(ref usage) = response.usage {
            info!("Chat completion received: {} tokens", usage.total_tokens);
        }

        Ok(ChatResponse {
            content,
            model: response.model,
            usage: response.usage.map(|u| super::provider::TokenUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }),
            finish_reason: choice.finish_reason.as_ref().map(|r| format!("{:?}", r)),
        })
    }

    async fn suggest_command(
        &self,
        context: &TerminalContext,
        user_input: &str,
    ) -> AiResult<CommandSuggestion> {
        if !self.config.enabled {
            return Err(AiError::ConfigError("AI is disabled".to_string()));
        }

        if self.config.privacy.require_confirmation {
            warn!("Command suggestion requested - privacy confirmation may be needed");
        }

        debug!("Suggesting command for input: {}", user_input);

        let system_prompt = Self::build_system_prompt_for_suggestions();
        let context_info = self.format_context(context);

        let user_message = format!(
            "Context:\n{}\n\nUser input: {}\n\nSuggest the best command to run.",
            context_info, user_input
        );

        let messages = vec![
            ChatMessage {
                role: super::provider::MessageRole::System,
                content: system_prompt,
            },
            ChatMessage {
                role: super::provider::MessageRole::User,
                content: user_message,
            },
        ];

        let response = self
            .chat(ChatRequest {
                messages,
                max_tokens: Some(500),
                temperature: Some(0.7),
                model: None,
            })
            .await?;

        let suggestion: CommandSuggestion =
            serde_json::from_str(&response.content).unwrap_or_else(|_| CommandSuggestion {
                command: response.content.clone(),
                explanation: "AI-generated suggestion".to_string(),
                alternatives: None,
                confidence: 0.5,
            });

        info!(
            "Command suggestion: {} (confidence: {:.2})",
            suggestion.command, suggestion.confidence
        );

        Ok(suggestion)
    }

    async fn explain_error(
        &self,
        error_message: &str,
        context: &TerminalContext,
    ) -> AiResult<String> {
        if !self.config.enabled {
            return Err(AiError::ConfigError("AI is disabled".to_string()));
        }

        debug!("Explaining error: {}", error_message);

        let system_prompt = Self::build_system_prompt_for_errors();
        let context_info = self.format_context(context);

        let redacted_error = self.redact_sensitive_info(error_message);

        let user_message = format!(
            "Context:\n{}\n\nError message:\n{}\n\nExplain this error and suggest how to fix it.",
            context_info, redacted_error
        );

        let messages = vec![
            ChatMessage {
                role: super::provider::MessageRole::System,
                content: system_prompt,
            },
            ChatMessage {
                role: super::provider::MessageRole::User,
                content: user_message,
            },
        ];

        let response = self
            .chat(ChatRequest {
                messages,
                max_tokens: Some(800),
                temperature: Some(0.7),
                model: None,
            })
            .await?;

        info!("Error explanation generated");

        Ok(response.content)
    }

    fn name(&self) -> &str {
        "OpenAI"
    }

    fn is_configured(&self) -> bool {
        self.config.enabled && self.config.api_key.is_some()
    }
}

pub struct OpenAIProviderFactory;

impl super::provider::AIProviderFactory for OpenAIProviderFactory {
    fn create(&self, config: &AiConfig) -> AiResult<Box<dyn AIProvider>> {
        let provider = OpenAIProvider::new(config.clone())?;
        Ok(Box::new(provider))
    }

    fn provider_type(&self) -> super::config::AiProviderType {
        super::config::AiProviderType::OpenAI
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_creation() {
        let config = AiConfig::openai("test-api-key");
        let provider = OpenAIProvider::new(config);
        assert!(provider.is_ok());
    }

    #[test]
    fn test_openai_provider_requires_api_key() {
        let config = AiConfig::default();
        let provider = OpenAIProvider::new(config);
        assert!(provider.is_err());
    }

    #[test]
    fn test_is_configured() {
        let config = AiConfig::openai("test-api-key");
        let provider = OpenAIProvider::new(config).unwrap();
        assert!(provider.is_configured());
    }

    #[test]
    fn test_redact_sensitive_info() {
        let config = AiConfig::openai("test-api-key");
        let provider = OpenAIProvider::new(config).unwrap();

        let text = "My password=secret123 and api_key=sk-1234567890";
        let redacted = provider.redact_sensitive_info(text);

        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_format_context() {
        let config = AiConfig::openai("test-api-key");
        let provider = OpenAIProvider::new(config).unwrap();

        let context = TerminalContext {
            current_directory: Some("/home/user".to_string()),
            shell_type: Some("bash".to_string()),
            recent_commands: vec!["ls -la".to_string()],
            ..Default::default()
        };

        let formatted = provider.format_context(&context);
        assert!(formatted.contains("bash"));
    }

    use crate::ai::provider::MessageRole;
    use crate::ai::test_support::FakeOpenAi;

    fn provider_for(server: &FakeOpenAi) -> OpenAIProvider {
        OpenAIProvider::new(AiConfig::openai("chave-teste").with_base_url(&server.base_url))
            .unwrap()
    }

    fn msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: content.to_string(),
        }
    }

    #[tokio::test]
    async fn test_chat_sends_roles_and_parses_response() {
        let server = FakeOpenAi::replying("olá").await;
        let p = provider_for(&server);
        let resp = p
            .chat(ChatRequest {
                messages: vec![
                    msg(MessageRole::System, "sys"),
                    msg(MessageRole::User, "pergunta"),
                    msg(MessageRole::Assistant, "resposta anterior"),
                ],
                max_tokens: Some(42),
                temperature: Some(0.1),
                model: None,
            })
            .await
            .unwrap();
        assert_eq!(resp.content, "olá");
        assert_eq!(resp.model, "modelo-falso");
        let usage = resp.usage.unwrap();
        assert_eq!(
            (
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens
            ),
            (7, 5, 12)
        );
        assert_eq!(resp.finish_reason.as_deref(), Some("Stop"));

        let body = &server.bodies()[0];
        assert_eq!(body["model"], p.model.as_str());
        assert_eq!(body["max_tokens"], 42);
        let roles: Vec<&str> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        // Regressão: a mensagem do assistente era enviada como "user"
        assert_eq!(roles, vec!["system", "user", "assistant"]);
        assert_eq!(body["messages"][2]["content"], "resposta anterior");
    }

    #[tokio::test]
    async fn test_chat_uses_config_defaults_and_handles_no_choices() {
        let body = serde_json::json!({
            "id": "x", "object": "chat.completion", "created": 1,
            "model": "m", "choices": []
        });
        let server = FakeOpenAi::start(200, body.to_string()).await;
        let mut config = AiConfig::openai("k").with_base_url(&server.base_url);
        config.max_tokens = 77;
        let p = OpenAIProvider::new(config).unwrap();
        let err = p
            .chat(ChatRequest {
                messages: vec![msg(MessageRole::User, "oi")],
                max_tokens: None,
                temperature: None,
                model: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AiError::InvalidResponse(m) if m.contains("No choices")));
        assert_eq!(server.bodies()[0]["max_tokens"], 77);
    }

    #[tokio::test]
    async fn test_api_error_is_reported() {
        let body = serde_json::json!({"error": {
            "message": "chave inválida", "type": "invalid_request_error",
            "param": null, "code": "invalid_api_key"}});
        let server = FakeOpenAi::start(401, body.to_string()).await;
        let p = provider_for(&server);
        let err = p
            .explain_error("boom", &TerminalContext::default())
            .await
            .unwrap_err();
        assert!(matches!(err, AiError::ApiError(m) if m.contains("chave inválida")));
    }

    #[tokio::test]
    async fn test_suggest_command_parses_json_or_falls_back() {
        let json =
            r#"{"command":"ls -la","explanation":"lista","alternatives":["ls"],"confidence":0.8}"#;
        let server = FakeOpenAi::replying(json).await;
        let p = provider_for(&server);
        let ctx = TerminalContext {
            current_directory: Some("/srv".into()),
            shell_type: Some("zsh".into()),
            ..Default::default()
        };
        let s = p.suggest_command(&ctx, "listar arquivos").await.unwrap();
        assert_eq!(s.command, "ls -la");
        assert_eq!(s.alternatives, Some(vec!["ls".to_string()]));
        assert!((s.confidence - 0.8).abs() < f32::EPSILON);
        let body = &server.bodies()[0];
        assert_eq!(body["max_tokens"], 500);
        let user = body["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("User input: listar arquivos"));
        assert!(user.contains("Shell: zsh"));
        // caminhos não são enviados por padrão
        assert!(user.contains("Current directory: [REDACTED]"));

        let server = FakeOpenAi::replying("git status").await;
        let s = provider_for(&server)
            .suggest_command(&TerminalContext::default(), "x")
            .await
            .unwrap();
        assert_eq!(s.command, "git status");
        assert_eq!(s.explanation, "AI-generated suggestion");
        assert!((s.confidence - 0.5).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn test_explain_error_redacts_message() {
        let server = FakeOpenAi::replying("explicação").await;
        let p = provider_for(&server);
        let out = p
            .explain_error("falhou com password=hunter2", &TerminalContext::default())
            .await
            .unwrap();
        assert_eq!(out, "explicação");
        let body = &server.bodies()[0];
        assert_eq!(body["max_tokens"], 800);
        let user = body["messages"][1]["content"].as_str().unwrap();
        assert!(!user.contains("hunter2"));
        assert!(user.contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn test_disabled_provider_refuses_every_call() {
        let mut config = AiConfig::openai("k").with_base_url("http://127.0.0.1:9/v1");
        config.enabled = false;
        let p = OpenAIProvider::new(config).unwrap();
        assert!(!p.is_configured());
        assert_eq!(p.name(), "OpenAI");
        let ctx = TerminalContext::default();
        assert!(matches!(
            p.suggest_command(&ctx, "x").await,
            Err(AiError::ConfigError(_))
        ));
        assert!(matches!(
            p.explain_error("x", &ctx).await,
            Err(AiError::ConfigError(_))
        ));
        assert!(matches!(
            p.chat(ChatRequest {
                messages: vec![],
                max_tokens: None,
                temperature: None,
                model: None
            })
            .await,
            Err(AiError::ConfigError(_))
        ));
    }

    #[test]
    fn test_format_context_respects_privacy_flags() {
        let mut config = AiConfig::openai("k");
        config.privacy.send_file_paths = true;
        config.privacy.send_shell_history = true;
        let p = OpenAIProvider::new(config).unwrap();
        let ctx = TerminalContext {
            current_directory: Some("/home/u".into()),
            recent_commands: (0..8).map(|i| format!("cmd{i} password=x{i}")).collect(),
            last_output: Some("token=abc".into()),
            ..Default::default()
        };
        let text = p.format_context(&ctx);
        assert!(text.contains("Current directory: /home/u"));
        assert!(text.contains("Recent commands:"));
        // só os 5 primeiros comandos, com segredos mascarados
        assert!(text.contains("cmd4") && !text.contains("cmd5"));
        assert!(!text.contains("password=x0"));
        assert!(text.contains("Last output:"));

        // sem histórico quando o envio está desligado
        let p = OpenAIProvider::new(AiConfig::openai("k")).unwrap();
        assert!(!p.format_context(&ctx).contains("Recent commands"));
    }

    #[test]
    fn test_factory_creates_openai_provider() {
        use crate::ai::provider::AIProviderFactory;
        let f = OpenAIProviderFactory;
        assert_eq!(f.provider_type(), crate::ai::config::AiProviderType::OpenAI);
        let p = f.create(&AiConfig::openai("k")).unwrap();
        assert_eq!(p.name(), "OpenAI");
        assert!(f.create(&AiConfig::default()).is_err());
    }
}
