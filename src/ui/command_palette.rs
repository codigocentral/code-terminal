//! Command palette simples para descoberta de ações.
//!
//! Inspirado em editores modernos (Ctrl+Shift+P): caixa de busca central
//! com lista filtrada de comandos disponíveis.

use crate::ui::i18n::{tr, tr_in, Language, Text};
use tracing::debug;

/// Identificador de um comando na palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteCommand {
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    NewWindow,
    SplitHorizontal,
    SplitVertical,
    Search,
    ToggleSidebar,
    RenameTab,
    JumpToUnread,
    ShowNotifications,
    ToggleInspector,
    AiExplain,
    AiSuggest,
    NewPersistentTab,
    TogglePersistent,
    BackgroundSessions,
    OpenBrowser,
    QuickConnectSsh,
    OpenSettings,
    /// Abre a lista de temas, com prévia ao vivo.
    ChooseTheme,
    ToggleTheme,
    /// Um tema da lista de "Choose Theme…" (índice em [`THEMES`]).
    SetTheme(usize),
    /// Aplica a cor digitada em hex (cor da tab, #24).
    ApplyHexColor,
    Quit,
}

/// Temas embutidos, na ordem da configuração.
pub use crate::ui::settings::THEMES;

/// O que a paleta está listando.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PaletteMode {
    /// Os comandos (Ctrl+Shift+P, botão ≡).
    #[default]
    Commands,
    /// Os temas de "Choose Theme…": passar por um mostra a prévia.
    Themes,
    /// Campo para digitar uma cor em hex (cor personalizada da tab).
    HexColor,
}

/// Descrição exibida para um comando.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    pub command: PaletteCommand,
    pub label: &'static str,
    pub shortcut: &'static str,
    /// Outras palavras que acham o comando na busca (ex.: "tema").
    pub keywords: &'static str,
    /// Nome traduzido do comando (os itens de tema usam o nome do tema).
    pub text: Option<Text>,
}

impl PaletteItem {
    const fn new(command: PaletteCommand, label: &'static str, shortcut: &'static str) -> Self {
        Self {
            command,
            label,
            shortcut,
            keywords: "",
            text: None,
        }
    }

    /// Comando com nome traduzido (o rótulo sai do idioma em uso).
    const fn cmd(command: PaletteCommand, text: Text, shortcut: &'static str) -> Self {
        Self {
            command,
            label: "",
            shortcut,
            keywords: "",
            text: Some(text),
        }
    }

    /// O item com o rótulo no idioma em uso.
    fn localized(&self) -> Self {
        let mut item = self.clone();
        if let Some(text) = self.text {
            item.label = tr(text);
        }
        item
    }

    const fn with_keywords(mut self, keywords: &'static str) -> Self {
        self.keywords = keywords;
        self
    }

    /// Se o item aparece na busca `query` (já em minúsculas).
    /// Acha pelo nome nos dois idiomas ("tema" e "theme").
    fn matches(&self, query: &str) -> bool {
        let named = |text| {
            Language::ALL
                .iter()
                .any(|&lang| tr_in(lang, text).to_lowercase().contains(query))
        };
        self.label.to_lowercase().contains(query)
            || self.text.is_some_and(named)
            || self.keywords.contains(query)
    }
}

