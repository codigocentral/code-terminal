use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub mod ai;
pub mod keybindings;
pub mod parser;
pub mod theme;
pub mod watcher;

pub use parser::{
    config_exists, get_config_path, load_config, load_config_from, parse_config, save_config,
    save_config_to,
};

pub use keybindings::{
    ClipboardShortcuts, KeyBindings, MultiplexerKeyBindings, MultiplexerShortcuts, SearchShortcuts,
    Shortcut, TabShortcuts,
};

/// Configuração principal do terminal
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Idioma da interface: "auto" (o do sistema), "en" ou "pt-BR". Fica
    /// antes das tabelas por causa do formato TOML.
    #[serde(default = "default_language")]
    pub language: String,

    /// Configurações de fonte
    #[serde(default)]
    pub font: FontConfig,

    /// Configurações de cores
    #[serde(default)]
    pub colors: ColorScheme,

    /// Configurações de terminal
    #[serde(default)]
    pub terminal: TerminalConfig,

    /// Configurações de janela
    #[serde(default)]
    pub window: WindowConfig,

    /// Atalhos de teclado
    #[serde(default)]
    pub keybindings: keybindings::KeyBindings,

    /// Atalhos de teclado para modo Multiplexer
    #[serde(default)]
    pub multiplexer_keybindings: keybindings::MultiplexerKeyBindings,

    /// Configurações de AI
    #[serde(default)]
    pub ai: ai::AIConfig,

    /// Sidebar de abas verticais
    #[serde(default)]
    pub sidebar: SidebarSettings,

    /// Sessão (restaurar tabs e painéis ao abrir)
    #[serde(default)]
    pub session: SessionSettings,

    /// Navegador (onde abrem os links clicados com Ctrl)
    #[serde(default)]
    pub browser: BrowserSettings,

    /// Modo servidor (`code-terminal --server`)
    #[serde(default)]
    pub server: ServerSettings,
}

/// Modo servidor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerSettings {
    /// Sem nenhuma janela aberta, soltar a GPU depois deste tempo ("30s",
    /// "5m", "1h", ou segundos). "0" ou "never" nunca solta. Solta, o driver
    /// devolve a memória dele (~35–70 MB) e a thread que acorda 100 vezes/s
    /// some; a próxima janela leva ~150–240 ms em vez de ~35 ms (#1).
    #[serde(default = "default_release_gpu_after")]
    pub release_gpu_after: String,
}

/// Padrão de [`ServerSettings::release_gpu_after`].
pub const DEFAULT_RELEASE_GPU_AFTER: &str = "5m";

fn default_release_gpu_after() -> String {
    DEFAULT_RELEASE_GPU_AFTER.to_string()
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            release_gpu_after: default_release_gpu_after(),
        }
    }
}

impl ServerSettings {
    /// Prazo para soltar a GPU sem janela; `None` = nunca. Valor inválido
    /// vale o padrão (5 min).
    pub fn release_gpu_after(&self) -> Option<std::time::Duration> {
        parse_release_after(&self.release_gpu_after).unwrap_or_else(|| {
            tracing::warn!(
                value = %self.release_gpu_after,
                "Invalid server.release_gpu_after, using {DEFAULT_RELEASE_GPU_AFTER}"
            );
            parse_release_after(DEFAULT_RELEASE_GPU_AFTER).flatten()
        })
    }
}

/// Lê "30s", "5m", "1h", "90" (segundos), "0"/"never" (nunca: `Some(None)`).
/// `None` se não der para ler.
pub fn parse_release_after(value: &str) -> Option<Option<std::time::Duration>> {
    let value = value.trim().to_ascii_lowercase();
    if value == "never" || value == "0" {
        return Some(None);
    }
    let (number, unit) = match value.find(|c: char| !c.is_ascii_digit()) {
        Some(at) => value.split_at(at),
        None => (value.as_str(), "s"),
    };
    let number: u64 = number.parse().ok()?;
    let secs = match unit.trim() {
        "s" | "sec" | "secs" => number,
        "m" | "min" => number.checked_mul(60)?,
        "h" => number.checked_mul(3600)?,
        _ => return None,
    };
    Some(Some(std::time::Duration::from_secs(secs)).filter(|d| !d.is_zero()))
}

fn default_language() -> String {
    "auto".to_string()
}

/// Onde abrir links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkTarget {
    /// Janela do navegador do Code Terminal (`code-terminal-browser`).
    #[default]
    Builtin,
    /// Navegador padrão do sistema.
    System,
}

/// Navegador.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BrowserSettings {
    /// Ctrl+clique num link abre aqui.
    #[serde(default)]
    pub open_links_in: LinkTarget,
}

