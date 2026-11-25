//! Servidor de socket remoto para controle externo do terminal
//!
//! Suporta Unix domain sockets (Linux/macOS) e TCP sockets (cross-platform).

use std::sync::Arc;

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio::time::Duration;

#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

use crate::remote::auth::{AuthConfig, Credentials};
use crate::remote::config::{ListenAddress, RemoteConfig};
use crate::remote::connection::{ConnectionInfo, ConnectionManager};
use crate::remote::handler::RpcHandler;
use crate::remote::protocol::{error_codes, Id, Message, Request, Response, RpcError};
use crate::remote::RemoteError;

/// Estado do servidor remoto
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error,
}

impl ServerState {
    /// Verificar se o servidor pode ser iniciado
    pub fn can_start(&self) -> bool {
        matches!(self, ServerState::Stopped | ServerState::Error)
    }

    /// Verificar se o servidor está rodando
    pub fn is_running(&self) -> bool {
        matches!(self, ServerState::Running)
    }
}

impl std::fmt::Display for ServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerState::Stopped => write!(f, "stopped"),
            ServerState::Starting => write!(f, "starting"),
            ServerState::Running => write!(f, "running"),
            ServerState::Stopping => write!(f, "stopping"),
            ServerState::Error => write!(f, "error"),
        }
    }
}

/// Servidor remoto para controle via socket
#[derive(Debug)]
pub struct RemoteServer {
    config: RemoteConfig,
    state: Arc<RwLock<ServerState>>,
    connection_manager: Arc<ConnectionManager>,
    handler: Arc<RpcHandler>,
    auth_required: bool,
    shutdown_tx: broadcast::Sender<()>,
    local_addr: Arc<RwLock<Option<ListenAddress>>>,
}

impl RemoteServer {
    /// Criar novo servidor com configuração
    pub fn new(config: RemoteConfig) -> Result<Self, RemoteError> {
        config
            .validate()
            .map_err(|e| RemoteError::Config(e.to_string()))?;

        if !config.enabled {
            return Err(RemoteError::Disabled);
        }

        let (shutdown_tx, _) = broadcast::channel(1);
        let auth_config = Self::auth_config_from_remote(&config);
        let auth_required = auth_config.enabled;

        Ok(Self {
            connection_manager: Arc::new(ConnectionManager::new(config.max_connections)),
            handler: Arc::new(RpcHandler::with_auth(auth_config)),
            auth_required,
            config,
            state: Arc::new(RwLock::new(ServerState::Stopped)),
            shutdown_tx,
            local_addr: Arc::new(RwLock::new(None)),
        })
    }

    /// Retornar estado atual
    pub async fn state(&self) -> ServerState {
        *self.state.read().await
    }

    /// Retornar endereço local (após bind)
    pub async fn local_addr(&self) -> Option<ListenAddress> {
        self.local_addr.read().await.clone()
    }

    /// Número de conexões ativas
    pub fn connection_count(&self) -> usize {
        self.connection_manager.connection_count()
    }

    /// Verificar se há conexões ativas
    pub fn has_connections(&self) -> bool {
        self.connection_manager.has_connections()
    }

    /// Acessar o handler JSON-RPC para registrar métodos
    pub fn handler(&self) -> &Arc<RpcHandler> {
        &self.handler
    }

    fn auth_config_from_remote(config: &RemoteConfig) -> AuthConfig {
        AuthConfig {
            enabled: config.auth_token.is_some(),
            token: config.auth_token.clone(),
            allowed_ips: None,
            blocked_ips: None,
            allow_localhost: false,
        }
    }

    async fn process_authenticated_json(
        handler: Arc<RpcHandler>,
        auth_required: bool,
        msg: &str,
        client_ip: Option<std::net::IpAddr>,
    ) -> Option<String> {
        if !auth_required {
            return handler.process_json(msg).await;
        }

        let message = match Message::from_str(msg) {
            Ok(message) => message,
            Err(error) => {
                return serde_json::to_string(&Response::from_error(None, error)).ok();
            }
        };

        let Some(credentials) = credentials_from_message(&message, client_ip) else {
            let id = message_id(&message);
            return auth_error_response(id, "Authentication required");
        };

        match handler.authenticate(&credentials, client_ip).await {
            Ok(auth_context) if auth_context.is_authenticated() => handler
                .handle_message(message_without_auth(message))
                .await
                .and_then(|m| m.to_json_string().ok()),
            Ok(_) => {
                let id = message_id(&message);
                auth_error_response(id, "Authentication required")
            }
            Err(error) => {
                let id = message_id(&message);
                auth_error_response(id, error.to_string())
            }
        }
    }

