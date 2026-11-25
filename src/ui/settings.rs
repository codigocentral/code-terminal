//! Tela de configurações: seções à esquerda, opções da seção à direita.
//!
//! Funciona pelo teclado (↑/↓ move, ←/→ muda, Tab troca de seção, Enter
//! salva, Esc cancela) e pelo mouse (o desenho em `window.rs` registra as
//! linhas e os botões). O estado só existe com a tela aberta: o `AppState`
//! cria a tela ao abrir e a descarta ao fechar.

use crate::config::keybindings::{KeyBindings, Shortcut, EDITABLE_SHORTCUTS};
use crate::config::theme::Theme;
use crate::config::Config;
use crate::ui::i18n::{shortcut_name, tr, Text};
use crate::ui::tab_animation::{AnimationSpeed, Motion, TabAnimation};
use tracing::{info, warn};

/// Temas embutidos, na ordem em que a configuração e a paleta os percorrem
/// (a mesma lista de [`Theme::BUILT_IN_THEMES`]).
pub const THEMES: &[&str] = Theme::BUILT_IN_THEMES;

/// Tema seguinte ao `current` na lista (o primeiro, se `current` não estiver nela).
pub fn next_theme(current: &str) -> &'static str {
    let index = THEMES
        .iter()
        .position(|name| name.eq_ignore_ascii_case(current))
        .map_or(0, |i| (i + 1) % THEMES.len());
    THEMES[index]
}

const FONT_FAMILIES: &[&str] = &[
    "Cascadia Mono",
    "Consolas",
    "Fira Code",
    "JetBrains Mono",
    "Monaco",
    "Source Code Pro",
];

const FONT_SIZES: &[f32] = &[10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 18.0];

const CURSOR_STYLES: &[&str] = &["block", "underline", "bar"];

/// Prazos para o servidor soltar a GPU sem janela (`server.release_gpu_after`,
/// #1). "0" é nunca.
const RELEASE_GPU_CHOICES: &[&str] = &["30s", "1m", "5m", "15m", "0"];

/// Como o prazo aparece na tela ("5 min", "nunca").
fn release_gpu_label(value: &str) -> String {
    match crate::config::parse_release_after(value) {
        Some(None) => tr(Text::Never).to_string(),
        Some(Some(delay)) => {
            let secs = delay.as_secs();
            if secs % 3600 == 0 {
                format!("{} h", secs / 3600)
            } else if secs % 60 == 0 {
                format!("{} min", secs / 60)
            } else {
                format!("{secs} s")
            }
        }
        None => value.to_string(),
    }
}

/// Larguras da sidebar oferecidas (pixels).
const SIDEBAR_WIDTHS: &[f32] = &[180.0, 200.0, 230.0, 260.0, 300.0, 360.0];

const AI_PROVIDERS: &[crate::config::ai::AIProvider] = &[
    crate::config::ai::AIProvider::Anthropic,
    crate::config::ai::AIProvider::OpenAI,
    crate::config::ai::AIProvider::Ollama,
    crate::config::ai::AIProvider::Local,
    crate::config::ai::AIProvider::Custom,
];

/// Campo de texto com o cursor no fim, sem seleção.
fn text_field(text: &str) -> crate::ui::line_edit::LineEdit {
    let mut edit = crate::ui::line_edit::LineEdit::new(text);
    edit.apply(crate::ui::line_edit::LineEditKey::End);
    edit
}

/// Seções da tela, na ordem da coluna da esquerda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Appearance,
    Tabs,
    Ssh,
    Language,
    Shortcuts,
    Integrations,
    About,
}

/// Ordem das seções.
pub const SECTIONS: &[SettingsSection] = &[
    SettingsSection::Appearance,
    SettingsSection::Tabs,
    SettingsSection::Ssh,
    SettingsSection::Language,
    SettingsSection::Shortcuts,
    SettingsSection::Integrations,
    SettingsSection::About,
];

impl SettingsSection {
    /// Nome na coluna da esquerda.
    pub fn label(self) -> &'static str {
        tr(match self {
            SettingsSection::Appearance => Text::SectionAppearance,
            SettingsSection::Tabs => Text::SectionTabs,
            SettingsSection::Ssh => Text::SectionSsh,
            SettingsSection::Language => Text::SectionLanguage,
            SettingsSection::Shortcuts => Text::SectionShortcuts,
            SettingsSection::Integrations => Text::SectionIntegrations,
            SettingsSection::About => Text::SectionAbout,
        })
    }
}

/// Uma opção da tela.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    Theme,
    FontFamily,
    FontSize,
    CursorStyle,
    TabAnimation,
    AnimationSpeed,
    FollowReducedMotion,
    ReplaceProgramSpinner,
    /// Abrir a janela maximizada.
    StartMaximized,
    Sidebar,
    SidebarWidth,
    SidebarMemory,
    LinkTarget,
    AiEnabled,
    AiProvider,
    AiModel,
    AiKeyEnv,
    RestoreSession,
    ConfirmClose,
    /// Toda aba nova já nasce persistente (#21).
    PersistentByDefault,
    PersistentTab,
    /// Idioma da interface.
    Language,
    /// Host `n` do `~/.ssh/config`.
    SshHost(usize),
    SshNew,
    /// Abre o seletor de conexão rápida.
    SshQuickConnect,
    /// Campo `n` do formulário de host (ver `settings_ssh::FORM_FIELDS`).
    SshFormField(usize),
    SshFormConnect,
    SshFormSave,
    SshFormBack,
    SshFormError,
    /// Atalho (id de [`EDITABLE_SHORTCUTS`]).
    Shortcut(&'static str),
    Version,
    Creator,
    Publisher,
    License,
    Memory,
    /// Prazo para o servidor soltar a GPU sem janela.
    ReleaseGpuAfter,
    ConfigFile,
}

/// Como a linha reage a ←/→, Enter e clique.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// Lista de valores: ←/→ e clique trocam.
    Choice,
    /// Liga/desliga.
    Toggle,
    /// Enter ou clique executam (conectar, abrir).
    Action,
    /// Enter ou clique esperam a tecla nova.
    Shortcut,
    /// Só mostra.
    Info,
    /// Campo de texto (digitar edita).
    Text,
}

/// Uma linha da seção.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsRow {
    pub field: SettingsField,
    pub label: String,
    pub value: String,
    pub kind: RowKind,
}

/// Host do `~/.ssh/config` como aparece na seção SSH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshEntry {
    /// Nome (`Host`), que o `ssh` resolve sozinho.
    pub name: String,
    /// `usuário@endereço:porta`, para mostrar.
    pub target: String,
    /// Os campos, quando o host é um bloco editável do `~/.ssh/config`
    /// (hosts de arquivos incluídos ou com curinga só conectam).
    pub editable: Option<crate::ssh::config_edit::HostEntry>,
}

/// Teclas que a tela entende.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsInputKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    /// Tab: próxima seção.
    NextSection,
    /// Shift+Tab: seção anterior.
    PreviousSection,
    /// Letra digitada (filtra os atalhos).
    Char(char),
    Backspace,
    /// Tecla nova de um atalho, já no formato do config ("ctrl+shift+t").
    Captured(String),
}

/// O que a tela pede ao app depois de uma tecla ou clique.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsOutcome {
    None,
    /// Mostrar o tema (prévia, sem gravar).
    PreviewTheme(&'static str),
    /// Salvar e fechar.
    Save,
    /// Fechar sem salvar.
    Cancel,
    /// Abrir uma tab com `ssh <host>`.
    ConnectSsh(String),
    /// Gravar o host no `~/.ssh/config` (criar quando `original` é `None`).
    SaveSshHost {
        original: Option<String>,
        entry: crate::ssh::config_edit::HostEntry,
    },
    /// Abrir o seletor de conexão SSH.
    OpenQuickConnect,
    /// Abrir uma tab persistente.
    NewPersistentTab,
    /// A animação mudou: mostrar a prévia (acordar o render por um tempo).
    PreviewAnimation,
    /// O idioma mudou: aplicar na hora (o Cancelar volta).
    PreviewLanguage(crate::ui::i18n::Language),
}

