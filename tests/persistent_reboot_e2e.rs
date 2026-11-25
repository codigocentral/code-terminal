//! Abas persistentes depois de "reiniciar a máquina": o daemon grava as abas
//! ao receber SIGTERM, todos os processos morrem, e a janela seguinte as
//! traz de volta com o histórico e o comando para retomar já digitado (só
//! roda sozinho na aba marcada "retomar automaticamente").

#![cfg(target_os = "linux")]

use code_terminal::app::state::AppState;
use code_terminal::config::Config;
use code_terminal::multiplexer::app_client;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

fn new_app() -> AppState {
    let mut config = Config::default();
    config.session.restore = false;
    let mut app = AppState::with_command(config, 100, 20, "/bin/sh", &[]).unwrap();
    app.desktop_notifications = false;
    app
}

fn text(rows: Vec<&Vec<code_terminal::terminal::grid::Cell>>) -> String {
    rows.into_iter()
        .map(|row| {
            row.iter()
                .map(|c| c.char)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tela e histórico da tab `index`.
fn everything(app: &mut AppState, index: usize) -> String {
    app.go_to_tab(index);
    app.active_tab_grid()
        .map(|grid| {
            let lines: Vec<_> = grid.scrollback_lines().iter().collect();
            let history = text(lines.iter().collect());
            format!("{history}\n{}", text(grid.visible_lines()))
        })
        .unwrap_or_default()
}

fn wait_in(app: &mut AppState, index: usize, needle: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        app.process_output();
        if everything(app, index).contains(needle) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
    false
}

/// PIDs dos daemons deste teste (pelo `XDG_RUNTIME_DIR` isolado).
fn daemon_pids(runtime: &Path) -> Vec<i32> {
    let marker = format!("XDG_RUNTIME_DIR={}", runtime.display());
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|entry| {
            let pid: i32 = entry.file_name().to_str()?.parse().ok()?;
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            let environ = std::fs::read(entry.path().join("environ")).ok()?;
            let is_daemon = String::from_utf8_lossy(&cmdline).contains("mux-server");
            let ours = environ.split(|&b| b == 0).any(|v| v == marker.as_bytes());
            (is_daemon && ours).then_some(pid)
        })
        .collect()
}

#[test]
fn test_tabs_come_back_after_reboot_with_history_and_resume_offer() {
    let runtime = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::env::set_var("XDG_RUNTIME_DIR", runtime.path());
    std::env::set_var("CODE_TERMINAL_STATE_DIR", state.path());
    std::env::set_var("SHELL", "/bin/sh");
    std::env::set_var(app_client::NO_SYSTEMD_ENV, "1");
    let cli = std::path::PathBuf::from(env!("CARGO_BIN_EXE_code-terminal-cli"));
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var(
        "PATH",
        format!("{}:{path}", cli.parent().unwrap().display()),
    );

    // Antes: duas abas persistentes. A 1 roda o "Claude Code" (o hook conta
    // a conversa ao daemon); a 2 roda um programa e retoma sozinha.
    let mut app = new_app();
    app.open_persistent_tab().unwrap();
    app.open_persistent_tab().unwrap();
    app.rename_tab(1, Some("claude"));
    app.go_to_tab(1);
    app.write_to_active_tab(
        b"seq 1 80; echo '{\"hook_event_name\":\"SessionStart\",\"session_id\":\"conversa-42\"}' | code-terminal-cli agent-hook --agent 'Claude Code'; echo antes-do-reboot\r",
    )
    .unwrap();
    assert!(
        wait_in(&mut app, 1, "antes-do-reboot"),
        "{}",
        everything(&mut app, 1)
    );
    app.go_to_tab(2);
    app.write_to_active_tab(b"echo segunda-aba; sleep 1000\r")
        .unwrap();
    assert!(wait_in(&mut app, 2, "segunda-aba"));
    std::thread::sleep(Duration::from_millis(1500));
    assert!(app.toggle_tab_auto_resume(2));
    app.sync_persistent_meta(true);

    // "Reinicia": a janela some, o systemd manda SIGTERM ao daemon (que
    // grava tudo) e os processos morrem.
    drop(app);
    let pids = daemon_pids(runtime.path());
    assert_eq!(pids.len(), 1, "{pids:?}");
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pids[0]),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    assert!(wait_until(|| daemon_pids(runtime.path()).is_empty()));
    assert!(!app_client::daemon_running());

    // Em disco: só o usuário lê.
    let tabs = state.path().join("tabs");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&tabs), 0o700);
    let files: Vec<_> = std::fs::read_dir(&tabs)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(files.len(), 4, "{files:?}");
    assert!(files.iter().all(|f| mode(f) == 0o600));

    // Depois: a janela nova sobe o daemon e as abas voltam.
    let mut app = new_app();
    assert_eq!(app.attach_detached_persistent_tabs(), 2);
    assert_eq!(app.tab_count(), 3);
    let titles: Vec<String> = app.tab_manager.tabs().iter().map(|t| t.title()).collect();
    assert!(titles.contains(&"claude".to_string()), "{titles:?}");
    let claude = titles.iter().position(|t| t == "claude").unwrap();
    let other = 3 - claude;

    // Histórico de antes, a marca da reabertura e o comando já digitado.
    assert!(
        wait_in(&mut app, claude, "claude --resume conversa-42"),
        "{}",
        everything(&mut app, claude)
    );
    let screen = everything(&mut app, claude);
    assert!(screen.contains("antes-do-reboot"), "{screen}");
    assert!(screen.contains("aba reaberta"), "{screen}");
    assert!(screen.lines().any(|l| l == "1"), "{screen}");
    // Sem Enter: o shell não tentou rodar (não há "not found").
    std::thread::sleep(Duration::from_millis(500));
    app.process_output();
    assert!(!everything(&mut app, claude).contains("not found"));

    // A aba marcada retoma sozinha: o `sleep 1000` volta a rodar.
    assert!(
        wait_in(&mut app, other, "sleep 1000"),
        "{}",
        everything(&mut app, other)
    );
    assert!(app.tab_auto_resumes(other));
    assert!(wait_until(|| {
        app.tab_manager.tabs()[other]
            .busy_process()
            .is_some_and(|p| p == "sleep")
    }));

    // Encerrar de vez apaga do disco.
    if let Ok(mut client) = code_terminal::multiplexer::SessionClient::connect() {
        for session in app_client::list_app_sessions() {
            let _ = client.destroy_session(&session.name);
        }
    }
    drop(app);
    assert!(wait_until(|| std::fs::read_dir(&tabs).unwrap().count() == 0));
    assert!(wait_until(|| daemon_pids(runtime.path()).is_empty()));
}
