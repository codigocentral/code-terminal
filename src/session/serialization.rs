//! Estruturas de dados para serialização de sessões
//!
//! Este módulo define as estruturas que representam o estado
//! serializável do terminal.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Dados completos de uma sessão
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionData {
    /// Versão do formato
    pub version: u32,
    /// Dados do layout (splits/panes)
    pub layout: LayoutData,
    /// Variáveis de ambiente
    #[serde(default)]
    pub environment: HashMap<String, String>,
    /// Diretório de trabalho inicial
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    /// Configurações ativas
    #[serde(default)]
    pub config: SessionConfigData,
}

impl SessionData {
    /// Cria novos dados de sessão vazios
    pub fn new() -> Self {
        Self {
            version: super::SESSION_FORMAT_VERSION,
            layout: LayoutData::default(),
            environment: HashMap::new(),
            working_directory: None,
            config: SessionConfigData::default(),
        }
    }

    /// Define o layout
    pub fn with_layout(mut self, layout: LayoutData) -> Self {
        self.layout = layout;
        self
    }

    /// Define o diretório de trabalho
    pub fn with_working_directory(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(dir.into());
        self
    }
}

/// Dados de configuração da sessão
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionConfigData {
    /// Shell padrão
    #[serde(default)]
    pub shell: Option<String>,
    /// Tema ativo
    #[serde(default)]
    pub theme: Option<String>,
    /// Tamanho do terminal (cols, rows)
    #[serde(default)]
    pub terminal_size: Option<(u16, u16)>,
}

/// Dados do layout de panes e splits
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutData {
    /// ID do pane ativo
    #[serde(default)]
    pub active_pane: Option<u32>,
    /// Árvore de splits
    #[serde(default)]
    pub root_split: Option<SplitData>,
    /// Panes individuais (para layouts sem splits)
    #[serde(default)]
    pub panes: Vec<PaneData>,
}

impl LayoutData {
    /// Cria um layout vazio
    pub fn new() -> Self {
        Self::default()
    }

    /// Adiciona um pane
    pub fn add_pane(&mut self, pane: PaneData) {
        self.panes.push(pane);
    }

    /// Define o pane ativo
    pub fn set_active_pane(&mut self, pane_id: u32) {
        self.active_pane = Some(pane_id);
    }

    /// Define a raiz do split
    pub fn set_root_split(&mut self, split: SplitData) {
        self.root_split = Some(split);
    }
}

/// Dados de um split (nó da árvore)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SplitData {
    /// Split com dois filhos
    #[serde(rename = "split")]
    Split {
        /// Direção do split
        direction: SplitDirection,
        /// Proporção do primeiro filho (0.0 - 1.0)
        ratio: f32,
        /// Primeiro filho
        first: Box<SplitData>,
        /// Segundo filho
        second: Box<SplitData>,
    },
    /// Pane folha
    #[serde(rename = "pane")]
    Pane(PaneData),
}

/// Direção de um split
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    /// Split horizontal (panes empilhados)
    Horizontal,
    /// Split vertical (panes lado a lado)
    Vertical,
}

/// Dados de um pane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneData {
    /// ID único do pane
    pub id: u32,
    /// Posição e tamanho (x, y, width, height)
    pub rect: RectData,
    /// Tabs no pane
    pub tabs: Vec<TabData>,
    /// ID da tab ativa
    pub active_tab: Option<u32>,
}

impl PaneData {
    /// Cria um novo pane
    pub fn new(id: u32) -> Self {
        Self {
            id,
            rect: RectData::default(),
            tabs: Vec::new(),
            active_tab: None,
        }
    }

    /// Define o retângulo
    pub fn with_rect(mut self, x: u32, y: u32, width: u32, height: u32) -> Self {
        self.rect = RectData {
            x,
            y,
            width,
            height,
        };
        self
    }

    /// Adiciona uma tab
    pub fn add_tab(&mut self, tab: TabData) {
        if self.active_tab.is_none() {
            self.active_tab = Some(tab.id);
        }
        self.tabs.push(tab);
    }

    /// Define a tab ativa
    pub fn set_active_tab(&mut self, tab_id: u32) {
        self.active_tab = Some(tab_id);
    }
}

/// Dados de retângulo (posição e tamanho)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RectData {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Dados de uma tab
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabData {
    /// ID único da tab
    pub id: u32,
    /// Título da tab
    pub title: String,
    /// Diretório de trabalho
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    /// Comando/shell em execução
    #[serde(default)]
    pub command: Option<String>,
    /// Posição do scroll
    #[serde(default)]
    pub scroll_position: usize,
    /// Estado da tab
    #[serde(default)]
    pub state: TabStateData,
}

impl TabData {
    /// Cria uma nova tab
    pub fn new(id: u32, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            working_directory: None,
            command: None,
            scroll_position: 0,
            state: TabStateData::default(),
        }
    }

    /// Define o diretório de trabalho
    pub fn with_working_directory(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(dir.into());
        self
    }

    /// Define o comando
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }
}

/// Estado de uma tab
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TabStateData {
    /// Tab ativa
    #[default]
    Active,
    /// Tab inativa
    Inactive,
    /// Tab em processo de fechamento
    Closing,
}

/// Histórico de comandos (opcional)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommandHistory {
    /// Comandos executados
    pub commands: Vec<CommandEntry>,
    /// Limite máximo
    pub limit: usize,
}

/// Entrada no histórico de comandos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandEntry {
    /// Comando
    pub command: String,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Diretório onde foi executado
    pub working_directory: PathBuf,
    /// Código de saída (se disponível)
    pub exit_code: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_data_creation() {
        let data = SessionData::new();
        assert_eq!(data.version, super::super::SESSION_FORMAT_VERSION);
        assert!(data.layout.panes.is_empty());
    }

    #[test]
    fn test_pane_data() {
        let mut pane = PaneData::new(1);
        pane.add_tab(TabData::new(1, "bash"));

        assert_eq!(pane.id, 1);
        assert_eq!(pane.tabs.len(), 1);
        assert_eq!(pane.active_tab, Some(1));
    }

    #[test]
    fn test_tab_data() {
        let tab = TabData::new(1, "bash")
            .with_working_directory("/home/user")
            .with_command("vim");

        assert_eq!(tab.id, 1);
        assert_eq!(tab.title, "bash");
        assert_eq!(tab.working_directory, Some(PathBuf::from("/home/user")));
        assert_eq!(tab.command, Some("vim".to_string()));
    }

    #[test]
    fn test_serialization() {
        let mut layout = LayoutData::new();
        let mut pane = PaneData::new(1);
        pane.add_tab(TabData::new(1, "bash"));
        layout.add_pane(pane);

        let data = SessionData::new().with_layout(layout);

        // Serializa para JSON
        let json = serde_json::to_string(&data).unwrap();
        assert!(!json.is_empty());

        // Desserializa
        let restored: SessionData = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.version, data.version);
        assert_eq!(restored.layout.panes.len(), 1);
    }

    #[test]
    fn test_split_data() {
        let split = SplitData::Split {
            direction: SplitDirection::Vertical,
            ratio: 0.5,
            first: Box::new(SplitData::Pane(PaneData::new(1))),
            second: Box::new(SplitData::Pane(PaneData::new(2))),
        };

        let json = serde_json::to_string(&split).unwrap();
        let restored: SplitData = serde_json::from_str(&json).unwrap();

        match restored {
            SplitData::Split {
                direction, ratio, ..
            } => {
                assert_eq!(direction, SplitDirection::Vertical);
                assert_eq!(ratio, 0.5);
            }
            _ => panic!("Expected Split variant"),
        }
    }
}
