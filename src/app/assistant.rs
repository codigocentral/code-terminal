//! Assistente de IA do terminal: explicar a saída do painel e sugerir um
//! comando a partir de uma descrição.
//!
//! A chamada roda numa thread de fundo (a interface nunca espera a rede).
//! Provedores, pela configuração `[ai]`:
//! - `anthropic`: Messages API (`claude-opus-5-5` por padrão, esforço baixo);
//! - `openai`, `local`, `custom`: API de chat compatível com OpenAI;
//! - `ollama`: `/api/chat` do Ollama.
//!
//! Antes de enviar, segredos com cara de token/senha são trocados por
//! `[redacted]` e a home vira `~`.

use crate::config::ai::{AIConfig, AIProvider};
use serde_json::{json, Value};

/// Modelo padrão quando o provedor é a Anthropic e a configuração ainda tem
/// o padrão de outro provedor.
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";
const ANTHROPIC_VERSION: &str = "2026-10-21";
/// Fallback do servidor quando o modelo recusa (o API escolhe o modelo).
const ANTHROPIC_FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// O que pedir ao modelo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssistantTask {
    /// Explicar a saída recente do painel (erros, avisos, o que aconteceu).
    Explain { output: String },
    /// Sugerir um comando para o pedido em linguagem natural.
    Suggest {
        request: String,
        cwd: Option<String>,
        shell: String,
    },
}

impl AssistantTask {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Explain { .. } => crate::ui::i18n::tr(crate::ui::i18n::Text::AiExplainTitle),
            Self::Suggest { .. } => {
                crate::ui::i18n::tr(crate::ui::i18n::Text::AiSuggestCommandTitle)
            }
        }
    }
}

/// Pedido HTTP pronto para enviar.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistantRequest {
    pub provider: AIProvider,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

/// Troca segredos óbvios por `[redacted]` e a home por `~`.
pub fn redact(text: &str, home: Option<&str>) -> String {
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let mut words = Vec::new();
        for word in line.split(' ') {
            words.push(redact_word(word));
        }
        out.push_str(&words.join(" "));
    }
    match home.filter(|h| h.len() > 1) {
        Some(home) => out.replace(home, "~"),
        None => out,
    }
}

fn redact_word(word: &str) -> String {
    const TOKEN_PREFIXES: &[&str] = &[
        "sk-",
        "sk_",
        "ghp_",
        "gho_",
        "ghs_",
        "github_pat_",
        "glpat-",
        "xoxb-",
        "xoxp-",
        "AKIA",
        "AIza",
        "eyJ",
    ];
    if TOKEN_PREFIXES.iter().any(|p| word.starts_with(p)) && word.len() >= 16 {
        return "[redacted]".to_string();
    }
    // chave=valor com nome de segredo
    if let Some((key, value)) = word.split_once('=') {
        let key_lower = key.to_ascii_lowercase();
        let secret = ["password", "passwd", "secret", "token", "apikey", "api_key"]
            .iter()
            .any(|k| key_lower.contains(k));
        if secret && !value.is_empty() {
            return format!("{key}=[redacted]");
        }
    }
    word.to_string()
}

fn prompts(task: &AssistantTask) -> (String, String) {
    match task {
        AssistantTask::Explain { output } => (
            "You help a developer understand terminal output. Explain briefly what \
             happened, the likely cause of any error, and the next step to fix it. \
             Plain text, at most 12 short lines, no preamble."
                .to_string(),
            format!("Terminal output (most recent at the bottom):\n\n{output}"),
        ),
        AssistantTask::Suggest {
            request,
            cwd,
            shell,
        } => (
            format!(
                "You turn a request into one shell command for {shell} on {}. Reply with \
                 the command in a single ```sh code block, then one short line explaining \
                 it. Prefer safe, non-destructive commands; never use sudo unless asked.",
                std::env::consts::OS
            ),
            match cwd {
                Some(cwd) => format!("Current directory: {cwd}\nRequest: {request}"),
                None => format!("Request: {request}"),
            },
        ),
    }
}

/// Chave de API: a variável configurada, senão a padrão do provedor.
pub fn api_key(config: &AIConfig) -> Option<String> {
    let var = if config.model.api_key_env.is_empty() {
        match config.provider {
            AIProvider::Anthropic => "ANTHROPIC_API_KEY",
            _ => "OPENAI_API_KEY",
        }
    } else {
        config.model.api_key_env.as_str()
    };
    std::env::var(var).ok().filter(|k| !k.trim().is_empty())
}

/// Nome do modelo para o provedor (troca o padrão de outro provedor).
pub fn model_for(config: &AIConfig) -> String {
    let name = config.model.name.trim();
    match config.provider {
        AIProvider::Anthropic if name.is_empty() || !name.starts_with("claude") => {
            DEFAULT_ANTHROPIC_MODEL.to_string()
        }
        _ => name.to_string(),
    }
}

