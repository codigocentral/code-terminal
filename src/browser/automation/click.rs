//! Click API - API de Clique
//!
//! Este módulo fornece funcionalidades para simular cliques do mouse
//! em elementos da página web.
//!
//! # Exemplo
//!
//! ```rust,no_run
//! use code_terminal::browser::automation::click::{ClickController, ClickOptions, MouseButton};
//! use code_terminal::browser::BrowserId;
//!
//! async fn click_example(browser_id: BrowserId) {
//!     let controller = ClickController::new(browser_id);
//!     
//!     // Clique simples
//!     controller.click("#submit-button", ClickOptions::default()).await.unwrap();
//!     
//!     // Clique com botão direito
//!     let options = ClickOptions::default()
//!         .with_button(MouseButton::Right)
//!         .with_delay_ms(100);
//!     controller.click(".context-menu", options).await.unwrap();
//! }
//! ```

use crate::browser::{BrowserError, BrowserId, BrowserResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

/// Controlador de cliques
#[derive(Debug, Clone)]
pub struct ClickController {
    browser_id: BrowserId,
}

impl ClickController {
    /// Cria um novo controlador de cliques
    pub fn new(browser_id: BrowserId) -> Self {
        Self { browser_id }
    }

    /// Retorna o browser associado.
    pub fn browser_id(&self) -> BrowserId {
        self.browser_id
    }

    /// Executa um clique em um elemento
    pub async fn click(
        &self,
        selector: impl AsRef<str>,
        options: ClickOptions,
    ) -> BrowserResult<ClickResult> {
        let selector = selector.as_ref();

        tracing::debug!("Clicking on '{}' with options: {:?}", selector, options);

        // Valida o selector
        if selector.is_empty() {
            return Err(BrowserError::InvalidUrl("Empty selector".to_string()));
        }

        // Delay opcional antes do clique
        if options.delay_before_ms > 0 {
            sleep(Duration::from_millis(options.delay_before_ms)).await;
        }

        // Na implementação real, executaria JavaScript no WebView:
        // 1. Encontrar elemento
        // 2. Scrollar para visibilidade (se necessário)
        // 3. Simular eventos mouseover, mousedown, click, mouseup
        // 4. Retornar resultado

        // Simulação do resultado
        let result = ClickResult {
            success: true,
            selector: selector.to_string(),
            button: options.button,
            coordinates: Some(Coordinates {
                x: options.offset_x,
                y: options.offset_y,
            }),
            clicked_at: chrono::Utc::now(),
            element_found: true,
            element_visible: true,
            element_enabled: true,
            double_click: options.double_click,
            triple_click: options.triple_click,
        };

        // Delay após o clique
        if options.delay_after_ms > 0 {
            sleep(Duration::from_millis(options.delay_after_ms)).await;
        }

        Ok(result)
    }

    /// Clica em coordenadas específicas (não em elemento)
    pub async fn click_at(
        &self,
        x: f64,
        y: f64,
        options: ClickOptions,
    ) -> BrowserResult<ClickResult> {
        tracing::debug!("Clicking at coordinates ({}, {})", x, y);

        // Na implementação real, dispararia evento de clique nas coordenadas

        let result = ClickResult {
            success: true,
            selector: format!("coordinates:{},{}", x, y),
            button: options.button,
            coordinates: Some(Coordinates { x, y }),
            clicked_at: chrono::Utc::now(),
            element_found: true,
            element_visible: true,
            element_enabled: true,
            double_click: false,
            triple_click: false,
        };

        Ok(result)
    }

    /// Clica no centro de um elemento
    pub async fn click_center(
        &self,
        selector: impl AsRef<str>,
        options: ClickOptions,
    ) -> BrowserResult<ClickResult> {
        let mut opts = options;
        opts.offset_x = 0.0; // Centro
        opts.offset_y = 0.0; // Centro
        self.click(selector, opts).await
    }

    /// Clica em múltiplos elementos sequencialmente
    pub async fn click_multiple(
        &self,
        selectors: &[impl AsRef<str>],
        options: ClickOptions,
    ) -> Vec<BrowserResult<ClickResult>> {
        let mut results = Vec::new();

        for selector in selectors {
            let result = self.click(selector.as_ref(), options).await;
            results.push(result);

            // Delay entre cliques
            if options.delay_between_clicks_ms > 0 {
                sleep(Duration::from_millis(options.delay_between_clicks_ms)).await;
            }
        }

        results
    }

    /// Clica e segura (para drag-and-drop)
    pub async fn click_and_hold(
        &self,
        selector: impl AsRef<str>,
        hold_duration_ms: u64,
    ) -> BrowserResult<ClickResult> {
        tracing::debug!(
            "Clicking and holding '{}' for {}ms",
            selector.as_ref(),
            hold_duration_ms
        );

        // Na implementação real:
        // 1. mousedown
        // 2. aguarda hold_duration_ms
        // 3. mouseup

        sleep(Duration::from_millis(hold_duration_ms)).await;

        Ok(ClickResult {
            success: true,
            selector: selector.as_ref().to_string(),
            button: MouseButton::Left,
            coordinates: None,
            clicked_at: chrono::Utc::now(),
            element_found: true,
            element_visible: true,
            element_enabled: true,
            double_click: false,
            triple_click: false,
        })
    }

    /// Clica com duplo-clique
    pub async fn double_click(
        &self,
        selector: impl AsRef<str>,
        options: ClickOptions,
    ) -> BrowserResult<ClickResult> {
        tracing::debug!("Double-clicking on '{}'", selector.as_ref());

        // Na implementação real, dispararia evento dblclick

        let mut result = self.click(selector, options).await?;
        result.double_click = true;

        Ok(result)
    }

    /// Clica com triple-clique (para seleção de texto)
    pub async fn triple_click(
        &self,
        selector: impl AsRef<str>,
        options: ClickOptions,
    ) -> BrowserResult<ClickResult> {
        tracing::debug!("Triple-clicking on '{}'", selector.as_ref());

        let mut result = self.click(selector, options).await?;
        result.triple_click = true;

        Ok(result)
    }

    /// Simula hover (mouseenter/mouseover)
    pub async fn hover(
        &self,
        selector: impl AsRef<str>,
        duration_ms: u64,
    ) -> BrowserResult<HoverResult> {
        tracing::debug!(
            "Hovering over '{}' for {}ms",
            selector.as_ref(),
            duration_ms
        );

        // Na implementação real, dispararia mouseenter/mouseover

        sleep(Duration::from_millis(duration_ms)).await;

        Ok(HoverResult {
            success: true,
            selector: selector.as_ref().to_string(),
            duration_ms,
            hovered_at: chrono::Utc::now(),
        })
    }

    /// Move o mouse para um elemento (sem clicar)
    pub async fn move_to(&self, selector: impl AsRef<str>) -> BrowserResult<MoveResult> {
        tracing::debug!("Moving mouse to '{}'", selector.as_ref());

        Ok(MoveResult {
            success: true,
            selector: selector.as_ref().to_string(),
            moved_at: chrono::Utc::now(),
        })
    }
}

/// Opções de clique
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ClickOptions {
    /// Botão do mouse
    pub button: MouseButton,
    /// Offset X em relação ao elemento (0.0 = centro)
    pub offset_x: f64,
    /// Offset Y em relação ao elemento (0.0 = centro)
    pub offset_y: f64,
    /// Se deve forçar o clique (mesmo se elemento estiver coberto)
    pub force: bool,
    /// Se deve scrollar elemento para visibilidade antes
    pub scroll_into_view: bool,
    /// Delay antes do clique (ms)
    pub delay_before_ms: u64,
    /// Delay após o clique (ms)
    pub delay_after_ms: u64,
    /// Delay entre cliques múltiplos (ms)
    pub delay_between_clicks_ms: u64,
    /// Se é duplo-clique
    pub double_click: bool,
    /// Se é triple-clique
    pub triple_click: bool,
    /// Modificadores de teclado
    pub modifiers: KeyModifiers,
}

impl Default for ClickOptions {
    fn default() -> Self {
        Self {
            button: MouseButton::Left,
            offset_x: 0.0,
            offset_y: 0.0,
            force: false,
            scroll_into_view: true,
            delay_before_ms: 0,
            delay_after_ms: 50,
            delay_between_clicks_ms: 100,
            double_click: false,
            triple_click: false,
            modifiers: KeyModifiers::default(),
        }
    }
}

impl ClickOptions {
    /// Cria opções para botão direito
    pub fn right_click() -> Self {
        Self {
            button: MouseButton::Right,
            delay_after_ms: 100,
            ..Default::default()
        }
    }

    /// Cria opções para clique do meio
    pub fn middle_click() -> Self {
        Self {
            button: MouseButton::Middle,
            ..Default::default()
        }
    }

    /// Define o botão
    pub fn with_button(mut self, button: MouseButton) -> Self {
        self.button = button;
        self
    }

    /// Define offset
    pub fn with_offset(mut self, x: f64, y: f64) -> Self {
        self.offset_x = x;
        self.offset_y = y;
        self
    }

    /// Define delay antes
    pub fn with_delay_before(mut self, ms: u64) -> Self {
        self.delay_before_ms = ms;
        self
    }

    /// Define delay depois
    pub fn with_delay_after(mut self, ms: u64) -> Self {
        self.delay_after_ms = ms;
        self
    }

    /// Define delay total
    pub fn with_delay_ms(mut self, ms: u64) -> Self {
        self.delay_before_ms = ms / 2;
        self.delay_after_ms = ms / 2;
        self
    }

    /// Habilita force click
    pub fn with_force(mut self) -> Self {
        self.force = true;
        self
    }

    /// Desabilita scroll automático
    pub fn without_scroll(mut self) -> Self {
        self.scroll_into_view = false;
        self
    }

    /// Habilita duplo-clique
    pub fn as_double_click(mut self) -> Self {
        self.double_click = true;
        self
    }

    /// Habilita triple-clique
    pub fn as_triple_click(mut self) -> Self {
        self.triple_click = true;
        self
    }

    /// Adiciona modificador Ctrl
    pub fn with_ctrl(mut self) -> Self {
        self.modifiers.ctrl = true;
        self
    }

    /// Adiciona modificador Shift
    pub fn with_shift(mut self) -> Self {
        self.modifiers.shift = true;
        self
    }

    /// Adiciona modificador Alt
    pub fn with_alt(mut self) -> Self {
        self.modifiers.alt = true;
        self
    }

    /// Adiciona modificador Meta (Cmd/Win)
    pub fn with_meta(mut self) -> Self {
        self.modifiers.meta = true;
        self
    }
}

/// Botão do mouse
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum MouseButton {
    /// Botão esquerdo
    #[default]
    Left,
    /// Botão direito
    Right,
    /// Botão do meio (scroll)
    Middle,
    /// Botão 4 (voltar)
    Back,
    /// Botão 5 (avançar)
    Forward,
}

impl MouseButton {
    /// Retorna o número do botão (para JavaScript)
    pub fn as_number(&self) -> u8 {
        match self {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::Back => 3,
            MouseButton::Forward => 4,
        }
    }
}

/// Modificadores de teclado para cliques
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct KeyModifiers {
    /// Ctrl
    pub ctrl: bool,
    /// Shift
    pub shift: bool,
    /// Alt/Option
    pub alt: bool,
    /// Meta/Cmd/Win
    pub meta: bool,
}

impl KeyModifiers {
    /// Retorna se algum modificador está ativo
    pub fn any(&self) -> bool {
        self.ctrl || self.shift || self.alt || self.meta
    }

    /// Retorna array de nomes dos modificadores ativos
    pub fn active_modifiers(&self) -> Vec<&'static str> {
        let mut mods = Vec::new();
        if self.ctrl {
            mods.push("Control");
        }
        if self.shift {
            mods.push("Shift");
        }
        if self.alt {
            mods.push("Alt");
        }
        if self.meta {
            mods.push("Meta");
        }
        mods
    }
}

