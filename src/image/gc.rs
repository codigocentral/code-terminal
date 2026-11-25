//! Image Garbage Collection
//!
//! Sistema de coleta de lixo para imagens que roda em background
//! e gerencia automaticamente o uso de memória do cache de imagens.
//!
//! # Características
//!
//! - Execução em background via tokio
//! - Políticas de evicção configuráveis (LRU, LFU, TTL, híbridas)
//! - Monitoramento de memória com thresholds
//! - Thread-safe via Arc<Mutex<>>
//! - Métricas e estatísticas
//!
//! # Uso
//!
//! ```rust,ignore
//! use codeterm::image::{ImageCache, ImageGarbageCollector, GcConfig};
//!
//! let cache = Arc::new(Mutex::new(ImageCache::new()));
//! let config = GcConfig::default();
//!
//! let gc = ImageGarbageCollector::new(cache.clone(), config);
//! gc.start()?; // Inicia em background
//!
//! // ... usar o cache ...
//!
//! gc.stop()?; // Para o GC
//! ```

use super::{ImageCache, ImageId};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::time::interval;
use tracing::{debug, error, info};

/// Configuração do Garbage Collector
#[derive(Debug, Clone)]
pub struct GcConfig {
    /// Intervalo entre execuções do GC
    pub interval: Duration,
    /// Idade máxima das imagens antes de serem removidas (TTL)
    pub max_age: Duration,
    /// Threshold de uso de memória para disparar GC (0.0-1.0)
    pub memory_threshold: f32,
    /// Threshold de contagem de imagens para disparar GC (0.0-1.0)
    pub count_threshold: f32,
    /// Política de evicção
    pub eviction_policy: GcEvictionPolicy,
    /// Número máximo de imagens para remover por ciclo
    pub max_evict_per_cycle: usize,
    /// Habilitar logging detalhado
    pub verbose: bool,
}

impl Default for GcConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(30),
            max_age: Duration::from_secs(300), // 5 minutos
            memory_threshold: 0.8,             // 80% da memória
            count_threshold: 0.9,              // 90% do limite de imagens
            eviction_policy: GcEvictionPolicy::default(),
            max_evict_per_cycle: 10,
            verbose: false,
        }
    }
}

impl GcConfig {
    /// Configuração agressiva (GC mais frequente)
    pub fn aggressive() -> Self {
        Self {
            interval: Duration::from_secs(10),
            max_age: Duration::from_secs(60),
            memory_threshold: 0.6,
            count_threshold: 0.7,
            eviction_policy: GcEvictionPolicy::Hybrid {
                lru_weight: 0.5,
                age_weight: 0.5,
            },
            max_evict_per_cycle: 20,
            verbose: false,
        }
    }

    /// Configuração conservadora (GC menos frequente)
    pub fn conservative() -> Self {
        Self {
            interval: Duration::from_secs(60),
            max_age: Duration::from_secs(600), // 10 minutos
            memory_threshold: 0.9,
            count_threshold: 0.95,
            eviction_policy: GcEvictionPolicy::Lru,
            max_evict_per_cycle: 5,
            verbose: false,
        }
    }

    /// Configuração para desenvolvimento (verbose)
    pub fn debug() -> Self {
        Self {
            verbose: true,
            ..Self::default()
        }
    }
}

/// Política de evicção para o GC
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum GcEvictionPolicy {
    /// Remove imagens menos recentemente usadas
    #[default]
    Lru,
    /// Remove imagens menos frequentemente usadas
    Lfu,
    /// Remove imagens mais antigas (baseado em creation time)
    Oldest,
    /// Remove imagens que excederam TTL
    Ttl,
    /// Política híbrida combinando múltiplos fatores
    Hybrid {
        /// Peso do LRU (0.0-1.0)
        lru_weight: f32,
        /// Peso da idade (0.0-1.0)
        age_weight: f32,
    },
}

/// Comandos para o GC
#[derive(Debug)]
enum GcCommand {
    /// Executar GC imediatamente
    ForceCollect,
    /// Atualizar configuração
    UpdateConfig(GcConfig),
    /// Obter estatísticas
    GetStats(tokio::sync::oneshot::Sender<GcStats>),
    /// Parar o GC
    Stop,
}