/// Restauração do workspace ao abrir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSettings {
    /// Reabrir as tabs, painéis e diretórios da última vez.
    #[serde(default = "default_session_restore")]
    pub restore: bool,
    /// Toda tab nova já nasce persistente (o shell vive no daemon e
    /// sobrevive a fechar a janela). Padrão: só as marcadas, uma a uma.
    #[serde(default)]
    pub persistent_tabs: bool,
    /// Gravar em disco o histórico e os dados das abas persistentes
    /// (`~/.local/state/code-terminal/tabs/`, só o próprio usuário lê), para
    /// elas voltarem depois de reiniciar a máquina. Desligar apaga o que houver.
    #[serde(default = "default_true")]
    pub save_scrollback: bool,
}

fn default_session_restore() -> bool {
    true
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            restore: default_session_restore(),
            persistent_tabs: false,
            save_scrollback: true,
        }
    }
}

/// Sidebar de abas verticais (diretório, branch, portas e avisos por tab).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SidebarSettings {
    /// Mostrar a sidebar ao abrir.
    #[serde(default = "default_sidebar_visible")]
    pub visible: bool,
    /// Largura em pixels lógicos.
    #[serde(default = "default_sidebar_width")]
    pub width: f32,
    /// Mostrar a memória de cada tab (soma do PSS do shell e dos filhos).
    #[serde(default = "default_true")]
    pub show_memory: bool,
}

fn default_sidebar_visible() -> bool {
    true
}

fn default_sidebar_width() -> f32 {
    230.0
}

impl Default for SidebarSettings {
    fn default() -> Self {
        Self {
            visible: default_sidebar_visible(),
            width: default_sidebar_width(),
            show_memory: true,
        }
    }
}

impl Config {
    /// Carrega a configuração do arquivo padrão
    pub fn load() -> Result<Self> {
        parser::load_config()
    }

    /// Carrega a configuração de um caminho específico
    pub fn load_from(path: &PathBuf) -> Result<Self> {
        parser::load_config_from(path)
    }

    /// Salva a configuração no arquivo padrão
    pub fn save(&self) -> Result<()> {
        parser::save_config(self)
    }

    /// Retorna o caminho do arquivo de configuração
    pub fn config_path() -> Result<PathBuf> {
        parser::get_config_path()
    }
}

/// Configurações de fonte
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontConfig {
    /// Nome da fonte (família)
    #[serde(default = "default_font_family")]
    pub family: String,

    /// Tamanho da fonte em pontos
    #[serde(default = "default_font_size")]
    pub size: f32,

    /// Espaçamento entre linhas (multiplicador)
    #[serde(default = "default_line_height")]
    pub line_height: f32,

    /// Usar fonte bold para textos em negrito
    #[serde(default = "default_true")]
    pub bold: bool,

    /// Usar fonte italic para textos em itálico
    #[serde(default = "default_true")]
    pub italic: bool,

    /// Habilitar ligatures (como ->, =>, ==)
    #[serde(default = "default_true")]
    pub ligatures: bool,

    /// Lista de fontes fallback (separadas por vírgula)
    #[serde(default = "default_font_fallback")]
    pub fallback: Vec<String>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: default_font_family(),
            size: default_font_size(),
            line_height: default_line_height(),
            bold: true,
            italic: true,
            ligatures: true,
            fallback: default_font_fallback(),
        }
    }
}

fn default_font_family() -> String {
    "JetBrains Mono".to_string()
}

fn default_font_size() -> f32 {
    12.0
}

fn default_line_height() -> f32 {
    1.2
}

fn default_font_fallback() -> Vec<String> {
    vec![
        "Fira Code".to_string(),
        "Source Code Pro".to_string(),
        "Cascadia Code".to_string(),
        "Consolas".to_string(),
        "Monaco".to_string(),
        "Menlo".to_string(),
        "DejaVu Sans Mono".to_string(),
        "Ubuntu Mono".to_string(),
    ]
}

