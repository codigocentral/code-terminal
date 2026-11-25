//! Modo servidor: um processo `code-terminal --server` fica rodando com a GPU
//! e as fontes já carregadas, sem janela. Rodar `code-terminal` com o
//! servidor de pé só pede uma janela nova pelo socket e sai: a janela aparece
//! em poucos milissegundos, sem pagar de novo a inicialização do driver de
//! vídeo (a mesma ideia do `foot --server` + `footclient`).
//!
//! Sem servidor, `code-terminal` abre do jeito normal.
//!
//! Protocolo: uma linha JSON por conexão ([`OpenRequest`]) e uma linha JSON de
//! resposta ([`OpenReply`]), enviada depois que a janela desenha o primeiro
//! frame. O socket fica em `$XDG_RUNTIME_DIR/code-terminal-server.sock` com
//! permissão `0600`.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Quanto o cliente espera a janela aparecer.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Pedido de janela nova.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    /// Diretório onde o shell da janela nova começa (o do cliente).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// Fecha a janela logo depois do primeiro frame (para medir a abertura).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub close_after_first_frame: bool,
}

/// O que o event loop devolve: milissegundos do pedido até o primeiro frame,
/// ou o erro ao abrir a janela.
pub type WindowResult = Result<u64, String>;

/// Resposta: a janela abriu (e quanto levou até o primeiro frame) ou o erro.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenReply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_frame_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl OpenReply {
    /// Janela aberta; `first_frame_ms` conta do pedido até o primeiro frame.
    pub fn opened(first_frame_ms: u64) -> Self {
        Self {
            ok: true,
            first_frame_ms: Some(first_frame_ms),
            error: None,
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            first_frame_ms: None,
            error: Some(message.into()),
        }
    }
}

/// Socket do servidor: `CODE_TERMINAL_SERVER_SOCKET` ou
/// `$XDG_RUNTIME_DIR/code-terminal-server.sock`.
pub fn default_server_socket() -> PathBuf {
    socket_from(std::env::var_os("CODE_TERMINAL_SERVER_SOCKET"))
}

fn socket_from(custom: Option<std::ffi::OsString>) -> PathBuf {
    match custom.filter(|path| !path.is_empty()) {
        Some(path) => PathBuf::from(path),
        None => dirs::runtime_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("code-terminal-server.sock"),
    }
}

/// `CODE_TERMINAL_NO_SERVER=1` faz o `code-terminal` ignorar o servidor e
/// abrir um processo próprio.
pub fn server_disabled_by_env() -> bool {
    disables_server(std::env::var_os("CODE_TERMINAL_NO_SERVER").as_deref())
}

/// Valor da variável que desliga o servidor: qualquer coisa menos vazio e `0`.
fn disables_server(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| !v.is_empty() && v != "0")
}

/// Pede uma janela ao servidor em `socket`. `None` quando não há servidor
/// escutando (o chamador abre a janela por conta própria).
pub fn request_window(socket: &Path, request: &OpenRequest) -> Option<std::io::Result<OpenReply>> {
    let stream = UnixStream::connect(socket).ok()?;
    Some(exchange(stream, request))
}

fn exchange(mut stream: UnixStream, request: &OpenRequest) -> std::io::Result<OpenReply> {
    stream.set_read_timeout(Some(CLIENT_TIMEOUT))?;
    let mut line = serde_json::to_string(request).map_err(std::io::Error::other)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply)?;
    if reply.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "server closed the connection",
        ));
    }
    serde_json::from_str(reply.trim()).map_err(std::io::Error::other)
}

/// Servidor escutando; apaga o socket quando é descartado.
pub struct ServerGuard {
    path: PathBuf,
}

