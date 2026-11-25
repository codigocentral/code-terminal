//! Transport layer for AI Agent communication
//!
//! This module provides the transport mechanisms for communicating with
//! external AI agents like Claude Code, Aider, and OpenCode.

use super::api::{AgentRequest, AgentResponse};
use super::error::{HookError, HookErrorExt, HookResult};
use serde::{Deserialize, Serialize};
// use std::io::BufRead;  // Currently unused
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{error, info, trace, warn};

/// Protocol types for agent communication
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolType {
    /// JSON-RPC over stdio
    JsonRpcStdio,
    /// JSON-RPC over Unix socket
    JsonRpcUnixSocket,
    /// JSON-RPC over TCP
    JsonRpcTcp,
    /// Custom protocol over stdio
    CustomStdio,
}

impl std::fmt::Display for ProtocolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolType::JsonRpcStdio => write!(f, "jsonrpc-stdio"),
            ProtocolType::JsonRpcUnixSocket => write!(f, "jsonrpc-unix"),
            ProtocolType::JsonRpcTcp => write!(f, "jsonrpc-tcp"),
            ProtocolType::CustomStdio => write!(f, "custom-stdio"),
        }
    }
}

/// Transport configuration
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// Protocol type
    pub protocol: ProtocolType,
    /// Command to spawn agent (for stdio transport)
    pub command: Option<String>,
    /// Arguments for command
    pub args: Vec<String>,
    /// Working directory
    pub cwd: Option<PathBuf>,
    /// Environment variables
    pub env: std::collections::HashMap<String, String>,
    /// Socket path (for Unix socket)
    pub socket_path: Option<PathBuf>,
    /// TCP address (for TCP transport)
    pub tcp_addr: Option<String>,
    /// Connection timeout
    pub connect_timeout_ms: u64,
    /// Request timeout
    pub request_timeout_ms: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            protocol: ProtocolType::JsonRpcStdio,
            command: None,
            args: Vec::new(),
            cwd: None,
            env: std::collections::HashMap::new(),
            socket_path: None,
            tcp_addr: None,
            connect_timeout_ms: 5000,
            request_timeout_ms: 30000,
        }
    }
}

/// Transport trait for agent communication
#[async_trait::async_trait]
pub trait AgentTransport: Send + Sync {
    /// Connect to the agent
    async fn connect(&mut self) -> HookResult<()>;

    /// Disconnect from the agent
    async fn disconnect(&mut self) -> HookResult<()>;

    /// Check if connected
    fn is_connected(&self) -> bool;

    /// Send a request and wait for response
    async fn send_request(&mut self, request: AgentRequest) -> HookResult<AgentResponse>;

    /// Send a notification (no response expected)
    async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> HookResult<()>;

    /// Get transport info
    fn transport_info(&self) -> TransportInfo;
}

/// Transport information
#[derive(Debug, Clone)]
pub struct TransportInfo {
    pub protocol: ProtocolType,
    pub connected: bool,
    pub endpoint: String,
}

/// JSON-RPC message wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Stdio transport implementation
pub struct StdioTransport {
    config: TransportConfig,
    child: Option<tokio::process::Child>,
    stdin: Option<Arc<Mutex<tokio::process::ChildStdin>>>,
    stdout: Option<Arc<Mutex<AsyncBufReader<tokio::process::ChildStdout>>>>,
    pending_requests: Arc<
        Mutex<std::collections::HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<JsonRpcMessage>>>,
    >,
    response_task: Option<tokio::task::JoinHandle<()>>,
    stderr_task: Option<tokio::task::JoinHandle<()>>,
}

impl StdioTransport {
    /// Creates a new stdio transport
    pub fn new(config: TransportConfig) -> Self {
        Self {
            config,
            child: None,
            stdin: None,
            stdout: None,
            pending_requests: Arc::new(Mutex::new(std::collections::HashMap::new())),
            response_task: None,
            stderr_task: None,
        }
    }

