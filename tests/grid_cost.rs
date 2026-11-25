//! Custo de CPU do grid com histórico cheio (#1): encher 10 mil linhas,
//! rolar o histórico (abre as linhas compactadas da tela) e digitar com o
//! histórico cheio. Só API pública, para comparar versões. Rodar em modo
//! otimizado:
//!
//! ```text
//! cargo test --release --test grid_cost -- --ignored --nocapture
//! ```

use code_terminal::terminal::grid::Grid;
use code_terminal::terminal::parser::Parser;
use std::time::{Duration, Instant};

const COLS: usize = 160;
const ROWS: usize = 40;
const LINES: usize = 10_000;

fn percentile(samples: &mut [Duration], p: f64) -> Duration {
    samples.sort();
    samples[((samples.len() - 1) as f64 * p) as usize]
}

fn report(label: &str, mut samples: Vec<Duration>) {
    let p50 = percentile(&mut samples, 0.50);
    let p99 = percentile(&mut samples, 0.99);
    println!(
        "RESULTADO {label} p50_us={:.2} p99_us={:.2} n={}",
        p50.as_secs_f64() * 1e6,
        p99.as_secs_f64() * 1e6,
        samples.len()
    );
}

/// Saída colorida e com largos: o pior caso razoável para o histórico.
fn line(i: usize) -> String {
    format!(
        "\x1b[1;38;2;{};{};200mlinha {i:06}\x1b[0m de historico com texto \x1b[48;5;{}mpara ocupar\x1b[0m boa parte da largura da tab 日本 🚀\r\n",
        i % 256,
        (i / 7) % 256,
        i % 256
    )
}

fn full_grid() -> (Grid, Duration, usize) {
    let mut grid = Grid::new(COLS, ROWS);
    let input: String = (0..LINES + ROWS).map(line).collect();
    let started = Instant::now();
    Parser::new(&mut grid).process(input.as_bytes());
    (grid, started.elapsed(), input.len())
}

#[test]
#[ignore = "medição"]
fn grid_cost() {
    let (mut grid, fill, bytes) = full_grid();
    println!(
        "RESULTADO encher linhas={LINES} ms={:.1} mb_s={:.1}",
        fill.as_secs_f64() * 1e3,
        bytes as f64 / fill.as_secs_f64() / 1e6
    );

    // Rolagem: 3 linhas por vez (roda do mouse), do fim até o topo.
    let mut scroll = Vec::new();
    let mut seen = 0usize;
    loop {
        let started = Instant::now();
        let moved = grid.scroll_up_lines(3);
        let visible = grid.visible_lines();
        seen += visible.iter().map(|row| row.len()).sum::<usize>();
        scroll.push(started.elapsed());
        if !moved {
            break;
        }
    }
    report("rolar_3_linhas", scroll);

    // Pular para o topo e voltar (Shift+PgUp / busca).
    let mut jump = Vec::new();
    for _ in 0..200 {
        let started = Instant::now();
        grid.scroll_to_top();
        seen += grid.visible_lines().len();
        grid.scroll_to_bottom();
        seen += grid.visible_lines().len();
        jump.push(started.elapsed());
    }
    report("topo_e_fim", jump);

    // Digitação com o histórico cheio: um caractere e o redesenho da tela;
    // a cada 40, um Enter (linha nova vai para o histórico).
    let mut typing = Vec::new();
    let mut newline = Vec::new();
    for i in 0..4000 {
        let data: &[u8] = if i % 40 == 39 { b"\r\n" } else { b"a" };
        let started = Instant::now();
        Parser::new(&mut grid).process(data);
        seen += grid.visible_lines().len();
        let took = started.elapsed();
        if i % 40 == 39 {
            newline.push(took);
        } else {
            typing.push(took);
        }
    }
    report("digitar_tecla", typing);
    report("digitar_enter", newline);
    assert!(seen > 0);
}

fn private_kb() -> u64 {
    let text = std::fs::read_to_string("/proc/self/smaps_rollup").unwrap_or_default();
    text.lines()
        .filter(|l| l.starts_with("Private_Clean:") || l.starts_with("Private_Dirty:"))
        .filter_map(|l| l.split_whitespace().nth(1)?.parse::<u64>().ok())
        .sum()
}

/// Memória de 20 abas 160×40 na tela alternativa (vim, htop), com um
/// histórico pequeno na principal (#1).
#[test]
#[ignore = "medição"]
#[cfg(target_os = "linux")]
fn alternate_screen_memory() {
    let before = private_kb();
    let mut grids: Vec<Grid> = (0..20)
        .map(|_| {
            let mut grid = Grid::new(COLS, ROWS);
            let input: String = (0..100).map(line).collect();
            Parser::new(&mut grid).process(input.as_bytes());
            grid
        })
        .collect();
    let main = private_kb();
    for grid in &mut grids {
        Parser::new(grid).process(b"\x1b[?1049hvim\r\n");
    }
    let alt = private_kb();
    println!(
        "RESULTADO tela_alternativa abas=20 principal_kb={} alternativa_kb={} por_aba_kb={}",
        main - before,
        alt - main,
        (alt - main) / 20
    );
    assert!(grids.iter().all(|g| g.using_alternate_buffer()));
}
