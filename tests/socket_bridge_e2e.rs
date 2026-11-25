//! Ponta a ponta: cliente do CLI -> Socket API (socket Unix) -> ponte do app
//! -> `AppState` real com PTYs. Fica num binário de teste próprio porque a
//! ponte é global ao processo.

#![cfg(unix)]

use code_terminal::app::api::handle_api_request;
use code_terminal::app::state::AppState;
use code_terminal::cli::client::RemoteClient;
use code_terminal::config::Config;
use code_terminal::multiplexer::socket_api::{
    clear_app_bridge, set_app_bridge, ApiServer, ApiServerConfig,
};
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn test_cli_drives_the_real_app_through_the_socket() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("ct.sock");
    let server = ApiServer::new(
        ApiServerConfig::with_unix_socket(&socket),
        Arc::new(RwLock::new(Config::default())),
    );
    server.start().unwrap();

    let app = Arc::new(Mutex::new(
        AppState::with_command(Config::default(), 80, 24, "/bin/cat", &[]).unwrap(),
    ));
    app.lock().desktop_notifications = false;
    let bridge_app = Arc::clone(&app);
    set_app_bridge(Arc::new(move |method, params| {
        handle_api_request(&mut bridge_app.lock(), method, params)
    }));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut client = runtime
        .block_on(RemoteClient::connect(socket.to_str().unwrap()))
        .unwrap();

    // Lista tabs do app real.
    let tabs = client.list_tabs().unwrap();
    assert_eq!(tabs.len(), 1);
    assert!(tabs[0].is_active);

    // Manda texto e lê de volta (o `cat` ecoa).
    client.send_input("ping from cli\r").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut screen = String::new();
    while Instant::now() < deadline {
        app.lock().process_output();
        screen = client.read_screen(None, None).unwrap();
        if screen.matches("ping from cli").count() >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(screen.contains("ping from cli"), "{screen}");

    // Divide e confere os painéis; notifica o primeiro painel.
    client.split("horizontal").unwrap();
    let panes = client.list_panes().unwrap();
    assert_eq!(panes.len(), 2);
    client.notify("Agent", "needs input").unwrap();
    assert_eq!(app.lock().notifications().history().count(), 1);

    clear_app_bridge();
    server.stop().ok();
}
