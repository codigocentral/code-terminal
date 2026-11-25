//! Hook genérico para AI agents personalizados

use super::{
    api::{AgentCapabilities, AgentRequest, AgentResponse, ResponseData},
    error::{HookError, HookResult},
    event::AgentEvent,
    registry::{AgentRegistration, HookHandler},
    types::{AgentInfo, AgentType, HookPriority, HookType},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::{debug, info, instrument};

/// Hook genérico para AI agents
#[derive(Debug)]
pub struct GenericAgentHook {
    /// Nome do agente
    name: String,
    /// Configuração
    config: GenericAgentConfig,
    /// Handlers registrados
    handlers: HashMap<HookType, Vec<GenericHandler>>,
}

/// Configuração para agente genérico
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericAgentConfig {
    /// Comando para executar o agente
    pub command: String,
    /// Argumentos fixos
    pub args: Vec<String>,
    /// Variáveis de ambiente
    pub env: HashMap<String, String>,
    /// Diretório de trabalho
    pub working_dir: Option<String>,
    /// Timeout padrão
    pub timeout_secs: u64,
    /// Formato de entrada
    pub input_format: DataFormat,
    /// Formato de saída
    pub output_format: DataFormat,
    /// Protocolo de comunicação
    pub protocol: ProtocolType,
    /// Endpoint (para HTTP/WebSocket)
    pub endpoint: Option<String>,
    /// Capacidades do agente
    pub capabilities: Vec<String>,
}

impl Default for GenericAgentConfig {
    fn default() -> Self {
        Self {
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            working_dir: None,
            timeout_secs: 60,
            input_format: DataFormat::Json,
            output_format: DataFormat::Json,
            protocol: ProtocolType::Stdio,
            endpoint: None,
            capabilities: Vec::new(),
        }
    }
}

/// Formato de dados
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataFormat {
    Json,
    Yaml,
    Xml,
    PlainText,
    Binary,
}

/// Tipo de protocolo
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolType {
    Stdio,
    Http,
    WebSocket,
    Grpc,
    UnixSocket,
    NamedPipe,
}

/// Handler genérico
#[derive(Debug, Clone)]
struct GenericHandler {
    hook_type: HookType,
    priority: HookPriority,
    callback: String, // Nome do callback ou comando
}

/// Resumo público de handler genérico registrado.
#[derive(Debug, Clone)]
pub struct GenericHandlerInfo {
    pub hook_type: HookType,
    pub priority: HookPriority,
    pub callback: String,
}

impl GenericAgentHook {
    /// Cria um novo hook genérico
    pub fn new(name: impl Into<String>, config: GenericAgentConfig) -> Self {
        Self {
            name: name.into(),
            config,
            handlers: HashMap::new(),
        }
    }

    /// Registra um handler
    pub fn register_handler(
        &mut self,
        hook_type: HookType,
        callback: impl Into<String>,
        priority: HookPriority,
    ) {
        let handler = GenericHandler {
            hook_type,
            priority,
            callback: callback.into(),
        };

        self.handlers.entry(hook_type).or_default().push(handler);
    }

    /// Lista handlers registrados.
    pub fn handlers(&self) -> Vec<GenericHandlerInfo> {
        self.handlers
            .values()
            .flat_map(|handlers| handlers.iter())
            .map(|handler| GenericHandlerInfo {
                hook_type: handler.hook_type,
                priority: handler.priority,
                callback: handler.callback.clone(),
            })
            .collect()
    }

    /// Remove handlers de um tipo
    pub fn unregister_handlers(&mut self, hook_type: &HookType) {
        self.handlers.remove(hook_type);
    }

    /// Executa comando genérico
    #[instrument(skip(self, input))]
    pub async fn execute(&self, input: Option<&str>) -> HookResult<String> {
        if self.config.command.is_empty() {
            return Err(HookError::ConfigError(
                "No command configured for generic agent".to_string(),
            ));
        }

        let mut cmd = Command::new(&self.config.command);
        cmd.args(&self.config.args);

        if let Some(dir) = &self.config.working_dir {
            cmd.current_dir(dir);
        }

        // Configurar variáveis de ambiente
        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        // Configurar stdin se houver input
        if input.is_some() {
            cmd.stdin(Stdio::piped());
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        debug!("Executing generic agent: {:?}", cmd);

        let mut child = cmd.spawn().map_err(|e| HookError::AgentError {
            agent: self.name.clone(),
            message: format!("Failed to spawn: {}", e),
        })?;

        // Envia o input em paralelo com a leitura da saída: escrever tudo
        // antes de ler travava (sem timeout) quando o processo enchia o pipe
        // de stdout antes de consumir todo o stdin
        let writer = match (input, child.stdin.take()) {
            (Some(input_data), Some(mut stdin)) => {
                let data = input_data.as_bytes().to_vec();
                Some(tokio::spawn(async move {
                    // stdin é fechado quando dropado, ao fim da task
                    stdin.write_all(&data).await
                }))
            }
            _ => None,
        };

        let output = timeout(
            Duration::from_secs(self.config.timeout_secs),
            child.wait_with_output(),
        )
        .await
        .map_err(|_| HookError::timeout(self.config.timeout_secs * 1000))?
        .map_err(|e| HookError::AgentError {
            agent: self.name.clone(),
            message: format!("Execution failed: {}", e),
        })?;

        if let Some(writer) = writer {
            match writer.await {
                // o processo pode sair sem ler todo o input
                Ok(Err(e)) if e.kind() != std::io::ErrorKind::BrokenPipe => {
                    return Err(HookError::CommunicationError(format!(
                        "Failed to write stdin: {}",
                        e
                    )));
                }
                _ => {}
            }
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if !output.status.success() {
            return Err(HookError::AgentError {
                agent: self.name.clone(),
                message: format!("Command failed: {}", stderr),
            });
        }

        Ok(stdout.to_string())
    }

    /// Envia evento para o agente
    #[instrument(skip(self, event))]
    pub async fn send_event(&self, event: &AgentEvent) -> HookResult<String> {
        let input = match self.config.input_format {
            DataFormat::Json => serde_json::to_string(event)
                .map_err(|e| HookError::SerializationError(e.to_string()))?,
            DataFormat::Yaml => serde_yaml::to_string(event)
                .map_err(|e| HookError::SerializationError(e.to_string()))?,
            DataFormat::PlainText => format!("{:?}", event),
            _ => {
                return Err(HookError::UnsupportedHook {
                    hook_type: format!("input format {:?}", self.config.input_format),
                })
            }
        };

        self.execute(Some(&input)).await
    }

    /// Cria agent registration
    pub fn create_registration(&self) -> AgentRegistration {
        let info = AgentInfo::new(AgentType::Generic, &self.name)
            .with_capability("custom")
            .with_metadata("protocol", format!("{:?}", self.config.protocol))
            .with_metadata("command", self.config.command.clone());

        let mut reg = AgentRegistration::new(info);

        if let Some(endpoint) = &self.config.endpoint {
            reg = reg.with_endpoint(endpoint);
        }

        reg
    }

    /// Processa uma requisição
    #[instrument(skip(self, request))]
    pub async fn process_request(&self, request: AgentRequest) -> HookResult<AgentResponse> {
        let start = std::time::Instant::now();

        let input = match self.config.input_format {
            DataFormat::Json => serde_json::to_string(&request)
                .map_err(|e| HookError::SerializationError(e.to_string()))?,
            DataFormat::Yaml => serde_yaml::to_string(&request)
                .map_err(|e| HookError::SerializationError(e.to_string()))?,
            DataFormat::PlainText => format!("{:?}", request),
            _ => {
                return Err(HookError::UnsupportedHook {
                    hook_type: format!("input format {:?}", self.config.input_format),
                })
            }
        };

        let result = match self.execute(Some(&input)).await {
            Ok(output) => {
                // Parsear output conforme formato
                let data = match self.config.output_format {
                    DataFormat::Json => {
                        // Tentar parsear como resposta JSON
                        ResponseData::Text { text: output }
                    }
                    _ => ResponseData::Text { text: output },
                };
                Ok(data)
            }
            Err(e) => Err(e),
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
            can_execute_commands: self.config.capabilities.contains(&"execute".to_string()),
            can_edit_files: self.config.capabilities.contains(&"edit".to_string()),
            can_analyze_code: self.config.capabilities.contains(&"analyze".to_string()),
            can_generate_tests: self.config.capabilities.contains(&"test".to_string()),
            can_refactor: self.config.capabilities.contains(&"refactor".to_string()),
            supports_streaming: self.config.protocol == ProtocolType::WebSocket,
            supports_files: true,
            max_file_size: Some(10 * 1024 * 1024),
            supported_languages: Vec::new(),
        }
    }

    /// Cria handler de hook
    pub fn create_handler(&self) -> HookHandler {
        let name = self.name.clone();
        HookHandler::new(move |event, _context| {
            info!("{} hook received event: {:?}", name, event.event_type);
            Ok(())
        })
    }

    /// Retorna nome do agente
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Retorna configuração
    pub fn config(&self) -> &GenericAgentConfig {
        &self.config
    }
}

/// Builder para configuração genérica
pub struct GenericAgentConfigBuilder {
    config: GenericAgentConfig,
}

impl GenericAgentConfigBuilder {
    /// Cria novo builder
    pub fn new() -> Self {
        Self {
            config: GenericAgentConfig::default(),
        }
    }

    /// Define comando
    pub fn command(mut self, cmd: impl Into<String>) -> Self {
        self.config.command = cmd.into();
        self
    }

    /// Adiciona argumento
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.config.args.push(arg.into());
        self
    }

    /// Define variável de ambiente
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.env.insert(key.into(), value.into());
        self
    }

    /// Define diretório de trabalho
    pub fn working_dir(mut self, dir: impl Into<String>) -> Self {
        self.config.working_dir = Some(dir.into());
        self
    }

    /// Define timeout
    pub fn timeout(mut self, secs: u64) -> Self {
        self.config.timeout_secs = secs;
        self
    }

    /// Define formato de entrada
    pub fn input_format(mut self, format: DataFormat) -> Self {
        self.config.input_format = format;
        self
    }

    /// Define formato de saída
    pub fn output_format(mut self, format: DataFormat) -> Self {
        self.config.output_format = format;
        self
    }

    /// Define protocolo
    pub fn protocol(mut self, protocol: ProtocolType) -> Self {
        self.config.protocol = protocol;
        self
    }

    /// Define endpoint
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config.endpoint = Some(endpoint.into());
        self
    }

    /// Adiciona capacidade
    pub fn capability(mut self, cap: impl Into<String>) -> Self {
        self.config.capabilities.push(cap.into());
        self
    }

    /// Build
    pub fn build(self) -> GenericAgentConfig {
        self.config
    }
}

