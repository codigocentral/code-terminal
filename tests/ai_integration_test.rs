use code_terminal::ai::{
    create_suggester, AiConfig, AiError, AiProviderType, AuditLogEntry, ChatMessage, ChatRequest,
    CommandSuggester, DataSharingLevel, DetectedError, DocumentationLink, ErrorDetector,
    ErrorExplainer, ErrorExplanation, ErrorSeverity, ErrorType, OpenAIProvider, PrivacyConfig,
    PrivacyControls, PrivacyManager, ProviderRegistry, QuickFix, SuggesterStats, SuggestionBuilder,
    TerminalContext,
};

// ============================================================================
// Tests de Configuração AI
// ============================================================================

#[test]
fn test_ai_config_openai_creation() {
    let config = AiConfig::openai("test-api-key-12345");

    assert!(config.enabled);
    assert_eq!(config.provider, AiProviderType::OpenAI);
    assert_eq!(config.api_key, Some("test-api-key-12345".to_string()));
    assert_eq!(config.model, "gpt-4o-mini");
}

#[test]
fn test_ai_config_with_custom_model() {
    let config = AiConfig::openai("test-key")
        .with_model("gpt-4")
        .with_max_tokens(2000)
        .with_temperature(0.5);

    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.max_tokens, 2000);
    assert_eq!(config.temperature, 0.5);
}

#[test]
fn test_ai_config_default() {
    let config = AiConfig::default();

    assert!(!config.enabled);
    assert_eq!(config.provider, AiProviderType::OpenAI);
    assert!(config.api_key.is_none());
}

// ============================================================================
// Tests de TerminalContext
// ============================================================================

#[test]
fn test_terminal_context_builder() {
    let context = TerminalContext::new()
        .with_directory("/home/user/projects")
        .with_shell("bash")
        .with_recent_commands(vec!["ls -la".to_string(), "git status".to_string()])
        .with_last_output("On branch main");

    assert_eq!(
        context.current_directory,
        Some("/home/user/projects".to_string())
    );
    assert_eq!(context.shell_type, Some("bash".to_string()));
    assert_eq!(context.recent_commands.len(), 2);
    assert_eq!(context.last_output, Some("On branch main".to_string()));
}

#[test]
fn test_terminal_context_add_command() {
    let mut context = TerminalContext::new();
    context.add_command("npm install");
    context.add_command("npm run dev");

    assert_eq!(context.recent_commands.len(), 2);
    assert_eq!(context.recent_commands[0], "npm install");
    assert_eq!(context.recent_commands[1], "npm run dev");
}

#[test]
fn test_terminal_context_command_limit() {
    let mut context = TerminalContext::new();

    for i in 0..60 {
        context.add_command(format!("command_{}", i));
    }

    assert_eq!(context.recent_commands.len(), 50);
}

#[test]
fn test_terminal_context_default() {
    let context = TerminalContext::default();
    assert!(context.current_directory.is_none());
    assert!(context.recent_commands.is_empty());
    assert!(context.last_output.is_none());
    assert!(context.shell_type.is_none());
    assert!(context.environment.is_empty());
}

// ============================================================================
// Tests de Chat/Provider
// ============================================================================

#[test]
fn test_chat_message_creation() {
    let msg = ChatMessage {
        role: code_terminal::ai::provider::MessageRole::User,
        content: "How do I list files?".to_string(),
    };

    assert_eq!(msg.role, code_terminal::ai::provider::MessageRole::User);
    assert_eq!(msg.content, "How do I list files?");
}

#[test]
fn test_chat_request_serialization() {
    let request = ChatRequest {
        messages: vec![
            ChatMessage {
                role: code_terminal::ai::provider::MessageRole::System,
                content: "You are a helpful assistant".to_string(),
            },
            ChatMessage {
                role: code_terminal::ai::provider::MessageRole::User,
                content: "Hello".to_string(),
            },
        ],
        max_tokens: Some(100),
        temperature: Some(0.7),
        model: None,
    };

    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("system"));
    assert!(json.contains("Hello"));
    assert!(json.contains("100"));
    assert!(json.contains("0.7"));
}

