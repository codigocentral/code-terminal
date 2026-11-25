//! Remote Client for JSON-RPC communication

use crate::cli::error::{CliError, CliResult};
use crate::config::Config;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{BufRead, Write};
use std::net::TcpStream;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::Path;

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: Value,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: u64,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

/// JSON-RPC error
#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

/// Remote client for connecting to the terminal
pub struct RemoteClient {
    stream: ClientStream,
    next_id: u64,
}

enum ClientStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl RemoteClient {
    /// Connect to the terminal
    pub async fn connect(addr: &str) -> CliResult<Self> {
        // Check if it's a Unix socket path or TCP address
        if addr.contains('/') || addr.contains('\\') {
            #[cfg(unix)]
            {
                let stream = UnixStream::connect(addr)
                    .map_err(|_| CliError::ServerNotRunning(addr.to_string()))?;
                stream.set_nonblocking(false)?;
                return Ok(Self {
                    stream: ClientStream::Unix(stream),
                    next_id: 1,
                });
            }
            #[cfg(not(unix))]
            {
                return Err(CliError::connection(
                    "Unix sockets not supported on this platform",
                ));
            }
        }

        // TCP connection
        let stream =
            TcpStream::connect(addr).map_err(|_| CliError::ServerNotRunning(addr.to_string()))?;
        stream.set_nonblocking(false)?;

        Ok(Self {
            stream: ClientStream::Tcp(stream),
            next_id: 1,
        })
    }

    /// Send a JSON-RPC request and get response
    pub fn call(&mut self, method: &str, params: Value) -> CliResult<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let request_json = serde_json::to_string(&request)?;
        let request_bytes = format!("{}\n", request_json);

        // Send request
        match &mut self.stream {
            ClientStream::Tcp(stream) => {
                stream.write_all(request_bytes.as_bytes())?;
                stream.flush()?;
            }
            #[cfg(unix)]
            ClientStream::Unix(stream) => {
                stream.write_all(request_bytes.as_bytes())?;
                stream.flush()?;
            }
        }

        // Read response
        let mut buffer = String::new();
        match &mut self.stream {
            ClientStream::Tcp(stream) => {
                let reader_stream = stream.try_clone()?;
                let mut reader = std::io::BufReader::new(reader_stream);
                reader.read_line(&mut buffer)?;
            }
            #[cfg(unix)]
            ClientStream::Unix(stream) => {
                let reader_stream = stream.try_clone()?;
                let mut reader = std::io::BufReader::new(reader_stream);
                reader.read_line(&mut buffer)?;
            }
        }

        let response: JsonRpcResponse =
            serde_json::from_str(&buffer).map_err(|e| CliError::InvalidResponse(e.to_string()))?;

        if response.jsonrpc != "2.0" {
            return Err(CliError::InvalidResponse(format!(
                "Unsupported JSON-RPC version: {}",
                response.jsonrpc
            )));
        }

        if response.id != id {
            return Err(CliError::InvalidResponse(format!(
                "Mismatched response id: expected {}, got {}",
                id, response.id
            )));
        }

        if let Some(error) = response.error {
            let data = error
                .data
                .map(|data| format!("; data: {}", data))
                .unwrap_or_default();
            return Err(CliError::Rpc {
                message: format!("{} (code: {}){}", error.message, error.code, data),
            });
        }

