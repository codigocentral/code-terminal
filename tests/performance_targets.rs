use code_terminal::config::Config;
use code_terminal::terminal::{Grid, Parser};
use code_terminal::ui::tabs::TabManager;
use std::time::{Duration, Instant};

const STARTUP_TARGET: Duration = Duration::from_millis(50);
const LATENCY_TARGET: Duration = Duration::from_millis(5);
const MEMORY_TARGET_BYTES: u64 = 30 * 1024 * 1024;

#[test]
fn core_startup_under_50ms() {
    let start = Instant::now();

    let _config = Config::default();
    let mut tab_manager = TabManager::new();
    tab_manager.create_tab("bash");
    let mut grid = Grid::new(80, 24);
    let mut parser = Parser::new(&mut grid);
    parser.process(b"\x1b[32mready\x1b[0m\n");

    let elapsed = start.elapsed();
    assert!(
        elapsed < STARTUP_TARGET,
        "core startup target exceeded: {:?} >= {:?}",
        elapsed,
        STARTUP_TARGET
    );
}

#[test]
fn parser_input_latency_under_5ms() {
    let mut grid = Grid::new(80, 24);
    let mut parser = Parser::new(&mut grid);
    let samples = 10_000;

    let start = Instant::now();
    for _ in 0..samples {
        parser.process(b"\x1b[10;20H\x1b[1;32mX\x1b[0m");
    }
    let average = start.elapsed() / samples;

    assert!(
        average < LATENCY_TARGET,
        "parser latency target exceeded: {:?} >= {:?}",
        average,
        LATENCY_TARGET
    );
}

#[test]
fn representative_core_memory_delta_under_30mb() {
    let before = current_rss_bytes();

    let mut grids = Vec::new();
    for _ in 0..10 {
        let mut grid = Grid::with_scrollback_limit(120, 40, 10_000);
        for _ in 0..200 {
            for _ in 0..120 {
                grid.write_char('X');
            }
            grid.newline();
        }
        grids.push(grid);
    }

    let after = current_rss_bytes();
    let delta = after.saturating_sub(before);
    assert!(
        delta < MEMORY_TARGET_BYTES,
        "core memory target exceeded: {} bytes >= {} bytes",
        delta,
        MEMORY_TARGET_BYTES
    );

    std::hint::black_box(grids);
}

#[cfg(windows)]
fn current_rss_bytes() -> u64 {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }

    // SAFETY: GetCurrentProcess returns a pseudo-handle for the current process.
    // The counters buffer is initialized with the documented size before the call.
    unsafe {
        let mut counters: ProcessMemoryCounters = zeroed();
        counters.cb = size_of::<ProcessMemoryCounters>() as u32;
        let ok = GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            size_of::<ProcessMemoryCounters>() as u32,
        );
        if ok == 0 {
            0
        } else {
            counters.working_set_size as u64
        }
    }
}

#[cfg(all(unix, not(windows)))]
fn current_rss_bytes() -> u64 {
    let Ok(statm) = std::fs::read_to_string("/proc/self/statm") else {
        return 0;
    };
    let Some(pages) = statm
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return 0;
    };
    pages * 4096
}

#[cfg(not(any(windows, unix)))]
fn current_rss_bytes() -> u64 {
    0
}
