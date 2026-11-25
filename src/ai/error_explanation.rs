use crate::ai::config::AiConfig;
use crate::ai::error::{AiError, AiResult};
use crate::ai::error_detection::{DetectedError, ErrorDetector, ErrorType};
use crate::ai::provider::{AIProvider, TerminalContext};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ErrorExplainer {
    detector: ErrorDetector,
    provider: Arc<RwLock<Option<Box<dyn AIProvider>>>>,
    config: AiConfig,
    enabled: Arc<RwLock<bool>>,
}

impl ErrorExplainer {
    pub fn new(config: AiConfig) -> Self {
        Self {
            detector: ErrorDetector::new(),
            provider: Arc::new(RwLock::new(None)),
            config,
            enabled: Arc::new(RwLock::new(true)),
        }
    }

    pub fn with_provider(mut self, provider: Box<dyn AIProvider>) -> Self {
        self.provider = Arc::new(RwLock::new(Some(provider)));
        self
    }

    pub fn with_detector(mut self, detector: ErrorDetector) -> Self {
        self.detector = detector;
        self
    }

    pub fn config(&self) -> &AiConfig {
        &self.config
    }

    pub async fn set_provider(&self, provider: Box<dyn AIProvider>) {
        let mut guard = self.provider.write().await;
        *guard = Some(provider);
    }

    pub async fn set_enabled(&self, enabled: bool) {
        let mut guard = self.enabled.write().await;
        *guard = enabled;
    }

    pub async fn is_enabled(&self) -> bool {
        *self.enabled.read().await
    }

    pub fn detect_error(&self, output: &str) -> Option<DetectedError> {
        self.detector.detect(output)
    }

    pub fn detect_all_errors(&self, output: &str) -> Vec<DetectedError> {
        self.detector.detect_all(output)
    }

    pub async fn explain(
        &self,
        error: &DetectedError,
        context: &TerminalContext,
    ) -> AiResult<ErrorExplanation> {
        if !self.is_enabled().await {
            return Err(AiError::ConfigError(
                "Error explanation is disabled".to_string(),
            ));
        }

        let provider_guard = self.provider.read().await;
        let provider = provider_guard.as_ref().ok_or_else(|| {
            AiError::ProviderNotAvailable("No AI provider configured".to_string())
        })?;

        if !provider.is_configured() {
            return Err(AiError::ConfigError(
                "AI provider is not properly configured".to_string(),
            ));
        }

        let ai_explanation = provider.explain_error(&error.message, context).await?;

        let explanation = ErrorExplanation {
            error: error.clone(),
            ai_explanation,
            quick_fixes: self.generate_quick_fixes(error),
            documentation_links: self.get_documentation_links(error),
            related_commands: self.get_related_commands(error),
        };

        Ok(explanation)
    }

    pub async fn explain_from_output(
        &self,
        output: &str,
        context: &TerminalContext,
    ) -> AiResult<Option<ErrorExplanation>> {
        let error = self.detect_error(output);

        match error {
            Some(error) => {
                let explanation = self.explain(&error, context).await?;
                Ok(Some(explanation))
            }
            None => Ok(None),
        }
    }