#[derive(Debug)]
pub struct SettingsScreen {
    section: usize,
    selected: usize,
    /// Primeira linha visível (a lista de atalhos rola).
    first: usize,
    theme_index: usize,
    font_size: f32,
    font_families: Vec<String>,
    font_family_index: usize,
    cursor_index: usize,
    animation: TabAnimation,
    speed: AnimationSpeed,
    follow_reduced_motion: bool,
    replace_program_spinner: bool,
    start_maximized: bool,
    /// Quando a animação foi trocada na tela (a prévia anima a partir daí).
    preview_since: Option<std::time::Instant>,
    /// 0 = automático (sistema), depois os de `Language::ALL`.
    language_index: usize,
    sidebar: bool,
    sidebar_width_index: usize,
    sidebar_memory: bool,
    link_target: crate::config::LinkTarget,
    ai_enabled: bool,
    ai_provider: crate::config::ai::AIProvider,
    ai_model: crate::ui::line_edit::LineEdit,
    ai_key_env: crate::ui::line_edit::LineEdit,
    restore_session: bool,
    confirm_close: bool,
    persistent_by_default: bool,
    keybindings: KeyBindings,
    /// Busca na lista de atalhos.
    query: String,
    /// Atalho esperando a tecla nova.
    capturing: Option<&'static str>,
    ssh_hosts: Vec<SshEntry>,
    /// Formulário de host aberto na seção SSH.
    ssh_form: Option<crate::ui::settings_ssh::SshForm>,
    memory: Option<String>,
    /// Prazos de soltar a GPU (os da lista e o do config, se for outro).
    release_gpu_choices: Vec<String>,
    release_gpu_index: usize,
    config_path: Option<String>,
}

/// Próximo índice numa lista circular.
fn cycle(index: usize, len: usize, forward: bool) -> usize {
    if len == 0 {
        0
    } else if forward {
        (index + 1) % len
    } else {
        (index + len - 1) % len
    }
}

fn on_off(value: bool) -> String {
    tr(if value { Text::On } else { Text::Off }).to_string()
}

impl SettingsScreen {
    /// Monta a tela com os valores atuais. `ssh_hosts` e `memory` são lidos
    /// por quem abre (o app), uma vez, na abertura.
    pub fn new(config: &Config, ssh_hosts: Vec<SshEntry>, memory: Option<String>) -> Self {
        let theme_index = THEMES
            .iter()
            .position(|&t| t.eq_ignore_ascii_case(&config.colors.name))
            .unwrap_or(0);
        let font_size_index = FONT_SIZES
            .iter()
            .position(|&s| (s - config.font.size).abs() < 0.5)
            .unwrap_or(4);
        // A fonte configurada entra na lista mesmo que não seja uma das
        // conhecidas: salvar não pode trocá-la sem o usuário pedir.
        let mut font_families: Vec<String> = FONT_FAMILIES.iter().map(|f| f.to_string()).collect();
        let font_family_index = match font_families
            .iter()
            .position(|f| config.font.family.eq_ignore_ascii_case(f))
        {
            Some(index) => index,
            None => {
                font_families.push(config.font.family.clone());
                font_families.len() - 1
            }
        };
        let cursor_index = CURSOR_STYLES
            .iter()
            .position(|c| c.eq_ignore_ascii_case(&config.terminal.cursor_style))
            .unwrap_or(0);
        // O prazo do config entra na lista mesmo que não seja um dos de lá.
        let mut release_gpu_choices: Vec<String> =
            RELEASE_GPU_CHOICES.iter().map(|c| c.to_string()).collect();
        let configured = config.server.release_gpu_after.trim().to_string();
        let release_gpu_index = match release_gpu_choices.iter().position(|c| {
            crate::config::parse_release_after(c) == crate::config::parse_release_after(&configured)
        }) {
            Some(index) => index,
            None => {
                release_gpu_choices.push(configured);
                release_gpu_choices.len() - 1
            }
        };
        Self {
            section: 0,
            selected: 0,
            first: 0,
            theme_index,
            font_size: FONT_SIZES[font_size_index],
            font_families,
            font_family_index,
            cursor_index,
            animation: config.window.tab_animation,
            speed: config.window.animation_speed,
            follow_reduced_motion: config.window.follow_reduced_motion,
            replace_program_spinner: config.window.replace_program_spinner,
            start_maximized: config.window.start_maximized,
            preview_since: None,
            language_index: language_choice_index(&config.language),
            sidebar: config.sidebar.visible,
            sidebar_width_index: SIDEBAR_WIDTHS
                .iter()
                .position(|&w| (w - config.sidebar.width).abs() < 1.0)
                .unwrap_or(usize::MAX),
            sidebar_memory: config.sidebar.show_memory,
            link_target: config.browser.open_links_in,
            ai_enabled: config.ai.enabled,
            ai_provider: config.ai.provider,
            ai_model: text_field(&config.ai.model.name),
            ai_key_env: text_field(&config.ai.model.api_key_env),
            restore_session: config.session.restore,
            confirm_close: config.window.confirm_close_tab,
            persistent_by_default: config.session.persistent_tabs,
            keybindings: config.keybindings.clone(),
            query: String::new(),
            capturing: None,
            ssh_hosts,
            ssh_form: None,
            memory,
            release_gpu_choices,
            release_gpu_index,
            config_path: Config::config_path()
                .ok()
                .map(|path| path.display().to_string()),
        }
    }

    /// Seção aberta.
    pub fn section(&self) -> SettingsSection {
        SECTIONS[self.section.min(SECTIONS.len() - 1)]
    }

    /// Abre a seção `index` (clique na coluna da esquerda).
    pub fn select_section(&mut self, index: usize) {
        if index < SECTIONS.len() && index != self.section {
            self.section = index;
            self.selected = 0;
            self.first = 0;
            self.query.clear();
            self.capturing = None;
            self.ssh_form = None;
        }
    }

    fn switch_section(&mut self, forward: bool) {
        self.select_section(cycle(self.section, SECTIONS.len(), forward));
    }

    /// Linha selecionada na seção.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Seleciona a linha `index` (mouse em cima). Fora da lista é ignorado.
    pub fn select(&mut self, index: usize) {
        if index < self.rows().len() {
            self.selected = index;
        }
    }

    /// Opção da linha selecionada.
    pub fn selected_field(&self) -> Option<SettingsField> {
        self.rows().get(self.selected).map(|row| row.field)
    }

    /// Primeira linha visível no último desenho.
    pub fn first_visible(&self) -> usize {
        self.first
    }

    /// Guarda a rolagem que o desenho usou.
    pub fn set_first_visible(&mut self, first: usize) {
        self.first = first;
    }

    /// Busca digitada na seção de atalhos.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Esperando a tecla nova de um atalho.
    pub fn is_capturing(&self) -> bool {
        self.capturing.is_some()
    }

