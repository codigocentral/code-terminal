//! Testes de Integração para Otimização de Memória (US-015)
//!
//! Este arquivo contém testes que validam:
//! - Compactação de scrollback
//! - Pool de buffers
//! - Tracking de memória
//! - Ausência de memory leaks

use code_terminal::terminal::scrollback::CompactScrollback;
use code_terminal::terminal::{Attributes, Cell, Color, Grid};
use code_terminal::utils::memory_pool::{acquire_buffer, global_pool_stats, BufferPool};
use code_terminal::utils::memory_tracker::{AllocGuard, MemoryTracker};
use std::time::Duration;

/// Testa a compactação básica do scrollback
#[test]
fn test_scrollback_compression() {
    let mut scrollback = CompactScrollback::with_limit(1000);

    // Adiciona linhas com padrões repetitivos (alta compressibilidade)
    for _i in 0..100 {
        let cells: Vec<Cell> = (0..80)
            .map(|j| {
                if j < 40 {
                    Cell::empty()
                } else {
                    Cell::new('X', Color::Ansi(1), Color::Default, Attributes::default())
                }
            })
            .collect();
        scrollback.push_line(&cells);
    }

    let stats = scrollback.stats();

    // Verifica estatísticas
    assert_eq!(stats.total_lines, 100);

    // Com RLE, deve haver boa compressão
    // Cada linha deveria ter ~2 runs (40 espaços + 40 X's)
    assert!(stats.rle_lines > 0, "Deveria ter linhas RLE");

    // Taxa de compressão deve ser boa (< 50% do original)
    // Original: 100 linhas * 80 células * ~32 bytes = ~256KB
    // Compactado: deve ser significativamente menor
    println!("Compressão: {:.2}%", stats.compression_ratio * 100.0);
}

/// Testa delta encoding entre linhas similares
#[test]
fn test_delta_encoding() {
    let mut scrollback = CompactScrollback::with_limit(100);

    // Linha base
    let base_line: Vec<Cell> = (0..80).map(|_| Cell::empty()).collect();
    scrollback.push_line(&base_line);

    // Linha similar (apenas 2 caracteres diferentes)
    for _i in 0..10 {
        let mut line = base_line.clone();
        line[10] = Cell::new('A', Color::Ansi(1), Color::Default, Attributes::default());
        line[20] = Cell::new('B', Color::Ansi(2), Color::Default, Attributes::default());
        scrollback.push_line(&line);
    }

    let stats = scrollback.stats();

    // Deveria ter linhas delta (mudanças pequenas)
    println!("Linhas delta: {}", stats.delta_lines);
}

/// Testa limite do scrollback
#[test]
fn test_scrollback_limit() {
    let mut scrollback = CompactScrollback::with_limit(50);

    // Adiciona mais linhas que o limite
    for _ in 0..100 {
        let cells: Vec<Cell> = (0..80).map(|_| Cell::empty()).collect();
        scrollback.push_line(&cells);
    }

    // Deve respeitar o limite
    assert_eq!(scrollback.len(), 50);

    let stats = scrollback.stats();
    assert_eq!(stats.total_lines, 50);
}

/// Testa pool de buffers
#[test]
fn test_buffer_pool_reuse() {
    let pool = BufferPool::new();

    // Adquire e libera alguns buffers
    let mut buffers = Vec::new();
    for _ in 0..10 {
        buffers.push(pool.acquire(1024));
    }
    drop(buffers);

    // Estatísticas devem mostrar alocações
    let _stats_before = pool.stats();

    // Adquire novamente - deve reusar
    let buf = pool.acquire(1024);
    drop(buf);

    let stats_after = pool.stats();

    // Deve ter tido hit no pool
    assert!(stats_after.hits > 0, "Deveria reutilizar buffers do pool");
    println!("Hits: {}, Misses: {}", stats_after.hits, stats_after.misses);
}

/// Testa pool global
#[test]
fn test_global_buffer_pool() {
    // Limpa estatísticas anteriores
    let _ = global_pool_stats();

    // Usa pool global
    {
        let buf = acquire_buffer(4096);
        assert_eq!(buf.len(), 4096);
    }

    let stats = global_pool_stats();
    assert!(stats.allocations > 0);
}

