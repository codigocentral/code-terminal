use serde::{Deserialize, Serialize};

/// Posição da sidebar
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum SidebarPosition {
    /// Sidebar à esquerda (padrão)
    #[default]
    Left,
    /// Sidebar à direita
    Right,
}

/// Configuração de abas verticais
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerticalTabsConfig {
    /// Altura de cada item de tab
    pub item_height: u32,
    /// Mostrar ícone de status
    pub show_status_icon: bool,
    /// Mostrar diretório de trabalho
    pub show_working_dir: bool,
    /// Mostrar branch git
    pub show_git_branch: bool,
    /// Mostrar indicador de porta
    pub show_port_indicator: bool,
    /// Truncar títulos longos
    pub truncate_titles: bool,
    /// Comprimento máximo do título
    pub max_title_length: usize,
}

impl Default for VerticalTabsConfig {
    fn default() -> Self {
        Self {
            item_height: 60,
            show_status_icon: true,
            show_working_dir: true,
            show_git_branch: true,
            show_port_indicator: true,
            truncate_titles: true,
            max_title_length: 25,
        }
    }
}

/// Configuração de detecção de Git
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitDetectionConfig {
    /// Habilitar detecção de branch
    pub enabled: bool,
    /// Mostrar indicador de repositório sujo
    pub show_dirty_indicator: bool,
    /// Mostrar commits ahead/behind
    pub show_ahead_behind: bool,
    /// Cache duration em segundos
    pub cache_duration_secs: u64,
}

impl Default for GitDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_dirty_indicator: true,
            show_ahead_behind: true,
            cache_duration_secs: 5,
        }
    }
}

/// Configuração de display de diretório
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryDisplayConfig {
    /// Habilitar display de diretório
    pub enabled: bool,
    /// Mostrar caminho completo ou apenas nome
    pub show_full_path: bool,
    /// Número máximo de componentes do path
    pub max_path_components: usize,
    /// Substituir home por ~
    pub substitute_home: bool,
}

impl Default for DirectoryDisplayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_full_path: false,
            max_path_components: 3,
            substitute_home: true,
        }
    }
}

/// Configuração de detecção de portas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortDetectionConfig {
    /// Habilitar detecção de portas
    pub enabled: bool,
    /// Portas comuns a monitorar
    pub common_ports: Vec<u16>,
    /// Scan range de portas
    pub scan_range: (u16, u16),
    /// Intervalo de atualização em segundos
    pub update_interval_secs: u64,
    /// Cache duration em segundos
    pub cache_duration_secs: u64,
}

impl Default for PortDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            common_ports: vec![3000, 8080, 8000, 4200, 5000, 9000],
            scan_range: (3000, 9000),
            update_interval_secs: 10,
            cache_duration_secs: 30,
        }
    }
}

/// Configuração completa da sidebar
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarConfig {
    /// Sidebar visível por padrão
    #[serde(default)]
    pub visible: bool,
    /// Posição da sidebar
    #[serde(default)]
    pub position: SidebarPosition,
    /// Largura da sidebar em pixels
    #[serde(default = "default_width")]
    pub width: u32,
    /// Largura em modo compacto
    #[serde(default = "default_compact_width")]
    pub compact_width: u32,
    /// Largura mínima
    #[serde(default = "default_min_width")]
    pub min_width: u32,
    /// Largura máxima
    #[serde(default = "default_max_width")]
    pub max_width: u32,
    /// Configuração de abas verticais
    #[serde(default)]
    pub vertical_tabs: VerticalTabsConfig,
    /// Configuração de detecção Git
    #[serde(default)]
    pub git_detection: GitDetectionConfig,
    /// Configuração de display de diretório
    #[serde(default)]
    pub directory_display: DirectoryDisplayConfig,
    /// Configuração de detecção de portas
    #[serde(default)]
    pub port_detection: PortDetectionConfig,
    /// Cor de fundo (hex)
    #[serde(default = "default_background")]
    pub background_color: String,
    /// Cor do texto
    #[serde(default = "default_foreground")]
    pub foreground_color: String,
    /// Cor de destaque
    #[serde(default = "default_accent")]
    pub accent_color: String,
    /// Cor do item selecionado
    #[serde(default = "default_selected")]
    pub selected_color: String,
    /// Cor do item ativo
    #[serde(default = "default_active")]
    pub active_color: String,
    /// Font size
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    /// Espaçamento entre itens
    #[serde(default)]
    pub item_spacing: u32,
    /// Padding interno
    #[serde(default)]
    pub padding: u32,
}

