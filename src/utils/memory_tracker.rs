//! Memory Tracker - Rastreamento de Memória e Detecção de Leaks
//!
//! Este módulo fornece ferramentas para:
//! - Tracking de alocações e liberações
//! - Detecção de memory leaks
//! - Relatórios de uso de memória
//! - Alertas de crescimento excessivo

use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Tracker global de memória
pub struct MemoryTracker {
    inner: Arc<Mutex<TrackerInner>>,
}

struct TrackerInner {
    /// Alocações ativas: (endereço, AllocInfo)
    allocations: HashMap<usize, AllocInfo>,
    /// Estatísticas por categoria
    category_stats: HashMap<String, CategoryStats>,
    /// Histórico de uso de memória
    history: VecDeque<(Instant, MemorySnapshot)>,
    /// Tempo de início
    start_time: Instant,
    /// Configuração
    config: TrackerConfig,
}

/// Informação de uma alocação
#[derive(Clone, Debug)]
struct AllocInfo {
    size: usize,
    category: String,
    backtrace: Option<String>,
    timestamp: Instant,
}

/// Estatísticas de uma categoria
#[derive(Clone, Copy, Debug, Default)]
pub struct CategoryStats {
    pub current_bytes: usize,
    pub current_allocs: usize,
    pub total_bytes: u64,
    pub total_allocs: u64,
    pub peak_bytes: usize,
}

/// Snapshot de memória em um momento
#[derive(Clone, Copy, Debug, Default)]
pub struct MemorySnapshot {
    pub total_allocated: usize,
    pub total_allocations: usize,
    pub category_count: usize,
}

/// Configuração do tracker
#[derive(Clone, Debug)]
pub struct TrackerConfig {
    /// Habilitar backtraces (lento)
    pub capture_backtraces: bool,
    /// Tamanho máximo do histórico
    pub max_history: usize,
    /// Intervalo entre snapshots automáticos
    pub snapshot_interval: Duration,
    /// Threshold de crescimento para alerta (bytes/segundo)
    pub growth_alert_threshold: usize,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            capture_backtraces: false,
            max_history: 1000,
            snapshot_interval: Duration::from_secs(5),
            growth_alert_threshold: 1024 * 1024, // 1MB/s
        }
    }
}

impl MemoryTracker {
    /// Cria um novo tracker
    pub fn new() -> Self {
        Self::with_config(TrackerConfig::default())
    }