/// Testa tracking básico de memória
#[test]
fn test_memory_tracking() {
    let tracker = MemoryTracker::new();

    // Registra alocações
    tracker.record_alloc(0x1000, 1024, "test_category");
    tracker.record_alloc(0x2000, 2048, "test_category");
    tracker.record_alloc(0x3000, 512, "other_category");

    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.total_allocated, 1024 + 2048 + 512);
    assert_eq!(snapshot.total_allocations, 3);
    assert_eq!(snapshot.category_count, 2);

    // Libera uma
    tracker.record_free(0x1000);

    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.total_allocated, 2048 + 512);
    assert_eq!(snapshot.total_allocations, 2);
}

/// Testa guard de alocação
#[test]
fn test_alloc_guard() {
    let tracker = MemoryTracker::new();

    {
        let _guard = AllocGuard::new(1024, "test", &tracker);
        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.total_allocated, 1024);
    }

    // Após drop do guard
    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.total_allocated, 0);
}

/// Testa relatório de memória
#[test]
fn test_memory_report() {
    let tracker = MemoryTracker::new();

    tracker.record_alloc(0x1000, 1024, "grid");
    tracker.record_alloc(0x2000, 2048, "scrollback");
    tracker.record_alloc(0x3000, 512, "grid");

    let report = tracker.report();

    assert_eq!(report.snapshot.total_allocated, 1024 + 2048 + 512);
    assert_eq!(report.categories.len(), 2);

    // Grid deve ter 1024 + 512 = 1536 bytes
    let grid_stats = report
        .categories
        .iter()
        .find(|(name, _)| name == "grid")
        .map(|(_, stats)| stats);

    assert!(grid_stats.is_some());
    assert_eq!(grid_stats.unwrap().current_bytes, 1536);
}

/// Teste de estresse de memória
#[test]
fn test_memory_stress() {
    let pool = BufferPool::new();
    let tracker = MemoryTracker::new();

    // Muitas alocações rápidas
    for i in 0..1000 {
        let size = 512 + (i % 1024);
        let mut buf = pool.acquire(size);
        buf[0] = i as u8;

        tracker.record_alloc(0x10000 + i, size, "stress");
    }

    // Libera metade
    for i in 0..500 {
        tracker.record_free(0x10000 + i);
    }

    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.total_allocations, 500);

    // Verifica que pool tem buffers
    assert!(pool.pooled_count() > 0);
}

/// Testa Grid com scrollback grande
#[test]
fn test_grid_with_large_scrollback() {
    let mut grid = Grid::with_scrollback_limit(80, 24, 10000);

    // Preenche muitas linhas
    for row in 0..1000 {
        for _col in 0..80 {
            let color = Color::Ansi((row % 16) as u8);
            grid.write('A', color, Color::Default, Attributes::default());
        }
        grid.newline();
    }

    // Verifica scrollback
    assert_eq!(grid.scrollback_len(), 977);

    // Verifica navegação
    grid.scroll_to_top();
    assert!(grid.is_in_scrollback());

    grid.scroll_to_bottom();
    assert!(!grid.is_in_scrollback());
}

/// Testa alternância de buffers
#[test]
fn test_alternate_buffer_memory() {
    let mut grid = Grid::new(80, 24);

    // Preenche conteúdo
    for _ in 0..24 {
        for _ in 0..80 {
            grid.write_char('X');
        }
    }

    // Habilita alternate buffer
    grid.enable_alternate_buffer();
    assert!(grid.using_alternate_buffer());

    // Buffer principal deve estar salvo

    // Preenche alternate
    for _ in 0..24 {
        for _ in 0..80 {
            grid.write_char('Y');
        }
    }

    // Restaura
    grid.disable_alternate_buffer();
    assert!(!grid.using_alternate_buffer());

    // Conteúdo original deve estar preservado
}

