//! Aviso rápido na janela ("Copiado", "Não foi possível colar").
//!
//! Um aviso por vez, no canto inferior direito, que some sozinho. O tempo
//! entra como parâmetro (`now`) para os testes não dependerem do relógio.

use std::time::{Duration, Instant};

/// Quanto tempo o aviso fica na tela.
pub const TOAST_DURATION: Duration = Duration::from_millis(1500);

/// Tipo do aviso (muda a cor da faixa).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// Deu certo (ex.: copiou).
    Info,
    /// Falhou (ex.: não colou).
    Error,
}

/// Um aviso mostrado num instante.
#[derive(Debug, Clone)]
pub struct Toast {
    /// Texto do aviso.
    pub text: String,
    /// Tipo do aviso.
    pub kind: ToastKind,
    shown_at: Instant,
}

impl Toast {
    /// Ainda deve aparecer em `now`?
    pub fn is_visible(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.shown_at) < TOAST_DURATION
    }
}

/// O aviso atual da janela (no máximo um).
#[derive(Debug, Default)]
pub struct ToastState {
    current: Option<Toast>,
}

impl ToastState {
    /// Mostra `text` a partir de `now`, trocando o aviso anterior.
    pub fn show(&mut self, text: impl Into<String>, kind: ToastKind, now: Instant) {
        self.current = Some(Toast {
            text: text.into(),
            kind,
            shown_at: now,
        });
    }

    /// O aviso que aparece em `now`, se ainda não expirou.
    pub fn visible(&self, now: Instant) -> Option<&Toast> {
        self.current.as_ref().filter(|toast| toast.is_visible(now))
    }
}

/// Texto do aviso de cópia: "Copied · 1 character", "Copiado · 42 caracteres".
pub fn copied_message(chars: usize) -> String {
    crate::ui::i18n::copied_message(chars)
}

/// Texto do aviso quando colar falha.
pub fn paste_failed() -> &'static str {
    crate::ui::i18n::tr(crate::ui::i18n::Text::PasteFailed)
}

/// Texto do aviso quando não há texto para colar (vazia ou só imagem).
pub fn nothing_to_paste() -> &'static str {
    crate::ui::i18n::tr(crate::ui::i18n::Text::NothingToPaste)
}

/// Texto do aviso quando copiar falha.
pub fn copy_failed() -> &'static str {
    crate::ui::i18n::tr(crate::ui::i18n::Text::CopyFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toast_appears_and_expires() {
        let start = Instant::now();
        let mut state = ToastState::default();
        assert!(state.visible(start).is_none());

        state.show("Copiado · 3 caracteres", ToastKind::Info, start);
        let shown = state.visible(start).map(|t| (t.text.clone(), t.kind));
        assert_eq!(
            shown,
            Some(("Copiado · 3 caracteres".to_string(), ToastKind::Info))
        );
        assert!(state
            .visible(start + TOAST_DURATION - Duration::from_millis(1))
            .is_some());
        assert!(state.visible(start + TOAST_DURATION).is_none());
    }

    #[test]
    fn new_toast_replaces_the_previous_one() {
        let start = Instant::now();
        let mut state = ToastState::default();
        state.show("primeiro", ToastKind::Info, start);
        let later = start + Duration::from_millis(1000);
        state.show(paste_failed(), ToastKind::Error, later);
        let toast = state
            .visible(later + Duration::from_millis(1000))
            .map(|t| (t.text.as_str(), t.kind));
        assert_eq!(toast, Some((paste_failed(), ToastKind::Error)));
    }

    #[test]
    fn copied_message_handles_singular_and_plural() {
        crate::ui::i18n::set_language(crate::ui::i18n::Language::PortugueseBr);
        assert_eq!(copied_message(1), "Copiado · 1 caractere");
        assert_eq!(copied_message(0), "Copiado · 0 caracteres");
        assert_eq!(copied_message(42), "Copiado · 42 caracteres");
        crate::ui::i18n::set_language(crate::ui::i18n::Language::English);
        assert_eq!(copied_message(42), "Copied · 42 characters");
    }
}
