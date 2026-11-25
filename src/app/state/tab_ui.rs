//! Tabs na sidebar: renomear no próprio cartão, menu de contexto (botão
//! direito) e arrastar para aninhar/desaninhar.
//!
//! O modelo dos grupos fica em [`crate::ui::tabs::TabManager`]; aqui ficam o
//! estado da interação (o que está sendo editado, o menu aberto, o arraste)
//! e as ações que mexem no `AppState` (atualizar a sidebar depois de mudar a
//! ordem, fechar tab com confirmação).

use super::AppState;
use crate::ui::i18n::{tr, Text};
use crate::ui::line_edit::{LineEdit, LineEditKey};
use crate::ui::tab_colors::ColorTarget;
use crate::ui::tabs::{DropTarget, TabId};
use std::time::{Duration, Instant};

/// Linhas do menu de contexto visíveis de uma vez (o resto rola).
pub const TAB_MENU_ROWS: usize = 10;

/// Dois cliques no mesmo cartão dentro deste intervalo = duplo clique.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// O mouse precisa andar isto (pixels) com o botão apertado para virar arraste.
const DRAG_THRESHOLD: f32 = 6.0;

/// Título de tab em edição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabRename {
    pub tab: TabId,
    pub edit: LineEdit,
    /// Título que aparecia quando a edição começou.
    original: String,
    /// Já era um título dado pelo usuário (senão era o automático).
    had_custom: bool,
}

/// O que um item do menu de contexto faz.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabMenuAction {
    Rename,
    NestInto(TabId),
    Unnest,
    Collapse,
    Expand,
    /// Tornar persistente / deixar de ser persistente.
    TogglePersistent,
    /// Retomar o programa sozinho depois de reiniciar a máquina.
    ToggleAutoResume,
    Close,
    /// Abre, no mesmo menu, a lista de cores (#24).
    Colors(ColorTarget),
    /// Aplica (ou tira, com `None`) a cor.
    SetColor(ColorTarget, Option<[u8; 3]>),
    /// Abre o campo para digitar a cor em hex.
    CustomColor(ColorTarget),
    /// Volta da lista de cores para o menu da tab.
    Back,
}

/// Item do menu: ação e texto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMenuEntry {
    pub action: TabMenuAction,
    pub label: String,
}

/// Menu de contexto de uma tab, aberto no ponto do clique.
#[derive(Debug, Clone, PartialEq)]
pub struct TabMenu {
    pub tab: TabId,
    pub x: f32,
    pub y: f32,
    pub entries: Vec<TabMenuEntry>,
    pub selected: usize,
    /// Primeiro item visível (quando há mais de [`TAB_MENU_ROWS`]).
    pub first: usize,
}

impl TabMenu {
    fn select(&mut self, index: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = index.min(self.entries.len() - 1);
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + TAB_MENU_ROWS {
            self.first = self.selected + 1 - TAB_MENU_ROWS;
        }
    }
}

/// Tab sendo arrastada na sidebar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarDrag {
    pub tab: TabId,
    pub start: (f32, f32),
    pub pos: (f32, f32),
    /// Passou do limite: é arraste (antes disso ainda é clique).
    pub dragging: bool,
}

/// Estado da interação com a sidebar.
#[derive(Debug, Default)]
pub(super) struct SidebarUi {
    rename: Option<TabRename>,
    menu: Option<TabMenu>,
    drag: Option<SidebarDrag>,
    last_click: Option<(TabId, Instant)>,
}

