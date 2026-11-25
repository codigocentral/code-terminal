use crate::config::ColorScheme;
use anyhow::{Context, Result};
use std::path::PathBuf;

pub struct Theme {
    pub name: String,
    pub colors: ColorScheme,
}

impl Theme {
    /// Aplica um tema built-in ao config, alterando as cores.
    /// Retorna true se o tema foi reconhecido.
    pub fn apply_to_config(name: &str, config: &mut crate::config::Config) -> bool {
        let mut colors = config.colors.clone();
        match name.to_lowercase().as_str() {
            "dark" => {
                colors = ColorScheme::default();
                colors.name = "dark".to_string();
            }
            "light" => {
                colors.background = "#fafafa".to_string();
                colors.foreground = "#2e2e2e".to_string();
                colors.ansi = vec![
                    "#1e1e1e".to_string(),
                    "#cd3131".to_string(),
                    "#0dbc79".to_string(),
                    "#e5e510".to_string(),
                    "#2472c8".to_string(),
                    "#bc3fbc".to_string(),
                    "#11a8cd".to_string(),
                    "#e5e5e5".to_string(),
                ];
                colors.ansi_bright = vec![
                    "#666666".to_string(),
                    "#f14c4c".to_string(),
                    "#23d18b".to_string(),
                    "#f5f543".to_string(),
                    "#3b8eea".to_string(),
                    "#d670d6".to_string(),
                    "#29b8db".to_string(),
                    "#e5e5e5".to_string(),
                ];
                colors.cursor = "#2e2e2e".to_string();
                colors.cursor_background = "#2e2e2e".to_string();
                colors.selection = "#2e2e2e".to_string();
                colors.selection_background = "#add6ff".to_string();
                colors.highlight_background = "#d3d3d3".to_string();
                colors.highlight_current = "#a8a8a8".to_string();
                colors.name = "light".to_string();
            }
            other => match palette(other) {
                Some(palette) => palette.apply(&mut colors, other),
                None => return false,
            },
        }
        config.colors = colors;
        true
    }

    pub fn load() -> Result<Self> {
        let config = crate::config::Config::load()?;
        Self::from_config(&config.colors)
    }

    pub fn load_from(path: &PathBuf) -> Result<Self> {
        let config = crate::config::Config::load_from(path)?;
        Self::from_config(&config.colors)
    }

    fn from_config(colors: &ColorScheme) -> Result<Self> {
        let theme_name = if colors.random_theme {
            Self::random_theme_name()
        } else {
            colors.name.clone()
        };

        if !Self::is_valid_theme(&theme_name) {
            tracing::warn!(
                "Invalid theme '{}', falling back to default 'dark'",
                theme_name
            );
            return Ok(Self::default());
        }

        let mut theme_colors = colors.clone();
        theme_colors.name = theme_name.clone();

        Ok(Self {
            name: theme_name,
            colors: theme_colors,
        })
    }

