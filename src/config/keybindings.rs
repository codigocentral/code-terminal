use serde::{Deserialize, Serialize};

/// Ações disponíveis para keybindings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    // Tab actions
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    GoToTab(usize),

    // Split actions
    SplitHorizontal,
    SplitVertical,
    ClosePane,
    NavigateUp,
    NavigateDown,
    NavigateLeft,
    NavigateRight,
    NextPane,
    PreviousPane,
    IncreasePaneSize,
    DecreasePaneSize,

    // Clipboard
    Copy,
    Paste,

    // Search
    SearchOpen,
    SearchClose,
    SearchNext,
    SearchPrevious,

    // Misc
    Quit,
    ToggleSidebar,
    ToggleFullscreen,
}

/// Configurações de atalhos de teclado para splits nativos (fora do modo multiplexer)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitKeyBindings {
    /// Split horizontal (padrão: Ctrl+Shift+E)
    #[serde(default = "default_split_horizontal")]
    pub split_horizontal: String,

    /// Split vertical (padrão: Ctrl+Shift+O)
    #[serde(default = "default_split_vertical")]
    pub split_vertical: String,

    /// Fechar pane (padrão: Ctrl+Shift+W)
    #[serde(default = "default_split_close_pane")]
    pub close_pane: String,

    /// Navegar para cima (padrão: Alt+Up)
    #[serde(default = "default_split_nav_up")]
    pub nav_up: String,

    /// Navegar para baixo (padrão: Alt+Down)
    #[serde(default = "default_split_nav_down")]
    pub nav_down: String,

    /// Navegar para esquerda (padrão: Alt+Left)
    #[serde(default = "default_split_nav_left")]
    pub nav_left: String,

    /// Navegar para direita (padrão: Alt+Right)
    #[serde(default = "default_split_nav_right")]
    pub nav_right: String,

    /// Próximo pane (padrão: Ctrl+Tab)
    #[serde(default = "default_split_next_pane")]
    pub next_pane: String,

    /// Pane anterior (padrão: Ctrl+Shift+Tab)
    #[serde(default = "default_split_prev_pane")]
    pub prev_pane: String,

    /// Aumentar tamanho (padrão: Ctrl+Shift++)
    #[serde(default = "default_split_increase")]
    pub increase_size: String,

    /// Diminuir tamanho (padrão: Ctrl+Shift+-)
    #[serde(default = "default_split_decrease")]
    pub decrease_size: String,
}

impl Default for SplitKeyBindings {
    fn default() -> Self {
        Self {
            split_horizontal: default_split_horizontal(),
            split_vertical: default_split_vertical(),
            close_pane: default_split_close_pane(),
            nav_up: default_split_nav_up(),
            nav_down: default_split_nav_down(),
            nav_left: default_split_nav_left(),
            nav_right: default_split_nav_right(),
            next_pane: default_split_next_pane(),
            prev_pane: default_split_prev_pane(),
            increase_size: default_split_increase(),
            decrease_size: default_split_decrease(),
        }
    }
}

fn default_split_horizontal() -> String {
    "ctrl+shift+e".to_string()
}

fn default_split_vertical() -> String {
    "ctrl+shift+o".to_string()
}

fn default_split_close_pane() -> String {
    "ctrl+shift+x".to_string()
}

fn default_split_nav_up() -> String {
    "ctrl+shift+up".to_string()
}

fn default_split_nav_down() -> String {
    "ctrl+shift+down".to_string()
}

fn default_split_nav_left() -> String {
    "ctrl+shift+left".to_string()
}

fn default_split_nav_right() -> String {
    "ctrl+shift+right".to_string()
}

fn default_split_next_pane() -> String {
    "ctrl+shift+bracketright".to_string()
}

fn default_split_prev_pane() -> String {
    "ctrl+shift+bracketleft".to_string()
}

fn default_split_increase() -> String {
    "ctrl+shift+equal".to_string()
}

fn default_split_decrease() -> String {
    "ctrl+shift+minus".to_string()
}

/// Gerenciador de atalhos para splits nativos
#[derive(Debug)]
pub struct NativeSplitShortcuts {
    pub split_horizontal: Shortcut,
    pub split_vertical: Shortcut,
    pub close_pane: Shortcut,
    pub nav_up: Shortcut,
    pub nav_down: Shortcut,
    pub nav_left: Shortcut,
    pub nav_right: Shortcut,
    pub next_pane: Shortcut,
    pub prev_pane: Shortcut,
    pub increase_size: Shortcut,
    pub decrease_size: Shortcut,
}

impl NativeSplitShortcuts {
    pub fn from_config(config: &SplitKeyBindings) -> Self {
        Self {
            split_horizontal: Shortcut::parse(&config.split_horizontal).unwrap_or_else(|| {
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: "e".to_string(),
                }
            }),
            split_vertical: Shortcut::parse(&config.split_vertical).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: true,
                alt: false,
                key: "o".to_string(),
            }),
            close_pane: Shortcut::parse(&config.close_pane).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: true,
                alt: false,
                key: "w".to_string(),
            }),
            nav_up: Shortcut::parse(&config.nav_up).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: true,
                key: "up".to_string(),
            }),
            nav_down: Shortcut::parse(&config.nav_down).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: true,
                key: "down".to_string(),
            }),
            nav_left: Shortcut::parse(&config.nav_left).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: true,
                key: "left".to_string(),
            }),
            nav_right: Shortcut::parse(&config.nav_right).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: true,
                key: "right".to_string(),
            }),
            next_pane: Shortcut::parse(&config.next_pane).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: false,
                alt: false,
                key: "tab".to_string(),
            }),
            prev_pane: Shortcut::parse(&config.prev_pane).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: true,
                alt: false,
                key: "tab".to_string(),
            }),
            increase_size: Shortcut::parse(&config.increase_size).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: true,
                alt: false,
                key: "plus".to_string(),
            }),
            decrease_size: Shortcut::parse(&config.decrease_size).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: true,
                alt: false,
                key: "minus".to_string(),
            }),
        }
    }

    /// Verifica se é uma navegação de pane e retorna a direção
    pub fn is_pane_navigation(
        &self,
        ctrl: bool,
        shift: bool,
        alt: bool,
        key: &str,
    ) -> Option<&str> {
        if self.nav_up.matches(ctrl, shift, alt, key) {
            Some("up")
        } else if self.nav_down.matches(ctrl, shift, alt, key) {
            Some("down")
        } else if self.nav_left.matches(ctrl, shift, alt, key) {
            Some("left")
        } else if self.nav_right.matches(ctrl, shift, alt, key) {
            Some("right")
        } else {
            None
        }
    }

    /// Tenta converter input em uma Action
    pub fn try_get_action(&self, ctrl: bool, shift: bool, alt: bool, key: &str) -> Option<Action> {
        if self.split_horizontal.matches(ctrl, shift, alt, key) {
            Some(Action::SplitHorizontal)
        } else if self.split_vertical.matches(ctrl, shift, alt, key) {
            Some(Action::SplitVertical)
        } else if self.close_pane.matches(ctrl, shift, alt, key) {
            Some(Action::ClosePane)
        } else if self.nav_up.matches(ctrl, shift, alt, key) {
            Some(Action::NavigateUp)
        } else if self.nav_down.matches(ctrl, shift, alt, key) {
            Some(Action::NavigateDown)
        } else if self.nav_left.matches(ctrl, shift, alt, key) {
            Some(Action::NavigateLeft)
        } else if self.nav_right.matches(ctrl, shift, alt, key) {
            Some(Action::NavigateRight)
        } else if self.next_pane.matches(ctrl, shift, alt, key) {
            Some(Action::NextPane)
        } else if self.prev_pane.matches(ctrl, shift, alt, key) {
            Some(Action::PreviousPane)
        } else if self.increase_size.matches(ctrl, shift, alt, key) {
            Some(Action::IncreasePaneSize)
        } else if self.decrease_size.matches(ctrl, shift, alt, key) {
            Some(Action::DecreasePaneSize)
        } else {
            None
        }
    }
}