    /// Iniciar servidor (bloqueante)
    pub async fn run(&self) -> Result<(), RemoteError> {
        // Verificar estado
        {
            let state = self.state.read().await;
            if !state.can_start() {
                return Err(RemoteError::InvalidState(state.to_string()));
            }
        }

        // Atualizar estado
        *self.state.write().await = ServerState::Starting;
        tracing::info!("Starting remote server...");

        // Iniciar servidor conforme configuração
        let result = match &self.config.socket_path {
            Some(path) => self.run_unix(path).await,
            None => match self.config.tcp_addr {
                Some(addr) => self.run_tcp(addr).await,
                None => Err(RemoteError::Config(
                    "No listen address configured".to_string(),
                )),
            },
        };

        if let Err(ref e) = result {
            *self.state.write().await = ServerState::Error;
            tracing::error!("Remote server error: {}", e);
        } else {
            *self.state.write().await = ServerState::Stopped;
            tracing::info!("Remote server stopped");
        }

        result
    }

    /// Iniciar servidor TCP
    async fn run_tcp(&self, addr: std::net::SocketAddr) -> Result<(), RemoteError> {
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| RemoteError::Bind(e.to_string()))?;

        let local_addr = listener
            .local_addr()
            .map_err(|e| RemoteError::Bind(e.to_string()))?;

        // Assina o shutdown antes de anunciar Running: um shutdown() logo após
        // ver o estado Running não pode se perder sem receptor.
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        *self.local_addr.write().await = Some(ListenAddress::Tcp(local_addr));
        *self.state.write().await = ServerState::Running;

        tracing::info!("Remote server listening on tcp://{}", local_addr);

        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            tracing::debug!("New TCP connection from {}", peer_addr);
                            self.handle_tcp_stream(stream, idle_timeout).await;
                        }
                        Err(e) => {
                            tracing::error!("Accept error: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    tracing::info!("Shutdown signal received");
                    break;
                }
            }
        }

        // Graceful shutdown
        self.shutdown_connections().await;
        Ok(())
    }

    /// Iniciar servidor Unix socket
    #[cfg(unix)]
    async fn run_unix(&self, path: &std::path::Path) -> Result<(), RemoteError> {
        // Remover socket antigo se existir
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| RemoteError::Bind(format!("Failed to remove old socket: {}", e)))?;
        }

        let listener = UnixListener::bind(path).map_err(|e| RemoteError::Bind(e.to_string()))?;

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        *self.local_addr.write().await = Some(ListenAddress::Unix(path.to_path_buf()));
        *self.state.write().await = ServerState::Running;

        tracing::info!("Remote server listening on unix:{}", path.display());

        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            tracing::debug!("New Unix socket connection");
                            self.handle_unix_stream(stream, idle_timeout).await;
                        }
                        Err(e) => {
                            tracing::error!("Accept error: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    tracing::info!("Shutdown signal received");
                    break;
                }
            }
        }

        // Cleanup
        self.shutdown_connections().await;

        // Remover socket file
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        Ok(())
    }

    /// Versão não-Unix (stub)
    #[cfg(not(unix))]
    async fn run_unix(&self, _path: &std::path::Path) -> Result<(), RemoteError> {
        Err(RemoteError::Config(
            "Unix sockets not supported on this platform".to_string(),
        ))
    }

    /// Processar stream TCP
    async fn handle_tcp_stream(&self, stream: TcpStream, idle_timeout: Duration) {
        let cm = self.connection_manager.clone();
        let handler = self.handler.clone();
        let auth_required = self.auth_required;
        let client_ip = stream.peer_addr().ok().map(|addr| addr.ip());

        let on_message = move |msg: String| {
            let handler = handler.clone();
            let client_ip = client_ip;
            async move {
                tracing::debug!("Received: {}", msg);
                Self::process_authenticated_json(handler, auth_required, &msg, client_ip).await
            }
        };

        match cm
            .handle_tcp_connection(stream, idle_timeout, on_message)
            .await
        {
            Ok(info) => {
                tracing::info!("TCP connection {} established", info.id);
            }
            Err(e) => {
                tracing::warn!("Failed to handle TCP connection: {}", e);
            }
        }
    }

    /// Processar stream Unix
    #[cfg(unix)]
    async fn handle_unix_stream(&self, stream: UnixStream, idle_timeout: Duration) {
        let cm = self.connection_manager.clone();
        let handler = self.handler.clone();
        let auth_required = self.auth_required;

        let on_message = move |msg: String| {
            let handler = handler.clone();
            async move {
                tracing::debug!("Received: {}", msg);
                Self::process_authenticated_json(handler, auth_required, &msg, None).await
            }
        };

        match cm
            .handle_unix_connection(stream, idle_timeout, on_message)
            .await
        {
            Ok(info) => {
                tracing::info!("Unix connection {} established", info.id);
            }
            Err(e) => {
                tracing::warn!("Failed to handle Unix connection: {}", e);
            }
        }
    }

    /// Desconectar todas as conexões
    async fn shutdown_connections(&self) {
        *self.state.write().await = ServerState::Stopping;

        if self.has_connections() {
            tracing::info!("Closing {} connections...", self.connection_count());
            self.connection_manager.shutdown_all().await;
        }
    }

    /// Sinalizar shutdown para o servidor
    pub fn shutdown(&self) -> Result<(), RemoteError> {
        let state = *self.state.blocking_read();
        if !state.is_running() {
            return Err(RemoteError::InvalidState(state.to_string()));
        }

        tracing::info!("Sending shutdown signal to remote server");
        self.shutdown_tx
            .send(())
            .map_err(|_| RemoteError::Shutdown("Failed to send shutdown signal".to_string()))?;

        Ok(())
    }

    /// Listar conexões ativas
    pub fn list_connections(&self) -> Vec<ConnectionInfo> {
        self.connection_manager.list_connections()
    }
}

