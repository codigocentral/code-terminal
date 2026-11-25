//! Browser Configuration - Configuração do navegador

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Configuração do browser
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    /// Página inicial
    pub homepage: HomepageConfig,
    /// Engine do WebView
    pub engine: WebViewEngine,
    /// Preferências
    pub preferences: BrowserPreferences,
    /// Zoom padrão (1.0 = 100%)
    pub default_zoom: f64,
    /// User agent customizado
    pub user_agent: Option<String>,
    /// Habilitar JavaScript
    pub javascript_enabled: bool,
    /// Habilitar cookies
    pub cookies_enabled: bool,
    /// Habilitar cache
    pub cache_enabled: bool,
    /// Diretório de cache
    pub cache_dir: Option<PathBuf>,
    /// Política de CORS
    pub cors_policy: CorsPolicy,
    /// Headers customizados
    pub custom_headers: HashMap<String, String>,
    /// Timeout de carregamento (segundos)
    pub load_timeout: u64,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            homepage: HomepageConfig::default(),
            engine: WebViewEngine::default(),
            preferences: BrowserPreferences::default(),
            default_zoom: 1.0,
            user_agent: None,
            javascript_enabled: true,
            cookies_enabled: true,
            cache_enabled: true,
            cache_dir: None,
            cors_policy: CorsPolicy::Default,
            custom_headers: HashMap::new(),
            load_timeout: 30,
        }
    }
}

impl BrowserConfig {
    /// Cria configuração com página inicial específica
    pub fn with_homepage(url: impl Into<String>) -> Self {
        Self {
            homepage: HomepageConfig::Url(url.into()),
            ..Default::default()
        }
    }

    /// Define a engine
    pub fn with_engine(mut self, engine: WebViewEngine) -> Self {
        self.engine = engine;
        self
    }

    /// Define o zoom
    pub fn with_zoom(mut self, zoom: f64) -> Self {
        self.default_zoom = zoom.clamp(0.25, 5.0);
        self
    }

    /// Define o user agent
    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Desabilita JavaScript
    pub fn without_javascript(mut self) -> Self {
        self.javascript_enabled = false;
        self
    }

    /// Desabilita cookies
    pub fn without_cookies(mut self) -> Self {
        self.cookies_enabled = false;
        self
    }

    /// Define timeout
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.load_timeout = seconds;
        self
    }

    /// Adiciona header customizado
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.custom_headers.insert(key.into(), value.into());
        self
    }
}

/// Configuração da página inicial
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum HomepageConfig {
    /// URL específica
    Url(String),
    /// Página em branco
    Blank,
    /// Página inicial customizada
    Custom { html: String, title: String },
}

impl Default for HomepageConfig {
    fn default() -> Self {
        HomepageConfig::Url("https://www.google.com".to_string())
    }
}

/// Engine do WebView
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WebViewEngine {
    /// WebKit (macOS/Linux)
    WebKit,
    /// WebView2 (Windows)
    WebView2,
    /// Edge WebView2 (Windows)
    EdgeWebView2,
    /// Auto-seleção baseada na plataforma
    #[default]
    Auto,
}

impl WebViewEngine {
    /// Retorna a engine apropriada para a plataforma
    pub fn for_current_platform() -> Self {
        #[cfg(target_os = "windows")]
        return WebViewEngine::WebView2;

        #[cfg(target_os = "macos")]
        return WebViewEngine::WebKit;

        #[cfg(target_os = "linux")]
        return WebViewEngine::WebKit;

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        return WebViewEngine::Auto;
    }
}

/// Preferências do browser
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserPreferences {
    /// Modo escuro
    pub dark_mode: bool,
    /// Tamanho da fonte
    pub font_size: u8,
    /// Família de fonte
    pub font_family: String,
    /// Largura mínima da viewport
    pub min_viewport_width: u32,
    /// Altura mínima da viewport
    pub min_viewport_height: u32,
    /// Habilitar devtools
    pub devtools_enabled: bool,
    /// Bloquear popups
    pub block_popups: bool,
    /// Bloquear ads
    pub block_ads: bool,
    /// Modo leitura
    pub reader_mode: bool,
}

impl Default for BrowserPreferences {
    fn default() -> Self {
        Self {
            dark_mode: false,
            font_size: 16,
            font_family: "system-ui, -apple-system, sans-serif".to_string(),
            min_viewport_width: 320,
            min_viewport_height: 240,
            devtools_enabled: cfg!(debug_assertions),
            block_popups: true,
            block_ads: false,
            reader_mode: false,
        }
    }
}

impl BrowserPreferences {
    /// Define modo escuro
    pub fn with_dark_mode(mut self, enabled: bool) -> Self {
        self.dark_mode = enabled;
        self
    }

    /// Define tamanho da fonte
    pub fn with_font_size(mut self, size: u8) -> Self {
        self.font_size = size.clamp(8, 32);
        self
    }

