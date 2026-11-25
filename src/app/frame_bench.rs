//! Medição de quadros na janela de verdade (`CODE_TERMINAL_FRAME_BENCH=1`).
//!
//! Serve para decidir quantos quadros a superfície guarda
//! (`window.frame_latency`): com 1, o macOS usa 2 drawables em vez de 3 e
//! gasta menos memória, mas pode perder quadros quando o desenho atrasa. A
//! janela abre, redesenha sem parar e passa por quatro fases:
//!
//! 1. **repouso** (2 s): o intervalo normal entre quadros (o vsync);
//! 2. **cat**: `cat` de um arquivo de ~50 MB na aba;
//! 3. **rolagem** (5 s): sobe e desce o histórico 3 linhas por quadro;
//! 4. **eco**: dentro de um `cat`, 200 teclas, uma por vez, do envio ao
//!    quadro que mostra o caractere.
//!
//! No fim imprime uma linha `BENCH ...` por fase (p50/p99 do intervalo entre
//! quadros e quantos passaram de 1,5 vsync, os "perdidos") e fecha o app.

use crate::app::state::AppState;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Tamanho do arquivo do `cat`.
const CAT_BYTES: usize = 50 * 1024 * 1024;
const WARMUP: Duration = Duration::from_secs(2);
const SCROLL: Duration = Duration::from_secs(5);
const CAT_TIMEOUT: Duration = Duration::from_secs(180);
const ECHO_SAMPLES: usize = 200;
const ECHO_TIMEOUT: Duration = Duration::from_secs(2);
const MARK: &str = "FIM-BENCH";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Warmup,
    Cat,
    Scroll,
    Echo,
}

/// O que a janela faz depois do quadro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchStep {
    /// Pedir o próximo quadro.
    Continue,
    /// Terminou: imprimir o relatório e sair.
    Finished(String),
}

#[derive(Debug, Default)]
struct EchoState {
    started: bool,
    sent_at: Option<Instant>,
    cursor_before: (usize, usize),
    sent: usize,
    samples: Vec<Duration>,
}

/// Estado da medição.
#[derive(Debug)]
pub struct FrameBench {
    phase: Phase,
    phase_started: Option<Instant>,
    last_frame: Option<Instant>,
    intervals: Vec<Duration>,
    vsync: Duration,
    file: PathBuf,
    latency: u32,
    scroll_up: bool,
    echo: EchoState,
    lines: Vec<String>,
}

impl FrameBench {
    /// Liga a medição se `CODE_TERMINAL_FRAME_BENCH` estiver definida (gera
    /// o arquivo do `cat` numa pasta temporária).
    pub fn from_env(latency: u32) -> Option<Self> {
        std::env::var_os("CODE_TERMINAL_FRAME_BENCH")?;
        let file = std::env::temp_dir().join("code-terminal-frame-bench.txt");
        if let Err(e) = write_big_file(&file) {
            tracing::warn!(error = %e, "Frame bench: could not write the test file");
            return None;
        }
        Some(Self::new(file, latency))
    }

    fn new(file: PathBuf, latency: u32) -> Self {
        Self {
            phase: Phase::Warmup,
            phase_started: None,
            last_frame: None,
            intervals: Vec::new(),
            vsync: Duration::from_millis(16),
            file,
            latency,
            scroll_up: true,
            echo: EchoState::default(),
            lines: Vec::new(),
        }
    }

    /// Um quadro foi apresentado em `now`.
    pub fn on_frame(&mut self, app: &mut AppState, now: Instant) -> BenchStep {
        if let Some(last) = self.last_frame {
            self.intervals.push(now - last);
        }
        self.last_frame = Some(now);
        let started = *self.phase_started.get_or_insert(now);
        let elapsed = now - started;
        match self.phase {
            Phase::Warmup => {
                if elapsed >= WARMUP {
                    self.vsync = percentile(&self.intervals, 0.5).max(Duration::from_millis(1));
                    self.finish_phase("repouso");
                    let command = format!("cat '{}'; echo {MARK}\r", self.file.display());
                    let _ = app.write_to_active_tab(command.as_bytes());
                    self.start(Phase::Cat, now);
                }
            }
            Phase::Cat => {
                if screen_has_line(app, MARK) || elapsed >= CAT_TIMEOUT {
                    self.finish_phase("cat");
                    self.start(Phase::Scroll, now);
                }
            }
            Phase::Scroll => {
                if let Some(grid) = app.active_tab_mut().and_then(|tab| tab.grid_mut()) {
                    let moved = if self.scroll_up {
                        grid.scroll_up_lines(3)
                    } else {
                        grid.scroll_down_lines(3)
                    };
                    if !moved {
                        self.scroll_up = !self.scroll_up;
                    }
                }
                if elapsed >= SCROLL {
                    if let Some(grid) = app.active_tab_mut().and_then(|tab| tab.grid_mut()) {
                        grid.scroll_to_bottom();
                    }
                    self.finish_phase("rolagem");
                    self.start(Phase::Echo, now);
                }
            }
            Phase::Echo => return self.echo_step(app, now),
        }
        BenchStep::Continue
    }

