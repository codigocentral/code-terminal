use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

type ErrorPattern = (Regex, ErrorType, ErrorSeverity);

static ERROR_PATTERNS: LazyLock<Vec<ErrorPattern>> = LazyLock::new(build_patterns);
static FILE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|\s|[\x27\x22])([^\s\x27\x22]+\.[a-zA-Z]{1,10})(?:\s|[\x27\x22]|$|:)")
        .expect("file detection regex literal must be valid")
});

fn error_regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("error detection regex literal must be valid")
}

fn build_patterns() -> Vec<ErrorPattern> {
    vec![
        (
            error_regex(r"command not found:?\s*(\S+)"),
            ErrorType::CommandNotFound,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"(\S+):\s*command not found"),
            ErrorType::CommandNotFound,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"(?i)permission denied"),
            ErrorType::PermissionDenied,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"access denied"),
            ErrorType::PermissionDenied,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"EACCES"),
            ErrorType::PermissionDenied,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"no such file or directory:?\s*(.+)"),
            ErrorType::FileNotFoundError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"cannot find (?:module|file):?\s*(.+)"),
            ErrorType::FileNotFoundError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"ENOENT"),
            ErrorType::FileNotFoundError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"syntax error"),
            ErrorType::SyntaxError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"unexpected token"),
            ErrorType::SyntaxError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"parse error"),
            ErrorType::SyntaxError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"(?:connection refused|network unreachable|ETIMEDOUT|ENOTFOUND)"),
            ErrorType::NetworkError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"curl:.*failed"),
            ErrorType::NetworkError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"wget:.*failed"),
            ErrorType::NetworkError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"error\[E\d{4}\]"),
            ErrorType::RustError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"cannot find (?:type|function|module|crate|item)"),
            ErrorType::RustError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"cargo: error"),
            ErrorType::RustError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"Traceback \(most recent call last\)"),
            ErrorType::PythonError,
            ErrorSeverity::High,
        ),
        (
            error_regex(
                r"(?:ModuleNotFoundError|ImportError|SyntaxError|TypeError|ValueError|KeyError)",
            ),
            ErrorType::PythonError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"npm ERR!"),
            ErrorType::NpmError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"ERESOLVE"),
            ErrorType::NpmError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"yarn error"),
            ErrorType::NpmError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"fatal:"),
            ErrorType::GitError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"merge conflict"),
            ErrorType::GitError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"HEAD detached"),
            ErrorType::GitError,
            ErrorSeverity::Low,
        ),
        (
            error_regex(r"docker: Error"),
            ErrorType::DockerError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"cannot connect to the Docker daemon"),
            ErrorType::DockerError,
            ErrorSeverity::Critical,
        ),
        (
            error_regex(r"no such container"),
            ErrorType::DockerError,
            ErrorSeverity::Medium,
        ),
        (
            error_regex(r"segmentation fault"),
            ErrorType::RuntimeError,
            ErrorSeverity::Critical,
        ),
        (
            error_regex(r"stack overflow"),
            ErrorType::RuntimeError,
            ErrorSeverity::Critical,
        ),
        (
            error_regex(r"out of memory"),
            ErrorType::RuntimeError,
            ErrorSeverity::Critical,
        ),
        (
            error_regex(r"build failed"),
            ErrorType::BuildError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"compilation failed"),
            ErrorType::BuildError,
            ErrorSeverity::High,
        ),
        (
            error_regex(r"linker error"),
            ErrorType::BuildError,
            ErrorSeverity::High,
        ),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorType {
    CommandNotFound,
    PermissionDenied,
    FileNotFoundError,
    SyntaxError,
    NetworkError,
    BuildError,
    RuntimeError,
    GitError,
    DockerError,
    NpmError,
    PythonError,
    RustError,
    Unknown,
}