/// Coordenadas 2D
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Coordinates {
    /// Coordenada X
    pub x: f64,
    /// Coordenada Y
    pub y: f64,
}

/// Resultado de um clique
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickResult {
    /// Se o clique foi bem-sucedido
    pub success: bool,
    /// Selector usado
    pub selector: String,
    /// Botão utilizado
    pub button: MouseButton,
    /// Coordenadas do clique (se aplicável)
    pub coordinates: Option<Coordinates>,
    /// Timestamp do clique
    pub clicked_at: chrono::DateTime<chrono::Utc>,
    /// Se o elemento foi encontrado
    pub element_found: bool,
    /// Se o elemento estava visível
    pub element_visible: bool,
    /// Se o elemento estava habilitado
    pub element_enabled: bool,
    /// Se foi duplo-clique
    pub double_click: bool,
    /// Se foi triple-clique
    pub triple_click: bool,
}

impl ClickResult {
    /// Retorna se o clique foi bem-sucedido
    pub fn is_success(&self) -> bool {
        self.success && self.element_found && self.element_visible && self.element_enabled
    }
}

/// Resultado de hover
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverResult {
    /// Se o hover foi bem-sucedido
    pub success: bool,
    /// Selector usado
    pub selector: String,
    /// Duração em ms
    pub duration_ms: u64,
    /// Timestamp
    pub hovered_at: chrono::DateTime<chrono::Utc>,
}

