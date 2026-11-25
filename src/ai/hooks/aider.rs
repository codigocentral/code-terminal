//! Hooks para Aider (Open source AI pair programming)

use super::{
    api::{AgentCapabilities, AgentCommand, AgentRequest, AgentResponse, ResponseData},
    error::{HookError, HookErrorExt, HookResult},
    registry::{AgentRegistration, HookHandler},
    types::{AgentInfo, AgentType},
};
use serde::{Deserialize, Serialize};

use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::{debug, info, instrument};

/// Hook para Aider
#[derive(Debug)]
pub struct AiderHook {
    /// Configuração
    config: AiderConfig,
    /// Estado
    state: AiderState,
}

/// Configuração do Aider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiderConfig {
    /// Caminho para executável aider
    pub executable_path: String,
    /// Modelo principal
    pub model: String,
    /// Modelo de edição (opcional)
    pub editor_model: Option<String>,
    /// Weak model para tarefas simples
    pub weak_model: Option<String>,
    /// API key OpenAI (se usar OpenAI)
    pub openai_api_key: Option<String>,
    /// API key Anthropic (se usar Claude)
    pub anthropic_api_key: Option<String>,
    /// Auto-commit
    pub auto_commits: bool,
    /// Dirty commits
    pub dirty_commits: bool,
    /// Watch files
    pub watch_files: bool,
    /// Lint cmd
    pub lint_cmd: Option<String>,
    /// Test cmd
    pub test_cmd: Option<String>,
    /// Timeout padrão
    pub timeout_secs: u64,
    /// Diretório de trabalho
    pub working_dir: Option<String>,
}

impl Default for AiderConfig {
    fn default() -> Self {
        Self {
            executable_path: "aider".to_string(),
            model: "sonnet".to_string(),
            editor_model: None,
            weak_model: None,
            openai_api_key: None,
            anthropic_api_key: None,
            auto_commits: true,
            dirty_commits: false,
            watch_files: false,
            lint_cmd: None,
            test_cmd: None,
            timeout_secs: 300,
            working_dir: None,
        }
    }
}

/// Estado do Aider
#[derive(Debug, Clone, Default)]
pub struct AiderState {
    /// Em sessão ativa
    in_session: bool,
    /// Arquivos no contexto
    context_files: Vec<String>,
    /// Mensagens na sessão
    session_messages: Vec<String>,
    /// Última atividade
    last_activity: Option<std::time::SystemTime>,
    /// Total de commits feitos
    total_commits: u32,
}

impl AiderHook {
    /// Cria um novo hook
    pub fn new(config: AiderConfig) -> Self {
        Self {
            config,
            state: AiderState::default(),
        }
    }

    /// Cria com configuração padrão
    pub fn default() -> Self {
        Self::new(AiderConfig::default())
    }

    /// Detecta se Aider está instalado
    pub fn detect() -> bool {
        AgentType::Aider.detect()
    }