/// Monta o pedido para o provedor configurado.
pub fn build_request(
    config: &AIConfig,
    task: &AssistantTask,
    api_key: Option<&str>,
) -> Result<AssistantRequest, String> {
    let (system, user) = prompts(task);
    let base = config.get_base_url();
    let base = base.trim_end_matches('/');
    let model = model_for(config);
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    let needs_key = !matches!(config.provider, AIProvider::Ollama | AIProvider::Local);
    if needs_key && api_key.is_none() {
        return Err("no API key".to_string());
    }
    for (key, value) in &config.model.headers {
        headers.push((key.clone(), value.clone()));
    }
    let (url, body) = match config.provider {
        AIProvider::Anthropic => {
            headers.push(("x-api-key".into(), api_key.unwrap_or_default().into()));
            headers.push(("anthropic-version".into(), ANTHROPIC_VERSION.into()));
            headers.push(("anthropic-beta".into(), ANTHROPIC_FALLBACK_BETA.into()));
            (
                format!("{base}/messages"),
                json!({
                    "model": model,
                    "max_tokens": 16000,
                    "system": system,
                    "messages": [{"role": "user", "content": user}],
                    // Tarefa curta: pouco raciocínio, resposta rápida.
                    "output_config": {"effort": "low"},
                    "fallbacks": "default",
                }),
            )
        }
        AIProvider::Ollama => (
            format!("{base}/chat"),
            json!({
                "model": model,
                "stream": false,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user},
                ],
            }),
        ),
        AIProvider::OpenAI | AIProvider::Local | AIProvider::Custom => {
            if let Some(key) = api_key {
                headers.push(("authorization".into(), format!("Bearer {key}")));
            }
            (
                format!("{base}/chat/completions"),
                json!({
                    "model": model,
                    "max_tokens": config.model.max_tokens.max(256),
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": user},
                    ],
                }),
            )
        }
    };
    Ok(AssistantRequest {
        provider: config.provider,
        url,
        headers,
        body,
    })
}

/// Texto da resposta do provedor.
pub fn parse_response(provider: AIProvider, response: &Value) -> Result<String, String> {
    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        return Err(message);
    }
    let text = match provider {
        AIProvider::Anthropic => {
            if response.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
                return Err("The model declined this request.".to_string());
            }
            // Só os blocos de texto (pode haver blocos de raciocínio vazios).
            response
                .get("content")
                .and_then(Value::as_array)
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
        }
        AIProvider::Ollama => response
            .pointer("/message/content")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => response
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .map(str::to_string),
    };
    text.map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .ok_or_else(|| "empty response".to_string())
}

/// Comando sugerido: o primeiro bloco de código, senão a primeira linha.
pub fn extract_command(answer: &str) -> Option<String> {
    let mut in_block = false;
    let mut lines = Vec::new();
    for line in answer.lines() {
        if line.trim_start().starts_with("```") {
            if in_block {
                break;
            }
            in_block = true;
            continue;
        }
        if in_block {
            lines.push(line);
        }
    }
    let command = if lines.is_empty() {
        answer.lines().next().unwrap_or("").trim().to_string()
    } else {
        lines.join("\n").trim().to_string()
    };
    (!command.is_empty()).then_some(command)
}