    /// Habilita devtools
    pub fn with_devtools(mut self) -> Self {
        self.devtools_enabled = true;
        self
    }

    /// Bloqueia ads
    pub fn with_ad_blocker(mut self) -> Self {
        self.block_ads = true;
        self
    }
}

/// Política de CORS
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CorsPolicy {
    /// Política padrão do navegador
    #[default]
    Default,
    /// Permissive (permite todas as origens)
    Permissive,
    /// Restritiva
    Strict,
    /// Customizada
    Custom { allow_origins: Vec<String> },
}

/// Configuração de proxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Host do proxy
    pub host: String,
    /// Porta do proxy
    pub port: u16,
    /// Usuário
    pub username: Option<String>,
    /// Senha
    pub password: Option<String>,
    /// URLs para ignorar
    pub bypass_list: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BrowserConfig::default();
        assert!(config.javascript_enabled);
        assert!(config.cookies_enabled);
        assert!(config.cache_enabled);
        assert_eq!(config.default_zoom, 1.0);
        assert_eq!(config.load_timeout, 30);
    }

    #[test]
    fn test_config_builder() {
        let config = BrowserConfig::default()
            .with_zoom(1.5)
            .with_user_agent("Custom/1.0")
            .without_javascript()
            .with_timeout(60);

        assert_eq!(config.default_zoom, 1.5);
        assert_eq!(config.user_agent, Some("Custom/1.0".to_string()));
        assert!(!config.javascript_enabled);
        assert_eq!(config.load_timeout, 60);
    }

    #[test]
    fn test_homepage_config() {
        let homepage = HomepageConfig::Url("https://example.com".to_string());
        match homepage {
            HomepageConfig::Url(url) => assert_eq!(url, "https://example.com"),
            _ => panic!("Expected URL variant"),
        }
    }

    #[test]
    fn test_preferences() {
        let prefs = BrowserPreferences::default()
            .with_dark_mode(true)
            .with_font_size(20)
            .with_ad_blocker();

        assert!(prefs.dark_mode);
        assert_eq!(prefs.font_size, 20);
        assert!(prefs.block_ads);
    }

    #[test]
    fn test_zoom_clamping() {
        let config = BrowserConfig::default().with_zoom(10.0); // Deve ser clamped para 5.0

        assert_eq!(config.default_zoom, 5.0);

        let config = BrowserConfig::default().with_zoom(0.1); // Deve ser clamped para 0.25

        assert_eq!(config.default_zoom, 0.25);
    }

    #[test]
    fn test_builders_and_clamping() {
        let config = BrowserConfig::with_homepage("https://home.test")
            .with_engine(WebViewEngine::WebView2)
            .with_zoom(10.0)
            .without_cookies()
            .with_header("X-Test", "1");
        assert!(matches!(config.homepage, HomepageConfig::Url(ref u) if u == "https://home.test"));
        assert_eq!(config.engine, WebViewEngine::WebView2);
        assert_eq!(config.default_zoom, 5.0);
        assert_eq!(BrowserConfig::default().with_zoom(0.0).default_zoom, 0.25);
        assert!(!config.cookies_enabled);
        assert_eq!(
            config.custom_headers.get("X-Test").map(String::as_str),
            Some("1")
        );

        let prefs = BrowserPreferences::default()
            .with_font_size(100)
            .with_devtools()
            .with_ad_blocker();
        assert_eq!(prefs.font_size, 32);
        assert!(prefs.devtools_enabled && prefs.block_ads);
        assert_eq!(BrowserPreferences::default().with_font_size(1).font_size, 8);

        #[cfg(target_os = "linux")]
        assert_eq!(WebViewEngine::for_current_platform(), WebViewEngine::WebKit);
        assert_eq!(WebViewEngine::default(), WebViewEngine::Auto);
        assert_eq!(CorsPolicy::default(), CorsPolicy::Default);
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut config = BrowserConfig::default();
        config.homepage = HomepageConfig::Custom {
            html: "<h1>oi</h1>".into(),
            title: "Início".into(),
        };
        config.cors_policy = CorsPolicy::Custom {
            allow_origins: vec!["https://a.test".into()],
        };
        let json = serde_json::to_string(&config).unwrap();
        let back: BrowserConfig = serde_json::from_str(&json).unwrap();
        assert!(
            matches!(back.homepage, HomepageConfig::Custom { ref title, .. } if title == "Início")
        );
        assert_eq!(back.cors_policy, config.cors_policy);

        let proxy = ProxyConfig {
            host: "proxy".into(),
            port: 3128,
            username: None,
            password: None,
            bypass_list: vec!["localhost".into()],
        };
        let json = serde_json::to_value(&proxy).unwrap();
        assert_eq!(json["port"], 3128);
    }
}