impl Default for NativeSplitShortcuts {
    fn default() -> Self {
        Self::from_config(&SplitKeyBindings::default())
    }
}

/// Configurações de atalhos de teclado
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeyBindings {
    /// Nova tab (padrão: Ctrl+Shift+T)
    #[serde(default = "default_new_tab")]
    pub new_tab: String,

    /// Fechar tab (padrão: Ctrl+Shift+W)
    #[serde(default = "default_close_tab")]
    pub close_tab: String,

    /// Próxima tab (padrão: Ctrl+Tab)
    #[serde(default = "default_next_tab")]
    pub next_tab: String,

    /// Tab anterior (padrão: Ctrl+Shift+Tab)
    #[serde(default = "default_previous_tab")]
    pub previous_tab: String,

    /// Ir para tab específica (padrão: Ctrl+1-9)
    #[serde(default = "default_go_to_tab")]
    pub go_to_tab_prefix: String,

    /// Copiar a seleção para o clipboard (padrão: Ctrl+Shift+C)
    #[serde(default = "default_copy")]
    pub copy: String,

    /// Colar do clipboard (padrão: Ctrl+Shift+V)
    #[serde(default = "default_paste")]
    pub paste: String,

    /// Abrir busca (padrão: Ctrl+Shift+F)
    #[serde(default = "default_search_open")]
    pub search_open: String,

    /// Fechar busca (padrão: Escape)
    #[serde(default = "default_search_close")]
    pub search_close: String,

    /// Próximo match (padrão: F3)
    #[serde(default = "default_search_next")]
    pub search_next: String,

    /// Match anterior (padrão: Shift+F3)
    #[serde(default = "default_search_previous")]
    pub search_previous: String,

    /// Atalhos de painéis (splits).
    #[serde(default)]
    pub splits: SplitKeyBindings,

    /// Mostrar/esconder a sidebar de abas verticais (padrão: Ctrl+Shift+B)
    #[serde(default = "default_toggle_sidebar")]
    pub toggle_sidebar: String,

    /// Ir para o painel com o aviso não lido mais recente (padrão: Ctrl+Shift+U)
    #[serde(default = "default_jump_to_unread")]
    pub jump_to_unread: String,

    /// Mostrar o histórico de avisos (padrão: Ctrl+Shift+I)
    #[serde(default = "default_show_notifications")]
    pub show_notifications: String,

    /// Inspetor de protocolo (padrão: Ctrl+Shift+D)
    #[serde(default = "default_toggle_inspector")]
    pub toggle_inspector: String,

    /// IA: explicar a saída do painel (padrão: Ctrl+Shift+A)
    #[serde(default = "default_ai_explain")]
    pub ai_explain: String,

    /// IA: sugerir um comando (padrão: Ctrl+Shift+K)
    #[serde(default = "default_ai_suggest")]
    pub ai_suggest: String,

    /// Renomear a tab ativa na sidebar (padrão: F2, só com a sidebar à vista)
    #[serde(default = "default_rename_tab")]
    pub rename_tab: String,

    /// Abrir as configurações (padrão: Ctrl+,)
    #[serde(default = "default_open_settings")]
    pub open_settings: String,
}

fn default_rename_tab() -> String {
    "f2".to_string()
}

fn default_open_settings() -> String {
    "ctrl+comma".to_string()
}

fn default_ai_explain() -> String {
    "ctrl+shift+a".to_string()
}

fn default_ai_suggest() -> String {
    "ctrl+shift+k".to_string()
}

fn default_toggle_inspector() -> String {
    "ctrl+shift+d".to_string()
}

fn default_toggle_sidebar() -> String {
    "ctrl+shift+b".to_string()
}

fn default_jump_to_unread() -> String {
    "ctrl+shift+u".to_string()
}

fn default_show_notifications() -> String {
    "ctrl+shift+i".to_string()
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            new_tab: default_new_tab(),
            close_tab: default_close_tab(),
            next_tab: default_next_tab(),
            previous_tab: default_previous_tab(),
            go_to_tab_prefix: default_go_to_tab(),
            copy: default_copy(),
            paste: default_paste(),
            search_open: default_search_open(),
            search_close: default_search_close(),
            search_next: default_search_next(),
            search_previous: default_search_previous(),
            splits: SplitKeyBindings::default(),
            toggle_sidebar: default_toggle_sidebar(),
            jump_to_unread: default_jump_to_unread(),
            show_notifications: default_show_notifications(),
            toggle_inspector: default_toggle_inspector(),
            ai_explain: default_ai_explain(),
            ai_suggest: default_ai_suggest(),
            rename_tab: default_rename_tab(),
            open_settings: default_open_settings(),
        }
    }
}

/// Id de cada atalho editável e o campo dele (uma lista só para ler e trocar).
macro_rules! editable_fields {
    ($m:ident) => {
        $m! {
            "new_tab" => new_tab,
            "close_tab" => close_tab,
            "next_tab" => next_tab,
            "previous_tab" => previous_tab,
            "rename_tab" => rename_tab,
            "toggle_sidebar" => toggle_sidebar,
            "open_settings" => open_settings,
            "copy" => copy,
            "paste" => paste,
            "search_open" => search_open,
            "search_next" => search_next,
            "search_previous" => search_previous,
            "jump_to_unread" => jump_to_unread,
            "show_notifications" => show_notifications,
            "toggle_inspector" => toggle_inspector,
            "ai_explain" => ai_explain,
            "ai_suggest" => ai_suggest,
            "splits.split_horizontal" => splits.split_horizontal,
            "splits.split_vertical" => splits.split_vertical,
            "splits.close_pane" => splits.close_pane,
            "splits.nav_up" => splits.nav_up,
            "splits.nav_down" => splits.nav_down,
            "splits.nav_left" => splits.nav_left,
            "splits.nav_right" => splits.nav_right,
            "splits.next_pane" => splits.next_pane,
            "splits.prev_pane" => splits.prev_pane,
            "splits.increase_size" => splits.increase_size,
            "splits.decrease_size" => splits.decrease_size,
        }
    };
}