/// Lista fixa de comandos disponíveis na palette.
pub static PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem::cmd(PaletteCommand::NewTab, Text::CmdNewTab, "Ctrl+Shift+T"),
    PaletteItem::cmd(PaletteCommand::CloseTab, Text::CmdCloseTab, "Ctrl+Shift+W"),
    PaletteItem::cmd(PaletteCommand::NextTab, Text::CmdNextTab, "Ctrl+Tab"),
    PaletteItem::cmd(
        PaletteCommand::PreviousTab,
        Text::CmdPreviousTab,
        "Ctrl+Shift+Tab",
    ),
    PaletteItem::cmd(
        PaletteCommand::NewWindow,
        Text::CmdNewWindow,
        "Ctrl+Shift+N",
    ),
    // Configurações e temas logo no começo: aparecem sem rolar quando a
    // paleta abre pelo botão ≡.
    PaletteItem::cmd(PaletteCommand::OpenSettings, Text::CmdSettings, "Ctrl+,")
        .with_keywords("open settings preferences options configurações configuracoes opções"),
    PaletteItem::cmd(PaletteCommand::ChooseTheme, Text::CmdChooseTheme, "")
        .with_keywords("theme themes colors color scheme tema temas cores aparência aparencia"),
    PaletteItem::cmd(
        PaletteCommand::SplitHorizontal,
        Text::CmdSplitHorizontal,
        "Ctrl+Shift+E",
    ),
    PaletteItem::cmd(
        PaletteCommand::SplitVertical,
        Text::CmdSplitVertical,
        "Ctrl+Shift+O",
    ),
    PaletteItem::cmd(PaletteCommand::Search, Text::CmdSearch, "Ctrl+Shift+F"),
    PaletteItem::cmd(
        PaletteCommand::ToggleSidebar,
        Text::CmdToggleSidebar,
        "Ctrl+Shift+B",
    ),
    PaletteItem::cmd(PaletteCommand::RenameTab, Text::CmdRenameTab, "F2"),
    PaletteItem::cmd(
        PaletteCommand::JumpToUnread,
        Text::CmdJumpToUnread,
        "Ctrl+Shift+U",
    ),
    PaletteItem::cmd(
        PaletteCommand::ShowNotifications,
        Text::CmdShowNotifications,
        "Ctrl+Shift+I",
    ),
    PaletteItem::cmd(
        PaletteCommand::ToggleInspector,
        Text::CmdToggleInspector,
        "Ctrl+Shift+D",
    ),
    PaletteItem::cmd(
        PaletteCommand::AiExplain,
        Text::CmdAiExplain,
        "Ctrl+Shift+A",
    ),
    PaletteItem::cmd(
        PaletteCommand::AiSuggest,
        Text::CmdAiSuggest,
        "Ctrl+Shift+K",
    ),
    PaletteItem::cmd(
        PaletteCommand::NewPersistentTab,
        Text::CmdNewPersistentTab,
        "",
    ),
    PaletteItem::cmd(
        PaletteCommand::TogglePersistent,
        Text::CmdTogglePersistent,
        "",
    )
    .with_keywords("persistent persistente"),
    PaletteItem::cmd(
        PaletteCommand::BackgroundSessions,
        Text::CmdBackgroundSessions,
        "",
    ),
    PaletteItem::cmd(PaletteCommand::OpenBrowser, Text::CmdOpenBrowser, ""),
    PaletteItem::cmd(
        PaletteCommand::QuickConnectSsh,
        Text::CmdQuickConnectSsh,
        "",
    ),
    PaletteItem::cmd(PaletteCommand::ToggleTheme, Text::CmdToggleTheme, "")
        .with_keywords("next theme tema"),
    PaletteItem::cmd(PaletteCommand::Quit, Text::CmdQuit, "Alt+F4"),
];

