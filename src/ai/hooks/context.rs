//! Contexto para hooks de agentes

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Contexto fornecido aos hooks de agente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentContext {
    /// ID da sessão
    pub session_id: String,
    /// ID da aba atual
    pub current_tab_id: u64,
    /// Informações do workspace
    pub workspace: WorkspaceInfo,
    /// Snapshot atual do terminal
    pub terminal: TerminalSnapshot,
    /// Variáveis de ambiente relevantes
    pub env_vars: HashMap<String, String>,
    /// Configurações do agente
    pub agent_config: HashMap<String, serde_json::Value>,
    /// Histórico de comandos recentes
    pub command_history: Vec<CommandEntry>,
    /// Seleção atual (se houver)
    pub current_selection: Option<String>,
    /// Arquivos abertos/relevantes
    pub relevant_files: Vec<FileInfo>,
    /// Metadados adicionais
    pub metadata: HashMap<String, serde_json::Value>,
}

impl AgentContext {
    /// Cria um novo contexto
    pub fn new(session_id: impl Into<String>, tab_id: u64) -> Self {
        Self {
            session_id: session_id.into(),
            current_tab_id: tab_id,
            workspace: WorkspaceInfo::default(),
            terminal: TerminalSnapshot::default(),
            env_vars: HashMap::new(),
            agent_config: HashMap::new(),
            command_history: Vec::new(),
            current_selection: None,
            relevant_files: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Define informações do workspace
    pub fn with_workspace(mut self, workspace: WorkspaceInfo) -> Self {
        self.workspace = workspace;
        self
    }

    /// Define snapshot do terminal
    pub fn with_terminal(mut self, terminal: TerminalSnapshot) -> Self {
        self.terminal = terminal;
        self
    }

    /// Adiciona variável de ambiente
    pub fn with_env_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env_vars.insert(key.into(), value.into());
        self
    }

    /// Adiciona configuração
    pub fn with_config(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.agent_config.insert(key.into(), value.into());
        self
    }

    /// Define seleção atual
    pub fn with_selection(mut self, selection: impl Into<String>) -> Self {
        self.current_selection = Some(selection.into());
        self
    }

    /// Adiciona arquivo relevante
    pub fn with_file(mut self, file: FileInfo) -> Self {
        self.relevant_files.push(file);
        self
    }

    /// Adiciona entrada ao histórico
    pub fn add_command_history(&mut self, entry: CommandEntry) {
        self.command_history.push(entry);
        // Limitar histórico
        if self.command_history.len() > 100 {
            self.command_history.remove(0);
        }
    }

    /// Obtém o diretório de trabalho atual
    pub fn current_directory(&self) -> &str {
        &self.workspace.current_directory
    }

    /// Verifica se está em um repositório git
    pub fn is_git_repo(&self) -> bool {
        self.workspace.git_info.is_some()
    }
}

/// Informações do workspace
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    /// Diretório atual
    pub current_directory: String,
    /// Diretório raiz do projeto (se detectado)
    pub project_root: Option<String>,
    /// Nome do projeto
    pub project_name: Option<String>,
    /// Tipo do projeto (rust, node, python, etc.)
    pub project_type: Option<String>,
    /// Informações do git
    pub git_info: Option<GitInfo>,
    /// Arquivos no diretório atual
    pub files_in_directory: Vec<String>,
    /// Linguagens detectadas
    pub detected_languages: Vec<String>,
}

impl WorkspaceInfo {
    /// Cria com diretório atual
    pub fn with_directory(dir: impl Into<String>) -> Self {
        Self {
            current_directory: dir.into(),
            ..Default::default()
        }
    }

    /// Define informações do git
    pub fn with_git(mut self, git: GitInfo) -> Self {
        self.git_info = Some(git);
        self
    }

    /// Detecta o tipo de projeto baseado em arquivos
    pub fn detect_project_type(&mut self) {
        self.project_type = None;

        for file in &self.files_in_directory {
            match file.as_str() {
                "Cargo.toml" => {
                    self.project_type = Some("rust".to_string());
                    return;
                }
                "package.json" => {
                    self.project_type = Some("node".to_string());
                    return;
                }
                "pyproject.toml" | "setup.py" | "requirements.txt" => {
                    self.project_type = Some("python".to_string());
                    return;
                }
                "go.mod" => {
                    self.project_type = Some("go".to_string());
                    return;
                }
                _ => {}
            }
        }
    }
}

/// Informações do git
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    /// Branch atual
    pub branch: String,
    /// Hash do commit atual
    pub commit_hash: String,
    /// Arquivos modificados
    pub modified_files: Vec<String>,
    /// Arquivos staged
    pub staged_files: Vec<String>,
    /// Arquivos untracked
    pub untracked_files: Vec<String>,
    /// Commits ahead do remote
    pub ahead: u32,
    /// Commits behind do remote
    pub behind: u32,
}

impl GitInfo {
    /// Cria info básica
    pub fn new(branch: impl Into<String>) -> Self {
        Self {
            branch: branch.into(),
            commit_hash: String::new(),
            modified_files: Vec::new(),
            staged_files: Vec::new(),
            untracked_files: Vec::new(),
            ahead: 0,
            behind: 0,
        }
    }
}