    /// Start the response reading task
    fn start_response_task(&mut self) {
        let Some(stdout) = self.stdout.clone() else {
            return;
        };
        let pending = Arc::clone(&self.pending_requests);

        let handle = tokio::spawn(async move {
            let mut stdout = stdout.lock().await;
            let mut line = String::new();

            loop {
                line.clear();
                match stdout.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF
                        trace!("Stdio transport EOF");
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        trace!("Received: {}", trimmed);

                        // Parse as JSON-RPC message
                        match serde_json::from_str::<JsonRpcMessage>(trimmed) {
                            Ok(msg) => {
                                if let Some(id) = msg.id {
                                    // Try to find pending request
                                    let mut pending = pending.lock().await;
                                    if let Some(tx) = pending.remove(&id) {
                                        let _ = tx.send(msg);
                                    }
                                }
                            }
                            Err(e) => {
                                trace!("Failed to parse message: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error reading from stdout: {}", e);
                        break;
                    }
                }
            }
        });

        self.response_task = Some(handle);
    }

    fn start_stderr_task(&mut self, stderr: tokio::process::ChildStderr) {
        let handle = tokio::spawn(async move {
            let mut stderr = AsyncBufReader::new(stderr);
            let mut line = String::new();

            loop {
                line.clear();
                match stderr.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            warn!(stderr = %trimmed, "AI agent stderr");
                        }
                    }
                    Err(error) => {
                        warn!(error = %error, "Failed to read AI agent stderr");
                        break;
                    }
                }
            }
        });

        self.stderr_task = Some(handle);
    }
}

#[async_trait::async_trait]
impl AgentTransport for StdioTransport {
    async fn connect(&mut self) -> HookResult<()> {
        if self.is_connected() {
            return Ok(());
        }

        let command = self.config.command.as_ref().ok_or_else(|| {
            HookError::configuration_error("No command specified for stdio transport")
        })?;

        info!("Starting agent process: {}", command);

        let mut cmd = tokio::process::Command::new(command);
        cmd.args(&self.config.args)
            .stdout(Stdio::piped())
            .stdin(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(cwd) = &self.config.cwd {
            cmd.current_dir(cwd);
        }

        for (key, value) in &self.config.env {
            cmd.env(key, value);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| HookError::communication(format!("Failed to spawn process: {}", e)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| HookError::communication("Failed to capture stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| HookError::communication("Failed to capture stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| HookError::communication("Failed to capture stderr"))?;

        self.child = Some(child);
        self.stdin = Some(Arc::new(Mutex::new(stdin)));
        self.stdout = Some(Arc::new(Mutex::new(AsyncBufReader::new(stdout))));

        // Start response reading task
        self.start_response_task();
        self.start_stderr_task(stderr);

        // Wait a moment for process to initialize
        tokio::time::sleep(Duration::from_millis(100)).await;

        info!("Stdio transport connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> HookResult<()> {
        if let Some(task) = self.response_task.take() {
            task.abort();
        }
        if let Some(task) = self.stderr_task.take() {
            task.abort();
        }

        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }

        self.stdin = None;
        self.stdout = None;

        info!("Stdio transport disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stdin.is_some() && self.stdout.is_some()
    }

    async fn send_request(&mut self, request: AgentRequest) -> HookResult<AgentResponse> {
        if !self.is_connected() {
            return Err(HookError::connection_error("Not connected"));
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        let id = request.id;

        // Register pending request
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, tx);
        }

        // Send request
        let msg = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: Some("agent.request".to_string()),
            params: Some(serde_json::to_value(&request).map_err(|e| {
                HookError::serialization_error(format!("Failed to serialize request: {}", e))
            })?),
            result: None,
            error: None,
        };

        let json = serde_json::to_string(&msg).map_err(|e| {
            HookError::serialization_error(format!("Failed to serialize message: {}", e))
        })?;

        {
            let stdin = self
                .stdin
                .as_ref()
                .ok_or_else(|| HookError::connection_error("Not connected"))?;
            let mut stdin = stdin.lock().await;
            stdin.write_all(json.as_bytes()).await.map_err(|e| {
                HookError::connection_error(format!("Failed to write to stdin: {}", e))
            })?;
            stdin.write_all(b"\n").await.map_err(|e| {
                HookError::connection_error(format!("Failed to write newline: {}", e))
            })?;
            stdin.flush().await.map_err(|e| {
                HookError::connection_error(format!("Failed to flush stdin: {}", e))
            })?;
        }

        trace!("Sent request: {}", json);

        // Wait for response with timeout
        let timeout_duration = Duration::from_millis(self.config.request_timeout_ms);
        let result = timeout(timeout_duration, rx).await;

        match result {
            Ok(Ok(msg)) => {
                if let Some(error) = msg.error {
                    Err(HookError::agent_error(
                        "unknown",
                        format!("Agent error ({}): {}", error.code, error.message),
                    ))
                } else if let Some(result) = msg.result {
                    serde_json::from_value::<AgentResponse>(result).map_err(|e| {
                        HookError::serialization_error(format!(
                            "Failed to deserialize response: {}",
                            e
                        ))
                    })
                } else {
                    Err(HookError::agent_error(
                        "unknown",
                        "Empty response from agent",
                    ))
                }
            }
            Ok(Err(_)) => Err(HookError::communication("Response channel closed")),
            Err(_) => {
                // Timeout - remove pending request
                let mut pending = self.pending_requests.lock().await;
                pending.remove(&id);
                Err(HookError::timeout(self.config.request_timeout_ms))
            }
        }
    }

    async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> HookResult<()> {
        if !self.is_connected() {
            return Err(HookError::connection_error("Not connected"));
        }

        let msg = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: Some(method.to_string()),
            params: Some(params),
            result: None,
            error: None,
        };

        let json = serde_json::to_string(&msg).map_err(|e| {
            HookError::serialization_error(format!("Failed to serialize message: {}", e))
        })?;

        let stdin = self
            .stdin
            .as_ref()
            .ok_or_else(|| HookError::connection_error("Not connected"))?;
        let mut stdin = stdin.lock().await;
        stdin
            .write_all(json.as_bytes())
            .await
            .map_err(|e| HookError::connection_error(format!("Failed to write to stdin: {}", e)))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| HookError::connection_error(format!("Failed to write newline: {}", e)))?;
        stdin
            .flush()
            .await
            .map_err(|e| HookError::connection_error(format!("Failed to flush stdin: {}", e)))?;

        Ok(())
    }