/// Estatísticas do Garbage Collector
#[derive(Debug, Clone, Copy, Default)]
pub struct GcStats {
    /// Número de ciclos de GC executados
    pub cycles_run: u64,
    /// Total de imagens removidas
    pub images_evicted: u64,
    /// Total de bytes liberados
    pub bytes_freed: u64,
    /// Tempo médio de execução por ciclo (ms)
    pub avg_cycle_time_ms: f64,
    /// Última execução
    pub last_run: Option<Instant>,
    /// Erros encontrados
    pub errors: u64,
}

/// Garbage Collector para imagens
pub struct ImageGarbageCollector {
    cache: Arc<Mutex<ImageCache>>,
    config: Arc<Mutex<GcConfig>>,
    stats: Arc<Mutex<GcStats>>,
    command_tx: mpsc::UnboundedSender<GcCommand>,
    command_rx: Option<mpsc::UnboundedReceiver<GcCommand>>,
    running: Arc<Mutex<bool>>,
}

impl ImageGarbageCollector {
    /// Criar um novo Garbage Collector
    pub fn new(cache: Arc<Mutex<ImageCache>>, config: GcConfig) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        Self {
            cache,
            config: Arc::new(Mutex::new(config)),
            stats: Arc::new(Mutex::new(GcStats::default())),
            command_tx,
            command_rx: Some(command_rx),
            running: Arc::new(Mutex::new(false)),
        }
    }

    /// Iniciar o GC em background
    pub fn start(&mut self) -> Result<(), GcError> {
        let mut running = self.running.lock();
        if *running {
            return Err(GcError::AlreadyRunning);
        }
        // O receptor só existe uma vez: sem ele o GC não pode ser reiniciado.
        // Pegar antes de marcar `running` evita ficar "rodando" sem loop.
        let command_rx = self.command_rx.take().ok_or(GcError::AlreadyRunning)?;
        *running = true;
        drop(running);

        let cache = self.cache.clone();
        let config = self.config.clone();
        let stats = self.stats.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            run_gc_loop(cache, config, stats, running, command_rx).await;
        });

        info!("Image garbage collector started");
        Ok(())
    }

    /// Parar o GC
    pub fn stop(&self) -> Result<(), GcError> {
        let mut running = self.running.lock();
        if !*running {
            return Err(GcError::NotRunning);
        }
        *running = false;
        drop(running);

        let _ = self.command_tx.send(GcCommand::Stop);
        info!("Image garbage collector stopped");
        Ok(())
    }

    /// Forçar execução do GC
    pub fn force_collect(&self) -> Result<(), GcError> {
        if !*self.running.lock() {
            return Err(GcError::NotRunning);
        }

        self.command_tx
            .send(GcCommand::ForceCollect)
            .map_err(|_| GcError::ChannelError)
    }

    /// Atualizar configuração
    pub fn update_config(&self, config: GcConfig) -> Result<(), GcError> {
        if !*self.running.lock() {
            return Err(GcError::NotRunning);
        }

        self.command_tx
            .send(GcCommand::UpdateConfig(config))
            .map_err(|_| GcError::ChannelError)
    }

    /// Obter estatísticas
    pub fn stats(&self) -> GcStats {
        *self.stats.lock()
    }

    /// Obter estatísticas pelo loop do GC.
    pub async fn request_stats(&self) -> Result<GcStats, GcError> {
        if !*self.running.lock() {
            return Err(GcError::NotRunning);
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.command_tx
            .send(GcCommand::GetStats(tx))
            .map_err(|_| GcError::ChannelError)?;
        rx.await.map_err(|_| GcError::ChannelError)
    }

    /// Verificar se está rodando
    pub fn is_running(&self) -> bool {
        *self.running.lock()
    }
}

