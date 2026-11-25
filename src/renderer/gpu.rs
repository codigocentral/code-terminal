use crate::config::{ColorScheme, FontConfig};
use crate::renderer::quad::{srgb_to_linear, QuadBatch, QuadRenderer};
use crate::renderer::text::TextRenderer;
use anyhow::Result;
use std::sync::Arc;
use wgpu::*;
use winit::window::Window;

pub struct GpuState {
    pub instance: Instance,
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
    /// Superfície da janela; `None` no modo sem janela (snapshot).
    pub surface: Option<Surface<'static>>,
    /// Formato e tamanho do alvo (também usado no modo sem janela).
    pub config: SurfaceConfiguration,
    /// Textura de destino no modo sem janela.
    offscreen: Option<Texture>,
    pub text_renderer: TextRenderer,
    /// Quads desenhados antes do texto do grid (fundos, seleção, cursor, tabs).
    pub quad_renderer: QuadRenderer,
    /// Quads desenhados depois do texto do grid (sublinhados, painéis, modais).
    pub overlay_quad_renderer: QuadRenderer,
    /// Imagens inline (iTerm2/kitty), desenhadas sobre os fundos das células.
    image_renderer: crate::renderer::image_quad::ImageQuadRenderer,
    /// Imagens do próximo frame.
    pending_images: Vec<crate::renderer::image_quad::ImageDraw>,
    /// Fator de escala da janela (DPI), usado para converter a fonte em pixels.
    scale_factor: f64,
}

/// Instância, adaptador e device compartilhados pelas janelas do processo.
/// Ligar a GPU é a parte cara da abertura (o driver da NVIDIA leva ~100 ms);
/// a segunda janela em diante, e as janelas do modo servidor, só criam a
/// própria superfície.
#[derive(Clone)]
struct SharedGpu {
    instance: Instance,
    adapter: Adapter,
    device: Device,
    queue: Queue,
}

static SHARED_GPU: std::sync::Mutex<Option<SharedGpu>> = std::sync::Mutex::new(None);

fn shared_gpu() -> Option<SharedGpu> {
    SHARED_GPU.lock().ok().and_then(|slot| slot.clone())
}

fn remember_gpu(gpu: SharedGpu) {
    if let Ok(mut slot) = SHARED_GPU.lock() {
        if slot.is_none() {
            *slot = Some(gpu);
        }
    }
}