impl std::fmt::Display for ErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandNotFound => write!(f, "Command Not Found"),
            Self::PermissionDenied => write!(f, "Permission Denied"),
            Self::FileNotFoundError => write!(f, "File Not Found"),
            Self::SyntaxError => write!(f, "Syntax Error"),
            Self::NetworkError => write!(f, "Network Error"),
            Self::BuildError => write!(f, "Build Error"),
            Self::RuntimeError => write!(f, "Runtime Error"),
            Self::GitError => write!(f, "Git Error"),
            Self::DockerError => write!(f, "Docker Error"),
            Self::NpmError => write!(f, "NPM Error"),
            Self::PythonError => write!(f, "Python Error"),
            Self::RustError => write!(f, "Rust Error"),
            Self::Unknown => write!(f, "Unknown Error"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedError {
    pub error_type: ErrorType,
    pub message: String,
    pub context: Vec<String>,
    pub line_number: Option<usize>,
    pub severity: ErrorSeverity,
    pub suggestions: Vec<String>,
    pub related_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "Low"),
            Self::Medium => write!(f, "Medium"),
            Self::High => write!(f, "High"),
            Self::Critical => write!(f, "Critical"),
        }
    }
}

pub struct ErrorDetector {
    context_lines: usize,
}

impl Default for ErrorDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ErrorDetector {
    pub fn new() -> Self {
        Self { context_lines: 3 }
    }

    pub fn with_context_lines(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }

    pub fn detect(&self, output: &str) -> Option<DetectedError> {
        let lines: Vec<&str> = output.lines().collect();

        for (line_idx, line) in lines.iter().enumerate() {
            for (regex, error_type, severity) in ERROR_PATTERNS.iter() {
                if let Some(captures) = regex.captures(line) {
                    let message = captures
                        .get(1)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_else(|| line.to_string());

                    let start = line_idx.saturating_sub(self.context_lines);
                    let end = (line_idx + self.context_lines + 1).min(lines.len());
                    let context: Vec<String> =
                        lines[start..end].iter().map(|s| s.to_string()).collect();

                    let suggestions = self.get_suggestions(*error_type, &message);
                    let related_files = self.extract_files(&context.join("\n"));

                    return Some(DetectedError {
                        error_type: *error_type,
                        message,
                        context,
                        line_number: Some(line_idx),
                        severity: *severity,
                        suggestions,
                        related_files,
                    });
                }
            }
        }

        None
    }

    pub fn detect_all(&self, output: &str) -> Vec<DetectedError> {
        let mut errors = Vec::new();
        let lines: Vec<&str> = output.lines().collect();
        let mut seen_positions = std::collections::HashSet::new();

        for (line_idx, line) in lines.iter().enumerate() {
            for (regex, error_type, severity) in ERROR_PATTERNS.iter() {
                if let Some(captures) = regex.captures(line) {
                    if seen_positions.contains(&line_idx) {
                        continue;
                    }
                    seen_positions.insert(line_idx);

                    let message = captures
                        .get(1)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_else(|| line.to_string());

                    let start = line_idx.saturating_sub(self.context_lines);
                    let end = (line_idx + self.context_lines + 1).min(lines.len());
                    let context: Vec<String> =
                        lines[start..end].iter().map(|s| s.to_string()).collect();

                    let suggestions = self.get_suggestions(*error_type, &message);
                    let related_files = self.extract_files(&context.join("\n"));

                    errors.push(DetectedError {
                        error_type: *error_type,
                        message,
                        context,
                        line_number: Some(line_idx),
                        severity: *severity,
                        suggestions,
                        related_files,
                    });
                }
            }
        }

        errors
    }

    fn get_suggestions(&self, error_type: ErrorType, _message: &str) -> Vec<String> {
        match error_type {
            ErrorType::CommandNotFound => vec![
                "Verifique se o comando está instalado".to_string(),
                "Verifique se o comando está no PATH".to_string(),
                "Tente reinstalar o programa".to_string(),
            ],
            ErrorType::PermissionDenied => vec![
                "Execute com sudo se necessário".to_string(),
                "Verifique as permissões do arquivo".to_string(),
                "Verifique se você é o proprietário do arquivo".to_string(),
            ],
            ErrorType::FileNotFoundError => vec![
                "Verifique se o caminho está correto".to_string(),
                "Verifique se o arquivo existe".to_string(),
                "Verifique o diretório de trabalho atual".to_string(),
            ],
            ErrorType::SyntaxError => vec![
                "Verifique a sintaxe do comando".to_string(),
                "Verifique se há caracteres especiais não escapados".to_string(),
                "Consulte a documentação do comando".to_string(),
            ],
            ErrorType::NetworkError => vec![
                "Verifique sua conexão com a internet".to_string(),
                "Verifique se o servidor está acessível".to_string(),
                "Verifique configurações de proxy/firewall".to_string(),
            ],
            ErrorType::RustError => vec![
                "Verifique se as dependências estão instaladas".to_string(),
                "Execute 'cargo clean' e tente novamente".to_string(),
                "Verifique a versão do Rust".to_string(),
            ],
            ErrorType::PythonError => vec![
                "Verifique se o módulo está instalado".to_string(),
                "Execute 'pip install -r requirements.txt'".to_string(),
                "Verifique o ambiente virtual".to_string(),
            ],
            ErrorType::NpmError => vec![
                "Execute 'npm install' para instalar dependências".to_string(),
                "Delete node_modules e package-lock.json e tente novamente".to_string(),
                "Verifique a versão do Node.js".to_string(),
            ],
            ErrorType::GitError => vec![
                "Verifique o status do repositório".to_string(),
                "Verifique se há conflitos de merge".to_string(),
                "Consulte 'git help' para mais informações".to_string(),
            ],
            ErrorType::DockerError => vec![
                "Verifique se o Docker está rodando".to_string(),
                "Verifique se a imagem/contêiner existe".to_string(),
                "Execute 'docker ps -a' para ver contêineres".to_string(),
            ],
            _ => vec!["Consulte a documentação para mais informações".to_string()],
        }
    }