    fn transport_info(&self) -> TransportInfo {
        TransportInfo {
            protocol: self.config.protocol,
            connected: self.is_connected(),
            endpoint: self.config.command.clone().unwrap_or_default(),
        }
    }
}

/// Unix socket transport implementation
#[cfg(unix)]
pub struct UnixSocketTransport {
    config: TransportConfig,
    /// Metade de escrita do socket; a de leitura fica com a task de respostas
    /// (antes a task segurava o lock do stream inteiro e toda escrita travava)
    stream: Option<Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>>,
    pending_requests: Arc<
        Mutex<std::collections::HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<JsonRpcMessage>>>,
    >,
    response_task: Option<tokio::task::JoinHandle<()>>,
}

#[cfg(unix)]
impl UnixSocketTransport {
    /// Creates a new Unix socket transport
    pub fn new(config: TransportConfig) -> Self {
        Self {
            config,
            stream: None,
            pending_requests: Arc::new(Mutex::new(std::collections::HashMap::new())),
            response_task: None,
        }
    }

    fn start_response_task(&mut self, read_half: tokio::net::unix::OwnedReadHalf) {
        let pending = Arc::clone(&self.pending_requests);

        let handle = tokio::spawn(async move {
            let reader = AsyncBufReader::new(read_half);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                trace!("Received: {}", line);

                match serde_json::from_str::<JsonRpcMessage>(&line) {
                    Ok(msg) => {
                        if let Some(id) = msg.id {
                            let mut pending = pending.lock().await;
                            if let Some(tx) = pending.remove(&id) {
                                let _ = tx.send(msg);
                            }
                        }
                    }
                    Err(e) => {
                        trace!("Failed to parse message: {}", e);
                    }
                }
            }
        });

