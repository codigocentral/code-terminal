pub mod error;
pub mod logging;
pub mod memory_pool;
pub mod memory_tracker;
pub mod profiling;

pub use memory_pool::{
    acquire_buffer, global_pool_stats, BufferPool, ObjectPool, PooledBuffer, PooledObject,
};
pub use memory_tracker::{
    next_alloc_id, AllocGuard, LeakInfo, MemoryReport, MemorySnapshot, MemoryTracker,
    MemoryWatcher, TrackedBox,
};
/// Inicia uma thread com nome: ela aparece em `/proc/<pid>/task/*/comm`,
/// no `top -H` e nos depuradores (sem nome, todas levam o nome do processo
/// e não dá para saber qual acorda à toa, #30). A falha já sai no log.
pub fn spawn_named<F, T>(name: &str, f: F) -> std::io::Result<std::thread::JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let spawned = std::thread::Builder::new().name(name.to_string()).spawn(f);
    if let Err(e) = &spawned {
        tracing::error!(thread = name, error = %e, "Failed to start thread");
    }
    spawned
}

pub use profiling::{
    get_global_profiler, print_profiling_report, start_profiling, stop_profiling, Profiler,
    ProfilingScope, ThreadMetrics, GLOBAL_PROFILER,
};
