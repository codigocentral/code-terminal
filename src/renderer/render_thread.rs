//! Thread dedicada para renderização GPU
//!
//! Executa renderização em thread separada para garantir 60 FPS constante,
//! mesmo quando o main thread está ocupado com I/O ou parsing.

use crate::config::{ColorScheme, FontConfig};
use crate::renderer::gpu::GpuState;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, trace, warn};
use winit::window::Window;

/// Dados para renderização (enviados da main thread)
#[derive(Debug, Clone)]
pub struct RenderData {
    /// Texto a ser renderizado
    pub text: String,
    /// Indica se os dados foram atualizados
    pub dirty: bool,
}

impl RenderData {
    pub fn new(text: String) -> Self {
        Self { text, dirty: true }
    }

    pub fn empty() -> Self {
        Self {
            text: String::new(),
            dirty: false,
        }
    }
}

/// Comandos enviados para a Render Thread
#[derive(Debug)]
pub enum RenderCommand {
    /// Atualiza dados para renderizar
    UpdateData(RenderData),
    /// Atualiza configuração de fonte
    UpdateFont(FontConfig),
    /// Atualiza esquema de cores
    UpdateColors(ColorScheme),
    /// Redimensiona surface
    Resize(u32, u32),
    /// Sinaliza para a thread parar
    Shutdown,
}

/// Estatísticas de renderização
#[derive(Debug, Default)]
pub struct RenderStats {
    /// Frames renderizados
    pub frames_rendered: u64,
    /// Frames perdidos (target missed)
    pub frames_missed: u64,
    /// Tempo médio de frame (ms)
    pub avg_frame_time_ms: f64,
    /// Frame time acumulado para média
    frame_time_accumulator: Duration,
    /// Contador para média móvel
    frame_time_samples: u32,
}

impl RenderStats {
    pub fn record_frame(&mut self, frame_time: Duration) {
        self.frames_rendered += 1;
        self.frame_time_accumulator += frame_time;
        self.frame_time_samples += 1;

        // Atualiza média a cada 60 frames
        if self.frame_time_samples >= 60 {
            self.avg_frame_time_ms =
                self.frame_time_accumulator.as_secs_f64() * 1000.0 / self.frame_time_samples as f64;
            self.frame_time_accumulator = Duration::ZERO;
            self.frame_time_samples = 0;
        }

        // Detecta frame drop (> 20ms = perdeu 60fps)
        if frame_time > Duration::from_millis(20) {
            self.frames_missed += 1;
        }
    }

    pub fn fps(&self) -> f64 {
        if self.avg_frame_time_ms > 0.0 {
            1000.0 / self.avg_frame_time_ms
        } else {
            0.0
        }
    }
}

