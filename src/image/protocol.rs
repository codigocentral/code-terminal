//! Kitty Graphics Protocol Implementation
//!
//! Implements the Kitty graphics protocol for inline image display.
//! Reference: https://sw.kovidgoyal.net/kitty/graphics-protocol/
//!
//! # Protocol Format
//!
//! The basic format is:
//! ```text
//! <ESC>_G<key>=<value>,<key>=<value>...<payload><ESC>\
//! ```
//!
//! Where:
//! - `<ESC>_G` (0x1b 0x5f 0x47) starts the graphics command
//! - Key-value pairs specify options
//! - Payload contains image data (optional)
//! - `<ESC>\` (0x1b 0x5c) ends the command
//!
//! # Key Options
//!
//! - `f`: Format (24=png, 32=rgba, etc.)
//! - `s`: Source width in pixels
//! - `v`: Source height in pixels
//! - `i`: Image ID (for referencing)
//! - `p`: Placement ID
//! - `t`: Transmission mode (d=direct, f=file, t=temp file, s=stream)
//! - `o`: More data follows (1=yes, 0=no)
//! - `m`: Cursor movement (0=none, 1=after)
//! - `a`: Action (t=transmit, d=display, T=transmit+display, d=delete)

use super::{ImageError, ImageFormat, ImageId, ImageResult};

/// Graphics command types
#[derive(Debug, Clone, PartialEq)]
pub enum GraphicsCommand {
    /// Transmit image data
    Transmit {
        data: Vec<u8>,
        format: ImageFormat,
        width: u32,
        height: u32,
    },
    /// Display an image at position
    Display {
        image_id: ImageId,
        row: usize,
        col: usize,
        width: usize,
        height: usize,
    },
    /// Delete an image
    Delete { image_id: ImageId },
    /// Clear all images
    Clear,
}

/// Transmission mode for image data
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransmissionMode {
    /// Direct data in escape sequence
    Direct,
    /// Read from file
    File,
    /// Read from temporary file (delete after)
    TempFile,
    /// Stream data across multiple sequences
    Stream,
}

impl TransmissionMode {
    /// Parse transmission mode from character
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'd' => Some(Self::Direct),
            'f' => Some(Self::File),
            't' => Some(Self::TempFile),
            's' => Some(Self::Stream),
            _ => None,
        }
    }
}

/// Action to perform
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    /// Transmit image
    Transmit,
    /// Display image
    Display,
    /// Transmit and display
    TransmitAndDisplay,
    /// Delete image
    Delete,
    /// Clear all images
    Clear,
}

impl Action {
    /// Parse action from character
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            't' => Some(Self::Transmit),
            'd' => Some(Self::Display),
            'T' => Some(Self::TransmitAndDisplay),
            'D' => Some(Self::Delete),
            'C' => Some(Self::Clear),
            _ => None,
        }
    }
}

/// Parser for graphics protocol sequences
pub struct ImageProtocolHandler {
    /// Buffer for incomplete stream data
    stream_buffer: Vec<u8>,
    /// Current stream ID
    stream_id: Option<u32>,
}

impl ImageProtocolHandler {
    /// Create a new protocol handler
    pub fn new() -> Self {
        Self {
            stream_buffer: Vec::new(),
            stream_id: None,
        }
    }

    /// Parse a graphics command from escape sequence payload
    ///
    /// The payload starts after `<ESC>_G` and ends before `<ESC>\`
    pub fn parse(&mut self, payload: &[u8]) -> ImageResult<Option<GraphicsCommand>> {
        // Convert to string for parsing keys
        let payload_str = String::from_utf8_lossy(payload);

        // Split metadata from data
        let (metadata, data) = if let Some(pos) = payload_str.find(';') {
            let (meta, rest) = payload_str.split_at(pos);
            (meta, &rest[1..]) // Skip the semicolon
        } else {
            (&payload_str[..], "")
        };

        // Parse key-value pairs
        let mut options = std::collections::HashMap::new();
        for pair in metadata.split(',') {
            if let Some(pos) = pair.find('=') {
                let key = &pair[..pos];
                let value = &pair[pos + 1..];
                options.insert(key, value);
            }
        }

        // Get action
        let action = options
            .get("a")
            .and_then(|v| Action::from_char(v.chars().next().unwrap_or('t')))
            .unwrap_or(Action::Transmit);

        // Get transmission mode
        let mode = options
            .get("t")
            .and_then(|v| TransmissionMode::from_char(v.chars().next().unwrap_or('d')))
            .unwrap_or(TransmissionMode::Direct);

        // Check if more data follows
        let more_data = options.get("m").map(|v| *v == "1").unwrap_or(false);

        // Handle streaming data
        if mode == TransmissionMode::Stream {
            self.handle_stream_data(data, more_data)?;
            if more_data {
                return Ok(None); // Wait for more data
            }
        }

        // Get image ID
        let image_id = options
            .get("i")
            .and_then(|v| v.parse().ok())
            .map(ImageId)
            .unwrap_or(ImageId(0));

        // Process based on action
        let command = match action {
            Action::Transmit | Action::TransmitAndDisplay => {
                let format = self.parse_format(options.get("f"));
                let width = options.get("s").and_then(|v| v.parse().ok()).unwrap_or(0);
                let height = options.get("v").and_then(|v| v.parse().ok()).unwrap_or(0);

                let data = if mode == TransmissionMode::Stream {
                    std::mem::take(&mut self.stream_buffer)
                } else {
                    // Decode base64 data
                    base64_decode(data)?
                };

                Some(GraphicsCommand::Transmit {
                    data,
                    format,
                    width,
                    height,
                })
            }
            Action::Display => {
                let row = options.get("r").and_then(|v| v.parse().ok()).unwrap_or(0);
                let col = options.get("c").and_then(|v| v.parse().ok()).unwrap_or(0);
                let width = options.get("w").and_then(|v| v.parse().ok()).unwrap_or(0);
                let height = options.get("h").and_then(|v| v.parse().ok()).unwrap_or(0);

                Some(GraphicsCommand::Display {
                    image_id,
                    row,
                    col,
                    width,
                    height,
                })
            }
            Action::Delete => Some(GraphicsCommand::Delete { image_id }),
            Action::Clear => Some(GraphicsCommand::Clear),
        };

        Ok(command)
    }