/// Resultado de movimento
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveResult {
    /// Se o movimento foi bem-sucedido
    pub success: bool,
    /// Selector usado
    pub selector: String,
    /// Timestamp
    pub moved_at: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_click_options_default() {
        let opts = ClickOptions::default();
        assert_eq!(opts.button, MouseButton::Left);
        assert_eq!(opts.offset_x, 0.0);
        assert!(opts.scroll_into_view);
        assert!(!opts.force);
    }

    #[test]
    fn test_click_options_builder() {
        let opts = ClickOptions::default()
            .with_button(MouseButton::Right)
            .with_offset(10.0, 20.0)
            .with_delay_ms(200)
            .with_ctrl()
            .with_shift();

        assert_eq!(opts.button, MouseButton::Right);
        assert_eq!(opts.offset_x, 10.0);
        assert_eq!(opts.offset_y, 20.0);
        assert!(opts.modifiers.ctrl);
        assert!(opts.modifiers.shift);
    }

    #[test]
    fn test_mouse_button_number() {
        assert_eq!(MouseButton::Left.as_number(), 0);
        assert_eq!(MouseButton::Middle.as_number(), 1);
        assert_eq!(MouseButton::Right.as_number(), 2);
    }

    #[test]
    fn test_key_modifiers() {
        let mods = KeyModifiers {
            ctrl: true,
            shift: false,
            alt: true,
            meta: false,
        };

        assert!(mods.any());
        let active = mods.active_modifiers();
        assert_eq!(active.len(), 2);
        assert!(active.contains(&"Control"));
        assert!(active.contains(&"Alt"));
    }

    #[test]
    fn test_right_click_options() {
        let opts = ClickOptions::right_click();
        assert_eq!(opts.button, MouseButton::Right);
    }

    #[test]
    fn test_click_result_success() {
        let result = ClickResult {
            success: true,
            selector: "#btn".to_string(),
            button: MouseButton::Left,
            coordinates: None,
            clicked_at: chrono::Utc::now(),
            element_found: true,
            element_visible: true,
            element_enabled: true,
            double_click: false,
            triple_click: false,
        };

        assert!(result.is_success());
    }

    #[test]
    fn test_click_result_failure() {
        let result = ClickResult {
            success: false,
            selector: "#btn".to_string(),
            button: MouseButton::Left,
            coordinates: None,
            clicked_at: chrono::Utc::now(),
            element_found: false,
            element_visible: false,
            element_enabled: true,
            double_click: false,
            triple_click: false,
        };

        assert!(!result.is_success());
    }

    fn fast() -> ClickOptions {
        ClickOptions::default()
            .with_delay_after(0)
            .with_delay_before(0)
    }

    #[tokio::test]
    async fn test_click_reports_options() {
        let browser_id = BrowserId::new();
        let controller = ClickController::new(browser_id);
        assert_eq!(controller.browser_id(), browser_id);

        let options = fast()
            .with_button(MouseButton::Middle)
            .with_offset(3.0, 4.0)
            .as_double_click()
            .as_triple_click()
            .with_delay_before(1)
            .with_delay_after(1);
        let result = controller.click("#ok", options).await.unwrap();
        assert!(result.is_success());
        assert_eq!(result.selector, "#ok");
        assert_eq!(result.button, MouseButton::Middle);
        assert_eq!(result.coordinates, Some(Coordinates { x: 3.0, y: 4.0 }));
        assert!(result.double_click);
        assert!(result.triple_click);
    }

    #[tokio::test]
    async fn test_click_rejects_empty_selector() {
        let controller = ClickController::new(BrowserId::new());
        let err = controller.click("", fast()).await.unwrap_err();
        assert!(err.to_string().contains("Empty selector"), "{err}");
        assert!(controller.double_click("", fast()).await.is_err());
        assert!(controller.triple_click("", fast()).await.is_err());
    }

    #[tokio::test]
    async fn test_click_at_uses_coordinates() {
        let controller = ClickController::new(BrowserId::new());
        let result = controller
            .click_at(12.5, 30.0, ClickOptions::right_click())
            .await
            .unwrap();
        // Regressão: o identificador tinha um ')' sobrando
        assert_eq!(result.selector, "coordinates:12.5,30");
        assert_eq!(result.button, MouseButton::Right);
        assert_eq!(result.coordinates, Some(Coordinates { x: 12.5, y: 30.0 }));
        assert!(!result.double_click);
    }

    #[tokio::test]
    async fn test_click_center_resets_offset() {
        let controller = ClickController::new(BrowserId::new());
        let result = controller
            .click_center("#c", fast().with_offset(50.0, 60.0))
            .await
            .unwrap();
        assert_eq!(result.coordinates, Some(Coordinates { x: 0.0, y: 0.0 }));
    }

    #[tokio::test]
    async fn test_click_multiple_keeps_order_and_errors() {
        let controller = ClickController::new(BrowserId::new());
        let mut options = fast();
        options.delay_between_clicks_ms = 1;
        let results = controller.click_multiple(&["#a", "", "#b"], options).await;
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].as_ref().unwrap().selector, "#a");
        assert!(results[1].is_err());
        assert_eq!(results[2].as_ref().unwrap().selector, "#b");

        options.delay_between_clicks_ms = 0;
        let results = controller.click_multiple(&["#x"], options).await;
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_hold_double_triple_hover_and_move() {
        let controller = ClickController::new(BrowserId::new());

        let held = controller.click_and_hold("#drag", 1).await.unwrap();
        assert_eq!(held.selector, "#drag");
        assert_eq!(held.button, MouseButton::Left);
        assert!(held.coordinates.is_none());

        let double = controller.double_click("#d", fast()).await.unwrap();
        assert!(double.double_click);
        assert!(!double.triple_click);

        let triple = controller.triple_click("#t", fast()).await.unwrap();
        assert!(triple.triple_click);

        let hover = controller.hover("#menu", 1).await.unwrap();
        assert!(hover.success);
        assert_eq!(hover.duration_ms, 1);
        assert_eq!(hover.selector, "#menu");

        let moved = controller.move_to("#alvo").await.unwrap();
        assert!(moved.success);
        assert_eq!(moved.selector, "#alvo");
    }

    #[test]
    fn test_click_options_presets_and_modifiers() {
        assert_eq!(ClickOptions::middle_click().button, MouseButton::Middle);
        assert_eq!(ClickOptions::right_click().delay_after_ms, 100);

        let opts = ClickOptions::default()
            .with_delay_ms(301)
            .with_force()
            .without_scroll()
            .with_alt()
            .with_meta();
        assert_eq!(opts.delay_before_ms, 150);
        assert_eq!(opts.delay_after_ms, 150);
        assert!(opts.force);
        assert!(!opts.scroll_into_view);
        assert_eq!(opts.modifiers.active_modifiers(), vec!["Alt", "Meta"]);

        let all = ClickOptions::default()
            .with_ctrl()
            .with_shift()
            .with_alt()
            .with_meta();
        assert_eq!(
            all.modifiers.active_modifiers(),
            vec!["Control", "Shift", "Alt", "Meta"]
        );
        assert!(!KeyModifiers::default().any());
        assert!(KeyModifiers::default().active_modifiers().is_empty());
    }

    #[test]
    fn test_mouse_button_extra_numbers_and_serde() {
        assert_eq!(MouseButton::Back.as_number(), 3);
        assert_eq!(MouseButton::Forward.as_number(), 4);
        assert_eq!(MouseButton::default(), MouseButton::Left);
        assert_eq!(
            serde_json::to_string(&MouseButton::Forward).unwrap(),
            "\"forward\""
        );
    }
}
