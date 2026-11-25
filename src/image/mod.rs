//! Image Protocol Module
//!
//! Implementa o Kitty Graphics Protocol para renderização de imagens inline.
//! Suporta formatos PNG, JPEG, GIF e outros via crate `image`.
//!
//! # Kitty Graphics Protocol
//!
//! O protocolo usa escape sequences OSC 1337 para transmitir imagens:
//! ```text
//! ESC _ G <payload> ESC \
//! ```
//!
//! Ou via APC sequences para transmissão chunked:
//! ```text
//! ESC _ G f=<format>,s=<width>,v=<height>,<data> ESC \
//! ```
//!
//! # Arquitetura
//!
//! ```text
//! Terminal Grid
//!     |
//!     v
//! ImageProtocolHandler
//!     |
//!     ├── ImageDecoder (PNG, JPEG, etc.)
//!     |
//!     ├── ImageCache (GPU textures)
//!     |
//!     └── ImageRenderer
//!             |
//!             v
//!         wgpu Renderer
//! ```

use std::time::{Duration, Instant};

pub mod cache;
pub mod decoder;
pub mod gc;
pub mod integration;
pub mod protocol;
pub mod renderer;

pub use cache::{
    CacheConfig, CacheMetrics, CacheStats, EvictionPolicy as CacheEvictionPolicy, ImageCache,
    ImageCacheEntry,
};
pub use decoder::{DecodedImage, ImageDecoder, ImageFormat};
pub use gc::{
    GcConfig, GcConfigBuilder, GcError, GcEvictionPolicy, GcStats, ImageGarbageCollector,
};
pub use protocol::{GraphicsCommand, ImageProtocolHandler, TransmissionMode};
pub use renderer::{
    ImageAlignment, ImageFormat as RendererImageFormat, ImageRenderCommand, ImageRenderer,
    ImageTexture, ImageUniforms, ImageVertex, RenderBatch, RenderQuad, ScalingMode,
};

/// Unique identifier for images
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(pub u32);

/// Image data and metadata
#[derive(Debug, Clone)]
pub struct Image {
    pub id: ImageId,
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub data: Vec<u8>,
    pub created_at: Instant,
    pub last_accessed: Instant,
}

impl Image {
    /// Create a new image
    pub fn new(id: ImageId, width: u32, height: u32, format: ImageFormat, data: Vec<u8>) -> Self {
        let now = Instant::now();
        Self {
            id,
            width,
            height,
            format,
            data,
            created_at: now,
            last_accessed: now,
        }
    }

    /// Mark image as accessed
    pub fn touch(&mut self) {
        self.last_accessed = Instant::now();
    }

    /// Check if image is older than given duration
    pub fn is_older_than(&self, duration: Duration) -> bool {
        self.last_accessed.elapsed() > duration
    }

    /// Get image size in bytes
    pub fn size_bytes(&self) -> usize {
        self.data.len()
    }
}

/// Image placement in terminal grid
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImagePlacement {
    pub image_id: ImageId,
    pub row: usize,
    pub col: usize,
    pub width_cells: usize,
    pub height_cells: usize,
    pub z_index: i32,
}

/// Image manager coordinates all image operations
pub struct ImageManager {
    pub protocol_handler: ImageProtocolHandler,
    cache: ImageCache,
    decoder: ImageDecoder,
    next_id: u32,
}

impl ImageManager {
    /// Create a new image manager
    pub fn new() -> Self {
        Self {
            protocol_handler: ImageProtocolHandler::new(),
            cache: ImageCache::new(),
            decoder: ImageDecoder::new(),
            next_id: 1,
        }
    }

    /// Generate a new unique image ID
    fn generate_id(&mut self) -> ImageId {
        let id = ImageId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// Process a graphics command from terminal
    pub fn process_command(&mut self, command: GraphicsCommand) -> Result<(), ImageError> {
        match command {
            GraphicsCommand::Transmit {
                data,
                format,
                width,
                height,
            } => {
                let id = self.generate_id();
                let image = Image::new(id, width, height, format, data);
                self.cache.store(image);
                Ok(())
            }
            GraphicsCommand::Display {
                image_id: _,
                row: _,
                col: _,
                width: _,
                height: _,
            } => {
                // Place image in grid (handled by terminal)
                Ok(())
            }
            GraphicsCommand::Delete { image_id } => {
                self.cache.remove(image_id);
                Ok(())
            }
            GraphicsCommand::Clear => {
                self.cache.clear();
                Ok(())
            }
        }
    }

    /// Get image from cache
    pub fn get_image(&self, id: ImageId) -> Option<Image> {
        self.cache.get(id)
    }

    /// Get the image decoder used by this manager.
    pub fn decoder(&self) -> &ImageDecoder {
        &self.decoder
    }

    /// Run garbage collection on old images
    pub fn garbage_collect(&mut self, max_age: Duration) {
        self.cache.remove_old(max_age);
    }

    /// Get total memory used by images
    pub fn memory_usage(&self) -> usize {
        self.cache.total_size()
    }
}

impl Default for ImageManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Image errors
#[derive(Debug, thiserror::Error)]
pub enum ImageError {
    #[error("Decode error: {0}")]
    Decode(String),

    #[error("Invalid format: {0}")]
    InvalidFormat(String),

    #[error("Image not found: {0:?}")]
    NotFound(ImageId),

    #[error("Cache full")]
    CacheFull,

    #[error("Invalid dimensions: {0}x{1}")]
    InvalidDimensions(u32, u32),

    #[error("Protocol error: {0}")]
    Protocol(String),
}

/// Result type for image operations
pub type ImageResult<T> = Result<T, ImageError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_image_id_generation() {
        let mut manager = ImageManager::new();

        let id1 = manager.generate_id();
        let id2 = manager.generate_id();

        assert_eq!(id1.0, 1);
        assert_eq!(id2.0, 2);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_image_creation() {
        let image = Image::new(ImageId(1), 100, 100, ImageFormat::Png, vec![0u8; 1024]);

        assert_eq!(image.width, 100);
        assert_eq!(image.height, 100);
        assert_eq!(image.size_bytes(), 1024);
    }

    #[test]
    fn test_image_placement() {
        let placement = ImagePlacement {
            image_id: ImageId(1),
            row: 5,
            col: 10,
            width_cells: 20,
            height_cells: 10,
            z_index: 0,
        };

        assert_eq!(placement.row, 5);
        assert_eq!(placement.col, 10);
        assert_eq!(placement.width_cells, 20);
        assert_eq!(placement.height_cells, 10);
    }
}
