//! `code-terminal-cli preview <arquivo>`: mostra o arquivo no próprio
//! terminal. Imagens aparecem inline (protocolo do iTerm2, que o Code
//! Terminal entende), markdown e JSON saem formatados e o resto com número
//! de linha.

use base64::Engine;
use std::path::Path;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const ITALIC: &str = "\x1b[3m";
const CYAN: &str = "\x1b[36m";
const BLUE: &str = "\x1b[34m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";

/// Tipo de conteúdo, pela extensão.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewKind {
    Image,
    Markdown,
    Json,
    Text,
}

pub fn kind_of(path: &Path) -> PreviewKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" => PreviewKind::Image,
        "md" | "markdown" => PreviewKind::Markdown,
        "json" => PreviewKind::Json,
        _ => PreviewKind::Text,
    }
}

/// Sequência do iTerm2 com a imagem (largura em células, `None` = natural).
pub fn image_sequence(bytes: &[u8], width_cells: Option<u32>) -> String {
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let width = width_cells
        .map(|w| format!(";width={w}"))
        .unwrap_or_default();
    format!(
        "\x1b]1337;File=inline=1;size={}{width}:{data}\x07\n",
        bytes.len()
    )
}

/// Ênfase inline: `**negrito**`, `*itálico*`/`_itálico_` e `` `código` ``.
fn render_inline(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 16);
    let mut rest = line;
    while !rest.is_empty() {
        // `_` no meio de palavra (snake_case) não é ênfase.
        let after_word = out.chars().last().is_some_and(char::is_alphanumeric);
        let (marker, style, end_style) = if rest.starts_with("**") {
            ("**", BOLD, RESET)
        } else if rest.starts_with('`') {
            ("`", YELLOW, RESET)
        } else if rest.starts_with('*') || (rest.starts_with('_') && !after_word) {
            (&rest[..1], ITALIC, RESET)
        } else if rest.starts_with('_') {
            out.push('_');
            rest = &rest[1..];
            continue;
        } else {
            let next = rest
                .find(['*', '`', '_'])
                .filter(|&i| i > 0)
                .unwrap_or(rest.len());
            out.push_str(&rest[..next]);
            rest = &rest[next..];
            continue;
        };
        let inner = &rest[marker.len()..];
        match inner.find(marker) {
            Some(end) if end > 0 => {
                out.push_str(style);
                out.push_str(&inner[..end]);
                out.push_str(end_style);
                rest = &inner[end + marker.len()..];
            }
            _ => {
                out.push_str(marker);
                rest = inner;
            }
        }
    }
    out
}

/// Markdown para texto com cores: títulos, listas, citações e blocos de código.
pub fn render_markdown(source: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in source.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            out.push_str(&format!("{DIM}{}{RESET}\n", "─".repeat(40)));
        } else if in_code {
            out.push_str(&format!("{GREEN}  {line}{RESET}\n"));
        } else {
            out.push_str(&render_markdown_line(line));
        }
    }
    out
}

/// Título `#`..`######`: nível e texto.
fn heading(trimmed: &str) -> Option<(usize, &str)> {
    let level = trimmed.chars().take_while(|&c| c == '#').count();
    (level > 0 && trimmed[level..].starts_with(' ')).then(|| (level, trimmed[level..].trim()))
}

/// Uma linha de markdown fora de bloco de código.
fn render_markdown_line(line: &str) -> String {
    let trimmed = line.trim_start();
    if let Some((level, text)) = heading(trimmed) {
        let color = match level {
            1 => MAGENTA,
            2 => CYAN,
            _ => BLUE,
        };
        let underline = if level == 1 {
            format!("{DIM}{}{RESET}\n", "═".repeat(text.chars().count()))
        } else {
            String::new()
        };
        return format!("{BOLD}{color}{text}{RESET}\n{underline}");
    }
    if let Some(item) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let indent = &line[..line.len() - trimmed.len()];
        return format!("{indent}{CYAN}•{RESET} {}\n", render_inline(item));
    }
    if let Some(quote) = trimmed.strip_prefix("> ") {
        return format!("{DIM}│{RESET} {ITALIC}{}{RESET}\n", render_inline(quote));
    }
    if trimmed.starts_with("---") && trimmed.chars().all(|c| c == '-') {
        return format!("{DIM}{}{RESET}\n", "─".repeat(40));
    }
    format!("{}\n", render_inline(line))
}

