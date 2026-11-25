//! Tela de configurações no `AppState`: abrir (montando o estado), fechar
//! (descartando), teclas, cliques e o que a tela pede (prévia de tema,
//! salvar, conectar SSH).

use super::AppState;
use crate::ui::settings::{SettingsInputKey, SettingsOutcome, SettingsScreen, SshEntry};

/// Clique na tela de configurações fora da lista de opções.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsClick {
    /// Seção `n` da coluna da esquerda.
    Section(usize),
    Save,
    Cancel,
}

/// Memória do próprio processo, para a seção Sobre (Linux: privada e PSS).
fn process_memory_text() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string("/proc/self/smaps_rollup").ok()?;
        let field = |name: &str| {
            text.lines().find_map(|line| {
                let rest = line.strip_prefix(name)?;
                rest.split_whitespace().next()?.parse::<u64>().ok()
            })
        };
        let private = field("Private_Clean:")? + field("Private_Dirty:")?;
        let pss = field("Pss:")?;
        Some(format!(
            "{} private \u{b7} {} PSS",
            crate::ui::sidebar_info::format_memory(private),
            crate::ui::sidebar_info::format_memory(pss)
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

impl AppState {
    /// Se a tela de configurações está aberta.
    pub fn is_settings_visible(&self) -> bool {
        self.settings_screen.is_some()
    }

    /// Abre a tela de configurações, lendo os hosts SSH e a memória agora.
    pub fn show_settings(&mut self) {
        if self.settings_screen.is_some() {
            return;
        }
        let hosts = self.ssh_entries();
        self.settings_screen = Some(Box::new(SettingsScreen::new(
            &self.config,
            hosts,
            process_memory_text(),
        )));
    }

    /// Hosts da seção SSH: os que o `ssh` conhece (para conectar) e, para
    /// os blocos simples do `~/.ssh/config`, os campos para editar.
    fn ssh_entries(&self) -> Vec<SshEntry> {
        use crate::ssh::config_edit::{ssh_config_path, SshConfigDocument};
        let document = ssh_config_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| SshConfigDocument::parse(&text));
        let target = |user: Option<&str>, host: &str, port: Option<u16>| {
            let user = user.map(|u| format!("{u}@")).unwrap_or_default();
            let port = port
                .filter(|&p| p != 22)
                .map(|p| format!(":{p}"))
                .unwrap_or_default();
            format!("{user}{host}{port}")
        };
        let mut entries: Vec<SshEntry> = self
            .ssh_manager
            .read()
            .get_all_hosts()
            .into_iter()
            .filter(|host| !host.pattern.contains(['*', '?']))
            .map(|host| {
                let editable = document.as_ref().and_then(|doc| doc.host(&host.pattern));
                // Bloco editável: mostra o que está escrito nele (o mesmo
                // que o formulário abre).
                let target = match &editable {
                    Some(entry) => target(
                        entry.user.as_deref(),
                        entry.host_name.as_deref().unwrap_or(&entry.alias),
                        entry.port,
                    ),
                    None => target(host.user.as_deref(), host.effective_host(), host.port),
                };
                SshEntry {
                    target,
                    editable,
                    name: host.pattern,
                }
            })
            .collect();
        // Blocos do arquivo que o `ssh_manager` ainda não leu (recém-gravados).
        for entry in document.iter().flat_map(|doc| doc.editable_hosts()) {
            if !entries.iter().any(|e| e.name == entry.alias) {
                entries.push(SshEntry {
                    name: entry.alias.clone(),
                    target: target(
                        entry.user.as_deref(),
                        entry.host_name.as_deref().unwrap_or(&entry.alias),
                        entry.port,
                    ),
                    editable: Some(entry),
                });
            }
        }
        entries
    }

    /// Grava o host no `~/.ssh/config` (backup + escrita atômica) e atualiza
    /// a tela. Erro vira mensagem no formulário.
    fn save_ssh_host(
        &mut self,
        original: Option<String>,
        entry: crate::ssh::config_edit::HostEntry,
    ) {
        use crate::ssh::config_edit::{
            save_with_backup, ssh_config_path, EditError, SshConfigDocument,
        };
        use crate::ui::i18n::{tr, Text};
        let result = (|| -> Result<Option<std::path::PathBuf>, String> {
            let path = ssh_config_path().ok_or_else(|| "~/.ssh/config".to_string())?;
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(e.to_string()),
            };
            let mut document = SshConfigDocument::parse(&text);
            document.upsert(original.as_deref(), &entry).map_err(|e| {
                tr(match e {
                    EditError::InvalidAlias => Text::SshInvalidAlias,
                    EditError::AliasExists => Text::SshAliasExists,
                    EditError::NotFound => Text::SshHostGone,
                    EditError::InvalidValue => Text::SshInvalidValue,
                })
                .to_string()
            })?;
            save_with_backup(&path, &document.render()).map_err(|e| e.to_string())
        })();
        match result {
            Ok(backup) => {
                if let Err(e) = self.ssh_manager.read().load_config() {
                    tracing::warn!(error = %e, "Failed to reload SSH config");
                }
                let hosts = self.ssh_entries();
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.ssh_saved(hosts);
                }
                let name = backup
                    .as_ref()
                    .and_then(|b| b.file_name())
                    .map(|n| n.to_string_lossy().into_owned());
                self.show_toast(
                    crate::ui::i18n::ssh_saved(name.as_deref()),
                    crate::ui::toast::ToastKind::Info,
                );
            }
            Err(message) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.set_ssh_error(message);
                }
            }
        }
    }

    /// Fecha a tela sem salvar (o tema da prévia volta ao de antes).
    pub fn hide_settings(&mut self) {
        self.settings_screen = None;
        self.finish_theme_preview(false);
        if let Some(language) = self.language_before_preview.take() {
            crate::ui::i18n::set_language(language);
        }
    }

    /// Tela de configurações aberta.
    pub fn settings_screen(&self) -> Option<&SettingsScreen> {
        self.settings_screen.as_deref()
    }

    /// Tecla para a tela. Devolve `true` se o config mudou.
    pub fn handle_settings_key(&mut self, key: SettingsInputKey) -> bool {
        let Some(screen) = self.settings_screen.as_mut() else {
            return false;
        };
        let outcome = screen.key(key);
        self.apply_settings_outcome(outcome)
    }

    /// Clique na linha `index` da seção aberta. Devolve `true` se o config mudou.
    pub fn activate_settings_row(&mut self, index: usize) -> bool {
        let Some(screen) = self.settings_screen.as_mut() else {
            return false;
        };
        let outcome = screen.activate(index);
        self.apply_settings_outcome(outcome)
    }

    /// Clique numa seção ou num botão. Devolve `true` se o config mudou.
    pub fn settings_click(&mut self, click: SettingsClick) -> bool {
        match click {
            SettingsClick::Section(index) => {
                if let Some(screen) = self.settings_screen.as_mut() {
                    screen.select_section(index);
                }
                false
            }
            SettingsClick::Save => self.apply_settings_outcome(SettingsOutcome::Save),
            SettingsClick::Cancel => self.apply_settings_outcome(SettingsOutcome::Cancel),
        }
    }

    fn apply_settings_outcome(&mut self, outcome: SettingsOutcome) -> bool {
        match outcome {
            SettingsOutcome::None => false,
            SettingsOutcome::PreviewTheme(name) => {
                if self.theme_before_preview.is_none() {
                    self.theme_before_preview = Some(self.config.colors.clone());
                }
                if crate::config::theme::Theme::apply_to_config(name, &mut self.config) {
                    self.config_dirty = true;
                }
                false
            }
            SettingsOutcome::Save => self.save_settings(),
            SettingsOutcome::Cancel => {
                self.hide_settings();
                false
            }
            SettingsOutcome::SaveSshHost { original, entry } => {
                self.save_ssh_host(original, entry);
                false
            }
            SettingsOutcome::ConnectSsh(host) => {
                self.hide_settings();
                if let Err(e) = self.open_tab_running("ssh", &[host]) {
                    tracing::warn!(error = %e, "Failed to open SSH tab");
                }
                false
            }
            SettingsOutcome::OpenQuickConnect => {
                self.hide_settings();
                self.show_quick_connect();
                false
            }
            SettingsOutcome::PreviewLanguage(language) => {
                if self.language_before_preview.is_none() {
                    self.language_before_preview = Some(crate::ui::i18n::language());
                }
                crate::ui::i18n::set_language(language);
                false
            }
            SettingsOutcome::PreviewAnimation => {
                // A prévia se mexe por um tempo e para: o app volta a dormir.
                let preview = self.settings_screen.as_ref().map(|screen| {
                    screen.preview_motion(
                        self.config.window.animations,
                        crate::ui::tab_animation::system_prefers_reduced_motion(),
                    )
                });
                if preview.is_some_and(|motion| motion.style.moves()) {
                    crate::ui::glow::keep_awake(crate::ui::glow::GLOW_DURATION);
                }
                false
            }
            SettingsOutcome::NewPersistentTab => {
                self.hide_settings();
                if let Err(e) = self.open_persistent_tab() {
                    tracing::warn!(error = %e, "Failed to open persistent tab");
                }
                false
            }
        }
    }

    /// Salva o que está na tela e fecha. Devolve `true` se o config mudou.
    fn save_settings(&mut self) -> bool {
        let Some(screen) = self.settings_screen.take() else {
            return false;
        };
        // A prévia já trocou as cores: compara com as de antes dela.
        let before = self.theme_before_preview.take();
        if let Some(before) = &before {
            self.config.colors = before.clone();
        }
        let changed = screen.save(&mut self.config);
        // O idioma escolhido fica (a prévia já o aplicou).
        self.language_before_preview = None;
        crate::ui::i18n::set_language(crate::ui::i18n::language_from_config(&self.config.language));
        if changed || before.is_some() {
            self.config_dirty = true;
            // A sidebar muda na hora (não só na próxima abertura).
            if self.sidebar_visible != self.config.sidebar.visible {
                self.toggle_sidebar();
            }
            // Memória por tab ligada/desligada aparece já no próximo quadro.
            self.refresh_sidebar(true);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::settings::{SettingsSection, THEMES};

    fn app() -> AppState {
        let mut app = AppState::new(80, 24).unwrap();
        app.config.colors.name = "dark".into();
        app.config.font.size = 14.0;
        app.config.font.family = "Cascadia Mono".into();
        app
    }

    /// A tela só existe aberta: fechar descarta o estado (#27).
    #[test]
    fn settings_state_exists_only_while_open() {
        let mut app = app();
        assert!(!app.is_settings_visible());
        assert!(app.settings_screen().is_none());
        app.show_settings();
        assert!(app.settings_screen().is_some());
        app.hide_settings();
        assert!(app.settings_screen().is_none());
    }

    #[test]
    fn saving_without_changes_does_not_mark_dirty() {
        let mut app = app();
        app.show_settings();
        assert!(!app.handle_settings_key(SettingsInputKey::Down));
        assert!(!app.handle_settings_key(SettingsInputKey::Enter));
        assert!(!app.config_dirty);
        assert!(!app.is_settings_visible());
        app.show_settings();
        assert!(!app.handle_settings_key(SettingsInputKey::Escape));
        assert!(!app.is_settings_visible());
    }

    #[test]
    fn theme_preview_then_cancel_restores_and_save_keeps() {
        let mut app = app();
        let before = app.config.colors.clone();
        app.show_settings();
        app.handle_settings_key(SettingsInputKey::Right);
        assert_eq!(app.config.colors.name, THEMES[1], "prévia na hora");
        assert!(app.config_dirty);
        app.settings_click(SettingsClick::Cancel);
        assert_eq!(app.config.colors, before);

        app.show_settings();
        app.activate_settings_row(0);
        assert!(app.settings_click(SettingsClick::Save));
        assert_eq!(app.config.colors.name, THEMES[1]);
        assert!(!app.is_settings_visible());
    }

    /// Troca de idioma na hora; Cancelar volta ao de antes, Salvar fica.
    #[test]
    fn language_switches_live_and_cancel_restores() {
        use crate::ui::i18n::{language, set_language, tr, Language, Text};
        let mut app = app();
        set_language(Language::English);
        app.show_settings();
        app.settings_click(SettingsClick::Section(3));
        app.handle_settings_key(SettingsInputKey::Right); // English
        app.handle_settings_key(SettingsInputKey::Right); // Português
        assert_eq!(language(), Language::PortugueseBr);
        assert_eq!(tr(Text::Save), "Salvar", "o próximo frame já sai traduzido");
        app.settings_click(SettingsClick::Cancel);
        assert_eq!(language(), Language::English);

        app.show_settings();
        app.settings_click(SettingsClick::Section(3));
        app.handle_settings_key(SettingsInputKey::Right);
        app.handle_settings_key(SettingsInputKey::Right);
        app.settings_click(SettingsClick::Save);
        assert_eq!(language(), Language::PortugueseBr);
        assert_eq!(app.config.language, "pt-BR");
        set_language(Language::English);
    }

    /// #32: gravar host pela tela usa o arquivo de teste (nunca o do
    /// usuário), faz backup e preserva o resto do arquivo.
    #[test]
    fn saving_an_ssh_host_writes_the_test_file_with_backup() {
        use crate::ssh::config_edit::{ssh_config_path, HostEntry};
        let path = ssh_config_path().unwrap();
        assert!(path.starts_with(std::env::temp_dir()), "{path:?}");
        let original = "# meu comentário\nInclude extra.conf\n\nHost velho\n    User eu\n";
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, original).unwrap();
        let mut app = app();
        app.show_settings();
        let entry = HostEntry {
            alias: "novo".into(),
            host_name: Some("192.0.2.1".into()),
            ..Default::default()
        };
        app.save_ssh_host(None, entry);
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.starts_with(original.trim_end()));
        assert!(written.ends_with("Host novo\n    HostName 192.0.2.1\n"));
        let backups: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("config.bak-"))
            .collect();
        assert!(!backups.is_empty());
        assert!(app
            .toast()
            .is_some_and(|t| t.text.contains("~/.ssh/config")));
        // O host novo aparece na lista, editável.
        let screen = app.settings_screen().unwrap();
        assert!(!screen.is_editing_ssh_host());
        // Alias repetido: erro no formulário, arquivo igual.
        app.settings_click(SettingsClick::Section(2));
        let again = HostEntry {
            alias: "velho".into(),
            ..Default::default()
        };
        app.save_ssh_host(None, again);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), written);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn section_click_switches_section() {
        let mut app = app();
        app.show_settings();
        app.settings_click(SettingsClick::Section(6));
        assert_eq!(
            app.settings_screen().unwrap().section(),
            SettingsSection::About
        );
        #[cfg(target_os = "linux")]
        assert!(app.settings_screen().unwrap().rows().iter().any(|row| {
            row.field == crate::ui::settings::SettingsField::Memory && row.value.contains("PSS")
        }));
    }
}
