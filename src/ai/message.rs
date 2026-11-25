use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MessageRole {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "user")]
    #[default]
    User,
    #[serde(rename = "assistant")]
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Conversation {
    pub messages: Vec<Message>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

impl Conversation {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            max_tokens: None,
            temperature: None,
        }
    }

    pub fn with_system_message(mut self, content: impl Into<String>) -> Self {
        self.messages.push(Message::system(content));
        self
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub fn add_user_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message::user(content));
    }

    pub fn add_assistant_message(&mut self, content: impl Into<String>) {
        self.messages.push(Message::assistant(content));
    }

    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    pub fn token_count_estimate(&self) -> usize {
        self.messages
            .iter()
            .map(|m| m.content.split_whitespace().count())
            .sum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<TokenUsage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub delta: String,
    pub done: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_user_creation() {
        let msg = Message::user("Hello");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_message_system_creation() {
        let msg = Message::system("You are helpful");
        assert_eq!(msg.role, MessageRole::System);
        assert_eq!(msg.content, "You are helpful");
    }

    #[test]
    fn test_message_assistant_creation() {
        let msg = Message::assistant("Hi there!");
        assert_eq!(msg.role, MessageRole::Assistant);
        assert_eq!(msg.content, "Hi there!");
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message::user("Test message");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("user"));
        assert!(json.contains("Test message"));
    }

    #[test]
    fn test_message_deserialization() {
        let json = r#"{"role":"user","content":"Hello"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content, "Hello");
    }

    #[test]
    fn test_message_role_default() {
        let role = MessageRole::default();
        assert_eq!(role, MessageRole::User);
    }

    #[test]
    fn test_message_role_equality() {
        assert_eq!(MessageRole::User, MessageRole::User);
        assert_ne!(MessageRole::User, MessageRole::Assistant);
        assert_ne!(MessageRole::System, MessageRole::User);
    }

    #[test]
    fn test_message_role_serialization() {
        let json = serde_json::to_string(&MessageRole::System).unwrap();
        assert_eq!(json, "\"system\"");

        let json = serde_json::to_string(&MessageRole::User).unwrap();
        assert_eq!(json, "\"user\"");

        let json = serde_json::to_string(&MessageRole::Assistant).unwrap();
        assert_eq!(json, "\"assistant\"");
    }

    #[test]
    fn test_message_role_deserialization() {
        let role: MessageRole = serde_json::from_str("\"system\"").unwrap();
        assert_eq!(role, MessageRole::System);

        let role: MessageRole = serde_json::from_str("\"assistant\"").unwrap();
        assert_eq!(role, MessageRole::Assistant);
    }

    #[test]
    fn test_conversation_new() {
        let conv = Conversation::new();
        assert!(conv.messages.is_empty());
        assert!(conv.max_tokens.is_none());
        assert!(conv.temperature.is_none());
    }

    #[test]
    fn test_conversation_default() {
        let conv = Conversation::default();
        assert!(conv.messages.is_empty());
    }

    #[test]
    fn test_conversation_with_system_message() {
        let conv = Conversation::new().with_system_message("You are a helpful assistant.");
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, MessageRole::System);
    }

    #[test]
    fn test_conversation_add_message() {
        let mut conv = Conversation::new();
        conv.add_message(Message::user("Hello"));
        conv.add_message(Message::assistant("Hi!"));
        assert_eq!(conv.messages.len(), 2);
    }

    #[test]
    fn test_conversation_add_user_message() {
        let mut conv = Conversation::new();
        conv.add_user_message("What is Rust?");
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, MessageRole::User);
        assert_eq!(conv.messages[0].content, "What is Rust?");
    }

    #[test]
    fn test_conversation_add_assistant_message() {
        let mut conv = Conversation::new();
        conv.add_assistant_message("Rust is a systems language");
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].role, MessageRole::Assistant);
    }

    #[test]
    fn test_conversation_with_max_tokens() {
        let conv = Conversation::new().with_max_tokens(2048);
        assert_eq!(conv.max_tokens, Some(2048));
    }

    #[test]
    fn test_conversation_with_temperature() {
        let conv = Conversation::new().with_temperature(0.5);
        assert_eq!(conv.temperature, Some(0.5));
    }

    #[test]
    fn test_conversation_clear() {
        let mut conv = Conversation::new().with_system_message("System");
        conv.add_user_message("Hello");
        assert_eq!(conv.messages.len(), 2);

        conv.clear();
        assert!(conv.messages.is_empty());
    }

    #[test]
    fn test_conversation_token_count_estimate() {
        let conv = Conversation::new()
            .with_system_message("You are helpful.")
            .with_max_tokens(1000);

        let estimate = conv.token_count_estimate();
        assert!(estimate > 0);
    }

    #[test]
    fn test_conversation_token_count_empty() {
        let conv = Conversation::new();
        assert_eq!(conv.token_count_estimate(), 0);
    }

    #[test]
    fn test_conversation_token_count_multiple_messages() {
        let mut conv = Conversation::new().with_system_message("System prompt here");
        conv.add_user_message("One two three");
        conv.add_assistant_message("Four five six seven");

        let estimate = conv.token_count_estimate();
        assert!(estimate >= 7);
    }

    #[test]
    fn test_conversation_builder_chain() {
        let mut conv = Conversation::new()
            .with_system_message("You are helpful")
            .with_max_tokens(1000)
            .with_temperature(0.7);

        conv.add_user_message("Hello");
        conv.add_assistant_message("Hi!");

        assert_eq!(conv.messages.len(), 3);
        assert_eq!(conv.max_tokens, Some(1000));
        assert_eq!(conv.temperature, Some(0.7));
    }

    #[test]
    fn test_completion_response() {
        let response = CompletionResponse {
            content: "Hello".to_string(),
            model: "gpt-4".to_string(),
            usage: Some(TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
            }),
            finish_reason: Some("stop".to_string()),
        };
        assert_eq!(response.content, "Hello");
        assert_eq!(response.model, "gpt-4");
        assert!(response.usage.is_some());
    }

    #[test]
    fn test_completion_response_serialization() {
        let response = CompletionResponse {
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
    fn test_token_usage() {
        let usage = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        assert_eq!(usage.prompt_tokens, 100);
        assert_eq!(usage.completion_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
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
        assert!(json.contains("\"total_tokens\":30"));
    }

    #[test]
    fn test_stream_chunk() {
        let chunk = StreamChunk {
            delta: "Hello".to_string(),
            done: false,
        };
        assert_eq!(chunk.delta, "Hello");
        assert!(!chunk.done);
    }

    #[test]
    fn test_stream_chunk_done() {
        let chunk = StreamChunk {
            delta: "".to_string(),
            done: true,
        };
        assert!(chunk.done);
    }

    #[test]
    fn test_stream_chunk_serialization() {
        let chunk = StreamChunk {
            delta: "test".to_string(),
            done: false,
        };
        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("false"));
    }

    #[test]
    fn test_stream_chunk_deserialization() {
        let json = r#"{"delta":"Hello","done":true}"#;
        let chunk: StreamChunk = serde_json::from_str(json).unwrap();
        assert_eq!(chunk.delta, "Hello");
        assert!(chunk.done);
    }
}