/// Handle para comunicação com a Render Thread
pub struct RenderThreadHandle {
    sender: Sender<RenderCommand>,
    thread: Option<JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl RenderThreadHandle {
    /// Envia dados para renderização
    pub fn update_data(&self, text: String) -> Result<(), mpsc::SendError<RenderCommand>> {
        self.sender
            .send(RenderCommand::UpdateData(RenderData::new(text)))
    }

    /// Atualiza configuração de fonte
    pub fn update_font(&self, config: FontConfig) -> Result<(), mpsc::SendError<RenderCommand>> {
        self.sender.send(RenderCommand::UpdateFont(config))
    }

    /// Atualiza esquema de cores
    pub fn update_colors(&self, scheme: ColorScheme) -> Result<(), mpsc::SendError<RenderCommand>> {
        self.sender.send(RenderCommand::UpdateColors(scheme))
    }

    /// Redimensiona surface
    pub fn resize(&self, width: u32, height: u32) -> Result<(), mpsc::SendError<RenderCommand>> {
        self.sender.send(RenderCommand::Resize(width, height))
    }

    /// Sinaliza shutdown
    pub fn shutdown(&self) {
        if let Err(error) = self.sender.send(RenderCommand::Shutdown) {
            tracing::debug!(error = %error, "Render thread already stopped before shutdown signal");
        }
    }

    /// Aguarda a thread finalizar
    pub fn join(&mut self) {
        if let Some(handle) = self.thread.take() {
            if handle.join().is_err() {
                tracing::warn!("Render thread panicked during join");
            }
        }
    }

    /// Verifica se a thread está ativa
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

impl Drop for RenderThreadHandle {
    fn drop(&mut self) {
        self.shutdown();
        self.join();
    }
}

/// Thread de renderização GPU
pub struct RenderThread;

impl RenderThread {
    /// Inicia a Render Thread
    ///
    /// # Arguments
    /// * `window` - Janela winit (Arc)
    /// * `font_config` - Configuração inicial de fonte
    /// * `color_scheme` - Esquema inicial de cores
    ///
    /// # Returns
    /// Handle para comunicação com a thread
    pub fn spawn(
        window: Arc<Window>,
        font_config: FontConfig,
        color_scheme: ColorScheme,
    ) -> Result<RenderThreadHandle> {
        let (sender, receiver) = mpsc::channel::<RenderCommand>();
        let running = Arc::new(AtomicBool::new(false));
        let running_for_thread = Arc::clone(&running);

        let thread = thread::Builder::new()
            .name("render".into())
            .spawn(move || {
                running_for_thread.store(true, Ordering::Release);

                let mut worker = match pollster::block_on(RenderWorker::new(
                    window,
                    font_config,
                    color_scheme,
                )) {
                    Ok(w) => w,
                    Err(e) => {
                        error!(error = %e, "Failed to create render worker");
                        running_for_thread.store(false, Ordering::Release);
                        return;
                    }
                };
                worker.run(receiver);
                running_for_thread.store(false, Ordering::Release);
            })?;

        Ok(RenderThreadHandle {
            sender,
            thread: Some(thread),
            running,
        })
    }
}

/// Worker interno que executa na thread de renderização
struct RenderWorker {
    gpu_state: GpuState,
    current_data: RenderData,
    stats: RenderStats,
    target_frame_time: Duration,
}

impl RenderWorker {
    /// Target FPS (60 Hz)
    const TARGET_FPS: u32 = 60;

    async fn new(
        window: Arc<Window>,
        font_config: FontConfig,
        color_scheme: ColorScheme,
    ) -> Result<Self> {
        let gpu_state = GpuState::new_with_config(window, &font_config, &color_scheme).await?;

        Ok(Self {
            gpu_state,
            current_data: RenderData::empty(),
            stats: RenderStats::default(),
            target_frame_time: Duration::from_secs_f64(1.0 / Self::TARGET_FPS as f64),
        })
    }

    /// Loop principal de renderização
    fn run(&mut self, receiver: Receiver<RenderCommand>) {
        info!(target_fps = Self::TARGET_FPS, "Render thread started");

        let mut last_frame_time = Instant::now();

        loop {
            let frame_start = Instant::now();

            // Processa comandos pendentes (non-blocking)
            if self.process_commands(&receiver).is_err() {
                // Channel closed, shutdown
                break;
            }

            // Renderiza frame
            if let Err(e) = self.render_frame() {
                match e {
                    wgpu::SurfaceError::Lost => {
                        warn!("Surface lost, attempting recovery");
                        // Recovery would need window size
                    }
                    wgpu::SurfaceError::OutOfMemory => {
                        error!("Out of memory in render thread");
                        break;
                    }
                    _ => {
                        warn!(error = ?e, "Render error");
                    }
                }
            }

            // Calcula tempo do frame
            let frame_time = frame_start.elapsed();
            self.stats.record_frame(frame_time);

            // Log de performance a cada 5 segundos
            let elapsed = last_frame_time.elapsed();
            if elapsed >= Duration::from_secs(5) {
                trace!(
                    frames = self.stats.frames_rendered,
                    missed = self.stats.frames_missed,
                    avg_ms = self.stats.avg_frame_time_ms,
                    fps = self.stats.fps(),
                    "Render stats"
                );
                last_frame_time = Instant::now();
            }

            // Throttling para manter 60 FPS
            if frame_time < self.target_frame_time {
                let sleep_time = self.target_frame_time - frame_time;
                spin_sleep::sleep(sleep_time);
            }
        }

        info!(stats = ?self.stats, "Render thread stopping");
    }

    /// Processa comandos recebidos (retorna Err se channel fechado)
    fn process_commands(&mut self, receiver: &Receiver<RenderCommand>) -> Result<(), ()> {
        // Processa todos os comandos pendentes (non-blocking)
        loop {
            match receiver.try_recv() {
                Ok(cmd) => {
                    if !self.handle_command(cmd) {
                        return Err(());
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Err(()),
            }
        }
        Ok(())
    }

    /// Handle para comandos individuais
    fn handle_command(&mut self, cmd: RenderCommand) -> bool {
        match cmd {
            RenderCommand::UpdateData(data) => {
                if data.dirty {
                    self.current_data = data;
                    self.gpu_state.set_text(&self.current_data.text);
                }
                true
            }
            RenderCommand::UpdateFont(config) => {
                self.gpu_state.update_font(&config);
                true
            }
            RenderCommand::UpdateColors(scheme) => {
                self.gpu_state.update_colors(&scheme);
                true
            }
            RenderCommand::Resize(width, height) => {
                self.gpu_state
                    .resize(winit::dpi::PhysicalSize::new(width, height));
                true
            }
            RenderCommand::Shutdown => {
                debug!("Render thread received shutdown");
                false
            }
        }
    }

    /// Renderiza um frame
    fn render_frame(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.gpu_state.render(None)
    }
}

/// Wrapper thread-safe para renderização
pub struct ThreadSafeRenderer {
    handle: RenderThreadHandle,
}

impl ThreadSafeRenderer {
    /// Cria um novo ThreadSafeRenderer
    pub fn new(
        window: Arc<Window>,
        font_config: FontConfig,
        color_scheme: ColorScheme,
    ) -> Result<Self> {
        let handle = RenderThread::spawn(window, font_config, color_scheme)?;
        Ok(Self { handle })
    }

    /// Atualiza texto para renderizar
    pub fn update_text(&self, text: String) -> Result<(), mpsc::SendError<RenderCommand>> {
        self.handle.update_data(text)
    }

    /// Atualiza fonte
    pub fn update_font(&self, config: FontConfig) -> Result<(), mpsc::SendError<RenderCommand>> {
        self.handle.update_font(config)
    }

    /// Atualiza cores
    pub fn update_colors(&self, scheme: ColorScheme) -> Result<(), mpsc::SendError<RenderCommand>> {
        self.handle.update_colors(scheme)
    }

    /// Redimensiona
    pub fn resize(&self, width: u32, height: u32) -> Result<(), mpsc::SendError<RenderCommand>> {
        self.handle.resize(width, height)
    }

    /// Verifica se a thread está ativa
    pub fn is_running(&self) -> bool {
        self.handle.is_running()
    }
}

impl Drop for ThreadSafeRenderer {
    fn drop(&mut self) {
        debug!("ThreadSafeRenderer dropping");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_data_creation() {
        let data = RenderData::new("Hello".to_string());
        assert_eq!(data.text, "Hello");
        assert!(data.dirty);
    }

    #[test]
    fn test_render_data_empty() {
        let data = RenderData::empty();
        assert!(data.text.is_empty());
        assert!(!data.dirty);
    }

    #[test]
    fn test_render_stats_record() {
        let mut stats = RenderStats::default();

        // Simula 60 frames
        for _ in 0..60 {
            stats.record_frame(Duration::from_millis(16));
        }

        assert_eq!(stats.frames_rendered, 60);
        assert!(stats.avg_frame_time_ms > 0.0);
    }

    #[test]
    fn test_render_stats_frame_drops() {
        let mut stats = RenderStats::default();

        // Frame rápido
        stats.record_frame(Duration::from_millis(10));
        assert_eq!(stats.frames_missed, 0);

        // Frame lento (drop)
        stats.record_frame(Duration::from_millis(25));
        assert_eq!(stats.frames_missed, 1);
    }

    #[test]
    fn test_render_stats_fps() {
        let mut stats = RenderStats::default();

        // Simula 60 frames de 16ms
        for _ in 0..60 {
            stats.record_frame(Duration::from_millis(16));
        }

        // FPS deve ser próximo de 60
        let fps = stats.fps();
        assert!(
            fps > 50.0 && fps < 70.0,
            "FPS should be around 60, got {}",
            fps
        );
    }
}
