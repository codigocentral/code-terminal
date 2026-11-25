//! Onboarding discreto exibido na primeira execução.
//!
//! Mostra um overlay curto com os atalhos essenciais e a opção de não
//! mostrar novamente. O flag "não mostrar" é persistido no diretório de
//! dados da aplicação.

use crate::config::KeyBindings;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Estado do onboarding.
#[derive(Debug)]
pub struct Onboarding {
    visible: bool,
    dismissed: bool,
}

impl Onboarding {
    pub fn new() -> Self {
        let dismissed = Self::load_dismissed();
        Self {
            visible: !dismissed,
            dismissed,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Muda a visibilidade sem gravar nada em disco.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn dismiss(&mut self) {
        self.visible = false;
        self.dismissed = true;
        if let Err(e) = Self::save_dismissed() {
            warn!(error = %e, "Failed to save onboarding dismissed flag");
        }
        debug!("Onboarding dismissed");
    }

    /// Mostra os atalhos configurados e os atalhos nativos de copiar e colar no macOS.
    pub fn shortcuts(keybindings: &KeyBindings) -> Vec<(String, &'static str)> {
        use crate::ui::i18n::{shortcut_name, tr, Text};
        vec![
            (
                KeyBindings::display(&keybindings.new_tab),
                shortcut_name("new_tab"),
            ),
            (
                KeyBindings::display(&keybindings.close_tab),
                shortcut_name("close_tab"),
            ),
            (
                KeyBindings::display(&keybindings.next_tab),
                shortcut_name("next_tab"),
            ),
            (
                if cfg!(target_os = "macos") {
                    "⌘C".to_string()
                } else {
                    KeyBindings::display(&keybindings.copy)
                },
                shortcut_name("copy"),
            ),
            (
                if cfg!(target_os = "macos") {
                    "⌘V".to_string()
                } else {
                    KeyBindings::display(&keybindings.paste)
                },
                shortcut_name("paste"),
            ),
            (
                KeyBindings::display(&keybindings.search_open),
                shortcut_name("search_open"),
            ),
            ("Ctrl+Shift+P".to_string(), tr(Text::CommandPalette)),
            ("Ctrl+Shift+N".to_string(), tr(Text::NewWindowLabel)),
        ]
    }

    /// Linhas de texto exibidas no overlay.
    pub fn lines(keybindings: &KeyBindings) -> Vec<String> {
        use crate::ui::i18n::{tr, Text};
        let mut lines = vec![tr(Text::Welcome).to_string(), String::new()];
        lines.extend(
            Self::shortcuts(keybindings)
                .into_iter()
                .map(|(keys, label)| format!("{keys:<14}{label}")),
        );
        lines.push(String::new());
        lines.push(tr(Text::PressAnyKey).to_string());
        lines
    }

    fn dismissed_flag_path() -> anyhow::Result<PathBuf> {
        let data_dir = dirs::data_local_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine local data directory"))?;
        let app_dir = data_dir.join("Code Terminal");
        std::fs::create_dir_all(&app_dir)?;
        Ok(app_dir.join("onboarding-dismissed"))
    }

    fn load_dismissed() -> bool {
        match Self::dismissed_flag_path() {
            Ok(path) => path.exists(),
            Err(e) => {
                warn!(error = %e, "Failed to determine onboarding flag path");
                false
            }
        }
    }

    fn save_dismissed() -> anyhow::Result<()> {
        let path = Self::dismissed_flag_path()?;
        std::fs::write(&path, b"1")?;
        Ok(())
    }
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onboarding_lines_not_empty() {
        assert!(!Onboarding::lines(&KeyBindings::default()).is_empty());
    }

    /// Regressão: o overlay dizia Ctrl+T, mas o atalho real era Ctrl+Shift+T.
    #[test]
    fn test_onboarding_shows_configured_shortcuts() {
        let mut keybindings = KeyBindings::default();
        let lines = Onboarding::lines(&keybindings).join("\n");
        assert!(lines.contains("Ctrl+Shift+T"), "{lines}");
        assert!(!lines.contains("Ctrl+T "), "{lines}");

        keybindings.new_tab = "alt+t".to_string();
        let shortcuts = Onboarding::shortcuts(&keybindings);
        assert_eq!(shortcuts[0], ("Alt+T".to_string(), "New tab"));
    }
}
