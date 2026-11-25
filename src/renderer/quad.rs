//! Renderizador simples de quadrados coloridos para UI básica.
//!
//! Usado para desenhar cursor, seleção, barra de tabs e outros overlays
//! sem depender de uma UI toolkit completa.

use wgpu::*;

/// Converte um canal sRGB (0..1) para linear.
///
/// As cores da UI são definidas em sRGB (`#1e1e1e`). Quando a superfície é
/// sRGB, a GPU codifica de novo na saída; sem esta conversão tudo sai
/// desbotado (o `#1e1e1e` vira um cinza médio).
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Converte uma cor RGBA sRGB para linear (o alfa fica como está).
pub fn srgb_to_linear_rgba(color: [f32; 4]) -> [f32; 4] {
    [
        srgb_to_linear(color[0]),
        srgb_to_linear(color[1]),
        srgb_to_linear(color[2]),
        color[3],
    ]
}

/// Blend "over" que mantém o alfa da janela em 1: só a cor é misturada.
/// Com `ALPHA_BLENDING` puro um quad translúcido deixava o framebuffer com
/// alfa < 1 e o compositor mostrava o papel de parede através do painel.
pub const OPAQUE_OVER_BLENDING: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::SrcAlpha,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    },
};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

/// Lote de quadrados a serem desenhados em um único draw call.
/// Coordenadas armazenadas em pixels (origem no topo-esquerdo).
#[derive(Debug, Default)]
pub struct QuadBatch {
    quads: Vec<Quad>,
}

#[derive(Copy, Clone, Debug)]
struct Quad {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: [f32; 4],
}

impl QuadBatch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adiciona um quadrado ao lote. Coordenadas em pixels, origem no topo-esquerdo.
    pub fn add(&mut self, x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) {
        self.quads.push(Quad {
            x,
            y,
            width,
            height,
            color,
        });
    }

    /// Adiciona um quadrado com uma cor do tema (sRGB) e opacidade.
    pub fn add_color(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: glyphon::Color,
        alpha: f32,
    ) {
        self.add(
            x,
            y,
            width,
            height,
            [
                color.r() as f32 / 255.0,
                color.g() as f32 / 255.0,
                color.b() as f32 / 255.0,
                alpha,
            ],
        );
    }

    /// Número de quadrados no lote.
    pub fn len(&self) -> usize {
        self.quads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.quads.is_empty()
    }

    pub fn clear(&mut self) {
        self.quads.clear();
    }

    fn vertex_count(&self) -> u32 {
        self.quads.len() as u32 * 6
    }

    /// Preenche um slice de vértices já convertidos para NDC.
    /// Com `linear`, as cores sRGB são convertidas para a superfície sRGB.
    fn fill_vertices(&self, width: f32, height: f32, linear: bool, out: &mut [Vertex]) {
        for (i, quad) in self.quads.iter().enumerate() {
            let color = if linear {
                srgb_to_linear_rgba(quad.color)
            } else {
                quad.color
            };
            let quad = Quad { color, ..*quad };
            let x1 = (2.0 * quad.x / width) - 1.0;
            let y1 = 1.0 - (2.0 * quad.y / height);
            let x2 = (2.0 * (quad.x + quad.width) / width) - 1.0;
            let y2 = 1.0 - (2.0 * (quad.y + quad.height) / height);

            let base = i * 6;
            out[base] = Vertex {
                position: [x1, y1],
                color: quad.color,
            };
            out[base + 1] = Vertex {
                position: [x2, y1],
                color: quad.color,
            };
            out[base + 2] = Vertex {
                position: [x1, y2],
                color: quad.color,
            };
            out[base + 3] = Vertex {
                position: [x1, y2],
                color: quad.color,
            };
            out[base + 4] = Vertex {
                position: [x2, y1],
                color: quad.color,
            };
            out[base + 5] = Vertex {
                position: [x2, y2],
                color: quad.color,
            };
        }
    }
}

/// Renderizador de quadrados coloridos sobre a tela.
pub struct QuadRenderer {
    pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    staging: Vec<Vertex>,
    buffer_capacity: u64,
    /// A superfície é sRGB: as cores precisam ir lineares.
    linear_output: bool,
}

