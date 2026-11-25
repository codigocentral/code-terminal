//! Servidor da Socket API

use super::{
    ApiError, ApiResult, JsonRpcRequest, JsonRpcResponse, KeyboardApi, PaneApi, SessionRendererApi,
    StateApi, TabApi, WorkspaceApi,
};
use crate::config::Config;
use parking_lot::RwLock;
use std::io::{BufRead, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, trace};

#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

/// Configuração do servidor de API
#[derive(Debug, Clone)]
pub struct ApiServerConfig {
    /// Endereço TCP (ex: "127.0.0.1:9123")
    pub tcp_addr: Option<String>,
    /// Caminho do Unix socket (ex: "/tmp/codeterm.sock")
    pub unix_socket_path: Option<String>,
    /// Timeout de conexão em segundos
    pub connection_timeout: u64,
    /// Máximo de conexões simultâneas
    pub max_connections: usize,
}

/// Caminho padrão do socket da API: `$XDG_RUNTIME_DIR/code-terminal.sock`
/// (ou `/tmp`). É o mesmo caminho que o `code-terminal-cli` procura.
#[cfg(unix)]
pub fn default_unix_socket_path() -> std::path::PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("code-terminal.sock")
}

impl Default for ApiServerConfig {
    /// No Unix a API escuta só num socket com permissão `0600`, para que outros
    /// usuários da máquina não mandem comandos ao terminal. No Windows,
    /// sem socket Unix, fica em TCP local.
    fn default() -> Self {
        #[cfg(unix)]
        let (tcp_addr, unix_socket_path) = (
            None,
            Some(default_unix_socket_path().to_string_lossy().to_string()),
        );
        #[cfg(not(unix))]
        let (tcp_addr, unix_socket_path) = (Some("127.0.0.1:9123".to_string()), None);
        Self {
            tcp_addr,
            unix_socket_path,
            connection_timeout: 300,
            max_connections: 100,
        }
    }
}

impl ApiServerConfig {
    /// Cria configuração com Unix socket (Unix/Linux)
    #[cfg(unix)]
    pub fn with_unix_socket(path: impl AsRef<Path>) -> Self {
        Self {
            tcp_addr: None,
            unix_socket_path: Some(path.as_ref().to_string_lossy().to_string()),
            ..Default::default()
        }
    }

    /// Cria configuração com TCP
    pub fn with_tcp(addr: impl Into<String>) -> Self {
        Self {
            tcp_addr: Some(addr.into()),
            unix_socket_path: None,
            ..Default::default()
        }
    }
}

/// Servidor da Socket API
pub struct ApiServer {
    config: ApiServerConfig,
    workspace_api: Arc<WorkspaceApi>,
    tab_api: Arc<TabApi>,
    pane_api: Arc<PaneApi>,
    keyboard_api: Arc<KeyboardApi>,
    state_api: Arc<StateApi>,
    session_renderer_api: Arc<SessionRendererApi>,
    running: Arc<RwLock<bool>>,
}