/// Estado atual da command palette.
#[derive(Debug, Default)]
pub struct CommandPalette {
    /// Se a palette está visível.
    pub visible: bool,
    /// Texto digitado pelo usuário.
    pub query: String,
    /// Índice do item selecionado na lista filtrada.
    pub selected_index: usize,
    /// Itens filtrados pela query atual.
    filtered_items: Vec<PaletteItem>,
    /// Primeiro item visível da lista (rolagem), lembrado entre frames.
    ///
    /// O desenho só o muda quando o item selecionado sai da área visível; o
    /// mouse em cima de uma linha seleciona `first + linha` e a lista não
    /// volta para o topo.
    first: usize,
    /// Comandos ou temas.
    mode: PaletteMode,
    /// Tema em uso quando a lista de temas abriu (marcado "current").
    theme_at_open: Option<usize>,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
            selected_index: 0,
            filtered_items: PALETTE_ITEMS.iter().map(PaletteItem::localized).collect(),
            first: 0,
            mode: PaletteMode::Commands,
            theme_at_open: None,
        }
    }

    /// Abre a palette.
    pub fn open(&mut self) {
        self.open_in(PaletteMode::Commands);
        debug!("Command palette opened");
    }

    /// Abre a lista de temas com o tema `current` selecionado.
    pub fn open_themes(&mut self, current: &str) {
        self.theme_at_open = THEMES
            .iter()
            .position(|name| name.eq_ignore_ascii_case(current));
        self.open_in(PaletteMode::Themes);
        self.selected_index = self.theme_at_open.unwrap_or(0);
        debug!("Theme chooser opened");
    }

    fn open_in(&mut self, mode: PaletteMode) {
        self.visible = true;
        self.mode = mode;
        self.query.clear();
        self.selected_index = 0;
        self.first = 0;
        self.update_filtered();
    }

    /// Abre o campo de cor em hex, já com `initial` digitado.
    pub fn open_hex_color(&mut self, initial: &str) {
        self.open_in(PaletteMode::HexColor);
        self.query = initial.to_string();
        self.update_filtered();
    }

    /// Cor digitada no modo hex, se já for uma cor válida.
    pub fn hex_color(&self) -> Option<[u8; 3]> {
        (self.mode == PaletteMode::HexColor)
            .then(|| crate::ui::tab_colors::parse_hex(&self.query))
            .flatten()
    }

    /// Texto quando a lista fica vazia.
    pub fn empty_text(&self) -> &'static str {
        match self.mode {
            PaletteMode::HexColor => crate::ui::i18n::tr(crate::ui::i18n::Text::NoValidColor),
            _ => tr(Text::NoMatchingCommands),
        }
    }

    /// Comandos ou temas.
    pub fn mode(&self) -> PaletteMode {
        self.mode
    }

    /// Tema selecionado na lista de temas (`None` listando comandos).
    pub fn selected_theme(&self) -> Option<&'static str> {
        match self.selected_item()?.command {
            PaletteCommand::SetTheme(index) if self.mode == PaletteMode::Themes => {
                THEMES.get(index).copied()
            }
            _ => None,
        }
    }

    /// Texto da caixa de busca vazia.
    pub fn placeholder(&self) -> &'static str {
        match self.mode {
            PaletteMode::Commands => tr(Text::PalettePlaceholder),
            PaletteMode::Themes => tr(Text::ThemesPlaceholder),
            PaletteMode::HexColor => {
                crate::ui::i18n::tr(crate::ui::i18n::Text::HexColorPlaceholder)
            }
        }
    }

    /// Primeiro item visível da última vez que a lista foi desenhada.
    pub fn first_visible(&self) -> usize {
        self.first
    }

    /// Guarda a rolagem que o desenho usou (ver [`Self::first_visible`]).
    pub fn set_first_visible(&mut self, first: usize) {
        self.first = first;
    }

    /// Fecha a palette.
    pub fn close(&mut self) {
        self.visible = false;
        self.query.clear();
        debug!("Command palette closed");
    }

    /// Alterna visibilidade.
    pub fn toggle(&mut self) {
        if self.visible {
            self.close();
        } else {
            self.open();
        }
    }

    /// Retorna true se a palette está visível.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Retorna os itens filtrados atualmente.
    pub fn items(&self) -> &[PaletteItem] {
        &self.filtered_items
    }

    /// Retorna o item selecionado, se houver.
    pub fn selected_item(&self) -> Option<&PaletteItem> {
        self.filtered_items.get(self.selected_index)
    }

    /// Adiciona um caractere à query.
    pub fn type_char(&mut self, c: char) {
        self.query.push(c);
        self.update_filtered();
    }

    /// Remove o último caractere da query.
    pub fn backspace(&mut self) {
        self.query.pop();
        self.update_filtered();
    }

    /// Move a seleção para cima.
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else if !self.filtered_items.is_empty() {
            self.selected_index = self.filtered_items.len() - 1;
        }
    }

    /// Move a seleção para baixo.
    pub fn move_down(&mut self) {
        if self.filtered_items.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.filtered_items.len();
    }

    /// Seleciona o item `index` da lista filtrada (mouse em cima do item).
    /// Índice fora da lista é ignorado.
    pub fn select(&mut self, index: usize) {
        if index < self.filtered_items.len() {
            self.selected_index = index;
        }
    }

    /// Move a seleção `lines` itens (positivo = para baixo), parando nas
    /// pontas: a roda do mouse não dá a volta na lista como as setas.
    pub fn scroll_by(&mut self, lines: i32) {
        let Some(last) = self.filtered_items.len().checked_sub(1) else {
            return;
        };
        let target = self.selected_index as i64 + i64::from(lines);
        self.selected_index = target.clamp(0, last as i64) as usize;
    }

    /// Confirma o item selecionado, retornando o comando.
    pub fn confirm(&mut self) -> Option<PaletteCommand> {
        let command = self.selected_item().map(|item| item.command);
        if command.is_some() {
            self.close();
        }
        command
    }

    fn update_filtered(&mut self) {
        let query_lower = self.query.to_lowercase();
        self.filtered_items = match self.mode {
            PaletteMode::Commands => PALETTE_ITEMS
                .iter()
                .filter(|item| item.matches(&query_lower))
                .map(PaletteItem::localized)
                .collect(),
            PaletteMode::Themes => THEMES
                .iter()
                .enumerate()
                .map(|(index, &name)| {
                    let mark = if self.theme_at_open == Some(index) {
                        tr(Text::CurrentMark)
                    } else {
                        ""
                    };
                    PaletteItem::new(PaletteCommand::SetTheme(index), name, mark)
                })
                .filter(|item| item.matches(&query_lower))
                .collect(),
            PaletteMode::HexColor => crate::ui::tab_colors::parse_hex(&self.query)
                .map(|_| {
                    PaletteItem::new(
                        PaletteCommand::ApplyHexColor,
                        crate::ui::i18n::tr(crate::ui::i18n::Text::ApplyColor),
                        "",
                    )
                })
                .into_iter()
                .collect(),
        };
        self.selected_index = self
            .selected_index
            .min(self.filtered_items.len().saturating_sub(1));
    }
}

