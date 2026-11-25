use code_terminal::terminal::{Grid, Parser};
use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};

/// Benchmark: Escrita de texto simples no grid
fn grid_write_simple_text(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_write_simple");

    // Teste com 100 linhas de texto simples
    let text = "Hello, World! This is a simple text line for testing.\n".repeat(100);
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("simple_text_100_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for c in text.chars() {
                if c == '\n' {
                    grid.newline();
                } else {
                    grid.write_char(c);
                }
            }
            black_box(&grid);
        });
    });

    // Teste com 1000 linhas
    let text_1000 = "Simple text line content here.\n".repeat(1000);
    group.throughput(Throughput::Bytes(text_1000.len() as u64));
    group.bench_function("simple_text_1000_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            for c in text_1000.chars() {
                if c == '\n' {
                    grid.newline();
                } else {
                    grid.write_char(c);
                }
            }
            black_box(&grid);
        });
    });

    group.finish();
}

/// Benchmark: Parser de escape sequences ANSI
fn parser_ansi_sequences(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser_ansi");

    // Texto colorido simples
    let colored_text = "\x1b[31mRed\x1b[0m \x1b[32mGreen\x1b[0m \x1b[34mBlue\x1b[0m\n".repeat(100);
    group.throughput(Throughput::Bytes(colored_text.len() as u64));
    group.bench_function("colored_text_100_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(colored_text.as_bytes());
            black_box(&parser);
        });
    });

    // Sequências complexas (SGR + cursor movement)
    let complex = "\x1b[1;34;42mBold Blue on Green\x1b[0m\x1b[2J\x1b[H".repeat(100);
    group.throughput(Throughput::Bytes(complex.len() as u64));
    group.bench_function("complex_sequences_100", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(complex.as_bytes());
            black_box(&parser);
        });
    });

    // Output de comando ls simulado (com cores)
    let ls_output =
        "\x1b[01;34mdir1\x1b[0m  \x1b[01;34mdir2\x1b[0m  \x1b[01;32mfile.txt\x1b[0m\n".repeat(200);
    group.throughput(Throughput::Bytes(ls_output.len() as u64));
    group.bench_function("ls_output_200_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(ls_output.as_bytes());
            black_box(&parser);
        });
    });

    group.finish();
}

/// Benchmark: Cursor movements com parser
fn cursor_movement_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("cursor_movement");

    let cursor_up = "\x1b[1ALine\n".repeat(1000);
    group.throughput(Throughput::Bytes(cursor_up.len() as u64));
    group.bench_function("cursor_up_1000", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(cursor_up.as_bytes());
            black_box(&parser);
        });
    });

    let cursor_position = "\x1b[10;20HText\n".repeat(1000);
    group.throughput(Throughput::Bytes(cursor_position.len() as u64));
    group.bench_function("cursor_position_1000", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(cursor_position.as_bytes());
            black_box(&parser);
        });
    });

    let cursor_save_restore = "\x1b[5;10H\x1b[s\x1b[20;40H\x1b[uX\n".repeat(500);
    group.throughput(Throughput::Bytes(cursor_save_restore.len() as u64));
    group.bench_function("cursor_save_restore_500", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(cursor_save_restore.as_bytes());
            black_box(&parser);
        });
    });

    group.finish();
}

/// Benchmark: Operações de scroll
fn scrolling_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("scrolling");

    // Scroll up (CSI S)
    let scroll_up = "Line\n\x1b[S".repeat(500);
    group.throughput(Throughput::Bytes(scroll_up.len() as u64));
    group.bench_function("scroll_up_500", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(scroll_up.as_bytes());
            black_box(&parser);
        });
    });

    // Scroll down (CSI T)
    let scroll_down = "Line\n\x1b[T".repeat(500);
    group.throughput(Throughput::Bytes(scroll_down.len() as u64));
    group.bench_function("scroll_down_500", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(scroll_down.as_bytes());
            black_box(&parser);
        });
    });

    // Auto-scroll com newline (encher scrollback)
    let auto_scroll = "Line content here\n".repeat(1000);
    group.throughput(Throughput::Bytes(auto_scroll.len() as u64));
    group.bench_function("auto_scroll_1000_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(auto_scroll.as_bytes());
            black_box(&parser);
        });
    });

    group.finish();
}

/// Benchmark: Operações de tela (clear, erase)
fn screen_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("screen_ops");

    // Clear screen (ED)
    let clear_screen = "Text\n\x1b[2J".repeat(500);
    group.throughput(Throughput::Bytes(clear_screen.len() as u64));
    group.bench_function("clear_screen_500", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(clear_screen.as_bytes());
            black_box(&parser);
        });
    });

    // Erase line (EL)
    let erase_line = "Text\x1b[2K\n".repeat(500);
    group.throughput(Throughput::Bytes(erase_line.len() as u64));
    group.bench_function("erase_line_500", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(erase_line.as_bytes());
            black_box(&parser);
        });
    });

    // Full screen redraw (simula vim redraw)
    let mut full_redraw = String::new();
    for row in 1..=24 {
        full_redraw.push_str(&format!("\x1b[{};1H", row));
        full_redraw.push_str("Line content here               ");
    }
    full_redraw.push_str("\x1b[1;1H");

    group.throughput(Throughput::Bytes(full_redraw.len() as u64));
    group.bench_function("full_redraw_24_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(full_redraw.as_bytes());
            black_box(&parser);
        });
    });

    group.finish();
}

