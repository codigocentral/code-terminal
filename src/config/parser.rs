use super::Config;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Nome da configuração para o `confy`, que acrescenta a extensão `.toml` sozinho.
/// Passar `"config.toml"` gerava `config.toml.toml`.
const CONFIG_NAME: &str = "config";

/// Nome do arquivo gravado pelas versões que passavam a extensão ao `confy`.
const LEGACY_CONFIG_FILE_NAME: &str = "config.toml.toml";

/// Diretório da aplicação
fn get_app_name() -> String {
    "code-terminal".to_string()
}

/// Variável de ambiente que troca o diretório de configuração
/// (perfis isolados, testes de integração).
pub const CONFIG_DIR_ENV: &str = "CODE_TERMINAL_CONFIG_DIR";

/// Diretório que substitui o padrão, se houver.
fn override_config_dir() -> Option<PathBuf> {
    std::env::var_os(CONFIG_DIR_ENV)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(unit_test_config_dir)
}

/// Nos testes de unidade a configuração vai para um diretório temporário:
/// um teste que salva configurações não pode sobrescrever a do usuário.
#[cfg(test)]
fn unit_test_config_dir() -> Option<PathBuf> {
    Some(std::env::temp_dir().join(format!("code-terminal-unit-tests-{}", std::process::id())))
}

#[cfg(not(test))]
fn unit_test_config_dir() -> Option<PathBuf> {
    None
}

/// Retorna o diretório de configuração
pub fn get_config_dir() -> Result<PathBuf> {
    let path = get_config_path().context("Failed to get configuration directory")?;

    // Pega o diretório pai (remove o arquivo)
    Ok(path.parent().unwrap_or(&path).to_path_buf())
}

/// Retorna o caminho completo do arquivo de configuração
pub fn get_config_path() -> Result<PathBuf> {
    if let Some(dir) = override_config_dir() {
        return Ok(dir.join(format!("{CONFIG_NAME}.toml")));
    }
    let path = confy::get_configuration_file_path(&get_app_name(), Some(CONFIG_NAME))
        .context("Failed to get configuration file path")?;
    Ok(path)
}

/// Carrega a configuração do arquivo padrão
/// Se o arquivo não existir, cria um com valores padrão
pub fn load_config() -> Result<Config> {
    let path = get_config_path()?;
    if let Err(e) = migrate_legacy_config(&path) {
        tracing::warn!(error = %e, "Failed to migrate legacy config file");
    }

    if !path.exists() {
        tracing::info!("Config file not found, creating default at {:?}", path);
        let config = Config::default();
        save_config(&config)?;
        return Ok(config);
    }

    let mut config = load_config_from(&path)?;
    if config.keybindings.upgrade_legacy_defaults() {
        tracing::info!("Upgraded legacy shortcuts that captured shell control keys");
        if let Err(e) = save_config_to(&config, &path) {
            tracing::warn!(error = %e, "Failed to save upgraded shortcuts");
        }
    }
    Ok(config)
}

/// Renomeia o `config.toml.toml` antigo para `path` quando só o antigo existe.
/// Retorna `true` se houve migração.
pub fn migrate_legacy_config(path: &Path) -> Result<bool> {
    let Some(dir) = path.parent() else {
        return Ok(false);
    };
    let legacy = dir.join(LEGACY_CONFIG_FILE_NAME);
    if path.exists() || !legacy.exists() {
        return Ok(false);
    }
    fs::rename(&legacy, path)
        .with_context(|| format!("Failed to rename {:?} to {:?}", legacy, path))?;
    tracing::info!("Migrated legacy config {:?} to {:?}", legacy, path);
    Ok(true)
}

/// Carrega a configuração de um caminho específico
pub fn load_config_from(path: &PathBuf) -> Result<Config> {
    tracing::info!("Loading config from {:?}", path);

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {:?}", path))?;

    parse_config(&content)
}

/// Faz o parse de uma string TOML para Config
pub fn parse_config(content: &str) -> Result<Config> {
    let config: Config = toml::from_str(content).context("Failed to parse config TOML")?;

    tracing::debug!("Config loaded successfully");
    Ok(config)
}

/// Salva a configuração no arquivo padrão
pub fn save_config(config: &Config) -> Result<()> {
    let path = get_config_path()?;
    save_config_to(config, &path)
}

/// Salva a configuração em um caminho específico
pub fn save_config_to(config: &Config, path: &PathBuf) -> Result<()> {
    // Garante que o diretório existe
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
    }

    let content = toml::to_string_pretty(config).context("Failed to serialize config to TOML")?;

    fs::write(path, content).with_context(|| format!("Failed to write config file: {:?}", path))?;

    tracing::info!("Config saved to {:?}", path);
    Ok(())
}

/// Cria um arquivo de configuração de exemplo
pub fn create_example_config() -> Result<String> {
    Ok(r##"# Code Terminal Configuration File
# Location: ~/.config/code-terminal/config.toml

[font]
family = "JetBrains Mono"
size = 12.0
line_height = 1.2
bold = true
italic = true

[colors]
name = "dark"
background = "#1e1e1e"
foreground = "#d4d4d4"
ansi = [
    "#1e1e1e",
    "#cd3131",
    "#0dbc79",
    "#e5e510",
    "#2472c8",
    "#bc3fbc",
    "#11a8cd",
    "#e5e5e5",
]
ansi_bright = [
    "#666666",
    "#f14c4c",
    "#23d18b",
    "#f5f543",
    "#3b8eea",
    "#d670d6",
    "#29b8db",
    "#e5e5e5",
]
cursor = "#d4d4d4"
cursor_background = "#d4d4d4"
selection = "#d4d4d4"
selection_background = "#264f78"

[terminal]
shell = ""
working_directory = ""
scrollback_limit = 10000
cursor_blink = true
cursor_style = "block"

[window]
title = "Code Terminal"
width = 800
height = 600
opacity = 1.0
decorations = true
always_on_top = false

# Keyboard Shortcuts
# Format: "modifier+key" (ctrl, shift, alt)
[keybindings]
new_tab = "ctrl+shift+t"
close_tab = "ctrl+w"
next_tab = "ctrl+tab"
previous_tab = "ctrl+shift+tab"
go_to_tab_prefix = "ctrl"  # Ctrl+1-9 to switch to specific tab
width = 800
height = 600
opacity = 1.0
decorations = true
always_on_top = false
"##
    .to_string())
}

