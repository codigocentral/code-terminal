//! Desenho das imagens inline: uma textura por imagem e um retângulo por
//! posição, recortado pela área do painel.

use crate::terminal::images::Pixels;
use std::collections::HashMap;
use wgpu::util::DeviceExt;
use wgpu::*;

/// Imagem a desenhar neste frame (coordenadas em pixels da janela).
#[derive(Clone, Debug)]
pub struct ImageDraw {
    pub id: u64,
    pub pixels: Pixels,
    /// Caixa reservada em células (a imagem cabe nela mantendo a proporção).
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Área visível (o painel): o que passar disso é cortado.
    pub clip: [f32; 4],
}

/// Retângulo final na tela e coordenadas de textura (já com recorte).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageQuad {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
}

/// Encaixa a imagem na caixa (proporção mantida, ancorada no canto) e corta
/// pela área visível. `None` se nada aparece.
pub fn fit_and_clip(draw: &ImageDraw) -> Option<ImageQuad> {
    let (img_w, img_h) = draw.pixels.dimensions();
    let (img_w, img_h) = (img_w.max(1) as f32, img_h.max(1) as f32);
    let scale = (draw.width / img_w).min(draw.height / img_h);
    let (w, h) = (img_w * scale, img_h * scale);
    let (x0, y0, x1, y1) = (draw.x, draw.y, draw.x + w, draw.y + h);
    let [cx, cy, cw, ch] = draw.clip;
    let (vx0, vy0) = (x0.max(cx), y0.max(cy));
    let (vx1, vy1) = (x1.min(cx + cw), y1.min(cy + ch));
    if vx1 <= vx0 || vy1 <= vy0 {
        return None;
    }
    Some(ImageQuad {
        rect: [vx0, vy0, vx1 - vx0, vy1 - vy0],
        uv: [
            (vx0 - x0) / w,
            (vy0 - y0) / h,
            (vx1 - x0) / w,
            (vy1 - y0) / h,
        ],
    })
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

const SHADER: &str = r#"
struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) uv: vec2<f32>) -> Out {
    var out: Out;
    out.position = vec4<f32>(position, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(input: Out) -> @location(0) vec4<f32> {
    return textureSample(image, image_sampler, input.uv);
}
"#;

struct CachedTexture {
    bind_group: BindGroup,
    used: bool,
}

/// Renderer das imagens inline.
pub struct ImageQuadRenderer {
    pipeline: RenderPipeline,
    layout: BindGroupLayout,
    sampler: Sampler,
    textures: HashMap<u64, CachedTexture>,
    vertex_buffer: Option<Buffer>,
    /// (id, primeiro vértice) de cada imagem do frame
    draws: Vec<(u64, u32)>,
}

impl ImageQuadRenderer {
    pub fn new(device: &Device, format: TextureFormat) -> Self {
        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("Image Shader"),
            source: ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("Image Bind Group Layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
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
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("Image Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as BufferAddress,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format,
                    blend: Some(crate::renderer::quad::OPAQUE_OVER_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: PrimitiveState::default(),
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("Image Sampler"),
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            textures: HashMap::new(),
            vertex_buffer: None,
            draws: Vec::new(),
        }
    }

    /// Envia texturas novas e monta os vértices do frame; descarta as
    /// texturas que não aparecem mais.
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        images: &[ImageDraw],
        screen: (u32, u32),
    ) {
        for cached in self.textures.values_mut() {
            cached.used = false;
        }
        self.draws.clear();
        let (sw, sh) = (screen.0.max(1) as f32, screen.1.max(1) as f32);
        let mut vertices: Vec<Vertex> = Vec::with_capacity(images.len() * 6);
        for draw in images {
            let Some(quad) = fit_and_clip(draw) else {
                continue;
            };
            if !self.textures.contains_key(&draw.id) {
                let bind_group = self.upload(device, queue, &draw.pixels);
                self.textures.insert(
                    draw.id,
                    CachedTexture {
                        bind_group,
                        used: true,
                    },
                );
            }
            if let Some(cached) = self.textures.get_mut(&draw.id) {
                cached.used = true;
            }
            let [x, y, w, h] = quad.rect;
            let [u0, v0, u1, v1] = quad.uv;
            let to_clip = |px: f32, py: f32| [px / sw * 2.0 - 1.0, 1.0 - py / sh * 2.0];
            let corners = [
                (to_clip(x, y), [u0, v0]),
                (to_clip(x + w, y), [u1, v0]),
                (to_clip(x, y + h), [u0, v1]),
                (to_clip(x + w, y + h), [u1, v1]),
            ];
            self.draws.push((draw.id, vertices.len() as u32));
            for i in [0, 1, 2, 2, 1, 3] {
                vertices.push(Vertex {
                    position: corners[i].0,
                    uv: corners[i].1,
                });
            }
        }
        self.textures.retain(|_, cached| cached.used);
        self.vertex_buffer = (!vertices.is_empty()).then(|| {
            device.create_buffer_init(&util::BufferInitDescriptor {
                label: Some("Image Vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: BufferUsages::VERTEX,
            })
        });
    }

    fn upload(&self, device: &Device, queue: &Queue, pixels: &Pixels) -> BindGroup {
        let (width, height) = pixels.dimensions();
        let size = Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("Inline Image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            pixels.as_raw(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width.max(1)),
                rows_per_image: Some(height.max(1)),
            },
            size,
        );
        let view = texture.create_view(&TextureViewDescriptor::default());
        device.create_bind_group(&BindGroupDescriptor {
            label: Some("Inline Image Bind Group"),
            layout: &self.layout,
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
        })
    }

    /// Desenha as imagens preparadas.
    pub fn render<'a>(&'a self, pass: &mut RenderPass<'a>) {
        let Some(buffer) = &self.vertex_buffer else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        for (id, first) in &self.draws {
            if let Some(cached) = self.textures.get(id) {
                pass.set_bind_group(0, &cached.bind_group, &[]);
                pass.draw(*first..*first + 6, 0..1);
            }
        }
    }

    /// Texturas em cache (para testes e métricas).
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn draw(img: (u32, u32), rect: [f32; 4], clip: [f32; 4]) -> ImageDraw {
        ImageDraw {
            id: 1,
            pixels: Arc::new(image::RgbaImage::new(img.0, img.1)),
            x: rect[0],
            y: rect[1],
            width: rect[2],
            height: rect[3],
            clip,
        }
    }

    #[test]
    fn test_fit_keeps_aspect_inside_the_box() {
        let quad = fit_and_clip(&draw(
            (100, 50),
            [10.0, 20.0, 40.0, 40.0],
            [0.0, 0.0, 500.0, 500.0],
        ))
        .unwrap();
        assert_eq!(quad.rect, [10.0, 20.0, 40.0, 20.0]);
        assert_eq!(quad.uv, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn test_clip_crops_rect_and_uv() {
        // Metade de cima já rolou para fora do painel.
        let quad = fit_and_clip(&draw(
            (10, 10),
            [0.0, -50.0, 100.0, 100.0],
            [0.0, 0.0, 200.0, 200.0],
        ))
        .unwrap();
        assert_eq!(quad.rect, [0.0, 0.0, 100.0, 50.0]);
        assert_eq!(quad.uv, [0.0, 0.5, 1.0, 1.0]);
        assert!(fit_and_clip(&draw(
            (10, 10),
            [0.0, 300.0, 50.0, 50.0],
            [0.0, 0.0, 200.0, 200.0]
        ))
        .is_none());
    }
}