/// Atalhos que a tela de configurações lista e deixa trocar, na ordem da
/// lista. O id é o nome do campo no `config.toml` (os de painel com o
/// prefixo `splits.`); o nome visível vem de `ui::i18n::shortcut_name`.
pub const EDITABLE_SHORTCUTS: &[&str] = &[
    "new_tab",
    "close_tab",
    "next_tab",
    "previous_tab",
    "rename_tab",
    "toggle_sidebar",
    "open_settings",
    "copy",
    "paste",
    "search_open",
    "search_next",
    "search_previous",
    "jump_to_unread",
    "show_notifications",
    "toggle_inspector",
    "ai_explain",
    "ai_suggest",
    "splits.split_horizontal",
    "splits.split_vertical",
    "splits.close_pane",
    "splits.nav_up",
    "splits.nav_down",
    "splits.nav_left",
    "splits.nav_right",
    "splits.next_pane",
    "splits.prev_pane",
    "splits.increase_size",
    "splits.decrease_size",
];

impl KeyBindings {
    /// Atalho de um item de [`EDITABLE_SHORTCUTS`].
    pub fn get(&self, id: &str) -> Option<&str> {
        macro_rules! pick {
            ($($id:literal => $($field:ident).+),* $(,)?) => {
                match id {
                    $($id => Some(self.$($field).+.as_str()),)*
                    _ => None,
                }
            };
        }
        editable_fields!(pick)
    }

    fn field_mut(&mut self, id: &str) -> Option<&mut String> {
        macro_rules! pick {
            ($($id:literal => $($field:ident).+),* $(,)?) => {
                match id {
                    $($id => Some(&mut self.$($field).+),)*
                    _ => None,
                }
            };
        }
        editable_fields!(pick)
    }

    /// Troca o atalho `id`. Devolve `false` se o id não existe ou o valor
    /// não é um atalho válido.
    pub fn set(&mut self, id: &str, value: &str) -> bool {
        if Shortcut::parse(value).is_none_or(|shortcut| shortcut.key.is_empty()) {
            return false;
        }
        match self.field_mut(id) {
            Some(field) => {
                *field = value.to_string();
                true
            }
            None => false,
        }
    }

    /// Troca os atalhos padrão antigos que roubavam teclas do shell.
    ///
    /// Ctrl+C, Ctrl+V, Ctrl+F e Ctrl+W sem Shift são teclas de controle
    /// (SIGINT, colar literal no readline, avançar caractere, apagar palavra);
    /// com eles o terminal nunca as entregava ao programa. Só mexe em valores
    /// iguais aos padrões antigos, para respeitar o que o usuário escolheu.
    /// Retorna `true` se algo mudou.
    pub fn upgrade_legacy_defaults(&mut self) -> bool {
        let mut changed = false;
        let mut upgrade = |value: &mut String, legacy: &str, current: String| {
            if value.eq_ignore_ascii_case(legacy) {
                *value = current;
                changed = true;
            }
        };
        upgrade(&mut self.copy, "ctrl+c", default_copy());
        upgrade(&mut self.paste, "ctrl+v", default_paste());
        upgrade(&mut self.search_open, "ctrl+f", default_search_open());
        upgrade(&mut self.close_tab, "ctrl+w", default_close_tab());
        changed
    }

    /// Texto legível de um atalho ("ctrl+shift+t" -> "Ctrl+Shift+T").
    pub fn display(binding: &str) -> String {
        binding
            .split('+')
            .map(|part| {
                // Teclas de pontuação aparecem como o símbolo ("Ctrl+,").
                match part {
                    "comma" => return ",".to_string(),
                    "period" => return ".".to_string(),
                    _ => {}
                }
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) if part.len() == 1 => first.to_uppercase().collect(),
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("+")
    }
}

fn default_new_tab() -> String {
    "ctrl+shift+t".to_string()
}

fn default_close_tab() -> String {
    "ctrl+shift+w".to_string()
}

fn default_next_tab() -> String {
    "ctrl+tab".to_string()
}

fn default_previous_tab() -> String {
    "ctrl+shift+tab".to_string()
}

fn default_go_to_tab() -> String {
    "ctrl".to_string()
}

fn default_copy() -> String {
    "ctrl+shift+c".to_string()
}

fn default_paste() -> String {
    "ctrl+shift+v".to_string()
}

fn default_search_open() -> String {
    "ctrl+shift+f".to_string()
}

fn default_search_close() -> String {
    "escape".to_string()
}

fn default_search_next() -> String {
    "f3".to_string()
}

fn default_search_previous() -> String {
    "shift+f3".to_string()
}

// ==================== MULTIPLEXER KEYBINDINGS ====================

/// Configurações de atalhos de teclado para o modo Multiplexer (tmux-like)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiplexerKeyBindings {
    /// Tecla prefixo (padrão: Ctrl+B)
    #[serde(default = "default_mux_prefix")]
    pub prefix: String,

    /// Criar nova sessão (prefix + C)
    #[serde(default = "default_mux_new_session")]
    pub new_session: String,

    /// Listar sessões (prefix + S)
    #[serde(default = "default_mux_list_sessions")]
    pub list_sessions: String,

    /// Anexar a sessão (prefix + A)
    #[serde(default = "default_mux_attach_session")]
    pub attach_session: String,

    /// Desanexar da sessão (prefix + D)
    #[serde(default = "default_mux_detach")]
    pub detach: String,

    /// Matar sessão (prefix + X)
    #[serde(default = "default_mux_kill_session")]
    pub kill_session: String,

    /// Renomear sessão (prefix + $)
    #[serde(default = "default_mux_rename_session")]
    pub rename_session: String,

    /// Nova janela (prefix + C)
    #[serde(default = "default_mux_new_window")]
    pub new_window: String,

    /// Próxima janela (prefix + N)
    #[serde(default = "default_mux_next_window")]
    pub next_window: String,

    /// Janela anterior (prefix + P)
    #[serde(default = "default_mux_previous_window")]
    pub previous_window: String,

    /// Ir para janela por número (prefix + 0-9)
    #[serde(default = "default_mux_go_to_window")]
    pub go_to_window_prefix: String,

    /// Dividir horizontalmente (prefix + ")
    #[serde(default = "default_mux_split_horizontal")]
    pub split_horizontal: String,

    /// Dividir verticalmente (prefix + %)
    #[serde(default = "default_mux_split_vertical")]
    pub split_vertical: String,

    /// Navegar para pane acima (prefix + ↑)
    #[serde(default = "default_mux_pane_up")]
    pub pane_up: String,

    /// Navegar para pane abaixo (prefix + ↓)
    #[serde(default = "default_mux_pane_down")]
    pub pane_down: String,

    /// Navegar para pane esquerda (prefix + ←)
    #[serde(default = "default_mux_pane_left")]
    pub pane_left: String,

