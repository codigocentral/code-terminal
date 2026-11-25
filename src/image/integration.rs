//! Image Protocol Integration Module (US-020)
//!
//! Este módulo contém stubs para integração do ImageRenderer ao GpuState.
//! Quando ativado, permite renderização de imagens inline no terminal.

use crate::image::renderer::{ImageRenderCommand, ImageRenderer};
use crate::image::ImageId;

/// Trait para integração de imagens ao renderer principal
///
/// Implementado por GpuState quando US-020 estiver completo.
pub trait ImageRenderIntegration {
    /// Envia um comando de renderização de imagem
    fn queue_image_render(&mut self, command: ImageRenderCommand);

    /// Retorna referência ao renderizador de imagens
    fn image_renderer(&self) -> Option<&ImageRenderer>;

    /// Retorna referência mutável ao renderizador de imagens
    fn image_renderer_mut(&mut self) -> Option<&mut ImageRenderer>;
}

/// Helper para criar comando de renderização simples
pub fn create_image_render_command(
    image_id: ImageId,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> ImageRenderCommand {
    ImageRenderCommand {
        image_id,
        x,
        y,
        width,
        height,
        z_index: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ImageId;

    #[test]
    fn test_create_image_render_command() {
        let cmd = create_image_render_command(ImageId(1), 10.0, 20.0, 100.0, 50.0);
        assert_eq!(cmd.image_id.0, 1);
        assert_eq!(cmd.x, 10.0);
        assert_eq!(cmd.y, 20.0);
        assert_eq!(cmd.width, 100.0);
        assert_eq!(cmd.height, 50.0);
    }
}
