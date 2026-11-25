//! Clipboard integration using arboard
//!
//! Provides cross-platform clipboard functionality for copy/paste operations.

use arboard::Clipboard;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tracing::{trace, warn};

/// Errors that can occur during clipboard operations
#[derive(Error, Debug)]
pub enum ClipboardError {
    #[error("Clipboard access failed: {0}")]
    AccessFailed(String),
    #[error("No content in clipboard")]
    EmptyClipboard,
    #[error("Content type not supported")]
    UnsupportedType,
}

/// Result type for clipboard operations
pub type Result<T> = std::result::Result<T, ClipboardError>;

/// Manages clipboard operations
///
/// Uses arboard for cross-platform clipboard access.
/// Operations are thread-safe through internal locking.
pub struct ClipboardManager {
    clipboard: Arc<Mutex<Clipboard>>,
}

impl std::fmt::Debug for ClipboardManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClipboardManager")
            .field("clipboard", &"<Clipboard>")
            .finish()
    }
}

impl ClipboardManager {
    /// Creates a new clipboard manager
    ///
    /// Returns None if clipboard access is not available
    pub fn new() -> Option<Self> {
        match Clipboard::new() {
            Ok(clipboard) => {
                trace!("Clipboard initialized successfully");
                Some(Self {
                    clipboard: Arc::new(Mutex::new(clipboard)),
                })
            }
            Err(e) => {
                warn!("Failed to initialize clipboard: {}", e);
                None
            }
        }
    }

    /// Copies text to the clipboard
    ///
    /// # Arguments
    /// * `text` - The text to copy
    ///
    /// # Returns
    /// Ok(()) on success, Err on failure
    pub fn copy_text(&self, text: impl Into<String>) -> Result<()> {
        let text = text.into();
        let mut clipboard = self
            .clipboard
            .lock()
            .map_err(|e| ClipboardError::AccessFailed(e.to_string()))?;

        clipboard
            .set_text(&text)
            .map_err(|e| ClipboardError::AccessFailed(e.to_string()))?;

        trace!("Copied {} characters to clipboard", text.len());
        Ok(())
    }

    /// Gets text from the clipboard
    ///
    /// # Returns
    /// The clipboard text, or error if empty/unavailable
    pub fn get_text(&self) -> Result<String> {
        let mut clipboard = self
            .clipboard
            .lock()
            .map_err(|e| ClipboardError::AccessFailed(e.to_string()))?;

        let text = clipboard.get_text().map_err(read_error)?;

        if text.is_empty() {
            return Err(ClipboardError::EmptyClipboard);
        }

        trace!("Retrieved {} characters from clipboard", text.len());
        Ok(text)
    }

    /// Checks if the clipboard has text content
    pub fn has_text(&self) -> bool {
        self.get_text().is_ok()
    }

    /// Clears the clipboard
    pub fn clear(&self) -> Result<()> {
        let mut clipboard = self
            .clipboard
            .lock()
            .map_err(|e| ClipboardError::AccessFailed(e.to_string()))?;

        clipboard
            .clear()
            .map_err(|e| ClipboardError::AccessFailed(e.to_string()))?;

        trace!("Clipboard cleared");
        Ok(())
    }

    /// Copia uma seleção do terminal: vai para a área de transferência e, no
    /// Linux, também para a seleção PRIMARY (a que o clique do meio cola).
    /// Falha na PRIMARY não desfaz a cópia principal: só fica no log.
    pub fn copy_selection_text(&self, text: &str) -> Result<()> {
        self.copy_text(text)?;
        #[cfg(target_os = "linux")]
        if let Err(e) = self.set_primary_text(text) {
            trace!("PRIMARY selection not updated: {}", e);
        }
        Ok(())
    }

    /// Grava na seleção PRIMARY (Linux).
    #[cfg(target_os = "linux")]
    pub fn set_primary_text(&self, text: &str) -> Result<()> {
        use arboard::{LinuxClipboardKind, SetExtLinux};
        let mut clipboard = self
            .clipboard
            .lock()
            .map_err(|e| ClipboardError::AccessFailed(e.to_string()))?;
        clipboard
            .set()
            .clipboard(LinuxClipboardKind::Primary)
            .text(text.to_owned())
            .map_err(|e| ClipboardError::AccessFailed(e.to_string()))
    }