/// Executa um comando da palette no contexto da aplicação.
pub fn execute_palette_command(command: PaletteCommand) -> PaletteAction {
    debug!(command = ?command, "Executing palette command");
    match command {
        PaletteCommand::NewTab => PaletteAction::NewTab,
        PaletteCommand::CloseTab => PaletteAction::CloseTab,
        PaletteCommand::NextTab => PaletteAction::NextTab,
        PaletteCommand::PreviousTab => PaletteAction::PreviousTab,
        PaletteCommand::NewWindow => PaletteAction::NewWindow,
        PaletteCommand::SplitHorizontal => PaletteAction::SplitHorizontal,
        PaletteCommand::SplitVertical => PaletteAction::SplitVertical,
        PaletteCommand::Search => PaletteAction::Search,
        PaletteCommand::ToggleSidebar => PaletteAction::ToggleSidebar,
        PaletteCommand::RenameTab => PaletteAction::RenameTab,
        PaletteCommand::JumpToUnread => PaletteAction::JumpToUnread,
        PaletteCommand::ShowNotifications => PaletteAction::ShowNotifications,
        PaletteCommand::ToggleInspector => PaletteAction::ToggleInspector,
        PaletteCommand::AiExplain => PaletteAction::AiExplain,
        PaletteCommand::AiSuggest => PaletteAction::AiSuggest,
        PaletteCommand::NewPersistentTab => PaletteAction::NewPersistentTab,
        PaletteCommand::TogglePersistent => PaletteAction::TogglePersistent,
        PaletteCommand::BackgroundSessions => PaletteAction::BackgroundSessions,
        PaletteCommand::OpenBrowser => PaletteAction::OpenBrowser,
        PaletteCommand::QuickConnectSsh => PaletteAction::QuickConnectSsh,
        PaletteCommand::OpenSettings => PaletteAction::OpenSettings,
        PaletteCommand::ChooseTheme => PaletteAction::ChooseTheme,
        PaletteCommand::ToggleTheme => PaletteAction::ToggleTheme,
        // A prévia já aplicou o tema; o AppState só grava (ver
        // `AppState::handle_command_palette_key`).
        PaletteCommand::SetTheme(_) | PaletteCommand::ApplyHexColor => PaletteAction::None,
        PaletteCommand::Quit => PaletteAction::Quit,
    }
}