#[test]
fn test_provider_registry_new() {
    let registry = ProviderRegistry::new();
    assert!(registry.available_providers().is_empty());
}

#[test]
fn test_openai_provider_creation() {
    let config = AiConfig::openai("test-api-key-12345");
    let provider = OpenAIProvider::new(config);

    assert!(provider.is_ok());
}

#[test]
fn test_openai_provider_missing_api_key() {
    let config = AiConfig::default();
    let result = OpenAIProvider::new(config);

    assert!(result.is_err());
}

// ============================================================================
// Tests de CommandSuggester
// ============================================================================

#[test]
fn test_command_suggester_creation() {
    let config = AiConfig::openai("test-key");
    let suggester = CommandSuggester::new(config);
    assert!(suggester.is_ok());
}

#[test]
fn test_command_suggester_enable_disable() {
    let config = AiConfig::openai("test-key");
    let suggester = CommandSuggester::new(config).unwrap();

    assert!(suggester.is_enabled());

    suggester.disable();
    assert!(!suggester.is_enabled());

    suggester.enable();
    assert!(suggester.is_enabled());
}

#[test]
fn test_command_suggester_context_update() {
    let config = AiConfig::openai("test-key");
    let suggester = CommandSuggester::new(config).unwrap();

    suggester.update_directory("/home/user");
    suggester.update_shell("zsh");
    suggester.add_command_to_history("git status");
    suggester.update_last_output("On branch main");

    let context = suggester.get_context();
    assert_eq!(context.current_directory, Some("/home/user".to_string()));
    assert_eq!(context.shell_type, Some("zsh".to_string()));
    assert_eq!(context.recent_commands.len(), 1);
    assert_eq!(context.last_output, Some("On branch main".to_string()));
}

#[test]
fn test_command_suggester_stats() {
    let config = AiConfig::openai("test-key");
    let suggester = CommandSuggester::new(config).unwrap();

    let stats = suggester.get_stats();
    assert_eq!(stats.total_suggestions, 0);
    assert_eq!(stats.accepted_suggestions, 0);
    assert_eq!(stats.acceptance_rate, 0.0);
    assert_eq!(stats.cache_size, 0);
}

#[test]
fn test_command_suggester_history_management() {
    let config = AiConfig::openai("test-key");
    let suggester = CommandSuggester::new(config).unwrap();

    suggester.clear_history();
    let history = suggester.get_history();
    assert!(history.is_empty());

    suggester.clear_cache();
}

#[test]
fn test_suggestion_builder() {
    let config = AiConfig::openai("test-key");
    let suggester = SuggestionBuilder::new("list files")
        .with_directory("/home/user")
        .with_shell("bash")
        .with_recent_commands(vec!["cd /tmp".to_string()])
        .with_config(config)
        .build();

    assert!(suggester.is_ok());

    let suggester = suggester.unwrap();
    let context = suggester.get_context();
    assert_eq!(context.current_directory, Some("/home/user".to_string()));
    assert_eq!(context.shell_type, Some("bash".to_string()));
}

// ============================================================================
// Tests de Error Detection
// ============================================================================

#[test]
fn test_error_detector_creation() {
    let _detector = ErrorDetector::new();
    // Não deve panicar
}

#[test]
fn test_error_detector_command_not_found() {
    let detector = ErrorDetector::new();

    let error = detector.detect("bash: foo: command not found");
    assert!(error.is_some());

    let error = error.unwrap();
    assert_eq!(error.error_type, ErrorType::CommandNotFound);
    assert_eq!(error.severity, ErrorSeverity::High);
}

#[test]
fn test_error_detector_permission_denied() {
    let detector = ErrorDetector::new();

    let error = detector.detect("bash: ./script.sh: Permission denied");
    assert!(error.is_some());

    let error = error.unwrap();
    assert_eq!(error.error_type, ErrorType::PermissionDenied);
}

#[test]
fn test_error_detector_python_error() {
    let detector = ErrorDetector::new();
    let output = r#"
Traceback (most recent call last):
  File "test.py", line 1, in <module>
    import missing_module
ModuleNotFoundError: No module named 'missing_module'
"#;

    let error = detector.detect(output);
    assert!(error.is_some());

    let error = error.unwrap();
    assert_eq!(error.error_type, ErrorType::PythonError);
    assert!(!error.suggestions.is_empty());
}