impl ServerGuard {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Começa a escutar em `path`. Cada pedido vai para `on_request` com um canal
/// por onde a resposta deve voltar (o event loop responde depois do primeiro
/// frame da janela).
pub fn start<F>(path: &Path, on_request: F) -> std::io::Result<ServerGuard>
where
    F: Fn(OpenRequest, mpsc::Sender<WindowResult>) + Send + Sync + 'static,
{
    let listener = bind(path)?;
    let on_request = std::sync::Arc::new(on_request);
    std::thread::Builder::new()
        .name("code-terminal-server".into())
        .spawn(move || accept_loop(listener, on_request))?;
    Ok(ServerGuard {
        path: path.to_path_buf(),
    })
}

/// Cria o socket com permissão `0600`. Um socket velho (de um servidor que
/// morreu) é removido; um servidor vivo no mesmo caminho é erro.
fn bind(path: &Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!(
                    "a code-terminal server is already listening on {}",
                    path.display()
                ),
            ));
        }
        std::fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

fn accept_loop<F>(listener: UnixListener, on_request: std::sync::Arc<F>)
where
    F: Fn(OpenRequest, mpsc::Sender<WindowResult>) + Send + Sync + 'static,
{
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let on_request = std::sync::Arc::clone(&on_request);
        let _ = std::thread::Builder::new()
            .name("code-terminal-server-conn".into())
            .spawn(move || serve(stream, on_request.as_ref()));
    }
}

fn serve<F>(stream: UnixStream, on_request: &F)
where
    F: Fn(OpenRequest, mpsc::Sender<WindowResult>),
{
    let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
    let mut line = String::new();
    let reply = match BufReader::new(&stream).read_line(&mut line) {
        Ok(_) => match serde_json::from_str::<OpenRequest>(line.trim()) {
            Ok(request) => {
                let (sender, receiver) = mpsc::channel();
                on_request(request, sender);
                match receiver.recv_timeout(CLIENT_TIMEOUT) {
                    Ok(Ok(ms)) => OpenReply::opened(ms),
                    Ok(Err(e)) => OpenReply::failed(e),
                    Err(_) => OpenReply::failed("window did not open in time"),
                }
            }
            Err(e) => OpenReply::failed(format!("invalid request: {e}")),
        },
        Err(e) => OpenReply::failed(format!("read failed: {e}")),
    };
    if let Ok(mut text) = serde_json::to_string(&reply) {
        text.push('\n');
        let mut stream = stream;
        let _ = stream.write_all(text.as_bytes());
    }
}

/// Espera SIGTERM, SIGINT ou SIGHUP numa thread e então chama `on_signal`
/// (o servidor apaga o socket e sai, em vez de deixá-lo para trás).
///
/// Usa um handler (tokio), não bloqueio de sinais: a máscara bloqueada seria
/// herdada pelos shells das janelas, que deixariam de morrer com SIGHUP e de
/// receber o Ctrl+C.
pub fn on_exit_signal<F>(on_signal: F)
where
    F: FnOnce() + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("code-terminal-server-signals".into())
        .spawn(move || {
            if wait_exit_signal().is_ok() {
                on_signal();
            }
        });
}