        self.response_task = Some(handle);
    }
}

#[cfg(unix)]
#[async_trait::async_trait]
impl AgentTransport for UnixSocketTransport {
    async fn connect(&mut self) -> HookResult<()> {
        if self.is_connected() {
            return Ok(());
        }

        let socket_path = self
            .config
            .socket_path
            .as_ref()
            .ok_or_else(|| HookError::configuration_error("No socket path specified"))?;

        info!("Connecting to Unix socket: {:?}", socket_path);

        let connect_timeout = Duration::from_millis(self.config.connect_timeout_ms);
        let stream = timeout(connect_timeout, UnixStream::connect(socket_path))
            .await
            .map_err(|_| HookError::timeout(self.config.connect_timeout_ms))?
            .map_err(|e| HookError::connection_error(format!("Failed to connect: {}", e)))?;

        let (read_half, write_half) = stream.into_split();
        self.stream = Some(Arc::new(Mutex::new(write_half)));
        self.start_response_task(read_half);

        info!("Unix socket transport connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> HookResult<()> {
        if let Some(task) = self.response_task.take() {
            task.abort();
        }

        self.stream = None;

        info!("Unix socket transport disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    async fn send_request(&mut self, request: AgentRequest) -> HookResult<AgentResponse> {
        if !self.is_connected() {
            return Err(HookError::connection_error("Not connected"));
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        let id = request.id;

        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, tx);
        }

        let msg = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: Some("agent.request".to_string()),
            params: Some(serde_json::to_value(&request).map_err(|e| {
                HookError::serialization_error(format!("Failed to serialize request: {}", e))
            })?),
            result: None,
            error: None,
        };

        let json = serde_json::to_string(&msg).map_err(|e| {
            HookError::serialization_error(format!("Failed to serialize message: {}", e))
        })?;

        {
            let stream = self
                .stream
                .as_ref()
                .ok_or_else(|| HookError::connection_error("Not connected"))?;
            let mut stream = stream.lock().await;
            stream.write_all(json.as_bytes()).await.map_err(|e| {
                HookError::connection_error(format!("Failed to write to socket: {}", e))
            })?;
            stream.write_all(b"\n").await.map_err(|e| {
                HookError::connection_error(format!("Failed to write newline: {}", e))
            })?;
            stream.flush().await.map_err(|e| {
                HookError::connection_error(format!("Failed to flush socket: {}", e))
            })?;
        }

        let timeout_duration = Duration::from_millis(self.config.request_timeout_ms);
        let result = timeout(timeout_duration, rx).await;

        match result {
            Ok(Ok(msg)) => {
                if let Some(error) = msg.error {
                    Err(HookError::agent_error(
                        "agent",
                        format!("Agent error ({}): {}", error.code, error.message),
                    ))
                } else if let Some(result) = msg.result {
                    serde_json::from_value::<AgentResponse>(result).map_err(|e| {
                        HookError::serialization_error(format!(
                            "Failed to deserialize response: {}",
                            e
                        ))
                    })
                } else {
                    Err(HookError::agent_error("agent", "Empty response from agent"))
                }
            }
            Ok(Err(_)) => Err(HookError::connection_error("Response channel closed")),
            Err(_) => {
                let mut pending = self.pending_requests.lock().await;
                pending.remove(&id);
                Err(HookError::timeout(self.config.request_timeout_ms))
            }
        }
    }

    async fn send_notification(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> HookResult<()> {
        if !self.is_connected() {
            return Err(HookError::connection_error("Not connected"));
        }

        let msg = JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: Some(method.to_string()),
            params: Some(params),
            result: None,
            error: None,
        };

        let json = serde_json::to_string(&msg).map_err(|e| {
            HookError::serialization_error(format!("Failed to serialize message: {}", e))
        })?;

        let stream = self
            .stream
            .as_ref()
            .ok_or_else(|| HookError::connection_error("Not connected"))?;
        let mut stream = stream.lock().await;
        stream.write_all(json.as_bytes()).await.map_err(|e| {
            HookError::connection_error(format!("Failed to write to socket: {}", e))
        })?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|e| HookError::connection_error(format!("Failed to write newline: {}", e)))?;
        stream
            .flush()
            .await
            .map_err(|e| HookError::connection_error(format!("Failed to flush socket: {}", e)))?;

