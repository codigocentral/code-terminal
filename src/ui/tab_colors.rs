//! Cores escolhidas pela pessoa para cada tab: a cor na sidebar e o fundo
//! do terminal.
//!
//! As cores prontas saem do tema atual (para manter o contraste); a cor fica
//! guardada como RGB na tab e no workspace, então trocar de tema não a muda.

use crate::config::ColorScheme;
use crate::ui::tabs::TabManager;

/// O que a cor pinta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorTarget {
    /// Faixa e realce do cartão na sidebar.
    Tab,
    /// Fundo do terminal da tab.
    Background,
}

/// Contraste mínimo recomendado entre texto e fundo (WCAG AA).
pub const MIN_CONTRAST: f32 = 4.5;

/// "#rrggbb" (ou "rrggbb", "#rgb") em RGB.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    let digits: Vec<u8> = hex
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<_>>()?;
    match digits.as_slice() {
        [r, g, b] => Some([r * 17, g * 17, b * 17]),
        [r1, r2, g1, g2, b1, b2] => Some([r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2]),
        _ => None,
    }
}

/// RGB em "#rrggbb".
pub fn to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn luminance(rgb: [u8; 3]) -> f32 {
    let channel = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2])
}

/// Razão de contraste entre duas cores (1 a 21), como no WCAG.
pub fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// As 8 cores prontas (nomes em `i18n::preset_color_name`) para `target`, tiradas do tema. Para o fundo, são as
/// mesmas cores bem misturadas ao fundo do tema, para o texto continuar
/// legível.
pub fn presets(theme: &ColorScheme, target: ColorTarget) -> [[u8; 3]; 8] {
    let ansi = |i: usize| {
        theme
            .ansi
            .get(i)
            .and_then(|hex| parse_hex(hex))
            .unwrap_or([128, 128, 128])
    };
    let bright = |i: usize| {
        theme
            .ansi_bright
            .get(i)
            .and_then(|hex| parse_hex(hex))
            .unwrap_or([160, 160, 160])
    };
    let orange = mix(ansi(1), ansi(3), 0.5);
    let base = [
        ansi(1),
        ansi(2),
        ansi(3),
        ansi(4),
        ansi(5),
        ansi(6),
        orange,
        bright(0),
    ];
    match target {
        ColorTarget::Tab => base,
        ColorTarget::Background => {
            // Bem misturada ao fundo: a mais forte que ainda deixa o texto
            // do tema legível (contraste mínimo do WCAG).
            let bg = parse_hex(&theme.background).unwrap_or([0, 0, 0]);
            base.map(|color| {
                [0.18, 0.14, 0.10, 0.06]
                    .into_iter()
                    .map(|t| mix(bg, color, t))
                    .find(|&tint| text_contrast(theme, tint) >= MIN_CONTRAST)
                    .unwrap_or_else(|| mix(bg, color, 0.03))
            })
        }
    }
}

/// Contraste do texto do tema sobre o fundo `background`.
pub fn text_contrast(theme: &ColorScheme, background: [u8; 3]) -> f32 {
    let fg = parse_hex(&theme.foreground).unwrap_or([255, 255, 255]);
    contrast_ratio(fg, background)
}

/// Cor da tab `index` na sidebar: a própria, senão a da tab de cima do
/// grupo (as de dentro herdam).
pub fn effective_tab_color(tabs: &TabManager, index: usize) -> Option<[u8; 3]> {
    let tab = tabs.tabs().get(index)?;
    tab.color.or_else(|| {
        tabs.parent_index(index)
            .and_then(|parent| tabs.tabs().get(parent))
            .and_then(|parent| parent.color)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip_and_short_form() {
        assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex("FF8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex(" #f80 "), Some([255, 136, 0]));
        assert_eq!(parse_hex("#ff80"), None);
        assert_eq!(parse_hex("#gg0000"), None);
        assert_eq!(parse_hex(""), None);
        assert_eq!(to_hex([255, 128, 0]), "#ff8000");
    }

    #[test]
    fn contrast_matches_wcag_extremes() {
        assert!((contrast_ratio([0, 0, 0], [255, 255, 255]) - 21.0).abs() < 0.01);
        assert!((contrast_ratio([10, 20, 30], [10, 20, 30]) - 1.0).abs() < 0.01);
        assert_eq!(
            contrast_ratio([0, 0, 0], [255, 255, 255]),
            contrast_ratio([255, 255, 255], [0, 0, 0])
        );
    }

    /// As cores prontas de fundo mantêm o texto legível em todos os temas:
    /// contraste de 4,5:1, ou quase o do próprio tema quando o tema já fica
    /// abaixo disso (solarized).
    #[test]
    fn background_presets_keep_text_readable_in_every_theme() {
        for name in crate::config::theme::Theme::BUILT_IN_THEMES {
            let mut config = crate::config::Config::default();
            assert!(crate::config::theme::Theme::apply_to_config(
                name,
                &mut config
            ));
            let theme_bg = parse_hex(&config.colors.background).unwrap();
            let own = text_contrast(&config.colors, theme_bg);
            for color in presets(&config.colors, ColorTarget::Background) {
                let ratio = text_contrast(&config.colors, color);
                assert!(
                    ratio >= MIN_CONTRAST || ratio >= own * 0.9,
                    "{name} {}: {ratio:.2} (tema {own:.2})",
                    to_hex(color)
                );
            }
            assert_eq!(presets(&config.colors, ColorTarget::Tab).len(), 8);
        }
    }

    #[test]
    fn children_inherit_the_group_color() {
        let mut tabs = TabManager::new();
        for _ in 0..3 {
            tabs.create_tab("bash");
        }
        tabs.nest_tab(1, 0);
        tabs.nest_tab(2, 0);
        tabs.get_tab_mut(0).unwrap().color = Some([1, 2, 3]);
        tabs.get_tab_mut(2).unwrap().color = Some([9, 9, 9]);
        assert_eq!(effective_tab_color(&tabs, 0), Some([1, 2, 3]));
        assert_eq!(effective_tab_color(&tabs, 1), Some([1, 2, 3]), "herda");
        assert_eq!(
            effective_tab_color(&tabs, 2),
            Some([9, 9, 9]),
            "a própria vence"
        );
        assert_eq!(effective_tab_color(&tabs, 7), None);
    }
}
