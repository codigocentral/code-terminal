use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug)]
pub struct ThreadMetrics {
    pub thread_name: String,
    pub cpu_time_ns: AtomicU64,
    pub wall_time_ns: AtomicU64,
    pub allocations: AtomicU64,
    pub deallocations: AtomicU64,
    pub start_time: Instant,
}

impl ThreadMetrics {
    pub fn new(name: &str) -> Self {
        Self {
            thread_name: name.to_string(),
            cpu_time_ns: AtomicU64::new(0),
            wall_time_ns: AtomicU64::new(0),
            allocations: AtomicU64::new(0),
            deallocations: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    pub fn add_cpu_time(&self, ns: u64) {
        self.cpu_time_ns.fetch_add(ns, Ordering::Relaxed);
    }

    pub fn add_wall_time(&self, ns: u64) {
        self.wall_time_ns.fetch_add(ns, Ordering::Relaxed);
    }

    pub fn record_allocation(&self) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_deallocation(&self) {
        self.deallocations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get_cpu_time_ms(&self) -> u64 {
        self.cpu_time_ns.load(Ordering::Relaxed) / 1_000_000
    }

    pub fn get_wall_time_ms(&self) -> u64 {
        self.wall_time_ns.load(Ordering::Relaxed) / 1_000_000
    }

    pub fn get_net_allocations(&self) -> i64 {
        let alloc = self.allocations.load(Ordering::Relaxed) as i64;
        let dealloc = self.deallocations.load(Ordering::Relaxed) as i64;
        alloc - dealloc
    }
}

pub struct Profiler {
    enabled: RwLock<bool>,
    thread_metrics: RwLock<HashMap<String, Arc<ThreadMetrics>>>,
    global_allocations: AtomicU64,
    global_deallocations: AtomicU64,
}

impl Profiler {
    pub fn new() -> Self {
        Self {
            enabled: RwLock::new(false),
            thread_metrics: RwLock::new(HashMap::new()),
            global_allocations: AtomicU64::new(0),
            global_deallocations: AtomicU64::new(0),
        }
    }

    pub fn enable(&self) {
        *self.enabled.write() = true;
    }

    pub fn disable(&self) {
        *self.enabled.write() = false;
    }

    pub fn is_enabled(&self) -> bool {
        *self.enabled.read()
    }

    pub fn register_thread(&self, name: &str) -> Arc<ThreadMetrics> {
        let metrics = Arc::new(ThreadMetrics::new(name));
        self.thread_metrics
            .write()
            .insert(name.to_string(), Arc::clone(&metrics));
        metrics
    }

    pub fn get_thread_metrics(&self, name: &str) -> Option<Arc<ThreadMetrics>> {
        self.thread_metrics.read().get(name).cloned()
    }

    pub fn get_all_metrics(&self) -> Vec<Arc<ThreadMetrics>> {
        self.thread_metrics.read().values().cloned().collect()
    }

    pub fn record_allocation(&self) {
        if self.is_enabled() {
            self.global_allocations.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_deallocation(&self) {
        if self.is_enabled() {
            self.global_deallocations.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn get_total_allocations(&self) -> u64 {
        self.global_allocations.load(Ordering::Relaxed)
    }

    pub fn get_total_deallocations(&self) -> u64 {
        self.global_deallocations.load(Ordering::Relaxed)
    }

    pub fn get_net_allocations(&self) -> i64 {
        self.get_total_allocations() as i64 - self.get_total_deallocations() as i64
    }

    pub fn print_report(&self) {
        if !self.is_enabled() {
            tracing::warn!("Profiler is disabled");
            return;
        }

        tracing::info!("=== Thread Profiling Report ===");
        tracing::info!("Global allocations: {}", self.get_total_allocations());
        tracing::info!("Global deallocations: {}", self.get_total_deallocations());
        tracing::info!("Net allocations: {}", self.get_net_allocations());

        for metrics in self.get_all_metrics() {
            tracing::info!(
                "Thread '{}': CPU={}ms, Wall={}ms, Allocations={}, Deallocations={}, Net={}",
                metrics.thread_name,
                metrics.get_cpu_time_ms(),
                metrics.get_wall_time_ms(),
                metrics.allocations.load(Ordering::Relaxed),
                metrics.deallocations.load(Ordering::Relaxed),
                metrics.get_net_allocations()
            );
        }
    }

    pub fn reset(&self) {
        *self.enabled.write() = false;
        self.thread_metrics.write().clear();
        self.global_allocations.store(0, Ordering::Relaxed);
        self.global_deallocations.store(0, Ordering::Relaxed);
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ProfilingScope {
    name: String,
    start: Instant,
    metrics: Option<Arc<ThreadMetrics>>,
    cpu_start: Option<u64>,
}

impl ProfilingScope {
    pub fn new(name: &str, metrics: Option<Arc<ThreadMetrics>>) -> Self {
        let cpu_start = metrics.as_ref().map(|_| get_thread_cpu_time());

        Self {
            name: name.to_string(),
            start: Instant::now(),
            metrics,
            cpu_start,
        }
    }
}

impl Drop for ProfilingScope {
    fn drop(&mut self) {
        let wall_time = self.start.elapsed().as_nanos() as u64;

        if let Some(ref metrics) = self.metrics {
            metrics.add_wall_time(wall_time);

            if let Some(cpu_start) = self.cpu_start {
                let cpu_time = get_thread_cpu_time().saturating_sub(cpu_start);
                metrics.add_cpu_time(cpu_time);
            }

            tracing::debug!(
                target: "profiling",
                thread = %metrics.thread_name,
                scope = %self.name,
                wall_time_ms = wall_time / 1_000_000,
                cpu_time_ms = self.cpu_start.map(|c| get_thread_cpu_time().saturating_sub(c) / 1_000_000).unwrap_or(0),
                "Profiling scope completed"
            );
        }
    }
}

/// Nanossegundos por tick de `/proc/*/stat` (USER_HZ é 100 no Linux)
#[cfg(target_os = "linux")]
const NS_PER_CLOCK_TICK: u64 = 10_000_000;

/// CPU (utime + stime) em ns a partir de uma linha de `/proc/*/stat`.
///
/// O nome do processo vem entre parênteses e pode ter espaços, então os
/// campos são contados a partir do último `)`.
#[cfg(target_os = "linux")]
fn parse_proc_stat_cpu_ns(content: &str) -> Option<u64> {
    let (_, after_comm) = content.rsplit_once(')')?;
    let fields: Vec<&str> = after_comm.split_whitespace().collect();
    // após o nome: state é o campo 3; utime (14) e stime (15) ficam em 11 e 12
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some((utime + stime) * NS_PER_CLOCK_TICK)
}

/// Tempo de CPU da thread atual em ns (antes lia o processo inteiro e
/// tratava ticks de 10 ms como se fossem 1 ms).
fn get_thread_cpu_time() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/thread-self/stat") {
            return parse_proc_stat_cpu_ns(&content).unwrap_or(0);
        }
    }
    0
}

use std::sync::LazyLock;

pub static GLOBAL_PROFILER: LazyLock<Profiler> = LazyLock::new(Profiler::new);

#[macro_export]
macro_rules! profile_scope {
    ($name:expr) => {
        let _scope = $crate::utils::profiling::ProfilingScope::new($name, None);
    };
    ($name:expr, $metrics:expr) => {
        let _scope = $crate::utils::profiling::ProfilingScope::new($name, Some($metrics));
    };
}

pub fn start_profiling() {
    GLOBAL_PROFILER.enable();
    tracing::info!("Profiling started");
}

pub fn stop_profiling() {
    GLOBAL_PROFILER.disable();
    tracing::info!("Profiling stopped");
}

pub fn print_profiling_report() {
    GLOBAL_PROFILER.print_report();
}

pub fn get_global_profiler() -> &'static Profiler {
    &GLOBAL_PROFILER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_profiler_creation() {
        let profiler = Profiler::new();
        assert!(!profiler.is_enabled());
    }

    #[test]
    fn test_enable_disable() {
        let profiler = Profiler::new();

        profiler.enable();
        assert!(profiler.is_enabled());

        profiler.disable();
        assert!(!profiler.is_enabled());
    }

    #[test]
    fn test_thread_metrics() {
        let profiler = Profiler::new();
        profiler.enable();

        let metrics = profiler.register_thread("test-thread");

        metrics.add_cpu_time(1_000_000); // 1 ms
        metrics.add_wall_time(2_000_000); // 2 ms
        metrics.record_allocation();

        assert_eq!(metrics.get_cpu_time_ms(), 1);
        assert_eq!(metrics.get_wall_time_ms(), 2);
        assert_eq!(metrics.get_net_allocations(), 1);
    }

    #[test]
    fn test_profiling_scope() {
        let profiler = Profiler::new();
        profiler.enable();

        let metrics = profiler.register_thread("test");

        {
            let _scope = ProfilingScope::new("test-scope", Some(metrics.clone()));
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        assert!(metrics.get_wall_time_ms() > 0);
    }

    #[test]
    fn test_global_profiler() {
        let profiler = get_global_profiler();

        profiler.enable();
        assert!(profiler.is_enabled());

        profiler.disable();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_proc_stat_handles_spaces_and_ticks() {
        // utime=7 stime=3 ticks; nome com espaços e parênteses
        let line = "4242 (my (weird) proc) S 1 2 3 4 5 6 7 8 9 10 7 3 0 0 20 0 1 0 100";
        assert_eq!(parse_proc_stat_cpu_ns(line), Some(10 * NS_PER_CLOCK_TICK));
        assert_eq!(parse_proc_stat_cpu_ns("sem parenteses"), None);
        assert_eq!(parse_proc_stat_cpu_ns("1 (x) S 1 2"), None);
        assert_eq!(
            parse_proc_stat_cpu_ns("1 (x) S 1 2 3 4 5 6 7 8 9 10 a b"),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_thread_cpu_time_advances_with_work() {
        let start = get_thread_cpu_time();
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        let mut x = 0u64;
        while get_thread_cpu_time() <= start && Instant::now() < deadline {
            for i in 0..100_000u64 {
                x = x.wrapping_mul(31).wrapping_add(i);
            }
        }
        std::hint::black_box(x);
        let spent = get_thread_cpu_time() - start;
        assert!(spent >= NS_PER_CLOCK_TICK, "cpu não avançou: {spent}");
    }

    #[test]
    fn test_scope_records_cpu_and_wall_time() {
        let profiler = Profiler::default();
        let metrics = profiler.register_thread("worker");
        {
            let _scope = ProfilingScope::new("trabalho", Some(metrics.clone()));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(metrics.wall_time_ns.load(Ordering::Relaxed) >= 5_000_000);
        // Escopo sem métricas não registra nada
        drop(ProfilingScope::new("solto", None));
        crate::profile_scope!("macro");
        crate::profile_scope!("macro com metricas", metrics.clone());
    }

    #[test]
    fn test_profiler_counts_only_when_enabled_and_reset() {
        let profiler = Profiler::new();
        profiler.record_allocation();
        profiler.record_deallocation();
        assert_eq!(profiler.get_total_allocations(), 0);

        profiler.enable();
        profiler.record_allocation();
        profiler.record_allocation();
        profiler.record_deallocation();
        assert_eq!(profiler.get_total_allocations(), 2);
        assert_eq!(profiler.get_total_deallocations(), 1);
        assert_eq!(profiler.get_net_allocations(), 1);

        let metrics = profiler.register_thread("t");
        metrics.record_allocation();
        metrics.record_deallocation();
        metrics.record_deallocation();
        assert_eq!(metrics.get_net_allocations(), -1);
        assert_eq!(profiler.get_thread_metrics("t").unwrap().thread_name, "t");
        assert!(profiler.get_thread_metrics("x").is_none());
        assert_eq!(profiler.get_all_metrics().len(), 1);
        profiler.print_report();

        profiler.reset();
        assert!(!profiler.is_enabled());
        assert!(profiler.get_all_metrics().is_empty());
        assert_eq!(profiler.get_total_allocations(), 0);
        profiler.print_report(); // desabilitado: só avisa
    }

    #[test]
    fn test_global_profiling_helpers() {
        // O estado global é compartilhado com test_global_profiler: aqui só
        // se verifica que os atalhos apontam para o mesmo profiler
        assert!(std::ptr::eq(get_global_profiler(), &*GLOBAL_PROFILER));
        start_profiling();
        print_profiling_report();
        stop_profiling();
    }
}