    fn start(&mut self, phase: Phase, now: Instant) {
        self.phase = phase;
        self.phase_started = Some(now);
        self.intervals.clear();
    }

    /// Fecha a fase: p50/p99 do intervalo entre quadros e os perdidos.
    fn finish_phase(&mut self, name: &str) {
        let limit = self.vsync.mul_f32(1.5);
        let dropped = self.intervals.iter().filter(|i| **i > limit).count();
        let line = format!(
            "BENCH latency={} fase={name} quadros={} p50_ms={:.2} p99_ms={:.2} perdidos={dropped} vsync_ms={:.2}",
            self.latency,
            self.intervals.len(),
            ms(percentile(&self.intervals, 0.5)),
            ms(percentile(&self.intervals, 0.99)),
            ms(self.vsync),
        );
        // Já sai na hora: se uma fase seguinte travar, as anteriores ficam.
        println!("{line}");
        self.lines.push(line);
    }

    fn echo_step(&mut self, app: &mut AppState, now: Instant) -> BenchStep {
        let cursor = app
            .active_tab_grid()
            .map(|grid| grid.cursor_position())
            .unwrap_or_default();
        if !self.echo.started {
            // `cat` sem argumento: o próprio terminal ecoa (modo canônico).
            let _ = app.write_to_active_tab(b"cat\r");
            self.echo.started = true;
            self.echo.sent_at = Some(now + Duration::from_millis(500));
            return BenchStep::Continue;
        }
        match self.echo.sent_at {
            Some(at) if at > now => {}
            Some(at) if self.echo.sent > 0 && cursor != self.echo.cursor_before => {
                self.echo.samples.push(now - at);
                self.echo.sent_at = None;
            }
            Some(at) if self.echo.sent > 0 && now - at > ECHO_TIMEOUT => {
                self.echo.sent_at = None;
            }
            Some(_) if self.echo.sent > 0 => {}
            _ => {
                if self.echo.sent >= ECHO_SAMPLES {
                    let _ = app.write_to_active_tab(b"\x03");
                    let samples = &self.echo.samples;
                    let line = format!(
                        "BENCH latency={} fase=eco amostras={} p50_ms={:.2} p99_ms={:.2}",
                        self.latency,
                        samples.len(),
                        ms(percentile(samples, 0.5)),
                        ms(percentile(samples, 0.99)),
                    );
                    println!("{line}");
                    self.lines.push(line);
                    return BenchStep::Finished(self.lines.join("\n"));
                }
                // Linha nova de tempos em tempos (o buffer canônico tem limite).
                let key: &[u8] = if self.echo.sent % 40 == 39 {
                    b"\r"
                } else {
                    b"x"
                };
                self.echo.cursor_before = cursor;
                let _ = app.write_to_active_tab(key);
                self.echo.sent += 1;
                self.echo.sent_at = Some(now);
            }
        }
        BenchStep::Continue
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Percentil `q` (0..=1) de uma lista (não precisa estar ordenada).
fn percentile(values: &[Duration], q: f64) -> Duration {
    if values.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted = values.to_vec();
    sorted.sort();
    let index = ((sorted.len() - 1) as f64 * q).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// A tela da aba ativa tem uma linha exatamente igual a `text`.
fn screen_has_line(app: &AppState, text: &str) -> bool {
    app.active_tab_grid().is_some_and(|grid| {
        grid.visible_lines().iter().any(|row| {
            row.iter()
                .map(|cell| cell.char)
                .collect::<String>()
                .trim_end()
                == text
        })
    })
}

/// ~50 MB de texto em linhas de ~100 caracteres (como um log grande).
fn write_big_file(path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    if std::fs::metadata(path).is_ok_and(|m| m.len() as usize >= CAT_BYTES) {
        return Ok(());
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut written = 0;
    let mut n = 0u64;
    while written < CAT_BYTES {
        let line = format!(
            "{n:08} 2026-11-22T03:14:{:02} INFO servico::modulo requisicao concluida em {} ms status=200 bytes={}\n",
            n % 60,
            n % 997,
            (n * 7919) % 100_000
        );
        out.write_all(line.as_bytes())?;
        written += line.len();
        n += 1;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percentile() {
        let values: Vec<Duration> = (1..=100).map(Duration::from_millis).collect();
        assert_eq!(percentile(&values, 0.5), Duration::from_millis(51));
        assert_eq!(percentile(&values, 0.99), Duration::from_millis(99));
        assert_eq!(percentile(&[], 0.5), Duration::ZERO);
    }

    #[test]
    fn test_finish_phase_counts_late_frames() {
        let mut bench = FrameBench::new(PathBuf::from("/tmp/x"), 1);
        bench.vsync = Duration::from_millis(16);
        bench.intervals = vec![
            Duration::from_millis(16),
            Duration::from_millis(17),
            Duration::from_millis(30),
            Duration::from_millis(16),
        ];
        bench.finish_phase("cat");
        assert!(bench.lines[0].contains("perdidos=1"), "{}", bench.lines[0]);
        assert!(bench.lines[0].contains("latency=1 fase=cat quadros=4"));
    }

    #[test]
    fn test_big_file_has_the_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        write_big_file(&path).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() as usize >= CAT_BYTES);
    }
}