    fn generate_quick_fixes(&self, error: &DetectedError) -> Vec<QuickFix> {
        let mut fixes = Vec::new();

        match error.error_type {
            ErrorType::CommandNotFound => {
                if let Some(cmd) = error.message.split_whitespace().next() {
                    fixes.push(QuickFix {
                        description: format!("Instalar {}", cmd),
                        command: Some(format!("sudo apt install {}", cmd)),
                        automatic: false,
                    });
                    fixes.push(QuickFix {
                        description: "Verificar PATH".to_string(),
                        command: Some("echo $PATH".to_string()),
                        automatic: false,
                    });
                }
            }
            ErrorType::PermissionDenied => {
                fixes.push(QuickFix {
                    description: "Adicionar permissão de execução".to_string(),
                    command: Some("chmod +x {}".to_string()),
                    automatic: false,
                });
                fixes.push(QuickFix {
                    description: "Executar com sudo".to_string(),
                    command: Some("sudo {}".to_string()),
                    automatic: false,
                });
            }
            ErrorType::FileNotFoundError => {
                fixes.push(QuickFix {
                    description: "Listar arquivos no diretório".to_string(),
                    command: Some("ls -la".to_string()),
                    automatic: false,
                });
                fixes.push(QuickFix {
                    description: "Verificar diretório atual".to_string(),
                    command: Some("pwd".to_string()),
                    automatic: false,
                });
            }
            ErrorType::GitError => {
                fixes.push(QuickFix {
                    description: "Verificar status do git".to_string(),
                    command: Some("git status".to_string()),
                    automatic: false,
                });
                fixes.push(QuickFix {
                    description: "Verificar log do git".to_string(),
                    command: Some("git log --oneline -5".to_string()),
                    automatic: false,
                });
            }
            ErrorType::NpmError => {
                fixes.push(QuickFix {
                    description: "Reinstalar dependências".to_string(),
                    command: Some("npm install".to_string()),
                    automatic: false,
                });
                fixes.push(QuickFix {
                    description: "Limpar cache do npm".to_string(),
                    command: Some("npm cache clean --force".to_string()),
                    automatic: false,
                });
            }
            ErrorType::PythonError => {
                fixes.push(QuickFix {
                    description: "Instalar dependências".to_string(),
                    command: Some("pip install -r requirements.txt".to_string()),
                    automatic: false,
                });
                fixes.push(QuickFix {
                    description: "Verificar ambiente virtual".to_string(),
                    command: Some("which python".to_string()),
                    automatic: false,
                });
            }
            ErrorType::RustError => {
                fixes.push(QuickFix {
                    description: "Limpar e reconstruir".to_string(),
                    command: Some("cargo clean && cargo build".to_string()),
                    automatic: false,
                });
                fixes.push(QuickFix {
                    description: "Verificar versão do Rust".to_string(),
                    command: Some("rustc --version".to_string()),
                    automatic: false,
                });
            }
            ErrorType::DockerError => {
                fixes.push(QuickFix {
                    description: "Verificar status do Docker".to_string(),
                    command: Some("systemctl status docker".to_string()),
                    automatic: false,
                });
                fixes.push(QuickFix {
                    description: "Listar contêineres".to_string(),
                    command: Some("docker ps -a".to_string()),
                    automatic: false,
                });
            }
            _ => {}
        }

        fixes
    }

    fn get_documentation_links(&self, error: &DetectedError) -> Vec<DocumentationLink> {
        let mut links = Vec::new();

        match error.error_type {
            ErrorType::CommandNotFound => {
                links.push(DocumentationLink {
                    title: "Linux Command Not Found".to_string(),
                    url: "https://linux.die.net/".to_string(),
                });
            }
            ErrorType::RustError => {
                links.push(DocumentationLink {
                    title: "Rust Error Index".to_string(),
                    url: "https://doc.rust-lang.org/error-index.html".to_string(),
                });
                links.push(DocumentationLink {
                    title: "Rust Book".to_string(),
                    url: "https://doc.rust-lang.org/book/".to_string(),
                });
            }
            ErrorType::PythonError => {
                links.push(DocumentationLink {
                    title: "Python Documentation".to_string(),
                    url: "https://docs.python.org/3/".to_string(),
                });
                links.push(DocumentationLink {
                    title: "Python Errors".to_string(),
                    url: "https://docs.python.org/3/tutorial/errors.html".to_string(),
                });
            }
            ErrorType::NpmError => {
                links.push(DocumentationLink {
                    title: "NPM Documentation".to_string(),
                    url: "https://docs.npmjs.com/".to_string(),
                });
            }
            ErrorType::GitError => {
                links.push(DocumentationLink {
                    title: "Git Documentation".to_string(),
                    url: "https://git-scm.com/doc".to_string(),
                });
            }
            ErrorType::DockerError => {
                links.push(DocumentationLink {
                    title: "Docker Documentation".to_string(),
                    url: "https://docs.docker.com/".to_string(),
                });
            }
            _ => {}
        }

        links
    }

