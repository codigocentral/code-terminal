//! URL Detection
//!
//! Detecta URLs no conteúdo do terminal e permite interação:
//! - Highlight visual de URLs
//! - Ctrl+Click para abrir
//! - Hover com tooltip

use regex::Regex;
use std::sync::LazyLock;

static HTTP_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"https?://[a-zA-Z0-9][-a-zA-Z0-9]*(?:\.[a-zA-Z0-9][-a-zA-Z0-9]*)+(?::\d+)?(?:/[-a-zA-Z0-9._~:/?#\[\]@!$&'()*+,;=%]*)?"
    ).expect("HTTP URL regex literal must be valid")
});

static FTP_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"ftp://[a-zA-Z0-9][-a-zA-Z0-9]*(?:\.[a-zA-Z0-9][-a-zA-Z0-9]*)+(?::\d+)?(?:/[-a-zA-Z0-9._~:/?#\[\]@!$&'()*+,;=%]*)?"
    ).expect("FTP URL regex literal must be valid")
});

static FILE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"file:///[-a-zA-Z0-9._~:/?#\[\]@!$&'()*+,;=%]*")
        .expect("file URL regex literal must be valid")
});

/// URL detectada no terminal
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedUrl {
    /// URL completa
    pub url: String,
    /// Linha onde começa
    pub row: usize,
    /// Coluna onde começa
    pub start_col: usize,
    /// Coluna onde termina
    pub end_col: usize,
    /// Protocolo (http, https, ftp, etc)
    pub protocol: String,
}

/// Detector de URLs
#[derive(Debug)]
pub struct UrlDetector {
    /// URLs detectadas
    urls: Vec<DetectedUrl>,
}

impl Default for UrlDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl UrlDetector {
    /// Cria um novo detector
    pub fn new() -> Self {
        Self { urls: Vec::new() }
    }

    /// Escaneia texto e retorna URLs detectadas
    pub fn detect_in_text(&self, text: &str, row: usize, offset_col: usize) -> Vec<DetectedUrl> {
        let mut urls = Vec::new();

        // Detecta HTTP/HTTPS
        for mat in HTTP_REGEX.find_iter(text) {
            let url = mat.as_str().to_string();
            let protocol = url.split(':').next().unwrap_or("http").to_string();
            urls.push(DetectedUrl {
                url,
                row,
                start_col: offset_col + mat.start(),
                end_col: offset_col + mat.end() - 1,
                protocol,
            });
        }

        // Detecta FTP
        for mat in FTP_REGEX.find_iter(text) {
            let url = mat.as_str().to_string();
            urls.push(DetectedUrl {
                url: url.clone(),
                row,
                start_col: offset_col + mat.start(),
                end_col: offset_col + mat.end() - 1,
                protocol: "ftp".to_string(),
            });
        }

        // Detecta file://
        for mat in FILE_REGEX.find_iter(text) {
            let url = mat.as_str().to_string();
            urls.push(DetectedUrl {
                url: url.clone(),
                row,
                start_col: offset_col + mat.start(),
                end_col: offset_col + mat.end() - 1,
                protocol: "file".to_string(),
            });
        }

        urls
    }

    /// Escaneia múltiplas linhas
    pub fn scan_lines(&self, lines: &[String]) -> Vec<DetectedUrl> {
        let mut urls = Vec::new();

        for (row, line) in lines.iter().enumerate() {
            let line_urls = self.detect_in_text(line, row, 0);
            urls.extend(line_urls);
        }

        urls
    }

    /// Verifica se uma posição está dentro de uma URL
    pub fn url_at_position(&self, row: usize, col: usize) -> Option<&DetectedUrl> {
        self.urls
            .iter()
            .find(|url| url.row == row && col >= url.start_col && col <= url.end_col)
    }

    /// Retorna todas as URLs detectadas
    pub fn urls(&self) -> &[DetectedUrl] {
        &self.urls
    }

    /// Limpa URLs detectadas
    pub fn clear(&mut self) {
        self.urls.clear();
    }

    /// Atualiza URLs escaneando linhas
    pub fn update(&mut self, lines: &[String]) {
        self.urls = self.scan_lines(lines);
    }
}

/// Abre uma URL no navegador padrão
pub fn open_url(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", ""])
            .arg(url)
            .spawn()?;
    }

    Ok(())
}

/// Verifica se um caractere é parte de uma URL
pub fn is_url_char(c: char) -> bool {
    c.is_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_detection_http() {
        let detector = UrlDetector::new();
        let text = "Visite https://example.com para mais info";

        let urls = detector.detect_in_text(text, 0, 0);

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].url, "https://example.com");
        assert_eq!(urls[0].protocol, "https");
    }

    #[test]
    fn test_url_detection_http_with_path() {
        let detector = UrlDetector::new();
        let text = "https://example.com/path/to/page?query=1";

        let urls = detector.detect_in_text(text, 0, 0);

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].url, "https://example.com/path/to/page?query=1");
    }

    #[test]
    fn test_url_detection_multiple() {
        let detector = UrlDetector::new();
        let text = "Sites: http://a.com e https://b.org";

        let urls = detector.detect_in_text(text, 0, 0);

        assert_eq!(urls.len(), 2);
    }

    #[test]
    fn test_url_detection_ftp() {
        let detector = UrlDetector::new();
        let text = "Download em ftp://files.example.com/data.zip";

        let urls = detector.detect_in_text(text, 0, 0);

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].url, "ftp://files.example.com/data.zip");
        assert_eq!(urls[0].protocol, "ftp");
    }

    #[test]
    fn test_url_detection_file() {
        let detector = UrlDetector::new();
        let text = "Abra file:///home/user/document.txt";

        let urls = detector.detect_in_text(text, 0, 0);

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].url, "file:///home/user/document.txt");
        assert_eq!(urls[0].protocol, "file");
    }

    #[test]
    fn test_url_at_position() {
        let mut detector = UrlDetector::new();
        let lines = vec!["Check https://rust-lang.org today".to_string()];

        detector.update(&lines);

        assert!(detector.url_at_position(0, 10).is_some());
        assert!(detector.url_at_position(0, 0).is_none()); // "Check"
    }

    #[test]
    fn test_url_detection_with_offset() {
        let detector = UrlDetector::new();
        let text = "Check https://example.com";

        let urls = detector.detect_in_text(text, 5, 10);

        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0].row, 5);
        assert_eq!(urls[0].start_col, 16); // 10 + 6 ("Check ")
    }

    #[test]
    fn test_scan_lines() {
        let detector = UrlDetector::new();
        let lines = vec![
            "Line 1: https://a.com".to_string(),
            "Line 2: no url here".to_string(),
            "Line 3: https://b.com".to_string(),
        ];

        let urls = detector.scan_lines(&lines);

        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].row, 0);
        assert_eq!(urls[1].row, 2);
    }

    #[test]
    fn test_is_url_char() {
        assert!(is_url_char('a'));
        assert!(is_url_char('1'));
        assert!(is_url_char('/'));
        assert!(is_url_char('?'));
        assert!(is_url_char('='));
        assert!(!is_url_char(' '));
        assert!(!is_url_char('\t'));
    }
}