#[test]
fn test_error_detector_rust_error() {
    let detector = ErrorDetector::new();

    let error = detector.detect("error[E0433]: failed to resolve: use of undeclared crate");
    assert!(error.is_some());

    let error = error.unwrap();
    assert_eq!(error.error_type, ErrorType::RustError);
}

#[test]
fn test_error_detector_no_error() {
    let detector = ErrorDetector::new();

    let error = detector.detect("Hello, World! Everything is fine.");
    assert!(error.is_none());
}

#[test]
fn test_error_detector_detect_all() {
    let detector = ErrorDetector::new();
    let output = r#"
bash: foo: command not found
permission denied: /root/file
npm ERR! missing script
"#;

    let errors = detector.detect_all(output);
    assert!(!errors.is_empty());
}

// #[test]
// fn test_error_detector_file_extraction() {
//     let detector = ErrorDetector::new();
//     let text = "Error in src/main.rs at line 10 and lib/test.py";
//
//     let files = detector.extract_files(text);
//     assert!(files.contains(&"src/main.rs".to_string()));
//     assert!(files.contains(&"lib/test.py".to_string()));
// }

// ============================================================================
// Tests de ErrorExplainer
// ============================================================================

#[test]
fn test_error_explainer_creation() {
    let config = AiConfig::openai("test-key");
    let _explainer = ErrorExplainer::new(config);
    // Não deve panicar
}

#[test]
fn test_error_explainer_detect_error() {
    let config = AiConfig::openai("test-key");
    let explainer = ErrorExplainer::new(config);

    let error = explainer.detect_error("bash: foo: command not found");
    assert!(error.is_some());
    assert_eq!(error.unwrap().error_type, ErrorType::CommandNotFound);
}

#[test]
fn test_error_explainer_detect_all_errors() {
    let config = AiConfig::openai("test-key");
    let explainer = ErrorExplainer::new(config);

    let output = "bash: foo: command not found\npermission denied: /root/file";
    let errors = explainer.detect_all_errors(output);
    assert!(!errors.is_empty());
}

#[tokio::test]
async fn test_error_explainer_disabled() {
    let config = AiConfig::openai("test-key");
    let explainer = ErrorExplainer::new(config);
    explainer.set_enabled(false).await;

    let detected = DetectedError {
        error_type: ErrorType::CommandNotFound,
        message: "foo: command not found".to_string(),
        context: vec![],
        line_number: Some(0),
        severity: ErrorSeverity::High,
        suggestions: vec![],
        related_files: vec![],
    };

    let context = TerminalContext::default();
    let result: Result<ErrorExplanation, AiError> = explainer.explain(&detected, &context).await;
    assert!(result.is_err());
}

// ============================================================================
// Tests de Privacy Controls
// ============================================================================

#[test]
fn test_privacy_controls_creation() {
    let config = code_terminal::config::ai::AIPrivacyConfig::default();
    let _controls = PrivacyControls::new(config);
    // Não deve panicar
}

#[test]
fn test_privacy_controls_sanitize_paths() {
    let config = code_terminal::config::ai::AIPrivacyConfig::default();
    let controls = PrivacyControls::new(config);

    let input = "File at /home/user/secret.key contains data";
    let result = controls.sanitize_for_ai(input, "file_paths").unwrap();

    // Deve conter [PATH] ou [REDACTED]
    assert!(
        result.contains("[PATH]")
            || result.contains("[REDACTED]")
            || !result.contains("/home/user")
    );
}

#[test]
fn test_privacy_controls_data_type_allowed() {
    let config = code_terminal::config::ai::AIPrivacyConfig::default();
    let controls = PrivacyControls::new(config);

    assert!(controls.is_data_type_allowed("terminal_output"));
    assert!(controls.is_data_type_allowed("command"));
    assert!(controls.is_data_type_allowed("error_message"));
    assert!(!controls.is_data_type_allowed("invalid_type"));
}

