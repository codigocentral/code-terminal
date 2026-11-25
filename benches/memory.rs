use code_terminal::terminal::{Attributes, Color, Grid, Parser};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

/// Benchmark: Alocação de grids de diferentes tamanhos
fn grid_allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_allocation");

    // Grid padrão 80x24
    group.bench_function("grid_80x24", |b| {
        b.iter(|| {
            let grid = Grid::new(80, 24);
            black_box(grid);
        });
    });

    // Grid médio 120x36
    group.bench_function("grid_120x36", |b| {
        b.iter(|| {
            let grid = Grid::new(120, 36);
            black_box(grid);
        });
    });

    // Grid grande 200x100
    group.bench_function("grid_200x100", |b| {
        b.iter(|| {
            let grid = Grid::new(200, 100);
            black_box(grid);
        });
    });

    // Grid muito grande 300x150
    group.bench_function("grid_300x150", |b| {
        b.iter(|| {
            let grid = Grid::new(300, 150);
            black_box(grid);
        });
    });

    // Grid com scrollback grande
    group.bench_function("grid_80x24_scrollback_50k", |b| {
        b.iter(|| {
            let grid = Grid::with_scrollback_limit(80, 24, 50000);
            black_box(grid);
        });
    });

    group.finish();
}

/// Benchmark: Crescimento do scrollback
fn scrollback_growth(c: &mut Criterion) {
    let mut group = c.benchmark_group("scrollback_growth");

    // Adiciona linhas ao scrollback
    let line = "X".repeat(80);

    group.throughput(Throughput::Elements(100));
    group.bench_function("add_100_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for _ in 0..100 {
                for c in line.chars() {
                    grid.write_char(c);
                }
                grid.newline();
            }
            black_box(&grid);
        });
    });

    group.throughput(Throughput::Elements(1000));
    group.bench_function("add_1000_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for _ in 0..1000 {
                for c in line.chars() {
                    grid.write_char(c);
                }
                grid.newline();
            }
            black_box(&grid);
        });
    });

    group.throughput(Throughput::Elements(10000));
    group.bench_function("add_10000_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for _ in 0..10000 {
                for c in line.chars() {
                    grid.write_char(c);
                }
                grid.newline();
            }
            black_box(&grid);
        });
    });

    group.finish();
}

/// Benchmark: Uso de memória por tipos de células
fn cell_storage(c: &mut Criterion) {
    let mut group = c.benchmark_group("cell_storage");

    // Célula simples (padrão)
    group.bench_function("cell_default", |b| {
        b.iter(|| {
            let grid = Grid::new(80, 24);
            black_box(grid.get_cells().len());
        });
    });

    // Preenche com caracteres simples
    group.bench_function("fill_simple_chars", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for row in 0..24 {
                for _col in 0..80 {
                    grid.write_char('A');
                }
                if row < 23 {
                    grid.newline();
                }
            }
            black_box(&grid);
        });
    });

    // Preenche com caracteres coloridos
    group.bench_function("fill_colored_chars", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let colors = [
                Color::Ansi(1),
                Color::Ansi(2),
                Color::Ansi(3),
                Color::Ansi(4),
                Color::Ansi(5),
                Color::Ansi(6),
            ];
            let attrs = Attributes::default();
            for row in 0..24 {
                for col in 0..80 {
                    let color = colors[(row + col) % colors.len()];
                    grid.write('A', color, Color::Default, attrs);
                }
                if row < 23 {
                    grid.newline();
                }
            }
            black_box(&grid);
        });
    });

    // Preenche com atributos completos
    group.bench_function("fill_with_all_attrs", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let attrs = Attributes {
                bold: true,
                italic: true,
                underline: true,
                blink: true,
                reverse: true,
                hidden: false,
                strikethrough: true,
                dim: true,
            };
            for row in 0..24 {
                for _col in 0..80 {
                    grid.write(
                        'A',
                        Color::Rgb(255, 128, 64),
                        Color::Rgb(32, 64, 128),
                        attrs,
                    );
                }
                if row < 23 {
                    grid.newline();
                }
            }
            black_box(&grid);
        });
    });

    // Preenche com caracteres wide (CJK)
    group.bench_function("fill_wide_chars", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let chars = ['あ', 'い', 'う', 'え', 'お', '字', '漢', '中'];
            for row in 0..24 {
                for col in 0..40 {
                    // Metade por causa de wide chars
                    grid.write_char(chars[(row + col) % chars.len()]);
                }
                if row < 23 {
                    grid.newline();
                }
            }
            black_box(&grid);
        });
    });

    group.finish();
}

/// Benchmark: Alternância de buffers
fn buffer_switching(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_switching");

    // Enable/disable alternate buffer várias vezes
    group.bench_function("alternate_switch_10", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            // Preenche conteúdo
            for _ in 0..24 {
                for _ in 0..80 {
                    grid.write_char('X');
                }
                grid.newline();
            }
            // Alterna buffers
            for _ in 0..10 {
                grid.enable_alternate_buffer();
                grid.disable_alternate_buffer();
            }
            black_box(&grid);
        });
    });

    // Cria múltiplos grids (simula múltiplas tabs)
    group.bench_function("multiple_grids_5_tabs", |b| {
        b.iter(|| {
            let grids: Vec<_> = (0..5)
                .map(|_| {
                    let mut grid = Grid::new(80, 24);
                    for _ in 0..24 {
                        for _ in 0..80 {
                            grid.write_char('X');
                        }
                        grid.newline();
                    }
                    grid
                })
                .collect();
            black_box(grids);
        });
    });

    group.bench_function("multiple_grids_10_tabs", |b| {
        b.iter(|| {
            let grids: Vec<_> = (0..10)
                .map(|_| {
                    let mut grid = Grid::new(80, 24);
                    for _ in 0..24 {
                        for _ in 0..80 {
                            grid.write_char('X');
                        }
                        grid.newline();
                    }
                    grid
                })
                .collect();
            black_box(grids);
        });
    });

    group.finish();
}