    /// Navegar para pane direita (prefix + →)
    #[serde(default = "default_mux_pane_right")]
    pub pane_right: String,

    /// Fechar pane atual (prefix + X)
    #[serde(default = "default_mux_close_pane")]
    pub close_pane: String,

    /// Alternar layout (prefix + Space)
    #[serde(default = "default_mux_next_layout")]
    pub next_layout: String,

    /// Zoom no pane (prefix + Z)
    #[serde(default = "default_mux_zoom_pane")]
    pub zoom_pane: String,

    /// Sincronizar panes (prefix + Y)
    #[serde(default = "default_mux_sync_panes")]
    pub sync_panes: String,

    /// Modo comando (prefix + :)
    #[serde(default = "default_mux_command_mode")]
    pub command_mode: String,

    /// Modo cópia (prefix + [)
    #[serde(default = "default_mux_copy_mode")]
    pub copy_mode: String,

    /// Colar buffer (prefix + ])
    #[serde(default = "default_mux_paste_buffer")]
    pub paste_buffer: String,

    /// Selecionar e copiar (Enter em modo cópia)
    #[serde(default = "default_mux_copy_selection")]
    pub copy_selection: String,

    /// Cancelar modo cópia (q em modo cópia)
    #[serde(default = "default_mux_cancel_copy")]
    pub cancel_copy: String,
}

impl Default for MultiplexerKeyBindings {
    fn default() -> Self {
        Self {
            prefix: default_mux_prefix(),
            new_session: default_mux_new_session(),
            list_sessions: default_mux_list_sessions(),
            attach_session: default_mux_attach_session(),
            detach: default_mux_detach(),
            kill_session: default_mux_kill_session(),
            rename_session: default_mux_rename_session(),
            new_window: default_mux_new_window(),
            next_window: default_mux_next_window(),
            previous_window: default_mux_previous_window(),
            go_to_window_prefix: default_mux_go_to_window(),
            split_horizontal: default_mux_split_horizontal(),
            split_vertical: default_mux_split_vertical(),
            pane_up: default_mux_pane_up(),
            pane_down: default_mux_pane_down(),
            pane_left: default_mux_pane_left(),
            pane_right: default_mux_pane_right(),
            close_pane: default_mux_close_pane(),
            next_layout: default_mux_next_layout(),
            zoom_pane: default_mux_zoom_pane(),
            sync_panes: default_mux_sync_panes(),
            command_mode: default_mux_command_mode(),
            copy_mode: default_mux_copy_mode(),
            paste_buffer: default_mux_paste_buffer(),
            copy_selection: default_mux_copy_selection(),
            cancel_copy: default_mux_cancel_copy(),
        }
    }
}

// Default functions for multiplexer keybindings
fn default_mux_prefix() -> String {
    "ctrl+b".to_string()
}

fn default_mux_new_session() -> String {
    ":new".to_string()
}

fn default_mux_list_sessions() -> String {
    "s".to_string()
}

fn default_mux_attach_session() -> String {
    "a".to_string()
}

fn default_mux_detach() -> String {
    "d".to_string()
}

fn default_mux_kill_session() -> String {
    "x".to_string()
}

fn default_mux_rename_session() -> String {
    "$".to_string()
}

fn default_mux_new_window() -> String {
    "c".to_string()
}

fn default_mux_next_window() -> String {
    "n".to_string()
}

fn default_mux_previous_window() -> String {
    "p".to_string()
}

fn default_mux_go_to_window() -> String {
    "".to_string() // Uses prefix + number directly
}

fn default_mux_split_horizontal() -> String {
    "\"".to_string()
}

fn default_mux_split_vertical() -> String {
    "%".to_string()
}

fn default_mux_pane_up() -> String {
    "up".to_string()
}

fn default_mux_pane_down() -> String {
    "down".to_string()
}

fn default_mux_pane_left() -> String {
    "left".to_string()
}

fn default_mux_pane_right() -> String {
    "right".to_string()
}

fn default_mux_close_pane() -> String {
    "x".to_string()
}

fn default_mux_next_layout() -> String {
    "space".to_string()
}

fn default_mux_zoom_pane() -> String {
    "z".to_string()
}

fn default_mux_sync_panes() -> String {
    "y".to_string()
}

fn default_mux_command_mode() -> String {
    ":".to_string()
}

fn default_mux_copy_mode() -> String {
    "[".to_string()
}

fn default_mux_paste_buffer() -> String {
    "]".to_string()
}

fn default_mux_copy_selection() -> String {
    "enter".to_string()
}

fn default_mux_cancel_copy() -> String {
    "q".to_string()
}

/// Gerenciador de atalhos para o modo Multiplexer
pub struct MultiplexerShortcuts {
    pub prefix: Shortcut,
    pub new_session: Shortcut,
    pub list_sessions: Shortcut,
    pub attach_session: Shortcut,
    pub detach: Shortcut,
    pub kill_session: Shortcut,
    pub rename_session: Shortcut,
    pub new_window: Shortcut,
    pub next_window: Shortcut,
    pub previous_window: Shortcut,
    pub split_horizontal: Shortcut,
    pub split_vertical: Shortcut,
    pub pane_up: Shortcut,
    pub pane_down: Shortcut,
    pub pane_left: Shortcut,
    pub pane_right: Shortcut,
    pub close_pane: Shortcut,
    pub next_layout: Shortcut,
    pub zoom_pane: Shortcut,
    pub sync_panes: Shortcut,
    pub command_mode: Shortcut,
    pub copy_mode: Shortcut,
    pub paste_buffer: Shortcut,
    pub copy_selection: Shortcut,
    pub cancel_copy: Shortcut,
}

