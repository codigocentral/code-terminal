//! Navegador em janela própria, controlado pelo terminal.
//!
//! O processo `code-terminal-browser` abre uma janela com um WebView (GTK +
//! WebKit no Linux, que funciona também no Wayland; WebView2/WKWebView no
//! Windows/macOS) e atende pedidos JSON, um por linha, num socket local:
//! abrir endereço, voltar, avançar, recarregar, ler o texto da página, rodar
//! JavaScript, informar URL e título, fechar.
//!
//! No Unix o socket fica em `$XDG_RUNTIME_DIR/code-terminal-browser.sock`
//! com permissão `0600`. No Windows é TCP local com um token aleatório
//! gravado num arquivo do perfil do usuário.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Pedido ao navegador.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum BrowserRequest {
    /// Abre o endereço (já normalizado por [`normalize_url`]).
    Open {
        url: String,
    },
    Back,
    Forward,
    Reload,
    /// Roda JavaScript; a resposta traz o resultado em JSON.
    Eval {
        js: String,
    },
    /// URL e título atuais.
    Info,
    /// Texto visível da página (`document.body.innerText`).
    Text,
    /// Traz a janela para a frente.
    Focus,
    /// Fecha a janela e encerra o processo.
    Close,
}

/// Resposta do navegador.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserReply {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Resultado de `eval`/`text`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Token de acesso (só no Windows, onde o canal é TCP).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl BrowserReply {
    pub fn ok() -> Self {
        Self {
            ok: true,
            ..Self::default()
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            ..Self::default()
        }
    }
}

/// Pedido com o token (Windows); no Unix o token vai vazio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    #[serde(default)]
    pub token: String,
    #[serde(flatten)]
    pub request: BrowserRequest,
}

/// Esquemas que o navegador abre. `javascript:`, `data:` e afins ficam de
/// fora: um link no terminal não pode virar código na página.
const ALLOWED_SCHEMES: &[&str] = &["http", "https", "file", "about"];

/// Transforma o que o usuário digitou num endereço que o navegador abre:
/// caminho local existente vira `file://`, `exemplo.com` vira `https://…`.
pub fn normalize_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok("about:blank".to_string());
    }
    let local = Path::new(input);
    if local.exists() {
        let absolute = std::fs::canonicalize(local).map_err(|e| e.to_string())?;
        return url::Url::from_file_path(&absolute)
            .map(|u| u.to_string())
            .map_err(|_| format!("invalid path: {input}"));
    }
    let candidate = match url::Url::parse(input) {
        Ok(url) if url.scheme().len() > 1 => url,
        // Sem esquema (ou "c:" de um caminho do Windows inexistente).
        _ => url::Url::parse(&format!("https://{input}")).map_err(|e| e.to_string())?,
    };
    if ALLOWED_SCHEMES.contains(&candidate.scheme()) {
        Ok(candidate.to_string())
    } else {
        Err(format!("scheme not allowed: {}", candidate.scheme()))
    }
}

/// Socket do navegador (Unix).
pub fn default_browser_socket() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("code-terminal-browser.sock")
}

/// Arquivo com porta e token do navegador (Windows).
pub fn default_browser_endpoint_file() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("code-terminal")
        .join("browser.endpoint")
}

/// Stream com o navegador.
pub trait BrowserStream: std::io::Read + Write + Send {}
impl<T: std::io::Read + Write + Send> BrowserStream for T {}

/// Abre a conexão padrão; devolve o stream e o token a mandar.
pub fn connect_default() -> std::io::Result<(Box<dyn BrowserStream>, String)> {
    #[cfg(unix)]
    {
        let stream = std::os::unix::net::UnixStream::connect(default_browser_socket())?;
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
        Ok((Box::new(stream), String::new()))
    }
    #[cfg(not(unix))]
    {
        let content = std::fs::read_to_string(default_browser_endpoint_file())?;
        let (port, token) = content.trim().split_once(' ').ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "bad endpoint file")
        })?;
        let stream = std::net::TcpStream::connect(("127.0.0.1", port.parse().unwrap_or(0)))?;
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
        Ok((Box::new(stream), token.to_string()))
    }
}