/// Benchmark: Parser buffer
fn parser_buffer(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_buffer");

    // Parser com buffer pequeno
    let small = b"Hello, World!\n".repeat(10);
    group.throughput(Throughput::Bytes(small.len() as u64));
    group.bench_function("parse_small_buffer", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(&small);
            black_box(&parser);
        });
    });

    // Parser com buffer médio
    let medium = b"Hello, World!\n".repeat(100);
    group.throughput(Throughput::Bytes(medium.len() as u64));
    group.bench_function("parse_medium_buffer", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(&medium);
            black_box(&parser);
        });
    });

    // Parser com buffer grande
    let large = b"Hello, World!\n".repeat(1000);
    group.throughput(Throughput::Bytes(large.len() as u64));
    group.bench_function("parse_large_buffer", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(&large);
            black_box(&parser);
        });
    });

    // Parser com muitas escape sequences
    let many_escapes = "\x1b[31mRed\x1b[0m\x1b[32mGreen\x1b[0m".repeat(100);
    group.throughput(Throughput::Bytes(many_escapes.len() as u64));
    group.bench_function("parse_many_escapes", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(many_escapes.as_bytes());
            black_box(&parser);
        });
    });

    group.finish();
}

/// Benchmark: Operações de string/linha
fn string_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("string_ops");

    // Cria linha e converte para string
    group.bench_function("line_to_string", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for _ in 0..80 {
                grid.write_char('X');
            }
            let cells = grid.get_cells();
            let line: String = cells[0].iter().map(|c| c.char).collect();
            black_box(line);
        });
    });

    // Concatena múltiplas linhas
    group.bench_function("concat_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for row in 0..24 {
                for _ in 0..80 {
                    grid.write_char('X');
                }
                if row < 23 {
                    grid.newline();
                }
            }
            let cells = grid.get_cells();
            let mut full = String::new();
            for row in cells.iter() {
                for cell in row.iter() {
                    full.push(cell.char);
                }
                full.push('\n');
            }
            black_box(full);
        });
    });

    // Busca em conteúdo
    group.bench_function("search_in_content", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for row in 0..24 {
                for col in 0..80 {
                    grid.write_char(if (row + col) % 7 == 0 { 'A' } else { 'X' });
                }
                if row < 23 {
                    grid.newline();
                }
            }
            let cells = grid.get_cells();
            let mut count = 0;
            for row in cells.iter() {
                for cell in row.iter() {
                    if cell.char == 'A' {
                        count += 1;
                    }
                }
            }
            black_box(count);
        });
    });

    group.finish();
}

/// Benchmark: Estado do terminal
fn terminal_state(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_state");

    // Cria grid com estado complexo
    group.bench_function("complex_state", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);

            // Simula estado complexo (vim-like)
            parser.process(b"\x1b[?1049h"); // Alternate buffer
            parser.process(b"\x1b[?25l"); // Hide cursor
            parser.process(b"\x1b[H"); // Home

            // Preenche conteúdo colorido
            for row in 0..24 {
                parser.process(format!("\x1b[{};1H", row + 1).as_bytes());
                parser.process(format!("\x1b[38;5;{}m", row % 256).as_bytes());
                for _ in 0..80 {
                    parser.process(b"X");
                }
            }

            parser.process(b"\x1b[H"); // Home
            parser.process(b"\x1b[?25h"); // Show cursor

            black_box(&parser);
        });
    });

    // Múltiplas modificações de estado
    group.bench_function("rapid_state_changes", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);

            for i in 0..100 {
                parser.process(format!("\x1b[{}m", 30 + (i % 8)).as_bytes()); // Cores
                parser.process(b"X");
                parser.process(format!("\x1b[{};{}H", (i % 24) + 1, (i % 80) + 1).as_bytes());
            }

            black_box(&parser);
        });
    });

    group.finish();
}

/// Benchmark: Comparação de tamanhos
fn size_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("size_comparison");

    // Grid vazio vs cheio
    group.bench_function("empty_grid_80x24", |b| {
        b.iter(|| {
            let grid = Grid::new(80, 24);
            black_box(grid);
        });
    });

    group.bench_function("full_grid_80x24", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for row in 0..24 {
                for _ in 0..80 {
                    grid.write_char('X');
                }
                if row < 23 {
                    grid.newline();
                }
            }
            black_box(grid);
        });
    });

    // Grid com scrollback
    group.bench_function("grid_with_scrollback_1000", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for _ in 0..1024 {
                // Mais que 24 linhas
                for _ in 0..80 {
                    grid.write_char('X');
                }
                grid.newline();
            }
            black_box(grid);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    grid_allocation,
    scrollback_growth,
    cell_storage,
    buffer_switching,
    parser_buffer,
    string_operations,
    terminal_state,
    size_comparison,
);
criterion_main!(benches);