#[test]
fn test_privacy_controls_data_sharing_level() {
    let config = code_terminal::config::ai::AIPrivacyConfig {
        anonymize_paths: true,
        anonymize_usernames: true,
        redact_secrets: true,
        ..Default::default()
    };
    let controls = PrivacyControls::new(config);

    assert_eq!(controls.get_data_sharing_level(), DataSharingLevel::Minimal);
}

#[test]
fn test_privacy_controls_confirmation_flow() {
    let config = code_terminal::config::ai::AIPrivacyConfig {
        require_confirmation: true,
        ..Default::default()
    };
    let controls = PrivacyControls::new(config);

    let confirmation_id = controls
        .request_confirmation("file_paths", "/home/user/data")
        .unwrap();

    let pending = controls.get_pending_confirmations().unwrap();
    assert_eq!(pending.len(), 1);

    controls.approve_confirmation(&confirmation_id).unwrap();

    let pending = controls.get_pending_confirmations().unwrap();
    assert_eq!(pending.len(), 0);
}

#[test]
fn test_privacy_controls_confirmation_deny() {
    let config = code_terminal::config::ai::AIPrivacyConfig {
        require_confirmation: true,
        ..Default::default()
    };
    let controls = PrivacyControls::new(config);

    let confirmation_id = controls
        .request_confirmation("file_paths", "/home/user/data")
        .unwrap();

    let result = controls.deny_confirmation(&confirmation_id);
    assert!(result.is_err());
}

// ============================================================================
// Tests de PrivacyManager
// ============================================================================

fn create_test_ai_privacy_config() -> code_terminal::config::ai::AIPrivacyConfig {
    code_terminal::config::ai::AIPrivacyConfig {
        anonymize_paths: true,
        anonymize_usernames: true,
        exclude_patterns: vec!["*.key".to_string()],
        exclude_env_vars: vec!["API_KEY".to_string(), "MY_SECRET".to_string()],
        require_confirmation: false,
        log_requests: true,
        redact_secrets: true,
    }
}

#[test]
fn test_privacy_manager_creation() {
    let config = create_test_ai_privacy_config();
    let _manager = PrivacyManager::new(config);
    // Não deve panicar
}

#[test]
fn test_privacy_manager_sanitize_command() {
    let config = create_test_ai_privacy_config();
    let manager = PrivacyManager::new(config);

    let command = "export api_key=secret123";
    let result = manager.sanitize_command(command).unwrap();

    assert!(!result.contains("secret123"));
}

#[test]
fn test_privacy_manager_file_path_exclusion() {
    let config = create_test_ai_privacy_config();
    let manager = PrivacyManager::new(config);

    assert!(!manager.should_send_file_path("secret.key"));
    assert!(manager.should_send_file_path("readme.md"));
}

#[test]
fn test_privacy_manager_env_var_exclusion() {
    let config = create_test_ai_privacy_config();
    let manager = PrivacyManager::new(config);

    assert!(!manager.should_send_env_var("API_KEY"));
    assert!(!manager.should_send_env_var("MY_SECRET"));
    assert!(manager.should_send_env_var("PATH"));
}

#[test]
fn test_privacy_manager_data_sharing_level() {
    let config = create_test_ai_privacy_config();
    let manager = PrivacyManager::new(config);

    let level = manager.get_data_sharing_level();
    assert_eq!(level, DataSharingLevel::Minimal);
}

#[test]
fn test_privacy_manager_audit_log() {
    let config = create_test_ai_privacy_config();
    let manager = PrivacyManager::new(config);

    manager.sanitize_terminal_output("test output").unwrap();

    let log = manager.get_audit_log().unwrap();
    // Deve ter pelo menos uma entrada
    assert!(!log.is_empty());
}

// ============================================================================
// Tests de PrivacyConfig
// ============================================================================

#[test]
fn test_privacy_config_default() {
    let config = PrivacyConfig::default();

    assert!(!config.send_shell_history);
    assert!(!config.send_file_paths);
    assert!(!config.send_environment_vars);
    assert!(config.require_confirmation);
    assert!(!config.log_requests);
    assert!(!config.redact_patterns.is_empty());
}

#[test]
fn test_privacy_config_redact_patterns() {
    let config = PrivacyConfig::default();

    let has_password_pattern = config
        .redact_patterns
        .iter()
        .any(|p| p.contains("password"));
    assert!(has_password_pattern);

    let has_api_key_pattern = config.redact_patterns.iter().any(|p| p.contains("api"));
    assert!(has_api_key_pattern);
}

