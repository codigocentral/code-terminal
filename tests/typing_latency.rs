//! Latência de digitação: do byte escrito no PTY até o eco voltar.
//!
//! Mede o caminho que a tecla faz sem a GPU (a renderização é igual nos dois
//! casos): tab comum (PTY local) e tab persistente (PTY no daemon, pelo
//! socket). Rodar com:
//!
//! ```text
//! cargo test --test typing_latency -- --ignored --nocapture
//! ```

#![cfg(unix)]

use code_terminal::terminal::pty::Pty;
use std::time::{Duration, Instant};

const SAMPLES: usize = 400;

/// Escreve um caractere por vez e espera o eco (o `cat` em modo canônico
/// não ecoa nada: quem ecoa é o próprio terminal, como num shell).
pub fn measure(pty: &Pty) -> Vec<Duration> {
    // Descarta o que já saiu (prompt, tela do anexo).
    let settle = Instant::now() + Duration::from_millis(300);
    while Instant::now() < settle {
        while pty.try_read().is_some() {}
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut samples = Vec::with_capacity(SAMPLES);
    for i in 0..SAMPLES {
        let c = b'a' + (i % 26) as u8;
        // Linha nova de tempos em tempos: o buffer canônico tem limite.
        let data: &[u8] = if i % 40 == 39 {
            b"\r"
        } else {
            std::slice::from_ref(&c)
        };
        let want = if i % 40 == 39 { b'\n' } else { c };
        let started = Instant::now();
        pty.write(data).unwrap();
        let deadline = started + Duration::from_secs(2);
        loop {
            if let Some(chunk) = pty.try_read() {
                if chunk.contains(&want) {
                    break;
                }
            } else if Instant::now() > deadline {
                panic!("no echo for byte {i}");
            } else {
                std::hint::spin_loop();
            }
        }
        samples.push(started.elapsed());
    }
    samples
}

pub fn report(label: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let pick = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
    let mean = samples.iter().sum::<Duration>() / samples.len() as u32;
    println!(
        "{label:<40} p50 {:>7.1?}  p90 {:>7.1?}  p99 {:>7.1?}  média {:>7.1?}",
        pick(0.5),
        pick(0.9),
        pick(0.99),
        mean
    );
}

#[test]
#[ignore = "medição: rodar com --ignored --nocapture"]
fn typing_latency_local_pty() {
    let pty = Pty::with_command(80, 24, "cat", &[]).unwrap();
    report("tab comum (PTY local)", measure(&pty));
}

/// Daemon e CLI isolados num diretório próprio (não toca no daemon real).
pub fn isolated_runtime() -> tempfile::TempDir {
    let runtime = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", runtime.path());
    std::env::set_var("SHELL", "/bin/sh");
    std::env::set_var("CODE_TERMINAL_MUX_NO_SYSTEMD", "1");
    let cli = std::path::PathBuf::from(env!("CARGO_BIN_EXE_code-terminal-cli"));
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{path}", cli.parent().unwrap().display()),
    );
    runtime
}

/// Como era a tab persistente antes: o PTY da tab roda
/// `code-terminal-cli mux-attach`, que repassa ao daemon (JSON, varredura a
/// cada 8 ms).
#[test]
#[ignore = "medição: rodar com --ignored --nocapture"]
fn typing_latency_cli_attach() {
    let _runtime = isolated_runtime();
    let cli = env!("CARGO_BIN_EXE_code-terminal-cli");
    let args = vec![
        "mux-attach".to_string(),
        "lat".to_string(),
        "--create".to_string(),
    ];
    let pty = Pty::with_command(80, 24, cli, &args).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    report("tab persistente antiga (CLI no PTY)", measure(&pty));
    let _ = pty.write(b"exit\r");
    std::thread::sleep(Duration::from_millis(300));
}

/// Tab persistente nova: PTY no daemon, quadros binários pelo socket.
#[test]
#[ignore = "medição: rodar com --ignored --nocapture"]
fn typing_latency_persistent_tab() {
    let _runtime = isolated_runtime();
    let spec = code_terminal::multiplexer::protocol::StreamCreate {
        command: Some(vec!["cat".into()]),
        env: std::env::vars().collect(),
        ..Default::default()
    };
    let pty = Pty::connect_persistent("lat-nova", 80, 24, Some(spec)).unwrap();
    report("tab persistente nova (PTY no daemon)", measure(&pty));
    pty.end_session().unwrap();
    std::thread::sleep(Duration::from_millis(300));
}
