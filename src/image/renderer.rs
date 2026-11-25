//! Image Renderer
//!
//! Renders images to the terminal using wgpu textures.
//! Handles texture upload, scaling, and placement in the grid.

use super::{decoder::DecodedImage, ImageError, ImageId};
use std::collections::HashMap;
use wgpu::util::{BufferInitDescriptor, DeviceExt};
use wgpu::*;

pub const SHADER_SOURCE: &str = include_str!("shaders/image.wgsl");

#[derive(Debug)]
pub struct ImageTexture {
    pub id: ImageId,
    pub width: u32,
    pub height: u32,
    texture: Texture,
    bind_group: BindGroup,
    sampler: Sampler,
}

impl ImageTexture {
    pub fn new(
        id: ImageId,
        width: u32,
        height: u32,
        texture: Texture,
        bind_group: BindGroup,
        sampler: Sampler,
    ) -> Self {
        Self {
            id,
            width,
            height,
            texture,
            bind_group,
            sampler,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn scaled_dimensions(
        &self,
        cell_width: f32,
        cell_height: f32,
        num_cols: usize,
        num_rows: usize,
    ) -> (f32, f32) {
        let target_width = cell_width * num_cols as f32;
        let target_height = cell_height * num_rows as f32;

        let aspect = self.width as f32 / self.height as f32;
        let target_aspect = target_width / target_height;

        if aspect > target_aspect {
            (target_width, target_width / aspect)
        } else {
            (target_height * aspect, target_height)
        }
    }

    pub fn bind_group(&self) -> &BindGroup {
        &self.bind_group
    }

    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageFormat {
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Bgra8Unorm,
}

impl From<ImageFormat> for TextureFormat {
    fn from(format: ImageFormat) -> Self {
        match format {
            ImageFormat::Rgba8Unorm => TextureFormat::Rgba8Unorm,
            ImageFormat::Rgba8UnormSrgb => TextureFormat::Rgba8UnormSrgb,
            ImageFormat::Bgra8Unorm => TextureFormat::Bgra8Unorm,
        }
    }
}

pub struct ImageRenderer {
    device: Device,
    queue: Queue,
    format: TextureFormat,
    pipeline: RenderPipeline,
    bind_group_layout: BindGroupLayout,
    sampler: Sampler,
    textures: HashMap<ImageId, ImageTexture>,
    max_texture_size: u32,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ImageVertex {
    pub position: [f32; 2],
    pub tex_coords: [f32; 2],
}

impl ImageVertex {
    const ATTRIBS: [VertexAttribute; 2] = [
        VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: VertexFormat::Float32x2,
        },
        VertexAttribute {
            offset: 8,
            shader_location: 1,
            format: VertexFormat::Float32x2,
        },
    ];

    const LAYOUT: VertexBufferLayout<'static> = VertexBufferLayout {
        array_stride: std::mem::size_of::<ImageVertex>() as BufferAddress,
        step_mode: VertexStepMode::Vertex,
        attributes: &Self::ATTRIBS,
    };
}

const QUAD_VERTICES: &[ImageVertex] = &[
    ImageVertex {
        position: [0.0, 0.0],
        tex_coords: [0.0, 0.0],
    },
    ImageVertex {
        position: [1.0, 0.0],
        tex_coords: [1.0, 0.0],
    },
    ImageVertex {
        position: [1.0, 1.0],
        tex_coords: [1.0, 1.0],
    },
    ImageVertex {
        position: [0.0, 1.0],
        tex_coords: [0.0, 1.0],
    },
];

const QUAD_INDICES: &[u16] = &[0, 1, 2, 0, 2, 3];

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ImageUniforms {
    pub transform: [f32; 6],
}

impl Default for ImageUniforms {
    fn default() -> Self {
        Self {
            transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        }
    }
}

impl ImageUniforms {
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        screen_width: f32,
        screen_height: f32,
    ) -> Self {
        let sx = width / screen_width * 2.0;
        let sy = height / screen_height * 2.0;
        let tx = (x / screen_width * 2.0) - 1.0;
        let ty = 1.0 - (y / screen_height * 2.0);

        Self {
            transform: [sx, 0.0, tx, 0.0, -sy, ty],
        }
    }
}

#[derive(Clone)]
pub struct ImageRenderCommand {
    pub image_id: ImageId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub z_index: i32,
}

impl ImageRenderer {
    pub fn new(device: Device, queue: Queue, format: TextureFormat) -> Result<Self, ImageError> {
        let max_texture_size = device.limits().max_texture_dimension_2d;

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Image Sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Image Bind Group Layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        view_dimension: TextureViewDimension::D2,
                        sample_type: TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Image Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Image Shader"),
            source: ShaderSource::Wgsl(SHADER_SOURCE.into()),
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Image Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[ImageVertex::LAYOUT],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Image Vertex Buffer"),
            contents: bytemuck::cast_slice(QUAD_VERTICES),
            usage: BufferUsages::VERTEX,
        });

