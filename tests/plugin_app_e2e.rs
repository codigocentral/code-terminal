//! Plugin WASM de verdade rodando dentro do `AppState`: carregado da pasta
//! de plugins, `update` periódico, leitura da tela, escrita no painel,
//! status, aviso e consumo de texto digitado.

#![cfg(unix)]

use code_terminal::app::state::AppState;
use code_terminal::config::Config;
use std::time::{Duration, Instant};

/// Plugin mínimo em WAT:
/// - `update`: lê o tamanho do painel e põe o status "ativo"; na primeira
///   vez escreve "[ola do plugin]" no painel.
/// - `on_input`: texto começando com `!` é consumido e vira aviso "pong".
const PLUGIN_WAT: &str = r#"
(module
  (import "host" "write" (func $write (param i32 i32) (result i32)))
  (import "host" "set_status_text" (func $status (param i32 i32) (result i32)))
  (import "host" "show_notification" (func $notify (param i32 i32 i32) (result i32)))
  (import "host" "get_buffer_size" (func $size (param i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "[ola do plugin]")
  (data (i32.const 16) "ativo")
  (data (i32.const 32) "pong")
  (global $greeted (mut i32) (i32.const 0))
  (func (export "malloc") (param i32) (result i32) (i32.const 1024))
  (func (export "init"))
  (func (export "update") (param i64)
    (drop (call $size (i32.const 512)))
    (drop (call $status (i32.const 16) (i32.const 5)))
    (if (i32.eqz (global.get $greeted))
      (then
        (global.set $greeted (i32.const 1))
        (drop (call $write (i32.const 0) (i32.const 15))))))
  (func (export "on_input") (param $ptr i32) (param $len i32) (result i32)
    (if (result i32) (i32.eq (i32.load8_u (local.get $ptr)) (i32.const 33))
      (then
        (drop (call $notify (i32.const 0) (i32.const 32) (i32.const 4)))
        (i32.const 1))
      (else (i32.const 0))))
  (func (export "shutdown"))
)
"#;

const MANIFEST: &str = r#"entry = "eco.wasm"

[plugin]
name = "eco"
version = "1.0.0"
description = "Plugin de teste"
"#;

fn screen(app: &AppState) -> String {
    app.active_tab_grid()
        .map(|grid| {
            grid.visible_lines()
                .iter()
                .map(|row| row.iter().map(|c| c.char).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[test]
fn test_wasm_plugin_runs_inside_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let plugin_dir = dir.path().join("plugins").join("eco");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(plugin_dir.join("plugin.toml"), MANIFEST).unwrap();
    std::fs::write(
        plugin_dir.join("eco.wasm"),
        wat::parse_str(PLUGIN_WAT).unwrap(),
    )
    .unwrap();

    let mut config = Config::default();
    config.session.restore = false;
    let mut app = AppState::with_command(config, 60, 8, "/bin/cat", &[]).unwrap();
    let loaded = app.enable_plugins(&dir.path().join("plugins"), &dir.path().join("estado"));
    assert_eq!(loaded, 1);
    assert_eq!(app.plugin_names(), vec!["eco".to_string()]);

    // O primeiro `update` sai depois do intervalo; o pedido é aplicado no
    // mesmo `process_output`.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !screen(&app).contains("[ola do plugin]") && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        app.process_output();
    }
    assert!(screen(&app).contains("[ola do plugin]"), "{}", screen(&app));
    assert_eq!(
        app.plugin_statuses(),
        vec![("eco".to_string(), "ativo".to_string())]
    );

    // Texto comum segue para o shell; `!ping` o plugin consome e avisa.
    assert!(!app.plugin_input("ls"));
    assert!(app.plugin_input("!ping"));
    app.process_output();
    let last = app.notifications().history().last().cloned();
    let last = last.expect("aviso do plugin");
    assert_eq!(last.title, "eco");
    assert_eq!(last.body, "pong");
}

#[test]
fn test_missing_plugins_dir_loads_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = AppState::with_command(Config::default(), 40, 5, "/bin/cat", &[]).unwrap();
    assert_eq!(app.enable_plugins(&dir.path().join("nada"), dir.path()), 0);
    assert!(app.plugin_names().is_empty());
    assert!(!app.plugin_input("!ping"));
}