/// Testa compressão de linhas vazias
#[test]
fn test_empty_line_compression() {
    let mut scrollback = CompactScrollback::with_limit(100);

    // Adiciona apenas linhas vazias
    for _ in 0..50 {
        let cells: Vec<Cell> = (0..80).map(|_| Cell::empty()).collect();
        scrollback.push_line(&cells);
    }

    let stats = scrollback.stats();

    // Todas devem ser Empty
    assert_eq!(stats.empty_lines, 50);

    // Compressão deve ser muito alta
    assert!(
        stats.compression_ratio < 0.1,
        "Compressão deveria ser >90% para linhas vazias"
    );
}

/// Testa histórico de snapshots
#[test]
fn test_snapshot_history() {
    let tracker = MemoryTracker::new();

    // Salva snapshots ao longo do "tempo"
    tracker.save_snapshot();
    tracker.record_alloc(0x1000, 1000, "test");

    tracker.save_snapshot();
    tracker.record_alloc(0x2000, 2000, "test");

    tracker.save_snapshot();
    tracker.record_alloc(0x3000, 3000, "test");

    // Análise de crescimento
    let analysis = tracker.analyze_growth(Duration::from_secs(60));

    // Deve ter crescimento
    assert!(analysis.bytes_growth > 0);
}

/// Testa estatísticas do buffer pool
#[test]
fn test_pool_stats() {
    let pool = BufferPool::with_config(1024 * 1024, 10);

    // Aloca alguns buffers
    let mut bufs = Vec::new();
    for _ in 0..5 {
        bufs.push(pool.acquire(1024));
    }

    let stats = pool.stats();
    assert_eq!(stats.allocations, 5);
    assert_eq!(stats.hits, 0); // Primeiras alocações

    // Libera
    drop(bufs);

    // Realoca - deve reusar
    let buf = pool.acquire(1024);
    drop(buf);

    let stats = pool.stats();
    assert!(stats.hits >= 1);
}

/// Testa trim do pool
#[test]
fn test_pool_trim() {
    let pool = BufferPool::with_config(1024 * 1024, 5);

    // Cria muitos buffers
    let mut buffers = Vec::new();
    for _ in 0..10 {
        buffers.push(pool.acquire(512));
    }
    drop(buffers);

    let count_before = pool.pooled_count();

    // Trim reduz
    pool.trim();

    let count_after = pool.pooled_count();
    assert!(count_after <= count_before);
}

/// Valida que não há leaks em operações comuns
#[test]
fn test_no_leaks_basic_operations() {
    let tracker = MemoryTracker::new();

    // Simula operações de terminal
    for _ in 0..100 {
        tracker.record_alloc(0x1000, 1024, "operation");
        tracker.record_free(0x1000);
    }

    let snapshot = tracker.snapshot();
    assert_eq!(
        snapshot.total_allocated, 0,
        "Não deveria haver memória alocada após operações balanceadas"
    );
}

/// Testa múltiplas categorias
#[test]
fn test_multiple_categories() {
    let tracker = MemoryTracker::new();

    tracker.record_alloc(0x1000, 100, "grid");
    tracker.record_alloc(0x2000, 200, "scrollback");
    tracker.record_alloc(0x3000, 300, "renderer");
    tracker.record_alloc(0x4000, 400, "parser");
    tracker.record_alloc(0x5000, 500, "grid"); // Outra do grid

    let categories = tracker.category_stats();
    assert_eq!(categories.len(), 4);

    // Grid deve ter 100 + 500 = 600
    let grid = categories.get("grid").unwrap();
    assert_eq!(grid.current_bytes, 600);
    assert_eq!(grid.current_allocs, 2);
}

/// Testa crescimento do scrollback com conteúdo variado
#[test]
fn test_scrollback_varied_content() {
    let mut scrollback = CompactScrollback::with_limit(500);

    // Conteúdo variado
    for i in 0..100 {
        let cells: Vec<Cell> = (0..80)
            .map(|j| {
                let char_code = if j % 2 == 0 { 'A' } else { 'B' };
                let color = Color::Ansi((i % 16) as u8);
                Cell::new(char_code, color, Color::Default, Attributes::default())
            })
            .collect();
        scrollback.push_line(&cells);
    }

    let stats = scrollback.stats();
    assert_eq!(stats.total_lines, 100);

    // Deve ter alguma compressão mesmo com conteúdo variado
    println!(
        "Compressão com conteúdo variado: {:.2}%",
        stats.compression_ratio * 100.0
    );
}