// ============================================================================
// Tests de Provider Types
// ============================================================================

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

// ============================================================================
// Tests de Error Types
// ============================================================================

#[test]
fn test_error_type_display() {
    assert_eq!(
        format!("{}", ErrorType::CommandNotFound),
        "Command Not Found"
    );
    assert_eq!(
        format!("{}", ErrorType::PermissionDenied),
        "Permission Denied"
    );
    assert_eq!(format!("{}", ErrorType::RustError), "Rust Error");
}

#[test]
fn test_error_severity_display() {
    assert_eq!(format!("{}", ErrorSeverity::Low), "Low");
    assert_eq!(format!("{}", ErrorSeverity::High), "High");
    assert_eq!(format!("{}", ErrorSeverity::Critical), "Critical");
}

// ============================================================================
// Tests de Serialization
// ============================================================================

#[test]
fn test_message_role_serialization() {
    use code_terminal::ai::provider::MessageRole;
    let roles = vec![
        (MessageRole::System, "system"),
        (MessageRole::User, "user"),
        (MessageRole::Assistant, "assistant"),
    ];

    for (role, expected) in roles {
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, format!("\"{}\"", expected));
    }
}

#[test]
fn test_message_role_deserialization() {
    use code_terminal::ai::provider::MessageRole;
    let role: MessageRole = serde_json::from_str("\"user\"").unwrap();
    assert_eq!(role, MessageRole::User);

    let role: MessageRole = serde_json::from_str("\"system\"").unwrap();
    assert_eq!(role, MessageRole::System);
}

#[test]
fn test_ai_config_serialization() {
    let config = AiConfig::openai("test-key")
        .with_model("gpt-4")
        .with_max_tokens(1000);

    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("gpt-4"));
    assert!(json.contains("1000"));

    let deserialized: AiConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.model, "gpt-4");
    assert_eq!(deserialized.max_tokens, 1000);
}

#[test]
fn test_terminal_context_serialization() {
    let context = TerminalContext::new()
        .with_directory("/home/user")
        .with_shell("bash");

    let json = serde_json::to_string(&context).unwrap();
    assert!(json.contains("/home/user"));
    assert!(json.contains("bash"));

    let deserialized: TerminalContext = serde_json::from_str(&json).unwrap();
    assert_eq!(
        deserialized.current_directory,
        Some("/home/user".to_string())
    );
    assert_eq!(deserialized.shell_type, Some("bash".to_string()));
}

// ============================================================================
// Tests de Create Functions
// ============================================================================

#[test]
fn test_create_suggester() {
    let config = AiConfig::openai("test-key");
    let result = create_suggester(config);
    assert!(result.is_ok());
}

#[test]
fn test_create_suggester_invalid() {
    let config = AiConfig::default(); // Sem API key
    let result = create_suggester(config);
    assert!(result.is_err());
}

// ============================================================================
// Tests de QuickFix e DocumentationLink
// ============================================================================

#[test]
fn test_quick_fix_creation() {
    let fix = QuickFix {
        description: "Install command".to_string(),
        command: Some("apt install foo".to_string()),
        automatic: false,
    };

    assert_eq!(fix.description, "Install command");
    assert_eq!(fix.command, Some("apt install foo".to_string()));
    assert!(!fix.automatic);
}

#[test]
fn test_documentation_link_creation() {
    let link = DocumentationLink {
        title: "Rust Book".to_string(),
        url: "https://doc.rust-lang.org/book/".to_string(),
    };

    assert_eq!(link.title, "Rust Book");
    assert_eq!(link.url, "https://doc.rust-lang.org/book/");
}

// ============================================================================
// Tests de Error Explanation
// ============================================================================