        let index_buffer = device.create_buffer_init(&BufferInitDescriptor {
            label: Some("Image Index Buffer"),
            contents: bytemuck::cast_slice(QUAD_INDICES),
            usage: BufferUsages::INDEX,
        });

        Ok(Self {
            device,
            queue,
            format,
            pipeline,
            bind_group_layout,
            sampler,
            textures: HashMap::new(),
            max_texture_size,
            vertex_buffer,
            index_buffer,
        })
    }

    pub fn create_texture(&mut self, id: ImageId, image: &DecodedImage) -> Result<(), ImageError> {
        if image.width > self.max_texture_size || image.height > self.max_texture_size {
            return Err(ImageError::InvalidDimensions(image.width, image.height));
        }

        let expected_size = (image.width * image.height * 4) as usize;
        if image.rgba_data.len() != expected_size {
            return Err(ImageError::Decode(format!(
                "Invalid texture data size: {} (expected: {})",
                image.rgba_data.len(),
                expected_size
            )));
        }

        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some(&format!("Image Texture {:?}", id)),
            size: Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: self.format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &image.rgba_data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );

        let view = texture.create_view(&TextureViewDescriptor::default());

        let bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("Image Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        let image_texture = ImageTexture::new(
            id,
            image.width,
            image.height,
            texture,
            bind_group,
            self.sampler.clone(),
        );
        self.textures.insert(id, image_texture);

        Ok(())
    }

    pub fn update_texture(&mut self, id: ImageId, image: &DecodedImage) -> Result<(), ImageError> {
        let texture = self.textures.get(&id).ok_or(ImageError::NotFound(id))?;

        if image.width != texture.width || image.height != texture.height {
            return Err(ImageError::InvalidDimensions(image.width, image.height));
        }

        self.queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &image.rgba_data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );

        Ok(())
    }

    pub fn destroy_texture(&mut self, id: ImageId) {
        self.textures.remove(&id);
    }

    pub fn has_texture(&self, id: ImageId) -> bool {
        self.textures.contains_key(&id)
    }

    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    pub fn render<'pass>(
        &'pass self,
        render_pass: &mut RenderPass<'pass>,
        commands: &[ImageRenderCommand],
        _screen_width: f32,
        _screen_height: f32,
    ) {
        let mut sorted_commands = commands.to_vec();
        sorted_commands.sort_by_key(|c| c.z_index);

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), IndexFormat::Uint16);

        for command in &sorted_commands {
            if let Some(texture) = self.textures.get(&command.image_id) {
                render_pass.set_bind_group(0, texture.bind_group(), &[]);
                render_pass.draw_indexed(0..6, 0, 0..1);
            }
        }
    }

    pub fn calculate_cell_coverage(
        &self,
        texture: &ImageTexture,
        cell_width: f32,
        cell_height: f32,
    ) -> (usize, usize) {
        let cols = (texture.width as f32 / cell_width).ceil() as usize;
        let rows = (texture.height as f32 / cell_height).ceil() as usize;
        (cols.max(1), rows.max(1))
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

impl Drop for ImageRenderer {
    fn drop(&mut self) {
        self.textures.clear();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RenderQuad {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub tex_coords: [f32; 4],
}

impl RenderQuad {
    pub fn vertices(&self) -> [[f32; 4]; 6] {
        let x1 = self.x;
        let y1 = self.y;
        let x2 = self.x + self.width;
        let y2 = self.y + self.height;

        let u1 = self.tex_coords[0];
        let v1 = self.tex_coords[1];
        let u2 = self.tex_coords[2];
        let v2 = self.tex_coords[3];

        [
            [x1, y1, u1, v1],
            [x2, y1, u2, v1],
            [x2, y2, u2, v2],
            [x1, y1, u1, v1],
            [x2, y2, u2, v2],
            [x1, y2, u1, v2],
        ]
    }
}

#[derive(Debug, Default)]
pub struct RenderBatch {
    quads: Vec<RenderQuad>,
}

impl RenderBatch {
    pub fn new() -> Self {
        Self { quads: Vec::new() }
    }

    pub fn add(&mut self, quad: RenderQuad) {
        self.quads.push(quad);
    }

    pub fn vertices(&self) -> Vec<f32> {
        self.quads
            .iter()
            .flat_map(|q| q.vertices().into_iter().flatten())
            .collect()
    }

    pub fn clear(&mut self) {
        self.quads.clear();
    }

    pub fn len(&self) -> usize {
        self.quads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalingMode {
    Fit,
    Fill,
    Stretch,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ImageAlignment {
    TopLeft,
    TopCenter,
    TopRight,
    CenterLeft,
    Center,
    CenterRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

/// Dispositivo wgpu para testes (adaptador de software, ex.: llvmpipe)
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) fn test_device() -> Option<(Device, Queue)> {
        let instance = Instance::new(&InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;
        pollster::block_on(adapter.request_device(
            &DeviceDescriptor {
                label: Some("image test device"),
                required_features: Features::empty(),
                required_limits:
                    Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                memory_hints: MemoryHints::default(),
            },
            None,
        ))
        .ok()
    }

    /// Textura 1x1 real, ou None sem adaptador disponível
    pub(crate) fn test_texture(id: ImageId) -> Option<std::sync::Arc<ImageTexture>> {
        let (device, queue) = test_device()?;
        let mut renderer = ImageRenderer::new(device, queue, TextureFormat::Rgba8Unorm).ok()?;
        renderer
            .create_texture(id, &DecodedImage::new(1, 1, vec![0; 4]))
            .ok()?;
        renderer.textures.remove(&id).map(std::sync::Arc::new)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::test_device;
    use super::*;

    #[test]
    fn test_image_vertex_layout() {
        assert_eq!(std::mem::size_of::<ImageVertex>(), 16);
    }

    #[test]
    fn test_render_quad_vertices() {
        let quad = RenderQuad {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            tex_coords: [0.0, 0.0, 1.0, 1.0],
        };

        let vertices = quad.vertices();
        assert_eq!(vertices.len(), 6);
        assert_eq!(vertices[0], [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(vertices[1], [100.0, 0.0, 1.0, 0.0]);
        assert_eq!(vertices[2], [100.0, 50.0, 1.0, 1.0]);
    }

    #[test]
    fn test_render_batch() {
        let mut batch = RenderBatch::new();
        assert!(batch.is_empty());

        batch.add(RenderQuad {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            tex_coords: [0.0, 0.0, 1.0, 1.0],
        });

        assert_eq!(batch.len(), 1);
        assert!(!batch.is_empty());

        let vertices = batch.vertices();
        assert_eq!(vertices.len(), 24);

        batch.clear();
        assert!(batch.is_empty());
    }

    #[test]
    fn test_image_uniforms() {
        let uniforms = ImageUniforms::new(100.0, 200.0, 50.0, 75.0, 800.0, 600.0);

        let sx = 50.0 / 800.0 * 2.0;
        let sy = 75.0 / 600.0 * 2.0;
        let tx = (100.0 / 800.0 * 2.0) - 1.0;
        let ty = 1.0 - (200.0 / 600.0 * 2.0);

        assert!((uniforms.transform[0] - sx).abs() < 0.001);
        assert!((uniforms.transform[4] - (-sy)).abs() < 0.001);
        assert!((uniforms.transform[2] - tx).abs() < 0.001);
        assert!((uniforms.transform[5] - ty).abs() < 0.001);
    }

    #[test]
    fn test_scaling_mode() {
        assert_eq!(ScalingMode::Fit, ScalingMode::Fit);
        assert_ne!(ScalingMode::Fit, ScalingMode::Fill);
    }

    #[test]
    fn test_image_alignment() {
        assert_eq!(ImageAlignment::Center, ImageAlignment::Center);
        assert_ne!(ImageAlignment::TopLeft, ImageAlignment::BottomRight);
    }

    // ---------------- GPU (adaptador de software, ex.: llvmpipe) ----------------

    /// Cria o renderer; sem adaptador disponível o teste é pulado.
    fn renderer() -> Option<ImageRenderer> {
        let Some((device, queue)) = test_device() else {
            eprintln!("sem adaptador wgpu: teste de GPU pulado");
            return None;
        };
        Some(ImageRenderer::new(device, queue, TextureFormat::Rgba8Unorm).unwrap())
    }

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> DecodedImage {
        DecodedImage::new(width, height, rgba.repeat((width * height) as usize))
    }

    #[test]
    fn test_texture_lifecycle() {
        let Some(mut renderer) = renderer() else {
            return;
        };
        let id = ImageId(1);
        assert!(!renderer.has_texture(id));

        renderer
            .create_texture(id, &solid(4, 2, [1, 2, 3, 4]))
            .unwrap();
        assert!(renderer.has_texture(id));
        assert_eq!(renderer.texture_count(), 1);

        // Atualização com o mesmo tamanho funciona; tamanho diferente não
        renderer
            .update_texture(id, &solid(4, 2, [9, 9, 9, 9]))
            .unwrap();
        assert!(matches!(
            renderer.update_texture(id, &solid(2, 2, [0; 4])),
            Err(ImageError::InvalidDimensions(2, 2))
        ));
        assert!(matches!(
            renderer.update_texture(ImageId(99), &solid(4, 2, [0; 4])),
            Err(ImageError::NotFound(ImageId(99)))
        ));

        let texture = renderer.textures.get(&id).unwrap();
        assert_eq!(texture.size(), (4, 2));
        let _ = texture.sampler();
        assert_eq!(renderer.calculate_cell_coverage(texture, 3.0, 1.5), (2, 2));
        // Imagem larga (aspecto 2) em área 20x40: limitada pela largura
        assert_eq!(texture.scaled_dimensions(10.0, 20.0, 2, 2), (20.0, 10.0));

        renderer
            .create_texture(ImageId(3), &solid(1, 4, [0; 4]))
            .unwrap();
        let tall = renderer.textures.get(&ImageId(3)).unwrap();
        // Imagem alta (aspecto 0.25): limitada pela altura
        assert_eq!(tall.scaled_dimensions(10.0, 20.0, 2, 2), (10.0, 40.0));
        assert_eq!(renderer.calculate_cell_coverage(tall, 100.0, 100.0), (1, 1));
        renderer.destroy_texture(ImageId(3));

        renderer
            .create_texture(ImageId(2), &solid(1, 1, [0; 4]))
            .unwrap();
        renderer.destroy_texture(id);
        assert!(!renderer.has_texture(id));
        assert_eq!(renderer.texture_count(), 1);
        renderer.clear();
        assert_eq!(renderer.texture_count(), 0);
    }

    #[test]
    fn test_create_texture_validates_input() {
        let Some(mut renderer) = renderer() else {
            return;
        };
        // Dados com tamanho errado
        let bad = DecodedImage::new(2, 2, vec![0; 3]);
        let err = renderer.create_texture(ImageId(1), &bad).unwrap_err();
        assert!(
            err.to_string().contains("Invalid texture data size"),
            "{err}"
        );

        // Dimensão acima do limite do dispositivo
        let too_big = renderer.max_texture_size + 1;
        let huge = DecodedImage::new(too_big, 1, Vec::new());
        assert!(matches!(
            renderer.create_texture(ImageId(2), &huge),
            Err(ImageError::InvalidDimensions(w, 1)) if w == too_big
        ));
        assert_eq!(renderer.texture_count(), 0);
    }

    /// Renderiza num alvo 4x4 e lê os pixels de volta.
    fn render_and_read(renderer: &ImageRenderer, commands: &[ImageRenderCommand]) -> Vec<u8> {
        let device = &renderer.device;
        let size = Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        };
        let target = device.create_texture(&TextureDescriptor {
            label: Some("alvo"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&TextureViewDescriptor::default());
        let padded_row = 256u32;
        let readback = device.create_buffer(&BufferDescriptor {
            label: Some("leitura"),
            size: (padded_row * 4) as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("passo"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            renderer.render(&mut pass, commands, 4.0, 4.0);
        }
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &readback,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(4),
                },
            },
            size,
        );
        renderer.queue.submit([encoder.finish()]);

        let slice = readback.slice(..);
        slice.map_async(MapMode::Read, |r| r.unwrap());
        device.poll(Maintain::Wait);
        let data = slice.get_mapped_range();
        // primeiro pixel da primeira linha
        data[..4].to_vec()
    }

    fn command(id: u32, z: i32) -> ImageRenderCommand {
        ImageRenderCommand {
            image_id: ImageId(id),
            x: 0.0,
            y: 0.0,
            width: 4.0,
            height: 4.0,
            z_index: z,
        }
    }

    #[test]
    fn test_render_draws_textures_in_z_order() {
        let Some(mut renderer) = renderer() else {
            return;
        };
        renderer
            .create_texture(ImageId(1), &solid(2, 2, [255, 0, 0, 255]))
            .unwrap();
        renderer
            .create_texture(ImageId(2), &solid(2, 2, [0, 0, 255, 255]))
            .unwrap();

        // Sem comandos (ou só com imagem inexistente) fica a cor de fundo
        assert_eq!(render_and_read(&renderer, &[]), vec![0, 0, 0, 255]);
        assert_eq!(
            render_and_read(&renderer, &[command(42, 0)]),
            vec![0, 0, 0, 255]
        );

        // O quad cobre o alvo todo; maior z_index é desenhado por último
        assert_eq!(
            render_and_read(&renderer, &[command(1, 0)]),
            vec![255, 0, 0, 255]
        );
        assert_eq!(
            render_and_read(&renderer, &[command(2, 5), command(1, 1)]),
            vec![0, 0, 255, 255]
        );
        assert_eq!(
            render_and_read(&renderer, &[command(1, 9), command(2, 1)]),
            vec![255, 0, 0, 255]
        );
    }

    // ---------------- CPU ----------------

    #[test]
    fn test_image_format_to_texture_format() {
        assert_eq!(
            TextureFormat::from(ImageFormat::Rgba8Unorm),
            TextureFormat::Rgba8Unorm
        );
        assert_eq!(
            TextureFormat::from(ImageFormat::Rgba8UnormSrgb),
            TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(
            TextureFormat::from(ImageFormat::Bgra8Unorm),
            TextureFormat::Bgra8Unorm
        );
    }

    #[test]
    fn test_uniforms_default_is_identity() {
        assert_eq!(
            ImageUniforms::default().transform,
            [1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        );
        // Imagem cobrindo a tela inteira
        let full = ImageUniforms::new(0.0, 0.0, 800.0, 600.0, 800.0, 600.0);
        assert_eq!(full.transform, [2.0, 0.0, -1.0, 0.0, -2.0, 1.0]);
    }

    #[test]
    fn test_batch_vertices_concatenate_quads() {
        let mut batch = RenderBatch::default();
        for i in 0..2 {
            batch.add(RenderQuad {
                x: i as f32 * 10.0,
                y: 0.0,
                width: 10.0,
                height: 5.0,
                tex_coords: [0.0, 0.0, 0.5, 0.5],
            });
        }
        let vertices = batch.vertices();
        assert_eq!(vertices.len(), 2 * 6 * 4);
        // Último vértice do segundo quad: (x1, y2, u1, v2)
        assert_eq!(&vertices[44..48], &[10.0, 5.0, 0.0, 0.5]);
    }

    #[test]
    fn test_quad_layout_constants() {
        assert_eq!(ImageVertex::LAYOUT.array_stride, 16);
        assert_eq!(ImageVertex::ATTRIBS[1].offset, 8);
        assert_eq!(QUAD_VERTICES.len(), 4);
        assert_eq!(QUAD_INDICES, &[0, 1, 2, 0, 2, 3]);
        assert!(SHADER_SOURCE.contains("fn vs_main"));
        assert!(SHADER_SOURCE.contains("fn fs_main"));
    }
}
