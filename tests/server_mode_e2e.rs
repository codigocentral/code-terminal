//! Modo servidor de ponta a ponta: `code-terminal --server` de verdade, o
//! cliente pede a janela pelo socket e sai depois do primeiro frame; SIGTERM
//! no servidor apaga o socket; sem servidor o cliente abre a janela sozinho.
//!
//! Abre janelas na tela, então só roda com `CODE_TERMINAL_SERVER_E2E=1`
//! (precisa de Wayland ou X11 e GPU; no CI não há tela).

#![cfg(unix)]

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn enabled() -> bool {
    std::env::var("CODE_TERMINAL_SERVER_E2E").as_deref() == Ok("1")
}

fn app(socket: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_code-terminal"));
    command
        .env("CODE_TERMINAL_SERVER_SOCKET", socket)
        .env_remove("CODE_TERMINAL_NO_SERVER");
    command
}

/// Cliente que sai logo depois do primeiro frame (imprime `first_frame_ms=N`).
fn open_and_measure(socket: &Path) -> (Output, u64) {
    let output = app(socket)
        .env("CODE_TERMINAL_EXIT_AFTER_FIRST_FRAME", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let ms = stdout
        .lines()
        .find_map(|line| line.strip_prefix("first_frame_ms="))
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or_else(|| panic!("no first_frame_ms in {stdout:?}"));
    (output, ms)
}

fn wait_for(mut check: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn test_server_opens_windows_and_cleans_up_on_sigterm() {
    if !enabled() {
        eprintln!("skipped: set CODE_TERMINAL_SERVER_E2E=1 (opens windows)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("server.sock");

    // Sem servidor: o cliente abre a própria janela.
    let (output, alone_ms) = open_and_measure(&socket);
    assert!(output.status.success());
    assert!(!socket.exists());

    let mut server = KillOnDrop(app(&socket).arg("--server").spawn().unwrap());
    let server = &mut server.0;
    assert!(wait_for(|| socket.exists()), "server socket did not appear");
    // A GPU liga em segundo plano; a primeira janela pode esperar por ela.
    let _ = open_and_measure(&socket);

    let mut times = Vec::new();
    for _ in 0..3 {
        let (output, ms) = open_and_measure(&socket);
        assert!(output.status.success());
        times.push(ms);
    }
    let best = *times.iter().min().unwrap();
    eprintln!("sem servidor: {alone_ms} ms; com servidor: {times:?} ms");
    assert!(
        best < alone_ms,
        "server window ({best} ms) should open faster than a new process ({alone_ms} ms)"
    );

    // Janela que fica aberta: o shell dela não pode herdar sinais bloqueados
    // (sem isso o Ctrl+C não chega aos programas e o SIGHUP não fecha o shell).
    assert!(app(&socket).status().unwrap().success());
    let shells = children_of(server.id());
    assert!(!shells.is_empty(), "server window has no shell");
    for pid in shells {
        // O bash bloqueia o SIGINT por um instante enquanto lê o `.bashrc`;
        // herdado do servidor, o bloqueio não sairia nunca.
        assert!(
            wait_for(|| blocked_signals(pid) == 0),
            "shell {pid} kept blocked signals {:#x}",
            blocked_signals(pid)
        );
    }

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(server.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    assert!(wait_for(|| server.try_wait().is_ok_and(|s| s.is_some())));
    assert!(!socket.exists(), "socket left behind after SIGTERM");
}

/// Mata o servidor se o teste falhar no meio (senão ele fica rodando).
struct KillOnDrop(std::process::Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Filhos diretos do processo (todas as threads).
fn children_of(pid: u32) -> Vec<u32> {
    let Ok(tasks) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };
    tasks
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("children")).ok())
        .flat_map(|list| {
            list.split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect::<Vec<u32>>()
        })
        .collect()
}

/// Máscara `SigBlk` de `/proc/<pid>/status`.
fn blocked_signals(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("SigBlk:"))
        .and_then(|hex| u64::from_str_radix(hex.trim(), 16).ok())
        .unwrap_or(u64::MAX)
}