/// Configurações de cores
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorScheme {
    /// Nome do tema
    #[serde(default = "default_theme_name")]
    pub name: String,

    /// Usar tema aleatório ao iniciar
    #[serde(default = "default_false")]
    pub random_theme: bool,

    /// Cor de fundo
    #[serde(default = "default_background")]
    pub background: String,

    /// Cor do texto
    #[serde(default = "default_foreground")]
    pub foreground: String,

    /// Cores ANSI 0-7 (normal)
    #[serde(default = "default_ansi_colors")]
    pub ansi: Vec<String>,

    /// Cores ANSI 8-15 (bright)
    #[serde(default = "default_ansi_bright")]
    pub ansi_bright: Vec<String>,

    /// Cor do cursor
    #[serde(default = "default_cursor")]
    pub cursor: String,

    /// Cor de fundo do cursor (inverse ou cor específica)
    #[serde(default = "default_cursor_background")]
    pub cursor_background: String,

    /// Cor de seleção
    #[serde(default = "default_foreground")]
    pub selection: String,

    /// Cor de fundo da seleção
    #[serde(default = "default_selection_background")]
    pub selection_background: String,

    /// Cor de fundo do highlight de busca
    #[serde(default = "default_highlight")]
    pub highlight_background: String,

    /// Cor do highlight do match atual
    #[serde(default = "default_highlight_current")]
    pub highlight_current: String,
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self {
            name: default_theme_name(),
            random_theme: false,
            background: default_background(),
            foreground: default_foreground(),
            ansi: default_ansi_colors(),
            ansi_bright: default_ansi_bright(),
            cursor: default_cursor(),
            cursor_background: default_cursor_background(),
            selection: default_foreground(),
            selection_background: default_selection_background(),
            highlight_background: default_highlight(),
            highlight_current: default_highlight_current(),
        }
    }
}

fn default_theme_name() -> String {
    "dark".to_string()
}

fn default_background() -> String {
    "#1e1e1e".to_string()
}

fn default_foreground() -> String {
    "#d4d4d4".to_string()
}

fn default_cursor() -> String {
    "#d4d4d4".to_string()
}

fn default_cursor_background() -> String {
    "#d4d4d4".to_string()
}

fn default_selection_background() -> String {
    "#264f78".to_string()
}

fn default_highlight() -> String {
    "#ffff00".to_string()
}

fn default_highlight_current() -> String {
    "#ffaa00".to_string()
}

fn default_ansi_colors() -> Vec<String> {
    vec![
        "#1e1e1e".to_string(), // black
        "#cd3131".to_string(), // red
        "#0dbc79".to_string(), // green
        "#e5e510".to_string(), // yellow
        "#2472c8".to_string(), // blue
        "#bc3fbc".to_string(), // magenta
        "#11a8cd".to_string(), // cyan
        "#e5e5e5".to_string(), // white
    ]
}

fn default_ansi_bright() -> Vec<String> {
    vec![
        "#666666".to_string(), // bright black
        "#f14c4c".to_string(), // bright red
        "#23d18b".to_string(), // bright green
        "#f5f543".to_string(), // bright yellow
        "#3b8eea".to_string(), // bright blue
        "#d670d6".to_string(), // bright magenta
        "#29b8db".to_string(), // bright cyan
        "#e5e5e5".to_string(), // bright white
    ]
}

/// Configurações de terminal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    /// Shell padrão (vazio = usa $SHELL)
    #[serde(default)]
    pub shell: String,

    /// Variáveis de ambiente adicionais
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,

    /// Diretório de trabalho inicial
    #[serde(default)]
    pub working_directory: String,

    /// Limite de scrollback em linhas
    #[serde(default = "default_scrollback_limit")]
    pub scrollback_limit: usize,

    /// Cursor blink
    #[serde(default = "default_true")]
    pub cursor_blink: bool,

    /// Estilo do cursor (block, line, underline)
    #[serde(default = "default_cursor_style")]
    pub cursor_style: String,

    /// Copiar ao selecionar: soltar o mouse depois de arrastar, ou dar duplo
    /// ou triplo clique, copia a seleção e mostra o aviso "Copiado".
    #[serde(default = "default_true")]
    pub copy_on_select: bool,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: String::new(),
            env: std::collections::HashMap::new(),
            working_directory: String::new(),
            scrollback_limit: default_scrollback_limit(),
            cursor_blink: true,
            cursor_style: default_cursor_style(),
            copy_on_select: true,
        }
    }
}

fn default_scrollback_limit() -> usize {
    10000
}

fn default_cursor_style() -> String {
    "block".to_string()
}

