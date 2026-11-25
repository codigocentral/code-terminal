//! Hooks para Claude Code (Anthropic)

use super::{
    api::{AgentCapabilities, AgentCommand, AgentRequest, AgentResponse, ResponseData},
    error::{HookError, HookResult},
    registry::{AgentRegistration, HookHandler},
    types::{AgentInfo, AgentType},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::{debug, info, instrument};

/// Hook para Claude Code
#[derive(Debug)]
pub struct ClaudeCodeHook {
    /// Configuração
    config: ClaudeCodeConfig,
    /// Estado interno
    state: ClaudeCodeState,
}

/// Configuração do Claude Code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeConfig {
    /// Caminho para o executável claude
    pub executable_path: String,
    /// API key (opcional, pode usar env)
    pub api_key: Option<String>,
    /// Modelo a usar
    pub model: String,
    /// Habilitar ferramentas
    pub enable_tools: bool,
    /// Modo verbose
    pub verbose: bool,
    /// Timeout padrão
    pub timeout_secs: u64,
    /// Diretório de trabalho
    pub working_dir: Option<String>,
    /// Args adicionais
    pub extra_args: Vec<String>,
}

impl Default for ClaudeCodeConfig {
    fn default() -> Self {
        Self {
            executable_path: "claude".to_string(),
            api_key: None,
            model: "claude-sonnet-4-20250514".to_string(),
            enable_tools: true,
            verbose: false,
            timeout_secs: 120,
            working_dir: None,
            extra_args: Vec::new(),
        }
    }
}

/// Estado do Claude Code
#[derive(Debug, Clone, Default)]
struct ClaudeCodeState {
    /// Processo em execução
    process_running: bool,
    /// Última atividade
    last_activity: Option<std::time::SystemTime>,
    /// Comandos pendentes
    pending_commands: Vec<String>,
    /// Respostas cacheadas
    response_cache: HashMap<String, String>,
}

/// Snapshot público do estado do Claude Code.
#[derive(Debug, Clone, Default)]
pub struct ClaudeCodeStateSnapshot {
    pub process_running: bool,
    pub last_activity: Option<std::time::SystemTime>,
    pub pending_command_count: usize,
    pub cached_response_count: usize,
}

impl ClaudeCodeHook {
    /// Cria um novo hook
    pub fn new(config: ClaudeCodeConfig) -> Self {
        Self {
            config,
            state: ClaudeCodeState::default(),
        }
    }

    /// Cria com configuração padrão
    pub fn default() -> Self {
        Self::new(ClaudeCodeConfig::default())
    }

    /// Detecta se Claude Code está instalado
    pub fn detect() -> bool {
        AgentType::ClaudeCode.detect()
    }

    /// Retorna um snapshot do estado interno.
    pub fn state_snapshot(&self) -> ClaudeCodeStateSnapshot {
        ClaudeCodeStateSnapshot {
            process_running: self.state.process_running,
            last_activity: self.state.last_activity,
            pending_command_count: self.state.pending_commands.len(),
            cached_response_count: self.state.response_cache.len(),
        }
    }