/// Teclas de input reconhecidas pela palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteInputKey {
    Char(char),
    Backspace,
    Up,
    Down,
    Enter,
    Escape,
}

/// Ação resultante da execução de um comando da palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteAction {
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    NewWindow,
    SplitHorizontal,
    SplitVertical,
    Search,
    ToggleSidebar,
    RenameTab,
    JumpToUnread,
    ShowNotifications,
    ToggleInspector,
    AiExplain,
    AiSuggest,
    NewPersistentTab,
    TogglePersistent,
    BackgroundSessions,
    OpenBrowser,
    QuickConnectSsh,
    OpenSettings,
    ChooseTheme,
    ToggleTheme,
    Quit,
    None,
}

impl PaletteAction {
    /// Converte um resultado de confirmação em ação.
    pub fn from_command(command: Option<PaletteCommand>) -> Self {
        match command {
            Some(cmd) => execute_palette_command(cmd),
            None => PaletteAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regressão: a palette mostrava Ctrl+T/Ctrl+W/Ctrl+F, mas os atalhos padrão
    /// são com Shift.
    #[test]
    fn test_palette_shortcuts_match_default_keybindings() {
        let keys = crate::config::KeyBindings::default();
        let splits = crate::config::keybindings::SplitKeyBindings::default();
        let expected = [
            (PaletteCommand::NewTab, &keys.new_tab),
            (PaletteCommand::CloseTab, &keys.close_tab),
            (PaletteCommand::NextTab, &keys.next_tab),
            (PaletteCommand::PreviousTab, &keys.previous_tab),
            (PaletteCommand::SplitHorizontal, &splits.split_horizontal),
            (PaletteCommand::SplitVertical, &splits.split_vertical),
            (PaletteCommand::Search, &keys.search_open),
            (PaletteCommand::OpenSettings, &keys.open_settings),
        ];
        for (command, binding) in expected {
            let item = PALETTE_ITEMS
                .iter()
                .find(|item| item.command == command)
                .unwrap();
            assert_eq!(
                item.shortcut,
                crate::config::KeyBindings::display(binding),
                "{command:?}"
            );
        }
    }

    /// Configurações e temas aparecem sem rolar e são achados em português.
    #[test]
    fn test_settings_and_themes_are_easy_to_find() {
        let first_rows: Vec<_> = PALETTE_ITEMS.iter().take(8).map(|i| i.command).collect();
        assert!(first_rows.contains(&PaletteCommand::OpenSettings));
        assert!(first_rows.contains(&PaletteCommand::ChooseTheme));
        let mut palette = CommandPalette::new();
        for query in ["theme", "tema", "cores"] {
            palette.open();
            for c in query.chars() {
                palette.type_char(c);
            }
            assert_eq!(
                palette.selected_item().map(|i| i.command),
                Some(PaletteCommand::ChooseTheme),
                "{query}"
            );
        }
        palette.open();
        for c in "config".chars() {
            palette.type_char(c);
        }
        assert_eq!(
            palette.selected_item().map(|i| i.command),
            Some(PaletteCommand::OpenSettings)
        );
    }

    #[test]
    fn test_hex_color_mode_offers_apply_only_for_valid_colors() {
        let mut palette = CommandPalette::new();
        palette.open_hex_color("#12");
        assert_eq!(palette.mode(), PaletteMode::HexColor);
        assert!(palette.items().is_empty());
        assert_eq!(palette.hex_color(), None);
        palette.type_char('3');
        assert_eq!(palette.hex_color(), Some([0x11, 0x22, 0x33]));
        assert_eq!(
            palette.selected_item().map(|i| i.command),
            Some(PaletteCommand::ApplyHexColor)
        );
        palette.open();
        assert_eq!(palette.hex_color(), None);
    }

    /// A busca acha pelo nome nos dois idiomas, e o rótulo segue o idioma
    /// em uso (#28).
    #[test]
    fn test_search_finds_commands_in_both_languages() {
        use crate::ui::i18n::set_language;
        set_language(Language::English);
        let mut palette = CommandPalette::new();
        palette.open();
        for c in "nova aba".chars() {
            palette.type_char(c);
        }
        assert_eq!(
            palette.selected_item().map(|i| (i.command, i.label)),
            Some((PaletteCommand::NewTab, "New Tab"))
        );
        set_language(Language::PortugueseBr);
        palette.open();
        for c in "new tab".chars() {
            palette.type_char(c);
        }
        assert_eq!(
            palette.selected_item().map(|i| (i.command, i.label)),
            Some((PaletteCommand::NewTab, "Nova aba"))
        );
        assert_eq!(palette.placeholder(), "Digite um comando...");
        set_language(Language::English);
    }

    #[test]
    fn test_theme_mode_lists_all_themes_and_marks_the_current_one() {
        crate::ui::i18n::set_language(Language::English);
        let mut palette = CommandPalette::new();
        palette.open_themes("NORD");
        assert_eq!(palette.mode(), PaletteMode::Themes);
        assert_eq!(palette.items().len(), THEMES.len());
        assert_eq!(THEMES.len(), 18);
        assert_eq!(palette.selected_theme(), Some("nord"));
        assert_eq!(palette.selected_item().unwrap().shortcut, "current");
        for c in "rose".chars() {
            palette.type_char(c);
        }
        assert_eq!(palette.items().len(), 2);
        palette.scroll_by(5);
        assert_eq!(palette.selected_theme(), Some("rose-pine-dawn"));
        palette.open();
        assert_eq!(palette.mode(), PaletteMode::Commands);
        assert_eq!(palette.selected_theme(), None);
    }

    #[test]
    fn test_palette_initially_closed() {
        let palette = CommandPalette::new();
        assert!(!palette.is_visible());
        assert!(!palette.items().is_empty());
    }

    #[test]
    fn test_palette_open_close() {
        let mut palette = CommandPalette::new();
        palette.open();
        assert!(palette.is_visible());
        palette.close();
        assert!(!palette.is_visible());
    }

    #[test]
    fn test_palette_toggle() {
        let mut palette = CommandPalette::new();
        palette.toggle();
        assert!(palette.is_visible());
        palette.toggle();
        assert!(!palette.is_visible());
    }

    #[test]
    fn test_palette_filter() {
        let mut palette = CommandPalette::new();
        palette.open();
        palette.type_char('n');
        palette.type_char('e');
        palette.type_char('w');
        assert!(palette
            .items()
            .iter()
            .all(|item| item.label.to_lowercase().contains("new")));
    }

    #[test]
    fn test_palette_navigation() {
        let mut palette = CommandPalette::new();
        palette.open();
        let first = palette.selected_item().cloned();
        palette.move_down();
        let second = palette.selected_item().cloned();
        assert_ne!(first, second);
        palette.move_up();
        assert_eq!(palette.selected_item(), first.as_ref());
    }

    #[test]
    fn test_palette_confirm() {
        let mut palette = CommandPalette::new();
        palette.open();
        let command = palette.confirm();
        assert!(command.is_some());
        assert!(!palette.is_visible());
    }

    #[test]
    fn test_palette_confirm_empty_after_filter() {
        let mut palette = CommandPalette::new();
        palette.open();
        for c in "zzzzzzz".chars() {
            palette.type_char(c);
        }
        assert!(palette.items().is_empty());
        let command = palette.confirm();
        assert!(command.is_none());
    }

    #[test]
    fn select_and_scroll_by_stay_inside_the_list() {
        let mut palette = CommandPalette::new();
        palette.open();
        let last = palette.items().len() - 1;
        palette.select(2);
        assert_eq!(palette.selected_index, 2);
        palette.select(last + 5);
        assert_eq!(palette.selected_index, 2, "índice fora da lista é ignorado");
        palette.scroll_by(-10);
        assert_eq!(palette.selected_index, 0, "não dá a volta para o fim");
        palette.scroll_by(3);
        assert_eq!(palette.selected_index, 3);
        palette.scroll_by(1000);
        assert_eq!(palette.selected_index, last, "não dá a volta para o começo");
    }
}