#[test]
fn test_error_explanation_to_markdown() {
    let detected = DetectedError {
        error_type: ErrorType::CommandNotFound,
        message: "foo: command not found".to_string(),
        context: vec![],
        line_number: Some(0),
        severity: ErrorSeverity::High,
        suggestions: vec![],
        related_files: vec![],
    };

    let explanation = ErrorExplanation {
        error: detected,
        ai_explanation: "The command 'foo' is not installed.".to_string(),
        quick_fixes: vec![QuickFix {
            description: "Install command".to_string(),
            command: Some("apt install foo".to_string()),
            automatic: false,
        }],
        documentation_links: vec![DocumentationLink {
            title: "Linux Commands".to_string(),
            url: "https://linux.die.net/".to_string(),
        }],
        related_commands: vec!["which".to_string(), "whereis".to_string()],
    };

    let md = explanation.to_markdown();
    assert!(md.contains("Command Not Found"));
    assert!(md.contains("AI Explanation"));
    assert!(md.contains("Quick Fixes"));
    assert!(md.contains("Documentation"));
    assert!(md.contains("Related Commands"));
}

// ============================================================================
// Tests de AiError
// ============================================================================

#[test]
fn test_ai_error_display() {
    let err = AiError::ApiError("rate limited".to_string());
    assert_eq!(format!("{}", err), "API error: rate limited");

    let err = AiError::ConfigError("missing key".to_string());
    assert_eq!(format!("{}", err), "Configuration error: missing key");

    let err = AiError::NetworkError("timeout".to_string());
    assert_eq!(format!("{}", err), "Network error: timeout");
}

#[test]
fn test_ai_error_from_serde() {
    let json_err = serde_json::from_str::<String>("invalid");
    assert!(json_err.is_err());

    let ai_err: AiError = json_err.unwrap_err().into();
    match ai_err {
        AiError::InvalidResponse(_) => {}
        _ => panic!("Expected InvalidResponse error"),
    }
}

// ============================================================================
// Tests de DataSharingLevel
// ============================================================================

#[test]
fn test_data_sharing_level_variants() {
    assert_eq!(DataSharingLevel::None as i32, 0);
    assert_eq!(DataSharingLevel::Minimal as i32, 1);
    assert_eq!(DataSharingLevel::Standard as i32, 2);
    assert_eq!(DataSharingLevel::Full as i32, 3);
}

// ============================================================================
// Tests de AuditLogEntry
// ============================================================================

#[test]
fn test_audit_log_entry_creation() {
    let entry = AuditLogEntry {
        timestamp: chrono::Utc::now(),
        action: "sanitize".to_string(),
        data_type: "terminal_output".to_string(),
        redacted_count: 5,
        approved: true,
        user_message: Some("Test message".to_string()),
    };

    assert_eq!(entry.action, "sanitize");
    assert_eq!(entry.data_type, "terminal_output");
    assert_eq!(entry.redacted_count, 5);
    assert!(entry.approved);
}

// ============================================================================
// Tests de CommandSuggestion
// ============================================================================

// #[test]
// fn test_command_suggestion_creation() {
//     let suggestion = CommandSuggestion {
//         command: "ls -la".to_string(),
//         explanation: "List all files with details".to_string(),
//         alternatives: Some(vec!["ls -lh".to_string()]),
//         confidence: 0.95,
//     };
//
//     assert_eq!(suggestion.command, "ls -la");
//     assert_eq!(suggestion.explanation, "List all files with details");
//     assert_eq!(suggestion.confidence, 0.95);
// }

// ============================================================================
// Tests de SuggesterStats
// ============================================================================

#[test]
fn test_suggester_stats_creation() {
    let stats = SuggesterStats {
        total_suggestions: 10,
        accepted_suggestions: 7,
        acceptance_rate: 0.7,
        cache_size: 5,
    };

    assert_eq!(stats.total_suggestions, 10);
    assert_eq!(stats.accepted_suggestions, 7);
    assert_eq!(stats.acceptance_rate, 0.7);
    assert_eq!(stats.cache_size, 5);
}

// ============================================================================
// Tests de SuggestionHistory
// ============================================================================

#[test]
fn test_suggestion_history_access() {
    let config = AiConfig::openai("test-key");
    let suggester = CommandSuggester::new(config).unwrap();

    // Histórico inicial deve estar vazio
    let history = suggester.get_history();
    assert!(history.is_empty());

    // Recent suggestions também deve estar vazio
    let recent = suggester.get_recent_suggestions(5);
    assert!(recent.is_empty());
}