    fn get_related_commands(&self, error: &DetectedError) -> Vec<String> {
        match error.error_type {
            ErrorType::CommandNotFound => vec![
                "which".to_string(),
                "type".to_string(),
                "whereis".to_string(),
            ],
            ErrorType::PermissionDenied => vec![
                "chmod".to_string(),
                "chown".to_string(),
                "ls -l".to_string(),
            ],
            ErrorType::FileNotFoundError => {
                vec!["find".to_string(), "locate".to_string(), "ls".to_string()]
            }
            ErrorType::GitError => vec![
                "git status".to_string(),
                "git log".to_string(),
                "git diff".to_string(),
            ],
            ErrorType::DockerError => vec![
                "docker ps".to_string(),
                "docker logs".to_string(),
                "docker inspect".to_string(),
            ],
            ErrorType::NpmError => vec![
                "npm list".to_string(),
                "npm outdated".to_string(),
                "npm audit".to_string(),
            ],
            ErrorType::PythonError => vec![
                "pip list".to_string(),
                "pip show".to_string(),
                "python --version".to_string(),
            ],
            ErrorType::RustError => vec![
                "cargo check".to_string(),
                "cargo clippy".to_string(),
                "cargo doc".to_string(),
            ],
            _ => vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ErrorExplanation {
    pub error: DetectedError,
    pub ai_explanation: String,
    pub quick_fixes: Vec<QuickFix>,
    pub documentation_links: Vec<DocumentationLink>,
    pub related_commands: Vec<String>,
}

impl ErrorExplanation {
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();

        md.push_str(&format!("# {} Error\n\n", self.error.error_type));
        md.push_str(&format!("**Severity:** {}\n\n", self.error.severity));
        md.push_str(&format!("**Message:** {}\n\n", self.error.message));

        md.push_str("## AI Explanation\n\n");
        md.push_str(&self.ai_explanation);
        md.push_str("\n\n");

        if !self.quick_fixes.is_empty() {
            md.push_str("## Quick Fixes\n\n");
            for fix in &self.quick_fixes {
                md.push_str(&format!("- {}", fix.description));
                if let Some(cmd) = &fix.command {
                    md.push_str(&format!(" (`{}`)", cmd));
                }
                md.push('\n');
            }
            md.push('\n');
        }

        if !self.documentation_links.is_empty() {
            md.push_str("## Documentation\n\n");
            for link in &self.documentation_links {
                md.push_str(&format!("- [{}]({})\n", link.title, link.url));
            }
            md.push('\n');
        }

        if !self.related_commands.is_empty() {
            md.push_str("## Related Commands\n\n");
            for cmd in &self.related_commands {
                md.push_str(&format!("- `{}`\n", cmd));
            }
        }

        md
    }
}

#[derive(Debug, Clone)]
pub struct QuickFix {
    pub description: String,
    pub command: Option<String>,
    pub automatic: bool,
}

#[derive(Debug, Clone)]
pub struct DocumentationLink {
    pub title: String,
    pub url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> AiConfig {
        AiConfig::default()
    }

    #[test]
    fn test_error_explainer_creation() {
        let config = create_test_config();
        let explainer = ErrorExplainer::new(config);
        assert!(explainer.detect_error("test").is_none());
    }

    #[test]
    fn test_detect_error() {
        let config = create_test_config();
        let explainer = ErrorExplainer::new(config);

        let error = explainer.detect_error("bash: foo: command not found");
        assert!(error.is_some());

        let error = error.unwrap();
        assert_eq!(error.error_type, ErrorType::CommandNotFound);
    }

    #[test]
    fn test_detect_all_errors() {
        let config = create_test_config();
        let explainer = ErrorExplainer::new(config);

        let output = r#"
bash: foo: command not found
permission denied: /root/file
"#;

        let errors = explainer.detect_all_errors(output);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_quick_fixes_generation() {
        let config = create_test_config();
        let explainer = ErrorExplainer::new(config);

        let detected = DetectedError {
            error_type: ErrorType::CommandNotFound,
            message: "foo: command not found".to_string(),
            context: vec![],
            line_number: Some(0),
            severity: crate::ai::error_detection::ErrorSeverity::High,
            suggestions: vec![],
            related_files: vec![],
        };

        let fixes = explainer.generate_quick_fixes(&detected);
        assert!(!fixes.is_empty());
    }

    #[test]
    fn test_error_explanation_to_markdown() {
        let detected = DetectedError {
            error_type: ErrorType::CommandNotFound,
            message: "foo: command not found".to_string(),
            context: vec![],
            line_number: Some(0),
            severity: crate::ai::error_detection::ErrorSeverity::High,
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
            documentation_links: vec![],
            related_commands: vec!["which".to_string()],
        };

        let md = explanation.to_markdown();
        assert!(md.contains("Command Not Found"));
        assert!(md.contains("AI Explanation"));
        assert!(md.contains("Quick Fixes"));
    }

    #[tokio::test]
    async fn test_explainer_disabled() {
        let config = create_test_config();
        let explainer = ErrorExplainer::new(config);
        explainer.set_enabled(false).await;

        let detected = DetectedError {
            error_type: ErrorType::CommandNotFound,
            message: "foo: command not found".to_string(),
            context: vec![],
            line_number: Some(0),
            severity: crate::ai::error_detection::ErrorSeverity::High,
            suggestions: vec![],
            related_files: vec![],
        };

        let context = TerminalContext::default();
        let result = explainer.explain(&detected, &context).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_documentation_links() {
        let config = create_test_config();
        let explainer = ErrorExplainer::new(config);

        let detected = DetectedError {
            error_type: ErrorType::RustError,
            message: "error[E0433]".to_string(),
            context: vec![],
            line_number: Some(0),
            severity: crate::ai::error_detection::ErrorSeverity::High,
            suggestions: vec![],
            related_files: vec![],
        };

        let links = explainer.get_documentation_links(&detected);
        assert!(!links.is_empty());
        assert!(links.iter().any(|l| l.title.contains("Rust")));
    }

    #[test]
    fn test_related_commands() {
        let config = create_test_config();
        let explainer = ErrorExplainer::new(config);

        let detected = DetectedError {
            error_type: ErrorType::GitError,
            message: "fatal: not a git repository".to_string(),
            context: vec![],
            line_number: Some(0),
            severity: crate::ai::error_detection::ErrorSeverity::High,
            suggestions: vec![],
            related_files: vec![],
        };

        let commands = explainer.get_related_commands(&detected);
        assert!(!commands.is_empty());
        assert!(commands.contains(&"git status".to_string()));
    }

    use crate::ai::error_detection::ErrorSeverity;
    use crate::ai::test_support::FakeProvider;

    fn err_of(error_type: ErrorType, message: &str) -> DetectedError {
        DetectedError {
            error_type,
            message: message.to_string(),
            context: vec![],
            line_number: None,
            severity: ErrorSeverity::Medium,
            suggestions: vec![],
            related_files: vec![],
        }
    }

    #[tokio::test]
    async fn test_explain_requires_configured_provider() {
        let e = ErrorExplainer::new(create_test_config());
        assert!(e.is_enabled().await);
        let ctx = TerminalContext::default();
        let err = err_of(ErrorType::GitError, "fatal");
        assert!(matches!(
            e.explain(&err, &ctx).await,
            Err(AiError::ProviderNotAvailable(_))
        ));

        let mut fake = FakeProvider::new("x");
        fake.configured = false;
        let e = e.with_provider(Box::new(fake));
        assert!(matches!(
            e.explain(&err, &ctx).await,
            Err(AiError::ConfigError(m)) if m.contains("not properly configured")
        ));
    }

    #[tokio::test]
    async fn test_explain_combines_ai_text_with_local_hints() {
        let fake = FakeProvider::new("O comando não existe.");
        let calls = fake.calls.clone();
        let e = ErrorExplainer::new(create_test_config()).with_provider(Box::new(fake));
        assert!(!e.config().enabled);

        let exp = e
            .explain(
                &err_of(ErrorType::CommandNotFound, "foo"),
                &TerminalContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(exp.ai_explanation, "O comando não existe.");
        assert_eq!(
            exp.quick_fixes[0].command.as_deref(),
            Some("sudo apt install foo")
        );
        assert_eq!(exp.documentation_links[0].url, "https://linux.die.net/");
        assert_eq!(exp.related_commands, vec!["which", "type", "whereis"]);
        assert_eq!(*calls.lock().unwrap(), vec!["explain:foo"]);

        let md = exp.to_markdown();
        assert!(md.starts_with("# Command Not Found Error\n\n"));
        assert!(md.contains("- Instalar foo (`sudo apt install foo`)\n"));
        assert!(md
            .contains("## Documentation\n\n- [Linux Command Not Found](https://linux.die.net/)\n"));
        assert!(md.contains("## Related Commands\n\n- `which`\n"));
    }

    #[tokio::test]
    async fn test_explain_from_output_and_provider_errors() {
        let e = ErrorExplainer::new(create_test_config())
            .with_detector(ErrorDetector::new().with_context_lines(0));
        e.set_provider(Box::new(FakeProvider::new("ok"))).await;
        let ctx = TerminalContext::default();
        assert!(e
            .explain_from_output("tudo certo", &ctx)
            .await
            .unwrap()
            .is_none());
        let exp = e
            .explain_from_output("bash: foo: command not found", &ctx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(exp.error.error_type, ErrorType::CommandNotFound);
        assert_eq!(exp.error.context.len(), 1);

        let mut failing = FakeProvider::new("x");
        failing.fail = true;
        e.set_provider(Box::new(failing)).await;
        assert!(matches!(
            e.explain_from_output("bash: foo: command not found", &ctx)
                .await,
            Err(AiError::ApiError(_))
        ));
    }

    #[test]
    fn test_local_hints_for_every_error_type() {
        let e = ErrorExplainer::new(create_test_config());
        // (tipo, 1º comando de quick fix, nº de links, 1º comando relacionado)
        let cases: Vec<(ErrorType, Option<&str>, usize, Option<&str>)> = vec![
            (
                ErrorType::PermissionDenied,
                Some("chmod +x {}"),
                0,
                Some("chmod"),
            ),
            (
                ErrorType::FileNotFoundError,
                Some("ls -la"),
                0,
                Some("find"),
            ),
            (
                ErrorType::GitError,
                Some("git status"),
                1,
                Some("git status"),
            ),
            (
                ErrorType::NpmError,
                Some("npm install"),
                1,
                Some("npm list"),
            ),
            (
                ErrorType::PythonError,
                Some("pip install -r requirements.txt"),
                2,
                Some("pip list"),
            ),
            (
                ErrorType::RustError,
                Some("cargo clean && cargo build"),
                2,
                Some("cargo check"),
            ),
            (
                ErrorType::DockerError,
                Some("systemctl status docker"),
                1,
                Some("docker ps"),
            ),
            (ErrorType::SyntaxError, None, 0, None),
            (ErrorType::Unknown, None, 0, None),
        ];
        for (ty, fix, links, related) in cases {
            let err = err_of(ty, "msg");
            let fixes = e.generate_quick_fixes(&err);
            assert_eq!(
                fixes.first().and_then(|f| f.command.as_deref()),
                fix,
                "{ty:?}"
            );
            if fix.is_some() {
                assert_eq!(fixes.len(), 2, "{ty:?}");
                assert!(fixes.iter().all(|f| !f.automatic));
            }
            assert_eq!(e.get_documentation_links(&err).len(), links, "{ty:?}");
            let rel = e.get_related_commands(&err);
            assert_eq!(rel.first().map(String::as_str), related, "{ty:?}");
        }
        // CommandNotFound sem mensagem: sem quick fix
        assert!(e
            .generate_quick_fixes(&err_of(ErrorType::CommandNotFound, ""))
            .is_empty());
    }

    #[test]
    fn test_markdown_omits_empty_sections() {
        let exp = ErrorExplanation {
            error: err_of(ErrorType::Unknown, "algo"),
            ai_explanation: "texto".into(),
            quick_fixes: vec![QuickFix {
                description: "manual".into(),
                command: None,
                automatic: true,
            }],
            documentation_links: vec![],
            related_commands: vec![],
        };
        let md = exp.to_markdown();
        assert!(md.contains("**Message:** algo"));
        assert!(md.contains("- manual\n"));
        assert!(!md.contains("Documentation"));
        assert!(!md.contains("Related Commands"));
    }
}