    fn random_theme_name() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};

        let themes = Self::built_in_themes();
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as usize)
            .unwrap_or(0);

        let index = seed % themes.len();
        themes[index].to_string()
    }

    /// Temas built-in que `is_valid_theme` aceita, na ordem em que a
    /// configuração e a paleta os percorrem. É a única lista: o seletor de
    /// tema (`ui::settings::THEMES`) e o sorteio de `random_theme` usam esta.
    ///
    /// Os temas de terceiros têm licença MIT; ver `THIRD_PARTY_NOTICES.md`.
    pub const BUILT_IN_THEMES: &'static [&'static str] = &[
        "dark",
        "light",
        "dracula",
        "monokai",
        "nord",
        "one-dark",
        "solarized-dark",
        "solarized-light",
        "catppuccin-mocha",
        "catppuccin-latte",
        "tokyo-night",
        "gruvbox-dark",
        "gruvbox-light",
        "rose-pine",
        "rose-pine-dawn",
        "kanagawa",
        "everforest-dark",
        "ayu-dark",
    ];

    fn built_in_themes() -> Vec<&'static str> {
        Self::BUILT_IN_THEMES.to_vec()
    }

    pub fn is_valid_theme(name: &str) -> bool {
        Self::BUILT_IN_THEMES.contains(&name.to_lowercase().as_str())
    }

    pub fn fallback() -> Self {
        Self::default()
    }

    pub fn get_theme_dir() -> Result<PathBuf> {
        let config_dir = crate::config::parser::get_config_dir()?;
        Ok(config_dir.join("themes"))
    }

    pub fn load_custom_theme(theme_name: &str) -> Result<Option<Self>> {
        Self::load_custom_theme_from(&Self::get_theme_dir()?, theme_name)
    }

    /// Carrega `<theme_name>.toml` de um diretório de temas específico
    pub fn load_custom_theme_from(
        themes_dir: &std::path::Path,
        theme_name: &str,
    ) -> Result<Option<Self>> {
        let theme_file = themes_dir.join(format!("{}.toml", theme_name));

        if !theme_file.exists() {
            return Ok(None);
        }

        let content = std::fs::read_to_string(&theme_file)
            .with_context(|| format!("Failed to read theme file: {:?}", theme_file))?;

        let colors: ColorScheme = toml::from_str(&content)
            .with_context(|| format!("Failed to parse theme file: {:?}", theme_file))?;

        Ok(Some(Self {
            name: theme_name.to_string(),
            colors,
        }))
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "dark".to_string(),
            colors: ColorScheme::default(),
        }
    }
}