/// Snapshot do terminal
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TerminalSnapshot {
    /// Conteúdo visível
    pub visible_content: Vec<String>,
    /// Número de linhas
    pub rows: u32,
    /// Número de colunas
    pub cols: u32,
    /// Posição do cursor
    pub cursor: CursorPosition,
    /// Título da aba
    pub title: String,
    /// Scrollback disponível
    pub scrollback_lines: usize,
    /// Estado do terminal
    pub mode: TerminalMode,
}

/// Posição do cursor
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CursorPosition {
    pub row: u32,
    pub col: u32,
}

/// Modo do terminal
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalMode {
    #[default]
    Normal,
    Alternate,
    Raw,
}

/// Entrada de comando no histórico
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEntry {
    /// Comando executado
    pub command: String,
    /// Timestamp
    pub timestamp: std::time::SystemTime,
    /// Código de saída
    pub exit_code: Option<i32>,
    /// Diretório onde foi executado
    pub directory: String,
    /// Duração
    pub duration_ms: Option<u64>,
}

impl CommandEntry {
    /// Cria uma nova entrada
    pub fn new(command: impl Into<String>, directory: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            timestamp: std::time::SystemTime::now(),
            exit_code: None,
            directory: directory.into(),
            duration_ms: None,
        }
    }

    /// Define código de saída
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// Define duração
    pub fn with_duration(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    /// Verifica se foi sucesso
    pub fn is_success(&self) -> bool {
        self.exit_code.map(|c| c == 0).unwrap_or(false)
    }
}

/// Informações de arquivo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileInfo {
    /// Caminho do arquivo
    pub path: PathBuf,
    /// Tipo do arquivo
    pub file_type: FileType,
    /// Tamanho em bytes
    pub size: Option<u64>,
    /// Última modificação
    pub modified: Option<std::time::SystemTime>,
    /// Conteúdo (se relevante/carregado)
    pub content: Option<String>,
    /// Hash do conteúdo
    pub content_hash: Option<String>,
}

impl FileInfo {
    /// Cria info básica
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            file_type: FileType::Unknown,
            size: None,
            modified: None,
            content: None,
            content_hash: None,
        }
    }

    /// Define o tipo
    pub fn with_type(mut self, file_type: FileType) -> Self {
        self.file_type = file_type;
        self
    }
}

/// Tipo de arquivo
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    SourceCode,
    Config,
    Documentation,
    Test,
    Build,
    Binary,
    Unknown,
}

/// Capabilities do contexto
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCapabilities {
    /// Pode ler arquivos
    pub can_read_files: bool,
    /// Pode escrever arquivos
    pub can_write_files: bool,
    /// Pode executar comandos
    pub can_execute_commands: bool,
    /// Pode acessar rede
    pub can_access_network: bool,
    /// Pode acessar clipboard
    pub can_access_clipboard: bool,
    /// Nível de acesso ao terminal
    pub terminal_access_level: TerminalAccessLevel,
}

impl Default for ContextCapabilities {
    fn default() -> Self {
        Self {
            can_read_files: true,
            can_write_files: false,
            can_execute_commands: false,
            can_access_network: false,
            can_access_clipboard: true,
            terminal_access_level: TerminalAccessLevel::ReadOnly,
        }
    }
}

/// Nível de acesso ao terminal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalAccessLevel {
    /// Sem acesso
    None,
    /// Apenas leitura
    ReadOnly,
    /// Leitura e notificações
    ReadAndNotify,
    /// Acesso completo
    Full,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_context() {
        let ctx = AgentContext::new("session-1", 1)
            .with_workspace(WorkspaceInfo::with_directory("/home/user/project"))
            .with_env_var("EDITOR", "vim")
            .with_config("auto_complete", true);

        assert_eq!(ctx.session_id, "session-1");
        assert_eq!(ctx.current_directory(), "/home/user/project");
        assert!(ctx.env_vars.contains_key("EDITOR"));
    }

    #[test]
    fn test_workspace_project_detection() {
        let mut workspace = WorkspaceInfo::with_directory("/project");
        workspace.files_in_directory = vec![
            "Cargo.toml".to_string(),
            "src".to_string(),
            "README.md".to_string(),
        ];
        workspace.detect_project_type();

        assert_eq!(workspace.project_type, Some("rust".to_string()));
    }

    #[test]
    fn test_command_entry() {
        let entry = CommandEntry::new("ls -la", "/home")
            .with_exit_code(0)
            .with_duration(100);

        assert!(entry.is_success());
        assert_eq!(entry.duration_ms, Some(100));
    }

    #[test]
    fn test_terminal_snapshot() {
        let snapshot = TerminalSnapshot {
            visible_content: vec!["line1".to_string(), "line2".to_string()],
            rows: 24,
            cols: 80,
            cursor: CursorPosition { row: 0, col: 5 },
            title: "bash".to_string(),
            scrollback_lines: 1000,
            mode: TerminalMode::Normal,
        };

        assert_eq!(snapshot.rows, 24);
        assert_eq!(snapshot.cursor.col, 5);
    }
}