    /// Tema escolhido na tela.
    pub fn theme_name(&self) -> &'static str {
        THEMES[self.theme_index.min(THEMES.len() - 1)]
    }

    /// Animação escolhida na tela, para a prévia (com "menos movimento"
    /// resolvido como o app faria).
    pub fn preview_motion(&self, animations: bool, system_reduced: bool) -> Motion {
        Motion::resolve(
            animations,
            self.animation,
            self.speed,
            self.follow_reduced_motion,
            system_reduced,
        )
    }

    /// Idade do "aviso" da prévia: anima a partir da última troca.
    pub fn preview_age(&self) -> std::time::Duration {
        self.preview_since
            .map_or(crate::ui::glow::GLOW_DURATION, |since| since.elapsed())
    }

    /// Largura escolhida da sidebar (`None` = a do config, fora da lista).
    fn sidebar_width(&self) -> Option<f32> {
        SIDEBAR_WIDTHS.get(self.sidebar_width_index).copied()
    }

    /// Sidebar visível escolhida na tela (aplicada ao salvar).
    pub fn sidebar_enabled(&self) -> bool {
        self.sidebar
    }

    /// Atalhos como estão na tela (com as trocas ainda não salvas).
    pub fn keybindings(&self) -> &KeyBindings {
        &self.keybindings
    }

    /// Linhas da seção aberta.
    pub fn rows(&self) -> Vec<SettingsRow> {
        let row = |field, label: &str, value: String, kind| SettingsRow {
            field,
            label: label.to_string(),
            value,
            kind,
        };
        match self.section() {
            SettingsSection::Appearance => vec![
                row(
                    SettingsField::Theme,
                    tr(Text::Theme),
                    self.theme_name().to_string(),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::FontFamily,
                    tr(Text::Font),
                    self.font_families[self.font_family_index].clone(),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::FontSize,
                    tr(Text::FontSize),
                    format!("{}", self.font_size),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::CursorStyle,
                    tr(Text::Cursor),
                    CURSOR_STYLES[self.cursor_index].to_string(),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::TabAnimation,
                    tr(Text::TabAnimationLabel),
                    self.animation.label().to_string(),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::AnimationSpeed,
                    tr(Text::AnimationSpeedLabel),
                    self.speed.label().to_string(),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::FollowReducedMotion,
                    tr(Text::FollowReducedMotion),
                    on_off(self.follow_reduced_motion),
                    RowKind::Toggle,
                ),
                row(
                    SettingsField::ReplaceProgramSpinner,
                    tr(Text::ReplaceProgramSpinner),
                    on_off(self.replace_program_spinner),
                    RowKind::Toggle,
                ),
                row(
                    SettingsField::StartMaximized,
                    tr(Text::StartMaximized),
                    on_off(self.start_maximized),
                    RowKind::Toggle,
                ),
            ],
            SettingsSection::Tabs => vec![
                row(
                    SettingsField::Sidebar,
                    tr(Text::Sidebar),
                    on_off(self.sidebar),
                    RowKind::Toggle,
                ),
                row(
                    SettingsField::RestoreSession,
                    tr(Text::RestoreTabs),
                    on_off(self.restore_session),
                    RowKind::Toggle,
                ),
                row(
                    SettingsField::ConfirmClose,
                    tr(Text::ConfirmClose),
                    on_off(self.confirm_close),
                    RowKind::Toggle,
                ),
                row(
                    SettingsField::PersistentByDefault,
                    tr(Text::PersistentByDefault),
                    on_off(self.persistent_by_default),
                    RowKind::Toggle,
                ),
                row(
                    SettingsField::PersistentTab,
                    tr(Text::NewPersistentTab),
                    tr(Text::Open).to_string(),
                    RowKind::Action,
                ),
                row(
                    SettingsField::SidebarWidth,
                    tr(Text::SidebarWidthLabel),
                    self.sidebar_width_label(),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::SidebarMemory,
                    tr(Text::SidebarMemoryLabel),
                    on_off(self.sidebar_memory),
                    RowKind::Toggle,
                ),
            ],
            SettingsSection::Integrations => vec![
                row(
                    SettingsField::LinkTarget,
                    tr(Text::OpenLinksIn),
                    tr(match self.link_target {
                        crate::config::LinkTarget::Builtin => Text::LinksBuiltin,
                        crate::config::LinkTarget::System => Text::LinksSystem,
                    })
                    .to_string(),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::AiEnabled,
                    tr(Text::AiEnabledLabel),
                    on_off(self.ai_enabled),
                    RowKind::Toggle,
                ),
                row(
                    SettingsField::AiProvider,
                    tr(Text::AiProviderLabel),
                    self.ai_provider.to_string(),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::AiModel,
                    tr(Text::AiModelLabel),
                    self.ai_model.text(),
                    RowKind::Text,
                ),
                row(
                    SettingsField::AiKeyEnv,
                    tr(Text::AiKeyEnvLabel),
                    self.ai_key_env.text(),
                    RowKind::Text,
                ),
            ],
            SettingsSection::Language => vec![row(
                SettingsField::Language,
                tr(Text::InterfaceLanguage),
                language_choice_label(self.language_index).to_string(),
                RowKind::Choice,
            )],
            SettingsSection::Ssh => match &self.ssh_form {
                Some(form) => self.ssh_form_rows(form),
                None => self.ssh_host_rows(),
            },
            SettingsSection::Shortcuts => {
                let query = self.query.to_lowercase();
                EDITABLE_SHORTCUTS
                    .iter()
                    .filter_map(|&id| {
                        let binding = self.keybindings.get(id)?;
                        let name = shortcut_name(id);
                        let shown = KeyBindings::display(binding);
                        let hit = query.is_empty()
                            || name.to_lowercase().contains(&query)
                            || shown.to_lowercase().contains(&query);
                        hit.then(|| SettingsRow {
                            field: SettingsField::Shortcut(id),
                            label: name.to_string(),
                            value: if self.capturing == Some(id) {
                                "\u{2026}".to_string()
                            } else {
                                shown
                            },
                            kind: RowKind::Shortcut,
                        })
                    })
                    .collect()
            }
            SettingsSection::About => vec![
                row(
                    SettingsField::Creator,
                    tr(Text::AboutCreator),
                    "Diogo Costa Santos".to_string(),
                    RowKind::Info,
                ),
                row(
                    SettingsField::Publisher,
                    tr(Text::AboutPublisher),
                    "Código Central".to_string(),
                    RowKind::Info,
                ),
                row(
                    SettingsField::License,
                    tr(Text::AboutLicense),
                    env!("CARGO_PKG_LICENSE").to_string(),
                    RowKind::Info,
                ),
                row(
                    SettingsField::Version,
                    tr(Text::Version),
                    env!("CARGO_PKG_VERSION").to_string(),
                    RowKind::Info,
                ),
                row(
                    SettingsField::Memory,
                    tr(Text::Memory),
                    self.memory.clone().unwrap_or_else(|| "-".to_string()),
                    RowKind::Info,
                ),
                row(
                    SettingsField::ReleaseGpuAfter,
                    tr(Text::ReleaseGpuAfter),
                    release_gpu_label(&self.release_gpu_choices[self.release_gpu_index]),
                    RowKind::Choice,
                ),
                row(
                    SettingsField::ConfigFile,
                    tr(Text::ConfigFile),
                    self.config_path.clone().unwrap_or_else(|| "-".to_string()),
                    RowKind::Info,
                ),
            ],
        }
    }

    /// Linhas da lista de hosts SSH.
    fn ssh_host_rows(&self) -> Vec<SettingsRow> {
        let mut rows: Vec<SettingsRow> = self
            .ssh_hosts
            .iter()
            .enumerate()
            .map(|(i, host)| SettingsRow {
                field: SettingsField::SshHost(i),
                label: format!("{}  {}", host.name, host.target),
                value: tr(if host.editable.is_some() {
                    Text::SshEditHost
                } else {
                    Text::SshConnect
                })
                .to_string(),
                kind: RowKind::Action,
            })
            .collect();
        let action = |field, label: Text, value: Text| SettingsRow {
            field,
            label: tr(label).to_string(),
            value: tr(value).to_string(),
            kind: RowKind::Action,
        };
        if rows.is_empty() {
            rows.push(SettingsRow {
                field: SettingsField::SshFormError,
                label: tr(Text::SshNoHosts).to_string(),
                value: String::new(),
                kind: RowKind::Info,
            });
        }
        rows.push(action(SettingsField::SshNew, Text::SshNewHost, Text::Open));
        rows.push(action(
            SettingsField::SshQuickConnect,
            Text::SshNewConnection,
            Text::Open,
        ));
        rows
    }

    /// Linhas do formulário de host.
    fn ssh_form_rows(&self, form: &crate::ui::settings_ssh::SshForm) -> Vec<SettingsRow> {
        use crate::ui::settings_ssh::FORM_FIELDS;
        let mut rows: Vec<SettingsRow> = FORM_FIELDS
            .iter()
            .enumerate()
            .map(|(i, &label)| SettingsRow {
                field: SettingsField::SshFormField(i),
                label: tr(label).to_string(),
                value: form.fields[i].text(),
                kind: RowKind::Text,
            })
            .collect();
        let action = |field, label: Text| SettingsRow {
            field,
            label: tr(label).to_string(),
            value: tr(Text::Open).to_string(),
            kind: RowKind::Action,
        };
        rows.push(SettingsRow {
            value: tr(Text::Save).to_string(),
            ..action(SettingsField::SshFormSave, Text::SshSaveHost)
        });
        if form.original.is_some() {
            rows.push(SettingsRow {
                value: tr(Text::SshConnect).to_string(),
                ..action(SettingsField::SshFormConnect, Text::SshConnectHost)
            });
        }
        rows.push(SettingsRow {
            value: String::new(),
            ..action(SettingsField::SshFormBack, Text::Back)
        });
        if let Some(error) = &form.error {
            rows.push(SettingsRow {
                field: SettingsField::SshFormError,
                label: error.clone(),
                value: String::new(),
                kind: RowKind::Info,
            });
        }
        rows
    }

    /// Campo de texto em edição e a posição do cursor (para desenhar).
    pub fn text_cursor(&self) -> Option<(usize, usize)> {
        let field = self.selected_field()?;
        let edit = match field {
            SettingsField::SshFormField(i) => self.ssh_form.as_ref()?.fields.get(i)?,
            SettingsField::AiModel => &self.ai_model,
            SettingsField::AiKeyEnv => &self.ai_key_env,
            _ => return None,
        };
        Some((self.selected, edit.cursor()))
    }

    /// Campo de texto fora do formulário SSH (modelo e variável da IA).
    fn text_key(&mut self, key: &SettingsInputKey) -> Option<SettingsOutcome> {
        use crate::ui::line_edit::LineEditKey;
        let edit = match self.selected_field()? {
            SettingsField::AiModel => &mut self.ai_model,
            SettingsField::AiKeyEnv => &mut self.ai_key_env,
            _ => return None,
        };
        let key = match key {
            SettingsInputKey::Char(c) if !c.is_control() => LineEditKey::Char(*c),
            SettingsInputKey::Backspace => LineEditKey::Backspace,
            SettingsInputKey::Left => LineEditKey::Left,
            SettingsInputKey::Right => LineEditKey::Right,
            _ => return None,
        };
        edit.apply(key);
        Some(SettingsOutcome::None)
    }

    fn sidebar_width_label(&self) -> String {
        SIDEBAR_WIDTHS
            .get(self.sidebar_width_index)
            .map_or_else(|| "custom".to_string(), |w| format!("{w:.0} px"))
    }

    /// O formulário de host está aberto.
    pub fn is_editing_ssh_host(&self) -> bool {
        self.ssh_form.is_some()
    }

    /// O app gravou o host: fecha o formulário e mostra a lista nova.
    pub fn ssh_saved(&mut self, hosts: Vec<SshEntry>) {
        self.ssh_hosts = hosts;
        self.ssh_form = None;
        self.selected = 0;
        self.first = 0;
    }

    /// O app não conseguiu gravar: a mensagem aparece no formulário.
    pub fn set_ssh_error(&mut self, message: String) {
        if let Some(form) = self.ssh_form.as_mut() {
            form.error = Some(message);
        }
    }

    fn open_ssh_form(&mut self, entry: Option<crate::ssh::config_edit::HostEntry>) {
        self.ssh_form = Some(crate::ui::settings_ssh::SshForm::new(entry.as_ref()));
        self.selected = 0;
        self.first = 0;
    }

    /// Tecla com o formulário de host aberto. `None` quando a tecla segue
    /// o caminho normal (setas, Enter numa ação, Tab).
    fn ssh_form_key(&mut self, key: &SettingsInputKey) -> Option<SettingsOutcome> {
        use crate::ui::line_edit::LineEditKey;
        self.ssh_form.as_ref()?;
        if *key == SettingsInputKey::Escape {
            self.ssh_form = None;
            self.selected = 0;
            return Some(SettingsOutcome::None);
        }
        let Some(SettingsField::SshFormField(field)) = self.selected_field() else {
            return None;
        };
        let edit = match key {
            SettingsInputKey::Char(c) if !c.is_control() => LineEditKey::Char(*c),
            SettingsInputKey::Backspace => LineEditKey::Backspace,
            SettingsInputKey::Left => LineEditKey::Left,
            SettingsInputKey::Right => LineEditKey::Right,
            // Enter num campo vai para o próximo.
            SettingsInputKey::Enter => {
                self.selected += 1;
                return Some(SettingsOutcome::None);
            }
            _ => return None,
        };
        if let Some(form) = self.ssh_form.as_mut() {
            form.edit(field, edit);
        }
        Some(SettingsOutcome::None)
    }

    /// Aplica uma tecla.
    pub fn key(&mut self, key: SettingsInputKey) -> SettingsOutcome {
        if let Some(outcome) = self.ssh_form_key(&key) {
            return outcome;
        }
        if let Some(outcome) = self.text_key(&key) {
            return outcome;
        }
        if let Some(id) = self.capturing {
            match key {
                SettingsInputKey::Escape => self.capturing = None,
                // Tecla que não vira atalho válido: continua esperando.
                SettingsInputKey::Captured(binding) if self.keybindings.set(id, &binding) => {
                    self.capturing = None;
                }
                _ => {}
            }
            return SettingsOutcome::None;
        }
        let total = self.rows().len();
        match key {
            SettingsInputKey::Up => {
                self.selected = cycle(self.selected, total, false);
                SettingsOutcome::None
            }
            SettingsInputKey::Down => {
                self.selected = cycle(self.selected, total, true);
                SettingsOutcome::None
            }
            SettingsInputKey::Left => self.change(false),
            SettingsInputKey::Right => self.change(true),
            SettingsInputKey::NextSection => {
                self.switch_section(true);
                SettingsOutcome::None
            }
            SettingsInputKey::PreviousSection => {
                self.switch_section(false);
                SettingsOutcome::None
            }
            SettingsInputKey::Enter => match self.rows().get(self.selected).map(|r| r.kind) {
                Some(RowKind::Action) | Some(RowKind::Shortcut) => self.activate(self.selected),
                _ => SettingsOutcome::Save,
            },
            SettingsInputKey::Escape => {
                if self.query.is_empty() {
                    SettingsOutcome::Cancel
                } else {
                    self.query.clear();
                    self.selected = 0;
                    SettingsOutcome::None
                }
            }
            SettingsInputKey::Char(c) => {
                if self.section() == SettingsSection::Shortcuts && !c.is_control() {
                    self.query.push(c);
                    self.selected = 0;
                    self.first = 0;
                }
                SettingsOutcome::None
            }
            SettingsInputKey::Backspace => {
                if self.query.pop().is_some() {
                    self.selected = 0;
                }
                SettingsOutcome::None
            }
            SettingsInputKey::Captured(_) => SettingsOutcome::None,
        }
    }

    /// Roda do mouse: move a seleção `lines` linhas, parando nas pontas.
    pub fn scroll_by(&mut self, lines: i32) {
        let Some(last) = self.rows().len().checked_sub(1) else {
            return;
        };
        let target = self.selected as i64 + i64::from(lines);
        self.selected = target.clamp(0, last as i64) as usize;
    }

    /// Clique na linha `index`: escolhe o próximo valor, liga/desliga,
    /// executa a ação ou começa a esperar a tecla do atalho.
    pub fn activate(&mut self, index: usize) -> SettingsOutcome {
        let Some(row) = self.rows().into_iter().nth(index) else {
            return SettingsOutcome::None;
        };
        self.selected = index;
        match (row.kind, row.field) {
            (RowKind::Choice | RowKind::Toggle, _) => self.change(true),
            (RowKind::Shortcut, SettingsField::Shortcut(id)) => {
                self.capturing = Some(id);
                SettingsOutcome::None
            }
            (RowKind::Action, SettingsField::SshHost(i)) => match self.ssh_hosts.get(i) {
                // Bloco editável: abre o formulário (de lá dá para conectar).
                Some(SshEntry {
                    editable: Some(entry),
                    ..
                }) => {
                    let entry = entry.clone();
                    self.open_ssh_form(Some(entry));
                    SettingsOutcome::None
                }
                Some(host) => SettingsOutcome::ConnectSsh(host.name.clone()),
                None => SettingsOutcome::None,
            },
            (RowKind::Action, SettingsField::SshNew) => {
                self.open_ssh_form(None);
                SettingsOutcome::None
            }
            (RowKind::Action, SettingsField::SshQuickConnect) => SettingsOutcome::OpenQuickConnect,
            (RowKind::Action, SettingsField::SshFormBack) => {
                self.ssh_form = None;
                self.selected = 0;
                SettingsOutcome::None
            }
            (RowKind::Action, SettingsField::SshFormConnect) => self
                .ssh_form
                .as_ref()
                .and_then(|form| form.original.clone())
                .map_or(SettingsOutcome::None, SettingsOutcome::ConnectSsh),
            (RowKind::Action, SettingsField::SshFormSave) => match self.ssh_form.as_mut() {
                Some(form) => match form.entry() {
                    Ok(entry) => SettingsOutcome::SaveSshHost {
                        original: form.original.clone(),
                        entry,
                    },
                    Err(message) => {
                        form.error = Some(message);
                        SettingsOutcome::None
                    }
                },
                None => SettingsOutcome::None,
            },
            (RowKind::Action, SettingsField::PersistentTab) => SettingsOutcome::NewPersistentTab,
            _ => SettingsOutcome::None,
        }
    }

    /// Muda o valor da linha selecionada.
    fn change(&mut self, forward: bool) -> SettingsOutcome {
        match self.selected_field() {
            Some(SettingsField::Theme) => {
                self.theme_index = cycle(self.theme_index, THEMES.len(), forward);
                return SettingsOutcome::PreviewTheme(self.theme_name());
            }
            Some(SettingsField::FontSize) => {
                let idx = FONT_SIZES
                    .iter()
                    .position(|&s| (s - self.font_size).abs() < 0.5)
                    .unwrap_or(4);
                let new_idx = if forward {
                    (idx + 1).min(FONT_SIZES.len() - 1)
                } else {
                    idx.saturating_sub(1)
                };
                self.font_size = FONT_SIZES[new_idx];
            }
            Some(SettingsField::FontFamily) => {
                self.font_family_index =
                    cycle(self.font_family_index, self.font_families.len(), forward);
            }
            Some(SettingsField::CursorStyle) => {
                self.cursor_index = cycle(self.cursor_index, CURSOR_STYLES.len(), forward);
            }
            Some(SettingsField::TabAnimation) => {
                let all = TabAnimation::ALL;
                let index = all.iter().position(|&a| a == self.animation).unwrap_or(0);
                self.animation = all[cycle(index, all.len(), forward)];
                self.preview_since = Some(std::time::Instant::now());
                return SettingsOutcome::PreviewAnimation;
            }
            Some(SettingsField::AnimationSpeed) => {
                let all = AnimationSpeed::ALL;
                let index = all.iter().position(|&a| a == self.speed).unwrap_or(1);
                self.speed = all[cycle(index, all.len(), forward)];
                self.preview_since = Some(std::time::Instant::now());
                return SettingsOutcome::PreviewAnimation;
            }
            Some(SettingsField::Language) => {
                self.language_index = cycle(self.language_index, LANGUAGE_CHOICES, forward);
                let code = language_choice_code(self.language_index);
                return SettingsOutcome::PreviewLanguage(crate::ui::i18n::language_from_config(
                    code,
                ));
            }
            Some(SettingsField::FollowReducedMotion) => {
                self.follow_reduced_motion = !self.follow_reduced_motion;
            }
            Some(SettingsField::ReplaceProgramSpinner) => {
                self.replace_program_spinner = !self.replace_program_spinner;
            }
            Some(SettingsField::StartMaximized) => {
                self.start_maximized = !self.start_maximized;
            }
            Some(SettingsField::ReleaseGpuAfter) => {
                self.release_gpu_index = cycle(
                    self.release_gpu_index,
                    self.release_gpu_choices.len(),
                    forward,
                );
            }
            Some(SettingsField::Sidebar) => self.sidebar = !self.sidebar,
            Some(SettingsField::SidebarWidth) => {
                self.sidebar_width_index = if self.sidebar_width_index >= SIDEBAR_WIDTHS.len() {
                    2
                } else {
                    cycle(self.sidebar_width_index, SIDEBAR_WIDTHS.len(), forward)
                };
            }
            Some(SettingsField::SidebarMemory) => self.sidebar_memory = !self.sidebar_memory,
            Some(SettingsField::LinkTarget) => {
                use crate::config::LinkTarget;
                self.link_target = match self.link_target {
                    LinkTarget::Builtin => LinkTarget::System,
                    LinkTarget::System => LinkTarget::Builtin,
                };
            }
            Some(SettingsField::AiEnabled) => self.ai_enabled = !self.ai_enabled,
            Some(SettingsField::AiProvider) => {
                let index = AI_PROVIDERS
                    .iter()
                    .position(|&p| p == self.ai_provider)
                    .unwrap_or(0);
                self.ai_provider = AI_PROVIDERS[cycle(index, AI_PROVIDERS.len(), forward)];
            }
            Some(SettingsField::RestoreSession) => self.restore_session = !self.restore_session,
            Some(SettingsField::ConfirmClose) => self.confirm_close = !self.confirm_close,
            Some(SettingsField::PersistentByDefault) => {
                self.persistent_by_default = !self.persistent_by_default;
            }
            _ => {}
        }
        SettingsOutcome::None
    }

    /// Aplica as escolhas no config e grava no disco se algo mudou.
    /// Devolve `true` se o config mudou.
    pub fn save(&self, config: &mut Config) -> bool {
        let theme_name = self.theme_name().to_string();
        let font_family = self.font_families[self.font_family_index].clone();
        let cursor = CURSOR_STYLES[self.cursor_index].to_string();
        let keybindings_changed = config.keybindings != self.keybindings;
        let release_gpu_after = self.release_gpu_choices[self.release_gpu_index].clone();

        let changed = !config.colors.name.eq_ignore_ascii_case(&theme_name)
            || config.font.size != self.font_size
            || config.font.family != font_family
            || config.terminal.cursor_style != cursor
            || config.sidebar.visible != self.sidebar
            || self
                .sidebar_width()
                .is_some_and(|w| (w - config.sidebar.width).abs() >= 1.0)
            || config.sidebar.show_memory != self.sidebar_memory
            || config.browser.open_links_in != self.link_target
            || config.ai.enabled != self.ai_enabled
            || config.ai.provider != self.ai_provider
            || config.ai.model.name != self.ai_model.text().trim()
            || config.ai.model.api_key_env != self.ai_key_env.text().trim()
            || config.session.restore != self.restore_session
            || config.window.confirm_close_tab != self.confirm_close
            || config.session.persistent_tabs != self.persistent_by_default
            || config.window.tab_animation != self.animation
            || config.window.animation_speed != self.speed
            || config.window.follow_reduced_motion != self.follow_reduced_motion
            || config.window.replace_program_spinner != self.replace_program_spinner
            || config.window.start_maximized != self.start_maximized
            || language_choice_index(&config.language) != self.language_index
            || config.server.release_gpu_after.trim() != release_gpu_after
            || keybindings_changed;

        if changed {
            Theme::apply_to_config(&theme_name, config);
            config.font.size = self.font_size;
            config.font.family.clone_from(&font_family);
            config.terminal.cursor_style = cursor;
            config.sidebar.visible = self.sidebar;
            if let Some(width) = self.sidebar_width() {
                config.sidebar.width = width;
            }
            config.sidebar.show_memory = self.sidebar_memory;
            config.browser.open_links_in = self.link_target;
            config.ai.enabled = self.ai_enabled;
            config.ai.provider = self.ai_provider;
            config.ai.model.name = self.ai_model.text().trim().to_string();
            config.ai.model.api_key_env = self.ai_key_env.text().trim().to_string();
            config.session.restore = self.restore_session;
            config.window.confirm_close_tab = self.confirm_close;
            config.session.persistent_tabs = self.persistent_by_default;
            config.window.tab_animation = self.animation;
            config.window.animation_speed = self.speed;
            config.window.follow_reduced_motion = self.follow_reduced_motion;
            config.window.replace_program_spinner = self.replace_program_spinner;
            config.window.start_maximized = self.start_maximized;
            config.keybindings = self.keybindings.clone();
            config.server.release_gpu_after = release_gpu_after;
            config.language = language_choice_code(self.language_index).to_string();
            match config.save() {
                Ok(()) => info!("Settings saved"),
                Err(e) => warn!(error = %e, "Failed to save settings"),
            }
        }
        changed
    }
}

