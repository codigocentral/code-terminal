//! Snapshot do workspace: tabs, árvore de painéis (com proporções) e o
//! diretório de cada painel. Salvo ao fechar a janela e restaurado ao abrir,
//! para voltar ao mesmo lugar (como o cmux).

use crate::ui::pane_layout::LayoutTree;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Versão do formato do arquivo.
pub const WORKSPACE_VERSION: u32 = 1;

/// Um painel salvo.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub cwd: Option<PathBuf>,
    /// Era o painel em foco da tab.
    #[serde(default)]
    pub focused: bool,
    /// Sessão do multiplexer em que o painel estava anexado (tab
    /// persistente): ao reabrir, o painel anexa de novo à mesma sessão.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mux_session: Option<String>,
}

/// Uma tab salva.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub layout: LayoutTree<PaneSnapshot>,
    /// Título dado pelo usuário (fixo); sem ele, o automático.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Índice (nesta lista) da tab de cima do grupo, se estiver aninhada.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<usize>,
    /// Grupo recolhido na sidebar.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    /// Cor da tab na sidebar ("#rrggbb").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Fundo do terminal da tab ("#rrggbb").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// O workspace inteiro.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub version: u32,
    pub active_tab: usize,
    pub sidebar_visible: bool,
    pub tabs: Vec<TabSnapshot>,
}

/// Arquivo padrão: `~/.local/share/Code Terminal/workspace.json`.
pub fn default_workspace_path() -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join("Code Terminal")
            .join("workspace.json"),
    )
}

/// Grava o snapshot (escrita atômica: arquivo temporário + rename).
pub fn save_workspace(path: &Path, snapshot: &WorkspaceSnapshot) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(snapshot).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)
}

/// Lê o snapshot; arquivo ausente, corrompido ou de outra versão vira `None`.
pub fn load_workspace(path: &Path) -> Option<WorkspaceSnapshot> {
    let data = std::fs::read(path).ok()?;
    let snapshot: WorkspaceSnapshot = serde_json::from_slice(&data).ok()?;
    (snapshot.version == WORKSPACE_VERSION && !snapshot.tabs.is_empty()).then_some(snapshot)
}

/// Apaga o snapshot (a janela fechou sem tabs: na próxima abre do zero).
pub fn clear_workspace(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            version: WORKSPACE_VERSION,
            active_tab: 0,
            sidebar_visible: true,
            tabs: vec![
                TabSnapshot {
                    layout: LayoutTree::Pane {
                        pane: PaneSnapshot {
                            cwd: Some("/tmp".into()),
                            focused: true,
                            mux_session: Some("dev".into()),
                        },
                    },
                    title: Some("api".into()),
                    parent: None,
                    collapsed: true,
                    color: Some("#ff8000".into()),
                    background: Some("#101820".into()),
                },
                TabSnapshot {
                    layout: LayoutTree::Pane {
                        pane: PaneSnapshot {
                            cwd: None,
                            focused: true,
                            mux_session: None,
                        },
                    },
                    title: None,
                    parent: Some(0),
                    collapsed: false,
                    color: None,
                    background: None,
                },
            ],
        }
    }

    #[test]
    fn test_save_load_clear() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/workspace.json");
        save_workspace(&path, &snapshot()).unwrap();
        assert_eq!(load_workspace(&path), Some(snapshot()));
        clear_workspace(&path);
        assert_eq!(load_workspace(&path), None);
    }

    /// Arquivo salvo antes dos títulos e grupos: abre com tabs soltas e
    /// título automático.
    #[test]
    fn test_old_files_without_title_and_groups_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.json");
        let old = r#"{"version":1,"active_tab":0,"sidebar_visible":true,
            "tabs":[{"layout":{"kind":"pane","pane":{"cwd":"/tmp","focused":true}}}]}"#;
        std::fs::write(&path, old).unwrap();
        let snapshot = load_workspace(&path).unwrap();
        let tab = &snapshot.tabs[0];
        assert_eq!(
            (tab.title.as_deref(), tab.parent, tab.collapsed),
            (None, None, false)
        );
        // E o que não tem valor não vai para o arquivo.
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(!json.contains("title") && !json.contains("parent") && !json.contains("collapsed"));
        assert!(!json.contains("color") && !json.contains("background"));
    }

    #[test]
    fn test_invalid_files_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("workspace.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_workspace(&path), None);
        let mut old = snapshot();
        old.version = 99;
        save_workspace(&path, &old).unwrap();
        assert_eq!(load_workspace(&path), None);
        let mut empty = snapshot();
        empty.tabs.clear();
        save_workspace(&path, &empty).unwrap();
        assert_eq!(load_workspace(&path), None);
    }
}
