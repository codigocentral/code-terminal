//! Mouse fora dos painéis modais: o que está sob o cursor (para mostrar os
//! botões × e ⋯ do cartão e o × do painel) e fechar tab ou painel pelo mouse.

use super::AppState;
use crate::ui::hover::{FooterButton, Hover};
use crate::ui::pane_layout::PaneId;

impl AppState {
    /// O que está sob o mouse.
    pub fn hover(&self) -> Hover {
        self.hover
    }

    /// Guarda o que está sob o mouse. Devolve `true` se mudou (precisa
    /// redesenhar para mostrar ou esconder os botões).
    pub fn set_hover(&mut self, hover: Hover) -> bool {
        std::mem::replace(&mut self.hover, hover) != hover
    }

    /// Fecha a tab `index` pelo mouse (×, clique do meio, menu): pede
    /// confirmação se houver programa rodando. Devolve `true` se não sobrou tab.
    pub fn close_tab_from_mouse(&mut self, index: usize) -> bool {
        let empty = self.close_tab(index);
        self.forget_closed_panes();
        self.refresh_sidebar(true);
        self.hover = Hover::None;
        empty
    }

    /// × de um painel da tab ativa: com programa rodando (e a confirmação
    /// ligada) pergunta antes, como ao fechar a tab. Devolve `true` se não
    /// sobrou tab.
    pub fn request_close_pane(&mut self, pane: PaneId) -> bool {
        let busy = self
            .active_tab()
            .and_then(|tab| tab.pane_buffer(pane))
            .and_then(|buffer| buffer.pty().foreground_process());
        self.hover = Hover::None;
        if busy.is_some() && self.config.window.confirm_close_tab {
            self.pending_close_pane = Some(pane);
            return false;
        }
        let empty = self.close_pane(pane);
        self.forget_closed_panes();
        self.refresh_sidebar(true);
        empty
    }

    /// Clique num botão do rodapé da sidebar.
    pub fn footer_action(&mut self, button: FooterButton) {
        self.hover = Hover::None;
        match button {
            FooterButton::Themes => self.open_theme_chooser(),
            FooterButton::Ssh => self.show_quick_connect(),
            FooterButton::Browser => self.open_browser_window(),
            FooterButton::Notifications => self.toggle_notifications_panel(),
            FooterButton::Ai => self.ai_open_suggest(),
            FooterButton::Settings => self.show_settings(),
        }
    }

    /// Programa rodando no painel esperando confirmação de fechar.
    pub fn pending_close_pane_program(&self) -> Option<String> {
        let pane = self.pending_close_pane?;
        self.active_tab()
            .and_then(|tab| tab.pane_buffer(pane))
            .and_then(|buffer| buffer.pty().foreground_process())
    }

    /// Se a confirmação aberta é de um painel (senão é de uma tab).
    pub fn is_pending_close_pane(&self) -> bool {
        self.pending_close_pane.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ui::hover::CardButton;
    use crate::ui::pane_layout::SplitDirection;

    fn app_with_split() -> AppState {
        let mut app = AppState::with_command(
            Config::default(),
            80,
            24,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap();
        app.split_active_pane_running(
            SplitDirection::Vertical,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap();
        app
    }

    #[test]
    fn set_hover_reports_changes_only() {
        let mut app = AppState::new(40, 5).unwrap();
        assert_eq!(app.hover(), Hover::None);
        let card = Hover::Card {
            tab: 1,
            button: None,
        };
        assert!(app.set_hover(card));
        assert!(!app.set_hover(card), "o mesmo alvo não redesenha");
        assert!(app.set_hover(Hover::Card {
            tab: 1,
            button: Some(CardButton::Close),
        }));
        assert!(app.set_hover(Hover::None));
    }

    #[cfg(unix)]
    #[test]
    fn close_pane_by_id_closes_that_pane_and_keeps_the_tab() {
        let mut app = app_with_split();
        let tab = app.active_tab().unwrap();
        assert_eq!(tab.pane_count(), 2);
        let focused = tab.focused_pane();
        let other = tab
            .layout()
            .rects(app.pane_area())
            .into_iter()
            .map(|(id, _)| id)
            .find(|&id| id != focused)
            .unwrap();
        // Sem programa em primeiro plano reconhecido, fecha direto.
        app.config.window.confirm_close_tab = false;
        assert!(!app.request_close_pane(other));
        let tab = app.active_tab().unwrap();
        assert_eq!(tab.pane_count(), 1);
        assert_eq!(tab.focused_pane(), focused);
        // Painel que não existe não faz nada.
        assert!(!app.close_pane(other));
        // O último painel fecha a tab (e a janela, se era a única).
        assert!(app.request_close_pane(focused));
    }

    #[cfg(unix)]
    #[test]
    fn pane_confirmation_cancel_and_confirm() {
        let mut app = app_with_split();
        let pane = app.active_tab().unwrap().focused_pane();
        app.pending_close_pane = Some(pane);
        assert!(app.has_pending_close_tab());
        assert!(app.is_pending_close_pane());
        app.cancel_close_pending_tab();
        assert!(!app.has_pending_close_tab());
        assert_eq!(app.active_tab().unwrap().pane_count(), 2);
        app.pending_close_pane = Some(pane);
        assert!(!app.confirm_close_pending_tab());
        assert_eq!(app.active_tab().unwrap().pane_count(), 1);
        assert!(!app.has_pending_close_tab());
    }

    #[test]
    fn footer_buttons_open_their_panels() {
        let mut app = AppState::new(40, 5).unwrap();
        app.footer_action(FooterButton::Settings);
        assert!(app.is_settings_visible());
        app.hide_settings();
        app.footer_action(FooterButton::Themes);
        assert!(app.is_choosing_theme());
        app.hide_command_palette();
        app.footer_action(FooterButton::Ssh);
        assert!(app.is_quick_connect_visible());
        app.hide_quick_connect();
        app.footer_action(FooterButton::Notifications);
        assert!(app.is_notifications_panel_visible());
    }

    #[test]
    fn close_tab_from_mouse_keeps_other_tabs() {
        let mut app = AppState::new(40, 5).unwrap();
        app.config.window.confirm_close_tab = false;
        app.create_new_tab().unwrap();
        assert_eq!(app.tab_count(), 2);
        assert!(!app.close_tab_from_mouse(0));
        assert_eq!(app.tab_count(), 1);
    }
}