    /// Lê a seleção PRIMARY (Linux): o texto selecionado por último em
    /// qualquer app, colado com o clique do meio.
    #[cfg(target_os = "linux")]
    pub fn get_primary_text(&self) -> Result<String> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        let mut clipboard = self
            .clipboard
            .lock()
            .map_err(|e| ClipboardError::AccessFailed(e.to_string()))?;
        let text = clipboard
            .get()
            .clipboard(LinuxClipboardKind::Primary)
            .text()
            .map_err(read_error)?;
        if text.is_empty() {
            return Err(ClipboardError::EmptyClipboard);
        }
        Ok(text)
    }
}

/// Erro de leitura: `ContentNotAvailable` quer dizer que não há texto (vazia
/// ou só uma imagem), não que o acesso falhou.
fn read_error(error: arboard::Error) -> ClipboardError {
    match error {
        arboard::Error::ContentNotAvailable => ClipboardError::EmptyClipboard,
        other => ClipboardError::AccessFailed(other.to_string()),
    }
}

impl Clone for ClipboardManager {
    fn clone(&self) -> Self {
        Self {
            clipboard: Arc::clone(&self.clipboard),
        }
    }
}

/// Global clipboard instance
///
/// This is lazily initialized on first access
pub fn get_clipboard() -> Option<ClipboardManager> {
    ClipboardManager::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_creation() {
        let clipboard = ClipboardManager::new();
        // Clipboard may not be available in test environments
        // Just verify it doesn't panic
        if let Some(cb) = clipboard {
            let _ = cb.has_text();
        }
    }

    // Note: Tests that manipulate the actual system clipboard are ignored by default
    // because they can cause heap corruption when run in parallel with other tests.
    // Run with: cargo test -- --ignored to execute these tests.

    #[test]
    fn test_read_error_without_text_is_empty_clipboard() {
        assert!(matches!(
            read_error(arboard::Error::ContentNotAvailable),
            ClipboardError::EmptyClipboard
        ));
        assert!(matches!(
            read_error(arboard::Error::ClipboardOccupied),
            ClipboardError::AccessFailed(_)
        ));
    }

    #[test]
    #[ignore = "Reads the system clipboard - run manually with: cargo test -- --ignored"]
    fn test_read_clipboard_without_format_error() {
        // Só lê: no Wayland, sem `wayland-data-control`, a leitura falhava com
        // "contents were not available in the requested format".
        let Some(clipboard) = ClipboardManager::new() else {
            eprintln!("Clipboard not available, skipping test");
            return;
        };
        match clipboard.get_text() {
            Ok(text) => eprintln!("clipboard has {} chars", text.chars().count()),
            Err(ClipboardError::EmptyClipboard) => eprintln!("clipboard empty"),
            Err(e) => panic!("clipboard read failed: {e}"),
        }
    }

    #[test]
    #[ignore = "Modifies system clipboard - run manually with: cargo test -- --ignored"]
    fn test_copy_and_get_text() {
        let Some(clipboard) = ClipboardManager::new() else {
            eprintln!("Clipboard not available, skipping test");
            return;
        };

        let test_text = "Hello, Terminal! 🦀";

        // Copy text
        clipboard.copy_text(test_text).unwrap();

        // Read it back
        let retrieved = clipboard.get_text().unwrap();
        assert_eq!(retrieved, test_text);
    }

    #[test]
    #[ignore = "Modifies system clipboard - run manually with: cargo test -- --ignored"]
    fn test_empty_clipboard() {
        let Some(clipboard) = ClipboardManager::new() else {
            eprintln!("Clipboard not available, skipping test");
            return;
        };

        // Clear clipboard
        clipboard.clear().unwrap();

        // Should fail to get text
        let result = clipboard.get_text();
        assert!(matches!(
            result,
            Err(ClipboardError::EmptyClipboard) | Err(ClipboardError::AccessFailed(_))
        ));
    }

    #[test]
    #[ignore = "Modifies system clipboard - run manually with: cargo test -- --ignored"]
    fn test_copy_multiline() {
        let Some(clipboard) = ClipboardManager::new() else {
            eprintln!("Clipboard not available, skipping test");
            return;
        };

        let test_text = "Line 1\nLine 2\nLine 3\n";

        clipboard.copy_text(test_text).unwrap();
        let retrieved = clipboard.get_text().unwrap();
        assert_eq!(retrieved, test_text);
    }
}