fn wait_exit_signal() -> std::io::Result<()> {
    use tokio::signal::unix::{signal, SignalKind};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?;
    runtime.block_on(async {
        let mut terminate = signal(SignalKind::terminate())?;
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut hangup = signal(SignalKind::hangup())?;
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
            _ = hangup.recv() => {}
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket_in(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join("server.sock")
    }

    #[test]
    fn test_no_server_means_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(request_window(&socket_in(&dir), &OpenRequest::default()).is_none());
    }

    #[test]
    fn test_request_reaches_handler_and_reply_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = std::sync::Arc::clone(&seen);
        let _guard = start(&path, move |request, reply| {
            log.lock().unwrap().push(request);
            let _ = reply.send(Ok(7));
        })
        .unwrap();

        let request = OpenRequest {
            cwd: Some(PathBuf::from("/tmp")),
            close_after_first_frame: false,
        };
        let reply = request_window(&path, &request).unwrap().unwrap();
        assert_eq!(reply, OpenReply::opened(7));
        assert_eq!(seen.lock().unwrap().as_slice(), &[request]);
    }

    #[test]
    fn test_socket_is_private_and_removed_on_drop() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let guard = start(&path, |_, reply| {
            let _ = reply.send(Ok(0));
        })
        .unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(guard.path(), path.as_path());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn test_second_server_is_refused_and_stale_socket_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        let first = start(&path, |_, reply| {
            let _ = reply.send(Ok(1));
        })
        .unwrap();
        let err = start(&path, |_, _| {}).err().unwrap();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);

        drop(first);

        // Socket sobrando de um servidor que morreu: o próximo assume.
        let stale = UnixListener::bind(dir.path().join("stale.sock")).unwrap();
        drop(stale);
        let stale_path = dir.path().join("stale.sock");
        assert!(stale_path.exists());
        // Outro teste em paralelo pode fazer fork no instante em que a escuta
        // ainda estava aberta: o filho a segura até o exec e o socket "velho"
        // ainda aceita conexão por um instante. Tenta de novo por até 2 s.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let _second = loop {
            match start(&stale_path, |_, reply| {
                let _ = reply.send(Ok(2));
            }) {
                Ok(server) => break server,
                Err(e)
                    if e.kind() == std::io::ErrorKind::AddrInUse
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => panic!("{e}"),
            }
        };
        let reply = request_window(&stale_path, &OpenRequest::default())
            .unwrap()
            .unwrap();
        assert_eq!(reply.first_frame_ms, Some(2));
    }

    #[test]
    fn test_handler_that_never_answers_and_bad_request() {
        let dir = tempfile::tempdir().unwrap();
        let path = socket_in(&dir);
        // O handler descarta o canal: o servidor responde com erro na hora.
        let _guard = start(&path, |request, reply| {
            if request.cwd.is_some() {
                let _ = reply.send(Err("no window".into()));
            }
        })
        .unwrap();
        let reply = request_window(&path, &OpenRequest::default())
            .unwrap()
            .unwrap();
        assert!(!reply.ok);
        assert!(reply.error.unwrap().contains("in time"));
        let failing = OpenRequest {
            cwd: Some(PathBuf::from("/")),
            close_after_first_frame: false,
        };
        let reply = request_window(&path, &failing).unwrap().unwrap();
        assert_eq!(reply, OpenReply::failed("no window"));

        let mut stream = UnixStream::connect(&path).unwrap();
        stream.write_all(b"isto nao e json\n").unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        let reply: OpenReply = serde_json::from_str(line.trim()).unwrap();
        assert!(!reply.ok);
        assert!(reply.error.unwrap().starts_with("invalid request"));
    }

    #[test]
    fn test_protocol_shape_and_env_switch() {
        let json = serde_json::to_string(&OpenRequest {
            cwd: Some(PathBuf::from("/x")),
            close_after_first_frame: true,
        })
        .unwrap();
        assert_eq!(json, r#"{"cwd":"/x","close_after_first_frame":true}"#);
        assert_eq!(
            serde_json::to_string(&OpenRequest::default()).unwrap(),
            "{}"
        );
        assert_eq!(
            serde_json::to_string(&OpenReply::failed("e")).unwrap(),
            r#"{"ok":false,"error":"e"}"#
        );
        assert!(socket_from(None).ends_with("code-terminal-server.sock"));
        assert!(socket_from(Some("".into())).ends_with("code-terminal-server.sock"));
        assert_eq!(
            socket_from(Some("/tmp/x.sock".into())),
            PathBuf::from("/tmp/x.sock")
        );
        assert!(!disables_server(None));
        assert!(!disables_server(Some("".as_ref())));
        assert!(!disables_server(Some("0".as_ref())));
        assert!(disables_server(Some("1".as_ref())));
    }
}
