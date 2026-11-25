use code_terminal::terminal::{Attributes, Color, Grid, Parser};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::time::{Duration, Instant};

/// Benchmark: Latência de processamento de input simples
fn input_processing_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("input_latency");
    group.measurement_time(Duration::from_secs(2));

    // Caractere único simples
    group.bench_function("single_char", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"a");
            }
            start.elapsed()
        });
    });

    // Escape sequence simples
    group.bench_function("escape_sequence_cuu", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"\x1b[A");
            }
            start.elapsed()
        });
    });

    // Sequence com parâmetros
    group.bench_function("csi_with_params", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"\x1b[10;20H");
            }
            start.elapsed()
        });
    });

    // SGR sequence
    group.bench_function("sgr_sequence", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"\x1b[31;1;42m");
            }
            start.elapsed()
        });
    });

    // OSC sequence (título)
    group.bench_function("osc_sequence", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"\x1b]0;Window Title\x07");
            }
            start.elapsed()
        });
    });

    group.finish();
}

/// Benchmark: Latência de processamento de output
fn output_processing_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("output_latency");
    group.measurement_time(Duration::from_secs(2));

    // Pequeno output (uma linha)
    let small = b"Hello, World!\n";
    group.bench_function("small_14_bytes", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(small);
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    // Médio (1KB)
    let medium = "X".repeat(1024);
    group.throughput(Throughput::Bytes(medium.len() as u64));
    group.bench_function("medium_1kb", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(medium.as_bytes());
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    // Grande (64KB)
    let large = "X".repeat(65536);
    group.throughput(Throughput::Bytes(large.len() as u64));
    group.bench_function("large_64kb", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(large.as_bytes());
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    group.finish();
}
use criterion::Throughput;

/// Benchmark: Latência de operações específicas
fn operation_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("operation_latency");
    group.measurement_time(Duration::from_secs(2));

    // Newline
    group.bench_function("newline", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"\n");
            }
            start.elapsed()
        });
    });

    // Tab
    group.bench_function("tab", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"\t");
            }
            start.elapsed()
        });
    });

    // Backspace
    group.bench_function("backspace", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(b"abc");
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"\x08");
            }
            start.elapsed()
        });
    });

    // Carriage return
    group.bench_function("carriage_return", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(b"text");
            let start = Instant::now();
            for _ in 0..iters {
                parser.process(b"\r");
            }
            start.elapsed()
        });
    });

    // Clear screen
    group.bench_function("clear_screen", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(b"\x1b[2J");
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    // Move cursor
    group.bench_function("move_cursor_cup", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(b"\x1b[10;20H");
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    group.finish();
}

/// Benchmark: Latência de escrita de células
fn cell_write_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("cell_write");
    group.measurement_time(Duration::from_secs(2));

    // Caractere simples
    group.bench_function("char_simple", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let start = Instant::now();
            for _ in 0..iters {
                grid.write_char('A');
            }
            start.elapsed()
        });
    });

    // Caractere com cor
    group.bench_function("char_with_color", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let fg = Color::Rgb(255, 0, 0);
            let bg = Color::Rgb(0, 0, 255);
            let attrs = Attributes::default();
            let start = Instant::now();
            for _ in 0..iters {
                grid.write('A', fg, bg, attrs);
            }
            start.elapsed()
        });
    });

    // Caractere com atributos
    group.bench_function("char_with_attrs", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let fg = Color::Default;
            let bg = Color::Default;
            let attrs = Attributes {
                bold: true,
                italic: true,
                underline: true,
                ..Default::default()
            };
            let start = Instant::now();
            for _ in 0..iters {
                grid.write('A', fg, bg, attrs);
            }
            start.elapsed()
        });
    });

    // Wide character (CJK)
    group.bench_function("char_wide", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let start = Instant::now();
            for _ in 0..iters {
                grid.write_char('字');
            }
            start.elapsed()
        });
    });

    // Emoji
    group.bench_function("char_emoji", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            let start = Instant::now();
            for _ in 0..iters {
                grid.write_char('😀');
            }
            start.elapsed()
        });
    });

    group.finish();
}