/// Muda a cada janela que abre: um pedido de soltar a GPU feito antes dela
/// (com outra geração) não vale mais.
static GPU_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Uma janela vai abrir: cancela qualquer soltura da GPU já agendada.
pub fn keep_shared_gpu() {
    GPU_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

/// Marca de "sem janelas desde agora", para [`release_shared_gpu_if_idle`].
pub fn gpu_idle_ticket() -> u64 {
    GPU_GENERATION.load(std::sync::atomic::Ordering::SeqCst)
}

/// Solta a instância, o adaptador e o device compartilhados: o driver
/// devolve a memória dele e, na NVIDIA, a thread que acorda 100 vezes/s
/// some (#1, #30). A próxima janela liga a GPU de novo (~120–200 ms a mais).
/// Guardar só a instância e o adaptador encurta isso em ~60 ms, mas segura
/// ~55 MB do driver; por isso sai tudo. Os recursos de cada janela (atlas de
/// glifos, pipelines, buffers) já saíram com a janela. `true` se havia GPU
/// para soltar.
pub fn release_shared_gpu() -> bool {
    let released = SHARED_GPU.lock().ok().and_then(|mut slot| slot.take());
    released.is_some()
}

/// Dorme `delay` (uma acordada só) e solta a GPU se nenhuma janela abriu
/// desde `ticket`. Chame numa thread própria.
pub fn release_shared_gpu_if_idle(ticket: u64, delay: std::time::Duration) -> bool {
    std::thread::sleep(delay);
    if gpu_idle_ticket() != ticket {
        return false;
    }
    let released = release_shared_gpu();
    if released {
        tracing::info!(idle_secs = delay.as_secs(), "GPU released: no window open");
    }
    released
}

/// Blocos de memória da GPU: começam em 256 KB e crescem até 64 MB.
///
/// O `MemoryHints::MemoryUsage` do wgpu começa com blocos de 8 MB. Na NVIDIA,
/// a memória visível pela CPU (envio de vértices, uniforms e glifos) fica
/// presa na RAM inteira assim que o bloco é mapeado, e um terminal usa uma
/// fração disso: eram ~7,7 MB privados a mais por processo (#1). Sem diferença
/// mensurável na abertura.
pub fn memory_hints() -> MemoryHints {
    const KB: u64 = 1024;
    MemoryHints::Manual {
        suballocated_device_memory_block_size: 256 * KB..64 * KB * KB,
    }
}

/// Ordem dos backends a tentar: Vulkan/Metal/DX12 e depois GL, ou o que
/// `CODE_TERMINAL_GPU_BACKEND` pedir (`vulkan`, `gl`, `metal`, `dx12`).
fn backend_order() -> Vec<Backends> {
    backends_for(std::env::var("CODE_TERMINAL_GPU_BACKEND").ok().as_deref())
}

fn backends_for(choice: Option<&str>) -> Vec<Backends> {
    match choice.map(str::to_ascii_lowercase).as_deref() {
        Some("vulkan") => vec![Backends::VULKAN],
        Some("gl") | Some("opengl") => vec![Backends::GL],
        Some("metal") => vec![Backends::METAL],
        Some("dx12") => vec![Backends::DX12],
        _ => vec![Backends::PRIMARY, Backends::GL],
    }
}

/// Converte o tamanho da fonte em pontos para pixels físicos.
/// Antes o valor em pontos era usado direto como pixels: 12pt saía com 12px,
/// pequeno demais para ler.
pub fn points_to_pixels(points: f32, scale_factor: f64) -> f32 {
    (points * 96.0 / 72.0 * scale_factor as f32).max(1.0)
}

/// Converte a cor de fundo (sRGB) para o formato esperado pela superfície.
pub fn clear_color(color: glyphon::Color, srgb_surface: bool) -> Color {
    let channel = |v: u8| {
        let c = v as f32 / 255.0;
        if srgb_surface {
            srgb_to_linear(c) as f64
        } else {
            c as f64
        }
    };
    Color {
        r: channel(color.r()),
        g: channel(color.g()),
        b: channel(color.b()),
        a: 1.0,
    }
}

/// Quadros que a superfície pode ter na fila (`window.frame_latency`). Com
/// 1, o macOS usa 2 drawables em vez de 3: menos memória (cada um tem o
/// tamanho da janela), com o risco de perder quadros quando o desenho
/// atrasa. `CODE_TERMINAL_FRAME_LATENCY` troca (medições).
static FRAME_LATENCY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(2);

/// Define o padrão (da configuração), antes de criar as janelas.
pub fn set_frame_latency(frames: u32) {
    FRAME_LATENCY.store(frames.clamp(1, 3), std::sync::atomic::Ordering::Relaxed);
}

/// Quadros na fila: a variável de ambiente, senão a configuração.
pub fn frame_latency() -> u32 {
    std::env::var("CODE_TERMINAL_FRAME_LATENCY")
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .map(|frames| frames.clamp(1, 3))
        .unwrap_or_else(|| FRAME_LATENCY.load(std::sync::atomic::Ordering::Relaxed))
}

/// Modo de alfa: opaco sempre que o compositor permitir, para o terminal não
/// ficar transparente onde a UI desenha algo translúcido.
pub fn pick_alpha_mode(available: &[CompositeAlphaMode]) -> CompositeAlphaMode {
    if available.contains(&CompositeAlphaMode::Opaque) {
        CompositeAlphaMode::Opaque
    } else {
        available
            .first()
            .copied()
            .unwrap_or(CompositeAlphaMode::Auto)
    }
}

fn scaled_font(font_config: &FontConfig, scale_factor: f64) -> FontConfig {
    let mut font = font_config.clone();
    font.size = points_to_pixels(font_config.size, scale_factor);
    font
}

impl GpuState {
    pub async fn new(window: Arc<Window>) -> Result<Self> {
        Self::new_with_config(window, &FontConfig::default(), &ColorScheme::default()).await
    }

    pub async fn new_with_font(window: Arc<Window>, font_config: &FontConfig) -> Result<Self> {
        Self::new_with_config(window, font_config, &ColorScheme::default()).await
    }

    /// Procura um adaptador: primeiro Vulkan/Metal/DX12 e só depois GL.
    /// Iniciar todos os backends juntos carregava os drivers EGL e Vulkan e
    /// custava memória e tempo de abertura.
    async fn request_adapter(
        window: &Arc<Window>,
    ) -> Result<(Instance, Surface<'static>, Adapter)> {
        for backends in backend_order() {
            let instance = Instance::new(&InstanceDescriptor {
                backends,
                ..Default::default()
            });
            let surface = match instance.create_surface(window.clone()) {
                Ok(surface) => surface,
                Err(e) => {
                    tracing::debug!(?backends, error = %e, "Surface creation failed");
                    continue;
                }
            };
            let adapter = instance
                .request_adapter(&RequestAdapterOptions {
                    power_preference: PowerPreference::LowPower,
                    compatible_surface: Some(&surface),
                    force_fallback_adapter: false,
                })
                .await;
            if let Some(adapter) = adapter {
                tracing::info!(backend = ?adapter.get_info().backend, gpu = %adapter.get_info().name, "GPU adapter selected");
                return Ok((instance, surface, adapter));
            }
        }
        Err(anyhow::anyhow!("No suitable GPU adapter found"))
    }

    pub async fn new_with_config(
        window: Arc<Window>,
        font_config: &FontConfig,
        color_scheme: &ColorScheme,
    ) -> Result<Self> {
        let (gpu, surface) = match Self::reuse_shared(&window) {
            Some(reused) => reused,
            None => Self::start_gpu(&window).await?,
        };
        let SharedGpu {
            instance,
            adapter,
            device,
            queue,
        } = gpu;

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| anyhow::anyhow!("Surface has no supported formats"))?;

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: PresentMode::AutoVsync,
            alpha_mode: pick_alpha_mode(&caps.alpha_modes),
            view_formats: vec![],
            desired_maximum_frame_latency: frame_latency(),
        };
        surface.configure(&device, &config);

        Self::assemble(
            (instance, adapter, device, queue),
            Some(surface),
            config,
            window.scale_factor(),
            font_config,
            color_scheme,
        )
    }

    /// GPU já ligada por outra janela (ou pelo [`GpuState::prewarm`]), se ela
    /// servir para a superfície desta janela.
    fn reuse_shared(window: &Arc<Window>) -> Option<(SharedGpu, Surface<'static>)> {
        let gpu = shared_gpu()?;
        let surface = gpu.instance.create_surface(window.clone()).ok()?;
        if !gpu.adapter.is_surface_supported(&surface) {
            tracing::debug!("Shared GPU adapter does not support this surface");
            return None;
        }
        tracing::debug!("Reusing the shared GPU device");
        Some((gpu, surface))
    }

    /// Liga a GPU do zero para esta janela e a guarda para as próximas.
    async fn start_gpu(window: &Arc<Window>) -> Result<(SharedGpu, Surface<'static>)> {
        let started = std::time::Instant::now();
        let (instance, surface, adapter) = Self::request_adapter(window).await?;
        let adapter_ms = started.elapsed().as_millis() as u64;
        let (device, queue) = Self::request_device(&adapter).await?;
        tracing::debug!(
            adapter_ms,
            device_ms = started.elapsed().as_millis() as u64 - adapter_ms,
            "GPU device ready"
        );
        let gpu = SharedGpu {
            instance,
            adapter,
            device,
            queue,
        };
        remember_gpu(gpu.clone());
        Ok((gpu, surface))
    }

    /// Liga a GPU antes de existir janela (modo servidor): a primeira janela
    /// já encontra o device pronto.
    pub async fn prewarm() -> Result<()> {
        if shared_gpu().is_some() {
            return Ok(());
        }
        let started = std::time::Instant::now();
        let (instance, adapter) = Self::request_headless_adapter().await?;
        let (device, queue) = Self::request_device(&adapter).await?;
        remember_gpu(SharedGpu {
            instance,
            adapter,
            device,
            queue,
        });
        tracing::info!(
            gpu_ms = started.elapsed().as_millis() as u64,
            "GPU ready before any window"
        );
        Ok(())
    }

    /// GPU sem janela: desenha numa textura `width`x`height` que pode ser
    /// lida com [`GpuState::capture`]. Usado pelo `--snapshot` e por testes visuais.
    pub async fn new_headless(
        width: u32,
        height: u32,
        font_config: &FontConfig,
        color_scheme: &ColorScheme,
    ) -> Result<Self> {
        let (instance, adapter) = Self::request_headless_adapter().await?;
        let (device, queue) = Self::request_device(&adapter).await?;
        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            format: HEADLESS_FORMAT,
            width: width.max(1),
            height: height.max(1),
            present_mode: PresentMode::AutoVsync,
            alpha_mode: CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: frame_latency(),
        };
        let mut state = Self::assemble(
            (instance, adapter, device, queue),
            None,
            config,
            1.0,
            font_config,
            color_scheme,
        )?;
        state.offscreen = Some(state.create_offscreen_texture());
        Ok(state)
    }

    async fn request_headless_adapter() -> Result<(Instance, Adapter)> {
        for backends in backend_order() {
            let instance = Instance::new(&InstanceDescriptor {
                backends,
                ..Default::default()
            });
            let adapter = instance
                .request_adapter(&RequestAdapterOptions {
                    power_preference: PowerPreference::LowPower,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                })
                .await;
            if let Some(adapter) = adapter {
                tracing::info!(backend = ?adapter.get_info().backend, gpu = %adapter.get_info().name, "Headless GPU adapter selected");
                return Ok((instance, adapter));
            }
        }
        Err(anyhow::anyhow!("No suitable GPU adapter found"))
    }

    async fn request_device(adapter: &Adapter) -> Result<(Device, Queue)> {
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("Code Terminal Device"),
                    required_features: Features::empty(),
                    required_limits: Limits::downlevel_webgl2_defaults()
                        .using_resolution(adapter.limits()),
                    memory_hints: memory_hints(),
                },
                None,
            )
            .await?;
        Ok((device, queue))
    }

    fn assemble(
        (instance, adapter, device, queue): (Instance, Adapter, Device, Queue),
        surface: Option<Surface<'static>>,
        config: SurfaceConfiguration,
        scale_factor: f64,
        font_config: &FontConfig,
        color_scheme: &ColorScheme,
    ) -> Result<Self> {
        let started = std::time::Instant::now();
        let mut text_renderer = TextRenderer::new_with_color_scheme(
            &device,
            &queue,
            config.format,
            &scaled_font(font_config, scale_factor),
            color_scheme,
        )?;
        text_renderer.resize(config.width, config.height, &queue);

        let quad_renderer = QuadRenderer::new(&device, config.format);
        let overlay_quad_renderer = QuadRenderer::new(&device, config.format);
        let image_renderer =
            crate::renderer::image_quad::ImageQuadRenderer::new(&device, config.format);
        tracing::debug!(
            renderers_ms = started.elapsed().as_millis() as u64,
            "Text, quad and image renderers ready"
        );

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface,
            config,
            offscreen: None,
            text_renderer,
            quad_renderer,
            overlay_quad_renderer,
            image_renderer,
            pending_images: Vec::new(),
            scale_factor,
        })
    }

    fn create_offscreen_texture(&self) -> Texture {
        self.device.create_texture(&TextureDescriptor {
            label: Some("Offscreen Target"),
            size: Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: self.config.format,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            if let Some(surface) = &self.surface {
                surface.configure(&self.device, &self.config);
            }
            if self.offscreen.is_some() {
                self.offscreen = Some(self.create_offscreen_texture());
            }
            self.text_renderer
                .resize(new_size.width, new_size.height, &self.queue);
        }
    }

    /// Atualiza o fator de escala (troca de monitor/DPI) e refaz a fonte.
    pub fn set_scale_factor(&mut self, scale_factor: f64, font_config: &FontConfig) {
        self.scale_factor = scale_factor;
        self.update_font(font_config);
    }

    pub fn set_text(&mut self, text: &str) {
        self.text_renderer.set_text(text);
    }

    /// Define as linhas coloridas do grid.
    pub fn set_lines(&mut self, lines: &[Vec<crate::renderer::text::TextSpan>]) {
        self.text_renderer.set_lines(lines);
    }

    /// Imagens inline a desenhar no próximo frame.
    pub fn set_images(&mut self, images: Vec<crate::renderer::image_quad::ImageDraw>) {
        self.pending_images = images;
    }

    /// Começa a montar os grids dos painéis de um frame.
    pub fn begin_grids(&mut self) {
        self.text_renderer.begin_grids();
    }

    /// Linhas do painel `id`, desenhadas a partir de (x, y) em pixels.
    pub fn set_pane_lines(
        &mut self,
        id: u64,
        x: f32,
        y: f32,
        lines: &[Vec<crate::renderer::text::TextSpan>],
    ) {
        self.text_renderer.set_pane_lines(id, x, y, lines);
    }

    /// Descarta grids de painéis que não aparecem mais.
    pub fn end_grids(&mut self) {
        self.text_renderer.end_grids();
    }

    pub fn set_text_offset_y(&mut self, offset: f32) {
        self.text_renderer.set_offset_y(offset);
    }

    /// Define onde o grid começa na janela, em pixels.
    pub fn set_grid_origin(&mut self, x: f32, y: f32) {
        self.text_renderer.set_grid_origin(x, y);
    }

    pub fn clear_text_overlays(&mut self) {
        self.text_renderer.clear_overlays();
    }

    /// Quantos textos de UI já foram adicionados neste frame.
    pub fn text_overlay_count(&self) -> usize {
        self.text_renderer.overlay_count()
    }

    /// Recorta os textos de UI no intervalo `range` onde os painéis os cobrem.
    pub fn clip_text_overlays_under(
        &mut self,
        range: std::ops::Range<usize>,
        panels: &[glyphon::TextBounds],
    ) {
        self.text_renderer.clip_overlays_under(range, panels);
    }

    pub fn add_text_overlay(
        &mut self,
        x: f32,
        y: f32,
        text: impl Into<String>,
        color: glyphon::Color,
        size: f32,
    ) {
        self.text_renderer.add_overlay(x, y, text, color, size);
    }

    /// Atualiza a fonte (tamanho em pontos, convertido com o DPI da janela).
    pub fn update_font(&mut self, font_config: &FontConfig) {
        self.text_renderer
            .update_font(&scaled_font(font_config, self.scale_factor));
    }

    pub fn update_colors(&mut self, color_scheme: &ColorScheme) {
        self.text_renderer.update_colors(color_scheme);
    }

    /// Retorna o tamanho estimado de uma célula do terminal em pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        self.text_renderer.cell_size()
    }

    /// Desenha um frame com os quads de `overlays` por cima do texto.
    pub fn render(&mut self, overlays: Option<&QuadBatch>) -> Result<(), SurfaceError> {
        self.render_layers(None, overlays)
    }

    /// Desenha um frame em camadas: fundo, `under`, texto do grid, `over`,
    /// textos de UI.
    pub fn render_layers(
        &mut self,
        under: Option<&QuadBatch>,
        over: Option<&QuadBatch>,
    ) -> Result<(), SurfaceError> {
        let Some(surface) = &self.surface else {
            // Sem janela: desenha na textura (lida depois por `capture`).
            if let Some(view) = self.offscreen_view() {
                self.draw_frame(&view, under, over);
            }
            return Ok(());
        };
        let output = surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&TextureViewDescriptor::default());
        self.draw_frame(&view, under, over);
        output.present();
        Ok(())
    }

    fn offscreen_view(&self) -> Option<TextureView> {
        self.offscreen
            .as_ref()
            .map(|texture| texture.create_view(&TextureViewDescriptor::default()))
    }

    fn draw_frame(
        &mut self,
        view: &TextureView,
        under: Option<&QuadBatch>,
        over: Option<&QuadBatch>,
    ) {
        if let Err(e) = self.text_renderer.prepare(&self.device, &self.queue) {
            tracing::error!("Text prepare error: {:?}", e);
        }
        self.image_renderer.prepare(
            &self.device,
            &self.queue,
            &self.pending_images,
            (self.config.width, self.config.height),
        );

        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let background = clear_color(
                self.text_renderer.get_colors().background,
                self.config.format.is_srgb(),
            );
            let mut render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(background),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            let (width, height) = (self.config.width, self.config.height);
            if let Some(batch) = under {
                self.quad_renderer.render(
                    &self.device,
                    &self.queue,
                    &mut render_pass,
                    batch,
                    width,
                    height,
                );
            }
            self.image_renderer.render(&mut render_pass);
            if let Err(e) = self.text_renderer.render_grid(&mut render_pass) {
                tracing::error!("Text render error: {:?}", e);
            }
            if let Some(batch) = over {
                self.overlay_quad_renderer.render(
                    &self.device,
                    &self.queue,
                    &mut render_pass,
                    batch,
                    width,
                    height,
                );
            }
            if let Err(e) = self.text_renderer.render_overlays(&mut render_pass) {
                tracing::error!("Overlay text render error: {:?}", e);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        self.text_renderer.trim_atlas();
    }

    /// Desenha um frame na textura sem janela e devolve os pixels (sRGB).
    pub fn capture(
        &mut self,
        under: Option<&QuadBatch>,
        over: Option<&QuadBatch>,
    ) -> Result<image::RgbaImage> {
        let view = self
            .offscreen_view()
            .ok_or_else(|| anyhow::anyhow!("capture needs a headless GpuState"))?;
        self.draw_frame(&view, under, over);
        let texture = self
            .offscreen
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("capture needs a headless GpuState"))?;

        let (width, height) = (self.config.width, self.config.height);
        let padded_row = padded_bytes_per_row(width);
        let buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("Capture Buffer"),
            size: padded_row as u64 * height as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("Capture Encoder"),
            });
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(height),
                },
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(Maintain::Wait);
        receiver
            .recv()
            .map_err(|e| anyhow::anyhow!("capture map channel closed: {e}"))?
            .map_err(|e| anyhow::anyhow!("capture map failed: {e}"))?;

        let pixels = unpad_rows(&slice.get_mapped_range(), width, height, padded_row);
        buffer.unmap();
        image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("capture produced a buffer of the wrong size"))
    }
}