    /// Obtém versão do Claude Code
    pub async fn get_version(&self) -> HookResult<String> {
        let output = Command::new(&self.config.executable_path)
            .arg("--version")
            .output()
            .await
            .map_err(|e| HookError::AgentError {
                agent: "claude-code".to_string(),
                message: format!("Failed to get version: {}", e),
            })?;

        if !output.status.success() {
            return Err(HookError::AgentError {
                agent: "claude-code".to_string(),
                message: "claude --version failed".to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Executa comando Claude Code
    #[instrument(skip(self))]
    pub async fn execute(&self, args: &[&str]) -> HookResult<String> {
        let mut cmd = Command::new(&self.config.executable_path);

        cmd.args(args);

        if let Some(dir) = &self.config.working_dir {
            cmd.current_dir(dir);
        }

        if self.config.verbose {
            cmd.arg("--verbose");
        }

        cmd.args(&self.config.extra_args);

        debug!("Executing: {:?}", cmd);

        let output = timeout(Duration::from_secs(self.config.timeout_secs), cmd.output())
            .await
            .map_err(|_| HookError::timeout(self.config.timeout_secs * 1000))?
            .map_err(|e| HookError::AgentError {
                agent: "claude-code".to_string(),
                message: format!("Execution failed: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(HookError::AgentError {
                agent: "claude-code".to_string(),
                message: format!("Command failed: {}", stderr),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Envia prompt para Claude Code
    #[instrument(skip(self, prompt))]
    pub async fn send_prompt(&self, prompt: &str) -> HookResult<String> {
        // Usar modo de uma única resposta
        let args = vec!["--prompt", prompt, "--output-format", "json"];
        self.execute(&args).await
    }

    /// Solicita edição de arquivo
    #[instrument(skip(self, instructions))]
    pub async fn edit_file(&self, file_path: &str, instructions: &str) -> HookResult<String> {
        let prompt = format!(
            "Edit the file {} according to these instructions: {}",
            file_path, instructions
        );
        self.send_prompt(&prompt).await
    }

    /// Analisa código
    #[instrument(skip(self))]
    pub async fn analyze_code(&self, file_path: &str) -> HookResult<String> {
        let prompt = format!(
            "Analyze the code in {} and provide a summary of potential issues, improvements, and best practices",
            file_path
        );
        self.send_prompt(&prompt).await
    }

    /// Gera testes
    #[instrument(skip(self))]
    pub async fn generate_tests(&self, file_path: &str) -> HookResult<String> {
        let prompt = format!(
            "Generate comprehensive unit tests for the code in {}",
            file_path
        );
        self.send_prompt(&prompt).await
    }

    /// Explica erro
    #[instrument(skip(self, error_output))]
    pub async fn explain_error(&self, command: &str, error_output: &str) -> HookResult<String> {
        let prompt = format!(
            "The command '{}' failed with this error:\n{}\n\nExplain what went wrong and how to fix it.",
            command, error_output
        );
        self.send_prompt(&prompt).await
    }

    /// Cria agent registration
    pub async fn create_registration(&self) -> HookResult<AgentRegistration> {
        let version = self.get_version().await.ok();

        let info = AgentInfo::new(AgentType::ClaudeCode, "Claude Code")
            .with_version(version.unwrap_or_else(|| "unknown".to_string()))
            .with_capability("code_edit")
            .with_capability("file_read")
            .with_capability("file_write")
            .with_capability("command_execute")
            .with_capability("error_explain")
            .with_capability("code_analyze")
            .with_capability("test_generate")
            .with_metadata("model", self.config.model.clone())
            .with_metadata("tools_enabled", self.config.enable_tools);

        Ok(AgentRegistration::new(info))
    }

    /// Processa uma requisição
    #[instrument(skip(self, request))]
    pub async fn process_request(&self, request: AgentRequest) -> HookResult<AgentResponse> {
        let start = std::time::Instant::now();

        let result = match request.command {
            AgentCommand::ExecuteCommand {
                command,
                args,
                cwd: _,
            } => {
                let full_cmd = format!("{} {}", command, args.join(" "));
                let output = self.execute(&["--bash", &full_cmd]).await?;
                Ok(ResponseData::Text { text: output })
            }
            AgentCommand::EditFile { path, instructions } => {
                let result = self.edit_file(&path, &instructions).await?;
                Ok(ResponseData::Text { text: result })
            }
            AgentCommand::ReadFile {
                path,
                offset: _,
                limit: _,
            } => {
                let prompt = format!("Read and display the contents of {}", path);
                let result = self.send_prompt(&prompt).await?;
                Ok(ResponseData::FileContent {
                    path,
                    content: result,
                    encoding: "utf-8".to_string(),
                })
            }
            AgentCommand::ExplainError {
                error_output,
                command,
            } => {
                let cmd = command.as_deref().unwrap_or("unknown");
                let result = self.explain_error(cmd, &error_output).await?;
                Ok(ResponseData::ErrorExplanation {
                    explanation: result,
                    suggestions: Vec::new(),
                    fix_available: true,
                })
            }
            AgentCommand::AnalyzeCode { path, .. } => {
                let result = self.analyze_code(&path).await?;
                Ok(ResponseData::Text { text: result })
            }
            AgentCommand::GenerateTests { path, .. } => {
                let result = self.generate_tests(&path).await?;
                Ok(ResponseData::Text { text: result })
            }
            AgentCommand::Ping => Ok(ResponseData::Pong {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            }),
            AgentCommand::GetCapabilities => {
                let caps = self.capabilities();
                Ok(ResponseData::Capabilities {
                    capabilities: caps
                        .supported_languages
                        .iter()
                        .map(|l| super::api::AgentCapability {
                            name: l.clone(),
                            description: format!("Support for {}", l),
                            version: "1.0".to_string(),
                            requires_permission: false,
                        })
                        .collect(),
                    // antes ia o primeiro idioma suportado ("rust") como versão
                    version: "1.0".to_string(),
                })
            }
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
        AgentCapabilities::full()
    }

    /// Cria handler de hook
    pub fn create_handler(&self) -> HookHandler {
        let _config = self.config.clone();

        HookHandler::new(move |event, _context| {
            // Handler síncrono simples - para operações async usar process_request
            info!("Claude Code hook received event: {:?}", event.event_type);

            // Aqui poderia implementar lógica de notificação para o Claude Code
            // Por exemplo, via arquivo de socket, stdin de processo, etc.

            Ok(())
        })
    }
}

/// Configuração do hook Claude Code a partir do arquivo de config
impl ClaudeCodeHook {
    /// Carrega configuração do arquivo de config
    pub fn from_config(config: &crate::config::Config) -> Option<Self> {
        // Verificar se há configuração para Claude Code
        let ai_config = &config.ai;
        // Aqui buscaríamos config específica de hooks
        // Por enquanto retorna default se Claude estiver habilitado
        if ai_config.enabled {
            return Some(Self::default());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_code_config_default() {
        let config = ClaudeCodeConfig::default();
        assert_eq!(config.executable_path, "claude");
        assert_eq!(config.model, "claude-sonnet-4-20250514");
        assert!(config.enable_tools);
    }

    #[test]
    fn test_claude_code_hook_creation() {
        let hook = ClaudeCodeHook::default();
        assert_eq!(hook.config.executable_path, "claude");
        assert!(!hook.state.process_running);
    }

    #[test]
    fn test_capabilities() {
        let hook = ClaudeCodeHook::default();
        let caps = hook.capabilities();

        assert!(caps.can_execute_commands);
        assert!(caps.can_edit_files);
        assert!(caps.can_analyze_code);
        assert!(caps.supports_streaming);
    }

    // Testes que requerem Claude instalado são ignorados por padrão
    #[tokio::test]
    #[ignore = "Requires Claude Code installation"]
    async fn test_detect() {
        let detected = ClaudeCodeHook::detect();
        // Resultado depende do ambiente
        info!(detected, "Claude Code detection completed");
    }

    #[tokio::test]
    #[ignore = "Requires Claude Code installation"]
    async fn test_get_version() {
        let hook = ClaudeCodeHook::default();
        let version = hook.get_version().await.unwrap();
        assert!(!version.is_empty());
        info!(version, "Claude Code version detected");
    }

    #[tokio::test]
    async fn test_create_registration() {
        let hook = ClaudeCodeHook::default();
        let reg = hook.create_registration().await.unwrap();

        assert_eq!(reg.info.agent_type, AgentType::ClaudeCode);
        assert_eq!(reg.info.name, "Claude Code");
        assert!(reg.info.capabilities.contains(&"code_edit".to_string()));
    }

    #[cfg(unix)]
    mod with_fake_cli {
        use super::*;
        use crate::ai::hooks::api::AnalysisType;
        use crate::ai::test_support::{fake_cli, fake_cli_calls};
        use tempfile::TempDir;

        const BODY: &str = r#"
case "$1" in
  --version) echo " 9.9.9 (Claude Code) " ;;
  --prompt) case "$2" in *FALHA*) echo "deu ruim" >&2; exit 3 ;; esac; echo "{\"ok\":true}" ;;
  --bash) echo "bash:$2" ;;
  *) echo "outro" ;;
esac"#;

        fn hook(
            dir: &TempDir,
            body: &str,
            extra: Vec<String>,
            verbose: bool,
        ) -> (ClaudeCodeHook, String) {
            let prog = fake_cli(dir, "claude", body);
            let hook = ClaudeCodeHook::new(ClaudeCodeConfig {
                executable_path: prog.clone(),
                verbose,
                extra_args: extra,
                working_dir: Some(dir.path().to_string_lossy().to_string()),
                timeout_secs: 5,
                ..Default::default()
            });
            (hook, prog)
        }

        #[tokio::test]
        async fn test_version_and_registration() {
            let dir = TempDir::new().unwrap();
            let (h, _) = hook(&dir, BODY, vec![], false);
            assert_eq!(h.get_version().await.unwrap(), "9.9.9 (Claude Code)");
            let reg = h.create_registration().await.unwrap();
            assert_eq!(reg.info.version.as_deref(), Some("9.9.9 (Claude Code)"));
            assert_eq!(reg.info.metadata["tools_enabled"], true);

            let (bad, _) = hook(&dir, "exit 1", vec![], false);
            assert!(matches!(
                bad.get_version().await,
                Err(HookError::AgentError { message, .. }) if message == "claude --version failed"
            ));
            let reg = bad.create_registration().await.unwrap();
            assert_eq!(reg.info.version.as_deref(), Some("unknown"));

            let missing = ClaudeCodeHook::new(ClaudeCodeConfig {
                executable_path: "/nao/existe/claude".into(),
                ..Default::default()
            });
            assert!(matches!(
                missing.get_version().await,
                Err(HookError::AgentError { .. })
            ));
            assert!(
                matches!(missing.execute(&["x"]).await, Err(HookError::AgentError { message, .. }) if message.starts_with("Execution failed"))
            );
        }

        #[tokio::test]
        async fn test_prompts_build_expected_arguments() {
            let dir = TempDir::new().unwrap();
            let (h, prog) = hook(&dir, BODY, vec!["--extra".into()], true);
            assert_eq!(h.send_prompt("oi").await.unwrap().trim(), "{\"ok\":true}");
            h.edit_file("a.rs", "renomear").await.unwrap();
            h.analyze_code("b.rs").await.unwrap();
            h.generate_tests("c.rs").await.unwrap();
            h.explain_error("make", "erro 2").await.unwrap();
            let calls = fake_cli_calls(&prog);
            assert_eq!(
                calls[0],
                vec![
                    "--prompt",
                    "oi",
                    "--output-format",
                    "json",
                    "--verbose",
                    "--extra"
                ]
            );
            assert!(calls[1][1]
                .contains("Edit the file a.rs according to these instructions: renomear"));
            assert!(calls[2][1].contains("Analyze the code in b.rs"));
            assert!(calls[3][1].contains("unit tests for the code in c.rs"));
            // o log grava um argumento por linha; o prompt tem quebras de linha
            assert_eq!(calls[4][1], "The command 'make' failed with this error:");
            assert_eq!(calls[4][2], "erro 2");

            let err = h.send_prompt("FALHA").await.unwrap_err();
            assert!(
                matches!(err, HookError::AgentError { message, .. } if message.contains("deu ruim"))
            );
        }

        #[tokio::test]
        async fn test_execute_times_out() {
            let dir = TempDir::new().unwrap();
            let prog = fake_cli(&dir, "lento", "sleep 5");
            let h = ClaudeCodeHook::new(ClaudeCodeConfig {
                executable_path: prog,
                timeout_secs: 0,
                ..Default::default()
            });
            assert!(matches!(
                h.execute(&["x"]).await,
                Err(HookError::Timeout { timeout_ms: 0 })
            ));
        }

        #[tokio::test]
        async fn test_process_request_for_each_command() {
            let dir = TempDir::new().unwrap();
            let (h, prog) = hook(&dir, BODY, vec![], false);

            let r = h
                .process_request(AgentRequest::new(AgentCommand::ExecuteCommand {
                    command: "ls".into(),
                    args: vec!["-la".into()],
                    cwd: None,
                }))
                .await
                .unwrap();
            assert!(r.success);
            assert!(
                matches!(r.data, ResponseData::Text { ref text } if text.trim() == "bash:ls -la")
            );

            for cmd in [
                AgentCommand::EditFile {
                    path: "x".into(),
                    instructions: "y".into(),
                },
                AgentCommand::AnalyzeCode {
                    path: "x".into(),
                    analysis_type: AnalysisType::default(),
                },
                AgentCommand::GenerateTests {
                    path: "x".into(),
                    max_tests: 3,
                },
            ] {
                let r = h.process_request(AgentRequest::new(cmd)).await.unwrap();
                assert!(r.success);
                assert!(matches!(r.data, ResponseData::Text { .. }));
            }

            let r = h
                .process_request(AgentRequest::new(AgentCommand::ReadFile {
                    path: "/etc/x".into(),
                    offset: None,
                    limit: None,
                }))
                .await
                .unwrap();
            assert!(
                matches!(r.data, ResponseData::FileContent { ref path, ref encoding, .. } if path == "/etc/x" && encoding == "utf-8")
            );

            let r = h
                .process_request(AgentRequest::new(AgentCommand::ExplainError {
                    error_output: "boom".into(),
                    command: None,
                }))
                .await
                .unwrap();
            assert!(matches!(
                r.data,
                ResponseData::ErrorExplanation {
                    fix_available: true,
                    ..
                }
            ));
            assert!(
                fake_cli_calls(&prog).last().unwrap()[1].contains("The command 'unknown' failed")
            );

            let r = h
                .process_request(AgentRequest::new(AgentCommand::Ping))
                .await
                .unwrap();
            assert!(matches!(r.data, ResponseData::Pong { timestamp } if timestamp > 0));

            let r = h
                .process_request(AgentRequest::new(AgentCommand::GetCapabilities))
                .await
                .unwrap();
            match r.data {
                ResponseData::Capabilities {
                    capabilities,
                    version,
                } => {
                    assert!(!capabilities.is_empty());
                    // Regressão: a versão era o primeiro idioma suportado
                    assert_eq!(version, "1.0");
                }
                other => panic!("{other:?}"),
            }

            let r = h
                .process_request(AgentRequest::new(AgentCommand::WriteFile {
                    path: "x".into(),
                    content: "y".into(),
                }))
                .await
                .unwrap();
            assert!(!r.success);
            assert!(
                matches!(r.data, ResponseData::Error { ref message, .. } if message.contains("WriteFile"))
            );
        }

        #[test]
        fn test_state_snapshot_handler_and_from_config() {
            let h = ClaudeCodeHook::default();
            let snap = h.state_snapshot();
            assert!(!snap.process_running);
            assert_eq!(snap.pending_command_count, 0);
            assert_eq!(snap.cached_response_count, 0);
            assert!(snap.last_activity.is_none());

            let handler = h.create_handler();
            let ev = crate::ai::hooks::event::AgentEvent::new(
                crate::ai::hooks::event::EventType::TerminalStart,
            );
            let ctx = crate::ai::hooks::context::AgentContext::new("s", 1);
            handler.handle(&ev, &ctx).unwrap();

            let mut config = crate::config::Config::default();
            config.ai.enabled = false;
            assert!(ClaudeCodeHook::from_config(&config).is_none());
            config.ai.enabled = true;
            assert_eq!(
                ClaudeCodeHook::from_config(&config)
                    .unwrap()
                    .config
                    .executable_path,
                "claude"
            );
            // detect() depende do ambiente; só não pode entrar em pânico
            let _ = ClaudeCodeHook::detect();
        }
    }
}
