//! Tabs persistentes de ponta a ponta, com o daemon de verdade
//! (`code-terminal-cli mux-server`, subido pelo app): o shell sobrevive à
//! janela fechar, a janela nova anexa e recebe a tela e o histórico.
//!
//! Cada teste roda com o daemon e o socket num diretório próprio
//! (`XDG_RUNTIME_DIR`), sem tocar no daemon do usuário, e um de cada vez
//! (eles dividem o daemon do processo de teste).

#![cfg(unix)]

use code_terminal::app::state::AppState;
use code_terminal::config::Config;
use code_terminal::multiplexer::app_client;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// Diretório do daemon dos testes e a vez de cada teste.
fn isolated() -> MutexGuard<'static, tempfile::TempDir> {
    static RUNTIME: OnceLock<Mutex<tempfile::TempDir>> = OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_RUNTIME_DIR", dir.path());
        std::env::set_var("SHELL", "/bin/sh");
        std::env::set_var(app_client::NO_SYSTEMD_ENV, "1");
        let cli = std::path::PathBuf::from(env!("CARGO_BIN_EXE_code-terminal-cli"));
        let path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var(
            "PATH",
            format!("{}:{path}", cli.parent().unwrap().display()),
        );
        Mutex::new(dir)
    });
    runtime.lock().unwrap_or_else(|e| e.into_inner())
}

fn new_app() -> AppState {
    let mut config = Config::default();
    config.session.restore = false;
    let mut app = AppState::with_command(config, 80, 12, "/bin/sh", &[]).unwrap();
    app.desktop_notifications = false;
    app
}

fn rows(grid: &code_terminal::terminal::grid::Grid, history: bool) -> Vec<String> {
    let text = |row: &Vec<code_terminal::terminal::grid::Cell>| {
        row.iter()
            .map(|c| c.char)
            .collect::<String>()
            .trim_end()
            .to_string()
    };
    if history {
        grid.scrollback_lines()
            .iter()
            .map(|row| text(&row))
            .collect()
    } else {
        grid.visible_lines().into_iter().map(text).collect()
    }
}

fn screen(app: &AppState) -> String {
    app.active_tab_grid()
        .map(|grid| rows(grid, false).join("\n"))
        .unwrap_or_default()
}