fn message_id(message: &Message) -> Option<Id> {
    match message {
        Message::Request(request) => request.id.clone(),
        Message::BatchRequest(requests) => requests.first().and_then(|request| request.id.clone()),
        Message::Response(response) => response.id.clone(),
        Message::BatchResponse(responses) => {
            responses.first().and_then(|response| response.id.clone())
        }
    }
}

fn credentials_from_message(
    message: &Message,
    client_ip: Option<std::net::IpAddr>,
) -> Option<Credentials> {
    match message {
        Message::Request(request) => credentials_from_request(request, client_ip),
        Message::BatchRequest(requests) => requests
            .iter()
            .find_map(|request| credentials_from_request(request, client_ip)),
        Message::Response(_) | Message::BatchResponse(_) => None,
    }
}

fn credentials_from_request(
    request: &Request,
    client_ip: Option<std::net::IpAddr>,
) -> Option<Credentials> {
    let token = request
        .params
        .as_ref()
        .and_then(extract_token_from_value)
        .or_else(|| {
            request
                .params
                .as_ref()
                .and_then(|params| params.get("auth").and_then(extract_token_from_value))
        });

    token.map(|token| Credentials {
        token: Some(token),
        client_ip: client_ip.map(|ip| ip.to_string()),
    })
}

fn extract_token_from_value(value: &serde_json::Value) -> Option<String> {
    value
        .get("token")
        .and_then(|token| token.as_str())
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            value
                .get("auth")
                .and_then(|auth| auth.get("token"))
                .and_then(|token| token.as_str())
                .filter(|token| !token.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn message_without_auth(message: Message) -> Message {
    match message {
        Message::Request(request) => Message::Request(request_without_auth(request)),
        Message::BatchRequest(requests) => {
            Message::BatchRequest(requests.into_iter().map(request_without_auth).collect())
        }
        other => other,
    }
}

fn request_without_auth(mut request: Request) -> Request {
    if let Some(params) = request.params.take() {
        request.params = Some(strip_auth_fields(params));
    }
    request
}

fn strip_auth_fields(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(object) = value.as_object_mut() {
        object.remove("auth");
        object.remove("token");
    }
    value
}

fn auth_error_response(id: Option<Id>, message: impl Into<String>) -> Option<String> {
    serde_json::to_string(&Response::from_error(
        id,
        RpcError::server_error(error_codes::UNAUTHORIZED, message, None),
    ))
    .ok()
}

/// Builder para criar servidor com opções customizadas
#[derive(Debug, Default)]
pub struct RemoteServerBuilder {
    config: Option<RemoteConfig>,
}

impl RemoteServerBuilder {
    /// Criar novo builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Definir configuração
    pub fn config(mut self, config: RemoteConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Construir servidor
    pub fn build(self) -> Result<RemoteServer, RemoteError> {
        let config = self
            .config
            .ok_or_else(|| RemoteError::Config("No configuration provided".to_string()))?;

        RemoteServer::new(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_state_display() {
        assert_eq!(ServerState::Stopped.to_string(), "stopped");
        assert_eq!(ServerState::Running.to_string(), "running");
        assert_eq!(ServerState::Error.to_string(), "error");
    }

    #[test]
    fn test_server_state_can_start() {
        assert!(ServerState::Stopped.can_start());
        assert!(ServerState::Error.can_start());
        assert!(!ServerState::Running.can_start());
        assert!(!ServerState::Starting.can_start());
    }

    #[test]
    fn test_server_state_is_running() {
        assert!(ServerState::Running.is_running());
        assert!(!ServerState::Stopped.is_running());
        assert!(!ServerState::Starting.is_running());
    }

    #[tokio::test]
    async fn test_server_creation_disabled() {
        let config = RemoteConfig {
            enabled: false,
            ..Default::default()
        };

        let result = RemoteServer::new(config);
        assert!(matches!(result, Err(RemoteError::Disabled)));
    }

    #[tokio::test]
    async fn test_server_creation_no_address() {
        let config = RemoteConfig {
            enabled: true,
            socket_path: None,
            tcp_addr: None,
            ..Default::default()
        };

        let result = RemoteServer::new(config);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_server_creation_ok() {
        let config = RemoteConfig::for_testing();
        let server = RemoteServer::new(config);
        assert!(server.is_ok());
    }

    #[test]
    fn test_server_builder() {
        let config = RemoteConfig::for_testing();
        let server = RemoteServerBuilder::new().config(config).build();

        assert!(server.is_ok());
    }

    #[test]
    fn test_server_builder_no_config() {
        let result = RemoteServerBuilder::new().build();
        assert!(matches!(result, Err(RemoteError::Config(_))));
    }

    #[tokio::test]
    async fn test_process_authenticated_json_requires_token() {
        let config = RemoteConfig {
            auth_token: Some("secret".to_string()),
            ..RemoteConfig::for_testing()
        };
        let server = RemoteServer::new(config).unwrap();
        server
            .handler()
            .register("ping", |_params| async { Ok(serde_json::json!("pong")) })
            .await;

        let response = RemoteServer::process_authenticated_json(
            server.handler.clone(),
            server.auth_required,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{}}"#,
            Some("127.0.0.1".parse().unwrap()),
        )
        .await
        .unwrap();

        assert!(response.contains("-32003"));
    }

    #[tokio::test]
    async fn test_process_authenticated_json_accepts_valid_token() {
        let config = RemoteConfig {
            auth_token: Some("secret".to_string()),
            ..RemoteConfig::for_testing()
        };
        let server = RemoteServer::new(config).unwrap();
        server
            .handler()
            .register("echo", |params| async move { Ok(params) })
            .await;

        let response = RemoteServer::process_authenticated_json(
            server.handler.clone(),
            server.auth_required,
            r#"{"jsonrpc":"2.0","id":1,"method":"echo","params":{"token":"secret","value":42}}"#,
            Some("127.0.0.1".parse().unwrap()),
        )
        .await
        .unwrap();

        assert!(response.contains("\"value\":42"));
        assert!(!response.contains("secret"));
    }

    #[tokio::test]
    async fn test_process_authenticated_json_rejects_invalid_token() {
        let config = RemoteConfig {
            auth_token: Some("secret".to_string()),
            ..RemoteConfig::for_testing()
        };
        let server = RemoteServer::new(config).unwrap();
        server
            .handler()
            .register("ping", |_params| async { Ok(serde_json::json!("pong")) })
            .await;

        let response = RemoteServer::process_authenticated_json(
            server.handler.clone(),
            server.auth_required,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"auth":{"token":"wrong"}}}"#,
            Some("127.0.0.1".parse().unwrap()),
        )
        .await
        .unwrap();

        assert!(response.contains("-32003"));
        assert!(response.contains("Invalid authentication token"));
    }

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    async fn wait_for_state(server: &RemoteServer, expected: ServerState) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while server.state().await != expected {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("estado esperado não foi atingido");
    }

    #[cfg(unix)]
    async fn wait_for_connections(server: &RemoteServer, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while server.connection_count() != count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("número de conexões esperado não foi atingido");
    }

    /// `shutdown()` usa `blocking_read`, então roda fora do executor.
    async fn shutdown_from_blocking_thread(server: Arc<RemoteServer>) -> Result<(), RemoteError> {
        tokio::task::spawn_blocking(move || server.shutdown())
            .await
            .expect("thread de shutdown")
    }

    async fn send_line<W, R>(writer: &mut W, reader: &mut BufReader<R>, json: &str) -> String
    where
        W: tokio::io::AsyncWrite + Unpin,
        R: tokio::io::AsyncRead + Unpin,
    {
        writer.write_all(json.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        line
    }

    #[test]
    fn test_server_state_display_all_variants() {
        assert_eq!(ServerState::Starting.to_string(), "starting");
        assert_eq!(ServerState::Stopping.to_string(), "stopping");
        assert_eq!(ServerState::Error.to_string(), "error");
    }

    #[test]
    fn test_shutdown_requires_running_server() {
        let server = RemoteServer::new(RemoteConfig::for_testing()).unwrap();
        match server.shutdown() {
            Err(RemoteError::InvalidState(state)) => assert_eq!(state, "stopped"),
            other => panic!("esperava InvalidState, veio {:?}", other),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_tcp_server_lifecycle_with_auth() {
        let config = RemoteConfig {
            auth_token: Some("segredo".to_string()),
            ..RemoteConfig::for_testing()
        };
        let server = Arc::new(RemoteServer::new(config).unwrap());
        server
            .handler()
            .register("echo", |params| async move { Ok(params) })
            .await;
        assert_eq!(server.state().await, ServerState::Stopped);
        assert!(server.local_addr().await.is_none());

        let running = tokio::spawn({
            let server = server.clone();
            async move { server.run().await }
        });
        wait_for_state(&server, ServerState::Running).await;

        // Rodar de novo enquanto está ativo é recusado
        match server.run().await {
            Err(RemoteError::InvalidState(state)) => assert_eq!(state, "running"),
            other => panic!("esperava InvalidState, veio {:?}", other),
        }

        let Some(ListenAddress::Tcp(addr)) = server.local_addr().await else {
            panic!("esperava endereço TCP");
        };
        assert_ne!(addr.port(), 0);

        let stream = TcpStream::connect(addr).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let denied = send_line(
            &mut write_half,
            &mut reader,
            r#"{"jsonrpc":"2.0","id":1,"method":"echo","params":{"v":1}}"#,
        )
        .await;
        assert!(denied.contains("-32003"), "{denied}");
        assert!(denied.contains("Authentication required"), "{denied}");

        let accepted = send_line(
            &mut write_half,
            &mut reader,
            r#"{"jsonrpc":"2.0","id":2,"method":"echo","params":{"token":"segredo","v":2}}"#,
        )
        .await;
        assert!(accepted.contains("\"v\":2"), "{accepted}");
        assert!(!accepted.contains("segredo"), "{accepted}");

        assert_eq!(server.connection_count(), 1);
        assert!(server.has_connections());
        assert_eq!(server.list_connections().len(), 1);

        shutdown_from_blocking_thread(server.clone()).await.unwrap();
        running.await.unwrap().unwrap();
        assert_eq!(server.state().await, ServerState::Stopped);
        assert!(!server.has_connections());

        // O cliente vê o socket fechado
        let mut rest = String::new();
        assert_eq!(reader.read_line(&mut rest).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_tcp_server_reports_bind_error() {
        let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let config = RemoteConfig {
            tcp_addr: Some(taken.local_addr().unwrap()),
            ..RemoteConfig::for_testing()
        };
        let server = RemoteServer::new(config).unwrap();
        assert!(matches!(server.run().await, Err(RemoteError::Bind(_))));
        assert_eq!(server.state().await, ServerState::Error);
        // Depois de erro o servidor pode tentar de novo
        assert!(server.state().await.can_start());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_unix_server_lifecycle_replaces_stale_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("remote.sock");
        std::fs::write(&path, b"stale").unwrap();

        let config = RemoteConfig {
            socket_path: Some(path.clone()),
            ..RemoteConfig::for_testing()
        };
        let server = Arc::new(RemoteServer::new(config).unwrap());
        server
            .handler()
            .register("ping", |_params| async { Ok(serde_json::json!("pong")) })
            .await;

        let running = tokio::spawn({
            let server = server.clone();
            async move { server.run().await }
        });
        wait_for_state(&server, ServerState::Running).await;
        assert_eq!(
            server.local_addr().await,
            Some(ListenAddress::Unix(path.clone()))
        );

        let stream = UnixStream::connect(&path).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let pong = send_line(
            &mut write_half,
            &mut reader,
            r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
        )
        .await;
        assert!(pong.contains("\"pong\""), "{pong}");
        wait_for_connections(&server, 1).await;

        shutdown_from_blocking_thread(server.clone()).await.unwrap();
        running.await.unwrap().unwrap();
        assert_eq!(server.state().await, ServerState::Stopped);
        assert!(!path.exists(), "socket deveria ser removido ao parar");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unix_server_reports_bind_error() {
        let dir = tempfile::tempdir().unwrap();
        let config = RemoteConfig {
            socket_path: Some(dir.path().join("nao-existe").join("remote.sock")),
            ..RemoteConfig::for_testing()
        };
        let server = RemoteServer::new(config).unwrap();
        assert!(matches!(server.run().await, Err(RemoteError::Bind(_))));
        assert_eq!(server.state().await, ServerState::Error);
    }

    async fn auth_server() -> RemoteServer {
        let config = RemoteConfig {
            auth_token: Some("segredo".to_string()),
            ..RemoteConfig::for_testing()
        };
        let server = RemoteServer::new(config).unwrap();
        server
            .handler()
            .register("echo", |params| async move { Ok(params) })
            .await;
        server
    }

    async fn process(server: &RemoteServer, json: &str) -> String {
        RemoteServer::process_authenticated_json(
            server.handler.clone(),
            server.auth_required,
            json,
            None,
        )
        .await
        .unwrap_or_default()
    }

    #[tokio::test]
    async fn test_process_authenticated_json_parse_error_and_responses() {
        let server = auth_server().await;
        let parse_error = process(&server, "{nao é json").await;
        assert!(parse_error.contains("-32700"), "{parse_error}");

        // Uma resposta não tem credenciais: exige autenticação
        let response = process(&server, r#"{"jsonrpc":"2.0","id":5,"result":1}"#).await;
        assert!(response.contains("-32003"), "{response}");
        assert!(response.contains("\"id\":5"), "{response}");
    }

    #[tokio::test]
    async fn test_process_authenticated_json_batch_uses_any_token_and_strips_it() {
        let server = auth_server().await;
        let batch = r#"[
            {"jsonrpc":"2.0","id":1,"method":"echo","params":{"a":1}},
            {"jsonrpc":"2.0","id":2,"method":"echo","params":{"auth":{"token":"segredo"},"b":2}}
        ]"#;
        let reply = process(&server, &batch.replace('\n', " ")).await;
        assert!(reply.starts_with('['), "{reply}");
        assert!(
            reply.contains("\"a\":1") && reply.contains("\"b\":2"),
            "{reply}"
        );
        assert!(!reply.contains("segredo"), "{reply}");

        // Batch sem token: o erro usa o id do primeiro item
        let denied = process(
            &server,
            r#"[{"jsonrpc":"2.0","id":9,"method":"echo","params":{}}]"#,
        )
        .await;
        assert!(
            denied.contains("-32003") && denied.contains("\"id\":9"),
            "{denied}"
        );
    }

    #[test]
    fn test_message_helpers() {
        let response = Response::success(Id::Num(3), "ok").unwrap();
        assert_eq!(
            message_id(&Message::Response(response.clone())),
            Some(Id::Num(3))
        );
        assert_eq!(
            message_id(&Message::BatchResponse(vec![response.clone()])),
            Some(Id::Num(3))
        );
        assert_eq!(message_id(&Message::BatchRequest(vec![])), None);
        assert!(
            credentials_from_message(&Message::BatchResponse(vec![response.clone()]), None)
                .is_none()
        );
        assert!(matches!(
            message_without_auth(Message::Response(response)),
            Message::Response(_)
        ));

        // Token vazio é ignorado; "auth.token" aninhado em params.auth também vale
        assert_eq!(
            extract_token_from_value(&serde_json::json!({"token": ""})),
            None
        );
        let request = Request {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "m".to_string(),
            params: Some(serde_json::json!({"auth": {"auth": {"token": "t"}}})),
        };
        let credentials =
            credentials_from_request(&request, Some("10.0.0.1".parse().unwrap())).unwrap();
        assert_eq!(credentials.token.as_deref(), Some("t"));
        assert_eq!(credentials.client_ip.as_deref(), Some("10.0.0.1"));

        assert_eq!(
            strip_auth_fields(serde_json::json!([1, 2])),
            serde_json::json!([1, 2])
        );
    }
}
