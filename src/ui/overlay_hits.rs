//! Mouse sobre painéis abertos (paleta de comandos, seletor SSH, configurações...).
//!
//! Cada frame registra onde cada painel modal foi desenhado ([`OverlayHits`]).
//! O roteamento do mouse consulta esse registro: assim o retângulo do clique é
//! exatamente o que está na tela, sem repetir a conta do desenho.
//!
//! Com um painel aberto, o mouse é dele: a roda rola a lista do painel, o clique
//! num item ativa o item, o clique fora fecha o painel, e nada disso chega ao
//! terminal por trás (nem ao scrollback, nem ao programa do PTY).

/// Retângulo em pixels da janela.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Se o ponto (x, y) está dentro do retângulo.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// Lista rolável desenhada dentro de um painel: onde fica e que faixa aparece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListGeometry {
    /// Canto superior esquerdo da primeira linha visível.
    pub x: f32,
    pub y: f32,
    /// Largura da área clicável de cada linha.
    pub width: f32,
    /// Altura de cada linha.
    pub row_height: f32,
    /// Índice do primeiro item visível.
    pub first: usize,
    /// Quantos itens aparecem.
    pub visible: usize,
    /// Total de itens na lista.
    pub total: usize,
}

impl ListGeometry {
    /// Índice do item sob (x, y), se houver um item visível ali.
    pub fn item_at(&self, x: f32, y: f32) -> Option<usize> {
        if self.row_height <= 0.0 || x < self.x || x >= self.x + self.width || y < self.y {
            return None;
        }
        let row = ((y - self.y) / self.row_height) as usize;
        if row >= self.visible {
            return None;
        }
        let index = self.first + row;
        (index < self.total).then_some(index)
    }
}

/// Primeiro item visível de uma lista rolável de `visible` linhas.
///
/// Parte da rolagem anterior (`first`) e só a mexe o mínimo para o item
/// `selected` aparecer: com o mouse destacando uma linha visível nada muda, e
/// a lista não volta para o topo (antes a rolagem era recalculada a partir do
/// topo a cada frame). Também não deixa linha vazia no fim.
pub fn follow_selection(first: usize, selected: usize, total: usize, visible: usize) -> usize {
    if visible == 0 || total <= visible {
        return 0;
    }
    let first = if selected < first {
        selected
    } else if selected >= first + visible {
        selected + 1 - visible
    } else {
        first
    };
    first.min(total - visible)
}

/// Painéis modais que tomam o mouse quando abertos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    CommandPalette,
    QuickConnect,
    Settings,
    Notifications,
    Assistant,
    CloseConfirmation,
    Onboarding,
    /// Menu de contexto de uma tab da sidebar (botão direito).
    TabMenu,
}

/// Um painel desenhado no frame: o retângulo e, se tiver, a lista clicável.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverlayHit {
    pub kind: OverlayKind,
    pub panel: Rect,
    pub list: Option<ListGeometry>,
}

/// Painéis modais do último frame, na ordem em que foram desenhados (o de
/// cima por último).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OverlayHits {
    overlays: Vec<OverlayHit>,
}

impl OverlayHits {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um painel desenhado.
    pub fn push(&mut self, kind: OverlayKind, panel: Rect, list: Option<ListGeometry>) {
        self.overlays.push(OverlayHit { kind, panel, list });
    }

    /// O painel de cima (o último desenhado), que recebe o mouse.
    pub fn top(&self) -> Option<&OverlayHit> {
        self.overlays.last()
    }

    pub fn is_empty(&self) -> bool {
        self.overlays.is_empty()
    }

    /// Os painéis, de baixo para cima.
    pub fn iter(&self) -> impl Iterator<Item = &OverlayHit> {
        self.overlays.iter()
    }
}

/// O que fazer com um evento de mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMouse {
    /// Nenhum painel aberto: o evento segue para a barra de tabs e o terminal.
    Pass,
    /// Um painel está aberto e fica com o evento, sem efeito.
    Consume,
    /// Clique fora: fecha o painel (como o Esc).
    Close(OverlayKind),
    /// Mouse sobre um item: destaca o item.
    Hover(OverlayKind, usize),
    /// Clique num item: ativa o item (como o Enter).
    Activate(OverlayKind, usize),
    /// Roda sobre o painel: move a lista (positivo = para baixo).
    Scroll(OverlayKind, i32),
}

/// Clique (botão pressionado) em (x, y).
pub fn route_click(hits: &OverlayHits, x: f32, y: f32) -> OverlayMouse {
    let Some(top) = hits.top() else {
        return OverlayMouse::Pass;
    };
    if !top.panel.contains(x, y) {
        return OverlayMouse::Close(top.kind);
    }
    match top.list.and_then(|list| list.item_at(x, y)) {
        Some(index) => OverlayMouse::Activate(top.kind, index),
        None => OverlayMouse::Consume,
    }
}