/// Envia o pedido (bloqueia: chame numa thread de fundo).
pub fn send(request: &AssistantRequest) -> Result<String, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        let mut builder = client.post(&request.url).json(&request.body);
        for (key, value) in &request.headers {
            builder = builder.header(key.as_str(), value.as_str());
        }
        let response = builder.send().await.map_err(|e| e.to_string())?;
        let status = response.status();
        let body: Value = response.json().await.map_err(|e| e.to_string())?;
        match parse_response(request.provider, &body) {
            Ok(text) if status.is_success() => Ok(text),
            Ok(_) => Err(format!("HTTP {status}")),
            Err(message) => Err(format!("{message} (HTTP {status})")),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: AIProvider) -> AIConfig {
        let mut config = AIConfig::default();
        config.enabled = true;
        config.provider = provider;
        config
    }

    #[test]
    fn test_redact_secrets_and_home() {
        let text = "export TOKEN=abc123\nusing ghp_ABCDEFGHIJKLMNOPQRSTUV in /home/example/proj\npassword=hunter2 ok";
        let out = redact(text, Some("/home/example"));
        assert_eq!(
            out,
            "export TOKEN=[redacted]\nusing [redacted] in ~/proj\npassword=[redacted] ok"
        );
        assert_eq!(redact("sk-short", None), "sk-short");
    }

    #[test]
    fn test_anthropic_request_shape() {
        let task = AssistantTask::Explain {
            output: "error: x".into(),
        };
        let request = build_request(&config(AIProvider::Anthropic), &task, Some("k")).unwrap();
        assert_eq!(request.url, "https://api.anthropic.com/v1/messages");
        let header = |name: &str| {
            request
                .headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(header("x-api-key"), Some("k"));
        assert_eq!(header("anthropic-version"), Some("2026-10-21"));
        assert_eq!(
            header("anthropic-beta"),
            Some("server-side-fallback-2026-07-01")
        );
        assert_eq!(request.body["model"], DEFAULT_ANTHROPIC_MODEL);
        assert_eq!(request.body["output_config"]["effort"], "low");
        assert_eq!(request.body["fallbacks"], "default");
        assert!(request.body.get("temperature").is_none());
        assert!(request.body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("error: x"));
        // Sem chave não monta.
        assert!(build_request(&config(AIProvider::Anthropic), &task, None).is_err());
    }

    #[test]
    fn test_openai_and_ollama_request_shape() {
        let task = AssistantTask::Suggest {
            request: "list big files".into(),
            cwd: Some("~/x".into()),
            shell: "bash".into(),
        };
        let openai = build_request(&config(AIProvider::OpenAI), &task, Some("k")).unwrap();
        assert_eq!(openai.url, "https://api.openai.com/v1/chat/completions");
        assert!(openai
            .headers
            .contains(&("authorization".into(), "Bearer k".into())));
        assert_eq!(
            openai.body["messages"][1]["content"],
            "Current directory: ~/x\nRequest: list big files"
        );
        let mut ollama_config = config(AIProvider::Ollama);
        ollama_config.model.name = "llama3".into();
        let ollama = build_request(&ollama_config, &task, None).unwrap();
        assert_eq!(ollama.url, "http://localhost:11434/api/chat");
        assert_eq!(ollama.body["stream"], false);
    }

    #[test]
    fn test_parse_responses() {
        let anthropic = json!({
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "text", "text": " The build failed. "}
            ],
            "stop_reason": "end_turn"
        });
        assert_eq!(
            parse_response(AIProvider::Anthropic, &anthropic).unwrap(),
            "The build failed."
        );
        let refused = json!({"content": [], "stop_reason": "refusal"});
        assert!(parse_response(AIProvider::Anthropic, &refused).is_err());
        let openai = json!({"choices": [{"message": {"content": "ok"}}]});
        assert_eq!(parse_response(AIProvider::OpenAI, &openai).unwrap(), "ok");
        let ollama = json!({"message": {"content": "hi"}});
        assert_eq!(parse_response(AIProvider::Ollama, &ollama).unwrap(), "hi");
        let error = json!({"error": {"message": "invalid x-api-key"}});
        assert_eq!(
            parse_response(AIProvider::Anthropic, &error).unwrap_err(),
            "invalid x-api-key"
        );
    }

    #[test]
    fn test_extract_command_and_model() {
        assert_eq!(
            extract_command("```sh\ndu -ah . | sort -rh | head\n```\nLists big files.").as_deref(),
            Some("du -ah . | sort -rh | head")
        );
        assert_eq!(extract_command("ls -la").as_deref(), Some("ls -la"));
        assert_eq!(extract_command(""), None);
        let mut cfg = config(AIProvider::Anthropic);
        cfg.model.name = "gpt-4o-mini".into();
        assert_eq!(model_for(&cfg), DEFAULT_ANTHROPIC_MODEL);
        cfg.model.name = "claude-sonnet-5-5".into();
        assert_eq!(model_for(&cfg), "claude-sonnet-5-5");
    }

    /// Ponta a ponta contra um servidor local que imita a Messages API.
    #[test]
    fn test_send_against_local_server() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let mut request = Vec::new();
            loop {
                let n = stream.read(&mut buf).unwrap();
                request.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(head_end) = text.find("\r\n\r\n") {
                    let length = text[..head_end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= head_end + 4 + length {
                        break;
                    }
                }
            }
            let body =
                r#"{"content":[{"type":"text","text":"It worked."}],"stop_reason":"end_turn"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            String::from_utf8_lossy(&request).to_string()
        });
        let mut cfg = config(AIProvider::Anthropic);
        cfg.model.base_url = Some(format!("http://{addr}/v1"));
        let request = build_request(
            &cfg,
            &AssistantTask::Explain { output: "x".into() },
            Some("test-key"),
        )
        .unwrap();
        assert_eq!(send(&request).unwrap(), "It worked.");
        let seen = server.join().unwrap().to_ascii_lowercase();
        assert!(seen.starts_with("post /v1/messages"));
        assert!(seen.contains("x-api-key: test-key"));
        assert!(seen.contains("anthropic-version: 2026-10-21"));
    }
}