fn default_width() -> u32 {
    250
}

fn default_compact_width() -> u32 {
    50
}

fn default_min_width() -> u32 {
    150
}

fn default_max_width() -> u32 {
    500
}

fn default_background() -> String {
    "#1a1a2e".to_string()
}

fn default_foreground() -> String {
    "#eeeeee".to_string()
}

fn default_accent() -> String {
    "#3498db".to_string()
}

fn default_selected() -> String {
    "#2d3a4f".to_string()
}

fn default_active() -> String {
    "#4a90d9".to_string()
}

fn default_font_size() -> f32 {
    14.0
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            visible: true,
            position: SidebarPosition::Left,
            width: 250,
            compact_width: 50,
            min_width: 150,
            max_width: 500,
            vertical_tabs: VerticalTabsConfig::default(),
            git_detection: GitDetectionConfig::default(),
            directory_display: DirectoryDisplayConfig::default(),
            port_detection: PortDetectionConfig::default(),
            background_color: "#1a1a2e".to_string(),
            foreground_color: "#eeeeee".to_string(),
            accent_color: "#3498db".to_string(),
            selected_color: "#2d3a4f".to_string(),
            active_color: "#4a90d9".to_string(),
            font_size: 14.0,
            item_spacing: 4,
            padding: 8,
        }
    }
}

impl SidebarConfig {
    /// Cria uma configuração com tema escuro
    pub fn dark_theme() -> Self {
        Self::default()
    }

    /// Cria uma configuração com tema claro
    pub fn light_theme() -> Self {
        Self {
            background_color: "#f5f5f5".to_string(),
            foreground_color: "#333333".to_string(),
            accent_color: "#1976d2".to_string(),
            selected_color: "#e3e3e3".to_string(),
            active_color: "#2196f3".to_string(),
            ..Default::default()
        }
    }

    /// Cria uma configuração compacta
    pub fn compact() -> Self {
        Self {
            width: 180,
            compact_width: 40,
            vertical_tabs: VerticalTabsConfig {
                item_height: 45,
                show_working_dir: false,
                show_git_branch: false,
                truncate_titles: true,
                max_title_length: 15,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Valida a configuração
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.width < self.min_width {
            return Err(ConfigError::WidthTooSmall);
        }
        if self.width > self.max_width {
            return Err(ConfigError::WidthTooLarge);
        }
        if self.compact_width >= self.width {
            return Err(ConfigError::CompactWidthInvalid);
        }
        Ok(())
    }

    /// Ajusta a largura dentro dos limites
    pub fn clamp_width(&self, width: u32) -> u32 {
        width.clamp(self.min_width, self.max_width)
    }
}

/// Erros de configuração
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigError {
    WidthTooSmall,
    WidthTooLarge,
    CompactWidthInvalid,
    InvalidColor,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::WidthTooSmall => write!(f, "Sidebar width is too small"),
            ConfigError::WidthTooLarge => write!(f, "Sidebar width is too large"),
            ConfigError::CompactWidthInvalid => {
                write!(f, "Compact width must be smaller than regular width")
            }
            ConfigError::InvalidColor => write!(f, "Invalid color format"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SidebarConfig::default();
        assert!(config.visible);
        assert_eq!(config.width, 250);
        assert_eq!(config.position, SidebarPosition::Left);
    }

    #[test]
    fn test_light_theme() {
        let config = SidebarConfig::light_theme();
        assert_eq!(config.background_color, "#f5f5f5");
        assert_eq!(config.foreground_color, "#333333");
    }

    #[test]
    fn test_compact_config() {
        let config = SidebarConfig::compact();
        assert_eq!(config.width, 180);
        assert_eq!(config.compact_width, 40);
    }

    #[test]
    fn test_config_validation() {
        let mut config = SidebarConfig::default();
        config.width = 100; // Below min_width
        assert!(matches!(config.validate(), Err(ConfigError::WidthTooSmall)));

        config.width = 600; // Above max_width
        assert!(matches!(config.validate(), Err(ConfigError::WidthTooLarge)));

        config.width = 250;
        config.compact_width = 300; // Invalid
        assert!(matches!(
            config.validate(),
            Err(ConfigError::CompactWidthInvalid)
        ));
    }

    #[test]
    fn test_clamp_width() {
        let config = SidebarConfig::default();
        assert_eq!(config.clamp_width(100), 150); // Min
        assert_eq!(config.clamp_width(300), 300); // OK
        assert_eq!(config.clamp_width(600), 500); // Max
    }
}