impl MultiplexerShortcuts {
    pub fn from_config(config: &MultiplexerKeyBindings) -> Self {
        Self {
            prefix: Shortcut::parse(&config.prefix).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: false,
                alt: false,
                key: "b".to_string(),
            }),
            new_session: Shortcut::parse(&config.new_session).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: ":".to_string(),
            }),
            list_sessions: Shortcut::parse(&config.list_sessions).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "s".to_string(),
            }),
            attach_session: Shortcut::parse(&config.attach_session).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "a".to_string(),
            }),
            detach: Shortcut::parse(&config.detach).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "d".to_string(),
            }),
            kill_session: Shortcut::parse(&config.kill_session).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "x".to_string(),
            }),
            rename_session: Shortcut::parse(&config.rename_session).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "$".to_string(),
            }),
            new_window: Shortcut::parse(&config.new_window).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "c".to_string(),
            }),
            next_window: Shortcut::parse(&config.next_window).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "n".to_string(),
            }),
            previous_window: Shortcut::parse(&config.previous_window).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "p".to_string(),
            }),
            split_horizontal: Shortcut::parse(&config.split_horizontal).unwrap_or_else(|| {
                Shortcut {
                    ctrl: false,
                    shift: false,
                    alt: false,
                    key: "\"".to_string(),
                }
            }),
            split_vertical: Shortcut::parse(&config.split_vertical).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "%".to_string(),
            }),
            pane_up: Shortcut::parse(&config.pane_up).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "up".to_string(),
            }),
            pane_down: Shortcut::parse(&config.pane_down).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "down".to_string(),
            }),
            pane_left: Shortcut::parse(&config.pane_left).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "left".to_string(),
            }),
            pane_right: Shortcut::parse(&config.pane_right).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "right".to_string(),
            }),
            close_pane: Shortcut::parse(&config.close_pane).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "x".to_string(),
            }),
            next_layout: Shortcut::parse(&config.next_layout).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "space".to_string(),
            }),
            zoom_pane: Shortcut::parse(&config.zoom_pane).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "z".to_string(),
            }),
            sync_panes: Shortcut::parse(&config.sync_panes).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "y".to_string(),
            }),
            command_mode: Shortcut::parse(&config.command_mode).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: ":".to_string(),
            }),
            copy_mode: Shortcut::parse(&config.copy_mode).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "[".to_string(),
            }),
            paste_buffer: Shortcut::parse(&config.paste_buffer).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "]".to_string(),
            }),
            copy_selection: Shortcut::parse(&config.copy_selection).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "enter".to_string(),
            }),
            cancel_copy: Shortcut::parse(&config.cancel_copy).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "q".to_string(),
            }),
        }
    }

    /// Verifica se uma tecla corresponde ao prefixo
    pub fn is_prefix(&self, ctrl: bool, shift: bool, alt: bool, key: &str) -> bool {
        self.prefix.matches(ctrl, shift, alt, key)
    }

    /// Verifica se é navegação para pane (retorna direção)
    pub fn is_pane_navigation(
        &self,
        ctrl: bool,
        shift: bool,
        alt: bool,
        key: &str,
    ) -> Option<&str> {
        if self.pane_up.matches(ctrl, shift, alt, key) {
            Some("up")
        } else if self.pane_down.matches(ctrl, shift, alt, key) {
            Some("down")
        } else if self.pane_left.matches(ctrl, shift, alt, key) {
            Some("left")
        } else if self.pane_right.matches(ctrl, shift, alt, key) {
            Some("right")
        } else {
            None
        }
    }
}

impl Default for MultiplexerShortcuts {
    fn default() -> Self {
        Self::from_config(&MultiplexerKeyBindings::default())
    }
}

/// Representa um atalho de teclado parseado
#[derive(Debug, Clone, PartialEq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

impl Shortcut {
    pub fn parse(s: &str) -> Option<Self> {
        let binding = s.to_lowercase();
        let parts: Vec<&str> = binding.split('+').map(str::trim).collect();

        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut key = None;

        for part in parts {
            match part {
                "ctrl" | "control" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" => alt = true,
                // O nome que `physical_key_to_string` usa para a tecla.
                "," if key.is_none() => key = Some("comma".to_string()),
                "." if key.is_none() => key = Some("period".to_string()),
                k if key.is_none() => key = Some(k.to_string()),
                _ => {}
            }
        }

        key.map(|k| Self {
            ctrl,
            shift,
            alt,
            key: k,
        })
    }

    pub fn matches(&self, ctrl: bool, shift: bool, alt: bool, key: &str) -> bool {
        self.ctrl == ctrl
            && self.shift == shift
            && self.alt == alt
            && self.key == key.to_lowercase()
    }

    pub fn matches_key_only(&self, key: &str) -> bool {
        !self.ctrl && !self.shift && !self.alt && self.key == key.to_lowercase()
    }
}

/// Gerenciador de atalhos para tabs
pub struct TabShortcuts {
    pub new_tab: Shortcut,
    pub close_tab: Shortcut,
    pub next_tab: Shortcut,
    pub previous_tab: Shortcut,
    pub go_to_tab_prefix: String,
}

impl TabShortcuts {
    pub fn from_config(config: &KeyBindings) -> Self {
        Self {
            new_tab: Shortcut::parse(&config.new_tab).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: true,
                alt: false,
                key: "t".to_string(),
            }),
            close_tab: Shortcut::parse(&config.close_tab).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: false,
                alt: false,
                key: "w".to_string(),
            }),
            next_tab: Shortcut::parse(&config.next_tab).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: false,
                alt: false,
                key: "tab".to_string(),
            }),
            previous_tab: Shortcut::parse(&config.previous_tab).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: true,
                alt: false,
                key: "tab".to_string(),
            }),
            go_to_tab_prefix: config.go_to_tab_prefix.clone(),
        }
    }

    pub fn is_go_to_tab(&self, ctrl: bool, _shift: bool, _alt: bool, key: &str) -> Option<usize> {
        if !ctrl || self.go_to_tab_prefix != "ctrl" {
            return None;
        }

        key.parse::<usize>()
            .ok()
            .map(|n| if n == 0 { 9 } else { n - 1 })
    }
}

impl Default for TabShortcuts {
    fn default() -> Self {
        Self::from_config(&KeyBindings::default())
    }
}

/// Gerenciador de atalhos para clipboard
pub struct ClipboardShortcuts {
    pub copy: Shortcut,
    pub paste: Shortcut,
}

impl ClipboardShortcuts {
    pub fn from_config(config: &KeyBindings) -> Self {
        Self {
            copy: Shortcut::parse(&config.copy).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: false,
                alt: false,
                key: "c".to_string(),
            }),
            paste: Shortcut::parse(&config.paste).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: false,
                alt: false,
                key: "v".to_string(),
            }),
        }
    }
}

impl Default for ClipboardShortcuts {
    fn default() -> Self {
        Self::from_config(&KeyBindings::default())
    }
}

/// Gerenciador de atalhos para busca
pub struct SearchShortcuts {
    pub search_open: Shortcut,
    pub search_close: Shortcut,
    pub search_next: Shortcut,
    pub search_previous: Shortcut,
}

impl SearchShortcuts {
    pub fn from_config(config: &KeyBindings) -> Self {
        Self {
            search_open: Shortcut::parse(&config.search_open).unwrap_or_else(|| Shortcut {
                ctrl: true,
                shift: false,
                alt: false,
                key: "f".to_string(),
            }),
            search_close: Shortcut::parse(&config.search_close).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "escape".to_string(),
            }),
            search_next: Shortcut::parse(&config.search_next).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: false,
                alt: false,
                key: "f3".to_string(),
            }),
            search_previous: Shortcut::parse(&config.search_previous).unwrap_or_else(|| Shortcut {
                ctrl: false,
                shift: true,
                alt: false,
                key: "f3".to_string(),
            }),
        }
    }
}