/// Formato do alvo sem janela: RGBA com codificação sRGB, igual ao PNG.
const HEADLESS_FORMAT: TextureFormat = TextureFormat::Rgba8UnormSrgb;

/// Linhas copiadas da GPU precisam de largura múltipla de 256 bytes.
fn padded_bytes_per_row(width: u32) -> u32 {
    let unpadded = width * 4;
    unpadded.div_ceil(COPY_BYTES_PER_ROW_ALIGNMENT) * COPY_BYTES_PER_ROW_ALIGNMENT
}

/// Remove o preenchimento do fim de cada linha.
fn unpad_rows(data: &[u8], width: u32, height: u32, padded_row: u32) -> Vec<u8> {
    let row = (width * 4) as usize;
    let mut pixels = Vec::with_capacity(row * height as usize);
    for chunk in data.chunks(padded_row as usize).take(height as usize) {
        pixels.extend_from_slice(&chunk[..row]);
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uma janela que abre depois do pedido cancela a soltura; sem janela
    /// nova, solta (e não há o que soltar duas vezes).
    #[test]
    fn test_release_is_cancelled_by_a_window_opening() {
        let ticket = gpu_idle_ticket();
        keep_shared_gpu();
        assert!(!release_shared_gpu_if_idle(
            ticket,
            std::time::Duration::ZERO
        ));
        let ticket = gpu_idle_ticket();
        // Nenhuma GPU ligada neste teste: nada para soltar, mas o pedido vale.
        release_shared_gpu_if_idle(ticket, std::time::Duration::ZERO);
        assert!(!release_shared_gpu());
    }

    /// Blocos pequenos de memória da GPU (#1): começam em 256 KB, não nos
    /// 8 MB do `MemoryUsage`, e podem crescer até 64 MB.
    #[test]
    fn test_gpu_memory_blocks_start_small() {
        match memory_hints() {
            MemoryHints::Manual {
                suballocated_device_memory_block_size: range,
            } => {
                assert_eq!(range.start, 256 * 1024);
                assert_eq!(range.end, 64 * 1024 * 1024);
            }
            other => panic!("unexpected hints: {other:?}"),
        }
    }

    #[test]
    fn test_backends_for() {
        assert_eq!(backends_for(None), vec![Backends::PRIMARY, Backends::GL]);
        assert_eq!(backends_for(Some("GL")), vec![Backends::GL]);
        assert_eq!(backends_for(Some("vulkan")), vec![Backends::VULKAN]);
        assert_eq!(backends_for(Some("nonsense")).len(), 2);
    }

    #[test]
    fn test_points_to_pixels() {
        assert_eq!(points_to_pixels(12.0, 1.0), 16.0);
        assert_eq!(points_to_pixels(12.0, 2.0), 32.0);
        assert_eq!(points_to_pixels(0.0, 1.0), 1.0);
    }

    /// Regressão: o fundo #1e1e1e ia sem conversão e aparecia cinza claro.
    #[test]
    fn test_clear_color_is_linear_on_srgb_surface() {
        let bg = glyphon::Color::rgb(0x1e, 0x1e, 0x1e);
        let linear = clear_color(bg, true);
        assert!(linear.r < 0.02, "{}", linear.r);
        assert_eq!(linear.a, 1.0);
        let plain = clear_color(bg, false);
        assert!((plain.r - 30.0 / 255.0).abs() < 1e-6);
    }

    #[test]
    fn test_padded_bytes_per_row_and_unpad() {
        assert_eq!(padded_bytes_per_row(64), 256);
        assert_eq!(padded_bytes_per_row(65), 512);
        let padded = 256;
        let mut data = vec![0u8; padded * 2];
        data[0] = 1;
        data[padded] = 2;
        let pixels = unpad_rows(&data, 3, 2, padded as u32);
        assert_eq!(pixels.len(), 3 * 4 * 2);
        assert_eq!(pixels[0], 1);
        assert_eq!(pixels[12], 2);
    }

    #[test]
    fn test_pick_alpha_mode_prefers_opaque() {
        assert_eq!(
            pick_alpha_mode(&[
                CompositeAlphaMode::PreMultiplied,
                CompositeAlphaMode::Opaque
            ]),
            CompositeAlphaMode::Opaque
        );
        assert_eq!(
            pick_alpha_mode(&[CompositeAlphaMode::PreMultiplied]),
            CompositeAlphaMode::PreMultiplied
        );
        assert_eq!(pick_alpha_mode(&[]), CompositeAlphaMode::Auto);
    }
}