/// Cores de um tema embutido (valores públicos de cada esquema).
struct Palette {
    background: &'static str,
    foreground: &'static str,
    ansi: [&'static str; 8],
    bright: [&'static str; 8],
    cursor: &'static str,
    selection_background: &'static str,
    highlight_background: &'static str,
    highlight_current: &'static str,
}

impl Palette {
    fn apply(&self, colors: &mut ColorScheme, name: &str) {
        let owned = |list: &[&str; 8]| list.iter().map(|c| c.to_string()).collect::<Vec<_>>();
        colors.name = name.to_string();
        colors.background = self.background.to_string();
        colors.foreground = self.foreground.to_string();
        colors.ansi = owned(&self.ansi);
        colors.ansi_bright = owned(&self.bright);
        colors.cursor = self.cursor.to_string();
        colors.cursor_background = self.cursor.to_string();
        colors.selection = self.foreground.to_string();
        colors.selection_background = self.selection_background.to_string();
        colors.highlight_background = self.highlight_background.to_string();
        colors.highlight_current = self.highlight_current.to_string();
    }
}

fn palette(name: &str) -> Option<Palette> {
    Some(match name {
        "dracula" => Palette {
            background: "#282a36",
            foreground: "#f8f8f2",
            ansi: [
                "#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd",
                "#f8f8f2",
            ],
            bright: [
                "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff",
                "#ffffff",
            ],
            cursor: "#f8f8f2",
            selection_background: "#44475a",
            highlight_background: "#6272a4",
            highlight_current: "#ffb86c",
        },
        "monokai" => Palette {
            background: "#272822",
            foreground: "#f8f8f2",
            ansi: [
                "#272822", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4",
                "#f8f8f2",
            ],
            bright: [
                "#75715e", "#f92672", "#a6e22e", "#f4bf75", "#66d9ef", "#ae81ff", "#a1efe4",
                "#f9f8f5",
            ],
            cursor: "#f8f8f0",
            selection_background: "#49483e",
            highlight_background: "#75715e",
            highlight_current: "#e6db74",
        },
        "nord" => Palette {
            background: "#2e3440",
            foreground: "#d8dee9",
            ansi: [
                "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0",
                "#e5e9f0",
            ],
            bright: [
                "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb",
                "#eceff4",
            ],
            cursor: "#d8dee9",
            selection_background: "#434c5e",
            highlight_background: "#4c566a",
            highlight_current: "#ebcb8b",
        },
        "one-dark" => Palette {
            background: "#282c34",
            foreground: "#abb2bf",
            ansi: [
                "#282c34", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2",
                "#abb2bf",
            ],
            bright: [
                "#5c6370", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2",
                "#ffffff",
            ],
            cursor: "#528bff",
            selection_background: "#3e4451",
            highlight_background: "#5c6370",
            highlight_current: "#d19a66",
        },
        "solarized-dark" => Palette {
            background: "#002b36",
            foreground: "#839496",
            ansi: [
                "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198",
                "#eee8d5",
            ],
            bright: [
                "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1",
                "#fdf6e3",
            ],
            cursor: "#93a1a1",
            selection_background: "#073642",
            highlight_background: "#586e75",
            highlight_current: "#b58900",
        },
        "solarized-light" => Palette {
            background: "#fdf6e3",
            foreground: "#657b83",
            ansi: [
                "#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198",
                "#eee8d5",
            ],
            bright: [
                "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1",
                "#fdf6e3",
            ],
            cursor: "#586e75",
            selection_background: "#eee8d5",
            highlight_background: "#93a1a1",
            highlight_current: "#b58900",
        },
        // Catppuccin (MIT) — https://github.com/catppuccin/catppuccin
        "catppuccin-mocha" => Palette {
            background: "#1e1e2e",
            foreground: "#cdd6f4",
            ansi: [
                "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5",
                "#bac2de",
            ],
            bright: [
                "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5",
                "#a6adc8",
            ],
            cursor: "#f5e0dc",
            selection_background: "#45475a",
            highlight_background: "#585b70",
            highlight_current: "#f9e2af",
        },
        "catppuccin-latte" => Palette {
            background: "#eff1f5",
            foreground: "#4c4f69",
            ansi: [
                "#5c5f77", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299",
                "#acb0be",
            ],
            bright: [
                "#6c6f85", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299",
                "#bcc0cc",
            ],
            cursor: "#dc8a78",
            selection_background: "#ccd0da",
            highlight_background: "#bcc0cc",
            highlight_current: "#df8e1d",
        },
        // Tokyo Night (MIT) — https://github.com/enkia/tokyo-night-vscode-theme
        "tokyo-night" => Palette {
            background: "#1a1b26",
            foreground: "#c0caf5",
            ansi: [
                "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff",
                "#a9b1d6",
            ],
            bright: [
                "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff",
                "#c0caf5",
            ],
            cursor: "#c0caf5",
            selection_background: "#283457",
            highlight_background: "#414868",
            highlight_current: "#e0af68",
        },
        // Gruvbox (MIT) — https://github.com/morhetz/gruvbox
        "gruvbox-dark" => Palette {
            background: "#282828",
            foreground: "#ebdbb2",
            ansi: [
                "#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a",
                "#a89984",
            ],
            bright: [
                "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c",
                "#ebdbb2",
            ],
            cursor: "#ebdbb2",
            selection_background: "#504945",
            highlight_background: "#665c54",
            highlight_current: "#fabd2f",
        },
        "gruvbox-light" => Palette {
            background: "#fbf1c7",
            foreground: "#3c3836",
            ansi: [
                "#fbf1c7", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a",
                "#7c6f64",
            ],
            bright: [
                "#928374", "#9d0006", "#79740e", "#b57614", "#076678", "#8f3f71", "#427b58",
                "#3c3836",
            ],
            cursor: "#3c3836",
            selection_background: "#d5c4a1",
            highlight_background: "#bdae93",
            highlight_current: "#b57614",
        },
        // Rosé Pine (MIT) — https://github.com/rose-pine/rose-pine-theme
        "rose-pine" => Palette {
            background: "#191724",
            foreground: "#e0def4",
            ansi: [
                "#26233a", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba",
                "#e0def4",
            ],
            bright: [
                "#6e6a86", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba",
                "#e0def4",
            ],
            cursor: "#e0def4",
            selection_background: "#403d52",
            highlight_background: "#524f67",
            highlight_current: "#f6c177",
        },
        "rose-pine-dawn" => Palette {
            background: "#faf4ed",
            foreground: "#575279",
            ansi: [
                "#f2e9e1", "#b4637a", "#286983", "#ea9d34", "#56949f", "#907aa9", "#d7827e",
                "#575279",
            ],
            bright: [
                "#9893a5", "#b4637a", "#286983", "#ea9d34", "#56949f", "#907aa9", "#d7827e",
                "#575279",
            ],
            cursor: "#575279",
            selection_background: "#dfdad9",
            highlight_background: "#cecacd",
            highlight_current: "#ea9d34",
        },
        // Kanagawa (MIT) — https://github.com/rebelot/kanagawa.nvim
        "kanagawa" => Palette {
            background: "#1f1f28",
            foreground: "#dcd7ba",
            ansi: [
                "#16161d", "#c34043", "#76946a", "#c0a36e", "#7e9cd8", "#957fb8", "#6a9589",
                "#c8c093",
            ],
            bright: [
                "#727169", "#e82424", "#98bb6c", "#e6c384", "#7fb4ca", "#938aa9", "#7aa89f",
                "#dcd7ba",
            ],
            cursor: "#c8c093",
            selection_background: "#2d4f67",
            highlight_background: "#54546d",
            highlight_current: "#ffa066",
        },
        // Everforest (MIT) — https://github.com/sainnhe/everforest
        "everforest-dark" => Palette {
            background: "#2d353b",
            foreground: "#d3c6aa",
            ansi: [
                "#343f44", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092",
                "#d3c6aa",
            ],
            bright: [
                "#859289", "#e67e80", "#a7c080", "#dbbc7f", "#7fbbb3", "#d699b6", "#83c092",
                "#d3c6aa",
            ],
            cursor: "#d3c6aa",
            selection_background: "#475258",
            highlight_background: "#56635f",
            highlight_current: "#dbbc7f",
        },
        // Ayu (MIT) — https://github.com/ayu-theme/ayu-colors
        "ayu-dark" => Palette {
            background: "#0b0e14",
            foreground: "#bfbdb6",
            ansi: [
                "#01060e", "#ea6c73", "#7fd962", "#f9af4f", "#53bdfa", "#cda1fa", "#90e1c6",
                "#c7c7c7",
            ],
            bright: [
                "#686868", "#f07178", "#aad94c", "#ffb454", "#59c2ff", "#d2a6ff", "#95e6cb",
                "#ffffff",
            ],
            cursor: "#e6b450",
            selection_background: "#273747",
            highlight_background: "#3d424d",
            highlight_current: "#e6b450",
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {

    /// Todo tema oferecido na configuração e na paleta tem cores próprias
    /// (antes só "dark" e "light" mudavam algo; os outros só trocavam o nome).
    #[test]
    fn test_every_built_in_theme_has_its_own_palette() {
        let mut backgrounds = std::collections::HashSet::new();
        for name in Theme::BUILT_IN_THEMES {
            let mut config = crate::config::Config::default();
            assert!(Theme::apply_to_config(name, &mut config), "{name}");
            assert_eq!(config.colors.name, *name);
            assert_eq!(config.colors.ansi.len(), 8);
            assert_eq!(config.colors.ansi_bright.len(), 8);
            assert!(
                backgrounds.insert(config.colors.background.clone()),
                "{name} repete fundo"
            );
        }
        for name in crate::ui::settings::THEMES {
            assert!(Theme::is_valid_theme(name), "{name}");
        }
    }

    /// Luminância relativa (WCAG 2.x) de uma cor `#rrggbb`.
    fn luminance(hex: &str) -> f64 {
        let channel = |i: usize| {
            let v = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap()) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
    }

    fn contrast(a: &str, b: &str) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// Todo tema embutido deixa o texto legível: contraste de pelo menos
    /// 4,5:1 (WCAG AA) entre o texto e o fundo.
    ///
    /// Exceção: o Solarized Light oficial fica em 4,13:1 (base00 sobre base3).
    /// Mantemos as cores do autor e exigimos pelo menos 4,0 dele.
    #[test]
    fn test_every_built_in_theme_has_readable_text() {
        for name in Theme::BUILT_IN_THEMES {
            let mut config = crate::config::Config::default();
            assert!(Theme::apply_to_config(name, &mut config), "{name}");
            let ratio = contrast(&config.colors.foreground, &config.colors.background);
            let minimum = if *name == "solarized-light" { 4.0 } else { 4.5 };
            assert!(ratio >= minimum, "{name}: contraste {ratio:.2}");
        }
    }

    /// Os temas novos estão no seletor e no sorteio.
    #[test]
    fn test_new_themes_are_offered() {
        for name in [
            "catppuccin-mocha",
            "catppuccin-latte",
            "tokyo-night",
            "gruvbox-dark",
            "gruvbox-light",
            "rose-pine",
            "rose-pine-dawn",
            "kanagawa",
            "everforest-dark",
            "ayu-dark",
        ] {
            assert!(Theme::is_valid_theme(name), "{name}");
            assert!(crate::ui::settings::THEMES.contains(&name), "{name}");
        }
        assert!(Theme::BUILT_IN_THEMES.len() >= 16);
    }

    use super::*;

    #[test]
    fn test_default_theme() {
        let theme = Theme::default();
        assert_eq!(theme.name, "dark");
    }

    #[test]
    fn test_is_valid_theme() {
        assert!(Theme::is_valid_theme("dark"));
        assert!(Theme::is_valid_theme("DRACULA"));
        assert!(Theme::is_valid_theme("light"));
        assert!(!Theme::is_valid_theme("nonexistent"));
        assert!(!Theme::is_valid_theme(""));
    }

    /// Regressão: o sorteio podia escolher temas que não são válidos.
    #[test]
    fn test_random_theme_is_always_valid() {
        for _ in 0..50 {
            assert!(Theme::is_valid_theme(&Theme::random_theme_name()));
        }
        assert!(Theme::built_in_themes()
            .iter()
            .all(|t| Theme::is_valid_theme(t)));
    }

    #[test]
    fn test_apply_to_config() {
        let mut config = crate::config::Config::default();
        assert!(Theme::apply_to_config("LIGHT", &mut config));
        assert_eq!(config.colors.name, "light");
        assert_eq!(config.colors.background, "#fafafa");
        assert_eq!(config.colors.ansi.len(), 8);

        assert!(Theme::apply_to_config("dark", &mut config));
        assert_eq!(config.colors.name, "dark");
        assert_eq!(config.colors.background, ColorScheme::default().background);

        assert!(!Theme::apply_to_config("inexistente", &mut config));
        assert_eq!(config.colors.name, "dark");
    }

    #[test]
    fn test_from_config_validates_and_randomizes() {
        let mut colors = ColorScheme {
            name: "Nord".to_string(),
            ..ColorScheme::default()
        };
        let theme = Theme::from_config(&colors).unwrap();
        assert_eq!(theme.name, "Nord");
        assert_eq!(theme.colors.name, "Nord");

        colors.name = "nao-existe".to_string();
        let theme = Theme::from_config(&colors).unwrap();
        assert_eq!(theme.name, Theme::fallback().name);

        colors.random_theme = true;
        let theme = Theme::from_config(&colors).unwrap();
        assert!(Theme::is_valid_theme(&theme.name));
    }

    #[test]
    fn test_load_from_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = crate::config::Config::default();
        config.colors.name = "dracula".to_string();
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();

        let theme = Theme::load_from(&path).unwrap();
        assert_eq!(theme.name, "dracula");
    }

    #[test]
    fn test_load_custom_theme_from_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Theme::load_custom_theme_from(dir.path(), "meu")
            .unwrap()
            .is_none());

        let colors = ColorScheme {
            name: "meu".to_string(),
            background: "#000001".to_string(),
            ..ColorScheme::default()
        };
        std::fs::write(
            dir.path().join("meu.toml"),
            toml::to_string(&colors).unwrap(),
        )
        .unwrap();
        let theme = Theme::load_custom_theme_from(dir.path(), "meu")
            .unwrap()
            .unwrap();
        assert_eq!(theme.name, "meu");
        assert_eq!(theme.colors.background, "#000001");

        std::fs::write(dir.path().join("quebrado.toml"), "isso não é toml = = =").unwrap();
        let error = Theme::load_custom_theme_from(dir.path(), "quebrado")
            .err()
            .unwrap();
        assert!(
            error.to_string().contains("Failed to parse theme file"),
            "{error}"
        );

        assert!(Theme::get_theme_dir().unwrap().ends_with("themes"));
    }
}