/// Botão solto: com painel aberto, nunca chega ao terminal.
pub fn route_release(hits: &OverlayHits) -> OverlayMouse {
    if hits.is_empty() {
        OverlayMouse::Pass
    } else {
        OverlayMouse::Consume
    }
}

/// Roda do mouse em (x, y), `lines` linhas (positivo = para baixo).
///
/// Sobre o painel, rola a lista dele; fora dele, a roda é engolida: com um
/// painel aberto o terminal por trás não rola.
pub fn route_wheel(hits: &OverlayHits, x: f32, y: f32, lines: i32) -> OverlayMouse {
    let Some(top) = hits.top() else {
        return OverlayMouse::Pass;
    };
    if lines != 0 && top.panel.contains(x, y) {
        OverlayMouse::Scroll(top.kind, lines)
    } else {
        OverlayMouse::Consume
    }
}

/// Movimento do mouse em (x, y): destaca o item sob o cursor.
pub fn route_motion(hits: &OverlayHits, x: f32, y: f32) -> OverlayMouse {
    let Some(top) = hits.top() else {
        return OverlayMouse::Pass;
    };
    match top.list.and_then(|list| list.item_at(x, y)) {
        Some(index) => OverlayMouse::Hover(top.kind, index),
        None => OverlayMouse::Consume,
    }
}

/// Lembra que o último aperto de botão foi de um painel, para engolir a
/// soltura que vem depois.
///
/// O clique fora do painel o fecha no aperto; quando o botão é solto o painel
/// já não existe, e a soltura iria ao terminal (copiar ao selecionar uma
/// seleção antiga, ou uma soltura avulsa para o programa do PTY).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPress {
    owned: bool,
}

impl OverlayPress {
    /// Registra um aperto: `by_overlay` diz se um painel ficou com ele.
    pub fn pressed(&mut self, by_overlay: bool) {
        self.owned = by_overlay;
    }