    /// Handle streaming data chunks
    fn handle_stream_data(&mut self, data: &str, more: bool) -> ImageResult<()> {
        // Decode and append to buffer
        let decoded = base64_decode(data)?;
        self.stream_buffer.extend_from_slice(&decoded);

        if !more {
            // Stream complete
            self.stream_id = None;
        }

        Ok(())
    }

    /// Parse image format from code
    fn parse_format(&self, code: Option<&&str>) -> ImageFormat {
        let code_num = code.and_then(|v| v.parse().ok()).unwrap_or(24);
        match code_num {
            24 => ImageFormat::Png,
            32 => ImageFormat::Rgba,
            _ => ImageFormat::Png,
        }
    }

    /// Check if data is a graphics protocol sequence
    pub fn is_graphics_sequence(data: &[u8]) -> bool {
        // Check for ESC _ G (0x1b 0x5f 0x47)
        data.len() >= 3 && data[0] == 0x1b && data[1] == 0x5f && data[2] == 0x47
    }

    /// Extract payload from complete sequence
    ///
    /// Input: ESC _ G <payload> ESC \
    /// Returns: payload
    pub fn extract_payload(data: &[u8]) -> Option<&[u8]> {
        if !Self::is_graphics_sequence(data) {
            return None;
        }

        // Find end marker ESC \ (0x1b 0x5c)
        for i in 3..data.len() - 1 {
            if data[i] == 0x1b && data[i + 1] == 0x5c {
                return Some(&data[3..i]);
            }
        }

        None
    }
}

impl Default for ImageProtocolHandler {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode base64 data
fn base64_decode(data: &str) -> ImageResult<Vec<u8>> {
    // Simple base64 decoder
    use base64::{engine::general_purpose::STANDARD, Engine};

    STANDARD
        .decode(data)
        .map_err(|e| ImageError::Protocol(format!("Base64 decode error: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn test_transmission_mode_parsing() {
        assert_eq!(
            TransmissionMode::from_char('d'),
            Some(TransmissionMode::Direct)
        );
        assert_eq!(
            TransmissionMode::from_char('f'),
            Some(TransmissionMode::File)
        );
        assert_eq!(
            TransmissionMode::from_char('t'),
            Some(TransmissionMode::TempFile)
        );
        assert_eq!(
            TransmissionMode::from_char('s'),
            Some(TransmissionMode::Stream)
        );
        assert_eq!(TransmissionMode::from_char('x'), None);
    }

    #[test]
    fn test_action_parsing() {
        assert_eq!(Action::from_char('t'), Some(Action::Transmit));
        assert_eq!(Action::from_char('T'), Some(Action::TransmitAndDisplay));
        assert_eq!(Action::from_char('d'), Some(Action::Display));
        assert_eq!(Action::from_char('D'), Some(Action::Delete));
        assert_eq!(Action::from_char('C'), Some(Action::Clear));
    }

    #[test]
    fn test_is_graphics_sequence() {
        assert!(ImageProtocolHandler::is_graphics_sequence(b"\x1b_G"));
        assert!(!ImageProtocolHandler::is_graphics_sequence(b"hello"));
        assert!(!ImageProtocolHandler::is_graphics_sequence(b"\x1b["));
    }

    #[test]
    fn test_extract_payload() {
        // ESC _ G payload ESC \
        let data = b"\x1b_Gf=24,s=100,v=100;base64data\x1b\\";
        let payload = ImageProtocolHandler::extract_payload(data);

        assert!(payload.is_some());
        assert_eq!(payload.unwrap(), b"f=24,s=100,v=100;base64data");
    }

    #[test]
    fn test_parse_transmit_command() {
        let mut handler = ImageProtocolHandler::new();

        // Create a simple base64 encoded "test" string
        let base64_data = base64::engine::general_purpose::STANDARD.encode(b"test");
        let payload = format!("f=24,s=100,v=100,a=t;{}", base64_data);

        let result = handler.parse(payload.as_bytes());
        assert!(result.is_ok());

        let command = result.unwrap();
        assert!(command.is_some());

        match command.unwrap() {
            GraphicsCommand::Transmit { width, height, .. } => {
                assert_eq!(width, 100);
                assert_eq!(height, 100);
            }
            _ => panic!("Expected Transmit command"),
        }
    }

    #[test]
    fn test_parse_delete_command() {
        let mut handler = ImageProtocolHandler::new();
        let payload = b"a=D,i=123";

        let result = handler.parse(payload).unwrap();
        assert!(matches!(result, Some(GraphicsCommand::Delete { .. })));
    }

    #[test]
    fn test_parse_clear_command() {
        let mut handler = ImageProtocolHandler::new();
        let payload = b"a=C";

        let result = handler.parse(payload).unwrap();
        assert_eq!(result, Some(GraphicsCommand::Clear));
    }
}