    /// Obtém versão do Aider
    pub async fn get_version(&self) -> HookResult<String> {
        let output = Command::new(&self.config.executable_path)
            .arg("--version")
            .output()
            .await
            .map_err(|e| HookError::AgentError {
                agent: "aider".to_string(),
                message: format!("Failed to get version: {}", e),
            })?;

        if !output.status.success() {
            return Err(HookError::AgentError {
                agent: "aider".to_string(),
                message: "aider --version failed".to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Executa comando Aider
    #[instrument(skip(self))]
    pub async fn execute(&self, args: &[&str]) -> HookResult<String> {
        let mut cmd = Command::new(&self.config.executable_path);
        cmd.args(args);

        if let Some(dir) = &self.config.working_dir {
            cmd.current_dir(dir);
        }

        // Configurar variáveis de ambiente
        if let Some(key) = &self.config.openai_api_key {
            cmd.env("OPENAI_API_KEY", key);
        }
        if let Some(key) = &self.config.anthropic_api_key {
            cmd.env("ANTHROPIC_API_KEY", key);
        }

        debug!("Executing: {:?}", cmd);

        let output = timeout(Duration::from_secs(self.config.timeout_secs), cmd.output())
            .await
            .map_err(|_| HookError::timeout(self.config.timeout_secs * 1000))?
            .map_err(|e| HookError::AgentError {
                agent: "aider".to_string(),
                message: format!("Execution failed: {}", e),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Aider pode escrever em stderr mesmo em sucesso
        let combined = format!("{}{}", stdout, stderr);

        Ok(combined)
    }

    /// Adiciona arquivo ao contexto
    #[instrument(skip(self))]
    pub async fn add_file(&mut self, file_path: &str) -> HookResult<()> {
        info!("Adding file to aider context: {}", file_path);

        let output = self.execute(&["--add", file_path]).await?;

        if output.contains("error") || output.contains("Error") {
            return Err(HookError::AgentError {
                agent: "aider".to_string(),
                message: format!("Failed to add file: {}", output),
            });
        }

        self.state.context_files.push(file_path.to_string());
        self.state.last_activity = Some(std::time::SystemTime::now());

        Ok(())
    }

    /// Remove arquivo do contexto
    #[instrument(skip(self))]
    pub async fn drop_file(&mut self, file_path: &str) -> HookResult<()> {
        info!("Dropping file from aider context: {}", file_path);

        let _output = self.execute(&["--drop", file_path]).await?;

        self.state.context_files.retain(|f| f != file_path);
        self.state.last_activity = Some(std::time::SystemTime::now());

        Ok(())
    }

    /// Envia mensagem para Aider
    #[instrument(skip(self, message))]
    pub async fn send_message(&mut self, message: &str) -> HookResult<String> {
        info!("Sending message to aider");

        let args = vec!["--message", message];
        let output = self.execute(&args).await?;

        self.state.session_messages.push(message.to_string());
        self.state.last_activity = Some(std::time::SystemTime::now());

        // Parsear resultado
        if output.contains("Applied edit") || output.contains(" commit ") {
            self.state.total_commits += 1;
        }

        Ok(output)
    }

    /// Solicita mudança no código
    #[instrument(skip(self, instructions))]
    pub async fn code_change(&mut self, instructions: &str) -> HookResult<String> {
        info!("Requesting code change from aider");
        self.send_message(instructions).await
    }

    /// Undo última mudança
    #[instrument(skip(self))]
    pub async fn undo(&self) -> HookResult<String> {
        info!("Undoing last aider change");
        self.execute(&["--undo"]).await
    }

    /// Limpa histórico
    #[instrument(skip(self))]
    pub async fn clear_history(&mut self) -> HookResult<String> {
        info!("Clearing aider history");
        self.state.session_messages.clear();
        self.execute(&["--clear"]).await
    }

    /// Mostra arquivos no contexto
    pub async fn ls(&self) -> HookResult<String> {
        self.execute(&["--ls"]).await
    }

    /// Roda lint
    #[instrument(skip(self))]
    pub async fn lint(&self) -> HookResult<String> {
        if let Some(lint_cmd) = &self.config.lint_cmd {
            self.execute(&["--lint", lint_cmd]).await
        } else {
            self.execute(&["--lint"]).await
        }
    }

    /// Roda testes
    #[instrument(skip(self))]
    pub async fn test(&self) -> HookResult<String> {
        if let Some(test_cmd) = &self.config.test_cmd {
            self.execute(&["--test", test_cmd]).await
        } else {
            self.execute(&["--test"]).await
        }
    }

    /// Gera commit message
    #[instrument(skip(self))]
    pub async fn commit(&self) -> HookResult<String> {
        info!("Generating commit with aider");
        self.execute(&["--commit"]).await
    }

    /// Cria agent registration
    pub async fn create_registration(&self) -> HookResult<AgentRegistration> {
        let version = self.get_version().await.ok();

        let info = AgentInfo::new(AgentType::Aider, "Aider")
            .with_version(version.unwrap_or_else(|| "unknown".to_string()))
            .with_capability("code_edit")
            .with_capability("file_add")
            .with_capability("file_drop")
            .with_capability("undo")
            .with_capability("git_commit")
            .with_capability("lint")
            .with_capability("test")
            .with_metadata("model", self.config.model.clone())
            .with_metadata("auto_commits", self.config.auto_commits);

        Ok(AgentRegistration::new(info))
    }

    /// Processa uma requisição
    #[instrument(skip(self, request))]
    pub async fn process_request(&mut self, request: AgentRequest) -> HookResult<AgentResponse> {
        let start = std::time::Instant::now();

        let result = match request.command {
            AgentCommand::ExecuteCommand { command, args, .. } => {
                let full_cmd = format!("{} {}", command, args.join(" "));
                let output = self.send_message(&full_cmd).await?;
                Ok(ResponseData::ExecutionResult {
                    stdout: output,
                    stderr: String::new(),
                    exit_code: 0,
                })
            }
            AgentCommand::EditFile { path, instructions } => {
                self.add_file(&path).await?;
                let result = self.code_change(&instructions).await?;
                Ok(ResponseData::Text { text: result })
            }
            AgentCommand::ReadFile { path, .. } => {
                let content = tokio::fs::read_to_string(&path).await.map_err(|e| {
                    HookError::internal_error(format!("Failed to read file: {}", e))
                })?;
                Ok(ResponseData::FileContent {
                    path,
                    content,
                    encoding: "utf-8".to_string(),
                })
            }
            AgentCommand::GitCommit { message: _, files } => {
                for file in files {
                    self.add_file(&file).await?;
                }
                let result = self.commit().await?;
                Ok(ResponseData::Text { text: result })
            }
            AgentCommand::Ping => Ok(ResponseData::Pong {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            }),
            AgentCommand::GetCapabilities => Ok(ResponseData::Capabilities {
                capabilities: vec![
                    super::api::AgentCapability {
                        name: "code_edit".to_string(),
                        description: "Edit code files with AI".to_string(),
                        version: "1.0".to_string(),
                        requires_permission: true,
                    },
                    super::api::AgentCapability {
                        name: "git_commit".to_string(),
                        description: "Generate commit messages".to_string(),
                        version: "1.0".to_string(),
                        requires_permission: false,
                    },
                ],
                version: "1.0".to_string(),
            }),
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
            can_edit_files: true,
            can_analyze_code: true,
            can_generate_tests: self.config.test_cmd.is_some(),
            can_refactor: true,
            supports_streaming: false,
            supports_files: true,
            max_file_size: Some(10 * 1024 * 1024), // 10MB
            supported_languages: vec![
                "rust".to_string(),
                "python".to_string(),
                "javascript".to_string(),
                "typescript".to_string(),
                "go".to_string(),
                "java".to_string(),
                "cpp".to_string(),
                "c".to_string(),
                "ruby".to_string(),
                "php".to_string(),
            ],
        }
    }

    /// Cria handler de hook
    pub fn create_handler(&self) -> HookHandler {
        HookHandler::new(|event, _context| {
            info!("Aider hook received event: {:?}", event.event_type);
            Ok(())
        })
    }

    /// Retorna arquivos no contexto
    pub fn context_files(&self) -> &[String] {
        &self.state.context_files
    }

    /// Retorna se está em sessão
    pub fn in_session(&self) -> bool {
        self.state.in_session
    }

    /// Retorna estatísticas
    pub fn stats(&self) -> &AiderState {
        &self.state
    }
}

/// Builder para configuração Aider
pub struct AiderConfigBuilder {
    config: AiderConfig,
}

impl AiderConfigBuilder {
    /// Cria novo builder
    pub fn new() -> Self {
        Self {
            config: AiderConfig::default(),
        }
    }

    /// Define modelo
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.config.model = model.into();
        self
    }

    /// Define editor model
    pub fn editor_model(mut self, model: impl Into<String>) -> Self {
        self.config.editor_model = Some(model.into());
        self
    }

    /// Define weak model
    pub fn weak_model(mut self, model: impl Into<String>) -> Self {
        self.config.weak_model = Some(model.into());
        self
    }

    /// Define API key OpenAI
    pub fn openai_key(mut self, key: impl Into<String>) -> Self {
        self.config.openai_api_key = Some(key.into());
        self
    }

    /// Define API key Anthropic
    pub fn anthropic_key(mut self, key: impl Into<String>) -> Self {
        self.config.anthropic_api_key = Some(key.into());
        self
    }

    /// Habilita auto-commits
    pub fn auto_commits(mut self, enable: bool) -> Self {
        self.config.auto_commits = enable;
        self
    }

    /// Define lint command
    pub fn lint_cmd(mut self, cmd: impl Into<String>) -> Self {
        self.config.lint_cmd = Some(cmd.into());
        self
    }

    /// Define test command
    pub fn test_cmd(mut self, cmd: impl Into<String>) -> Self {
        self.config.test_cmd = Some(cmd.into());
        self
    }

    /// Define timeout
    pub fn timeout(mut self, secs: u64) -> Self {
        self.config.timeout_secs = secs;
        self
    }

    /// Define diretório de trabalho
    pub fn working_dir(mut self, dir: impl Into<String>) -> Self {
        self.config.working_dir = Some(dir.into());
        self
    }

    /// Build
    pub fn build(self) -> AiderConfig {
        self.config
    }
}

impl Default for AiderConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aider_config_default() {
        let config = AiderConfig::default();
        assert_eq!(config.executable_path, "aider");
        assert_eq!(config.model, "sonnet");
        assert!(config.auto_commits);
    }

    #[test]
    fn test_aider_config_builder() {
        let config = AiderConfigBuilder::new()
            .model("gpt-4")
            .auto_commits(false)
            .timeout(600)
            .build();

        assert_eq!(config.model, "gpt-4");
        assert!(!config.auto_commits);
        assert_eq!(config.timeout_secs, 600);
    }

    #[test]
    fn test_aider_hook_creation() {
        let hook = AiderHook::default();
        assert_eq!(hook.config.model, "sonnet");
        assert!(!hook.in_session());
        assert!(hook.context_files().is_empty());
    }

    #[test]
    fn test_capabilities() {
        let hook = AiderHook::default();
        let caps = hook.capabilities();

        assert!(caps.can_edit_files);
        assert!(caps.can_refactor);
        assert!(!caps.can_execute_commands);
        assert!(caps.supports_files);
    }

    #[tokio::test]
    async fn test_create_registration() {
        let hook = AiderHook::default();
        let reg = hook.create_registration().await.unwrap();

        assert_eq!(reg.info.agent_type, AgentType::Aider);
        assert_eq!(reg.info.name, "Aider");
        assert!(reg.info.capabilities.contains(&"code_edit".to_string()));
        assert!(reg.info.capabilities.contains(&"git_commit".to_string()));
    }

    // Testes que requerem Aider instalado são ignorados
    #[tokio::test]
    #[ignore = "Requires Aider installation"]
    async fn test_detect() {
        let detected = AiderHook::detect();
        info!(detected, "Aider detection completed");
    }

    #[tokio::test]
    #[ignore = "Requires Aider installation"]
    async fn test_get_version() {
        let hook = AiderHook::default();
        let version = hook.get_version().await.unwrap();
        assert!(!version.is_empty());
        info!(version, "Aider version detected");
    }

    #[cfg(unix)]
    mod with_fake_cli {
        use super::*;
        use crate::ai::test_support::{fake_cli, fake_cli_calls};
        use tempfile::TempDir;

        const BODY: &str = r#"
case "$1" in
  --version) echo "aider 0.86.1" ;;
  --add) case "$2" in *ruim*) echo "Error: arquivo inexistente" ;; *) echo "Added $2" ;; esac ;;
  --message) case "$2" in env) echo "$OPENAI_API_KEY|$ANTHROPIC_API_KEY|$(pwd)" ;; *) echo "Applied edit to main.rs" ;; esac ;;
  *) echo "ok $*"; echo "aviso" >&2 ;;
