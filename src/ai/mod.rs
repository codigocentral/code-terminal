pub mod config;
pub mod error;
pub mod error_detection;
pub mod error_explanation;
pub mod hooks;
pub mod message;
pub mod openai;
pub mod privacy;
pub mod privacy_manager;
pub mod provider;
pub mod suggestions;
#[cfg(test)]
pub(crate) mod test_support;
pub mod ui_integration;

pub use config::{AiConfig, AiProviderType, PrivacyConfig};
pub use error::{AiError, AiResult};
pub use error_detection::{DetectedError, ErrorDetector, ErrorSeverity, ErrorType};
pub use error_explanation::{DocumentationLink, ErrorExplainer, ErrorExplanation, QuickFix};
pub use message::{
    CompletionResponse, Conversation, Message, MessageRole, StreamChunk, TokenUsage,
};
pub use openai::OpenAIProvider;
pub use privacy::{AuditLogEntry, DataSharingLevel, PrivacyControls, PrivacyError};
pub use privacy_manager::PrivacyManager;
pub use provider::{
    AIProvider, AIProviderFactory, BoxedAiProvider, ChatMessage, ChatRequest, ChatResponse,
    CommandSuggestion, ProviderRegistry, TerminalContext, TokenUsage as ProviderTokenUsage,
};
pub use suggestions::{CommandSuggester, SuggesterStats, SuggestionBuilder, SuggestionHistory};

use crate::ai::openai::OpenAIProviderFactory;

pub fn create_provider(config: &AiConfig) -> AiResult<BoxedAiProvider> {
    let mut registry = ProviderRegistry::new();
    registry.register(OpenAIProviderFactory);
    registry.create(config)
}

pub fn create_suggester(config: AiConfig) -> AiResult<CommandSuggester> {
    CommandSuggester::new(config)
}
