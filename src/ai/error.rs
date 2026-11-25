use thiserror::Error;

#[derive(Error, Debug)]
pub enum AiError {
    #[error("API error: {0}")]
    ApiError(String),

    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Provider not available: {0}")]
    ProviderNotAvailable(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Context too long: {0}")]
    ContextTooLong(String),

    #[error("Privacy violation: {0}")]
    PrivacyError(String),

    #[error("Provider error: {0}")]
    ProviderError(String),

    #[error("Suggestion failed: {0}")]
    SuggestionFailed(String),
}

impl From<reqwest::Error> for AiError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            AiError::Timeout(err.to_string())
        } else if err.is_connect() {
            AiError::NetworkError(err.to_string())
        } else if err.is_status() {
            AiError::ApiError(err.to_string())
        } else {
            AiError::NetworkError(err.to_string())
        }
    }
}

impl From<serde_json::Error> for AiError {
    fn from(err: serde_json::Error) -> Self {
        AiError::InvalidResponse(err.to_string())
    }
}

pub type AiResult<T> = Result<T, AiError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_display() {
        let err = AiError::ApiError("rate limited".to_string());
        assert_eq!(format!("{}", err), "API error: rate limited");
    }

    #[test]
    fn test_auth_error_display() {
        let err = AiError::AuthError("invalid key".to_string());
        assert_eq!(format!("{}", err), "Authentication failed: invalid key");
    }

    #[test]
    fn test_rate_limit_display() {
        let err = AiError::RateLimit("too many requests".to_string());
        assert_eq!(format!("{}", err), "Rate limit exceeded: too many requests");
    }

    #[test]
    fn test_network_error_display() {
        let err = AiError::NetworkError("connection refused".to_string());
        assert_eq!(format!("{}", err), "Network error: connection refused");
    }

    #[test]
    fn test_timeout_display() {
        let err = AiError::Timeout("30s exceeded".to_string());
        assert_eq!(format!("{}", err), "Timeout: 30s exceeded");
    }

    #[test]
    fn test_invalid_response_display() {
        let err = AiError::InvalidResponse("unexpected JSON".to_string());
        assert_eq!(format!("{}", err), "Invalid response: unexpected JSON");
    }

    #[test]
    fn test_config_error_display() {
        let err = AiError::ConfigError("missing api_key".to_string());
        assert_eq!(format!("{}", err), "Configuration error: missing api_key");
    }

    #[test]
    fn test_provider_not_available_display() {
        let err = AiError::ProviderNotAvailable("OpenAI".to_string());
        assert_eq!(format!("{}", err), "Provider not available: OpenAI");
    }

    #[test]
    fn test_model_not_found_display() {
        let err = AiError::ModelNotFound("gpt-5".to_string());
        assert_eq!(format!("{}", err), "Model not found: gpt-5");
    }

    #[test]
    fn test_context_too_long_display() {
        let err = AiError::ContextTooLong("128k tokens".to_string());
        assert_eq!(format!("{}", err), "Context too long: 128k tokens");
    }

    #[test]
    fn test_privacy_error_display() {
        let err = AiError::PrivacyError("PII detected".to_string());
        assert_eq!(format!("{}", err), "Privacy violation: PII detected");
    }

    #[test]
    fn test_provider_error_display() {
        let err = AiError::ProviderError("internal error".to_string());
        assert_eq!(format!("{}", err), "Provider error: internal error");
    }

    #[test]
    fn test_suggestion_failed_display() {
        let err = AiError::SuggestionFailed("empty input".to_string());
        assert_eq!(format!("{}", err), "Suggestion failed: empty input");
    }

    #[test]
    fn test_ai_result_ok() {
        let result: AiResult<String> = Ok("success".to_string());
        assert!(result.is_ok());
        assert_eq!(result.ok(), Some("success".to_string()));
    }

    #[test]
    fn test_ai_result_err() {
        let result: AiResult<String> = Err(AiError::Timeout("test".to_string()));
        assert!(result.is_err());
    }

    #[test]
    fn test_serde_json_error_conversion() {
        let json_err = serde_json::from_str::<String>("invalid json");
        assert!(json_err.is_err());

        let ai_err: AiError = json_err.unwrap_err().into();
        match ai_err {
            AiError::InvalidResponse(_) => {}
            _ => panic!("Expected InvalidResponse error"),
        }
    }

    #[test]
    fn test_error_debug() {
        let err = AiError::ApiError("test".to_string());
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("ApiError"));
    }
}