/// Corta o título para caber num item do menu.
fn short(title: &str, max: usize) -> String {
    if title.chars().count() <= max {
        return title.to_string();
    }
    let cut: String = title.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

impl AppState {
    fn tab_id_at(&self, index: usize) -> Option<TabId> {
        self.tab_manager.tabs().get(index).map(|tab| tab.id)
    }

    /// A ordem das tabs mudou: a sidebar (alinhada por índice) é refeita já.
    fn tabs_reordered(&mut self) {
        self.refresh_sidebar(true);
    }

    // === Renomear ===

    /// Começa a editar o título da tab `index` no cartão da sidebar (mostra
    /// a sidebar se estiver escondida).
    pub fn start_tab_rename(&mut self, index: usize) -> bool {
        let Some(tab) = self.tab_manager.tabs().get(index) else {
            return false;
        };
        let original = tab.title();
        let rename = TabRename {
            tab: tab.id,
            edit: LineEdit::new(&original),
            had_custom: tab.custom_title().is_some(),
            original,
        };
        self.sidebar_ui.menu = None;
        self.sidebar_ui.rename = Some(rename);
        if !self.sidebar_visible {
            self.sidebar_visible = true;
            self.refresh_sidebar(true);
        }
        true
    }

    /// Começa a renomear a tab ativa.
    pub fn start_rename_active_tab(&mut self) -> bool {
        self.start_tab_rename(self.tab_manager.active_tab_index())
    }

    /// Título em edição, se houver.
    pub fn tab_rename(&self) -> Option<&TabRename> {
        self.sidebar_ui.rename.as_ref()
    }

    pub fn is_renaming_tab(&self) -> bool {
        self.sidebar_ui.rename.is_some()
    }

    /// Tecla no campo de edição do título.
    pub fn tab_rename_key(&mut self, key: LineEditKey) {
        if let Some(rename) = self.sidebar_ui.rename.as_mut() {
            rename.edit.apply(key);
        }
    }

    /// Cola texto no campo de edição do título.
    pub fn tab_rename_paste(&mut self, text: &str) {
        if let Some(rename) = self.sidebar_ui.rename.as_mut() {
            rename.edit.insert_str(text);
        }
    }

    /// Cola a área de transferência no campo de edição do título.
    pub fn tab_rename_paste_clipboard(&mut self) {
        let text = self.clipboard.as_ref().and_then(|c| c.get_text().ok());
        if let Some(text) = text {
            self.tab_rename_paste(&text);
        }
    }

    /// Salva o título digitado (Enter, clique fora). Vazio volta ao título
    /// automático; o automático confirmado sem mudança não vira fixo.
    pub fn commit_tab_rename(&mut self) -> bool {
        let Some(rename) = self.sidebar_ui.rename.take() else {
            return false;
        };
        let text = rename.edit.text();
        let text = text.trim();
        let Some(index) = self.tab_manager.tab_index_of(rename.tab) else {
            return false;
        };
        if !rename.had_custom && text == rename.original.trim() {
            return false;
        }
        if let Some(tab) = self.tab_manager.get_tab_mut(index) {
            tab.set_custom_title(Some(text));
        }
        self.refresh_sidebar(true);
        true
    }

    /// Desiste da edição (Esc): o título fica como estava.
    pub fn cancel_tab_rename(&mut self) {
        self.sidebar_ui.rename = None;
    }

    /// Define (ou limpa, com `None`/vazio) o título fixo da tab `index`.
    pub fn rename_tab(&mut self, index: usize, title: Option<&str>) -> bool {
        let Some(tab) = self.tab_manager.get_tab_mut(index) else {
            return false;
        };
        tab.set_custom_title(title);
        self.refresh_sidebar(true);
        true
    }

    // === Grupos ===

    /// Põe a tab `child` no grupo da tab `parent`.
    pub fn nest_tab(&mut self, child: usize, parent: usize) -> bool {
        let changed = self.tab_manager.nest_tab(child, parent);
        if changed {
            self.tabs_reordered();
        }
        changed
    }

    /// Tira a tab `index` do grupo.
    pub fn unnest_tab(&mut self, index: usize) -> bool {
        let changed = self.tab_manager.unnest_tab(index);
        if changed {
            self.tabs_reordered();
        }
        changed
    }

    /// Recolhe/abre o grupo da tab `index`.
    pub fn toggle_tab_group(&mut self, index: usize) -> bool {
        let changed = self.tab_manager.toggle_collapsed(index);
        if changed {
            self.tabs_reordered();
        }
        changed
    }

    /// Atalho Ctrl+N: a `n`-ésima linha da sidebar (pula grupos recolhidos).
    pub fn go_to_visible_tab(&mut self, n: usize) {
        if let Some(index) = self.tab_manager.visible_tab_at(n) {
            self.go_to_tab(index);
        }
    }

    // === Menu de contexto ===

    /// Abre o menu da tab `index` no ponto (x, y) da janela.
    pub fn open_tab_menu(&mut self, index: usize, x: f32, y: f32) -> bool {
        let Some(tab) = self.tab_manager.tabs().get(index) else {
            return false;
        };
        let tab_id = tab.id;
        let mut entries = vec![TabMenuEntry {
            action: TabMenuAction::Rename,
            label: tr(Text::MenuRename).to_string(),
        }];
        if self.tab_manager.parent_index(index).is_some() {
            entries.push(TabMenuEntry {
                action: TabMenuAction::Unnest,
                label: tr(Text::MenuUnnest).to_string(),
            });
        }
        if self.tab_manager.child_count(index) > 0 {
            entries.push(if tab.is_collapsed() {
                TabMenuEntry {
                    action: TabMenuAction::Expand,
                    label: tr(Text::MenuExpand).to_string(),
                }
            } else {
                TabMenuEntry {
                    action: TabMenuAction::Collapse,
                    label: tr(Text::MenuCollapse).to_string(),
                }
            });
        }
        for candidate in self.tab_manager.nest_candidates(index) {
            let other = &self.tab_manager.tabs()[candidate];
            entries.push(TabMenuEntry {
                action: TabMenuAction::NestInto(other.id),
                label: format!(
                    "{} \u{203a} {}",
                    tr(Text::MenuNestInto),
                    short(&other.title(), 28)
                ),
            });
        }
        if self.can_toggle_persistent(index) {
            entries.push(TabMenuEntry {
                action: TabMenuAction::TogglePersistent,
                label: if self.is_tab_persistent(index) {
                    tr(Text::MenuStopPersistent).to_string()
                } else {
                    tr(Text::MenuMakePersistent).to_string()
                },
            });
        }
        if self.is_tab_persistent(index) {
            entries.push(TabMenuEntry {
                action: TabMenuAction::ToggleAutoResume,
                label: if self.tab_auto_resumes(index) {
                    tr(Text::MenuAutoResumeOn).to_string()
                } else {
                    tr(Text::MenuAutoResumeOff).to_string()
                },
            });
        }
        entries.extend(Self::color_menu_items());
        entries.push(TabMenuEntry {
            action: TabMenuAction::Close,
            label: tr(Text::MenuClose).to_string(),
        });
        self.commit_tab_rename();
        self.sidebar_ui.menu = Some(TabMenu {
            tab: tab_id,
            x,
            y,
            entries,
            selected: 0,
            first: 0,
        });
        true
    }

    pub fn tab_menu(&self) -> Option<&TabMenu> {
        self.sidebar_ui.menu.as_ref()
    }

    pub fn is_tab_menu_visible(&self) -> bool {
        self.sidebar_ui.menu.is_some()
    }

    pub fn close_tab_menu(&mut self) {
        self.sidebar_ui.menu = None;
    }

    /// Destaca o item `index` (mouse em cima).
    pub fn select_tab_menu_item(&mut self, index: usize) {
        if let Some(menu) = self.sidebar_ui.menu.as_mut() {
            menu.select(index);
        }
    }

    /// Move a seleção `delta` itens (setas, roda), parando nas pontas.
    pub fn move_tab_menu_selection(&mut self, delta: i32) {
        if let Some(menu) = self.sidebar_ui.menu.as_mut() {
            let target = menu.selected as i64 + i64::from(delta);
            menu.select(target.max(0) as usize);
        }
    }

    /// Executa o item `index` do menu e fecha o menu. Devolve `true` se a
    /// janela ficou sem tabs (fechou a última).
    pub fn activate_tab_menu_item(&mut self, index: usize) -> bool {
        let Some(menu) = self.sidebar_ui.menu.take() else {
            return false;
        };
        let Some(action) = menu.entries.get(index).map(|entry| entry.action) else {
            return false;
        };
        let Some(tab) = self.tab_manager.tab_index_of(menu.tab) else {
            return false;
        };
        match action {
            TabMenuAction::Rename => {
                self.start_tab_rename(tab);
            }
            TabMenuAction::Unnest => {
                self.unnest_tab(tab);
            }
            TabMenuAction::Collapse | TabMenuAction::Expand => {
                self.toggle_tab_group(tab);
            }
            TabMenuAction::NestInto(parent) => {
                if let Some(parent) = self.tab_manager.tab_index_of(parent) {
                    self.nest_tab(tab, parent);
                }
            }
            TabMenuAction::TogglePersistent => self.toggle_tab_persistent_notify(tab),
            TabMenuAction::ToggleAutoResume => {
                self.toggle_tab_auto_resume(tab);
            }
            TabMenuAction::Close => {
                let empty = self.close_tab(tab);
                self.forget_closed_panes();
                self.refresh_sidebar(true);
                return empty;
            }
            TabMenuAction::Colors(target) => {
                // A lista de cores abre no lugar do menu, no mesmo ponto.
                self.sidebar_ui.menu = Some(TabMenu {
                    entries: self.color_entries(tab, target),
                    selected: 0,
                    first: 0,
                    ..menu
                });
            }
            TabMenuAction::SetColor(target, color) => {
                self.set_tab_color(tab, target, color);
            }
            TabMenuAction::CustomColor(target) => {
                self.start_custom_tab_color(tab, target);
            }
            TabMenuAction::Back => {
                self.open_tab_menu(tab, menu.x, menu.y);
            }
        }
        false
    }

    /// Enter no menu: executa o item destacado.
    pub fn activate_selected_tab_menu_item(&mut self) -> bool {
        let selected = self.sidebar_ui.menu.as_ref().map(|menu| menu.selected);
        selected.is_some_and(|index| self.activate_tab_menu_item(index))
    }

    // === Clique e arraste ===

    /// Clique (botão esquerdo apertado) no cartão da tab `index`: ativa a tab
    /// e prepara um possível arraste. Devolve `true` no segundo clique de um
    /// duplo clique no mesmo cartão.
    pub fn sidebar_press(&mut self, index: usize, x: f32, y: f32) -> bool {
        self.sidebar_press_at(index, x, y, Instant::now())
    }

    fn sidebar_press_at(&mut self, index: usize, x: f32, y: f32, now: Instant) -> bool {
        let Some(tab) = self.tab_id_at(index) else {
            return false;
        };
        self.go_to_tab(index);
        self.sidebar_ui.drag = Some(SidebarDrag {
            tab,
            start: (x, y),
            pos: (x, y),
            dragging: false,
        });
        let double = self
            .sidebar_ui
            .last_click
            .is_some_and(|(last, at)| last == tab && now.duration_since(at) <= DOUBLE_CLICK);
        self.sidebar_ui.last_click = if double { None } else { Some((tab, now)) };
        double
    }

    /// Arraste em andamento (ou botão apertado sobre um cartão).
    pub fn sidebar_drag(&self) -> Option<&SidebarDrag> {
        self.sidebar_ui.drag.as_ref()
    }

    /// Mouse andou com o botão apertado. Devolve se há um arraste visível.
    pub fn update_sidebar_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(drag) = self.sidebar_ui.drag.as_mut() else {
            return false;
        };
        drag.pos = (x, y);
        let (dx, dy) = (x - drag.start.0, y - drag.start.1);
        if !drag.dragging && (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD {
            drag.dragging = true;
            // Arrastar não é a primeira metade de um duplo clique.
            self.sidebar_ui.last_click = None;
        }
        drag.dragging
    }

    /// Botão solto: aplica o arraste em `target` (`None` = solto fora da
    /// sidebar, desiste). Devolve se a ordem ou os grupos mudaram.
    pub fn finish_sidebar_drag(&mut self, target: Option<DropTarget>) -> bool {
        let Some(drag) = self.sidebar_ui.drag.take() else {
            return false;
        };
        let (true, Some(target)) = (drag.dragging, target) else {
            return false;
        };
        let Some(index) = self.tab_manager.tab_index_of(drag.tab) else {
            return false;
        };
        let changed = self.tab_manager.drop_tab(index, target);
        if changed {
            self.tabs_reordered();
        }
        changed
    }
}

// Os testes criam um AppState de verdade (um PTY com `/bin/sh`), trocado
// logo por tabs sem PTY.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ui::tabs::TabManager;

    /// App com `n` tabs sem PTY (só o modelo).
    fn app(n: usize) -> AppState {
        let mut app = AppState::with_command(
            Config::default(),
            80,
            24,
            "/bin/sh",
            &["-c".into(), "true".into()],
        )
        .unwrap();
        app.tab_manager = TabManager::new();
        app.desktop_notifications = false;
        for _ in 0..n {
            app.tab_manager.create_tab("bash");
        }
        app
    }

    fn titles(app: &AppState) -> Vec<String> {
        app.tab_manager.tabs().iter().map(|t| t.title()).collect()
    }

    #[test]
    fn test_rename_enter_saves_and_is_fixed() {
        let mut app = app(2);
        let auto = app.tab_manager.tabs()[1].title();
        assert!(app.start_tab_rename(1));
        assert!(app.tab_rename().unwrap().edit.is_all_selected());
        for c in "api".chars() {
            app.tab_rename_key(LineEditKey::Char(c));
        }
        assert!(app.commit_tab_rename());
        assert!(!app.is_renaming_tab());
        assert_eq!(titles(&app)[1], "api");
        // O processo mudar não troca o título dado pelo usuário.
        app.tab_manager.update_title(1, "vim");
        assert_eq!(titles(&app)[1], "api");
        // Vazio volta ao automático.
        app.start_tab_rename(1);
        app.tab_rename_key(LineEditKey::Backspace);
        app.commit_tab_rename();
        assert!(app.tab_manager.tabs()[1].custom_title().is_none());
        assert!(titles(&app)[1].contains("vim"));
        assert_ne!(titles(&app)[1], auto);
    }

    /// O programa troca o título (OSC 0), mas o título do usuário fica.
    #[test]
    fn test_custom_title_wins_over_program_title() {
        let mut app = AppState::with_command(
            Config::default(),
            80,
            24,
            "/bin/sh",
            &[
                "-c".into(),
                "printf '\\033]0;do-programa\\007'; sleep 30".into(),
            ],
        )
        .unwrap();
        let title = |app: &AppState| app.active_tab().unwrap().title();
        let deadline = Instant::now() + Duration::from_secs(5);
        while title(&app) != "do-programa" && Instant::now() < deadline {
            app.process_output();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(title(&app), "do-programa");
        app.rename_tab(0, Some("meu"));
        assert_eq!(title(&app), "meu");
        app.rename_tab(0, Some(""));
        assert_eq!(title(&app), "do-programa");
    }

    #[test]
    fn test_rename_escape_keeps_title() {
        let mut app = app(1);
        let before = titles(&app);
        app.start_rename_active_tab();
        app.tab_rename_key(LineEditKey::Char('z'));
        app.cancel_tab_rename();
        assert_eq!(titles(&app), before);
        assert!(app.tab_manager.tabs()[0].custom_title().is_none());
    }

    #[test]
    fn test_confirming_unchanged_auto_title_does_not_fix_it() {
        let mut app = app(1);
        app.start_rename_active_tab();
        assert!(!app.commit_tab_rename());
        assert!(app.tab_manager.tabs()[0].custom_title().is_none());
        // Mas um título fixo confirmado igual continua fixo.
        app.rename_tab(0, Some("  meu  "));
        assert_eq!(app.tab_manager.tabs()[0].custom_title(), Some("meu"));
        app.start_rename_active_tab();
        app.tab_rename_key(LineEditKey::End);
        app.tab_rename_paste("\nx");
        app.commit_tab_rename();
        assert_eq!(titles(&app)[0], "meu x");
    }

    #[test]
    fn test_rename_shows_hidden_sidebar_and_hiding_commits() {
        let mut app = app(1);
        app.sidebar_visible = false;
        app.start_rename_active_tab();
        assert!(app.is_sidebar_visible());
        app.tab_rename_key(LineEditKey::Char('k'));
        app.toggle_sidebar();
        assert!(!app.is_renaming_tab());
        assert_eq!(titles(&app)[0], "k");
        assert!(!app.start_tab_rename(9));
    }

    #[test]
    fn test_menu_entries_depend_on_group() {
        let mut app = app(3);
        app.nest_tab(1, 0);
        app.open_tab_menu(0, 10.0, 20.0);
        let actions: Vec<_> = app
            .tab_menu()
            .unwrap()
            .entries
            .iter()
            .map(|e| e.action)
            .collect();
        let ids: Vec<TabId> = app.tab_manager.tabs().iter().map(|t| t.id).collect();
        let mut expected = vec![
            TabMenuAction::Rename,
            TabMenuAction::Collapse,
            TabMenuAction::NestInto(ids[2]),
        ];
        // "Tornar persistente" só onde dá para entregar o shell ao daemon.
        if crate::app::state::adoption_supported() {
            expected.push(TabMenuAction::TogglePersistent);
        }
        expected.push(TabMenuAction::Colors(ColorTarget::Tab));
        expected.push(TabMenuAction::Colors(ColorTarget::Background));
        expected.push(TabMenuAction::Close);
        assert_eq!(actions, expected);
        app.open_tab_menu(1, 0.0, 0.0);
        let menu = app.tab_menu().unwrap();
        assert_eq!(menu.entries[1].action, TabMenuAction::Unnest);
        assert!(menu.entries[2].label.starts_with(tr(Text::MenuNestInto)));
        assert!(!app.open_tab_menu(7, 0.0, 0.0));
    }

    #[test]
    fn test_menu_actions() {
        let mut app = app(3);
        let ids: Vec<TabId> = app.tab_manager.tabs().iter().map(|t| t.id).collect();
        // Aninhar a 3ª na 1ª pelo menu.
        app.open_tab_menu(2, 0.0, 0.0);
        let nest = app
            .tab_menu()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.action == TabMenuAction::NestInto(ids[0]))
            .unwrap();
        assert!(!app.activate_tab_menu_item(nest));
        assert!(!app.is_tab_menu_visible());
        assert_eq!(app.tab_manager.parent_index(1), Some(0));
        // Recolher, expandir e tirar do grupo.
        app.open_tab_menu(0, 0.0, 0.0);
        app.activate_tab_menu_item(1);
        assert!(app.tab_manager.tabs()[0].is_collapsed());
        app.open_tab_menu(0, 0.0, 0.0);
        assert_eq!(
            app.tab_menu().unwrap().entries[1].action,
            TabMenuAction::Expand
        );
        app.activate_tab_menu_item(1);
        app.open_tab_menu(1, 0.0, 0.0);
        app.activate_tab_menu_item(1);
        assert_eq!(app.tab_manager.parent_index(1), None);
        assert_eq!(app.tab_manager.child_count(0), 0);
        // Renomear abre a edição.
        app.open_tab_menu(0, 0.0, 0.0);
        app.activate_tab_menu_item(0);
        assert_eq!(app.tab_rename().map(|r| r.tab), Some(ids[0]));
        // Fechar fecha (tab sem programa rodando: sem confirmação).
        app.cancel_tab_rename();
        app.open_tab_menu(0, 0.0, 0.0);
        let close = app.tab_menu().unwrap().entries.len() - 1;
        app.move_tab_menu_selection(99);
        assert_eq!(app.tab_menu().unwrap().selected, close);
        assert!(!app.activate_selected_tab_menu_item());
        assert_eq!(app.tab_count(), 2);
        assert!(!app.activate_tab_menu_item(0));
    }

    #[test]
    fn test_menu_scrolls_with_selection() {
        let mut app = app(TAB_MENU_ROWS + 4);
        app.open_tab_menu(0, 0.0, 0.0);
        let total = app.tab_menu().unwrap().entries.len();
        assert!(total > TAB_MENU_ROWS);
        app.select_tab_menu_item(total - 1);
        let menu = app.tab_menu().unwrap();
        assert_eq!(menu.first, total - TAB_MENU_ROWS);
        app.move_tab_menu_selection(-100);
        assert_eq!(app.tab_menu().unwrap().first, 0);
        app.close_tab_menu();
        app.move_tab_menu_selection(1);
        assert!(!app.is_tab_menu_visible());
    }

    #[test]
    fn test_double_click_detection() {
        let mut app = app(2);
        let t0 = Instant::now();
        assert!(!app.sidebar_press_at(0, 5.0, 50.0, t0));
        assert!(app.sidebar_press_at(0, 5.0, 50.0, t0 + Duration::from_millis(200)));
        // Terceiro clique começa outra contagem; outro cartão não conta.
        assert!(!app.sidebar_press_at(0, 5.0, 50.0, t0 + Duration::from_millis(300)));
        assert!(!app.sidebar_press_at(1, 5.0, 50.0, t0 + Duration::from_millis(350)));
        assert!(!app.sidebar_press_at(1, 5.0, 50.0, t0 + Duration::from_millis(900)));
        assert_eq!(app.active_tab_index(), 1);
    }

    #[test]
    fn test_drag_needs_threshold_then_applies() {
        let mut app = app(3);
        let ids: Vec<TabId> = app.tab_manager.tabs().iter().map(|t| t.id).collect();
        app.sidebar_press(2, 10.0, 200.0);
        assert!(!app.update_sidebar_drag(12.0, 202.0));
        // Soltar sem ter arrastado é só clique.
        assert!(!app.finish_sidebar_drag(Some(DropTarget::Nest(0))));
        assert!(app.sidebar_drag().is_none());

        app.sidebar_press(2, 10.0, 200.0);
        assert!(app.update_sidebar_drag(10.0, 40.0));
        assert!(app.finish_sidebar_drag(Some(DropTarget::Nest(0))));
        assert_eq!(app.tab_manager.tabs()[1].id, ids[2]);
        assert_eq!(app.tab_manager.parent_index(1), Some(0));

        // Solta fora da sidebar: desiste.
        app.sidebar_press(1, 10.0, 100.0);
        app.update_sidebar_drag(400.0, 100.0);
        assert!(!app.finish_sidebar_drag(None));
        // Arrasta para o espaço do fim: sai do grupo.
        app.sidebar_press(1, 10.0, 100.0);
        app.update_sidebar_drag(10.0, 300.0);
        assert!(app.finish_sidebar_drag(Some(DropTarget::Gap(3))));
        assert_eq!(app.tab_manager.parent_index(2), None);
        assert!(!app.finish_sidebar_drag(None));
    }

    #[test]
    fn test_group_wrappers_and_visible_shortcut() {
        let mut app = app(3);
        assert!(app.nest_tab(1, 0));
        assert!(!app.nest_tab(1, 0));
        assert!(app.toggle_tab_group(0));
        app.go_to_visible_tab(1);
        assert_eq!(app.active_tab_index(), 2);
        app.go_to_visible_tab(9);
        assert_eq!(app.active_tab_index(), 2);
        assert!(app.unnest_tab(1));
        assert!(!app.unnest_tab(1));
        assert_eq!(short("abcdef", 4), "abc…");
        assert_eq!(short("abc", 4), "abc");
    }
}