        Ok(())
    }

    fn transport_info(&self) -> TransportInfo {
        TransportInfo {
            protocol: self.config.protocol,
            connected: self.is_connected(),
            endpoint: self
                .config
                .socket_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        }
    }
}

#[cfg(not(unix))]
pub struct UnixSocketTransport;

#[cfg(not(unix))]
impl UnixSocketTransport {
    pub fn new(_config: TransportConfig) -> Self {
        Self
    }
}

/// Factory for creating transports
pub struct TransportFactory;

impl TransportFactory {
    /// Creates a transport based on configuration
    pub fn create(config: TransportConfig) -> Box<dyn AgentTransport> {
        match config.protocol {
            ProtocolType::JsonRpcStdio | ProtocolType::CustomStdio => {
                Box::new(StdioTransport::new(config))
            }
            #[cfg(unix)]
            ProtocolType::JsonRpcUnixSocket => Box::new(UnixSocketTransport::new(config)),
            #[cfg(not(unix))]
            ProtocolType::JsonRpcUnixSocket => {
                // Unix sockets not supported on non-unix platforms
                Box::new(StdioTransport::new(config))
            }
            ProtocolType::JsonRpcTcp => {
                // TODO: Implement TCP transport
                Box::new(StdioTransport::new(config))
            }
        }
    }

    /// Creates a transport for a specific agent type
    pub fn create_for_agent(agent_type: &str) -> Option<Box<dyn AgentTransport>> {
        match agent_type {
            "claude-code" => {
                let config = TransportConfig {
                    protocol: ProtocolType::JsonRpcStdio,
                    command: Some("claude".to_string()),
                    args: vec!["--jsonrpc".to_string()],
                    ..Default::default()
                };
                Some(Box::new(StdioTransport::new(config)))
            }
            "aider" => {
                let config = TransportConfig {
                    protocol: ProtocolType::JsonRpcStdio,
                    command: Some("aider".to_string()),
                    args: vec!["--jsonrpc".to_string()],
                    ..Default::default()
                };
                Some(Box::new(StdioTransport::new(config)))
            }
            "opencode" => {
                let config = TransportConfig {
                    protocol: ProtocolType::JsonRpcStdio,
                    command: Some("opencode".to_string()),
                    args: vec!["--jsonrpc".to_string()],
                    ..Default::default()
                };
                Some(Box::new(StdioTransport::new(config)))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transport_config_default() {
        let config = TransportConfig::default();
        assert_eq!(config.protocol, ProtocolType::JsonRpcStdio);
        assert_eq!(config.connect_timeout_ms, 5000);
        assert_eq!(config.request_timeout_ms, 30000);
    }

    #[test]
    fn test_protocol_type_display() {
        assert_eq!(ProtocolType::JsonRpcStdio.to_string(), "jsonrpc-stdio");
        assert_eq!(ProtocolType::JsonRpcUnixSocket.to_string(), "jsonrpc-unix");
    }

    #[tokio::test]
    async fn test_stdio_transport_not_connected() {
        let config = TransportConfig::default();
        let transport = StdioTransport::new(config);

        assert!(!transport.is_connected());
    }

    #[cfg(unix)]
    use tempfile::TempDir;

    #[cfg(unix)]
    /// Agente falso em sh: responde JSON-RPC conforme o conteúdo da linha.
    const FAKE_AGENT: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":"\([0-9a-f-]*\)".*/\1/p')
  case "$line" in
    *'"method":"note"'*) echo "nota recebida" >&2; echo "$line" >> "$NOTES" ;;
    *ERRO*) printf '{"jsonrpc":"2.0","id":"%s","error":{"code":-7,"message":"falhou"}}\n' "$id" ;;
    *VAZIO*) printf '{"jsonrpc":"2.0","id":"%s"}\n' "$id" ;;
    *MALFORMADO*) printf '{"jsonrpc":"2.0","id":"%s","result":{"x":1}}\n' "$id" ;;
    *SILENCIO*) : ;;
    *) printf '\n'; echo 'isto nao e json';
       printf '{"jsonrpc":"2.0","id":"%s","result":{"request_id":"%s","success":true,"text":"ok","processing_time_ms":3}}\n' "$id" "$id" ;;
  esac