    fn extract_files(&self, text: &str) -> Vec<String> {
        let mut files = Vec::new();

        for cap in FILE_PATTERN.captures_iter(text) {
            if let Some(file) = cap.get(1) {
                let file_str = file.as_str();
                if !file_str.starts_with("http")
                    && !file_str.starts_with("git")
                    && file_str.len() < 256
                {
                    files.push(file_str.to_string());
                }
            }
        }

        files.sort();
        files.dedup();
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_command_not_found() {
        let detector = ErrorDetector::new();
        let output = "bash: foo: command not found";

        let error = detector.detect(output);
        assert!(error.is_some());

        let error = error.unwrap();
        assert_eq!(error.error_type, ErrorType::CommandNotFound);
        assert_eq!(error.severity, ErrorSeverity::High);
    }

    #[test]
    fn test_detect_permission_denied() {
        let detector = ErrorDetector::new();
        let output = "bash: ./script.sh: Permission denied";

        let error = detector.detect(output);
        assert!(error.is_some());

        let error = error.unwrap();
        assert_eq!(error.error_type, ErrorType::PermissionDenied);
    }

    #[test]
    fn test_detect_python_error() {
        let detector = ErrorDetector::new();
        let output = r#"
Traceback (most recent call last):
  File "test.py", line 1, in <module>
    import missing_module
ModuleNotFoundError: No module named 'missing_module'
"#;

        let error = detector.detect(output);
        assert!(error.is_some());

        let error = error.unwrap();
        assert_eq!(error.error_type, ErrorType::PythonError);
        assert!(!error.suggestions.is_empty());
    }

    #[test]
    fn test_detect_rust_error() {
        let detector = ErrorDetector::new();
        let output = "error[E0433]: failed to resolve: use of undeclared crate or module `missing`";

        let error = detector.detect(output);
        assert!(error.is_some());

        let error = error.unwrap();
        assert_eq!(error.error_type, ErrorType::RustError);
    }

    #[test]
    fn test_detect_no_error() {
        let detector = ErrorDetector::new();
        let output = "Hello, World!";

        let error = detector.detect(output);
        assert!(error.is_none());
    }

    #[test]
    fn test_detect_all() {
        let detector = ErrorDetector::new();
        let output = r#"
bash: foo: command not found
permission denied: /root/file
npm ERR! missing script
"#;

        let errors = detector.detect_all(output);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_extract_files() {
        let detector = ErrorDetector::new();
        let text = "Error in src/main.rs at line 10 and lib/test.py";

        let files = detector.extract_files(text);
        assert!(files.contains(&"src/main.rs".to_string()));
        assert!(files.contains(&"lib/test.py".to_string()));
    }

    #[test]
    fn test_error_severity_display() {
        assert_eq!(format!("{}", ErrorSeverity::Low), "Low");
        assert_eq!(format!("{}", ErrorSeverity::Critical), "Critical");
    }

    #[test]
    fn test_error_type_display() {
        assert_eq!(
            format!("{}", ErrorType::CommandNotFound),
            "Command Not Found"
        );
        assert_eq!(format!("{}", ErrorType::RustError), "Rust Error");
    }
}