impl Drop for ImageGarbageCollector {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Loop principal do GC
async fn run_gc_loop(
    cache: Arc<Mutex<ImageCache>>,
    config: Arc<Mutex<GcConfig>>,
    stats: Arc<Mutex<GcStats>>,
    running: Arc<Mutex<bool>>,
    mut command_rx: mpsc::UnboundedReceiver<GcCommand>,
) {
    let interval_duration = config.lock().interval;
    let mut interval_timer = interval(interval_duration);

    loop {
        tokio::select! {
            _ = interval_timer.tick() => {
                if !*running.lock() {
                    break;
                }

                let cfg = config.lock().clone();
                if let Err(e) = run_gc_cycle(&cache, &cfg, &stats) {
                    error!("GC cycle failed: {}", e);
                    stats.lock().errors += 1;
                }
            }

            Some(cmd) = command_rx.recv() => {
                match cmd {
                    GcCommand::Stop => {
                        info!("GC received stop command");
                        break;
                    }
                    GcCommand::ForceCollect => {
                        debug!("GC force collect requested");
                        let cfg = config.lock().clone();
                        if let Err(e) = run_gc_cycle(&cache, &cfg, &stats) {
                            error!("Forced GC cycle failed: {}", e);
                            stats.lock().errors += 1;
                        }
                    }
                    GcCommand::UpdateConfig(new_config) => {
                        info!("GC config updated");
                        *config.lock() = new_config.clone();
                        interval_timer = interval(new_config.interval);
                    }
                    GcCommand::GetStats(reply) => {
                        let current_stats = *stats.lock();
                        let _ = reply.send(current_stats);
                    }
                }
            }
        }
    }

    info!("GC loop terminated");
}

/// Executar um ciclo de GC
fn run_gc_cycle(
    cache: &Arc<Mutex<ImageCache>>,
    config: &GcConfig,
    stats: &Arc<Mutex<GcStats>>,
) -> Result<(), GcError> {
    let start = Instant::now();

    let (needs_gc, eviction_count) = {
        let cache_guard = cache.lock();
        let cache_stats = cache_guard.stats();

        let memory_pressure = cache_stats.memory_usage_percent() / 100.0;
        let count_pressure = cache_stats.image_count_percent() / 100.0;

        let needs_gc =
            memory_pressure >= config.memory_threshold || count_pressure >= config.count_threshold;

        let eviction_count = if needs_gc {
            calculate_eviction_count(&cache_stats, config)
        } else {
            0
        };

        (needs_gc, eviction_count)
    };

    if !needs_gc {
        if config.verbose {
            debug!("GC cycle skipped - no pressure detected");
        }
        return Ok(());
    }

    if config.verbose {
        debug!("GC cycle starting - evicting {} images", eviction_count);
    }

    let evicted = perform_eviction(cache, config, eviction_count)?;

    let elapsed = start.elapsed();
    update_stats(stats, evicted.images, evicted.bytes, elapsed);

    if config.verbose {
        info!(
            "GC cycle completed: {} images evicted, {} bytes freed in {:?}",
            evicted.images, evicted.bytes, elapsed
        );
    }

    Ok(())
}

/// Resultado de evicção
struct EvictionResult {
    images: u64,
    bytes: u64,
}

/// Calcular quantas imagens remover
fn calculate_eviction_count(cache_stats: &super::cache::CacheStats, config: &GcConfig) -> usize {
    let memory_pressure = cache_stats.memory_usage_percent() / 100.0;
    let count_pressure = cache_stats.image_count_percent() / 100.0;

    let pressure = memory_pressure.max(count_pressure);

    let base_count = if pressure > 0.9 {
        (cache_stats.image_count as f32 * 0.3) as usize
    } else if pressure > 0.8 {
        (cache_stats.image_count as f32 * 0.2) as usize
    } else {
        (cache_stats.image_count as f32 * 0.1) as usize
    };

    base_count.min(config.max_evict_per_cycle).max(1)
}

/// Realizar evicção baseada na política
fn perform_eviction(
    cache: &Arc<Mutex<ImageCache>>,
    config: &GcConfig,
    count: usize,
) -> Result<EvictionResult, GcError> {
    let ids_to_evict = select_images_for_eviction(cache, &config.eviction_policy, count)?;

    let mut evicted_images = 0u64;
    let mut evicted_bytes = 0u64;

    {
        let cache_guard = cache.lock();
        for id in ids_to_evict {
            if let Some(image) = cache_guard.remove(id) {
                evicted_images += 1;
                evicted_bytes += image.size_bytes() as u64;
            }
        }
    }

    Ok(EvictionResult {
        images: evicted_images,
        bytes: evicted_bytes,
    })
}

/// Selecionar imagens para evicção baseado na política
fn select_images_for_eviction(
    cache: &Arc<Mutex<ImageCache>>,
    policy: &GcEvictionPolicy,
    count: usize,
) -> Result<Vec<ImageId>, GcError> {
    // Lê os tempos sem `get()`: get() registra acesso e zerava o LRU de todas
    // as imagens antes de pontuá-las, tornando a escolha arbitrária.
    let entries = cache.lock().access_times();
    if entries.is_empty() {
        return Ok(Vec::new());
    }

    let mut scored: Vec<(ImageId, f64)> = entries
        .iter()
        .map(|(id, last_accessed, created_at)| {
            let score = match policy {
                GcEvictionPolicy::Lru => last_accessed.elapsed().as_secs_f64(),
                GcEvictionPolicy::Oldest => created_at.elapsed().as_secs_f64(),
                GcEvictionPolicy::Ttl => {
                    let elapsed = last_accessed.elapsed().as_secs_f64();
                    if elapsed > 300.0 {
                        1000.0
                    } else {
                        0.0
                    }
                }
                GcEvictionPolicy::Lfu => 0.0,
                GcEvictionPolicy::Hybrid {
                    lru_weight,
                    age_weight,
                } => {
                    let lru_score = last_accessed.elapsed().as_secs_f64();
                    let age_score = created_at.elapsed().as_secs_f64();
                    (*lru_weight as f64 * lru_score) + (*age_weight as f64 * age_score)
                }
            };
            (*id, score)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let to_evict: Vec<ImageId> = scored.into_iter().take(count).map(|(id, _)| id).collect();

    Ok(to_evict)
}

/// Atualizar estatísticas
fn update_stats(stats: &Arc<Mutex<GcStats>>, evicted: u64, bytes_freed: u64, elapsed: Duration) {
    let mut stats_guard = stats.lock();

    let total_time = stats_guard.avg_cycle_time_ms * stats_guard.cycles_run as f64
        + elapsed.as_secs_f64() * 1000.0;
    stats_guard.cycles_run += 1;
    stats_guard.avg_cycle_time_ms = total_time / stats_guard.cycles_run as f64;
    stats_guard.images_evicted += evicted;
    stats_guard.bytes_freed += bytes_freed;
    stats_guard.last_run = Some(Instant::now());
}

/// Erros do GC
#[derive(Debug, thiserror::Error)]
pub enum GcError {
    #[error("GC already running")]
    AlreadyRunning,

    #[error("GC not running")]
    NotRunning,

    #[error("Channel error")]
    ChannelError,

    #[error("Lock error")]
    LockError,
}

/// Builder para configuração do GC
#[derive(Debug, Default)]
pub struct GcConfigBuilder {
    config: GcConfig,
}

impl GcConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn interval(mut self, duration: Duration) -> Self {
        self.config.interval = duration;
        self
    }

    pub fn max_age(mut self, duration: Duration) -> Self {
        self.config.max_age = duration;
        self
    }

    pub fn memory_threshold(mut self, threshold: f32) -> Self {
        self.config.memory_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn count_threshold(mut self, threshold: f32) -> Self {
        self.config.count_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    pub fn eviction_policy(mut self, policy: GcEvictionPolicy) -> Self {
        self.config.eviction_policy = policy;
        self
    }

    pub fn max_evict_per_cycle(mut self, count: usize) -> Self {
        self.config.max_evict_per_cycle = count;
        self
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.config.verbose = verbose;
        self
    }

    pub fn build(self) -> GcConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::Image;
    use tokio::time::sleep;

    fn create_test_cache() -> Arc<Mutex<ImageCache>> {
        Arc::new(Mutex::new(ImageCache::with_limits(10, 10000)))
    }

    #[test]
    fn test_gc_config_default() {
        let config = GcConfig::default();
        assert_eq!(config.interval, Duration::from_secs(30));
        assert_eq!(config.max_age, Duration::from_secs(300));
        assert!((config.memory_threshold - 0.8).abs() < 0.01);
    }

    #[test]
    fn test_gc_config_presets() {
        let aggressive = GcConfig::aggressive();
        assert!(aggressive.interval < GcConfig::default().interval);

        let conservative = GcConfig::conservative();
        assert!(conservative.interval > GcConfig::default().interval);
    }

    #[test]
    fn test_gc_config_builder() {
        let config = GcConfigBuilder::new()
            .interval(Duration::from_secs(60))
            .max_age(Duration::from_secs(120))
            .memory_threshold(0.5)
            .verbose(true)
            .build();

        assert_eq!(config.interval, Duration::from_secs(60));
        assert_eq!(config.max_age, Duration::from_secs(120));
        assert!((config.memory_threshold - 0.5).abs() < 0.01);
        assert!(config.verbose);
    }

    #[test]
    fn test_gc_creation() {
        let cache = create_test_cache();
        let config = GcConfig::default();

        let gc = ImageGarbageCollector::new(cache, config);
        assert!(!gc.is_running());
    }

    #[test]
    fn test_gc_stats_default() {
        let stats = GcStats::default();
        assert_eq!(stats.cycles_run, 0);
        assert_eq!(stats.images_evicted, 0);
        assert_eq!(stats.bytes_freed, 0);
    }

    #[test]
    fn test_eviction_policy_default() {
        let policy = GcEvictionPolicy::default();
        assert_eq!(policy, GcEvictionPolicy::Lru);
    }

    #[test]
    fn test_gc_error_display() {
        let err = GcError::AlreadyRunning;
        assert_eq!(format!("{}", err), "GC already running");

        let err = GcError::NotRunning;
        assert_eq!(format!("{}", err), "GC not running");
    }

    #[tokio::test]
    async fn test_gc_start_stop() {
        let cache = create_test_cache();
        let config = GcConfig::default();

        let mut gc = ImageGarbageCollector::new(cache, config);
        assert!(!gc.is_running());

        gc.start().unwrap();
        assert!(gc.is_running());

        sleep(Duration::from_millis(100)).await;

        gc.stop().unwrap();

        sleep(Duration::from_millis(50)).await;
        assert!(!gc.is_running());
    }

    #[tokio::test]
    async fn test_gc_double_start() {
        let cache = create_test_cache();
        let config = GcConfig::default();

        let mut gc = ImageGarbageCollector::new(cache, config);
        gc.start().unwrap();

        let result = gc.start();
        assert!(matches!(result, Err(GcError::AlreadyRunning)));

        gc.stop().unwrap();
    }

    #[tokio::test]
    async fn test_gc_stop_without_start() {
        let cache = create_test_cache();
        let config = GcConfig::default();

        let gc = ImageGarbageCollector::new(cache, config);
        let result = gc.stop();

        assert!(matches!(result, Err(GcError::NotRunning)));
    }

    #[tokio::test]
    async fn test_gc_force_collect() {
        let cache = create_test_cache();

        // Adiciona algumas imagens para criar pressão na cache
        {
            let cache_guard = cache.lock();
            for i in 1..=5 {
                let image = Image::new(
                    ImageId(i),
                    10,
                    10,
                    crate::image::ImageFormat::Png,
                    vec![0u8; 1000], // 1KB cada
                );
                cache_guard.store(image);
            }
        }

        let config = GcConfig::debug();

        let mut gc = ImageGarbageCollector::new(cache, config);
        gc.start().unwrap();

        sleep(Duration::from_millis(50)).await;

        // Force collect deve rodar mesmo sem pressão de memória
        // porque o comando ForceCollect sempre executa run_gc_cycle
        gc.force_collect().unwrap();

        sleep(Duration::from_millis(100)).await;

        let stats = gc.stats();
        // O ciclo pode ou não ter rodado dependendo se há pressão
        // O importante é que o GC está funcionando sem panics
        assert!(stats.errors == 0, "GC should not have errors");

        gc.stop().unwrap();
    }

    fn image(id: u32, size: usize) -> Image {
        Image::new(
            ImageId(id),
            10,
            10,
            crate::image::ImageFormat::Png,
            vec![0u8; size],
        )
    }

    fn filled_cache(count: u32) -> Arc<Mutex<ImageCache>> {
        let cache = create_test_cache();
        for i in 1..=count {
            cache.lock().store(image(i, 1000));
        }
        cache
    }

    #[test]
    fn test_cycle_skips_without_pressure() {
        let cache = filled_cache(2);
        let stats = Arc::new(Mutex::new(GcStats::default()));
        run_gc_cycle(&cache, &GcConfig::debug(), &stats).unwrap();
        assert_eq!(cache.lock().len(), 2);
        assert_eq!(stats.lock().cycles_run, 0);
    }

    #[test]
    fn test_cycle_evicts_under_pressure_and_updates_stats() {
        // 10/10 imagens: pressão de contagem 100% -> remove 30% (3)
        let cache = filled_cache(10);
        let stats = Arc::new(Mutex::new(GcStats::default()));
        run_gc_cycle(&cache, &GcConfig::debug(), &stats).unwrap();

        assert_eq!(cache.lock().len(), 7);
        let s = *stats.lock();
        assert_eq!(s.cycles_run, 1);
        assert_eq!(s.images_evicted, 3);
        assert_eq!(s.bytes_freed, 3000);
        assert!(s.last_run.is_some());

        // Limite por ciclo
        let cache = filled_cache(10);
        let config = GcConfigBuilder::new().max_evict_per_cycle(1).build();
        run_gc_cycle(&cache, &config, &stats).unwrap();
        assert_eq!(cache.lock().len(), 9);
        assert_eq!(stats.lock().cycles_run, 2);
    }

    /// Regressão: a seleção usava `get()`, que registra acesso e zerava o LRU
    /// de todas as imagens antes da pontuação.
    #[test]
    fn test_lru_eviction_picks_least_recently_used() {
        let cache = filled_cache(3);
        std::thread::sleep(Duration::from_millis(30));
        // 2 e 3 foram usadas recentemente; 1 é a menos recente
        cache.lock().get(ImageId(2));
        cache.lock().get(ImageId(3));
        let hits_before = cache.lock().metrics().hits;

        let ids = select_images_for_eviction(&cache, &GcEvictionPolicy::Lru, 1).unwrap();
        assert_eq!(ids, vec![ImageId(1)]);
        // Selecionar não conta como acesso
        assert_eq!(cache.lock().metrics().hits, hits_before);
    }

    #[test]
    fn test_selection_policies() {
        let cache = filled_cache(3);
        std::thread::sleep(Duration::from_millis(20));
        cache.lock().store(image(4, 1000)); // a mais nova

        let oldest = select_images_for_eviction(&cache, &GcEvictionPolicy::Oldest, 3).unwrap();
        assert_eq!(oldest.len(), 3);
        assert!(!oldest.contains(&ImageId(4)));

        let hybrid = select_images_for_eviction(
            &cache,
            &GcEvictionPolicy::Hybrid {
                lru_weight: 0.5,
                age_weight: 0.5,
            },
            3,
        )
        .unwrap();
        assert!(!hybrid.contains(&ImageId(4)));

        // TTL e LFU não diferenciam aqui, mas respeitam a contagem
        for policy in [GcEvictionPolicy::Ttl, GcEvictionPolicy::Lfu] {
            assert_eq!(
                select_images_for_eviction(&cache, &policy, 2)
                    .unwrap()
                    .len(),
                2
            );
        }

        let empty = create_test_cache();
        assert!(
            select_images_for_eviction(&empty, &GcEvictionPolicy::Lru, 5)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn test_eviction_count_by_pressure() {
        let stats_for = |count: usize| {
            let cache = ImageCache::with_limits(100, usize::MAX);
            for i in 0..count {
                cache.store(image(i as u32, 1));
            }
            cache.stats()
        };
        let config = GcConfigBuilder::new().max_evict_per_cycle(100).build();
        assert_eq!(calculate_eviction_count(&stats_for(95), &config), 28); // >90%: 30%
        assert_eq!(calculate_eviction_count(&stats_for(85), &config), 17); // >80%: 20%
        assert_eq!(calculate_eviction_count(&stats_for(50), &config), 5); // 10%
                                                                          // Sempre remove pelo menos 1
        assert_eq!(calculate_eviction_count(&stats_for(1), &config), 1);
        let limited = GcConfigBuilder::new().max_evict_per_cycle(2).build();
        assert_eq!(calculate_eviction_count(&stats_for(95), &limited), 2);
    }

    #[test]
    fn test_update_stats_averages_cycle_time() {
        let stats = Arc::new(Mutex::new(GcStats::default()));
        update_stats(&stats, 1, 10, Duration::from_millis(10));
        update_stats(&stats, 2, 20, Duration::from_millis(30));
        let s = *stats.lock();
        assert_eq!(s.cycles_run, 2);
        assert_eq!(s.images_evicted, 3);
        assert_eq!(s.bytes_freed, 30);
        assert!(
            (s.avg_cycle_time_ms - 20.0).abs() < 0.5,
            "{}",
            s.avg_cycle_time_ms
        );
    }

    #[tokio::test]
    async fn test_running_gc_handles_commands() {
        let cache = filled_cache(10);
        let mut gc = ImageGarbageCollector::new(cache.clone(), GcConfig::default());

        // Comandos exigem GC rodando
        assert!(matches!(gc.force_collect(), Err(GcError::NotRunning)));
        assert!(matches!(
            gc.update_config(GcConfig::default()),
            Err(GcError::NotRunning)
        ));
        assert!(matches!(gc.request_stats().await, Err(GcError::NotRunning)));

        gc.start().unwrap();
        gc.force_collect().unwrap();
        // request_stats é processado depois do force_collect (mesma fila)
        let stats = gc.request_stats().await.unwrap();
        assert!(stats.cycles_run >= 1);
        assert!(cache.lock().len() < 10);

        gc.update_config(GcConfig::conservative()).unwrap();
        let stats_after = gc.request_stats().await.unwrap();
        assert_eq!(stats_after.errors, 0);
        assert_eq!(gc.stats().images_evicted, stats_after.images_evicted);

        gc.stop().unwrap();
    }

    /// Regressão: start() depois de stop() marcava o GC como rodando sem loop.
    #[tokio::test]
    async fn test_restart_after_stop_is_rejected_without_running() {
        let mut gc = ImageGarbageCollector::new(create_test_cache(), GcConfig::default());
        gc.start().unwrap();
        gc.stop().unwrap();
        assert!(matches!(gc.start(), Err(GcError::AlreadyRunning)));
        assert!(!gc.is_running());
    }

    #[tokio::test]
    async fn test_interval_tick_runs_cycles() {
        let cache = filled_cache(10);
        let config = GcConfigBuilder::new()
            .interval(Duration::from_millis(10))
            .build();
        let mut gc = ImageGarbageCollector::new(cache.clone(), config);
        gc.start().unwrap();
        sleep(Duration::from_millis(80)).await;
        assert!(gc.stats().cycles_run >= 1);
        gc.stop().unwrap();
    }

    #[test]
    fn test_builder_clamps_and_presets() {
        let config = GcConfigBuilder::new()
            .memory_threshold(1.5)
            .count_threshold(-1.0)
            .eviction_policy(GcEvictionPolicy::Oldest)
            .build();
        assert_eq!(config.memory_threshold, 1.0);
        assert_eq!(config.count_threshold, 0.0);
        assert_eq!(config.eviction_policy, GcEvictionPolicy::Oldest);

        assert!(matches!(
            GcConfig::aggressive().eviction_policy,
            GcEvictionPolicy::Hybrid { .. }
        ));
        assert_eq!(GcConfig::conservative().max_evict_per_cycle, 5);
        assert!(GcConfig::debug().verbose);
        assert_eq!(GcError::ChannelError.to_string(), "Channel error");
        assert_eq!(GcError::LockError.to_string(), "Lock error");
    }
}