esac"#;

        fn hook(dir: &TempDir, builder: AiderConfigBuilder) -> (AiderHook, String) {
            let prog = fake_cli(dir, "aider", BODY);
            let mut config = builder.working_dir(dir.path().to_string_lossy()).build();
            config.executable_path = prog.clone();
            (AiderHook::new(config), prog)
        }

        #[tokio::test]
        async fn test_files_messages_and_commands() {
            let dir = TempDir::new().unwrap();
            let (mut h, prog) = hook(
                &dir,
                AiderConfigBuilder::new()
                    .openai_key("sk-o")
                    .anthropic_key("sk-a")
                    .editor_model("e")
                    .weak_model("w"),
            );
            assert_eq!(h.config.editor_model.as_deref(), Some("e"));
            assert_eq!(h.config.weak_model.as_deref(), Some("w"));
            assert_eq!(h.get_version().await.unwrap(), "aider 0.86.1");

            h.add_file("src/main.rs").await.unwrap();
            h.add_file("src/lib.rs").await.unwrap();
            assert_eq!(h.context_files(), ["src/main.rs", "src/lib.rs"]);
            assert!(matches!(
                h.add_file("ruim.rs").await,
                Err(HookError::AgentError { message, .. }) if message.contains("arquivo inexistente")
            ));
            h.drop_file("src/main.rs").await.unwrap();
            assert_eq!(h.context_files(), ["src/lib.rs"]);

            let out = h.send_message("refatorar").await.unwrap();
            assert!(out.contains("Applied edit"));
            h.code_change("mais").await.unwrap();
            assert_eq!(h.stats().total_commits, 2);
            assert_eq!(h.stats().session_messages.len(), 2);
            assert!(h.stats().last_activity.is_some());

            // chaves vão por variável de ambiente e o diretório é respeitado
            let env = h.send_message("env").await.unwrap();
            let parts: Vec<&str> = env.trim().split('|').collect();
            assert_eq!(&parts[..2], ["sk-o", "sk-a"]);
            assert_eq!(
                std::fs::canonicalize(parts[2]).unwrap(),
                std::fs::canonicalize(dir.path()).unwrap()
            );

            // stdout e stderr são combinados
            assert_eq!(h.undo().await.unwrap(), "ok --undo\naviso\n");
            h.clear_history().await.unwrap();
            assert!(h.stats().session_messages.is_empty());
            h.ls().await.unwrap();
            h.lint().await.unwrap();
            h.test().await.unwrap();
            h.commit().await.unwrap();

            let calls = fake_cli_calls(&prog);
            let firsts: Vec<&str> = calls.iter().map(|c| c[0].as_str()).collect();
            assert_eq!(
                firsts,
                vec![
                    "--version",
                    "--add",
                    "--add",
                    "--add",
                    "--drop",
                    "--message",
                    "--message",
                    "--message",
                    "--undo",
                    "--clear",
                    "--ls",
                    "--lint",
                    "--test",
                    "--commit"
                ]
            );
            assert_eq!(calls[11], vec!["--lint"]);
        }

        #[tokio::test]
        async fn test_lint_and_test_commands_are_forwarded() {
            let dir = TempDir::new().unwrap();
            let (h, prog) = hook(
                &dir,
                AiderConfigBuilder::new()
                    .lint_cmd("cargo clippy")
                    .test_cmd("cargo test"),
            );
            assert!(h.capabilities().can_generate_tests);
            h.lint().await.unwrap();
            h.test().await.unwrap();
            let calls = fake_cli_calls(&prog);
            assert_eq!(calls[0], vec!["--lint", "cargo clippy"]);
            assert_eq!(calls[1], vec!["--test", "cargo test"]);
        }

        #[tokio::test]
        async fn test_process_request_variants() {
            let dir = TempDir::new().unwrap();
            let (mut h, prog) = hook(&dir, AiderConfigBuilder::new());

            let r = h
                .process_request(AgentRequest::new(AgentCommand::ExecuteCommand {
                    command: "explique".into(),
                    args: vec!["isto".into()],
                    cwd: None,
                }))
                .await
                .unwrap();
            assert!(r.success);
            assert!(matches!(
                r.data,
                ResponseData::ExecutionResult { exit_code: 0, .. }
            ));
            assert_eq!(fake_cli_calls(&prog)[0], vec!["--message", "explique isto"]);

            let r = h
                .process_request(AgentRequest::new(AgentCommand::EditFile {
                    path: "a.rs".into(),
                    instructions: "x".into(),
                }))
                .await
                .unwrap();
            assert!(matches!(r.data, ResponseData::Text { .. }));
            assert_eq!(h.context_files(), ["a.rs"]);

            let file = dir.path().join("leia.txt");
            std::fs::write(&file, "conteúdo").unwrap();
            let r = h
                .process_request(AgentRequest::new(AgentCommand::ReadFile {
                    path: file.to_string_lossy().to_string(),
                    offset: None,
                    limit: None,
                }))
                .await
                .unwrap();
            assert!(
                matches!(r.data, ResponseData::FileContent { ref content, .. } if content == "conteúdo")
            );
            assert!(h
                .process_request(AgentRequest::new(AgentCommand::ReadFile {
                    path: "/nao/existe".into(),
                    offset: None,
                    limit: None,
                }))
                .await
                .is_err());

            let r = h
                .process_request(AgentRequest::new(AgentCommand::GitCommit {
                    message: "m".into(),
                    files: vec!["b.rs".into()],
                }))
                .await
                .unwrap();
            assert!(matches!(r.data, ResponseData::Text { ref text } if text.contains("--commit")));

            let r = h
                .process_request(AgentRequest::new(AgentCommand::Ping))
                .await
                .unwrap();
            assert!(matches!(r.data, ResponseData::Pong { .. }));
            let r = h
                .process_request(AgentRequest::new(AgentCommand::GetCapabilities))
                .await
                .unwrap();
            assert!(
                matches!(r.data, ResponseData::Capabilities { ref capabilities, .. } if capabilities.len() == 2)
            );
            let r = h
                .process_request(AgentRequest::new(AgentCommand::Refactor {
                    path: "x".into(),
                    instructions: "y".into(),
                }))
                .await
                .unwrap();
            assert!(!r.success);
            assert!(
                matches!(r.data, ResponseData::Error { ref message, .. } if message.contains("Refactor"))
            );
        }

        #[tokio::test]
        async fn test_execute_failures() {
            let dir = TempDir::new().unwrap();
            let prog = fake_cli(&dir, "lento", "sleep 5");
            let h = AiderHook::new(AiderConfig {
                executable_path: prog,
                timeout_secs: 0,
                ..Default::default()
            });
            assert!(matches!(
                h.execute(&["x"]).await,
                Err(HookError::Timeout { .. })
            ));

            let bad = fake_cli(&dir, "falho", "exit 1");
            let h = AiderHook::new(AiderConfig {
                executable_path: bad,
                ..Default::default()
            });
            assert!(matches!(
                h.get_version().await,
                Err(HookError::AgentError { message, .. }) if message == "aider --version failed"
            ));
            let h = AiderHook::new(AiderConfig {
                executable_path: "/nao/existe/aider".into(),
                ..Default::default()
            });
            assert!(h.get_version().await.is_err());
            assert!(
                matches!(h.execute(&[]).await, Err(HookError::AgentError { message, .. }) if message.starts_with("Execution failed"))
            );
            assert!(!h.in_session());
            h.create_handler()
                .handle(
                    &crate::ai::hooks::event::AgentEvent::new(
                        crate::ai::hooks::event::EventType::TerminalStart,
                    ),
                    &crate::ai::hooks::context::AgentContext::new("s", 0),
                )
                .unwrap();
            let _ = AiderHook::detect();
        }
    }
}
