//! Cor da tab e fundo do terminal por tab (#24): itens do menu do botão
//! direito, aplicar/tirar a cor, cor digitada em hex e o aviso de contraste.

use super::tab_ui::{TabMenuAction, TabMenuEntry};
use super::AppState;
use crate::ui::i18n::{low_contrast_warning, preset_color_name, tr, Text};
use crate::ui::tab_colors::{presets, text_contrast, ColorTarget, MIN_CONTRAST};
use crate::ui::toast::ToastKind;

impl AppState {
    /// Itens do menu da tab que abrem as listas de cores.
    pub(super) fn color_menu_items() -> [TabMenuEntry; 2] {
        [
            TabMenuEntry {
                action: TabMenuAction::Colors(ColorTarget::Tab),
                label: tr(Text::TabColorMenu).to_string(),
            },
            TabMenuEntry {
                action: TabMenuAction::Colors(ColorTarget::Background),
                label: tr(Text::BackgroundMenu).to_string(),
            },
        ]
    }

    /// Lista de cores para a tab `index`: voltar, as 8 do tema,
    /// personalizada e nenhuma.
    pub(super) fn color_entries(&self, index: usize, target: ColorTarget) -> Vec<TabMenuEntry> {
        let current = self
            .tab_manager
            .tabs()
            .get(index)
            .and_then(|tab| match target {
                ColorTarget::Tab => tab.color,
                ColorTarget::Background => tab.background,
            });
        let mut entries = vec![TabMenuEntry {
            action: TabMenuAction::Back,
            label: tr(Text::Back).to_string(),
        }];
        for (i, color) in presets(&self.config.colors, target).into_iter().enumerate() {
            let mut label = preset_color_name(i).to_string();
            if target == ColorTarget::Background
                && text_contrast(&self.config.colors, color) < MIN_CONTRAST
            {
                label = format!("{label}  ({})", tr(Text::LowContrast));
            }
            if current == Some(color) {
                label = format!("{label}  \u{2713}");
            }
            entries.push(TabMenuEntry {
                action: TabMenuAction::SetColor(target, Some(color)),
                label,
            });
        }
        entries.push(TabMenuEntry {
            action: TabMenuAction::CustomColor(target),
            label: tr(Text::CustomColor).to_string(),
        });
        entries.push(TabMenuEntry {
            action: TabMenuAction::SetColor(target, None),
            label: tr(Text::NoColor).to_string(),
        });
        entries
    }

    /// Aplica (ou tira) a cor da tab `index`. Fundo com pouco contraste com
    /// o texto do tema ganha um aviso. Devolve `false` se a tab não existe.
    pub fn set_tab_color(
        &mut self,
        index: usize,
        target: ColorTarget,
        color: Option<[u8; 3]>,
    ) -> bool {
        let Some(tab) = self.tab_manager.get_tab_mut(index) else {
            return false;
        };
        match target {
            ColorTarget::Tab => tab.color = color,
            ColorTarget::Background => tab.background = color,
        }
        if let (ColorTarget::Background, Some(color)) = (target, color) {
            let ratio = text_contrast(&self.config.colors, color);
            if ratio < MIN_CONTRAST {
                self.show_toast(low_contrast_warning(ratio), ToastKind::Error);
            }
        }
        true
    }

    /// "Personalizada…": abre o campo para digitar a cor em hex.
    pub fn start_custom_tab_color(&mut self, index: usize, target: ColorTarget) {
        let Some(tab) = self.tab_manager.tabs().get(index) else {
            return;
        };
        let current = match target {
            ColorTarget::Tab => tab.color,
            ColorTarget::Background => tab.background,
        };
        self.custom_color = Some((tab.id, target));
        let initial = current
            .map(crate::ui::tab_colors::to_hex)
            .unwrap_or_else(|| "#".into());
        self.command_palette.open_hex_color(&initial);
    }