impl QuadRenderer {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Quad Shader"),
            source: ShaderSource::Wgsl(include_str!("quad.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("Quad Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Quad Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &[
                        VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: VertexFormat::Float32x2,
                        },
                        VertexAttribute {
                            offset: std::mem::size_of::<[f32; 2]>() as u64,
                            shader_location: 1,
                            format: VertexFormat::Float32x4,
                        },
                    ],
                }],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(OPAQUE_OVER_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
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
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Capacidade inicial: 256 quads (cursor, tabs, seleção etc.).
        let initial_capacity = 256 * 6 * std::mem::size_of::<Vertex>() as u64;
        let vertex_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("Quad Vertex Buffer"),
            size: initial_capacity,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            vertex_buffer,
            staging: Vec::with_capacity(256 * 6),
            buffer_capacity: initial_capacity,
            linear_output: format.is_srgb(),
        }
    }

    /// Renderiza um lote de quadrados.
    pub fn render(
        &mut self,
        device: &Device,
        queue: &Queue,
        render_pass: &mut RenderPass<'_>,
        batch: &QuadBatch,
        viewport_width: u32,
        viewport_height: u32,
    ) {
        if batch.is_empty() || viewport_width == 0 || viewport_height == 0 {
            return;
        }

        let vertex_count = batch.vertex_count() as usize;
        let bytes_needed = vertex_count as u64 * std::mem::size_of::<Vertex>() as u64;

        // Realoca o buffer se necessário.
        if bytes_needed > self.buffer_capacity {
            let new_capacity = (bytes_needed * 2).max(self.buffer_capacity * 2);
            self.vertex_buffer = device.create_buffer(&BufferDescriptor {
                label: Some("Quad Vertex Buffer"),
                size: new_capacity,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.buffer_capacity = new_capacity;
            self.staging.reserve(vertex_count);
        }

        self.staging.clear();
        self.staging.resize(
            vertex_count,
            Vertex {
                position: [0.0, 0.0],
                color: [0.0, 0.0, 0.0, 0.0],
            },
        );

        batch.fill_vertices(
            viewport_width as f32,
            viewport_height as f32,
            self.linear_output,
            &mut self.staging,
        );

        queue.write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.staging));

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..bytes_needed));
        render_pass.draw(0..vertex_count as u32, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_srgb_to_linear_endpoints_and_midtones() {
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        // #1e1e1e (0.1176) é bem mais escuro em linear.
        let dark = srgb_to_linear(30.0 / 255.0);
        assert!((dark - 0.0129).abs() < 0.001, "{dark}");
    }

    #[test]
    fn test_linear_rgba_keeps_alpha() {
        let c = srgb_to_linear_rgba([0.5, 0.5, 0.5, 0.3]);
        assert_eq!(c[3], 0.3);
        assert!(c[0] < 0.5);
    }

    #[test]
    fn test_fill_vertices_converts_only_when_linear() {
        let mut batch = QuadBatch::new();
        batch.add_color(
            0.0,
            0.0,
            10.0,
            10.0,
            glyphon::Color::rgb(128, 128, 128),
            1.0,
        );
        assert_eq!(batch.len(), 1);
        let blank = Vertex {
            position: [0.0, 0.0],
            color: [0.0; 4],
        };
        let mut srgb = vec![blank; 6];
        batch.fill_vertices(100.0, 100.0, false, &mut srgb);
        let mut linear = vec![blank; 6];
        batch.fill_vertices(100.0, 100.0, true, &mut linear);
        assert!((srgb[0].color[0] - 128.0 / 255.0).abs() < 1e-6);
        assert!(linear[0].color[0] < srgb[0].color[0]);
        // Quad cobre de (-1, 1) até (-0.8, 0.8) em NDC.
        assert_eq!(srgb[0].position, [-1.0, 1.0]);
        assert_eq!(srgb[5].position, [-0.8, 0.8]);
    }

    #[test]
    fn test_blend_keeps_window_opaque() {
        // alfa_out = 1 * src_a + (1 - src_a) * 1 = 1 quando o destino é opaco.
        assert_eq!(OPAQUE_OVER_BLENDING.alpha.src_factor, BlendFactor::One);
        assert_eq!(
            OPAQUE_OVER_BLENDING.alpha.dst_factor,
            BlendFactor::OneMinusSrcAlpha
        );
    }
}