    /// Cria um tracker com configuração customizada
    pub fn with_config(config: TrackerConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TrackerInner {
                allocations: HashMap::new(),
                category_stats: HashMap::new(),
                history: VecDeque::with_capacity(config.max_history),
                start_time: Instant::now(),
                config,
            })),
        }
    }

    /// Registra uma alocação
    pub fn record_alloc(&self, ptr: usize, size: usize, category: &str) {
        let mut inner = self.inner.lock();

        let info = AllocInfo {
            size,
            category: category.to_string(),
            backtrace: None, // Simplificado - poderia usar backtrace crate
            timestamp: Instant::now(),
        };

        inner.allocations.insert(ptr, info);

        let stats = inner
            .category_stats
            .entry(category.to_string())
            .or_default();
        stats.current_bytes += size;
        stats.current_allocs += 1;
        stats.total_bytes += size as u64;
        stats.total_allocs += 1;
        if stats.current_bytes > stats.peak_bytes {
            stats.peak_bytes = stats.current_bytes;
        }
    }

    /// Registra uma liberação
    pub fn record_free(&self, ptr: usize) -> Option<(usize, String)> {
        let mut inner = self.inner.lock();

        if let Some(info) = inner.allocations.remove(&ptr) {
            if let Some(stats) = inner.category_stats.get_mut(&info.category) {
                stats.current_bytes = stats.current_bytes.saturating_sub(info.size);
                stats.current_allocs = stats.current_allocs.saturating_sub(1);
            }
            Some((info.size, info.category))
        } else {
            None
        }
    }

    /// Tira um snapshot da memória atual
    pub fn snapshot(&self) -> MemorySnapshot {
        let inner = self.inner.lock();
        Self::snapshot_from_inner(&inner)
    }

    /// Salva um snapshot no histórico
    pub fn save_snapshot(&self) {
        let mut inner = self.inner.lock();
        let snapshot = Self::snapshot_from_inner(&inner);

        if inner.history.len() >= inner.config.max_history {
            inner.history.pop_front();
        }

        inner.history.push_back((Instant::now(), snapshot));
    }

    /// Retorna estatísticas por categoria
    pub fn category_stats(&self) -> HashMap<String, CategoryStats> {
        let inner = self.inner.lock();
        inner.category_stats.clone()
    }

    /// Verifica por memory leaks (alocações não liberadas)
    pub fn check_leaks(&self) -> Vec<LeakInfo> {
        let inner = self.inner.lock();
        Self::check_leaks_from_inner(&inner)
    }

    /// Analisa crescimento de memória
    pub fn analyze_growth(&self, window: Duration) -> GrowthAnalysis {
        let inner = self.inner.lock();
        Self::analyze_growth_from_inner(&inner, window)
    }

    /// Gera relatório completo
    pub fn report(&self) -> MemoryReport {
        let inner = self.inner.lock();
        let snapshot = Self::snapshot_from_inner(&inner);
        let uptime = Instant::now().duration_since(inner.start_time);

        let mut categories: Vec<(String, CategoryStats)> = inner
            .category_stats
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();

        categories.sort_by_key(|(_, stats)| std::cmp::Reverse(stats.current_bytes));

        MemoryReport {
            snapshot,
            uptime_seconds: uptime.as_secs(),
            categories,
            potential_leaks: Self::check_leaks_from_inner(&inner).len(),
            growth_analysis: Self::analyze_growth_from_inner(&inner, Duration::from_secs(60)),
        }
    }

    fn snapshot_from_inner(inner: &TrackerInner) -> MemorySnapshot {
        let total_allocated: usize = inner.category_stats.values().map(|s| s.current_bytes).sum();

        let total_allocations: usize = inner
            .category_stats
            .values()
            .map(|s| s.current_allocs)
            .sum();

        MemorySnapshot {
            total_allocated,
            total_allocations,
            category_count: inner.category_stats.len(),
        }
    }

    fn check_leaks_from_inner(inner: &TrackerInner) -> Vec<LeakInfo> {
        let now = Instant::now();

        inner
            .allocations
            .iter()
            .filter(|(_, info)| {
                // Considera leak se alocado há mais de 5 minutos
                now.duration_since(info.timestamp) > Duration::from_secs(300)
            })
            .map(|(ptr, info)| LeakInfo {
                address: *ptr,
                size: info.size,
                category: info.category.clone(),
                age_seconds: now.duration_since(info.timestamp).as_secs(),
                backtrace: info.backtrace.clone(),
            })
            .collect()
    }

    fn analyze_growth_from_inner(inner: &TrackerInner, window: Duration) -> GrowthAnalysis {
        let now = Instant::now();

        // Encontra snapshot mais antigo dentro da janela
        let old_snapshot = inner
            .history
            .iter()
            .find(|(t, _)| now.duration_since(*t) <= window)
            .map(|(t, s)| (*t, *s));

        let current = Self::snapshot_from_inner(inner);

        if let Some((taken_at, old)) = old_snapshot {
            // A taxa usa o tempo real desde o snapshot, não o tamanho da
            // janela (que subestimava o crescimento recente)
            let elapsed = now.duration_since(taken_at).as_secs_f64();
            let bytes_diff = current.total_allocated as i64 - old.total_allocated as i64;
            let bytes_per_second = if elapsed > 0.0 {
                bytes_diff as f64 / elapsed
            } else {
                0.0
            };

            GrowthAnalysis {
                window_seconds: elapsed,
                bytes_growth: bytes_diff,
                bytes_per_second,
                allocations_growth: current.total_allocations as i64 - old.total_allocations as i64,
                is_concerning: bytes_per_second > inner.config.growth_alert_threshold as f64,
            }
        } else {
            GrowthAnalysis::default()
        }
    }

    /// Limpa o histórico
    pub fn clear_history(&self) {
        self.inner.lock().history.clear();
    }

    /// Reseta todas as estatísticas
    pub fn reset(&self) {
        let mut inner = self.inner.lock();
        inner.allocations.clear();
        inner.category_stats.clear();
        inner.history.clear();
        inner.start_time = Instant::now();
    }
}

impl Default for MemoryTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Informação de um memory leak detectado
#[derive(Clone, Debug)]
pub struct LeakInfo {
    pub address: usize,
    pub size: usize,
    pub category: String,
    pub age_seconds: u64,
    pub backtrace: Option<String>,
}

/// Análise de crescimento de memória
#[derive(Clone, Copy, Debug, Default)]
pub struct GrowthAnalysis {
    pub window_seconds: f64,
    pub bytes_growth: i64,
    pub bytes_per_second: f64,
    pub allocations_growth: i64,
    pub is_concerning: bool,
}