        Ok(response.result.unwrap_or(Value::Null))
    }

    /// Ping the server
    pub async fn ping(&mut self) -> CliResult<()> {
        let _ = self.call("state.api_info", Value::Null)?;
        Ok(())
    }

    /// List tabs
    pub fn list_tabs(&mut self) -> CliResult<Vec<TabInfo>> {
        let result = self.call("tab.list", Value::Null)?;
        let tabs: Vec<TabInfo> = serde_json::from_value(result)?;
        Ok(tabs)
    }

    /// Create new tab
    pub fn new_tab(
        &mut self,
        title: Option<&str>,
        cwd: Option<&Path>,
        command: Vec<String>,
    ) -> CliResult<usize> {
        let mut params = serde_json::Map::new();
        if let Some(t) = title {
            params.insert("title".to_string(), Value::String(t.to_string()));
        }
        if let Some(c) = cwd {
            params.insert(
                "cwd".to_string(),
                Value::String(c.to_string_lossy().to_string()),
            );
        }
        if !command.is_empty() {
            params.insert(
                "command".to_string(),
                Value::Array(command.into_iter().map(Value::String).collect()),
            );
        }
        let result = self.call("tab.create", Value::Object(params))?;
        let tab: TabInfo = serde_json::from_value(result)?;
        let _ = self.call("tab.switch", serde_json::json!({ "id": tab.id }))?;
        Ok(tab.index)
    }

    /// Close tab
    pub fn close_tab(&mut self, index: usize) -> CliResult<()> {
        let tabs = self.list_tabs()?;
        let tab = tabs
            .into_iter()
            .find(|tab| tab.index == index)
            .ok_or_else(|| CliError::InvalidResponse(format!("Tab index not found: {}", index)))?;
        let params = serde_json::json!({"id": tab.id});
        let _ = self.call("tab.close", params)?;
        Ok(())
    }

    /// Rename a tab (fixed title); `None` or empty restores the automatic title
    pub fn rename_tab(&mut self, index: usize, title: Option<&str>) -> CliResult<()> {
        let params = serde_json::json!({"index": index, "title": title});
        let _ = self.call("tab.rename", params)?;
        Ok(())
    }

    /// Switch to tab
    pub fn goto_tab(&mut self, index: usize) -> CliResult<()> {
        let params = serde_json::json!({"index": index});
        let _ = self.call("tab.switch", params)?;
        Ok(())
    }

    /// Send keys
    pub fn send_keys(&mut self, keys: &str) -> CliResult<()> {
        let params = serde_json::json!({"keys": keys});
        let _ = self.call("keyboard.send", params)?;
        Ok(())
    }

    /// Send input
    pub fn send_input(&mut self, text: &str) -> CliResult<()> {
        let params = serde_json::json!({"text": text});
        let _ = self.call("keyboard.text", params)?;
        Ok(())
    }

    /// Get terminal size
    pub fn get_size(&mut self) -> CliResult<(u16, u16)> {
        let result = self.call("state.get", Value::Null)?;
        let state: TerminalStateResponse = serde_json::from_value(result)?;
        Ok((state.terminal_size.cols, state.terminal_size.rows))
    }

    /// Set terminal size
    pub fn set_size(&mut self, cols: u16, rows: u16) -> CliResult<()> {
        let params = serde_json::json!({"cols": cols, "rows": rows});
        let _ = self.call("set_size", params)?;
        Ok(())
    }

    /// Get info
    pub fn info(&mut self) -> CliResult<Info> {
        let state = self.call("state.get", Value::Null)?;
        let state: TerminalStateResponse = serde_json::from_value(state)?;
        let active_tab = state
            .tabs
            .iter()
            .find(|tab| tab.is_active)
            .map(|tab| tab.index)
            .unwrap_or(0);

        Ok(Info {
            version: state.version,
            tab_count: state.tabs.len(),
            active_tab,
            size: SizeInfo {
                cols: state.terminal_size.cols,
                rows: state.terminal_size.rows,
            },
        })
    }

    /// Send notification
    pub fn notify(&mut self, title: &str, message: &str) -> CliResult<()> {
        self.notify_in(title, message, None)
    }

    /// Como [`RemoteClient::notify`]; numa aba persistente, `mux_session`
    /// acha o painel mesmo depois de a janela ter sido reaberta.
    pub fn notify_in(
        &mut self,
        title: &str,
        message: &str,
        mux_session: Option<&str>,
    ) -> CliResult<()> {
        let mut params = serde_json::json!({ "title": title, "body": message });
        if let Some(session) = mux_session {
            params["mux_session"] = Value::from(session);
        }
        // Dentro de um painel do Code Terminal, acende o anel deste painel.
        if let Some(pane) = current_pane_id() {
            params["pane"] = Value::from(pane);
        }
        let _ = self.call("notification.send", params)?;
        Ok(())
    }

    /// Lê o texto visível de um painel (o atual por padrão).
    pub fn read_screen(&mut self, lines: Option<usize>, pane: Option<u64>) -> CliResult<String> {
        let mut params = serde_json::json!({});
        if let Some(lines) = lines {
            params["lines"] = Value::from(lines);
        }
        if let Some(pane) = pane.or_else(current_pane_id) {
            params["pane"] = Value::from(pane);
        }
        let result = self.call("terminal.read", params)?;
        Ok(result
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// List panes
    pub fn list_panes(&mut self) -> CliResult<Vec<PaneInfo>> {
        let result = self.call("pane.list", Value::Null)?;
        let panes: Vec<PaneInfo> = serde_json::from_value(result)?;
        Ok(panes)
    }

    /// Split pane
    pub fn split(&mut self, direction: &str) -> CliResult<()> {
        let state = self.call("state.get", Value::Null)?;
        let state: TerminalStateResponse = serde_json::from_value(state)?;
        let params = serde_json::json!({
            "tab_id": state.active_tab,
            "params": {"direction": direction}
        });
        let _ = self.call("pane.split", params)?;
        Ok(())
    }

    /// Get config
    pub fn get_config(&mut self, key: &str) -> CliResult<String> {
        let config = Config::load().map_err(|e| CliError::config(e.to_string()))?;
        config_value(&config, key)
    }

    /// Set config
    pub fn set_config(&mut self, key: &str, value: &str) -> CliResult<()> {
        let mut config = Config::load().map_err(|e| CliError::config(e.to_string()))?;
        apply_config_override(&mut config, key, value)?;
        config.save().map_err(|e| CliError::config(e.to_string()))?;
        Ok(())
    }

    /// Reset config
    pub fn reset_config(&mut self) -> CliResult<()> {
        Config::default()
            .save()
            .map_err(|e| CliError::config(e.to_string()))?;
        Ok(())
    }
}

fn config_value(config: &Config, key: &str) -> CliResult<String> {
    match key {
        "font.size" => Ok(config.font.size.to_string()),
        "font.family" => Ok(config.font.family.clone()),
        "theme" => Ok(config.colors.name.clone()),
        "window.opacity" => Ok(config.window.opacity.to_string()),
        "terminal.shell" => Ok(config.terminal.shell.clone()),
        _ => Err(CliError::InvalidInput(format!(
            "Unsupported config key: {}",
            key
        ))),
    }
}

fn apply_config_override(config: &mut Config, key: &str, value: &str) -> CliResult<()> {
    match key {
        "font.size" => {
            config.font.size = value.parse().map_err(|_| {
                CliError::InvalidInput(format!("Invalid float for {}: {}", key, value))
            })?;
        }
        "font.family" => {
            config.font.family = value.to_string();
        }
        "theme" => {
            config.colors.name = value.to_string();
        }
        "window.opacity" => {
            config.window.opacity = value.parse().map_err(|_| {
                CliError::InvalidInput(format!("Invalid float for {}: {}", key, value))
            })?;
        }
        "terminal.shell" => {
            config.terminal.shell = value.to_string();
        }
        _ => {
            return Err(CliError::InvalidInput(format!(
                "Unsupported config key: {}",
                key
            )));
        }
    }

    Ok(())
}

/// Tab information
#[derive(Debug, Deserialize, Serialize)]
pub struct TabInfo {
    pub id: u64,
    pub index: usize,
    pub title: String,
    pub is_active: bool,
    pub pid: Option<u32>,
    #[serde(alias = "working_dir")]
    pub cwd: Option<String>,
}

/// Painel onde o CLI está rodando (variável posta pelo Code Terminal).
pub fn current_pane_id() -> Option<u64> {
    std::env::var("CODE_TERMINAL_PANE_ID").ok()?.parse().ok()
}

/// Pane information
#[derive(Debug, Deserialize, Serialize)]
pub struct PaneInfo {
    pub id: u64,
    pub index: usize,
    pub is_active: bool,
}

/// Size information
#[derive(Debug, Deserialize, Serialize)]
pub struct SizeInfo {
    pub cols: u16,
    pub rows: u16,
}

/// Terminal info
#[derive(Debug, Deserialize, Serialize)]
pub struct Info {
    pub version: String,
    pub tab_count: usize,
    pub active_tab: usize,
    pub size: SizeInfo,
}

#[derive(Debug, Deserialize)]
struct TerminalSizeResponse {
    cols: u16,
    rows: u16,
}

#[derive(Debug, Deserialize)]
struct TerminalStateResponse {
    version: String,
    tabs: Vec<TabInfo>,
    active_tab: u64,
    terminal_size: TerminalSizeResponse,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::cli::test_support::terminal_handler;
    use crate::cli::test_support::FakeRpc;
    use serde_json::json;

    fn connect(addr: &str) -> CliResult<RemoteClient> {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(RemoteClient::connect(addr))
    }

    #[cfg(unix)]
    #[test]
    fn test_connect_over_unix_socket_and_call() {
        let srv = FakeRpc::unix(terminal_handler);
        let mut c = connect(&srv.addr).unwrap();
        let tabs = c.list_tabs().unwrap();
        assert_eq!(tabs.len(), 2);
        // alias "working_dir" -> cwd
        assert_eq!(tabs[0].cwd.as_deref(), Some("/home/u"));
        assert_eq!(tabs[1].cwd, None);
        assert_eq!(c.get_size().unwrap(), (120, 40));
        assert_eq!(srv.methods(), vec!["tab.list", "state.get"]);
    }

    #[cfg(unix)]
    #[test]
    fn test_connect_failures_report_server_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nada.sock");
        let r = connect(&missing.to_string_lossy());
        assert!(matches!(r, Err(CliError::ServerNotRunning(p)) if p.ends_with("nada.sock")));

        // porta efêmera que acabou de ser liberada: ninguém escuta nela
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let addr = format!("127.0.0.1:{}", port);
        assert!(matches!(connect(&addr), Err(CliError::ServerNotRunning(a)) if a == addr));
    }

    #[test]
    fn test_call_increments_ids_and_returns_null_without_result() {
        let srv = FakeRpc::tcp(|_, _| Ok(Value::Null));
        let mut c = connect(&srv.addr).unwrap();
        assert_eq!(c.call("a", json!({"x": 1})).unwrap(), Value::Null);
        assert_eq!(c.next_id, 2);
        rt_block(c.ping()).unwrap();
        assert_eq!(c.next_id, 3);
        assert_eq!(srv.methods(), vec!["a", "state.api_info"]);
        assert_eq!(srv.params_of("a").unwrap(), json!({"x": 1}));
    }

    fn rt_block<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn test_call_rejects_wrong_version_id_and_garbage() {
        let srv = FakeRpc::raw(|id| format!(r#"{{"jsonrpc":"1.0","id":{},"result":1}}"#, id));
        let mut c = connect(&srv.addr).unwrap();
        assert!(
            matches!(c.call("m", Value::Null), Err(CliError::InvalidResponse(m)) if m.contains("1.0"))
        );

        let srv = FakeRpc::raw(|id| format!(r#"{{"jsonrpc":"2.0","id":{},"result":1}}"#, id + 5));
        let mut c = connect(&srv.addr).unwrap();
        assert!(matches!(
            c.call("m", Value::Null),
            Err(CliError::InvalidResponse(m)) if m.contains("expected 1, got 6")
        ));

        let srv = FakeRpc::raw(|_| "isto não é json".to_string());
        let mut c = connect(&srv.addr).unwrap();
        assert!(matches!(
            c.call("m", Value::Null),
            Err(CliError::InvalidResponse(_))
        ));
    }

    #[test]
    fn test_call_formats_rpc_error_with_and_without_data() {
        let srv = FakeRpc::tcp(|m, _| match m {
            "with" => Err((-32000, "falhou".into(), Some(json!({"k": 1})))),
            _ => Err((-32601, "Method not found".into(), None)),
        });
        let mut c = connect(&srv.addr).unwrap();
        match c.call("with", Value::Null) {
            Err(CliError::Rpc { message }) => {
                assert_eq!(message, r#"falhou (code: -32000); data: {"k":1}"#)
            }
            other => panic!("{:?}", other),
        }
        match c.call("without", Value::Null) {
            Err(CliError::Rpc { message }) => {
                assert_eq!(message, "Method not found (code: -32601)")
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn test_typed_methods_reject_unexpected_shapes() {
        let srv = FakeRpc::tcp(|_, _| Ok(json!({"nada": true})));
        let mut c = connect(&srv.addr).unwrap();
        assert!(matches!(c.list_tabs(), Err(CliError::Serialization(_))));
        assert!(matches!(c.list_panes(), Err(CliError::Serialization(_))));
        assert!(matches!(c.get_size(), Err(CliError::Serialization(_))));
        assert!(matches!(c.info(), Err(CliError::Serialization(_))));
        assert!(matches!(
            c.split("vertical"),
            Err(CliError::Serialization(_))
        ));
        assert!(matches!(
            c.new_tab(None, None, vec![]),
            Err(CliError::Serialization(_))
        ));
    }

    #[test]
    fn test_info_defaults_active_tab_to_zero() {
        let srv = FakeRpc::tcp(|_, _| {
            Ok(json!({
                "version": "9",
                "tabs": [{"id": 1, "index": 4, "title": "x", "is_active": false, "pid": null, "cwd": null}],
                "active_tab": 1,
                "terminal_size": {"cols": 1, "rows": 2}
            }))
        });
        let mut c = connect(&srv.addr).unwrap();
        let info = c.info().unwrap();
        assert_eq!(info.active_tab, 0);
        assert_eq!(info.tab_count, 1);
        assert_eq!((info.size.cols, info.size.rows), (1, 2));
    }

    #[test]
    fn test_config_value_reads_supported_keys() {
        let mut config = Config::default();
        config.font.size = 13.5;
        config.font.family = "Fira".into();
        config.colors.name = "dracula".into();
        config.window.opacity = 0.75;
        config.terminal.shell = "/bin/zsh".into();
        assert_eq!(config_value(&config, "font.size").unwrap(), "13.5");
        assert_eq!(config_value(&config, "font.family").unwrap(), "Fira");
        assert_eq!(config_value(&config, "theme").unwrap(), "dracula");
        assert_eq!(config_value(&config, "window.opacity").unwrap(), "0.75");
        assert_eq!(config_value(&config, "terminal.shell").unwrap(), "/bin/zsh");
        assert!(matches!(
            config_value(&config, "x.y"),
            Err(CliError::InvalidInput(m)) if m.contains("x.y")
        ));
    }

    #[test]
    fn test_apply_config_override_round_trips_with_config_value() {
        let mut config = Config::default();
        for (key, value) in [
            ("font.size", "18"),
            ("font.family", "JetBrains Mono"),
            ("theme", "nord"),
            ("window.opacity", "0.5"),
            ("terminal.shell", "/usr/bin/fish"),
        ] {
            apply_config_override(&mut config, key, value).unwrap();
            assert_eq!(config_value(&config, key).unwrap(), value);
        }
        assert!(matches!(
            apply_config_override(&mut config, "font.size", "grande"),
            Err(CliError::InvalidInput(m)) if m.contains("grande")
        ));
        assert!(matches!(
            apply_config_override(&mut config, "window.opacity", "meio"),
            Err(CliError::InvalidInput(_))
        ));
        assert!(matches!(
            apply_config_override(&mut config, "nope", "1"),
            Err(CliError::InvalidInput(m)) if m.contains("nope")
        ));
        // valor inválido não altera o anterior
        assert_eq!(config_value(&config, "font.size").unwrap(), "18");
    }

    #[test]
    fn test_tab_info_serialization() {
        let tab = TabInfo {
            id: 1,
            index: 0,
            title: "test".to_string(),
            is_active: true,
            pid: Some(1234),
            cwd: Some("/home".to_string()),
        };

        let json = serde_json::to_string(&tab).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("1234"));

        let deserialized: TabInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(tab.index, deserialized.index);
        assert_eq!(tab.title, deserialized.title);
    }

    #[test]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method: "ping".to_string(),
            params: Value::Null,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("2.0"));
        assert!(json.contains("ping"));
        assert!(json.contains("\"id\":1"));
    }

    #[test]
    fn test_json_rpc_response_deserialization() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":"pong"}"#;
        let response: JsonRpcResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, 1);
        assert_eq!(response.result, Some(Value::String("pong".to_string())));
        assert!(response.error.is_none());
    }

    #[test]
    fn test_json_rpc_error_deserialization() {
        let json =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;
        let response: JsonRpcResponse = serde_json::from_str(json).unwrap();

        assert!(response.result.is_none());
        assert!(response.error.is_some());

        let error = response.error.unwrap();
        assert_eq!(error.code, -32601);
        assert_eq!(error.message, "Method not found");
    }

    #[test]
    fn test_size_info() {
        let size = SizeInfo { cols: 80, rows: 24 };
        assert_eq!(size.cols, 80);
        assert_eq!(size.rows, 24);
    }

    #[test]
    fn test_pane_info() {
        let pane = PaneInfo {
            id: 1,
            index: 0,
            is_active: true,
        };
        assert!(pane.is_active);
        assert_eq!(pane.id, 1);
    }
}
