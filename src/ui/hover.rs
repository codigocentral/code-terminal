//! O que está sob o mouse fora dos painéis modais: cartão da sidebar (com os
//! botões × e ⋯) e painel dividido (com o × de fechar).
//!
//! Os botões só aparecem com o mouse em cima. Desenho (`window.rs`) e clique
//! (`main.rs`) usam os mesmos retângulos daqui.

use crate::ui::overlay_hits::Rect;
use crate::ui::pane_layout::PaneId;
use crate::ui::tabs::TabId;

/// Botão no canto do cartão de uma tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardButton {
    /// × fecha a tab (pede confirmação se houver programa rodando).
    Close,
    /// ⋯ abre o mesmo menu do botão direito.
    More,
}

/// O que está sob o mouse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Hover {
    #[default]
    None,
    /// Cartão da tab na sidebar; `button` quando o mouse está num botão.
    Card {
        tab: TabId,
        button: Option<CardButton>,
    },
    /// Painel de uma tab dividida; `close` com o mouse no ×.
    Pane { pane: PaneId, close: bool },
    /// Botão do rodapé da sidebar.
    Footer(FooterButton),
}

/// Atalhos do rodapé da sidebar, na ordem em que aparecem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterButton {
    Themes,
    Ssh,
    Browser,
    Notifications,
    Ai,
    Settings,
}

impl FooterButton {
    pub const ALL: [FooterButton; 6] = [
        FooterButton::Themes,
        FooterButton::Ssh,
        FooterButton::Browser,
        FooterButton::Notifications,
        FooterButton::Ai,
        FooterButton::Settings,
    ];

    /// Ícone (glifo das fontes de símbolos comuns).
    pub fn icon(self) -> &'static str {
        match self {
            FooterButton::Themes => "\u{25d0}",
            FooterButton::Ssh => "\u{21c4}",
            FooterButton::Browser => "\u{25ce}",
            FooterButton::Notifications => "\u{2709}",
            FooterButton::Ai => "\u{2726}",
            FooterButton::Settings => "\u{2699}",
        }
    }

    /// Dica que aparece com o mouse em cima.
    pub fn tooltip(self) -> &'static str {
        use crate::ui::i18n::{tr, Text};
        tr(match self {
            FooterButton::Themes => Text::FooterThemes,
            FooterButton::Ssh => Text::FooterSsh,
            FooterButton::Browser => Text::FooterBrowser,
            FooterButton::Notifications => Text::FooterNotifications,
            FooterButton::Ai => Text::FooterAi,
            FooterButton::Settings => Text::FooterSettings,
        })
    }
}

/// Altura do rodapé da sidebar.
pub const FOOTER_HEIGHT: f32 = 34.0;

/// Faixa do rodapé, no pé da sidebar.
pub fn footer_rect(sidebar_width: f32, window_height: f32) -> Rect {
    Rect::new(
        0.0,
        window_height - FOOTER_HEIGHT,
        sidebar_width - 1.0,
        FOOTER_HEIGHT,
    )
}

/// Botão `button` do rodapé: os seis dividem a largura, numa linha.
pub fn footer_button_rect(button: FooterButton, sidebar_width: f32, window_height: f32) -> Rect {
    let bar = footer_rect(sidebar_width, window_height);
    let index = FooterButton::ALL
        .iter()
        .position(|&b| b == button)
        .unwrap_or(0);
    let slot = (bar.width - 8.0) / FooterButton::ALL.len() as f32;
    Rect::new(
        bar.x + 4.0 + index as f32 * slot + 1.0,
        bar.y + 4.0,
        slot - 2.0,
        bar.height - 8.0,
    )
}

/// Botão do rodapé sob (x, y), se houver.
pub fn footer_button_at(
    sidebar_width: f32,
    window_height: f32,
    x: f32,
    y: f32,
) -> Option<FooterButton> {
    FooterButton::ALL
        .into_iter()
        .find(|&b| footer_button_rect(b, sidebar_width, window_height).contains(x, y))
}

/// Lado de cada botão do cartão.
pub const CARD_BUTTON_SIZE: f32 = 20.0;

/// Distância da borda direita da sidebar até o ×.
const CARD_BUTTON_RIGHT: f32 = 8.0;

/// Espaço entre os botões do cartão.
const CARD_BUTTON_GAP: f32 = 4.0;

/// Largura que os dois botões ocupam no topo do cartão (o título encolhe).
pub const CARD_BUTTONS_WIDTH: f32 = 2.0 * CARD_BUTTON_SIZE + CARD_BUTTON_GAP + CARD_BUTTON_RIGHT;