/// Manda um pedido e lê a resposta.
pub fn request_on(
    stream: &mut dyn BrowserStream,
    token: &str,
    request: BrowserRequest,
) -> std::io::Result<BrowserReply> {
    let envelope = Envelope {
        token: token.to_string(),
        request,
    };
    let mut line = serde_json::to_string(&envelope).map_err(std::io::Error::other)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    if reply.trim().is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "browser closed the connection",
        ));
    }
    serde_json::from_str(reply.trim()).map_err(std::io::Error::other)
}

/// Pedido ao navegador padrão (já rodando).
pub fn request(request: BrowserRequest) -> std::io::Result<BrowserReply> {
    let (mut stream, token) = connect_default()?;
    request_on(stream.as_mut(), &token, request)
}

/// Executável do navegador: ao lado do executável atual, ou no PATH.
pub fn browser_executable() -> PathBuf {
    let name = if cfg!(windows) {
        "code-terminal-browser.exe"
    } else {
        "code-terminal-browser"
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Abre `url` no navegador, subindo a janela se ela ainda não existe.
pub fn open_in_browser(url: &str) -> Result<BrowserReply, String> {
    let url = normalize_url(url)?;
    if let Ok(reply) = request(BrowserRequest::Open { url: url.clone() }) {
        let _ = request(BrowserRequest::Focus);
        return Ok(reply);
    }
    spawn_browser(&url)?;
    // Espera o socket aparecer (a primeira página já vai no argumento).
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        match request(BrowserRequest::Info) {
            Ok(reply) => return Ok(reply),
            Err(_) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(format!("browser did not start: {e}")),
        }
    }
}

fn spawn_browser(url: &str) -> Result<(), String> {
    let mut command = std::process::Command::new(browser_executable());
    command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to start {}: {e}", browser_executable().display()))
}

/// Script que devolve o texto visível da página.
pub const TEXT_SCRIPT: &str = "document.body ? document.body.innerText : ''";

/// Resultado de `evaluate_script_with_callback` (JSON) como texto: strings
/// JSON viram o texto puro; o resto fica como veio.
pub fn eval_result_text(json: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(serde_json::Value::String(text)) => text,
        Ok(serde_json::Value::Null) => String::new(),
        _ => json.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            normalize_url("example.com").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            normalize_url("http://localhost:3000/x").unwrap(),
            "http://localhost:3000/x"
        );
        assert_eq!(normalize_url("").unwrap(), "about:blank");
        assert!(normalize_url("javascript:alert(1)").is_err());
        assert!(normalize_url("data:text/html,oi").is_err());
        let dir = tempfile::tempdir().unwrap();
        let page = dir.path().join("p.html");
        std::fs::write(&page, "<p>oi</p>").unwrap();
        let url = normalize_url(&page.to_string_lossy()).unwrap();
        assert!(
            url.starts_with("file://") && url.ends_with("/p.html"),
            "{url}"
        );
    }

    #[test]
    fn test_protocol_round_trip() {
        let envelope = Envelope {
            token: "t".into(),
            request: BrowserRequest::Eval { js: "1+1".into() },
        };
        let json = serde_json::to_string(&envelope).unwrap();
        assert_eq!(json, r#"{"token":"t","cmd":"eval","js":"1+1"}"#);
        assert_eq!(serde_json::from_str::<Envelope>(&json).unwrap(), envelope);
        let plain: Envelope = serde_json::from_str(r#"{"cmd":"back"}"#).unwrap();
        assert_eq!(plain.request, BrowserRequest::Back);
        let reply = serde_json::to_string(&BrowserReply::error("x")).unwrap();
        assert_eq!(reply, r#"{"ok":false,"error":"x"}"#);
    }

    #[test]
    fn test_eval_result_text() {
        assert_eq!(eval_result_text(r#""linha\nduas""#), "linha\nduas");
        assert_eq!(eval_result_text("2"), "2");
        assert_eq!(eval_result_text("null"), "");
    }
}