impl ApiServer {
    /// Cria um novo servidor
    pub fn new(config: ApiServerConfig, app_config: Arc<RwLock<Config>>) -> Self {
        let workspace_api = Arc::new(WorkspaceApi::new());
        let tab_api = Arc::new(TabApi::new());
        let pane_api = Arc::new(PaneApi::new());
        let keyboard_api = Arc::new(KeyboardApi::new());
        let state_api = Arc::new(StateApi::new(app_config));
        let session_renderer_api = Arc::new(SessionRendererApi::new());

        Self {
            config,
            workspace_api,
            tab_api,
            pane_api,
            keyboard_api,
            state_api,
            session_renderer_api,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Inicia o servidor
    pub fn start(&self) -> ApiResult<()> {
        let mut running = self.running.write();
        if *running {
            return Err(ApiError::InvalidState("Server already running".to_string()));
        }
        *running = true;
        drop(running);

        info!("Starting Socket API server...");

        if let Err(error) = self.start_listeners() {
            // Sem listener o servidor não está rodando; sem isso um bind que
            // falhou deixava `is_running() == true` e bloqueava novo start.
            *self.running.write() = false;
            return Err(error);
        }

        info!("Socket API server started");
        Ok(())
    }

    fn start_listeners(&self) -> ApiResult<()> {
        // Inicia servidor TCP se configurado
        if let Some(ref addr) = self.config.tcp_addr {
            self.start_tcp_server(addr.clone())?;
        }

        // Inicia servidor Unix se configurado
        #[cfg(unix)]
        if let Some(ref path) = self.config.unix_socket_path {
            self.start_unix_server(path.clone())?;
        }

        Ok(())
    }

    /// Loop do servidor TCP (método associado para evitar capturar self)
    fn tcp_server_loop(
        listener: TcpListener,
        running: Arc<RwLock<bool>>,
        workspace_api: Arc<WorkspaceApi>,
        tab_api: Arc<TabApi>,
        pane_api: Arc<PaneApi>,
        keyboard_api: Arc<KeyboardApi>,
        state_api: Arc<StateApi>,
        session_renderer_api: Arc<SessionRendererApi>,
    ) {
        for stream in listener.incoming() {
            if !*running.read() {
                break;
            }

            match stream {
                Ok(stream) => {
                    let peer_addr = stream.peer_addr().ok();
                    debug!("New TCP connection from {:?}", peer_addr);

                    let workspace_api = Arc::clone(&workspace_api);
                    let tab_api = Arc::clone(&tab_api);
                    let pane_api = Arc::clone(&pane_api);
                    let keyboard_api = Arc::clone(&keyboard_api);
                    let state_api = Arc::clone(&state_api);
                    let session_renderer_api = Arc::clone(&session_renderer_api);

                    let _ = crate::utils::spawn_named("socket-api-conn", move || {
                        handle_connection(
                            stream,
                            workspace_api,
                            tab_api,
                            pane_api,
                            keyboard_api,
                            state_api,
                            session_renderer_api,
                        );
                    });
                }
                Err(e) => {
                    error!("TCP connection error: {}", e);
                }
            }
        }
    }

    /// Inicia servidor TCP
    fn start_tcp_server(&self, addr: String) -> ApiResult<()> {
        let listener = TcpListener::bind(&addr)
            .map_err(|e| ApiError::InternalError(format!("Failed to bind TCP: {}", e)))?;

        info!("TCP server listening on {}", addr);

        let running = Arc::clone(&self.running);
        let workspace_api = Arc::clone(&self.workspace_api);
        let tab_api = Arc::clone(&self.tab_api);
        let pane_api = Arc::clone(&self.pane_api);
        let keyboard_api = Arc::clone(&self.keyboard_api);
        let state_api = Arc::clone(&self.state_api);
        let session_renderer_api = Arc::clone(&self.session_renderer_api);

        // Spawn a thread that doesn't capture self
        let _ = crate::utils::spawn_named("socket-api-tcp", move || {
            Self::tcp_server_loop(
                listener,
                running,
                workspace_api,
                tab_api,
                pane_api,
                keyboard_api,
                state_api,
                session_renderer_api,
            );
        });

        Ok(())
    }

    /// Inicia servidor Unix socket
    #[cfg(unix)]
    fn start_unix_server(&self, path: String) -> ApiResult<()> {
        use std::os::unix::fs::PermissionsExt;

        // Socket de outra instância viva: não roubamos o caminho dela.
        if UnixStream::connect(&path).is_ok() {
            return Err(ApiError::InternalError(format!(
                "Another instance is already listening on {}",
                path
            )));
        }
        // Socket órfão de uma execução anterior.
        let _ = std::fs::remove_file(&path);

        let listener = UnixListener::bind(&path)
            .map_err(|e| ApiError::InternalError(format!("Failed to bind Unix socket: {}", e)))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| {
            ApiError::InternalError(format!("Failed to restrict Unix socket permissions: {}", e))
        })?;

        info!("Unix socket server listening on {}", path);

        let running = Arc::clone(&self.running);
        let workspace_api = Arc::clone(&self.workspace_api);
        let tab_api = Arc::clone(&self.tab_api);
        let pane_api = Arc::clone(&self.pane_api);
        let keyboard_api = Arc::clone(&self.keyboard_api);
        let state_api = Arc::clone(&self.state_api);
        let session_renderer_api = Arc::clone(&self.session_renderer_api);
        let _ = crate::utils::spawn_named("socket-api-unix", move || {
            for stream in listener.incoming() {
                if !*running.read() {
                    break;
                }

                match stream {
                    Ok(stream) => {
                        debug!("New Unix socket connection");

                        let workspace_api = Arc::clone(&workspace_api);
                        let tab_api = Arc::clone(&tab_api);
                        let pane_api = Arc::clone(&pane_api);
                        let keyboard_api = Arc::clone(&keyboard_api);
                        let state_api = Arc::clone(&state_api);
                        let session_renderer_api = Arc::clone(&session_renderer_api);

                        let _ = crate::utils::spawn_named("socket-api-conn", move || {
                            handle_unix_connection(
                                stream,
                                workspace_api,
                                tab_api,
                                pane_api,
                                keyboard_api,
                                state_api,
                                session_renderer_api,
                            );
                        });
                    }
                    Err(e) => {
                        error!("Unix socket connection error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Para o servidor
    pub fn stop(&self) -> ApiResult<()> {
        let mut running = self.running.write();
        if !*running {
            return Err(ApiError::InvalidState("Server not running".to_string()));
        }
        *running = false;

        // Remove Unix socket
        #[cfg(unix)]
        if let Some(ref path) = self.config.unix_socket_path {
            let _ = std::fs::remove_file(path);
        }

        info!("Socket API server stopped");
        Ok(())
    }

    /// Verifica se o servidor está rodando
    pub fn is_running(&self) -> bool {
        *self.running.read()
    }

    /// Retorna referências às APIs
    pub fn workspace_api(&self) -> &WorkspaceApi {
        &self.workspace_api
    }

    pub fn tab_api(&self) -> &TabApi {
        &self.tab_api
    }

    pub fn pane_api(&self) -> &PaneApi {
        &self.pane_api
    }

    pub fn keyboard_api(&self) -> &KeyboardApi {
        &self.keyboard_api
    }

    pub fn state_api(&self) -> &StateApi {
        &self.state_api
    }

    pub fn session_renderer_api(&self) -> &SessionRendererApi {
        &self.session_renderer_api
    }

    /// Configura a integração de session renderer
    pub fn configure_session_renderer(&self, integration: crate::pane::SessionRendererIntegration) {
        self.session_renderer_api.set_integration(integration);
        info!("SessionRenderer integration configured in ApiServer");
    }
}

/// Handler para conexão TCP
fn handle_connection(
    stream: TcpStream,
    workspace_api: Arc<WorkspaceApi>,
    tab_api: Arc<TabApi>,
    pane_api: Arc<PaneApi>,
    keyboard_api: Arc<KeyboardApi>,
    state_api: Arc<StateApi>,
    session_renderer_api: Arc<SessionRendererApi>,
) {
    use std::io::BufReader;

    let peer_addr = stream.peer_addr().ok();
    let reader = match stream.try_clone() {
        Ok(read_half) => BufReader::new(read_half),
        Err(e) => {
            error!("Failed to clone TCP stream: {}", e);
            return;
        }
    };
    let mut writer = stream;

    for line in reader.lines() {
        match line {
            Ok(line) => {
                trace!("Received: {}", line);

                let response = handle_request(
                    &line,
                    &workspace_api,
                    &tab_api,
                    &pane_api,
                    &keyboard_api,
                    &state_api,
                    &session_renderer_api,
                );

                if let Some(response) = response {
                    let json = match serde_json::to_string(&response) {
                        Ok(j) => j,
                        Err(e) => {
                            error!("Failed to serialize response: {}", e);
                            continue;
                        }
                    };

                    if let Err(e) = writeln!(writer, "{}", json) {
                        error!("Failed to write response: {}", e);
                        break;
                    }

                    if let Err(e) = writer.flush() {
                        error!("Failed to flush stream: {}", e);
                        break;
                    }
                }
            }
            Err(_e) => {
                trace!("Connection closed: {:?}", peer_addr);
                break;
            }
        }
    }
}

/// Handler para conexão Unix socket
#[cfg(unix)]
fn handle_unix_connection(
    mut stream: UnixStream,
    workspace_api: Arc<WorkspaceApi>,
    tab_api: Arc<TabApi>,
    pane_api: Arc<PaneApi>,
    keyboard_api: Arc<KeyboardApi>,
    state_api: Arc<StateApi>,
    session_renderer_api: Arc<SessionRendererApi>,
) {
    use std::io::BufReader;
    use std::net::Shutdown;

    let reader = match stream.try_clone() {
        Ok(read_half) => BufReader::new(read_half),
        Err(e) => {
            error!("Failed to clone Unix stream: {}", e);
            return;
        }
    };

    for line in reader.lines() {
        match line {
            Ok(line) => {
                trace!("Received: {}", line);

                let response = handle_request(
                    &line,
                    &workspace_api,
                    &tab_api,
                    &pane_api,
                    &keyboard_api,
                    &state_api,
                    &session_renderer_api,
                );

                if let Some(response) = response {
                    let json = match serde_json::to_string(&response) {
                        Ok(j) => j,
                        Err(e) => {
                            error!("Failed to serialize response: {}", e);
                            continue;
                        }
                    };

                    if let Err(e) = writeln!(stream, "{}", json) {
                        error!("Failed to write response: {}", e);
                        break;
                    }

                    if let Err(e) = stream.flush() {
                        error!("Failed to flush stream: {}", e);
                        break;
                    }
                }
            }
            Err(_e) => {
                trace!("Unix socket connection closed");
                break;
            }
        }
    }

    let _ = stream.shutdown(Shutdown::Both);
}

/// Ponte para o estado real do app: recebe (método, parâmetros) e devolve
/// `None` quando o app não atende o método (segue para as APIs abaixo).
pub type AppBridge =
    Arc<dyn Fn(&str, &serde_json::Value) -> Option<ApiResult<serde_json::Value>> + Send + Sync>;

static APP_BRIDGE: std::sync::RwLock<Option<AppBridge>> = std::sync::RwLock::new(None);

/// Liga a ponte do app (chamado pelo `main` quando a janela sobe).
pub fn set_app_bridge(bridge: AppBridge) {
    if let Ok(mut slot) = APP_BRIDGE.write() {
        *slot = Some(bridge);
    }
}

/// Desliga a ponte do app.
pub fn clear_app_bridge() {
    if let Ok(mut slot) = APP_BRIDGE.write() {
        *slot = None;
    }
}

fn call_app_bridge(
    method: &str,
    params: &serde_json::Value,
) -> Option<ApiResult<serde_json::Value>> {
    let bridge = APP_BRIDGE.read().ok()?.clone()?;
    bridge(method, params)
}

/// Processa uma requisição
fn handle_request(
    line: &str,
    workspace_api: &WorkspaceApi,
    tab_api: &TabApi,
    pane_api: &PaneApi,
    keyboard_api: &KeyboardApi,
    state_api: &StateApi,
    session_renderer_api: &SessionRendererApi,
) -> Option<JsonRpcResponse> {
    // Tenta parse como request
    let request: JsonRpcRequest = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Some(JsonRpcResponse::error(
                None,
                ApiError::InvalidParams(format!("Invalid JSON: {}", e)),
            ));
        }
    };

    // Notificações (sem ID) não precisam de resposta
    let id = request.id.clone();

    // Extrai namespace e método
    let parts: Vec<&str> = request.method.split('.').collect();
    if parts.len() != 2 {
        return Some(JsonRpcResponse::error(
            id,
            ApiError::InvalidParams(format!(
                "Invalid method format: {}. Expected 'namespace.method'",
                request.method
            )),
        ));
    }

    let (namespace, _method) = (parts[0], parts[1]);
    let full_method = request.method.as_str();

    // Métodos atendidos pelo app (estado real das tabs e painéis).
    if let Some(result) = call_app_bridge(full_method, &request.params) {
        return Some(match result {
            Ok(value) => JsonRpcResponse::success(id, value),
            Err(e) => JsonRpcResponse::error(id, e),
        });
    }

    // Roteia para a API apropriada
    let result = match namespace {
        "workspace" => workspace_api.handle_method(full_method, request.params),
        "tab" => tab_api.handle_method(full_method, request.params),
        "pane" => pane_api.handle_method(full_method, request.params),
        "keyboard" => keyboard_api.handle_method(full_method, request.params),
        "state" => state_api.handle_method(full_method, request.params),
        "session_renderer" => session_renderer_api.handle_method(full_method, Some(request.params)),
        _ => Err(ApiError::MethodNotFound(format!(
            "Unknown namespace: {}",
            namespace
        ))),
    };

    match result {
        Ok(value) => Some(JsonRpcResponse::success(id, value)),
        Err(e) => Some(JsonRpcResponse::error(id, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::MultiplexerSession;
    use crate::pane::SessionRendererIntegration;
    use serde_json::{json, Value};
    use std::io::{BufReader, Read};
    use std::net::Shutdown;
    use std::time::{Duration, Instant};

    #[cfg(unix)]
    #[test]
    fn test_api_server_config_default_is_private_unix_socket() {
        let config = ApiServerConfig::default();
        assert_eq!(config.tcp_addr, None);
        assert_eq!(
            config.unix_socket_path.as_deref(),
            Some(default_unix_socket_path().to_string_lossy().as_ref())
        );
        assert!(config
            .unix_socket_path
            .unwrap()
            .ends_with("code-terminal.sock"));
        assert_eq!(config.max_connections, 100);
    }

    #[cfg(not(unix))]
    #[test]
    fn test_api_server_config_default() {
        let config = ApiServerConfig::default();
        assert_eq!(config.tcp_addr, Some("127.0.0.1:9123".to_string()));
        assert_eq!(config.max_connections, 100);
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_socket_is_owner_only_and_not_stolen() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api.sock");
        let first = new_server(ApiServerConfig::with_unix_socket(&path));
        first.start().unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        let second = new_server(ApiServerConfig::with_unix_socket(&path));
        match second.start() {
            Err(ApiError::InternalError(message)) => {
                assert!(message.contains("Another instance"), "{message}")
            }
            other => panic!("esperava recusa, veio {:?}", other),
        }
        assert!(UnixStream::connect(&path).is_ok());
        first.stop().unwrap();
    }

    #[test]
    fn test_api_server_config_tcp() {
        let config = ApiServerConfig::with_tcp("0.0.0.0:8080");
        assert_eq!(config.tcp_addr, Some("0.0.0.0:8080".to_string()));
        assert!(config.unix_socket_path.is_none());
    }

    #[test]
    fn test_json_rpc_response_success() {
        let response = JsonRpcResponse::success(
            Some(serde_json::Value::Number(1.into())),
            serde_json::json!({"status": "ok"}),
        );

        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_some());
        assert!(response.error.is_none());
    }

    #[test]
    fn test_json_rpc_response_error() {
        let response = JsonRpcResponse::error(
            Some(serde_json::Value::Number(1.into())),
            ApiError::NotFound("test".to_string()),
        );

        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_none());
        assert!(response.error.is_some());

        let error = response.error.unwrap();
        assert_eq!(error.code, -32001);
    }

    #[test]
    fn test_tcp_server_e2e_multiple_clients_tabs_panes_and_session_rendering() {
        let addr = reserve_tcp_addr();
        let server = ApiServer::new(
            ApiServerConfig::with_tcp(addr.clone()),
            Arc::new(RwLock::new(Config::default())),
        );

        let mut renderer_integration = SessionRendererIntegration::new();
        let session = MultiplexerSession::new(Some("tcp-e2e".to_string()), (80, 24));
        let session_id = session.id.to_string();
        renderer_integration.connect_session(&session).unwrap();
        server.configure_session_renderer(renderer_integration);
        server.start().unwrap();

        let mut client_a = connect_with_retry(&addr);
        let mut client_b = connect_with_retry(&addr);

        let created_tab = rpc_call(
            &mut client_a,
            1,
            "tab.create",
            json!({ "title": "tcp-tab", "working_dir": "D:\\" }),
        );
        assert!(created_tab.get("error").is_none(), "{created_tab}");
        let tab_id = created_tab["result"]["id"].as_u64().expect("tab id");

        let tab_list = rpc_call(&mut client_b, 2, "tab.list", Value::Null);
        assert!(tab_list.get("error").is_none(), "{tab_list}");
        assert!(tab_list["result"]
            .as_array()
            .expect("tab list")
            .iter()
            .any(|tab| tab["id"].as_u64() == Some(tab_id)));

        let split = rpc_call(
            &mut client_a,
            3,
            "pane.split",
            json!({
                "tab_id": tab_id,
                "params": {
                    "direction": "vertical",
                    "size_percent": 50
                }
            }),
        );
        assert!(split.get("error").is_none(), "{split}");
        assert_eq!(split["result"]["tab_id"].as_u64(), Some(tab_id));

        let render_list = rpc_call(&mut client_b, 4, "session_renderer.list", Value::Null);
        assert!(render_list.get("error").is_none(), "{render_list}");
        assert!(render_list["result"]["rendered_sessions_list"]
            .as_array()
            .expect("rendered sessions")
            .iter()
            .any(|session| session["session_id"].as_str() == Some(session_id.as_str())));

        let start = rpc_call(
            &mut client_a,
            5,
            "session_renderer.start",
            json!({ "session_id": session_id }),
        );
        assert!(start.get("error").is_none(), "{start}");

        let stop = rpc_call(
            &mut client_b,
            6,
            "session_renderer.stop",
            json!({ "session_id": session_id }),
        );
        assert!(stop.get("error").is_none(), "{stop}");

        let after_stop = rpc_call(&mut client_a, 7, "session_renderer.list", Value::Null);
        assert!(after_stop.get("error").is_none(), "{after_stop}");
        assert!(after_stop["result"]["rendered_sessions_list"]
            .as_array()
            .expect("rendered sessions after stop")
            .is_empty());

        client_a.shutdown(Shutdown::Both).ok();
        client_b.shutdown(Shutdown::Both).ok();
        server.stop().unwrap();
        TcpStream::connect(&addr).ok();
    }

    fn reserve_tcp_addr() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        addr.to_string()
    }

    fn connect_with_retry(addr: &str) -> TcpStream {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match TcpStream::connect(addr) {
                Ok(stream) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    return stream;
                }
                Err(error) if Instant::now() < deadline => {
                    let _ = error;
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => panic!("failed to connect to {addr}: {error}"),
            }
        }
    }

    fn rpc_call(stream: &mut TcpStream, id: u64, method: &str, params: Value) -> Value {
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        writeln!(stream, "{request}").unwrap();
        stream.flush().unwrap();

        let reader_stream = stream.try_clone().unwrap();
        let mut reader = BufReader::new(reader_stream);
        let mut response = String::new();
        reader.read_line(&mut response).unwrap();
        if response.is_empty() {
            let mut tail = String::new();
            reader.read_to_string(&mut tail).ok();
            panic!("empty JSON-RPC response; tail={tail:?}");
        }
        serde_json::from_str(&response).unwrap()
    }

    fn new_server(config: ApiServerConfig) -> ApiServer {
        ApiServer::new(config, Arc::new(RwLock::new(Config::default())))
    }

    fn dispatch(server: &ApiServer, line: &str) -> JsonRpcResponse {
        handle_request(
            line,
            server.workspace_api(),
            server.tab_api(),
            server.pane_api(),
            server.keyboard_api(),
            server.state_api(),
            server.session_renderer_api(),
        )
        .expect("resposta")
    }

    /// A ponte do app atende os métodos dela; o resto segue para as APIs.
    #[test]
    fn test_app_bridge_takes_its_methods_first() {
        let server = new_server(ApiServerConfig::default());
        set_app_bridge(Arc::new(|method, params| {
            (method == "bridgetest.echo").then(|| Ok(params.clone()))
        }));
        let echoed = dispatch(
            &server,
            r#"{"jsonrpc":"2.0","id":1,"method":"bridgetest.echo","params":{"x":1}}"#,
        );
        let fallback = dispatch(
            &server,
            r#"{"jsonrpc":"2.0","id":2,"method":"bridgetest.other","params":{}}"#,
        );
        clear_app_bridge();
        assert_eq!(echoed.result, Some(json!({"x": 1})));
        assert!(fallback.error.is_some());
    }

    #[test]
    fn test_handle_request_errors() {
        let server = new_server(ApiServerConfig::default());

        let invalid = dispatch(&server, "{não é json");
        let error = invalid.error.unwrap();
        assert_eq!(error.code, -32602);
        assert!(error.message.contains("Invalid JSON"), "{}", error.message);

        let format = dispatch(&server, r#"{"jsonrpc":"2.0","id":1,"method":"semponto"}"#);
        assert!(format
            .error
            .unwrap()
            .message
            .contains("Expected 'namespace.method'"));

        let unknown = dispatch(&server, r#"{"jsonrpc":"2.0","id":2,"method":"nope.list"}"#);
        let error = unknown.error.unwrap();
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("Unknown namespace: nope"));
        assert_eq!(unknown.id, Some(json!(2)));
    }

    #[test]
    fn test_handle_request_routes_each_namespace() {
        let server = new_server(ApiServerConfig::default());
        for method in [
            "workspace.list",
            "tab.list",
            "pane.list",
            "keyboard.list_shortcuts",
            "state.get",
        ] {
            let line = json!({"jsonrpc": "2.0", "id": 1, "method": method}).to_string();
            let response = dispatch(&server, &line);
            assert!(response.error.is_none(), "{method}: {:?}", response.error);
        }
        // session_renderer chega na API, que exige a integração configurada
        let renderer = dispatch(
            &server,
            r#"{"jsonrpc":"2.0","id":1,"method":"session_renderer.list"}"#,
        );
        assert_eq!(renderer.error.unwrap().code, -32003);
        // Os acessores expõem as mesmas instâncias usadas pelo roteador
        server.tab_api().create(Default::default()).unwrap();
        let list = dispatch(&server, r#"{"jsonrpc":"2.0","id":3,"method":"tab.list"}"#);
        assert_eq!(list.result.unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_start_stop_state_transitions() {
        let server = new_server(ApiServerConfig {
            tcp_addr: None,
            unix_socket_path: None,
            ..ApiServerConfig::default()
        });
        assert!(!server.is_running());
        assert!(matches!(server.stop(), Err(ApiError::InvalidState(_))));
        server.start().unwrap();
        assert!(server.is_running());
        assert!(matches!(server.start(), Err(ApiError::InvalidState(_))));
        server.stop().unwrap();
        assert!(!server.is_running());
    }

    /// Regressão: um bind que falhava deixava o servidor marcado como rodando.
    #[test]
    fn test_failed_bind_leaves_server_stopped() {
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = new_server(ApiServerConfig::with_tcp(
            taken.local_addr().unwrap().to_string(),
        ));
        match server.start() {
            Err(ApiError::InternalError(message)) => {
                assert!(message.contains("Failed to bind TCP"), "{message}")
            }
            other => panic!("esperava erro de bind, veio {:?}", other),
        }
        assert!(!server.is_running());
        assert!(matches!(server.stop(), Err(ApiError::InvalidState(_))));
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_server_e2e_and_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api.sock");
        std::fs::write(&path, b"stale").unwrap();
        let config = ApiServerConfig::with_unix_socket(&path);
        assert!(config.tcp_addr.is_none());
        let server = new_server(config);
        server.start().unwrap();

        let stream = UnixStream::connect(&path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        let mut ask = |line: &str| -> Value {
            writeln!(writer, "{line}").unwrap();
            writer.flush().unwrap();
            let mut response = String::new();
            reader.read_line(&mut response).unwrap();
            serde_json::from_str(&response).unwrap()
        };

        let created =
            ask(r#"{"jsonrpc":"2.0","id":1,"method":"tab.create","params":{"title":"unix"}}"#);
        assert_eq!(created["result"]["title"], "unix");
        let invalid = ask("lixo");
        assert_eq!(invalid["error"]["code"], -32602);

        server.stop().unwrap();
        assert!(!path.exists(), "o socket deveria ser removido no stop");
        // Acorda o loop de accept para ele ver `running == false` e sair
        UnixStream::connect(&path).ok();
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_bind_error_resets_running() {
        let dir = tempfile::tempdir().unwrap();
        let server = new_server(ApiServerConfig::with_unix_socket(
            dir.path().join("nao-existe").join("api.sock"),
        ));
        match server.start() {
            Err(ApiError::InternalError(message)) => {
                assert!(message.contains("Failed to bind Unix socket"), "{message}")
            }
            other => panic!("esperava erro de bind, veio {:?}", other),
        }
        assert!(!server.is_running());
    }
}
