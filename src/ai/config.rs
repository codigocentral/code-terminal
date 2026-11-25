use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum AiProviderType {
    #[serde(rename = "openai")]
    #[default]
    OpenAI,
    #[serde(rename = "anthropic")]
    Anthropic,
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "custom")]
    Custom,
}

impl std::fmt::Display for AiProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAI => write!(f, "openai"),
            Self::Anthropic => write!(f, "anthropic"),
            Self::Ollama => write!(f, "ollama"),
            Self::Custom => write!(f, "custom"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub provider: AiProviderType,

    #[serde(default = "default_model")]
    pub model: String,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default = "default_base_url")]
    pub base_url: Option<String>,

    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    #[serde(default = "default_temperature")]
    pub temperature: f32,

    #[serde(default)]
    pub timeout_seconds: u64,

    #[serde(default)]
    pub privacy: PrivacyConfig,

    #[serde(default)]
    pub extra: HashMap<String, String>,
}

fn default_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_base_url() -> Option<String> {
    None
}

fn default_max_tokens() -> u32 {
    1024
}

fn default_temperature() -> f32 {
    0.7
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: AiProviderType::default(),
            model: default_model(),
            api_key: None,
            base_url: None,
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
            timeout_seconds: 30,
            privacy: PrivacyConfig::default(),
            extra: HashMap::new(),
        }
    }
}

impl AiConfig {
    pub fn openai(api_key: impl Into<String>) -> Self {
        Self {
            enabled: true,
            provider: AiProviderType::OpenAI,
            api_key: Some(api_key.into()),
            ..Default::default()
        }
    }

    pub fn anthropic(api_key: impl Into<String>) -> Self {
        Self {
            enabled: true,
            provider: AiProviderType::Anthropic,
            api_key: Some(api_key.into()),
            model: "claude-3-5-sonnet-20241022".to_string(),
            ..Default::default()
        }
    }

    pub fn ollama() -> Self {
        Self {
            enabled: true,
            provider: AiProviderType::Ollama,
            base_url: Some("http://localhost:11434".to_string()),
            model: "llama3.2".to_string(),
            ..Default::default()
        }
    }