/// Relatório completo de memória
#[derive(Clone, Debug)]
pub struct MemoryReport {
    pub snapshot: MemorySnapshot,
    pub uptime_seconds: u64,
    pub categories: Vec<(String, CategoryStats)>,
    pub potential_leaks: usize,
    pub growth_analysis: GrowthAnalysis,
}

impl MemoryReport {
    /// Formata o relatório como string
    pub fn format(&self) -> String {
        let mut output = String::new();

        output.push_str("=== Memory Report ===\n");
        output.push_str(&format!("Uptime: {} seconds\n", self.uptime_seconds));
        output.push_str(&format!(
            "Total allocated: {} bytes\n",
            self.snapshot.total_allocated
        ));
        output.push_str(&format!(
            "Total allocations: {}\n",
            self.snapshot.total_allocations
        ));
        output.push_str(&format!("Categories: {}\n", self.snapshot.category_count));
        output.push_str(&format!("Potential leaks: {}\n", self.potential_leaks));

        if self.growth_analysis.window_seconds > 0.0 {
            output.push_str(&format!(
                "\nGrowth (last {}s): {:.2} bytes/sec\n",
                self.growth_analysis.window_seconds, self.growth_analysis.bytes_per_second
            ));
            if self.growth_analysis.is_concerning {
                output.push_str("WARNING: Memory growth is concerning!\n");
            }
        }

        if !self.categories.is_empty() {
            output.push_str("\nTop categories:\n");
            for (name, stats) in self.categories.iter().take(10) {
                output.push_str(&format!(
                    "  {}: {} bytes ({} allocs, peak: {})\n",
                    name, stats.current_bytes, stats.current_allocs, stats.peak_bytes
                ));
            }
        }

        output
    }
}

use std::collections::VecDeque;

// Re-export CategoryStats
pub type CategoryStatsPublic = CategoryStats;

/// Wrapper seguro para tracking automático
pub struct TrackedBox<T> {
    inner: Box<T>,
    tracker: Option<MemoryTracker>,
}

impl<T> TrackedBox<T> {
    /// Cria um novo TrackedBox
    pub fn new(value: T, tracker: &MemoryTracker, category: &str) -> Self {
        let ptr = Box::into_raw(Box::new(value));
        let size = std::mem::size_of::<T>();

        tracker.record_alloc(ptr as usize, size, category);

        Self {
            inner: unsafe { Box::from_raw(ptr) },
            tracker: Some(tracker.clone()),
        }
    }

    /// Obtém referência ao valor
    pub fn get(&self) -> &T {
        &self.inner
    }

    /// Obtém referência mutável ao valor
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T> std::ops::Deref for TrackedBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> std::ops::DerefMut for TrackedBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T> Drop for TrackedBox<T> {
    fn drop(&mut self) {
        let ptr = self.inner.as_ref() as *const T as usize;
        if let Some(ref tracker) = self.tracker {
            tracker.record_free(ptr);
        }
    }
}

/// Macro para tracking de alocações
#[macro_export]
macro_rules! track_alloc {
    ($tracker:expr, $size:expr, $category:expr) => {
        $tracker.record_alloc(
            $crate::utils::memory_tracker::next_alloc_id(),
            $size,
            $category,
        )
    };
}

static ALLOC_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Gera próximo ID de alocação (para simulação)
pub fn next_alloc_id() -> usize {
    ALLOC_COUNTER.fetch_add(1, Ordering::SeqCst) as usize
}

/// Guarda de alocação temporária
pub struct AllocGuard {
    id: usize,
    tracker: Option<MemoryTracker>,
}

impl AllocGuard {
    pub fn new(size: usize, category: &str, tracker: &MemoryTracker) -> Self {
        let id = next_alloc_id();
        tracker.record_alloc(id, size, category);

        Self {
            id,
            tracker: Some(tracker.clone()),
        }
    }
}

impl Drop for AllocGuard {
    fn drop(&mut self) {
        if let Some(ref tracker) = self.tracker {
            tracker.record_free(self.id);
        }
    }
}

/// Verificador de memória em background
pub struct MemoryWatcher {
    tracker: MemoryTracker,
    interval: Duration,
    running: Arc<AtomicUsize>, // 0 = stopped, 1 = running
}

impl MemoryWatcher {
    /// Cria um novo watcher
    pub fn new(tracker: MemoryTracker, interval: Duration) -> Self {
        Self {
            tracker,
            interval,
            running: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Inicia o watcher em uma thread separada
    pub fn start<F>(&self, callback: F)
    where
        F: Fn(MemoryReport) + Send + Sync + 'static,
    {
        let was_running = self.running.swap(1, Ordering::SeqCst);
        if was_running == 1 {
            return; // Já rodando
        }

        let tracker = self.tracker.clone();
        let interval = self.interval;
        let running = self.running.clone();

        let _ = crate::utils::spawn_named("memory-watch", move || {
            while running.load(Ordering::SeqCst) == 1 {
                std::thread::sleep(interval);

                if running.load(Ordering::SeqCst) == 0 {
                    break;
                }

                tracker.save_snapshot();
                let report = tracker.report();

                // Alerta se necessário
                if report.growth_analysis.is_concerning || report.potential_leaks > 10 {
                    tracing::warn!(
                        "Memory concern detected: {} potential leaks, {:.2} bytes/sec growth",
                        report.potential_leaks,
                        report.growth_analysis.bytes_per_second
                    );
                }

                callback(report);
            }
        });
    }

    /// Para o watcher
    pub fn stop(&self) {
        self.running.store(0, Ordering::SeqCst);
    }

    /// Verifica se está rodando
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst) == 1
    }
}

impl Clone for MemoryTracker {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_tracker_basic() {
        let tracker = MemoryTracker::new();