    /// Enter no campo de cor: aplica a cor digitada.
    pub(super) fn apply_custom_tab_color(&mut self, color: Option<[u8; 3]>) {
        let Some((tab, target)) = self.custom_color.take() else {
            return;
        };
        if let (Some(index), Some(color)) = (self.tab_manager.tab_index_of(tab), color) {
            self.set_tab_color(index, target, Some(color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::command_palette::PaletteInputKey;

    fn app() -> AppState {
        let mut app = AppState::new(40, 5).unwrap();
        crate::config::theme::Theme::apply_to_config("dark", &mut app.config);
        app
    }

    /// Escolher, trocar e tirar a cor da aba e o fundo pelo menu (#24).
    #[test]
    fn menu_sets_changes_and_clears_colors() {
        let mut app = app();
        app.open_tab_menu(0, 10.0, 40.0);
        let entries = &app.tab_menu().unwrap().entries;
        let colors_at = entries
            .iter()
            .position(|e| e.action == TabMenuAction::Colors(ColorTarget::Tab))
            .unwrap();
        app.activate_tab_menu_item(colors_at);
        // A lista abre no mesmo menu: voltar, 8 cores, personalizada, nenhuma.
        let menu = app.tab_menu().unwrap();
        assert_eq!(menu.entries.len(), 11);
        assert_eq!(menu.entries[0].action, TabMenuAction::Back);
        let red = presets(&app.config.colors, ColorTarget::Tab)[0];
        app.activate_tab_menu_item(1);
        assert_eq!(app.tab_manager.tabs()[0].color, Some(red));
        assert!(app.tab_menu().is_none());

        // Trocar: a marca ✓ fica na cor atual.
        app.open_tab_menu(0, 10.0, 40.0);
        app.activate_tab_menu_item(colors_at);
        assert!(app.tab_menu().unwrap().entries[1]
            .label
            .contains('\u{2713}'));
        app.activate_tab_menu_item(2);
        assert_ne!(app.tab_manager.tabs()[0].color, Some(red));

        // Tirar.
        app.open_tab_menu(0, 10.0, 40.0);
        app.activate_tab_menu_item(colors_at);
        app.activate_tab_menu_item(10);
        assert_eq!(app.tab_manager.tabs()[0].color, None);

        // Voltar reabre o menu da tab.
        app.open_tab_menu(0, 10.0, 40.0);
        app.activate_tab_menu_item(colors_at + 1);
        app.activate_tab_menu_item(0);
        assert!(app
            .tab_menu()
            .unwrap()
            .entries
            .iter()
            .any(|e| e.action == TabMenuAction::Rename));

        // Fundo.
        app.close_tab_menu();
        let bg = presets(&app.config.colors, ColorTarget::Background)[3];
        assert!(app.set_tab_color(0, ColorTarget::Background, Some(bg)));
        assert_eq!(app.tab_manager.tabs()[0].background, Some(bg));
        assert!(!app.set_tab_color(9, ColorTarget::Tab, None));
    }

    /// Aviso de contraste baixo: texto claro do tema sobre fundo claro.
    #[test]
    fn low_contrast_background_shows_a_warning() {
        let mut app = app();
        app.set_tab_color(0, ColorTarget::Background, Some([20, 20, 30]));
        assert!(app.toast().is_none(), "fundo escuro: sem aviso");
        app.set_tab_color(0, ColorTarget::Background, Some([230, 230, 230]));
        let toast = app.toast().expect("aviso de contraste");
        assert!(toast.text.contains(":1"), "{}", toast.text);
    }

    #[test]
    fn custom_hex_color_through_the_palette() {
        let mut app = app();
        app.start_custom_tab_color(0, ColorTarget::Tab);
        assert!(app.is_command_palette_visible());
        for c in "ff8000".chars() {
            app.handle_command_palette_key(&PaletteInputKey::Char(c));
        }
        app.handle_command_palette_key(&PaletteInputKey::Enter);
        assert!(!app.is_command_palette_visible());
        assert_eq!(app.tab_manager.tabs()[0].color, Some([255, 128, 0]));

        // Esc desiste sem mexer.
        app.start_custom_tab_color(0, ColorTarget::Tab);
        app.handle_command_palette_key(&PaletteInputKey::Escape);
        assert_eq!(app.tab_manager.tabs()[0].color, Some([255, 128, 0]));
        assert!(app.custom_color.is_none());
    }
}