/// Benchmark: Cargas de trabalho realistas
fn realistic_workloads(c: &mut Criterion) {
    let mut group = c.benchmark_group("realistic");

    // Simula saída de comando cat em arquivo grande
    let cat_output =
        "This is a line of text from a file that is being displayed with the cat command\n"
            .repeat(500);
    group.throughput(Throughput::Bytes(cat_output.len() as u64));
    group.bench_function("cat_large_file_500_lines", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(cat_output.as_bytes());
            black_box(&parser);
        });
    });

    // Simula saída de htop (atualizações frequentes com cores)
    let htop_frame = {
        let mut frame = String::new();
        frame.push_str("\x1b[?25l\x1b[H"); // Hide cursor, home
        frame.push_str("\x1b[7m  PID USER      PR  NI    VIRT    RES    SHR S  %CPU %MEM     TIME+ COMMAND     \x1b[0m\n");
        for i in 0..20 {
            frame.push_str(&format!(
                "\x1b[44m\x1b[37m{:>5}\x1b[0m user      20   0  123456  78901  12345 R  \x1b[32m{:>5.1}\x1b[0m  \x1b[33m{:>4.1}\x1b[0m   0:00.01 process{}\n",
                1000 + i, 10.0 + (i as f32 * 0.5), 5.0 + (i as f32 * 0.2), i
            ));
        }
        frame.push_str("\x1b[?25h"); // Show cursor
        frame
    };
    group.throughput(Throughput::Bytes(htop_frame.len() as u64));
    group.bench_function("htop_frame", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(htop_frame.as_bytes());
            black_box(&parser);
        });
    });

    // Simula saída de compilador (GCC/rustc com warnings coloridos)
    let compiler_output = {
        let mut output = String::new();
        output.push_str("\x1b[0m\x1b[1m\x1b[32m   Compiling\x1b[0m my_project v0.1.0\n");
        for i in 0..50 {
            output.push_str(&format!(
                "\x1b[0m\x1b[1m\x1b[33mwarning\x1b[0m: unused variable: `x{}`\n  \x1b[0m\x1b[36m-->\x1b[0m src/main.rs:{}:5\n",
                i, 10 + i
            ));
        }
        output.push_str("\x1b[0m\x1b[1m\x1b[32m    Finished\x1b[0m dev [unoptimized + debuginfo] target(s) in 1.23s\n");
        output
    };
    group.throughput(Throughput::Bytes(compiler_output.len() as u64));
    group.bench_function("compiler_output_50_warnings", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(compiler_output.as_bytes());
            black_box(&parser);
        });
    });

    group.finish();
}

/// Benchmark: SGR (Select Graphic Rendition) - atributos de texto
fn sgr_attributes(c: &mut Criterion) {
    let mut group = c.benchmark_group("sgr");

    // SGR básico (cores ANSI)
    let basic_sgr = "\x1b[31mRed\x1b[0m\x1b[32mGreen\x1b[0m\x1b[34mBlue\x1b[0m\n".repeat(500);
    group.throughput(Throughput::Bytes(basic_sgr.len() as u64));
    group.bench_function("basic_sgr_500", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(basic_sgr.as_bytes());
            black_box(&parser);
        });
    });

    // SGR 256 cores
    let sgr_256 = {
        let mut s = String::new();
        for i in 0..=255 {
            s.push_str(&format!("\x1b[38;5;{}mX\x1b[0m", i));
        }
        s.push('\n');
        s
    };
    group.throughput(Throughput::Bytes(sgr_256.len() as u64));
    group.bench_function("sgr_256_colors", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(sgr_256.as_bytes());
            black_box(&parser);
        });
    });

    // SGR True Color (RGB)
    let sgr_rgb = {
        let mut s = String::new();
        for r in (0..=255).step_by(32) {
            for g in (0..=255).step_by(32) {
                s.push_str(&format!("\x1b[38;2;{};{};128mX\x1b[0m", r, g));
            }
        }
        s.push('\n');
        s
    };
    group.throughput(Throughput::Bytes(sgr_rgb.len() as u64));
    group.bench_function("sgr_true_color", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(sgr_rgb.as_bytes());
            black_box(&parser);
        });
    });

    // SGR com múltiplos atributos
    let sgr_complex = "\x1b[1;3;4;5;7;31;42mComplex Attributes\x1b[0m\n".repeat(200);
    group.throughput(Throughput::Bytes(sgr_complex.len() as u64));
    group.bench_function("sgr_complex_200", |b| {
        b.iter(|| {
            let mut grid = Grid::new(80, 24);
            let mut parser = Parser::new(&mut grid);
            parser.process(sgr_complex.as_bytes());
            black_box(&parser);
        });
    });

    group.finish();
}

/// Benchmark: Diferentes tamanhos de grid
fn grid_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("grid_sizes");

    let text = "ABCDEFGHIJ".repeat(100);

    for (cols, rows) in [(80, 24), (120, 30), (200, 50), (300, 80)] {
        group.bench_function(format!("grid_{}x{}", cols, rows), |b| {
            b.iter(|| {
                let mut grid = Grid::new(cols, rows);
                let mut parser = Parser::new(&mut grid);
                parser.process(text.as_bytes());
                black_box(&parser);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    grid_write_simple_text,
    parser_ansi_sequences,
    cursor_movement_parsing,
    scrolling_operations,
    screen_operations,
    realistic_workloads,
    sgr_attributes,
    grid_sizes,
);
criterion_main!(benches);