    /// Na soltura: `true` se ela pertence a um aperto do painel e deve ser
    /// engolida. Vale uma vez.
    pub fn take_release(&mut self) -> bool {
        std::mem::take(&mut self.owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette() -> OverlayHits {
        let mut hits = OverlayHits::new();
        hits.push(
            OverlayKind::CommandPalette,
            Rect::new(100.0, 50.0, 400.0, 300.0),
            Some(ListGeometry {
                x: 108.0,
                y: 106.0,
                width: 384.0,
                row_height: 30.0,
                first: 2,
                visible: 5,
                total: 9,
            }),
        );
        hits
    }

    #[test]
    fn follow_selection_keeps_the_view_while_the_selection_is_visible() {
        // 21 itens, 14 linhas: rolou até o item 15 (rolagem 2).
        assert_eq!(follow_selection(0, 15, 21, 14), 2);
        // O mouse destaca a primeira linha visível (item 2): a lista fica.
        assert_eq!(follow_selection(2, 2, 21, 14), 2);
        assert_eq!(follow_selection(2, 15, 21, 14), 2);
        // Teclado sai pela borda: rola o mínimo.
        assert_eq!(follow_selection(2, 1, 21, 14), 1);
        assert_eq!(follow_selection(2, 16, 21, 14), 3);
        // Lista que cabe inteira, lista encolhida pelo filtro, sem linhas.
        assert_eq!(follow_selection(5, 3, 10, 14), 0);
        assert_eq!(follow_selection(7, 7, 9, 5), 4);
        assert_eq!(follow_selection(3, 3, 9, 0), 0);
    }

    /// Regressão (#22): rolar com a roda e depois mexer o mouse voltava a
    /// lista para o topo, porque a rolagem era recalculada a partir do topo e
    /// o item sob o cursor virava outro.
    #[test]
    fn hover_after_wheel_scroll_keeps_position_and_hits_the_item_under_the_cursor() {
        let (total, visible) = (21, 14);
        let list = |first| ListGeometry {
            x: 0.0,
            y: 100.0,
            width: 300.0,
            row_height: 30.0,
            first,
            visible,
            total,
        };
        // Roda: seleção vai ao item 15, a lista desce até mostrá-lo.
        let mut selected = 15;
        let mut first = follow_selection(0, selected, total, visible);
        assert_eq!(first, 2);
        // Mouse sobre a terceira linha da tela: item 4.
        let mut hits = OverlayHits::new();
        hits.push(
            OverlayKind::CommandPalette,
            Rect::new(0.0, 0.0, 300.0, 600.0),
            Some(list(first)),
        );
        match route_motion(&hits, 50.0, 100.0 + 2.0 * 30.0 + 5.0) {
            OverlayMouse::Hover(_, index) => selected = index,
            other => panic!("esperava Hover, veio {other:?}"),
        }
        assert_eq!(selected, 4);
        first = follow_selection(first, selected, total, visible);
        assert_eq!(first, 2, "a lista não volta para o topo");
        assert_eq!(list(first).item_at(50.0, 165.0), Some(4));
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect::new(10.0, 10.0, 20.0, 5.0);
        assert!(r.contains(10.0, 10.0));
        assert!(r.contains(29.9, 14.9));
        assert!(!r.contains(30.0, 12.0));
        assert!(!r.contains(15.0, 15.0));
        assert!(!r.contains(9.9, 12.0));
    }

    #[test]
    fn item_at_maps_rows_after_scroll() {
        let list = palette().top().unwrap().list.unwrap();
        assert_eq!(list.item_at(200.0, 106.0), Some(2));
        assert_eq!(list.item_at(200.0, 136.0), Some(3));
        assert_eq!(list.item_at(200.0, 106.0 + 4.0 * 30.0 + 29.0), Some(6));
        // Abaixo da última linha visível, acima da lista e fora da largura.
        assert_eq!(list.item_at(200.0, 106.0 + 5.0 * 30.0), None);
        assert_eq!(list.item_at(200.0, 105.0), None);
        assert_eq!(list.item_at(107.0, 120.0), None);
        assert_eq!(list.item_at(492.0, 120.0), None);
    }

    #[test]
    fn item_at_ignores_rows_past_the_end() {
        let list = ListGeometry {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            row_height: 10.0,
            first: 0,
            visible: 5,
            total: 3,
        };
        assert_eq!(list.item_at(5.0, 25.0), Some(2));
        assert_eq!(list.item_at(5.0, 35.0), None);
    }

    #[test]
    fn nothing_open_passes_everything() {
        let hits = OverlayHits::new();
        assert_eq!(route_click(&hits, 1.0, 1.0), OverlayMouse::Pass);
        assert_eq!(route_release(&hits), OverlayMouse::Pass);
        assert_eq!(route_wheel(&hits, 1.0, 1.0, 3), OverlayMouse::Pass);
        assert_eq!(route_motion(&hits, 1.0, 1.0), OverlayMouse::Pass);
    }

    #[test]
    fn click_on_item_activates_it() {
        assert_eq!(
            route_click(&palette(), 200.0, 140.0),
            OverlayMouse::Activate(OverlayKind::CommandPalette, 3)
        );
    }

    #[test]
    fn click_inside_panel_but_off_the_list_is_consumed() {
        // Caixa de busca, no topo do painel.
        assert_eq!(route_click(&palette(), 200.0, 70.0), OverlayMouse::Consume);
    }

    #[test]
    fn click_outside_closes_and_never_reaches_terminal() {
        assert_eq!(
            route_click(&palette(), 20.0, 20.0),
            OverlayMouse::Close(OverlayKind::CommandPalette)
        );
        assert_eq!(route_release(&palette()), OverlayMouse::Consume);
    }

    #[test]
    fn wheel_inside_scrolls_the_menu() {
        assert_eq!(
            route_wheel(&palette(), 200.0, 200.0, 3),
            OverlayMouse::Scroll(OverlayKind::CommandPalette, 3)
        );
        assert_eq!(
            route_wheel(&palette(), 200.0, 200.0, -1),
            OverlayMouse::Scroll(OverlayKind::CommandPalette, -1)
        );
    }

    #[test]
    fn wheel_outside_is_swallowed_not_sent_to_terminal() {
        assert_eq!(
            route_wheel(&palette(), 20.0, 20.0, 3),
            OverlayMouse::Consume
        );
        assert_eq!(
            route_wheel(&palette(), 200.0, 200.0, 0),
            OverlayMouse::Consume
        );
    }

    #[test]
    fn motion_over_item_highlights_it() {
        assert_eq!(
            route_motion(&palette(), 200.0, 110.0),
            OverlayMouse::Hover(OverlayKind::CommandPalette, 2)
        );
        assert_eq!(route_motion(&palette(), 20.0, 20.0), OverlayMouse::Consume);
    }

    #[test]
    fn the_topmost_panel_gets_the_mouse() {
        let mut hits = palette();
        hits.push(
            OverlayKind::CloseConfirmation,
            Rect::new(150.0, 150.0, 100.0, 50.0),
            None,
        );
        // Dentro da paleta, mas fora da confirmação, que está por cima.
        assert_eq!(
            route_click(&hits, 120.0, 120.0),
            OverlayMouse::Close(OverlayKind::CloseConfirmation)
        );
        assert_eq!(route_click(&hits, 160.0, 160.0), OverlayMouse::Consume);
    }

    #[test]
    fn release_after_an_overlay_press_is_swallowed_once() {
        let mut press = OverlayPress::default();
        press.pressed(true);
        assert!(press.take_release());
        assert!(!press.take_release(), "só a soltura daquele aperto");
    }

    #[test]
    fn release_after_a_terminal_press_goes_through() {
        let mut press = OverlayPress::default();
        press.pressed(true);
        press.pressed(false);
        assert!(!press.take_release());
    }
}
