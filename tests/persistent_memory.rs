//! Memória das tabs persistentes: quanto a interface e o daemon gastam com
//! 1 e com 5 tabs cheias (10 000 linhas de histórico cada), comparado com
//! tabs comuns e com o jeito antigo (o `code-terminal-cli mux-attach`
//! rodando dentro do PTY da tab, com um grid inteiro no daemon).
//!
//! Mede PSS e memória privada (`/proc/<pid>/smaps_rollup`) da interface
//! (este processo, sem GPU: o que a GPU gasta não muda com as tabs) e dos
//! processos auxiliares (daemon, CLI). Cada cenário precisa de um processo
//! limpo, então rode um por vez:
//!
//! ```text
//! for t in comum_0 comum_1 comum_3 comum_5 comum_10 antigo_1 antigo_5 novo_1 novo_5; do
//!   cargo test --test persistent_memory -- --ignored --exact memoria_$t --nocapture | grep RESULTADO
//! done
//! ```

#![cfg(target_os = "linux")]

use code_terminal::app::state::AppState;
use code_terminal::config::Config;
use std::time::{Duration, Instant};

/// Linhas por tab (`MEM_LINES` troca: com mais que o histórico, o registro
/// do daemon passa pela compactação).
fn lines() -> usize {
    std::env::var("MEM_LINES")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(10_000)
}
const COLS: u16 = 160;
const ROWS: u16 = 40;

#[derive(Default, Clone, Copy)]
struct Mem {
    pss_kb: u64,
    private_kb: u64,
}

fn rollup(pid: u32) -> Mem {
    let text = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).unwrap_or_default();
    let field = |name: &str| {
        text.lines()
            .find(|line| line.starts_with(name))
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0)
    };
    Mem {
        pss_kb: field("Pss:"),
        private_kb: field("Private_Clean:") + field("Private_Dirty:"),
    }
}

/// Processos deste teste (pelo `XDG_RUNTIME_DIR` isolado) cujo comando
/// contém `needle`.
fn helpers(runtime: &std::path::Path, needle: &str) -> Mem {
    let marker = format!("XDG_RUNTIME_DIR={}", runtime.display());
    let mut total = Mem::default();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        if !String::from_utf8_lossy(&cmdline).contains(needle) {
            continue;
        }
        let environ = std::fs::read(entry.path().join("environ")).unwrap_or_default();
        if !environ
            .split(|&b| b == 0)
            .any(|var| var == marker.as_bytes())
        {
            continue;
        }
        let mem = rollup(pid);
        total.pss_kb += mem.pss_kb;
        total.private_kb += mem.private_kb;
    }
    total
}

fn isolated() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", dir.path());
    std::env::set_var("SHELL", "/bin/sh");
    std::env::set_var("CODE_TERMINAL_MUX_NO_SYSTEMD", "1");
    let cli = std::path::PathBuf::from(env!("CARGO_BIN_EXE_code-terminal-cli"));
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{path}", cli.parent().unwrap().display()),
    );
    dir
}

fn app() -> AppState {
    let mut config = Config::default();
    config.session.restore = false;
    let mut app = AppState::with_command(config, COLS, ROWS, "/bin/sh", &[]).unwrap();
    app.desktop_notifications = false;
    app
}