done
"#;

    #[cfg(unix)]
    fn stdio_config(dir: &TempDir, script: &str, timeout_ms: u64) -> TransportConfig {
        let mut env = std::collections::HashMap::new();
        env.insert(
            "NOTES".to_string(),
            dir.path().join("notes.log").to_string_lossy().to_string(),
        );
        TransportConfig {
            protocol: ProtocolType::JsonRpcStdio,
            command: Some("sh".to_string()),
            args: vec!["-c".to_string(), script.to_string()],
            cwd: Some(dir.path().to_path_buf()),
            env,
            request_timeout_ms: timeout_ms,
            ..Default::default()
        }
    }

    #[test]
    fn test_protocol_type_display_all() {
        assert_eq!(ProtocolType::JsonRpcTcp.to_string(), "jsonrpc-tcp");
        assert_eq!(ProtocolType::CustomStdio.to_string(), "custom-stdio");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_stdio_transport_round_trip_with_fake_agent() {
        let dir = TempDir::new().unwrap();
        let mut t = StdioTransport::new(stdio_config(&dir, FAKE_AGENT, 5000));
        t.connect().await.unwrap();
        assert!(t.is_connected());
        // conectar de novo é no-op
        t.connect().await.unwrap();
        let info = t.transport_info();
        assert!(info.connected);
        assert_eq!(info.endpoint, "sh");
        assert_eq!(info.protocol, ProtocolType::JsonRpcStdio);

        let req = AgentRequest::execute_command("ls");
        let id = req.id;
        let resp = t.send_request(req).await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.request_id, id);
        assert_eq!(resp.processing_time_ms, 3);

        let err = t
            .send_request(AgentRequest::execute_command("ERRO"))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, HookError::AgentError { message, .. } if message == "Agent error (-7): falhou")
        );

        let err = t
            .send_request(AgentRequest::execute_command("VAZIO"))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, HookError::AgentError { message, .. } if message == "Empty response from agent")
        );

        let err = t
            .send_request(AgentRequest::execute_command("MALFORMADO"))
            .await
            .unwrap_err();
        assert!(matches!(err, HookError::SerializationError(_)));

        t.send_notification("note", serde_json::json!({"k": "v"}))
            .await
            .unwrap();
        let notes = dir.path().join("notes.log");
        for _ in 0..200 {
            if notes.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let line = std::fs::read_to_string(&notes).unwrap();
        let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(v["method"], "note");
        assert_eq!(v["params"]["k"], "v");
        assert!(v.get("id").is_none());

        t.disconnect().await.unwrap();
        assert!(!t.is_connected());
        assert!(!t.transport_info().connected);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_stdio_transport_timeout_clears_pending() {
        let dir = TempDir::new().unwrap();
        let mut t = StdioTransport::new(stdio_config(&dir, FAKE_AGENT, 150));
        t.connect().await.unwrap();
        let err = t
            .send_request(AgentRequest::execute_command("SILENCIO"))
            .await
            .unwrap_err();
        assert!(matches!(err, HookError::Timeout { timeout_ms: 150 }));
        assert!(t.pending_requests.lock().await.is_empty());
        t.disconnect().await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_stdio_transport_env_and_cwd_are_applied() {
        let dir = TempDir::new().unwrap();
        let mut cfg = stdio_config(&dir, "pwd > saida.txt; echo \"$NOTES\" >> saida.txt", 1000);
        cfg.env.insert("EXTRA".into(), "1".into());
        let mut t = StdioTransport::new(cfg);
        t.connect().await.unwrap();
        let out = dir.path().join("saida.txt");
        for _ in 0..200 {
            if std::fs::read_to_string(&out).is_ok_and(|s| s.lines().count() == 2) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let text = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            std::fs::canonicalize(lines[0]).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
        assert!(lines[1].ends_with("notes.log"));

        // o processo já saiu: escrever no stdin falha
        tokio::time::sleep(Duration::from_millis(50)).await;
        let err = t
            .send_request(AgentRequest::execute_command("x"))
            .await
            .unwrap_err();
        assert!(matches!(err, HookError::CommunicationError(_)), "{err:?}");
        t.disconnect().await.unwrap();
    }

    #[tokio::test]
    async fn test_stdio_transport_connect_errors_and_not_connected() {
        let mut t = StdioTransport::new(TransportConfig::default());
        assert!(matches!(t.connect().await, Err(HookError::ConfigError(_))));
        assert_eq!(t.transport_info().endpoint, "");

        let mut t = StdioTransport::new(TransportConfig {
            command: Some("/nao/existe/agente".into()),
            ..Default::default()
        });
        assert!(matches!(
            t.connect().await,
            Err(HookError::CommunicationError(m)) if m.starts_with("Failed to spawn process")
        ));
        assert!(matches!(
            t.send_request(AgentRequest::execute_command("x")).await,
            Err(HookError::CommunicationError(m)) if m == "Not connected"
        ));
        assert!(matches!(
            t.send_notification("n", serde_json::Value::Null).await,
            Err(HookError::CommunicationError(m)) if m == "Not connected"
        ));
        // desconectar sem conexão é inofensivo
        t.disconnect().await.unwrap();
    }

    /// Servidor JSON-RPC num socket Unix: responde "agent.request" e grava
    /// notificações.
    #[cfg(unix)]
    fn unix_server(dir: &TempDir) -> (PathBuf, Arc<Mutex<Vec<serde_json::Value>>>) {
        let path = dir.path().join("agent.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let seen: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
        let seen2 = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let seen = Arc::clone(&seen2);
                tokio::spawn(async move {
                    let (r, mut w) = stream.into_split();
                    let mut lines = AsyncBufReader::new(r).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
                        seen.lock().await.push(v.clone());
                        let Some(id) = v.get("id").cloned() else {
                            continue;
                        };
                        let tag = ["ERRO", "VAZIO", "MALFORMADO", "SILENCIO"]
                            .into_iter()
                            .find(|t| line.contains(t))
                            .unwrap_or("");
                        let reply = match tag {
                            "ERRO" => serde_json::json!({"jsonrpc": "2.0", "id": id,
                                "error": {"code": 3, "message": "ruim"}}),
                            "VAZIO" => serde_json::json!({"jsonrpc": "2.0", "id": id}),
                            "MALFORMADO" => {
                                serde_json::json!({"jsonrpc": "2.0", "id": id, "result": 5})
                            }
                            "SILENCIO" => continue,
                            _ => serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                                "request_id": id, "success": true, "text": "oi",
                                "processing_time_ms": 9}}),
                        };
                        w.write_all(format!("{}\nlixo\n", reply).as_bytes())
                            .await
                            .unwrap();
                    }
                });
            }
        });
        (path, seen)
    }

    #[cfg(unix)]
    /// Falha (em vez de travar o teste) se a operação não terminar em 5s.
    async fn within<T>(fut: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(5), fut)
            .await
            .expect("operação travou")
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unix_socket_transport_round_trip_does_not_deadlock() {
        // Regressão: a task de leitura segurava o lock do stream e qualquer
        // escrita (send_request/send_notification) ficava presa para sempre
        let dir = TempDir::new().unwrap();
        let (path, seen) = unix_server(&dir);
        let mut t = UnixSocketTransport::new(TransportConfig {
            protocol: ProtocolType::JsonRpcUnixSocket,
            socket_path: Some(path.clone()),
            request_timeout_ms: 2000,
            ..Default::default()
        });
        t.connect().await.unwrap();
        t.connect().await.unwrap();
        assert!(t.is_connected());
        let info = t.transport_info();
        assert_eq!(info.endpoint, path.to_string_lossy());
        assert_eq!(info.protocol, ProtocolType::JsonRpcUnixSocket);

        let req = AgentRequest::execute_command("ls");
        let id = req.id;
        let resp = within(t.send_request(req)).await.unwrap();
        assert!(resp.success);
        assert_eq!(resp.request_id, id);

        let err = within(t.send_request(AgentRequest::execute_command("ERRO")))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, HookError::AgentError { agent, message } if agent == "agent" && message == "Agent error (3): ruim")
        );
        let err = within(t.send_request(AgentRequest::execute_command("VAZIO")))
            .await
            .unwrap_err();
        assert!(
            matches!(&err, HookError::AgentError { message, .. } if message == "Empty response from agent")
        );
        let err = within(t.send_request(AgentRequest::execute_command("MALFORMADO")))
            .await
            .unwrap_err();
        assert!(matches!(err, HookError::SerializationError(_)));

        within(t.send_notification("ping", serde_json::json!([1])))
            .await
            .unwrap();
        for _ in 0..200 {
            if seen.lock().await.iter().any(|v| v["method"] == "ping") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let seen = seen.lock().await;
        let note = seen.iter().find(|v| v["method"] == "ping").unwrap();
        assert_eq!(note["params"], serde_json::json!([1]));
        assert!(note.get("id").is_none());
        drop(seen);

        t.disconnect().await.unwrap();
        assert!(!t.is_connected());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unix_socket_transport_timeout_and_errors() {
        let dir = TempDir::new().unwrap();
        let (path, _) = unix_server(&dir);
        let mut t = UnixSocketTransport::new(TransportConfig {
            socket_path: Some(path),
            request_timeout_ms: 100,
            ..Default::default()
        });
        t.connect().await.unwrap();
        let err = t
            .send_request(AgentRequest::execute_command("SILENCIO"))
            .await
            .unwrap_err();
        assert!(matches!(err, HookError::Timeout { timeout_ms: 100 }));
        assert!(t.pending_requests.lock().await.is_empty());
        t.disconnect().await.unwrap();
        assert!(matches!(
            t.send_request(AgentRequest::execute_command("x")).await,
            Err(HookError::CommunicationError(m)) if m == "Not connected"
        ));
        assert!(matches!(
            t.send_notification("n", serde_json::Value::Null).await,
            Err(HookError::CommunicationError(_))
        ));

        let mut none = UnixSocketTransport::new(TransportConfig::default());
        assert!(matches!(
            none.connect().await,
            Err(HookError::ConfigError(_))
        ));
        assert_eq!(none.transport_info().endpoint, "");

        let mut missing = UnixSocketTransport::new(TransportConfig {
            socket_path: Some(dir.path().join("nada.sock")),
            ..Default::default()
        });
        assert!(matches!(
            missing.connect().await,
            Err(HookError::CommunicationError(m)) if m.starts_with("Failed to connect")
        ));
    }

    #[test]
    fn test_factory_builds_transports() {
        for protocol in [
            ProtocolType::JsonRpcStdio,
            ProtocolType::CustomStdio,
            ProtocolType::JsonRpcUnixSocket,
            ProtocolType::JsonRpcTcp,
        ] {
            let t = TransportFactory::create(TransportConfig {
                protocol,
                command: Some("agente".into()),
                ..Default::default()
            });
            assert!(!t.is_connected());
            assert_eq!(t.transport_info().protocol, protocol);
        }

        for (name, cmd) in [
            ("claude-code", "claude"),
            ("aider", "aider"),
            ("opencode", "opencode"),
        ] {
            let t = TransportFactory::create_for_agent(name).unwrap();
            let info = t.transport_info();
            assert_eq!(info.endpoint, cmd);
            assert_eq!(info.protocol, ProtocolType::JsonRpcStdio);
            assert!(!info.connected);
        }
        assert!(TransportFactory::create_for_agent("desconhecido").is_none());
    }
}
