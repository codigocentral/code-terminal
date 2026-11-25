use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AIProvider {
    #[default]
    OpenAI,
    Anthropic,
    Local,
    Ollama,
    Custom,
}

impl std::fmt::Display for AIProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAI => write!(f, "openai"),
            Self::Anthropic => write!(f, "anthropic"),
            Self::Local => write!(f, "local"),
            Self::Ollama => write!(f, "ollama"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AIConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub provider: AIProvider,

    #[serde(default)]
    pub model: AIModelConfig,

    #[serde(default)]
    pub features: AIFeaturesConfig,

    #[serde(default)]
    pub privacy: AIPrivacyConfig,

    #[serde(default)]
    pub advanced: AIAdvancedConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIModelConfig {
    #[serde(default = "default_model_name")]
    pub name: String,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,

    #[serde(default = "default_top_p")]
    pub top_p: f32,

    #[serde(default)]
    pub api_key_env: String,

    #[serde(default)]
    pub base_url: Option<String>,

    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_model_name() -> String {
    "gpt-4o-mini".to_string()
}

fn default_temperature() -> f32 {
    0.7
}

fn default_max_tokens() -> usize {
    1024
}

fn default_top_p() -> f32 {
    1.0
}

impl Default for AIModelConfig {
    fn default() -> Self {
        Self {
            name: default_model_name(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            top_p: default_top_p(),
            api_key_env: String::new(),
            base_url: None,
            headers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIFeaturesConfig {
    #[serde(default = "default_true")]
    pub command_suggestions: bool,

    #[serde(default = "default_true")]
    pub error_explanation: bool,

    #[serde(default = "default_true")]
    pub command_completion: bool,

    #[serde(default)]
    pub auto_suggestions: bool,

    #[serde(default = "default_suggestion_delay_ms")]
    pub suggestion_delay_ms: u64,

    #[serde(default = "default_max_history_lines")]
    pub max_history_lines: usize,

    #[serde(default)]
    pub context_aware: bool,
}

fn default_suggestion_delay_ms() -> u64 {
    500
}

fn default_max_history_lines() -> usize {
    100
}

impl Default for AIFeaturesConfig {
    fn default() -> Self {
        Self {
            command_suggestions: true,
            error_explanation: true,
            command_completion: true,
            auto_suggestions: false,
            suggestion_delay_ms: default_suggestion_delay_ms(),
            max_history_lines: default_max_history_lines(),
            context_aware: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIPrivacyConfig {
    #[serde(default = "default_true")]
    pub anonymize_paths: bool,

    #[serde(default = "default_true")]
    pub anonymize_usernames: bool,

    #[serde(default)]
    pub exclude_patterns: Vec<String>,

    #[serde(default)]
    pub exclude_env_vars: Vec<String>,

    #[serde(default)]
    pub require_confirmation: bool,

    #[serde(default)]
    pub log_requests: bool,

    #[serde(default)]
    pub redact_secrets: bool,
}

impl Default for AIPrivacyConfig {
    fn default() -> Self {
        Self {
            anonymize_paths: true,
            anonymize_usernames: true,
            exclude_patterns: vec![
                "*.key".to_string(),
                "*.pem".to_string(),
                "*.p12".to_string(),
                "*.pfx".to_string(),
                "*id_rsa*".to_string(),
                "*.env".to_string(),
                ".env.*".to_string(),
            ],
            exclude_env_vars: vec![
                "API_KEY".to_string(),
                "SECRET".to_string(),
                "PASSWORD".to_string(),
                "TOKEN".to_string(),
                "CREDENTIAL".to_string(),
                "PRIVATE".to_string(),
            ],
            require_confirmation: false,
            log_requests: false,
            redact_secrets: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIAdvancedConfig {
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: usize,

    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,

    #[serde(default)]
    pub cache_enabled: bool,

    #[serde(default = "default_cache_ttl_secs")]
    pub cache_ttl_secs: u64,

    #[serde(default)]
    pub proxy_url: Option<String>,
}

fn default_timeout_secs() -> u64 {
    30
}

fn default_retry_attempts() -> usize {
    3
}

fn default_retry_delay_ms() -> u64 {
    1000
}

fn default_cache_ttl_secs() -> u64 {
    3600
}

impl Default for AIAdvancedConfig {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout_secs(),
            retry_attempts: default_retry_attempts(),
            retry_delay_ms: default_retry_delay_ms(),
            cache_enabled: true,
            cache_ttl_secs: default_cache_ttl_secs(),
            proxy_url: None,
        }
    }
}

impl AIConfig {
    pub fn get_api_key(&self) -> Option<String> {
        if self.model.api_key_env.is_empty() {
            std::env::var("OPENAI_API_KEY").ok()
        } else {
            std::env::var(&self.model.api_key_env).ok()
        }
    }

    pub fn get_base_url(&self) -> String {
        self.model
            .base_url
            .clone()
            .unwrap_or_else(|| match self.provider {
                AIProvider::OpenAI => "https://api.openai.com/v1".to_string(),
                AIProvider::Anthropic => "https://api.anthropic.com/v1".to_string(),
                AIProvider::Ollama => "http://localhost:11434/api".to_string(),
                AIProvider::Local | AIProvider::Custom => "http://localhost:8080/v1".to_string(),
            })
    }

    pub fn is_feature_enabled(&self, feature: &str) -> bool {
        if !self.enabled {
            return false;
        }
        match feature {
            "command_suggestions" => self.features.command_suggestions,
            "error_explanation" => self.features.error_explanation,
            "command_completion" => self.features.command_completion,
            "auto_suggestions" => self.features.auto_suggestions,
            "context_aware" => self.features.context_aware,
            _ => false,
        }
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_config_default() {
        let config = AIConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.provider, AIProvider::OpenAI);
        assert!(config.features.command_suggestions);
    }

    #[test]
    fn test_ai_provider_display() {
        assert_eq!(format!("{}", AIProvider::OpenAI), "openai");
        assert_eq!(format!("{}", AIProvider::Anthropic), "anthropic");
        assert_eq!(format!("{}", AIProvider::Ollama), "ollama");
    }

    #[test]
    fn test_ai_config_get_base_url() {
        let config = AIConfig::default();
        assert_eq!(config.get_base_url(), "https://api.openai.com/v1");

        let mut config = AIConfig::default();
        config.provider = AIProvider::Anthropic;
        assert_eq!(config.get_base_url(), "https://api.anthropic.com/v1");

        config.provider = AIProvider::Ollama;
        assert_eq!(config.get_base_url(), "http://localhost:11434/api");
    }

    #[test]
    fn test_ai_config_custom_base_url() {
        let mut config = AIConfig::default();
        config.model.base_url = Some("https://custom.api.com/v1".to_string());
        assert_eq!(config.get_base_url(), "https://custom.api.com/v1");
    }

    #[test]
    fn test_ai_config_is_feature_enabled() {
        let config = AIConfig::default();
        assert!(!config.is_feature_enabled("command_suggestions"));

        let mut config = AIConfig::default();
        config.enabled = true;
        assert!(config.is_feature_enabled("command_suggestions"));
        assert!(config.is_feature_enabled("error_explanation"));
        assert!(!config.is_feature_enabled("auto_suggestions"));
    }

    #[test]
    fn test_ai_privacy_default_patterns() {
        let privacy = AIPrivacyConfig::default();
        assert!(privacy.exclude_patterns.contains(&"*.key".to_string()));
        assert!(privacy.exclude_patterns.contains(&"*.env".to_string()));
        assert!(privacy.exclude_env_vars.contains(&"API_KEY".to_string()));
    }

    #[test]
    fn test_ai_model_defaults() {
        let model = AIModelConfig::default();
        assert_eq!(model.name, "gpt-4o-mini");
        assert_eq!(model.temperature, 0.7);
        assert_eq!(model.max_tokens, 1024);
    }

    #[test]
    fn test_ai_features_defaults() {
        let features = AIFeaturesConfig::default();
        assert!(features.command_suggestions);
        assert!(features.error_explanation);
        assert!(!features.auto_suggestions);
        assert_eq!(features.suggestion_delay_ms, 500);
    }

    #[test]
    fn test_ai_advanced_defaults() {
        let advanced = AIAdvancedConfig::default();
        assert_eq!(advanced.timeout_secs, 30);
        assert_eq!(advanced.retry_attempts, 3);
        assert!(advanced.cache_enabled);
    }

    #[test]
    fn test_ai_config_serialization() {
        let config = AIConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("enabled = false"));
        assert!(toml_str.contains("provider = \"openai\""));
    }

    #[test]
    fn test_ai_config_deserialization() {
        let toml_str = r#"
            enabled = true
            provider = "anthropic"

            [model]
            name = "claude-3-opus"
            temperature = 0.5
            max_tokens = 2048
        "#;

        let config: AIConfig = toml::from_str(toml_str).unwrap();
        assert!(config.enabled);
        assert_eq!(config.provider, AIProvider::Anthropic);
        assert_eq!(config.model.name, "claude-3-opus");
        assert_eq!(config.model.temperature, 0.5);
    }
}