/// Enche cada tab (menos a primeira) com o histórico e espera terminar.
fn fill(app: &mut AppState, tabs: usize) {
    let command = format!(
        "seq -f 'linha %06g de historico com texto para ocupar boa parte da largura da tab' 1 {}; echo fim-$((6*7))\r",
        lines()
    );
    for index in 1..=tabs {
        app.go_to_tab(index);
        app.write_to_active_tab(command.as_bytes()).unwrap();
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut done = vec![false; tabs + 1];
    while Instant::now() < deadline && done[1..].iter().any(|d| !d) {
        app.process_output();
        for (index, flag) in done.iter_mut().enumerate().skip(1) {
            app.go_to_tab(index);
            let screen: String = app
                .active_tab_grid()
                .map(|grid| {
                    grid.visible_lines()
                        .iter()
                        .map(|row| row.iter().map(|c| c.char).collect::<String>())
                        .collect()
                })
                .unwrap_or_default();
            *flag |= screen.contains("fim-42");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(done[1..].iter().all(|d| *d), "tabs did not finish");
    // Deixa o daemon e o CLI processarem o resto.
    std::thread::sleep(Duration::from_millis(1500));
    app.process_output();
}

fn report(scenario: &str, tabs: usize, runtime: &std::path::Path) {
    let ui = rollup(std::process::id());
    let daemon = helpers(runtime, "mux-server");
    let cli = helpers(runtime, "mux-attach");
    println!(
        "RESULTADO {scenario} tabs={tabs} ui_pss={} ui_priv={} daemon_pss={} daemon_priv={} cli_pss={} cli_priv={}",
        ui.pss_kb, ui.private_kb, daemon.pss_kb, daemon.private_kb, cli.pss_kb, cli.private_kb
    );
}

fn regular(tabs: usize) {
    let runtime = isolated();
    let mut app = app();
    for _ in 0..tabs {
        app.create_new_tab().unwrap();
    }
    fill(&mut app, tabs);
    report("comum", tabs, runtime.path());
}

fn old_cli(tabs: usize) {
    let runtime = isolated();
    let mut app = app();
    for i in 0..tabs {
        let (program, args) = code_terminal::app::state::mux_attach_command(&format!("antiga{i}"));
        app.open_tab_running(&program, &args).unwrap();
    }
    std::thread::sleep(Duration::from_millis(1500));
    fill(&mut app, tabs);
    report("antigo", tabs, runtime.path());
    drop(app);
    if let Ok(mut client) = code_terminal::multiplexer::SessionClient::connect() {
        for session in client.list_sessions().unwrap_or_default() {
            let _ = client.destroy_session(&session.name);
        }
    }
}

/// PIDs dos processos deste teste cujo comando contém `needle`.
fn pids_of(runtime: &std::path::Path, needle: &str) -> Vec<u32> {
    let marker = format!("XDG_RUNTIME_DIR={}", runtime.display());
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            let environ = std::fs::read(entry.path().join("environ")).ok()?;
            (String::from_utf8_lossy(&cmdline).contains(needle)
                && environ.split(|&b| b == 0).any(|v| v == marker.as_bytes()))
            .then_some(pid)
        })
        .collect()
}

/// Trocas de contexto voluntárias de todas as threads (quantas vezes o
/// processo acordou).
fn wakeups(pid: u32) -> u64 {
    std::fs::read_dir(format!("/proc/{pid}/task"))
        .map(|tasks| {
            tasks
                .flatten()
                .filter_map(|task| std::fs::read_to_string(task.path().join("status")).ok())
                .filter_map(|status| {
                    status
                        .lines()
                        .find(|l| l.starts_with("voluntary_ctxt_switches"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|n| n.parse::<u64>().ok())
                })
                .sum()
        })
        .unwrap_or(0)
}

fn new_persistent(tabs: usize) {
    let runtime = isolated();
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("CODE_TERMINAL_STATE_DIR", state.path());
    let mut app = app();
    for _ in 0..tabs {
        app.open_persistent_tab().unwrap();
    }
    fill(&mut app, tabs);
    report("novo", tabs, runtime.path());
    // Parado: depois da gravação (até 30 s após a última mudança), o daemon
    // não deve acordar à toa.
    let daemon_pid = pids_of(runtime.path(), "mux-server")[0];
    if std::env::var_os("MEM_IDLE").is_some() {
        std::thread::sleep(Duration::from_secs(35));
        let before = wakeups(daemon_pid);
        std::thread::sleep(Duration::from_secs(30));
        println!(
            "RESULTADO ocioso tabs={tabs} acordou_em_30s={}",
            wakeups(daemon_pid) - before
        );
    }
    drop(app);
    std::thread::sleep(Duration::from_millis(500));
    // Sem janela: só o daemon segura as tabs.
    let daemon = helpers(runtime.path(), "mux-server");
    println!(
        "RESULTADO novo-sem-janela tabs={tabs} daemon_pss={} daemon_priv={}",
        daemon.pss_kb, daemon.private_kb
    );
    // Desligamento: SIGTERM grava tudo; quanto ocupa em disco por aba.
    let _ = std::process::Command::new("kill")
        .arg(daemon_pid.to_string())
        .status();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !pids_of(runtime.path(), "mux-server").is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let disk: u64 = std::fs::read_dir(state.path().join("tabs"))
        .map(|files| {
            files
                .flatten()
                .filter_map(|f| f.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0);
    println!(
        "RESULTADO disco tabs={tabs} linhas={} total_kb={} por_aba_kb={}",
        lines(),
        disk / 1024,
        disk / 1024 / tabs as u64
    );
}

#[test]
#[ignore = "medição"]
fn memoria_comum_1() {
    regular(1);
}

#[test]
#[ignore = "medição"]
fn memoria_comum_5() {
    regular(5);
}

/// Sem tab cheia: a base para calcular o custo por tab (#1).
#[test]
#[ignore = "medição"]
fn memoria_comum_0() {
    regular(0);
}

#[test]
#[ignore = "medição"]
fn memoria_comum_3() {
    regular(3);
}

#[test]
#[ignore = "medição"]
fn memoria_comum_10() {
    regular(10);
}

#[test]
#[ignore = "medição"]
fn memoria_antigo_1() {
    old_cli(1);
}

#[test]
#[ignore = "medição"]
fn memoria_antigo_5() {
    old_cli(5);
}

#[test]
#[ignore = "medição"]
fn memoria_novo_1() {
    new_persistent(1);
}

#[test]
#[ignore = "medição"]
fn memoria_novo_5() {
    new_persistent(5);
}
