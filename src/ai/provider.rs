use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::config::{AiConfig, AiProviderType};
use super::error::{AiError, AiResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<TokenUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSuggestion {
    pub command: String,
    pub explanation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alternatives: Option<Vec<String>>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TerminalContext {
    pub current_directory: Option<String>,
    pub recent_commands: Vec<String>,
    pub last_output: Option<String>,
    pub shell_type: Option<String>,
    pub environment: HashMap<String, String>,
}

#[async_trait]
pub trait AIProvider: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> AiResult<ChatResponse>;

    async fn suggest_command(
        &self,
        context: &TerminalContext,
        user_input: &str,
    ) -> AiResult<CommandSuggestion>;

    async fn explain_error(
        &self,
        error_message: &str,
        context: &TerminalContext,
    ) -> AiResult<String>;

    fn name(&self) -> &str;

    fn is_configured(&self) -> bool;
}

pub type BoxedAiProvider = Box<dyn AIProvider>;

pub trait AIProviderFactory: Send + Sync {
    fn create(&self, config: &AiConfig) -> AiResult<BoxedAiProvider>;

    fn provider_type(&self) -> AiProviderType;
}

pub struct ProviderRegistry {
    factories: HashMap<AiProviderType, Box<dyn AIProviderFactory>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    pub fn register<F: AIProviderFactory + 'static>(&mut self, factory: F) {
        self.factories
            .insert(factory.provider_type(), Box::new(factory));
    }

    pub fn create(&self, config: &AiConfig) -> AiResult<BoxedAiProvider> {
        let factory = self
            .factories
            .get(&config.provider)
            .ok_or_else(|| AiError::ProviderNotAvailable(format!("{:?}", config.provider)))?;
        factory.create(config)
    }

    pub fn available_providers(&self) -> Vec<AiProviderType> {
        self.factories.keys().cloned().collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_directory(mut self, dir: impl Into<String>) -> Self {
        self.current_directory = Some(dir.into());
        self
    }

    pub fn with_shell(mut self, shell: impl Into<String>) -> Self {
        self.shell_type = Some(shell.into());
        self
    }

    pub fn with_recent_commands(mut self, commands: Vec<String>) -> Self {
        self.recent_commands = commands;
        self
    }

    pub fn with_last_output(mut self, output: impl Into<String>) -> Self {
        self.last_output = Some(output.into());
        self
    }

    pub fn add_command(&mut self, command: impl Into<String>) {
        self.recent_commands.push(command.into());
        if self.recent_commands.len() > 50 {
            self.recent_commands.remove(0);
        }
    }
}

impl CommandSuggestion {
    pub fn new(command: impl Into<String>, explanation: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            explanation: explanation.into(),
            alternatives: None,
            confidence: 0.8,
        }
    }

    pub fn with_alternatives(mut self, alternatives: Vec<String>) -> Self {
        self.alternatives = Some(alternatives);
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_context_default() {
        let ctx = TerminalContext::default();
        assert!(ctx.current_directory.is_none());
        assert!(ctx.recent_commands.is_empty());
        assert!(ctx.last_output.is_none());
        assert!(ctx.shell_type.is_none());
        assert!(ctx.environment.is_empty());
    }

    #[test]
    fn test_terminal_context_builder() {
        let ctx = TerminalContext::new()
            .with_directory("/home/user")
            .with_shell("bash")
            .with_last_output("command output")
            .with_recent_commands(vec!["ls".to_string(), "cd".to_string()]);

        assert_eq!(ctx.current_directory, Some("/home/user".to_string()));
        assert_eq!(ctx.shell_type, Some("bash".to_string()));
        assert_eq!(ctx.last_output, Some("command output".to_string()));
        assert_eq!(ctx.recent_commands.len(), 2);
    }

    #[test]
    fn test_terminal_context_add_command() {
        let mut ctx = TerminalContext::new();
        ctx.add_command("ls");
        ctx.add_command("cd");
        assert_eq!(ctx.recent_commands.len(), 2);
        assert_eq!(ctx.recent_commands[0], "ls");
    }

    #[test]
    fn test_terminal_context_command_limit() {
        let mut ctx = TerminalContext::new();
        for i in 0..60 {
            ctx.add_command(format!("cmd{}", i));
        }
        assert_eq!(ctx.recent_commands.len(), 50);
    }

    #[test]
    fn test_command_suggestion_new() {
        let suggestion = CommandSuggestion::new("ls -la", "List all files");
        assert_eq!(suggestion.command, "ls -la");
        assert_eq!(suggestion.explanation, "List all files");
        assert!(suggestion.alternatives.is_none());
        assert_eq!(suggestion.confidence, 0.8);
    }

    #[test]
    fn test_command_suggestion_builder() {
        let suggestion = CommandSuggestion::new("git status", "Check git status")
            .with_alternatives(vec!["git diff".to_string()])
            .with_confidence(0.95);

        assert!(suggestion.alternatives.is_some());
        assert_eq!(suggestion.confidence, 0.95);
    }

    #[test]
    fn test_command_suggestion_confidence_clamp() {
        let suggestion = CommandSuggestion::new("cmd", "test").with_confidence(1.5);
        assert_eq!(suggestion.confidence, 1.0);

        let suggestion = CommandSuggestion::new("cmd", "test").with_confidence(-0.5);
        assert_eq!(suggestion.confidence, 0.0);
    }

    #[test]
    fn test_command_suggestion_serialization() {
        let suggestion = CommandSuggestion::new("ls -la", "List files").with_confidence(0.95);
        let json = serde_json::to_string(&suggestion).unwrap();
        assert!(json.contains("ls -la"));
        assert!(json.contains("0.95"));
    }

    #[test]
    fn test_chat_request_serialization() {
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: "Hello".to_string(),
            }],
            max_tokens: Some(100),
            temperature: Some(0.7),
            model: Some("gpt-4".to_string()),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("Hello"));
        assert!(json.contains("100"));
        assert!(json.contains("0.7"));
    }

    #[test]
    fn test_chat_request_skip_none() {
        let request = ChatRequest {
            messages: vec![],
            max_tokens: None,
            temperature: None,
            model: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("model"));
    }

    #[test]
    fn test_message_role_serialization() {
        let role = MessageRole::User;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"user\"");

        let role = MessageRole::System;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"system\"");

        let role = MessageRole::Assistant;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"assistant\"");
    }

    #[test]
    fn test_message_role_deserialization() {
        let role: MessageRole = serde_json::from_str("\"user\"").unwrap();
        assert_eq!(role, MessageRole::User);

        let role: MessageRole = serde_json::from_str("\"system\"").unwrap();
        assert_eq!(role, MessageRole::System);
    }

    #[test]
    fn test_message_role_equality() {
        assert_eq!(MessageRole::User, MessageRole::User);
        assert_ne!(MessageRole::User, MessageRole::Assistant);
    }

    #[test]
    fn test_token_usage() {
        let usage = TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_token_usage_serialization() {
        let usage = TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains("\"prompt_tokens\":10"));
        assert!(json.contains("\"completion_tokens\":20"));
        assert!(json.contains("\"total_tokens\":30"));
    }

    #[test]
    fn test_chat_response() {
        let response = ChatResponse {
            content: "Hello, how can I help?".to_string(),
            model: "gpt-4".to_string(),
            usage: Some(TokenUsage {
                prompt_tokens: 5,
                completion_tokens: 10,
                total_tokens: 15,
            }),
            finish_reason: Some("stop".to_string()),
        };
        assert_eq!(response.content, "Hello, how can I help?");
        assert!(response.usage.is_some());
        assert_eq!(response.finish_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_chat_response_serialization() {
        let response = ChatResponse {
            content: "test".to_string(),
            model: "gpt-4".to_string(),
            usage: None,
            finish_reason: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("gpt-4"));
    }

    #[test]
    fn test_chat_message() {
        let msg = ChatMessage {
            role: MessageRole::User,
            content: "What is Rust?".to_string(),
        };
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "What is Rust?");
    }

    #[test]
    fn test_provider_registry_new() {
        let registry = ProviderRegistry::new();
        assert!(registry.available_providers().is_empty());
    }

    #[test]
    fn test_provider_registry_default() {
        let registry = ProviderRegistry::default();
        assert!(registry.available_providers().is_empty());
    }

    #[test]
    fn test_provider_registry_missing_provider() {
        let registry = ProviderRegistry::new();
        let config = AiConfig::default();
        let result = registry.create(&config);
        assert!(result.is_err());
        if let Err(AiError::ProviderNotAvailable(msg)) = result {
            assert!(msg.contains("OpenAI"));
        } else {
            panic!("Expected ProviderNotAvailable error");
        }
    }
}
