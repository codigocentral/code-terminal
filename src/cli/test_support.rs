//! Servidores falsos usados pelos testes da CLI.
//!
//! - [`FakeRpc`]: servidor JSON-RPC (uma requisição por linha) num socket Unix
//!   ou TCP efêmero, que grava as chamadas recebidas e responde via closure.
//! - [`FakeMux`]: servidor do protocolo do multiplexer (`Message` em JSON por
//!   linha) num TCP efêmero.

use crate::multiplexer::{Message, MessageType, SessionClient};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

/// Resposta de um handler JSON-RPC: `Ok(result)` ou `Err((code, message, data))`.
pub type RpcReply = Result<Value, (i32, String, Option<Value>)>;

/// Chamadas gravadas: (método, params).
pub type Calls = Arc<Mutex<Vec<(String, Value)>>>;

/// Servidor JSON-RPC falso.
pub struct FakeRpc {
    pub addr: String,
    pub calls: Calls,
    _dir: tempfile::TempDir,
}

impl FakeRpc {
    /// Sobe o servidor num socket Unix dentro de um diretório temporário.
    #[cfg(unix)]
    pub fn unix<F>(handler: F) -> Self
    where
        F: Fn(&str, &Value) -> RpcReply + Send + 'static,
    {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rpc.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let calls: Calls = Arc::default();
        let c = calls.clone();
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let reader = stream.try_clone().unwrap();
                serve_rpc(BufReader::new(reader), stream, &handler, &c);
            }
        });
        Self {
            addr: path.to_string_lossy().to_string(),
            calls,
            _dir: dir,
        }
    }

    /// Sobe o servidor em 127.0.0.1 numa porta efêmera.
    pub fn tcp<F>(handler: F) -> Self
    where
        F: Fn(&str, &Value) -> RpcReply + Send + 'static,
    {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let calls: Calls = Arc::default();
        let c = calls.clone();
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let reader = stream.try_clone().unwrap();
                serve_rpc(BufReader::new(reader), stream, &handler, &c);
            }
        });
        Self {
            addr,
            calls,
            _dir: tempfile::tempdir().unwrap(),
        }
    }

    /// Sobe um servidor que responde cada linha com o texto cru dado (sem JSON-RPC).
    pub fn raw(reply: impl Fn(u64) -> String + Send + 'static) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let mut writer = stream.try_clone().unwrap();
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let req: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                    let id = req["id"].as_u64().unwrap_or(0);
                    if writeln!(writer, "{}", reply(id)).is_err() {
                        break;
                    }
                }
            }
        });
        Self {
            addr,
            calls: Arc::default(),
            _dir: tempfile::tempdir().unwrap(),
        }
    }

    /// Métodos chamados, na ordem.
    pub fn methods(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(m, _)| m.clone())
            .collect()
    }

    /// Params da última chamada ao método.
    pub fn params_of(&self, method: &str) -> Option<Value> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|(m, _)| m == method)
            .map(|(_, p)| p.clone())
    }
}

fn serve_rpc<R: BufRead, W: Write, F>(reader: R, mut writer: W, handler: &F, calls: &Calls)
where
    F: Fn(&str, &Value) -> RpcReply,
{
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let req: Value = serde_json::from_str(&line).unwrap();
        let method = req["method"].as_str().unwrap_or_default().to_string();
        let params = req["params"].clone();
        calls.lock().unwrap().push((method.clone(), params.clone()));
        let resp = match handler(&method, &params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": req["id"], "result": result}),
            Err((code, message, data)) => {
                let mut err = json!({"code": code, "message": message});
                if let Some(d) = data {
                    err["data"] = d;
                }
                json!({"jsonrpc": "2.0", "id": req["id"], "error": err})
            }
        };
        if writeln!(writer, "{}", resp).is_err() {
            break;
        }
    }
}

/// Estado padrão de um terminal falso: duas abas (a de índice 1 ativa),
/// 120x40, versão 1.2.3.
pub fn terminal_handler(method: &str, params: &Value) -> RpcReply {
    let tabs = json!([
        {"id": 10, "index": 0, "title": "shell", "is_active": false, "pid": 100, "cwd": "/home/u"},
        {"id": 11, "index": 1, "title": "vim", "is_active": true, "pid": null, "working_dir": null}
    ]);
    match method {
        "tab.list" => Ok(tabs),
        "tab.create" => Ok(json!({
            "id": 7,
            "index": 3,
            "title": params.get("title").cloned().unwrap_or(json!("new")),
            "is_active": true,
            "pid": null,
            "cwd": null
        })),
        "state.get" => Ok(json!({
            "version": "1.2.3",
            "tabs": tabs,
            "active_tab": 11,
            "terminal_size": {"cols": 120, "rows": 40}
        })),
        "pane.list" => Ok(json!([
            {"id": 5, "index": 0, "is_active": true},
            {"id": 6, "index": 1, "is_active": false}
        ])),
        _ => Ok(Value::Null),
    }
}

/// Servidor falso do multiplexer.
pub struct FakeMux {
    pub addr: String,
    pub requests: Arc<Mutex<Vec<MessageType>>>,
}

impl FakeMux {
    /// Sobe o servidor; `handler` decide a resposta de cada requisição.
    pub fn start<F>(handler: F) -> Self
    where
        F: Fn(&MessageType) -> MessageType + Send + 'static,
    {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let requests: Arc<Mutex<Vec<MessageType>>> = Arc::default();
        let r = requests.clone();
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                let mut writer = stream.try_clone().unwrap();
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let msg = Message::decode(&line).unwrap();
                    r.lock().unwrap().push(msg.msg_type.clone());
                    let resp = Message::new(msg.id, handler(&msg.msg_type));
                    if writeln!(writer, "{}", resp.encode().unwrap()).is_err() {
                        break;
                    }
                }
            }
        });
        Self { addr, requests }
    }

    /// Conecta um `SessionClient` a este servidor.
    pub fn client(&self) -> SessionClient {
        SessionClient::connect_to(&self.addr).unwrap()
    }

    /// Requisições recebidas, na ordem.
    pub fn requests(&self) -> Vec<MessageType> {
        self.requests.lock().unwrap().clone()
    }
}