impl Default for SearchShortcuts {
    fn default() -> Self {
        Self::from_config(&KeyBindings::default())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_every_editable_shortcut_can_be_read_and_changed() {
        let mut kb = KeyBindings::default();
        for id in EDITABLE_SHORTCUTS {
            let value = kb.get(id).unwrap_or_else(|| panic!("{id}")).to_string();
            assert!(Shortcut::parse(&value).is_some(), "{id} = {value}");
            assert!(kb.set(id, "ctrl+alt+f9"), "{id}");
            assert_eq!(kb.get(id), Some("ctrl+alt+f9"));
        }
        assert!(!kb.set("nao_existe", "ctrl+a"));
        assert!(!kb.set("new_tab", ""));
        assert_eq!(kb.get("nao_existe"), None);
    }

    /// Ctrl+, abre as configurações; "ctrl+," e "ctrl+comma" valem igual.
    #[test]
    fn test_open_settings_comma_binding() {
        let kb = KeyBindings::default();
        assert_eq!(KeyBindings::display(&kb.open_settings), "Ctrl+,");
        for binding in ["ctrl+comma", "Ctrl+,"] {
            let shortcut = Shortcut::parse(binding).unwrap();
            assert!(shortcut.matches(true, false, false, "comma"), "{binding}");
            assert!(!shortcut.matches(true, true, false, "comma"));
        }
    }

    /// Regressão: Ctrl+C copiava em vez de mandar SIGINT ao programa.
    #[test]
    fn test_defaults_leave_shell_control_keys_alone() {
        let kb = KeyBindings::default();
        for binding in [
            &kb.copy,
            &kb.paste,
            &kb.search_open,
            &kb.close_tab,
            &kb.new_tab,
        ] {
            let shortcut = Shortcut::parse(binding).unwrap();
            assert!(
                !shortcut.ctrl || shortcut.shift || shortcut.alt,
                "{binding} rouba uma tecla de controle do shell"
            );
        }
    }

    #[test]
    fn test_upgrade_legacy_defaults_only_touches_old_values() {
        let mut kb = KeyBindings {
            copy: "ctrl+c".into(),
            paste: "Ctrl+V".into(),
            search_open: "ctrl+alt+f".into(),
            close_tab: "ctrl+w".into(),
            ..KeyBindings::default()
        };
        assert!(kb.upgrade_legacy_defaults());
        assert_eq!(kb.copy, "ctrl+shift+c");
        assert_eq!(kb.paste, "ctrl+shift+v");
        assert_eq!(kb.search_open, "ctrl+alt+f");
        assert_eq!(kb.close_tab, "ctrl+shift+w");
        assert!(!kb.upgrade_legacy_defaults());
    }

    #[test]
    fn test_display_binding() {
        assert_eq!(KeyBindings::display("ctrl+shift+t"), "Ctrl+Shift+T");
        assert_eq!(KeyBindings::display("f3"), "F3");
        assert_eq!(KeyBindings::display("escape"), "Escape");
    }

    use super::*;

    #[test]
    fn test_shortcut_parse() {
        let s = Shortcut::parse("ctrl+shift+t").unwrap();
        assert!(s.ctrl);
        assert!(s.shift);
        assert!(!s.alt);
        assert_eq!(s.key, "t");
    }

    #[test]
    fn test_shortcut_parse_simple() {
        let s = Shortcut::parse("ctrl+w").unwrap();
        assert!(s.ctrl);
        assert!(!s.shift);
        assert_eq!(s.key, "w");
    }

    #[test]
    fn test_shortcut_matches() {
        let s = Shortcut::parse("ctrl+shift+t").unwrap();
        assert!(s.matches(true, true, false, "t"));
        assert!(!s.matches(true, false, false, "t"));
        assert!(!s.matches(true, true, false, "w"));
    }

    #[test]
    fn test_is_go_to_tab() {
        let shortcuts = TabShortcuts::default();
        assert_eq!(shortcuts.is_go_to_tab(true, false, false, "1"), Some(0));
        assert_eq!(shortcuts.is_go_to_tab(true, false, false, "5"), Some(4));
        assert_eq!(shortcuts.is_go_to_tab(true, false, false, "9"), Some(8));
        assert_eq!(shortcuts.is_go_to_tab(false, false, false, "1"), None);
        assert_eq!(shortcuts.is_go_to_tab(true, false, false, "a"), None);
    }

    #[test]
    fn test_key_bindings_default() {
        let bindings = KeyBindings::default();
        assert_eq!(bindings.new_tab, "ctrl+shift+t");
        assert_eq!(bindings.close_tab, "ctrl+shift+w");
        assert_eq!(bindings.next_tab, "ctrl+tab");
        assert_eq!(bindings.copy, "ctrl+shift+c");
        assert_eq!(bindings.paste, "ctrl+shift+v");
        assert_eq!(bindings.search_open, "ctrl+shift+f");
        assert_eq!(bindings.search_close, "escape");
        assert_eq!(bindings.search_next, "f3");
        assert_eq!(bindings.search_previous, "shift+f3");
    }

    #[test]
    fn test_clipboard_shortcuts_default() {
        let shortcuts = ClipboardShortcuts::default();
        assert!(shortcuts.copy.ctrl);
        assert!(shortcuts.copy.shift);
        assert_eq!(shortcuts.copy.key, "c");
        assert!(shortcuts.paste.ctrl);
        assert!(shortcuts.paste.shift);
        assert_eq!(shortcuts.paste.key, "v");
    }

    #[test]
    fn test_search_shortcuts_default() {
        let shortcuts = SearchShortcuts::default();
        assert!(shortcuts.search_open.ctrl);
        assert_eq!(shortcuts.search_open.key, "f");
        assert!(shortcuts.search_close.matches_key_only("escape"));
        assert!(shortcuts.search_next.matches_key_only("f3"));
        assert!(!shortcuts.search_previous.ctrl);
        assert!(shortcuts.search_previous.shift);
        assert_eq!(shortcuts.search_previous.key, "f3");
    }

    #[test]
    fn test_clipboard_shortcuts_from_config() {
        let mut config = KeyBindings::default();
        config.copy = "ctrl+shift+c".to_string();
        config.paste = "ctrl+shift+v".to_string();

        let shortcuts = ClipboardShortcuts::from_config(&config);
        assert!(shortcuts.copy.ctrl);
        assert!(shortcuts.copy.shift);
        assert_eq!(shortcuts.copy.key, "c");
        assert!(shortcuts.paste.ctrl);
        assert!(shortcuts.paste.shift);
        assert_eq!(shortcuts.paste.key, "v");
    }

    #[test]
    fn test_search_shortcuts_from_config() {
        let mut config = KeyBindings::default();
        config.search_open = "ctrl+shift+f".to_string();
        config.search_next = "ctrl+g".to_string();

        let shortcuts = SearchShortcuts::from_config(&config);
        assert!(shortcuts.search_open.ctrl);
        assert!(shortcuts.search_open.shift);
        assert_eq!(shortcuts.search_open.key, "f");
        assert!(shortcuts.search_next.ctrl);
        assert!(!shortcuts.search_next.shift);
        assert_eq!(shortcuts.search_next.key, "g");
    }

    #[test]
    fn test_shortcut_matches_key_only() {
        let escape = Shortcut::parse("escape").unwrap();
        assert!(escape.matches_key_only("escape"));
        assert!(escape.matches_key_only("Escape")); // case insensitive
        assert!(!escape.matches(true, false, false, "escape"));

        let f3 = Shortcut::parse("f3").unwrap();
        assert!(f3.matches_key_only("f3"));
        assert!(!f3.matches(true, false, false, "f3"));
    }

    #[test]
    fn test_shortcut_parse_case_insensitive() {
        let s1 = Shortcut::parse("Ctrl+Shift+T").unwrap();
        let s2 = Shortcut::parse("ctrl+shift+t").unwrap();
        assert_eq!(s1, s2);
    }

    // ==================== MULTIPLEXER KEYBINDINGS TESTS ====================

    #[test]
    fn test_multiplexer_keybindings_default() {
        let bindings = MultiplexerKeyBindings::default();
        assert_eq!(bindings.prefix, "ctrl+b");
        assert_eq!(bindings.new_session, ":new");
        assert_eq!(bindings.list_sessions, "s");
        assert_eq!(bindings.attach_session, "a");
        assert_eq!(bindings.detach, "d");
        assert_eq!(bindings.kill_session, "x");
        assert_eq!(bindings.rename_session, "$");
        assert_eq!(bindings.new_window, "c");
        assert_eq!(bindings.next_window, "n");
        assert_eq!(bindings.previous_window, "p");
        assert_eq!(bindings.split_horizontal, "\"");
        assert_eq!(bindings.split_vertical, "%");
        assert_eq!(bindings.pane_up, "up");
        assert_eq!(bindings.pane_down, "down");
        assert_eq!(bindings.pane_left, "left");
        assert_eq!(bindings.pane_right, "right");
        assert_eq!(bindings.close_pane, "x");
        assert_eq!(bindings.next_layout, "space");
        assert_eq!(bindings.zoom_pane, "z");
        assert_eq!(bindings.sync_panes, "y");
        assert_eq!(bindings.command_mode, ":");
        assert_eq!(bindings.copy_mode, "[");
        assert_eq!(bindings.paste_buffer, "]");
        assert_eq!(bindings.copy_selection, "enter");
        assert_eq!(bindings.cancel_copy, "q");
    }

    #[test]
    fn test_multiplexer_shortcuts_default() {
        let shortcuts = MultiplexerShortcuts::default();
        assert!(shortcuts.prefix.ctrl);
        assert!(!shortcuts.prefix.shift);
        assert_eq!(shortcuts.prefix.key, "b");
        assert_eq!(shortcuts.new_window.key, "c");
        assert_eq!(shortcuts.detach.key, "d");
        assert_eq!(shortcuts.list_sessions.key, "s");
        assert_eq!(shortcuts.split_horizontal.key, "\"");
        assert_eq!(shortcuts.split_vertical.key, "%");
    }

    #[test]
    fn test_multiplexer_shortcuts_is_prefix() {
        let shortcuts = MultiplexerShortcuts::default();
        assert!(shortcuts.is_prefix(true, false, false, "b"));
        assert!(!shortcuts.is_prefix(false, false, false, "b"));
        assert!(!shortcuts.is_prefix(true, true, false, "b"));
    }

    #[test]
    fn test_multiplexer_shortcuts_pane_navigation() {
        let shortcuts = MultiplexerShortcuts::default();

        // Test pane navigation detection
        assert_eq!(
            shortcuts.is_pane_navigation(false, false, false, "up"),
            Some("up")
        );
        assert_eq!(
            shortcuts.is_pane_navigation(false, false, false, "down"),
            Some("down")
        );
        assert_eq!(
            shortcuts.is_pane_navigation(false, false, false, "left"),
            Some("left")
        );
        assert_eq!(
            shortcuts.is_pane_navigation(false, false, false, "right"),
            Some("right")
        );
        assert_eq!(shortcuts.is_pane_navigation(true, false, false, "up"), None);
        assert_eq!(shortcuts.is_pane_navigation(false, false, true, "up"), None);
        assert_eq!(shortcuts.is_pane_navigation(false, false, false, "x"), None);
    }

    #[test]
    fn test_multiplexer_shortcuts_from_config() {
        let mut config = MultiplexerKeyBindings::default();
        config.prefix = "ctrl+a".to_string();
        config.new_window = "w".to_string();
        config.detach = "q".to_string();

        let shortcuts = MultiplexerShortcuts::from_config(&config);
        assert!(shortcuts.prefix.ctrl);
        assert_eq!(shortcuts.prefix.key, "a");
        assert_eq!(shortcuts.new_window.key, "w");
        assert_eq!(shortcuts.detach.key, "q");
    }

    #[test]
    fn test_multiplexer_keybindings_custom() {
        let bindings = MultiplexerKeyBindings {
            prefix: "ctrl+space".to_string(),
            new_session: "c".to_string(),
            list_sessions: "l".to_string(),
            attach_session: "a".to_string(),
            detach: "d".to_string(),
            kill_session: "k".to_string(),
            rename_session: "r".to_string(),
            new_window: "n".to_string(),
            next_window: "]".to_string(),
            previous_window: "[".to_string(),
            go_to_window_prefix: "".to_string(),
            split_horizontal: "h".to_string(),
            split_vertical: "v".to_string(),
            pane_up: "up".to_string(),
            pane_down: "down".to_string(),
            pane_left: "left".to_string(),
            pane_right: "right".to_string(),
            close_pane: "x".to_string(),
            next_layout: "l".to_string(),
            zoom_pane: "z".to_string(),
            sync_panes: "s".to_string(),
            command_mode: ";".to_string(),
            copy_mode: "e".to_string(),
            paste_buffer: "p".to_string(),
            copy_selection: "y".to_string(),
            cancel_copy: "q".to_string(),
        };

        let shortcuts = MultiplexerShortcuts::from_config(&bindings);
        assert!(shortcuts.prefix.ctrl);
        assert_eq!(shortcuts.prefix.key, "space");
        assert_eq!(shortcuts.new_session.key, "c");
        assert_eq!(shortcuts.list_sessions.key, "l");
        assert!(shortcuts.is_prefix(true, false, false, "space"));
    }

    #[test]
    fn test_multiplexer_shortcuts_window_navigation() {
        let shortcuts = MultiplexerShortcuts::default();

        assert_eq!(shortcuts.next_window.key, "n");
        assert_eq!(shortcuts.previous_window.key, "p");
        assert!(!shortcuts.next_window.ctrl);
        assert!(!shortcuts.previous_window.ctrl);
    }

    #[test]
    fn test_multiplexer_shortcuts_pane_operations() {
        let shortcuts = MultiplexerShortcuts::default();

        assert_eq!(shortcuts.zoom_pane.key, "z");
        assert_eq!(shortcuts.close_pane.key, "x");
        assert_eq!(shortcuts.sync_panes.key, "y");
        assert_eq!(shortcuts.next_layout.key, "space");
    }

    #[test]
    fn test_multiplexer_shortcuts_copy_mode() {
        let shortcuts = MultiplexerShortcuts::default();

        assert_eq!(shortcuts.copy_mode.key, "[");
        assert_eq!(shortcuts.paste_buffer.key, "]");
        assert_eq!(shortcuts.copy_selection.key, "enter");
        assert_eq!(shortcuts.cancel_copy.key, "q");
    }

    // ==================== NATIVE SPLIT KEYBINDINGS TESTS ====================

    #[test]
    fn test_native_split_keybindings_default() {
        let bindings = SplitKeyBindings::default();
        assert_eq!(bindings.split_horizontal, "ctrl+shift+e");
        assert_eq!(bindings.split_vertical, "ctrl+shift+o");
        assert_eq!(bindings.close_pane, "ctrl+shift+x");
        // Alt+setas fica com o shell (palavra a palavra); Ctrl+Tab troca de tab.
        assert_eq!(bindings.nav_up, "ctrl+shift+up");
        assert_eq!(bindings.nav_down, "ctrl+shift+down");
        assert_eq!(bindings.nav_left, "ctrl+shift+left");
        assert_eq!(bindings.nav_right, "ctrl+shift+right");
        assert_eq!(bindings.next_pane, "ctrl+shift+bracketright");
        assert_eq!(bindings.prev_pane, "ctrl+shift+bracketleft");
        assert_eq!(bindings.increase_size, "ctrl+shift+equal");
        assert_eq!(bindings.decrease_size, "ctrl+shift+minus");
    }

    #[test]
    fn test_native_split_shortcuts_default() {
        let shortcuts = NativeSplitShortcuts::default();
        assert!(shortcuts.split_horizontal.ctrl);
        assert!(shortcuts.split_horizontal.shift);
        assert_eq!(shortcuts.split_horizontal.key, "e");

        assert!(shortcuts.split_vertical.ctrl);
        assert!(shortcuts.split_vertical.shift);
        assert_eq!(shortcuts.split_vertical.key, "o");

        assert!(shortcuts.nav_up.ctrl && shortcuts.nav_up.shift);
        assert!(!shortcuts.nav_up.alt);
        assert_eq!(shortcuts.nav_up.key, "up");

        assert!(shortcuts.nav_left.ctrl && !shortcuts.nav_left.alt);
        assert_eq!(shortcuts.nav_left.key, "left");
    }

    #[test]
    fn test_native_split_shortcuts_try_get_action() {
        let shortcuts = NativeSplitShortcuts::default();

        // Split horizontal: Ctrl+Shift+E
        assert_eq!(
            shortcuts.try_get_action(true, true, false, "e"),
            Some(Action::SplitHorizontal)
        );

        // Split vertical: Ctrl+Shift+O
        assert_eq!(
            shortcuts.try_get_action(true, true, false, "o"),
            Some(Action::SplitVertical)
        );

        // Close pane: Ctrl+Shift+X (Ctrl+Shift+W fecha a tab)
        assert_eq!(
            shortcuts.try_get_action(true, true, false, "x"),
            Some(Action::ClosePane)
        );

        // Navigation: Ctrl+Shift+Setas
        for (key, action) in [
            ("up", Action::NavigateUp),
            ("down", Action::NavigateDown),
            ("left", Action::NavigateLeft),
            ("right", Action::NavigateRight),
        ] {
            assert_eq!(
                shortcuts.try_get_action(true, true, false, key),
                Some(action)
            );
            // Alt+setas continua com o shell.
            assert_eq!(shortcuts.try_get_action(false, false, true, key), None);
        }

        // Next/Prev pane (Ctrl+Tab fica com as tabs)
        assert_eq!(
            shortcuts.try_get_action(true, true, false, "bracketright"),
            Some(Action::NextPane)
        );
        assert_eq!(
            shortcuts.try_get_action(true, true, false, "bracketleft"),
            Some(Action::PreviousPane)
        );
        assert_eq!(shortcuts.try_get_action(true, false, false, "tab"), None);

        // Resize
        assert_eq!(
            shortcuts.try_get_action(true, true, false, "equal"),
            Some(Action::IncreasePaneSize)
        );
        assert_eq!(
            shortcuts.try_get_action(true, true, false, "minus"),
            Some(Action::DecreasePaneSize)
        );

        // No match
        assert_eq!(shortcuts.try_get_action(false, false, false, "x"), None);
        assert_eq!(shortcuts.try_get_action(true, false, false, "e"), None);
    }

    #[test]
    fn test_native_split_shortcuts_pane_navigation() {
        let shortcuts = NativeSplitShortcuts::default();

        // Test pane navigation detection
        assert_eq!(
            shortcuts.is_pane_navigation(true, true, false, "up"),
            Some("up")
        );
        assert_eq!(
            shortcuts.is_pane_navigation(true, true, false, "down"),
            Some("down")
        );
        assert_eq!(
            shortcuts.is_pane_navigation(true, true, false, "left"),
            Some("left")
        );
        assert_eq!(
            shortcuts.is_pane_navigation(true, true, false, "right"),
            Some("right")
        );
        assert_eq!(shortcuts.is_pane_navigation(true, false, false, "up"), None);
        assert_eq!(
            shortcuts.is_pane_navigation(false, false, false, "up"),
            None
        );
    }

    #[test]
    fn test_native_split_shortcuts_from_config() {
        let mut config = SplitKeyBindings::default();
        config.split_horizontal = "ctrl+h".to_string();
        config.split_vertical = "ctrl+v".to_string();
        config.nav_up = "ctrl+up".to_string();

        let shortcuts = NativeSplitShortcuts::from_config(&config);
        assert!(shortcuts.split_horizontal.ctrl);
        assert!(!shortcuts.split_horizontal.shift);
        assert_eq!(shortcuts.split_horizontal.key, "h");

        assert!(shortcuts.split_vertical.ctrl);
        assert_eq!(shortcuts.split_vertical.key, "v");

        assert!(shortcuts.nav_up.ctrl);
        assert!(!shortcuts.nav_up.alt);
        assert_eq!(shortcuts.nav_up.key, "up");
    }

    #[test]
    fn test_action_enum() {
        // Test that Action enum variants exist and can be matched
        let actions = vec![
            Action::NewTab,
            Action::CloseTab,
            Action::NextTab,
            Action::PreviousTab,
            Action::GoToTab(1),
            Action::SplitHorizontal,
            Action::SplitVertical,
            Action::ClosePane,
            Action::NavigateUp,
            Action::NavigateDown,
            Action::NavigateLeft,
            Action::NavigateRight,
            Action::NextPane,
            Action::PreviousPane,
            Action::IncreasePaneSize,
            Action::DecreasePaneSize,
            Action::Copy,
            Action::Paste,
            Action::SearchOpen,
            Action::SearchClose,
            Action::SearchNext,
            Action::SearchPrevious,
            Action::Quit,
            Action::ToggleSidebar,
            Action::ToggleFullscreen,
        ];

        // Verify we can iterate over all variants
        assert!(!actions.is_empty());

        // Verify specific variants for splits
        assert_eq!(Action::SplitHorizontal, Action::SplitHorizontal);
        assert_ne!(Action::SplitHorizontal, Action::SplitVertical);
    }
}