/// JSON indentado e colorido (chaves, textos, números, literais).
pub fn render_json(source: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(source).map_err(|e| e.to_string())?;
    let mut out = String::new();
    write_json(&value, 0, &mut out);
    out.push('\n');
    Ok(out)
}

fn write_json(value: &serde_json::Value, indent: usize, out: &mut String) {
    use serde_json::Value;
    let pad = |n: usize| "  ".repeat(n);
    match value {
        Value::Object(map) if !map.is_empty() => {
            out.push_str("{\n");
            for (i, (key, item)) in map.iter().enumerate() {
                out.push_str(&format!("{}{BLUE}{key:?}{RESET}: ", pad(indent + 1)));
                write_json(item, indent + 1, out);
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{}}}", pad(indent)));
        }
        Value::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad(indent + 1));
                write_json(item, indent + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&format!("{}]", pad(indent)));
        }
        Value::String(text) => out.push_str(&format!("{GREEN}{text:?}{RESET}")),
        Value::Number(n) => out.push_str(&format!("{YELLOW}{n}{RESET}")),
        Value::Bool(b) => out.push_str(&format!("{MAGENTA}{b}{RESET}")),
        Value::Null => out.push_str(&format!("{DIM}null{RESET}")),
        other => out.push_str(&other.to_string()),
    }
}

/// Texto com número de linha.
pub fn render_text(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let width = lines.len().max(1).to_string().len();
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{DIM}{:>width$}│{RESET} {line}\n", i + 1))
        .collect()
}

/// Monta a saída do preview de um arquivo.
pub fn preview(path: &Path, width_cells: Option<u32>) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    match kind_of(path) {
        PreviewKind::Image => Ok(image_sequence(&bytes, width_cells)),
        kind => {
            let text = String::from_utf8_lossy(&bytes);
            match kind {
                PreviewKind::Markdown => Ok(render_markdown(&text)),
                PreviewKind::Json => render_json(&text).or_else(|_| Ok(render_text(&text))),
                _ => Ok(render_text(&text)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kind_of() {
        assert_eq!(kind_of(Path::new("a/b.PNG")), PreviewKind::Image);
        assert_eq!(kind_of(Path::new("README.md")), PreviewKind::Markdown);
        assert_eq!(kind_of(Path::new("x.json")), PreviewKind::Json);
        assert_eq!(kind_of(Path::new("main.rs")), PreviewKind::Text);
        assert_eq!(kind_of(Path::new("Makefile")), PreviewKind::Text);
    }

    #[test]
    fn test_image_sequence_is_iterm2_inline() {
        let seq = image_sequence(b"abc", Some(30));
        assert!(seq.starts_with("\x1b]1337;File=inline=1;size=3;width=30:YWJj\x07"));
    }

    #[test]
    fn test_markdown_rendering() {
        let out = render_markdown(
            "# Title\n- **bold** and `code`\n> quote\n```\nlet x = 1;\n```\nplain *it*\n",
        );
        assert!(out.contains(&format!("{BOLD}{MAGENTA}Title{RESET}")));
        assert!(out.contains(&format!(
            "{CYAN}•{RESET} {BOLD}bold{RESET} and {YELLOW}code{RESET}"
        )));
        assert!(out.contains(&format!("{GREEN}  let x = 1;{RESET}")));
        assert!(out.contains(&format!("plain {ITALIC}it{RESET}")));
        // Marcadores sem par ficam como estão.
        assert_eq!(render_inline("a * b"), "a * b");
        assert_eq!(render_inline("snake_case_name"), "snake_case_name");
    }

    #[test]
    fn test_json_and_text_rendering() {
        let out = render_json(r#"{"a":[1,true,null],"b":"x"}"#).unwrap();
        assert!(out.contains(&format!("{BLUE}\"a\"{RESET}: [")));
        assert!(out.contains(&format!("{YELLOW}1{RESET}")));
        assert!(out.contains(&format!("{GREEN}\"x\"{RESET}")));
        assert!(render_json("{oops").is_err());
        assert_eq!(
            render_text("a\nb\n"),
            format!("{DIM}1│{RESET} a\n{DIM}2│{RESET} b\n")
        );
    }

    #[test]
    fn test_preview_reads_files() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("x.md");
        std::fs::write(&md, "## Sub\n").unwrap();
        assert!(preview(&md, None).unwrap().contains("Sub"));
        let bad_json = dir.path().join("y.json");
        std::fs::write(&bad_json, "{oops").unwrap();
        assert!(preview(&bad_json, None).unwrap().contains("{oops"));
        assert!(preview(&dir.path().join("nada.txt"), None).is_err());
    }
}
