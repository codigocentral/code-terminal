//! Events - Eventos do browser

use serde::{Deserialize, Serialize};
use url::Url;

/// Trait para handlers de eventos do browser
pub trait BrowserEventHandler: Send + Sync {
    /// Chamado quando um evento do browser ocorre
    fn on_event(&mut self, event: super::BrowserEvent);
}

/// Tipo de evento de navegação
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationEventType {
    /// Navegação iniciada
    Started,
    /// Navegação concluída
    Completed,
    /// Navegação falhou
    Failed,
    /// Redirecionamento
    Redirected,
    /// Navegação cancelada
    Cancelled,
}

impl std::fmt::Display for NavigationEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NavigationEventType::Started => write!(f, "started"),
            NavigationEventType::Completed => write!(f, "completed"),
            NavigationEventType::Failed => write!(f, "failed"),
            NavigationEventType::Redirected => write!(f, "redirected"),
            NavigationEventType::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Evento de navegação
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationEvent {
    /// Tipo do evento
    pub event_type: NavigationEventType,
    /// URL atual
    pub url: Url,
    /// URL anterior (se houver)
    pub previous_url: Option<Url>,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl NavigationEvent {
    /// Cria um novo evento de navegação
    pub fn new(event_type: NavigationEventType, url: Url) -> Self {
        Self {
            event_type,
            url,
            previous_url: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Define a URL anterior
    pub fn with_previous(mut self, url: Url) -> Self {
        self.previous_url = Some(url);
        self
    }
}

/// Tipo de evento de carregamento
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadEventType {
    /// Carregamento iniciado
    Started,
    /// Progresso de carregamento
    Progress,
    /// Carregamento concluído
    Finished,
    /// Carregamento falhou
    Failed,
    /// Carregamento interrompido
    Stopped,
}

impl std::fmt::Display for LoadEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadEventType::Started => write!(f, "started"),
            LoadEventType::Progress => write!(f, "progress"),
            LoadEventType::Finished => write!(f, "finished"),
            LoadEventType::Failed => write!(f, "failed"),
            LoadEventType::Stopped => write!(f, "stopped"),
        }
    }
}

/// Evento de carregamento
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadEvent {
    /// Tipo do evento
    pub event_type: LoadEventType,
    /// URL
    pub url: Url,
    /// Progresso (0-100)
    pub progress: u8,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl LoadEvent {
    /// Cria um novo evento de carregamento
    pub fn new(event_type: LoadEventType, url: Url) -> Self {
        Self {
            event_type,
            url,
            progress: 0,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Define o progresso
    pub fn with_progress(mut self, progress: u8) -> Self {
        self.progress = progress;
        self
    }
}

/// Nível de log do console
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsoleLevel {
    /// Log
    Log,
    /// Debug
    Debug,
    /// Info
    Info,
    /// Warning
    Warn,
    /// Error
    Error,
}

impl std::fmt::Display for ConsoleLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsoleLevel::Log => write!(f, "log"),
            ConsoleLevel::Debug => write!(f, "debug"),
            ConsoleLevel::Info => write!(f, "info"),
            ConsoleLevel::Warn => write!(f, "warn"),
            ConsoleLevel::Error => write!(f, "error"),
        }
    }
}

/// Evento do console
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleEvent {
    /// Nível do log
    pub level: ConsoleLevel,
    /// Mensagem
    pub message: String,
    /// Fonte (arquivo e linha)
    pub source: Option<(String, u32)>,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ConsoleEvent {
    /// Cria um novo evento de console
    pub fn new(level: ConsoleLevel, message: impl Into<String>) -> Self {
        Self {
            level,
            message: message.into(),
            source: None,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Define a fonte
    pub fn with_source(mut self, file: impl Into<String>, line: u32) -> Self {
        self.source = Some((file.into(), line));
        self
    }
}

/// Evento do browser
#[derive(Debug, Clone)]
pub enum BrowserEvent {
    /// Evento de navegação
    Navigation(NavigationEvent),
    /// Evento de carregamento
    Load(LoadEvent),
    /// Evento de console
    Console(ConsoleEvent),
    /// Erro
    Error(String),
    /// Título mudou
    TitleChanged(String),
    /// Favicon carregado
    FaviconLoaded(Vec<u8>),
    /// Download iniciado
    DownloadStarted { url: String, filename: String },
    /// Download progresso
    DownloadProgress { id: u64, progress: f64 },
    /// Download concluído
    DownloadCompleted { id: u64, path: std::path::PathBuf },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation_event_type_display() {
        assert_eq!(NavigationEventType::Started.to_string(), "started");
        assert_eq!(NavigationEventType::Completed.to_string(), "completed");
        assert_eq!(NavigationEventType::Failed.to_string(), "failed");
    }

    #[test]
    fn test_load_event_type_display() {
        assert_eq!(LoadEventType::Started.to_string(), "started");
        assert_eq!(LoadEventType::Finished.to_string(), "finished");
        assert_eq!(LoadEventType::Failed.to_string(), "failed");
    }

    #[test]
    fn test_console_level_display() {
        assert_eq!(ConsoleLevel::Log.to_string(), "log");
        assert_eq!(ConsoleLevel::Info.to_string(), "info");
        assert_eq!(ConsoleLevel::Error.to_string(), "error");
    }

    #[test]
    fn test_navigation_event_creation() {
        let url = Url::parse("https://example.com").unwrap();
        let event = NavigationEvent::new(NavigationEventType::Started, url.clone());

        assert!(matches!(event.event_type, NavigationEventType::Started));
        assert_eq!(event.url, url);
        assert!(event.previous_url.is_none());
    }

    #[test]
    fn test_navigation_event_with_previous() {
        let url1 = Url::parse("https://example.com/page1").unwrap();
        let url2 = Url::parse("https://example.com/page2").unwrap();
        let event = NavigationEvent::new(NavigationEventType::Started, url2.clone())
            .with_previous(url1.clone());

        assert_eq!(event.previous_url, Some(url1));
    }

    #[test]
    fn test_load_event_creation() {
        let url = Url::parse("https://example.com").unwrap();
        let event = LoadEvent::new(LoadEventType::Progress, url.clone()).with_progress(50);

        assert_eq!(event.progress, 50);
        assert_eq!(event.url, url);
    }

    #[test]
    fn test_console_event_creation() {
        let event = ConsoleEvent::new(ConsoleLevel::Info, "Test message");

        assert!(matches!(event.level, ConsoleLevel::Info));
        assert_eq!(event.message, "Test message");
        assert!(event.source.is_none());
    }

    #[test]
    fn test_console_event_with_source() {
        let event = ConsoleEvent::new(ConsoleLevel::Error, "Error!").with_source("script.js", 42);

        assert_eq!(event.source, Some(("script.js".to_string(), 42)));
    }
}