/// Verifica se o arquivo de configuração existe
pub fn config_exists() -> bool {
    get_config_path().map(|p| p.exists()).unwrap_or(false)
}

/// Retorna um exemplo de configuração formatado
pub fn get_example_config() -> String {
    create_example_config().unwrap_or_else(|_| Config::default().to_string())
}

use std::fmt;

impl fmt::Display for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match toml::to_string_pretty(self) {
            Ok(s) => write!(f, "{}", s),
            Err(_) => write!(f, "<Config>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_path_has_single_toml_extension() {
        let path = get_config_path().unwrap();
        assert_eq!(path.file_name().unwrap(), "config.toml");
    }

    /// Regressão: `test_settings_save_changes_config` gravava o tema "light"
    /// na `~/.config/code-terminal/config.toml` de quem rodava os testes.
    #[test]
    fn test_unit_tests_never_touch_real_config() {
        let path = get_config_path().unwrap();
        assert!(path.starts_with(std::env::temp_dir()), "{path:?}");
        if let Some(real_dir) = dirs::config_dir() {
            assert!(
                !path.starts_with(real_dir.join("code-terminal")),
                "{path:?}"
            );
        }
        assert_eq!(get_config_dir().unwrap(), path.parent().unwrap());
    }

    #[test]
    fn test_migrate_legacy_config_renames_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            dir.path().join(LEGACY_CONFIG_FILE_NAME),
            "[font]\nsize = 20.0\n",
        )
        .unwrap();

        assert!(migrate_legacy_config(&path).unwrap());
        assert!(path.exists());
        assert!(!dir.path().join(LEGACY_CONFIG_FILE_NAME).exists());
        assert_eq!(load_config_from(&path).unwrap().font.size, 20.0);
    }

    #[test]
    fn test_migrate_legacy_config_keeps_current_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[font]\nsize = 11.0\n").unwrap();
        fs::write(
            dir.path().join(LEGACY_CONFIG_FILE_NAME),
            "[font]\nsize = 20.0\n",
        )
        .unwrap();

        assert!(!migrate_legacy_config(&path).unwrap());
        assert_eq!(load_config_from(&path).unwrap().font.size, 11.0);
    }

    #[test]
    fn test_migrate_legacy_config_without_files_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!migrate_legacy_config(&dir.path().join("config.toml")).unwrap());
    }

    #[test]
    fn test_parse_config() {
        let toml = r##"
[font]
family = "Fira Code"
size = 14.0

[colors]
name = "custom"
background = "#000000"
foreground = "#ffffff"
ansi = ["#000000", "#ff0000", "#00ff00", "#ffff00", "#0000ff", "#ff00ff", "#00ffff", "#ffffff"]
ansi_bright = ["#808080", "#ff8080", "#80ff80", "#ffff80", "#8080ff", "#ff80ff", "#80ffff", "#ffffff"]
cursor = "#ffffff"
cursor_background = "#ffffff"
selection = "#ffffff"
selection_background = "#404040"

[terminal]
scrollback_limit = 5000
cursor_blink = false
cursor_style = "line"

[window]
width = 1024
height = 768
opacity = 0.95
"##;

        let config = parse_config(toml).unwrap();
        assert_eq!(config.font.family, "Fira Code");
        assert_eq!(config.font.size, 14.0);
        assert_eq!(config.colors.name, "custom");
        assert_eq!(config.terminal.scrollback_limit, 5000);
        assert_eq!(config.window.width, 1024);
    }

    #[test]
    fn test_parse_minimal_config() {
        let toml = r#"
[font]
size = 16.0
"#;

        let config = parse_config(toml).unwrap();
        assert_eq!(config.font.size, 16.0);
        // Defaults should be filled in
        assert_eq!(config.font.family, "JetBrains Mono");
        assert_eq!(config.terminal.scrollback_limit, 10000);
    }

    #[test]
    fn test_save_and_load_config() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let config = Config {
            font: super::super::FontConfig {
                family: "Test Font".to_string(),
                size: 13.0,
                line_height: 1.5,
                bold: false,
                italic: true,
                fallback: vec![],
                ligatures: true,
            },
            ..Config::default()
        };

        let toml = toml::to_string_pretty(&config).unwrap();
        temp_file.write_all(toml.as_bytes()).unwrap();

        let loaded = load_config_from(&temp_file.path().to_path_buf()).unwrap();
        assert_eq!(loaded.font.family, "Test Font");
        assert_eq!(loaded.font.size, 13.0);
        assert!(!loaded.font.bold);
    }

    #[test]
    fn test_create_example_config() {
        let example = create_example_config().unwrap();
        assert!(example.contains("[font]"));
        assert!(example.contains("[colors]"));
        assert!(example.contains("[terminal]"));
        assert!(example.contains("[window]"));
    }

    #[test]
    fn test_invalid_config() {
        let toml = "invalid toml content {{{";
        assert!(parse_config(toml).is_err());
    }
}