        // Registra algumas alocações
        tracker.record_alloc(0x1000, 1024, "test");
        tracker.record_alloc(0x2000, 2048, "test");

        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.total_allocated, 3072);
        assert_eq!(snapshot.total_allocations, 2);

        // Libera uma
        tracker.record_free(0x1000);

        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.total_allocated, 2048);
        assert_eq!(snapshot.total_allocations, 1);
    }

    #[test]
    fn test_memory_tracker_categories() {
        let tracker = MemoryTracker::new();

        tracker.record_alloc(0x1000, 1024, "category_a");
        tracker.record_alloc(0x2000, 2048, "category_b");
        tracker.record_alloc(0x3000, 512, "category_a");

        let stats = tracker.category_stats();
        assert_eq!(stats.len(), 2);

        let cat_a = stats.get("category_a").unwrap();
        assert_eq!(cat_a.current_bytes, 1536);
        assert_eq!(cat_a.current_allocs, 2);
    }

    #[test]
    fn test_memory_report() {
        let tracker = MemoryTracker::new();

        tracker.record_alloc(0x1000, 1024, "grid");
        tracker.record_alloc(0x2000, 2048, "scrollback");

        let report = tracker.report();
        assert_eq!(report.snapshot.total_allocated, 3072);
        assert_eq!(report.categories.len(), 2);
    }

    #[test]
    fn test_alloc_guard() {
        let tracker = MemoryTracker::new();

        {
            let _guard = AllocGuard::new(1024, "test", &tracker);
            let snapshot = tracker.snapshot();
            assert_eq!(snapshot.total_allocated, 1024);
        }

        // Após drop do guard, deve estar liberado
        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.total_allocated, 0);
    }

    #[test]
    fn test_growth_analysis() {
        let tracker = MemoryTracker::new();

        // Simula crescimento
        tracker.save_snapshot();
        tracker.record_alloc(0x1000, 1024 * 1024, "test"); // 1MB
        std::thread::sleep(Duration::from_millis(100));
        tracker.save_snapshot();

        let analysis = tracker.analyze_growth(Duration::from_secs(1));
        // Deve ter crescimento significativo
        assert!(analysis.bytes_growth > 0);
    }

    #[test]
    fn test_tracked_box() {
        let tracker = MemoryTracker::new();

        {
            let tracked = TrackedBox::new(vec![1, 2, 3, 4, 5], &tracker, "vector");
            assert_eq!(tracked.get().len(), 5);

            let snapshot = tracker.snapshot();
            assert!(snapshot.total_allocated > 0);
        }

        // Após drop, deve estar liberado
        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.total_allocated, 0);
    }

    /// Regressão: a taxa dividia pelo tamanho da janela em vez do tempo real
    /// desde o snapshot, escondendo crescimento rápido.
    #[test]
    fn test_growth_rate_uses_elapsed_time() {
        let tracker = MemoryTracker::new();
        tracker.save_snapshot();
        std::thread::sleep(Duration::from_millis(50));
        tracker.record_alloc(1, 1024 * 1024, "grid");

        let analysis = tracker.analyze_growth(Duration::from_secs(10));
        assert_eq!(analysis.bytes_growth, 1024 * 1024);
        assert_eq!(analysis.allocations_growth, 1);
        assert!(analysis.window_seconds < 5.0, "{}", analysis.window_seconds);
        // ~1 MiB em ~50 ms: bem acima do limite de 1 MiB/s
        assert!(analysis.bytes_per_second > 2.0 * 1024.0 * 1024.0);
        assert!(analysis.is_concerning);

        let report = tracker.report();
        let text = report.format();
        assert!(
            text.contains("WARNING: Memory growth is concerning!"),
            "{text}"
        );
        assert!(text.contains("grid: 1048576 bytes (1 allocs, peak: 1048576)"));
    }

    #[test]
    fn test_growth_without_history_is_default() {
        let tracker = MemoryTracker::default();
        let analysis = tracker.analyze_growth(Duration::from_secs(1));
        assert_eq!(analysis.bytes_growth, 0);
        assert!(!analysis.is_concerning);
        let text = tracker.report().format();
        assert!(!text.contains("Growth"));
        assert!(!text.contains("Top categories"));
        assert!(text.starts_with("=== Memory Report ==="));
    }

    #[test]
    fn test_free_peak_and_unknown_pointer() {
        let tracker = MemoryTracker::new();
        tracker.record_alloc(1, 100, "a");
        tracker.record_alloc(2, 50, "a");
        assert_eq!(tracker.record_free(1), Some((100, "a".to_string())));
        assert_eq!(tracker.record_free(1), None);
        let stats = tracker.category_stats()["a"];
        assert_eq!(stats.current_bytes, 50);
        assert_eq!(stats.peak_bytes, 150);
        assert_eq!(stats.total_bytes, 150);
        assert_eq!(stats.total_allocs, 2);
    }

    #[test]
    fn test_history_is_bounded_and_clearable() {
        let tracker = MemoryTracker::with_config(TrackerConfig {
            max_history: 2,
            ..TrackerConfig::default()
        });
        for _ in 0..5 {
            tracker.save_snapshot();
        }
        assert_eq!(tracker.inner.lock().history.len(), 2);
        tracker.clear_history();
        assert!(tracker.inner.lock().history.is_empty());

        tracker.record_alloc(1, 10, "x");
        tracker.save_snapshot();
        tracker.reset();
        assert_eq!(tracker.snapshot().total_allocated, 0);
        assert!(tracker.category_stats().is_empty());
        assert!(tracker.inner.lock().history.is_empty());
    }

    #[test]
    fn test_old_allocations_are_reported_as_leaks() {
        let tracker = MemoryTracker::new();
        tracker.record_alloc(0xAA, 64, "cache");
        let Some(old) = Instant::now().checked_sub(Duration::from_secs(301)) else {
            return; // relógio monotônico recente demais para simular
        };
        tracker
            .inner
            .lock()
            .allocations
            .get_mut(&0xAA)
            .unwrap()
            .timestamp = old;
        tracker.record_alloc(0xBB, 32, "cache"); // recente: não é leak

        let leaks = tracker.check_leaks();
        assert_eq!(leaks.len(), 1);
        assert_eq!(leaks[0].address, 0xAA);
        assert_eq!(leaks[0].size, 64);
        assert_eq!(leaks[0].category, "cache");
        assert!(leaks[0].age_seconds >= 301);
        assert_eq!(tracker.report().potential_leaks, 1);
    }

    #[test]
    fn test_report_sorts_categories_by_current_bytes() {
        let tracker = MemoryTracker::new();
        tracker.record_alloc(1, 10, "pequena");
        tracker.record_alloc(2, 1000, "grande");
        let report = tracker.report();
        let names: Vec<_> = report.categories.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["grande", "pequena"]);
        assert_eq!(report.snapshot.category_count, 2);
    }

    #[test]
    fn test_tracked_box_mutation_and_macro() {
        let tracker = MemoryTracker::new();
        let mut tracked = TrackedBox::new(vec![1u8], &tracker, "vec");
        tracked.get_mut().push(2);
        tracked.push(3); // DerefMut
        assert_eq!(*tracked, vec![1, 2, 3]); // Deref
        drop(tracked);
        assert_eq!(tracker.snapshot().total_allocations, 0);

        let first = next_alloc_id();
        assert!(next_alloc_id() > first);
        track_alloc!(tracker, 7, "macro");
        assert_eq!(tracker.category_stats()["macro"].current_bytes, 7);
    }

    #[test]
    fn test_watcher_reports_periodically() {
        let tracker = MemoryTracker::new();
        tracker.record_alloc(1, 5, "w");
        let watcher = MemoryWatcher::new(tracker.clone(), Duration::from_millis(5));
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = parking_lot::Mutex::new(tx);
        watcher.start(move |report| {
            let _ = tx.lock().send(report.snapshot.total_allocated);
        });
        assert!(watcher.is_running());
        // Segundo start não cria outra thread
        watcher.start(|_| {});

        let total = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(total, 5);
        watcher.stop();
        assert!(!watcher.is_running());
    }
}