/// Configurações de janela
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    /// Título da janela
    #[serde(default = "default_window_title")]
    pub title: String,

    /// Largura inicial
    #[serde(default = "default_window_width")]
    pub width: u32,

    /// Altura inicial
    #[serde(default = "default_window_height")]
    pub height: u32,

    /// Opacidade do background (0.0 - 1.0)
    #[serde(default = "default_opacity")]
    pub opacity: f32,

    /// Decorations da janela (bordas, título)
    #[serde(default = "default_true")]
    pub decorations: bool,

    /// Sempre no topo
    #[serde(default = "default_false")]
    pub always_on_top: bool,

    /// Confirmar antes de fechar tab com processo rodando
    #[serde(default = "default_true")]
    pub confirm_close_tab: bool,

    /// Animações (cores do anel de aviso e das tabs). `false` deixa tudo
    /// parado, para quem prefere menos movimento.
    #[serde(default = "default_true")]
    pub animations: bool,

    /// Estilo da animação da tab com aviso (ring, pulse, wave, spinner,
    /// dot, none).
    #[serde(default)]
    pub tab_animation: crate::ui::tab_animation::TabAnimation,

    /// Velocidade da animação (slow, normal, fast).
    #[serde(default)]
    pub animation_speed: crate::ui::tab_animation::AnimationSpeed,

    /// Parar as animações quando o sistema pede menos movimento.
    #[serde(default = "default_true")]
    pub follow_reduced_motion: bool,

    /// Trocar o indicador que o programa põe no título (◐, ✳, ⠋…) pelo do
    /// terminal.
    #[serde(default = "default_false")]
    pub replace_program_spinner: bool,

    /// Abrir a janela maximizada (no macOS: "zoom", preenchendo a tela, não
    /// tela cheia em outro Space). `false` usa `width` x `height`.
    #[serde(default = "default_true")]
    pub start_maximized: bool,

    /// Quadros que a janela pode ter na fila da GPU (1 a 3). Menos gasta
    /// menos memória (no macOS, cada quadro tem o tamanho da janela), mais
    /// segura quadros quando o desenho atrasa.
    #[serde(default = "default_frame_latency")]
    pub frame_latency: u32,
}

fn default_frame_latency() -> u32 {
    2
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: default_window_title(),
            width: default_window_width(),
            height: default_window_height(),
            opacity: default_opacity(),
            decorations: true,
            always_on_top: false,
            confirm_close_tab: true,
            animations: true,
            tab_animation: Default::default(),
            animation_speed: Default::default(),
            follow_reduced_motion: true,
            replace_program_spinner: false,
            start_maximized: true,
            frame_latency: default_frame_latency(),
        }
    }
}

fn default_window_title() -> String {
    "Code Terminal".to_string()
}

fn default_window_width() -> u32 {
    800
}

fn default_window_height() -> u32 {
    600
}

fn default_opacity() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_false() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_release_gpu_after_formats() {
        use std::time::Duration;
        assert_eq!(
            parse_release_after("30s"),
            Some(Some(Duration::from_secs(30)))
        );
        assert_eq!(
            parse_release_after(" 5m "),
            Some(Some(Duration::from_secs(300)))
        );
        assert_eq!(
            parse_release_after("1h"),
            Some(Some(Duration::from_secs(3600)))
        );
        assert_eq!(
            parse_release_after("90"),
            Some(Some(Duration::from_secs(90)))
        );
        assert_eq!(parse_release_after("0"), Some(None));
        assert_eq!(parse_release_after("0s"), Some(None));
        assert_eq!(parse_release_after("never"), Some(None));
        assert_eq!(parse_release_after("5x"), None);
        assert_eq!(parse_release_after(""), None);
    }

    #[test]
    fn test_server_settings_default_and_toml() {
        use std::time::Duration;
        let config = Config::default();
        assert_eq!(config.server.release_gpu_after, "5m");
        assert_eq!(
            config.server.release_gpu_after(),
            Some(Duration::from_secs(300))
        );
        let parsed: Config = toml::from_str("[server]\nrelease_gpu_after = \"0\"\n").unwrap();
        assert_eq!(parsed.server.release_gpu_after(), None);
        let invalid = ServerSettings {
            release_gpu_after: "depois".into(),
        };
        assert_eq!(invalid.release_gpu_after(), Some(Duration::from_secs(300)));
        let empty: Config = toml::from_str("").unwrap();
        assert_eq!(empty.server, ServerSettings::default());
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.font.size, 12.0);
        assert_eq!(config.colors.ansi.len(), 8);
        assert_eq!(config.terminal.scrollback_limit, 10000);
        assert!(!config.ai.enabled);
    }

    #[test]
    fn test_copy_on_select_defaults_on_and_can_be_turned_off() {
        assert!(Config::default().terminal.copy_on_select);
        let on: Config = toml::from_str("[terminal]\nshell = \"\"\n").unwrap();
        assert!(on.terminal.copy_on_select);
        let off: Config = toml::from_str("[terminal]\ncopy_on_select = false\n").unwrap();
        assert!(!off.terminal.copy_on_select);
    }

    #[test]
    fn test_font_config_default() {
        let font = FontConfig::default();
        assert_eq!(font.size, 12.0);
        assert_eq!(font.line_height, 1.2);
        assert!(font.bold);
    }

    #[test]
    fn test_color_scheme_default() {
        let colors = ColorScheme::default();
        assert_eq!(colors.ansi.len(), 8);
        assert_eq!(colors.ansi_bright.len(), 8);
        assert!(colors.background.starts_with('#'));
    }
}