impl Default for GenericAgentConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generic_agent_config_default() {
        let config = GenericAgentConfig::default();
        assert!(config.command.is_empty());
        assert_eq!(config.timeout_secs, 60);
        assert!(matches!(config.input_format, DataFormat::Json));
        assert!(matches!(config.protocol, ProtocolType::Stdio));
    }

    #[test]
    fn test_generic_agent_config_builder() {
        let config = GenericAgentConfigBuilder::new()
            .command("my-agent")
            .arg("--server")
            .arg("--port")
            .arg("8080")
            .env("API_KEY", "secret")
            .timeout(120)
            .protocol(ProtocolType::Http)
            .capability("custom-task")
            .build();

        assert_eq!(config.command, "my-agent");
        assert_eq!(config.args, vec!["--server", "--port", "8080"]);
        assert_eq!(config.env.get("API_KEY"), Some(&"secret".to_string()));
        assert_eq!(config.timeout_secs, 120);
        assert!(matches!(config.protocol, ProtocolType::Http));
    }

    #[test]
    fn test_generic_agent_hook_creation() {
        let config = GenericAgentConfig::default();
        let hook = GenericAgentHook::new("my-agent", config);

        assert_eq!(hook.name(), "my-agent");
        assert!(hook.handlers.is_empty());
    }

    #[test]
    fn test_register_handler() {
        let config = GenericAgentConfig::default();
        let mut hook = GenericAgentHook::new("my-agent", config);

        hook.register_handler(
            HookType::OnCommandExecute,
            "handle_command",
            HookPriority::Normal,
        );

        assert_eq!(hook.handlers.len(), 1);
        assert!(hook.handlers.contains_key(&HookType::OnCommandExecute));
    }

    #[test]
    fn test_create_registration() {
        let config = GenericAgentConfigBuilder::new()
            .command("my-agent")
            .endpoint("http://localhost:8080")
            .build();

        let hook = GenericAgentHook::new("my-agent", config);
        let reg = hook.create_registration();

        assert_eq!(reg.info.agent_type, AgentType::Generic);
        assert_eq!(reg.info.name, "my-agent");
        assert!(reg.endpoint.is_some());
    }

    #[test]
    fn test_capabilities() {
        let config = GenericAgentConfigBuilder::new()
            .capability("execute")
            .capability("edit")
            .build();

        let hook = GenericAgentHook::new("my-agent", config);
        let caps = hook.capabilities();

        assert!(caps.can_execute_commands);
        assert!(caps.can_edit_files);
        assert!(!caps.can_analyze_code);
    }

    // Teste que requer um comando real
    #[tokio::test]
    #[ignore = "Requires a real command"]
    async fn test_execute() {
        let config = GenericAgentConfigBuilder::new()
            .command("echo")
            .arg("hello")
            .build();

        let hook = GenericAgentHook::new("echo", config);
        let output = hook.execute(None).await.unwrap();

        assert!(output.contains("hello"));
    }

    #[cfg(unix)]
    fn sh_hook(script: &str, builder: GenericAgentConfigBuilder) -> GenericAgentHook {
        let config = builder.command("sh").arg("-c").arg(script).build();
        GenericAgentHook::new("sh-agent", config)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_execute_with_env_cwd_and_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let hook = sh_hook(
            "printf '%s|%s|' \"$CHAVE\" \"$(pwd)\"; cat",
            GenericAgentConfigBuilder::new()
                .env("CHAVE", "valor")
                .working_dir(dir.path().to_string_lossy()),
        );
        let out = hook.execute(Some("entrada")).await.unwrap();
        let parts: Vec<&str> = out.split('|').collect();
        assert_eq!(parts[0], "valor");
        assert_eq!(
            std::fs::canonicalize(parts[1]).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
        assert_eq!(parts[2], "entrada");
        assert_eq!(
            hook.config().working_dir.as_deref(),
            Some(dir.path().to_str().unwrap())
        );

        // sem input o stdin não é usado
        let echo = sh_hook("echo oi", GenericAgentConfigBuilder::new());
        assert_eq!(echo.execute(None).await.unwrap(), "oi\n");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_large_input_does_not_deadlock() {
        // Regressão: o input era todo escrito antes de ler a saída; com mais
        // que o buffer do pipe, `cat` travava e o timeout não valia
        let hook = sh_hook("cat", GenericAgentConfigBuilder::new().timeout(10));
        let big = "x".repeat(1024 * 1024);
        let out = tokio::time::timeout(Duration::from_secs(20), hook.execute(Some(&big)))
            .await
            .expect("execute travou")
            .unwrap();
        assert_eq!(out.len(), big.len());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_process_exiting_without_reading_stdin_is_ok() {
        let hook = sh_hook("echo cedo", GenericAgentConfigBuilder::new());
        let big = "y".repeat(512 * 1024);
        assert_eq!(hook.execute(Some(&big)).await.unwrap(), "cedo\n");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_execute_errors() {
        let empty = GenericAgentHook::new("vazio", GenericAgentConfig::default());
        assert!(matches!(
            empty.execute(None).await,
            Err(HookError::ConfigError(_))
        ));

        let missing = GenericAgentHook::new(
            "x",
            GenericAgentConfigBuilder::new()
                .command("/nao/existe/bin")
                .build(),
        );
        assert!(matches!(
            missing.execute(None).await,
            Err(HookError::AgentError { message, .. }) if message.starts_with("Failed to spawn")
        ));

        let failing = sh_hook(
            "echo problema >&2; exit 4",
            GenericAgentConfigBuilder::new(),
        );
        assert!(matches!(
            failing.execute(None).await,
            Err(HookError::AgentError { agent, message }) if agent == "sh-agent" && message.contains("problema")
        ));

        let slow = sh_hook("sleep 5", GenericAgentConfigBuilder::new().timeout(0));
        assert!(matches!(
            slow.execute(None).await,
            Err(HookError::Timeout { .. })
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_send_event_in_each_format() {
        use crate::ai::hooks::event::EventType;
        let event = AgentEvent::new(EventType::TabClosed { tab_id: 9 });

        let json = sh_hook(
            "cat",
            GenericAgentConfigBuilder::new().input_format(DataFormat::Json),
        );
        let v: serde_json::Value =
            serde_json::from_str(&json.send_event(&event).await.unwrap()).unwrap();
        assert_eq!(v["event_type"]["type"], "tab_closed");

        let yaml = sh_hook(
            "cat",
            GenericAgentConfigBuilder::new().input_format(DataFormat::Yaml),
        );
        assert!(yaml
            .send_event(&event)
            .await
            .unwrap()
            .contains("tab_closed"));

        let text = sh_hook(
            "cat",
            GenericAgentConfigBuilder::new().input_format(DataFormat::PlainText),
        );
        assert!(text.send_event(&event).await.unwrap().contains("TabClosed"));

        for fmt in [DataFormat::Xml, DataFormat::Binary] {
            let h = sh_hook("cat", GenericAgentConfigBuilder::new().input_format(fmt));
            assert!(matches!(
                h.send_event(&event).await,
                Err(HookError::UnsupportedHook { .. })
            ));
            assert!(matches!(
                h.process_request(AgentRequest::execute_command("x")).await,
                Err(HookError::UnsupportedHook { .. })
            ));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_process_request_wraps_output_and_errors() {
        let echo = sh_hook(
            "cat",
            GenericAgentConfigBuilder::new()
                .input_format(DataFormat::Json)
                .output_format(DataFormat::Json),
        );
        let req = AgentRequest::execute_command("ls");
        let id = req.id;
        let r = echo.process_request(req).await.unwrap();
        assert!(r.success);
        assert_eq!(r.request_id, id);
        match r.data {
            ResponseData::Text { text } => {
                let v: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(v["id"], id.to_string());
            }
            other => panic!("{other:?}"),
        }

        for fmt in [DataFormat::Yaml, DataFormat::PlainText] {
            let h = sh_hook(
                "cat",
                GenericAgentConfigBuilder::new()
                    .input_format(fmt)
                    .output_format(DataFormat::PlainText),
            );
            let r = h
                .process_request(AgentRequest::execute_command("ls"))
                .await
                .unwrap();
            assert!(r.success);
        }

        let failing = sh_hook("exit 1", GenericAgentConfigBuilder::new());
        let r = failing
            .process_request(AgentRequest::execute_command("ls"))
            .await
            .unwrap();
        assert!(!r.success);
        assert!(matches!(r.data, ResponseData::Error { .. }));
    }

    #[test]
    fn test_handlers_listing_and_removal() {
        let mut hook = GenericAgentHook::new("h", GenericAgentConfig::default());
        hook.register_handler(HookType::OnCommandExecute, "a", HookPriority::High);
        hook.register_handler(HookType::OnCommandExecute, "b", HookPriority::Low);
        hook.register_handler(HookType::OnError, "c", HookPriority::Normal);
        let mut callbacks: Vec<String> = hook.handlers().into_iter().map(|h| h.callback).collect();
        callbacks.sort();
        assert_eq!(callbacks, vec!["a", "b", "c"]);
        hook.unregister_handlers(&HookType::OnCommandExecute);
        let left = hook.handlers();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].hook_type, HookType::OnError);
        assert_eq!(left[0].priority, HookPriority::Normal);

        let ws = GenericAgentHook::new(
            "ws",
            GenericAgentConfigBuilder::new()
                .protocol(ProtocolType::WebSocket)
                .capability("analyze")
                .capability("test")
                .capability("refactor")
                .build(),
        );
        let caps = ws.capabilities();
        assert!(
            caps.supports_streaming
                && caps.can_analyze_code
                && caps.can_generate_tests
                && caps.can_refactor
        );
        assert!(ws.create_registration().endpoint.is_none());
        ws.create_handler()
            .handle(
                &AgentEvent::new(crate::ai::hooks::event::EventType::TerminalStart),
                &crate::ai::hooks::context::AgentContext::new("s", 0),
            )
            .unwrap();
        let _ = GenericAgentConfigBuilder::default().build();
    }
}