/// Onde fica o botão no cartão que começa em `row_y`, na sidebar de largura
/// `sidebar_width`: × no canto de cima à direita, ⋯ ao lado.
pub fn card_button_rect(button: CardButton, row_y: f32, sidebar_width: f32) -> Rect {
    let close_x = sidebar_width - CARD_BUTTON_RIGHT - CARD_BUTTON_SIZE;
    let x = match button {
        CardButton::Close => close_x,
        CardButton::More => close_x - CARD_BUTTON_GAP - CARD_BUTTON_SIZE,
    };
    Rect::new(x, row_y + 6.0, CARD_BUTTON_SIZE, CARD_BUTTON_SIZE)
}

/// Botão do cartão sob (x, y), se houver.
pub fn card_button_at(row_y: f32, sidebar_width: f32, x: f32, y: f32) -> Option<CardButton> {
    [CardButton::Close, CardButton::More]
        .into_iter()
        .find(|&button| card_button_rect(button, row_y, sidebar_width).contains(x, y))
}

/// Lado do × de um painel.
pub const PANE_CLOSE_SIZE: f32 = 18.0;

/// × na ponta direita da barra de título do painel que começa em (x, y),
/// com `width` pixels de largura e barra de `bar_height` pixels (#32).
pub fn pane_close_rect(x: f32, y: f32, width: f32, bar_height: f32) -> Rect {
    let size = PANE_CLOSE_SIZE.min(bar_height - 2.0).max(8.0);
    Rect::new(
        x + width - size - 4.0,
        y + ((bar_height - size) / 2.0).max(0.0),
        size,
        size,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_buttons_sit_top_right_and_do_not_overlap() {
        let close = card_button_rect(CardButton::Close, 100.0, 240.0);
        let more = card_button_rect(CardButton::More, 100.0, 240.0);
        assert_eq!(close.x + close.width, 240.0 - CARD_BUTTON_RIGHT);
        assert!(more.x + more.width <= close.x);
        assert_eq!(close.y, 106.0);
        assert!(240.0 - more.x <= CARD_BUTTONS_WIDTH);
    }

    #[test]
    fn card_button_hit_test() {
        let (y, w) = (100.0, 240.0);
        let close = card_button_rect(CardButton::Close, y, w);
        let more = card_button_rect(CardButton::More, y, w);
        assert_eq!(
            card_button_at(y, w, close.x + 1.0, close.y + 1.0),
            Some(CardButton::Close)
        );
        assert_eq!(
            card_button_at(y, w, more.x + more.width - 1.0, more.y + 10.0),
            Some(CardButton::More)
        );
        // Entre os dois, abaixo deles, no título e no cartão de cima.
        assert_eq!(
            card_button_at(y, w, more.x + more.width + 1.0, close.y + 5.0),
            None
        );
        assert_eq!(
            card_button_at(y, w, close.x + 5.0, close.y + close.height),
            None
        );
        assert_eq!(card_button_at(y, w, 20.0, y + 10.0), None);
        assert_eq!(card_button_at(y, w, close.x + 5.0, y - 10.0), None);
    }

    #[test]
    fn footer_buttons_fill_one_row_and_hit_test() {
        let (w, h) = (240.0, 700.0);
        let bar = footer_rect(w, h);
        assert_eq!(bar.y + bar.height, h);
        let mut last_right = 0.0;
        for button in FooterButton::ALL {
            let rect = footer_button_rect(button, w, h);
            assert!(rect.x >= last_right, "{button:?} sobrepõe o anterior");
            assert!(rect.x + rect.width <= w);
            assert!(rect.y >= bar.y && rect.y + rect.height <= h);
            last_right = rect.x + rect.width;
            assert_eq!(
                footer_button_at(w, h, rect.x + rect.width / 2.0, rect.y + 2.0),
                Some(button)
            );
            assert!(!button.tooltip().is_empty() && !button.icon().is_empty());
        }
        assert_eq!(footer_button_at(w, h, 50.0, bar.y - 5.0), None);
        assert_eq!(footer_button_at(w, h, w + 5.0, h - 10.0), None);
    }

    #[test]
    fn pane_close_is_inside_the_pane_top_right() {
        let rect = pane_close_rect(300.0, 40.0, 400.0, 20.0);
        assert!(rect.x + rect.width <= 700.0 && rect.x > 600.0);
        // Dentro da barra de título (20 px).
        assert!(rect.y >= 40.0 && rect.y + rect.height <= 60.0);
        assert!(rect.contains(690.0, 50.0));
        assert!(!rect.contains(650.0, 50.0));
    }
}
