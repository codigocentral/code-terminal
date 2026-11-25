//! Fakes para os testes do módulo de IA: nenhum teste fala com uma API real.
//!
//! - [`FakeProvider`]: implementa [`AIProvider`] com respostas fixas.
//! - [`FakeOpenAi`]: servidor HTTP local efêmero que imita
//!   `POST /v1/chat/completions` e grava os corpos recebidos.

use super::error::{AiError, AiResult};
use super::provider::{AIProvider, ChatRequest, ChatResponse, CommandSuggestion, TerminalContext};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Provider falso: devolve `reply` (ou erro, se `fail`) e grava as entradas.
pub struct FakeProvider {
    pub configured: bool,
    pub fail: bool,
    pub reply: String,
    pub calls: Arc<Mutex<Vec<String>>>,
}

impl FakeProvider {
    pub fn new(reply: &str) -> Self {
        Self {
            configured: true,
            fail: false,
            reply: reply.to_string(),
            calls: Arc::default(),
        }
    }

    fn record(&self, what: String) -> AiResult<()> {
        self.calls.lock().unwrap().push(what);
        if self.fail {
            Err(AiError::ApiError("falha simulada".to_string()))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl AIProvider for FakeProvider {
    async fn chat(&self, request: ChatRequest) -> AiResult<ChatResponse> {
        self.record(format!("chat:{}", request.messages.len()))?;
        Ok(ChatResponse {
            content: self.reply.clone(),
            model: "fake".to_string(),
            usage: None,
            finish_reason: None,
        })
    }

    async fn suggest_command(
        &self,
        _context: &TerminalContext,
        user_input: &str,
    ) -> AiResult<CommandSuggestion> {
        self.record(format!("suggest:{user_input}"))?;
        Ok(CommandSuggestion {
            command: self.reply.clone(),
            explanation: format!("para {user_input}"),
            alternatives: None,
            confidence: 0.9,
        })
    }

    async fn explain_error(
        &self,
        error_message: &str,
        _context: &TerminalContext,
    ) -> AiResult<String> {
        self.record(format!("explain:{error_message}"))?;
        Ok(self.reply.clone())
    }

    fn name(&self) -> &str {
        "fake"
    }

    fn is_configured(&self) -> bool {
        self.configured
    }
}

/// Servidor HTTP falso da API de chat da OpenAI.
pub struct FakeOpenAi {
    /// URL base para `AiConfig::with_base_url` (termina em `/v1`).
    pub base_url: String,
    /// Corpos JSON das requisições recebidas, na ordem.
    pub bodies: Arc<Mutex<Vec<serde_json::Value>>>,
}

impl FakeOpenAi {
    /// Responde todas as requisições com `status` e `body`.
    pub async fn start(status: u16, body: String) -> Self {
        Self::start_with_headers(status, body, Vec::new()).await
    }

    /// Como [`FakeOpenAi::start`], com cabeçalhos extras na resposta.
    pub async fn start_with_headers(
        status: u16,
        body: String,
        headers: Vec<(&'static str, String)>,
    ) -> Self {
        let extra: String = headers
            .iter()
            .map(|(k, v)| format!("{k}: {v}\r\n"))
            .collect();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let bodies: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
        let seen = Arc::clone(&bodies);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let seen = Arc::clone(&seen);
                let body = body.clone();
                let extra = extra.clone();
                tokio::spawn(Self::serve_connection(stream, seen, status, body, extra));
            }
        });
        Self {
            base_url: format!("http://{addr}/v1"),
            bodies,
        }
    }

    /// Atende uma conexão: grava o corpo JSON recebido e responde `status`/`body`.
    async fn serve_connection(
        mut stream: tokio::net::TcpStream,
        seen: Arc<Mutex<Vec<serde_json::Value>>>,
        status: u16,
        body: String,
        extra: String,
    ) {
        use tokio::io::AsyncWriteExt;
        let Some((headers, request_body)) = read_request(&mut stream).await else {
            return;
        };
        if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&request_body) {
            // guarda os cabeçalhos da requisição em "__headers"
            if let Some(obj) = v.as_object_mut() {
                obj.insert("__headers".into(), headers.clone().into());
            }
            seen.lock().unwrap().push(v);
        }
        let reason = if status == 200 { "OK" } else { "Error" };
        let resp = format!(
            "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\n\
             {extra}content-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        let _ = stream.shutdown().await;
    }

    /// Responde 200 com uma completion cujo conteúdo é `content`.
    pub async fn replying(content: &str) -> Self {
        let body = serde_json::json!({
            "id": "chatcmpl-teste",
            "object": "chat.completion",
            "created": 1,
            "model": "modelo-falso",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": content},
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 7, "completion_tokens": 5, "total_tokens": 12}
        });
        Self::start(200, body.to_string()).await
    }

    /// Corpos recebidos até agora.
    pub fn bodies(&self) -> Vec<serde_json::Value> {
        self.bodies.lock().unwrap().clone()
    }
}

/// Lê cabeçalhos (em minúsculas) e corpo (Content-Length) de uma requisição
/// HTTP; `None` se a conexão fechar antes do fim dos cabeçalhos.
async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<(String, Vec<u8>)> {
    use tokio::io::AsyncReadExt;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p + 4;
        }
    };
    let headers = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
    let len = headers
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while buf.len() < header_end + len {
        let n = match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = buf.split_off(header_end);
    Some((headers, body))
}

/// Cria um executável falso (script sh) que grava cada chamada em
/// `<script>.log` (um argumento por linha, seguido de `---`) e roda `body`.
#[cfg(unix)]
pub fn fake_cli(dir: &tempfile::TempDir, name: &str, body: &str) -> String {
    crate::test_exec::write_fake_executable(
        &dir.path().join(name),
        &format!(
            "{{ for a in \"$@\"; do printf '%s\\n' \"$a\"; done; echo ---; }} >> \"$0.log\"\n{body}"
        ),
    )
}

/// Chamadas gravadas por [`fake_cli`]: uma lista de argumentos por chamada.
#[cfg(unix)]
pub fn fake_cli_calls(program: &str) -> Vec<Vec<String>> {
    let log = std::fs::read_to_string(format!("{program}.log")).unwrap_or_default();
    let mut calls = Vec::new();
    let mut cur = Vec::new();
    for line in log.lines() {
        if line == "---" {
            calls.push(std::mem::take(&mut cur));
        } else {
            cur.push(line.to_string());
        }
    }
    calls
}