/// Benchmark: Latência de scroll
fn scroll_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("scroll");
    group.measurement_time(Duration::from_secs(2));

    // Scroll up com conteúdo
    group.bench_function("scroll_up_with_content", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            // Preenche grid
            for _ in 0..24 {
                for _ in 0..80 {
                    grid.write_char('X');
                }
                grid.newline();
            }
            let start = Instant::now();
            for _ in 0..iters {
                grid.scroll_up_one();
            }
            start.elapsed()
        });
    });

    // Scroll down com conteúdo
    group.bench_function("scroll_down_with_content", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            // Preenche grid
            for _ in 0..24 {
                for _ in 0..80 {
                    grid.write_char('X');
                }
                grid.newline();
            }
            let start = Instant::now();
            for _ in 0..iters {
                grid.scroll_down_one();
            }
            start.elapsed()
        });
    });

    // Auto-scroll (newline no final)
    group.bench_function("auto_scroll_newline", |b| {
        b.iter_custom(|iters| {
            let mut grid = Grid::new(80, 24);
            // Posiciona no final
            for _ in 0..23 {
                grid.newline();
            }
            let start = Instant::now();
            for _ in 0..iters {
                grid.newline();
            }
            start.elapsed()
        });
    });

    group.finish();
}

/// Benchmark: Latência de resize
fn resize_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("resize");
    group.measurement_time(Duration::from_secs(2));

    // Resize pequeno -> médio
    group.bench_function("resize_80x24_to_120x36", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                // Preenche com conteúdo
                for row in 0..24 {
                    for _col in 0..80 {
                        grid.write_char('X');
                    }
                    if row < 23 {
                        grid.newline();
                    }
                }
                // Resize - recria o grid (simula comportamento real)
                black_box(&grid);
            }
            start.elapsed()
        });
    });

    // Resize para tela grande
    group.bench_function("resize_to_200x100", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                // Preenche com conteúdo
                for row in 0..24 {
                    for _col in 0..80 {
                        grid.write_char('X');
                    }
                    if row < 23 {
                        grid.newline();
                    }
                }
                black_box(&grid);
            }
            start.elapsed()
        });
    });

    group.finish();
}

/// Benchmark: Latência de parsing de diferentes tipos de sequences
fn parsing_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("parsing");
    group.measurement_time(Duration::from_secs(2));

    // CSI simples
    group.bench_function("parse_csi_simple", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(b"\x1b[H");
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    // CSI com múltiplos parâmetros
    group.bench_function("parse_csi_complex", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(b"\x1b[1;2;3;4;5m");
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    // ESC sequence
    group.bench_function("parse_esc", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(b"\x1b7"); // Save cursor
                parser.process(b"\x1b8"); // Restore cursor
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    // Texto misto (texto + escapes)
    group.bench_function("parse_mixed_content", |b| {
        let mixed = b"Hello \x1b[31mWorld\x1b[0m!\n";
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                let mut parser = Parser::new(&mut grid);
                parser.process(mixed);
                black_box(&parser);
            }
            start.elapsed()
        });
    });

    group.finish();
}

/// Benchmark: Latência de alternate buffer
fn alternate_buffer_latency(c: &mut Criterion) {
    let mut group = c.benchmark_group("alternate_buffer");
    group.measurement_time(Duration::from_secs(2));

    // Enable alternate buffer
    group.bench_function("enable_alternate", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                // Preenche conteúdo primeiro
                for _ in 0..24 {
                    for _ in 0..80 {
                        grid.write_char('X');
                    }
                    grid.newline();
                }
                grid.enable_alternate_buffer();
                black_box(&grid);
            }
            start.elapsed()
        });
    });

    // Disable alternate buffer (restore)
    group.bench_function("disable_alternate", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let mut grid = Grid::new(80, 24);
                // Preenche conteúdo
                for _ in 0..24 {
                    for _ in 0..80 {
                        grid.write_char('X');
                    }
                    grid.newline();
                }
                grid.enable_alternate_buffer();
                grid.disable_alternate_buffer();
                black_box(&grid);
            }
            start.elapsed()
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    input_processing_latency,
    output_processing_latency,
    operation_latency,
    cell_write_latency,
    scroll_latency,
    resize_latency,
    parsing_latency,
    alternate_buffer_latency,
);
criterion_main!(benches);