fn wait_for(app: &mut AppState, text: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        app.process_output();
        if screen(app).contains(text) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

fn wait_until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

fn detached() -> Vec<code_terminal::multiplexer::SessionInfo> {
    app_client::list_app_sessions()
        .into_iter()
        .filter(|s| s.state == "detached")
        .collect()
}

/// Encerra o que sobrou no daemon (o próximo teste começa limpo).
fn cleanup() {
    if let Ok(mut client) = code_terminal::multiplexer::SessionClient::connect() {
        for session in app_client::list_app_sessions() {
            let _ = client.destroy_session(&session.name);
        }
    }
    wait_until(|| app_client::list_app_sessions().is_empty());
}

/// Aba nova persistente: o shell continua vivo depois que o app sai; o app
/// novo traz a aba de volta com a tela, o histórico e o mesmo processo.
#[test]
fn test_persistent_tab_survives_window_and_brings_back_scrollback() {
    let _guard = isolated();
    let mut app = new_app();
    let name = app.open_persistent_tab().unwrap();
    assert_eq!(app.tab_count(), 2);
    assert!(app.is_active_tab_persistent());
    let pane = app.active_tab().unwrap().focused_pane();
    assert_eq!(app.mux_session_of(pane), Some(name.as_str()));
    app.write_to_active_tab(b"export MARCA=viva; seq 1 200; echo fim-$((40+2))\r")
        .unwrap();
    assert!(wait_for(&mut app, "fim-42"), "{}", screen(&app));
    let pid = app
        .active_tab()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .pty()
        .pid();
    app.rename_tab(1, Some("meu servidor"));
    app.sync_persistent_meta(true);

    // "Fecha a janela": a aba comum morre, a persistente só desanexa.
    drop(app);
    assert!(
        wait_until(|| detached().iter().any(|s| s.name == name)),
        "{:?}",
        app_client::list_app_sessions()
    );

    let mut app = new_app();
    assert_eq!(app.attach_detached_persistent_tabs(), 1);
    assert_eq!(app.tab_count(), 2);
    app.go_to_tab(1);
    assert!(app.is_active_tab_persistent());
    assert_eq!(app.active_tab().unwrap().title(), "meu servidor");
    assert!(wait_for(&mut app, "fim-42"), "{}", screen(&app));
    // O histórico (scrollback) voltou inteiro, não só a tela visível.
    let history = rows(app.active_tab_grid().unwrap(), true);
    assert!(history.iter().any(|l| l == "1"), "{history:?}");
    assert!(history.iter().any(|l| l == "150"), "{history:?}");
    // Mesmo processo, mesmo ambiente.
    let buffer = app.active_tab().unwrap().buffer.as_ref().unwrap();
    assert_eq!(buffer.pty().pid(), pid);
    app.write_to_active_tab(b"echo ainda-$MARCA\r").unwrap();
    assert!(wait_for(&mut app, "ainda-viva"), "{}", screen(&app));

    // `exit` encerra: a sessão some do daemon.
    app.write_to_active_tab(b"exit\r").unwrap();
    assert!(wait_until(|| {
        app.process_output();
        app.close_exited_tabs();
        app_client::list_app_sessions().is_empty()
    }));
    assert_eq!(app.tab_count(), 1);
    cleanup();
}

/// "Tornar persistente" numa aba que já roda: o mesmo shell (com o que já
/// tinha) passa para o daemon e sobrevive ao app.
#[cfg(target_os = "linux")]
#[test]
fn test_running_tab_becomes_persistent_without_restarting_the_shell() {
    let _guard = isolated();
    let mut app = new_app();
    app.write_to_active_tab(b"export ANTES=sim; echo pronto-$((1+1))\r")
        .unwrap();
    assert!(wait_for(&mut app, "pronto-2"), "{}", screen(&app));
    let pid = app
        .active_tab()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .pty()
        .pid();
    assert!(!app.is_tab_persistent(0));

    app.make_tab_persistent(0).unwrap();
    assert!(app.is_tab_persistent(0));
    app.write_to_active_tab(b"echo depois-$ANTES\r").unwrap();
    assert!(wait_for(&mut app, "depois-sim"), "{}", screen(&app));
    drop(app);
    assert!(wait_until(|| detached().len() == 1));

    let mut app = new_app();
    assert_eq!(app.attach_detached_persistent_tabs(), 1);
    app.go_to_tab(1);
    // A tela de antes da conversão também volta (veio junto com o PTY).
    assert!(wait_for(&mut app, "pronto-2"), "{}", screen(&app));
    assert_eq!(
        app.active_tab()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .pty()
            .pid(),
        pid
    );
    app.write_to_active_tab(b"echo vivo-$ANTES\r").unwrap();
    assert!(wait_for(&mut app, "vivo-sim"), "{}", screen(&app));
    drop(app);
    cleanup();
}

/// Fechar de propósito pergunta; "Só esconder" deixa a aba viva em segundo
/// plano (não volta sozinha) até ser chamada de volta; "Encerrar" mata.
#[test]
fn test_close_persistent_tab_asks_hide_or_end() {
    let _guard = isolated();
    let mut app = new_app();
    let name = app.open_persistent_tab().unwrap();
    app.write_to_active_tab(b"echo escondida-$((3*3))\r")
        .unwrap();
    assert!(wait_for(&mut app, "escondida-9"), "{}", screen(&app));

    assert!(!app.close_active_tab());
    assert!(app.pending_close_is_persistent());
    assert!(!app.hide_pending_close_tab());
    assert_eq!(app.tab_count(), 1);
    assert!(wait_until(|| detached().iter().any(|s| s.name == name)));
    // Escondida não volta sozinha numa janela nova.
    assert_eq!(app.attach_detached_persistent_tabs(), 0);
    let background = app.background_sessions();
    assert_eq!(background.len(), 1, "{background:?}");
    assert_eq!(background[0].session, name);

    assert_eq!(app.show_all_background_sessions(), 1);
    assert_eq!(app.tab_count(), 2);
    assert!(wait_for(&mut app, "escondida-9"), "{}", screen(&app));

    // Agora encerrar de vez.
    assert!(!app.close_active_tab());
    assert!(!app.confirm_close_pending_tab());
    assert!(wait_until(|| app_client::list_app_sessions().is_empty()));
    cleanup();
}

/// Desmarcar "persistente": a aba volta a morrer com a janela.
#[test]
fn test_unpersisted_tab_dies_with_the_window() {
    let _guard = isolated();
    let mut app = new_app();
    app.open_persistent_tab().unwrap();
    assert!(wait_until(|| app_client::list_app_sessions().len() == 1));
    assert!(!app.toggle_tab_persistent(1).unwrap());
    assert!(!app.is_tab_persistent(1));
    // Fechar a aba agora não pergunta "esconder".
    drop(app);
    assert!(
        wait_until(|| app_client::list_app_sessions().is_empty()),
        "{:?}",
        app_client::list_app_sessions()
    );
    cleanup();
}

/// Aba comum continua como sempre: morre com a janela, nada fica no daemon.
#[test]
fn test_regular_tab_is_not_kept() {
    let _guard = isolated();
    let mut app = new_app();
    app.create_new_tab().unwrap();
    assert!(!app.is_active_tab_persistent());
    drop(app);
    assert!(app_client::list_app_sessions().is_empty());
    cleanup();
}

/// O workspace salvo (modo sem servidor) também anexa as abas persistentes.
#[test]
fn test_workspace_restore_reattaches_persistent_tab() {
    let _guard = isolated();
    let mut app = new_app();
    let name = app.open_persistent_tab().unwrap();
    app.write_to_active_tab(b"echo workspace-ok\r").unwrap();
    assert!(wait_for(&mut app, "workspace-ok"), "{}", screen(&app));
    let workspace = app.capture_workspace();
    let saved = serde_json::to_string(&workspace).unwrap();
    assert!(
        saved.contains(&format!("\"mux_session\":\"{name}\"")),
        "{saved}"
    );
    drop(app);
    assert!(wait_until(|| detached().len() == 1));

    let mut app = new_app();
    app.restore_workspace(&workspace).unwrap();
    assert_eq!(app.attach_detached_persistent_tabs(), 0, "already attached");
    app.go_to_tab(1);
    let pane = app.active_tab().unwrap().focused_pane();
    assert_eq!(app.mux_session_of(pane), Some(name.as_str()));
    assert!(wait_for(&mut app, "workspace-ok"), "{}", screen(&app));
    drop(app);
    cleanup();
}

/// Fechar um painel de uma aba persistente encerra só o shell dele; o
/// painel novo de uma aba persistente também é persistente.
#[test]
fn test_closing_a_pane_of_a_persistent_tab_ends_only_that_pane() {
    let _guard = isolated();
    let mut app = new_app();
    app.open_persistent_tab().unwrap();
    app.split_active_pane(code_terminal::ui::pane_layout::SplitDirection::Vertical)
        .unwrap();
    assert!(wait_until(|| app_client::list_app_sessions().len() == 2));
    let pane = app.active_tab().unwrap().focused_pane();
    let session = app.mux_session_of(pane).unwrap().to_string();
    assert!(!app.close_pane(pane));
    assert!(app.is_active_tab_persistent());
    assert!(
        wait_until(|| app_client::list_app_sessions()
            .iter()
            .all(|s| s.name != session)),
        "{:?}",
        app_client::list_app_sessions()
    );
    assert_eq!(app_client::list_app_sessions().len(), 1);
    drop(app);
    cleanup();
}