/// Opções do seletor de idioma: automático e os idiomas.
const LANGUAGE_CHOICES: usize = 1 + crate::ui::i18n::Language::ALL.len();

fn language_choice_index(config_value: &str) -> usize {
    match config_value.trim() {
        "" | "auto" => 0,
        code => crate::ui::i18n::Language::from_code(code)
            .and_then(|lang| {
                crate::ui::i18n::Language::ALL
                    .iter()
                    .position(|&l| l == lang)
            })
            .map_or(0, |i| i + 1),
    }
}

fn language_choice_code(index: usize) -> &'static str {
    index
        .checked_sub(1)
        .and_then(|i| crate::ui::i18n::Language::ALL.get(i))
        .map_or("auto", |lang| lang.code())
}

fn language_choice_label(index: usize) -> &'static str {
    index
        .checked_sub(1)
        .and_then(|i| crate::ui::i18n::Language::ALL.get(i))
        .map_or_else(|| tr(Text::LanguageAuto), |lang| lang.native_name())
}

/// Atalho no formato do config a partir dos modificadores e do nome da
/// tecla (o de `physical_key_to_string`). Só modificador, ou tecla sem
/// nome, não vale.
pub fn binding_from_key(ctrl: bool, shift: bool, alt: bool, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if ctrl {
        parts.push("ctrl");
    }
    if shift {
        parts.push("shift");
    }
    if alt {
        parts.push("alt");
    }
    parts.push(key);
    let binding = parts.join("+");
    Shortcut::parse(&binding).map(|_| binding)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> SettingsScreen {
        SettingsScreen::new(
            &Config::default(),
            vec![SshEntry {
                name: "prod".into(),
                target: "admin@10.0.0.1".into(),
                editable: None,
            }],
            Some("40 MB".into()),
        )
    }

    #[test]
    fn test_next_theme_cycles_and_recovers() {
        assert_eq!(next_theme("dark"), "light");
        assert_eq!(next_theme("SOLARIZED-LIGHT"), "catppuccin-mocha");
        let last = THEMES[THEMES.len() - 1];
        assert_eq!(next_theme(&last.to_uppercase()), "dark");
        assert_eq!(next_theme("tema-que-nao-existe"), "dark");
    }

    #[test]
    fn test_cycle() {
        assert_eq!(cycle(0, 3, false), 2);
        assert_eq!(cycle(2, 3, true), 0);
        assert_eq!(cycle(5, 0, true), 0);
    }

    #[test]
    fn test_sections_and_rows() {
        let mut s = screen();
        assert_eq!(s.section(), SettingsSection::Appearance);
        assert_eq!(s.selected_field(), Some(SettingsField::Theme));
        s.key(SettingsInputKey::NextSection);
        assert_eq!(s.section(), SettingsSection::Tabs);
        assert_eq!(s.selected(), 0);
        s.key(SettingsInputKey::PreviousSection);
        s.key(SettingsInputKey::PreviousSection);
        assert_eq!(s.section(), SettingsSection::About);
        let about = s.rows();
        assert!(about.iter().any(|r| r.value == "40 MB"));
        assert!(about.iter().any(|r| r.value == env!("CARGO_PKG_VERSION")));
        s.select_section(2);
        assert_eq!(s.section(), SettingsSection::Ssh);
        let rows = s.rows();
        assert_eq!(rows.len(), 3);
        assert!(rows[0].label.contains("prod"));
        assert_eq!(rows[1].field, SettingsField::SshNew);
        assert_eq!(rows[2].field, SettingsField::SshQuickConnect);
        s.select_section(99);
        assert_eq!(s.section(), SettingsSection::Ssh);
    }

    #[test]
    fn test_theme_change_asks_for_preview() {
        let mut s = screen();
        let before = s.theme_name();
        match s.key(SettingsInputKey::Right) {
            SettingsOutcome::PreviewTheme(name) => assert_ne!(name, before),
            other => panic!("{other:?}"),
        }
        // Clique na linha do tema também troca.
        assert!(matches!(s.activate(0), SettingsOutcome::PreviewTheme(_)));
    }

    #[test]
    fn test_enter_saves_except_on_action_rows() {
        let mut s = screen();
        assert_eq!(s.key(SettingsInputKey::Enter), SettingsOutcome::Save);
        assert_eq!(s.key(SettingsInputKey::Escape), SettingsOutcome::Cancel);
        s.select_section(2);
        assert_eq!(
            s.key(SettingsInputKey::Enter),
            SettingsOutcome::ConnectSsh("prod".into())
        );
        assert_eq!(s.activate(2), SettingsOutcome::OpenQuickConnect);
        s.select_section(1);
        assert_eq!(s.activate(4), SettingsOutcome::NewPersistentTab);
        // Clique num liga/desliga troca o valor.
        let before = s.rows()[0].value.clone();
        assert_eq!(s.activate(0), SettingsOutcome::None);
        assert_ne!(s.rows()[0].value, before);
        assert_eq!(s.activate(42), SettingsOutcome::None);
    }

    #[test]
    fn test_no_ssh_hosts_offers_new_connection() {
        let mut s = SettingsScreen::new(&Config::default(), Vec::new(), None);
        s.select_section(2);
        let rows = s.rows();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].kind, RowKind::Info);
        assert_eq!(rows[1].field, SettingsField::SshNew);
        assert_eq!(s.activate(2), SettingsOutcome::OpenQuickConnect);
    }

    /// #32: opções de sidebar, links e IA vão para o config ao salvar.
    #[test]
    fn test_integrations_and_sidebar_options_are_saved() {
        let mut config = Config::default();
        config.sidebar.width = 230.0;
        let mut s = SettingsScreen::new(&config, Vec::new(), None);
        let position =
            |s: &SettingsScreen, field| s.rows().iter().position(|r| r.field == field).unwrap();
        s.select_section(1);
        let width = position(&s, SettingsField::SidebarWidth);
        assert_eq!(s.rows()[width].value, "230 px");
        s.activate(width);
        assert_eq!(s.rows()[width].value, "260 px");
        s.activate(position(&s, SettingsField::SidebarMemory));
        let integrations = SECTIONS
            .iter()
            .position(|&x| x == SettingsSection::Integrations)
            .unwrap();
        s.select_section(integrations);
        s.activate(position(&s, SettingsField::LinkTarget));
        s.activate(position(&s, SettingsField::AiEnabled));
        let provider = position(&s, SettingsField::AiProvider);
        let before = s.rows()[provider].value.clone();
        s.select(provider);
        s.key(SettingsInputKey::Right);
        assert_ne!(s.rows()[provider].value, before);
        // Campo de texto: digitar edita, ←/→ movem o cursor (não trocam valor).
        let model = position(&s, SettingsField::AiModel);
        s.select(model);
        assert_eq!(s.rows()[model].kind, RowKind::Text);
        for _ in 0..config.ai.model.name.chars().count() {
            s.key(SettingsInputKey::Backspace);
        }
        for c in "llama3".chars() {
            s.key(SettingsInputKey::Char(c));
        }
        s.key(SettingsInputKey::Left);
        assert_eq!(s.text_cursor(), Some((model, 5)));
        let mut saved = config.clone();
        assert!(s.save(&mut saved));
        assert_eq!(saved.sidebar.width, 260.0);
        assert_ne!(saved.sidebar.show_memory, config.sidebar.show_memory);
        assert_ne!(saved.browser.open_links_in, config.browser.open_links_in);
        assert_ne!(saved.ai.enabled, config.ai.enabled);
        assert_ne!(saved.ai.provider, config.ai.provider);
        assert_eq!(saved.ai.model.name, "llama3");
        // Largura fora da lista fica como está até escolher outra.
        config.sidebar.width = 245.0;
        let s = SettingsScreen::new(&config, Vec::new(), None);
        let mut kept = config.clone();
        assert!(!s.save(&mut kept));
        assert_eq!(kept.sidebar.width, 245.0);
    }

    /// #32: criar e editar host pelo formulário; host de arquivo incluído
    /// (não editável) só conecta.
    #[test]
    fn test_ssh_form_create_edit_and_back() {
        use crate::ssh::config_edit::HostEntry;
        let prod = HostEntry {
            alias: "prod".into(),
            host_name: Some("10.0.0.1".into()),
            user: Some("admin".into()),
            port: None,
            identity_file: None,
        };
        let mut s = SettingsScreen::new(
            &Config::default(),
            vec![
                SshEntry {
                    name: "prod".into(),
                    target: "admin@10.0.0.1".into(),
                    editable: Some(prod.clone()),
                },
                SshEntry {
                    name: "incluido".into(),
                    target: "x".into(),
                    editable: None,
                },
            ],
            None,
        );
        s.select_section(2);
        // Host não editável conecta direto.
        assert_eq!(
            s.activate(1),
            SettingsOutcome::ConnectSsh("incluido".into())
        );
        // Host editável abre o formulário com os valores.
        assert_eq!(s.activate(0), SettingsOutcome::None);
        assert!(s.is_editing_ssh_host());
        let rows = s.rows();
        assert_eq!(rows[0].value, "prod");
        assert_eq!(rows[1].value, "10.0.0.1");
        assert_eq!(rows[0].kind, RowKind::Text);
        assert_eq!(s.text_cursor(), Some((0, 4)));
        // Digitar edita o campo selecionado; Enter vai para o próximo.
        s.key(SettingsInputKey::Enter);
        s.key(SettingsInputKey::Enter);
        for c in "-ops".chars() {
            s.key(SettingsInputKey::Char(c));
        }
        assert_eq!(s.rows()[2].value, "admin-ops");
        s.key(SettingsInputKey::Backspace);
        assert_eq!(s.rows()[2].value, "admin-op");
        // Salvar devolve o pedido para o app gravar.
        let save = s
            .rows()
            .iter()
            .position(|r| r.field == SettingsField::SshFormSave)
            .unwrap();
        match s.activate(save) {
            SettingsOutcome::SaveSshHost { original, entry } => {
                assert_eq!(original.as_deref(), Some("prod"));
                assert_eq!(entry.user.as_deref(), Some("admin-op"));
            }
            other => panic!("{other:?}"),
        }
        // Erro do app aparece no formulário.
        s.set_ssh_error("ruim".into());
        assert!(s.rows().iter().any(|r| r.label == "ruim"));
        // Conectar a partir do formulário.
        let connect = s
            .rows()
            .iter()
            .position(|r| r.field == SettingsField::SshFormConnect)
            .unwrap();
        assert_eq!(
            s.activate(connect),
            SettingsOutcome::ConnectSsh("prod".into())
        );
        // Esc volta à lista.
        s.key(SettingsInputKey::Escape);
        assert!(!s.is_editing_ssh_host());
        // Host novo: porta inválida não sai do formulário.
        let new = s
            .rows()
            .iter()
            .position(|r| r.field == SettingsField::SshNew)
            .unwrap();
        s.activate(new);
        assert!(s
            .rows()
            .iter()
            .all(|r| r.field != SettingsField::SshFormConnect));
        for c in "novo".chars() {
            s.key(SettingsInputKey::Char(c));
        }
        s.select(3);
        s.key(SettingsInputKey::Char('x'));
        let save = s
            .rows()
            .iter()
            .position(|r| r.field == SettingsField::SshFormSave)
            .unwrap();
        assert_eq!(s.activate(save), SettingsOutcome::None);
        assert!(s
            .rows()
            .iter()
            .any(|r| r.field == SettingsField::SshFormError));
        // Gravado: o formulário fecha e a lista nova aparece.
        s.ssh_saved(vec![]);
        assert!(!s.is_editing_ssh_host());
    }

    #[test]
    fn test_shortcut_filter_and_capture() {
        let mut s = screen();
        s.select_section(4);
        let all = s.rows().len();
        assert_eq!(all, EDITABLE_SHORTCUTS.len());
        for c in "pane".chars() {
            s.key(SettingsInputKey::Char(c));
        }
        let filtered = s.rows();
        assert!(filtered.len() < all && !filtered.is_empty());
        assert!(filtered
            .iter()
            .all(|r| r.label.to_lowercase().contains("pane")));
        // Enter numa linha espera a tecla; a tecla nova entra na lista.
        let SettingsField::Shortcut(id) = filtered[0].field else {
            panic!("esperava atalho");
        };
        s.key(SettingsInputKey::Enter);
        assert!(s.is_capturing());
        assert_eq!(s.rows()[0].value, "\u{2026}");
        // Enquanto espera, setas e Enter não fazem nada.
        assert_eq!(s.key(SettingsInputKey::Enter), SettingsOutcome::None);
        s.key(SettingsInputKey::Captured("ctrl+alt+p".into()));
        assert!(!s.is_capturing());
        assert_eq!(s.keybindings().get(id), Some("ctrl+alt+p"));
        // Esc limpa a busca antes de cancelar a tela.
        assert_eq!(s.key(SettingsInputKey::Escape), SettingsOutcome::None);
        assert_eq!(s.rows().len(), all);
        assert_eq!(s.key(SettingsInputKey::Escape), SettingsOutcome::Cancel);
    }

    /// Os campos mudam o config ao salvar; a fonte desconhecida é mantida.
    #[test]
    fn test_save_applies_choices_and_keeps_unknown_font() {
        let mut config = Config::default();
        config.font.family = "Iosevka Term".into();
        let mut s = SettingsScreen::new(&config, Vec::new(), None);
        assert!(s.rows()[1].value.contains("Iosevka Term"));
        s.select(3);
        s.key(SettingsInputKey::Right); // cursor: block -> underline
        s.select_section(1);
        s.activate(0); // sidebar
        s.activate(1); // restaurar
        s.activate(2); // confirmar
        s.activate(3); // abas novas persistentes (#21)
        s.select_section(4);
        s.activate(0);
        s.key(SettingsInputKey::Captured("ctrl+alt+t".into()));
        let mut applied = config.clone();
        assert!(s.save(&mut applied));
        assert_eq!(applied.terminal.cursor_style, "underline");
        assert_eq!(applied.sidebar.visible, !config.sidebar.visible);
        assert_eq!(applied.session.restore, !config.session.restore);
        assert_eq!(
            applied.window.confirm_close_tab,
            !config.window.confirm_close_tab
        );
        assert_eq!(applied.font.family, "Iosevka Term");
        assert_eq!(
            applied.session.persistent_tabs,
            !config.session.persistent_tabs
        );
        assert_eq!(applied.keybindings.new_tab, "ctrl+alt+t");
    }

    #[test]
    fn test_save_without_changes_reports_nothing() {
        let mut config = Config::default();
        config.colors.name = "dark".into();
        config.font.size = 14.0;
        config.font.family = "Cascadia Mono".into();
        let s = SettingsScreen::new(&config, Vec::new(), None);
        assert!(!s.save(&mut config));
    }

    #[test]
    fn test_about_shows_creator_publisher_and_license() {
        let mut settings = screen();
        let about = SECTIONS
            .iter()
            .position(|section| *section == SettingsSection::About)
            .unwrap();
        settings.select_section(about);

        let rows = settings.rows();
        for (field, value) in [
            (SettingsField::Creator, "Diogo Costa Santos"),
            (SettingsField::Publisher, "Código Central"),
            (SettingsField::License, env!("CARGO_PKG_LICENSE")),
        ] {
            let row = rows.iter().find(|row| row.field == field).unwrap();
            assert_eq!(row.value, value);
            assert_eq!(row.kind, RowKind::Info);
        }
    }

    /// Prazo de soltar a GPU no Sobre: mostra o do config, troca e salva (#1).
    #[test]
    fn test_release_gpu_choice_shows_cycles_and_saves() {
        let mut s = screen();
        s.select_section(
            SECTIONS
                .iter()
                .position(|&x| x == SettingsSection::About)
                .unwrap(),
        );
        let row = s
            .rows()
            .iter()
            .position(|r| r.field == SettingsField::ReleaseGpuAfter)
            .unwrap();
        assert_eq!(s.rows()[row].kind, RowKind::Choice);
        assert_eq!(s.rows()[row].value, "5 min");
        s.select(row);
        s.key(SettingsInputKey::Right);
        assert_eq!(s.rows()[row].value, "15 min");
        s.key(SettingsInputKey::Right);
        assert_eq!(s.rows()[row].value, tr(Text::Never));
        let mut config = Config::default();
        assert!(s.save(&mut config));
        assert_eq!(config.server.release_gpu_after, "0");
        assert_eq!(config.server.release_gpu_after(), None);
    }

    /// Prazo do config fora da lista continua lá e não muda ao salvar.
    #[test]
    fn test_release_gpu_keeps_a_custom_value() {
        let mut config = Config::default();
        config.server.release_gpu_after = "2m".into();
        let s = SettingsScreen::new(&config, Vec::new(), None);
        assert_eq!(s.release_gpu_choices[s.release_gpu_index], "2m");
        assert_eq!(release_gpu_label("2m"), "2 min");
        assert_eq!(release_gpu_label("30s"), "30 s");
        assert_eq!(release_gpu_label("1h"), "1 h");
        let mut same = config.clone();
        s.save(&mut same);
        assert_eq!(same.server.release_gpu_after, "2m");
    }

    /// Seletor de animação na Aparência: troca, pede prévia e salva (#26).
    #[test]
    fn test_animation_choice_previews_and_saves() {
        let mut s = screen();
        let row = s
            .rows()
            .iter()
            .position(|r| r.field == SettingsField::TabAnimation)
            .unwrap();
        assert_eq!(
            s.preview_age(),
            crate::ui::glow::GLOW_DURATION,
            "parada até mexer"
        );
        s.select(row);
        assert_eq!(
            s.key(SettingsInputKey::Right),
            SettingsOutcome::PreviewAnimation
        );
        assert_eq!(s.rows()[row].value, TabAnimation::Pulse.label());
        assert!(s.preview_age() < crate::ui::glow::GLOW_DURATION);
        assert_eq!(s.preview_motion(true, false).style, TabAnimation::Pulse);
        assert_eq!(
            s.preview_motion(true, true).style,
            TabAnimation::None,
            "segue o sistema"
        );
        s.select(row + 1);
        assert_eq!(s.activate(row + 1), SettingsOutcome::PreviewAnimation);
        s.activate(row + 3);
        let mut config = Config::default();
        assert!(s.save(&mut config));
        assert_eq!(config.window.tab_animation, TabAnimation::Pulse);
        assert_eq!(config.window.animation_speed, AnimationSpeed::Fast);
        assert!(config.window.replace_program_spinner);
    }

    /// "Abrir maximizada" fica na Aparência, liga por padrão e salva.
    #[test]
    fn test_start_maximized_toggle_saves() {
        let mut s = screen();
        let row = s
            .rows()
            .iter()
            .position(|r| r.field == SettingsField::StartMaximized)
            .unwrap();
        assert_eq!(s.rows()[row].kind, RowKind::Toggle);
        let on = s.rows()[row].value.clone();
        s.activate(row);
        assert_ne!(s.rows()[row].value, on);
        let mut config = Config::default();
        assert!(config.window.start_maximized, "padrão: maximizada");
        assert!(s.save(&mut config));
        assert!(!config.window.start_maximized);
    }

    /// Idioma: seção própria, troca na hora (prévia) e salva no config (#28).
    #[test]
    fn test_language_choice_previews_and_saves() {
        use crate::ui::i18n::Language;
        let mut s = screen();
        let section = SECTIONS
            .iter()
            .position(|&sec| sec == SettingsSection::Language)
            .unwrap();
        s.select_section(section);
        assert_eq!(s.rows().len(), 1);
        assert_eq!(
            s.key(SettingsInputKey::Right),
            SettingsOutcome::PreviewLanguage(Language::English)
        );
        assert_eq!(
            s.key(SettingsInputKey::Right),
            SettingsOutcome::PreviewLanguage(Language::PortugueseBr)
        );
        assert_eq!(s.rows()[0].value, Language::PortugueseBr.native_name());
        let mut config = Config::default();
        assert!(s.save(&mut config));
        assert_eq!(config.language, "pt-BR");
        assert_eq!(language_choice_index("pt-BR"), 2);
        assert_eq!(language_choice_index("auto"), 0);
        assert_eq!(language_choice_code(0), "auto");
    }

    #[test]
    fn test_binding_from_key() {
        assert_eq!(
            binding_from_key(true, true, false, "t").as_deref(),
            Some("ctrl+shift+t")
        );
        assert_eq!(
            binding_from_key(false, false, false, "f5").as_deref(),
            Some("f5")
        );
        assert_eq!(binding_from_key(true, false, false, ""), None);
    }

    #[test]
    fn test_scroll_by_stops_at_the_ends() {
        let mut s = screen();
        s.select_section(4);
        s.scroll_by(-5);
        assert_eq!(s.selected(), 0);
        s.scroll_by(1000);
        assert_eq!(s.selected(), EDITABLE_SHORTCUTS.len() - 1);
    }
}