    pub fn custom(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            enabled: true,
            provider: AiProviderType::Custom,
            base_url: Some(base_url.into()),
            model: model.into(),
            ..Default::default()
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = tokens;
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = temp.clamp(0.0, 2.0);
        self
    }

    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout_seconds = seconds;
        self
    }

    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn get_api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    pub fn get_base_url(&self) -> &str {
        self.base_url.as_deref().unwrap_or(match self.provider {
            AiProviderType::OpenAI => "https://api.openai.com/v1",
            AiProviderType::Anthropic => "https://api.anthropic.com/v1",
            AiProviderType::Ollama => "http://localhost:11434",
            AiProviderType::Custom => "",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    #[serde(default)]
    pub send_shell_history: bool,

    #[serde(default)]
    pub send_file_paths: bool,

    #[serde(default)]
    pub send_environment_vars: bool,

    #[serde(default)]
    pub redact_patterns: Vec<String>,

    #[serde(default = "default_true")]
    pub require_confirmation: bool,

    #[serde(default)]
    pub log_requests: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            send_shell_history: false,
            send_file_paths: false,
            send_environment_vars: false,
            redact_patterns: vec![
                r"[A-Za-z0-9]{32,}".to_string(),
                r"sk-[A-Za-z0-9]{20,}".to_string(),
                r"password\s*[=:]\s*\S+".to_string(),
                r"token\s*[=:]\s*\S+".to_string(),
                r"secret\s*[=:]\s*\S+".to_string(),
                r"api[_-]?key\s*[=:]\s*\S+".to_string(),
            ],
            require_confirmation: true,
            log_requests: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_config_default() {
        let config = AiConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.provider, AiProviderType::OpenAI);
        assert_eq!(config.model, "gpt-4o-mini");
        assert_eq!(config.max_tokens, 1024);
        assert_eq!(config.temperature, 0.7);
        assert_eq!(config.timeout_seconds, 30);
        assert!(config.api_key.is_none());
        assert!(config.base_url.is_none());
    }

    #[test]
    fn test_ai_config_openai() {
        let config = AiConfig::openai("test-key");
        assert!(config.enabled);
        assert_eq!(config.provider, AiProviderType::OpenAI);
        assert_eq!(config.api_key, Some("test-key".to_string()));
        assert_eq!(config.model, "gpt-4o-mini");
    }

    #[test]
    fn test_ai_config_anthropic() {
        let config = AiConfig::anthropic("anthropic-key");
        assert!(config.enabled);
        assert_eq!(config.provider, AiProviderType::Anthropic);
        assert_eq!(config.api_key, Some("anthropic-key".to_string()));
        assert_eq!(config.model, "claude-3-5-sonnet-20241022");
    }

    #[test]
    fn test_ai_config_ollama() {
        let config = AiConfig::ollama();
        assert!(config.enabled);
        assert_eq!(config.provider, AiProviderType::Ollama);
        assert!(config.api_key.is_none());
        assert_eq!(config.base_url, Some("http://localhost:11434".to_string()));
        assert_eq!(config.model, "llama3.2");
    }

    #[test]
    fn test_ai_config_custom() {
        let config = AiConfig::custom("http://localhost:8080", "local-model");
        assert!(config.enabled);
        assert_eq!(config.provider, AiProviderType::Custom);
        assert_eq!(config.base_url, Some("http://localhost:8080".to_string()));
        assert_eq!(config.model, "local-model");
    }

    #[test]
    fn test_ai_config_with_model() {
        let config = AiConfig::openai("key").with_model("gpt-4");
        assert_eq!(config.model, "gpt-4");
    }

    #[test]
    fn test_ai_config_with_max_tokens() {
        let config = AiConfig::openai("key").with_max_tokens(2048);
        assert_eq!(config.max_tokens, 2048);
    }

    #[test]
    fn test_ai_config_with_temperature() {
        let config = AiConfig::openai("key").with_temperature(0.5);
        assert_eq!(config.temperature, 0.5);
    }

    #[test]
    fn test_ai_config_temperature_clamp_high() {
        let config = AiConfig::openai("key").with_temperature(3.0);
        assert_eq!(config.temperature, 2.0);
    }

    #[test]
    fn test_ai_config_temperature_clamp_low() {
        let config = AiConfig::openai("key").with_temperature(-1.0);
        assert_eq!(config.temperature, 0.0);
    }

    #[test]
    fn test_ai_config_with_timeout() {
        let config = AiConfig::openai("key").with_timeout(60);
        assert_eq!(config.timeout_seconds, 60);
    }

    #[test]
    fn test_ai_config_get_api_key() {
        let config = AiConfig::openai("my-key");
        assert_eq!(config.get_api_key(), Some("my-key"));

        let config = AiConfig::ollama();
        assert_eq!(config.get_api_key(), None);
    }

    #[test]
    fn test_ai_config_get_base_url_openai() {
        let config = AiConfig::openai("key");
        assert_eq!(config.get_base_url(), "https://api.openai.com/v1");
    }

    #[test]
    fn test_ai_config_get_base_url_anthropic() {
        let config = AiConfig::anthropic("key");
        assert_eq!(config.get_base_url(), "https://api.anthropic.com/v1");
    }

    #[test]
    fn test_ai_config_get_base_url_ollama() {
        let config = AiConfig::ollama();
        assert_eq!(config.get_base_url(), "http://localhost:11434");
    }

    #[test]
    fn test_ai_config_get_base_url_custom() {
        let config = AiConfig::custom("http://custom.api", "model");
        assert_eq!(config.get_base_url(), "http://custom.api");
    }

    #[test]
    fn test_ai_config_get_base_url_custom_default() {
        let config = AiConfig {
            provider: AiProviderType::Custom,
            base_url: None,
            ..Default::default()
        };
        assert_eq!(config.get_base_url(), "");
    }

    #[test]
    fn test_ai_config_override_base_url() {
        let config = AiConfig::openai("key");
        let config = AiConfig {
            base_url: Some("https://proxy.openai.com/v1".to_string()),
            ..config
        };
        assert_eq!(config.get_base_url(), "https://proxy.openai.com/v1");
    }

    #[test]
    fn test_privacy_config_default() {
        let privacy = PrivacyConfig::default();
        assert!(!privacy.send_shell_history);
        assert!(!privacy.send_file_paths);
        assert!(!privacy.send_environment_vars);
        assert!(privacy.require_confirmation);
        assert!(!privacy.log_requests);
        assert!(!privacy.redact_patterns.is_empty());
    }

    #[test]
    fn test_privacy_config_redact_patterns() {
        let privacy = PrivacyConfig::default();
        assert!(privacy.redact_patterns.iter().any(|p| p.contains("sk-")));
        assert!(privacy
            .redact_patterns
            .iter()
            .any(|p| p.contains("password")));
        assert!(privacy.redact_patterns.iter().any(|p| p.contains("token")));
        assert!(privacy.redact_patterns.iter().any(|p| p.contains("secret")));
    }

    #[test]
    fn test_ai_provider_type_default() {
        let provider = AiProviderType::default();
        assert_eq!(provider, AiProviderType::OpenAI);
    }

    #[test]
    fn test_ai_provider_type_display() {
        assert_eq!(format!("{}", AiProviderType::OpenAI), "openai");
        assert_eq!(format!("{}", AiProviderType::Anthropic), "anthropic");
        assert_eq!(format!("{}", AiProviderType::Ollama), "ollama");
        assert_eq!(format!("{}", AiProviderType::Custom), "custom");
    }

    #[test]
    fn test_ai_provider_type_equality() {
        assert_eq!(AiProviderType::OpenAI, AiProviderType::OpenAI);
        assert_ne!(AiProviderType::OpenAI, AiProviderType::Anthropic);
    }

    #[test]
    fn test_ai_provider_type_serialization() {
        let provider = AiProviderType::OpenAI;
        let json = serde_json::to_string(&provider).unwrap();
        assert_eq!(json, "\"openai\"");

        let provider = AiProviderType::Anthropic;
        let json = serde_json::to_string(&provider).unwrap();
        assert_eq!(json, "\"anthropic\"");
    }

    #[test]
    fn test_ai_provider_type_deserialization() {
        let provider: AiProviderType = serde_json::from_str("\"openai\"").unwrap();
        assert_eq!(provider, AiProviderType::OpenAI);

        let provider: AiProviderType = serde_json::from_str("\"ollama\"").unwrap();
        assert_eq!(provider, AiProviderType::Ollama);
    }

    #[test]
    fn test_ai_config_serialization() {
        let config = AiConfig::openai("test-key");
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("openai"));
        assert!(json.contains("test-key"));
    }

    #[test]
    fn test_ai_config_deserialization() {
        let json = r#"{"enabled":true,"provider":"openai","model":"gpt-4","api_key":"key"}"#;
        let config: AiConfig = serde_json::from_str(json).unwrap();
        assert!(config.enabled);
        assert_eq!(config.provider, AiProviderType::OpenAI);
        assert_eq!(config.model, "gpt-4");
    }

    #[test]
    fn test_ai_config_extra_field() {
        let mut config = AiConfig::default();
        config
            .extra
            .insert("custom_option".to_string(), "value".to_string());
        assert_eq!(
            config.extra.get("custom_option"),
            Some(&"value".to_string())
        );
    }

    #[test]
    fn test_ai_config_builder_chain() {
        let config = AiConfig::openai("key")
            .with_model("gpt-4-turbo")
            .with_max_tokens(4096)
            .with_temperature(0.3)
            .with_timeout(120);

        assert!(config.enabled);
        assert_eq!(config.model, "gpt-4-turbo");
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.temperature, 0.3);
        assert_eq!(config.timeout_seconds, 120);
    }
}
