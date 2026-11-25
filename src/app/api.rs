//! Métodos da Socket API aplicados ao estado real da janela.
//!
//! O servidor roda em outra thread; a requisição chega aqui pelo event loop
//! (ver `main.rs`), então este código mexe direto no `AppState`. Os nomes e
//! formatos seguem o que o `code-terminal-cli` já usa (`tab.list`,
//! `tab.create`, `keyboard.send`...) e acrescentam o que agentes precisam:
//! `notification.send`, `terminal.read`, `pane.focus`.

use crate::app::state::AppState;
use crate::multiplexer::socket_api::ApiError;
use crate::ui::pane_layout::{FocusDirection, PaneId, SplitDirection};
use serde_json::{json, Value};

type ApiResult = Result<Value, ApiError>;

/// Métodos que o app atende. Os demais seguem para as APIs antigas.
pub fn handles(method: &str) -> bool {
    matches!(
        method,
        "tab.list"
            | "tab.create"
            | "tab.new"
            | "tab.switch"
            | "tab.select"
            | "tab.close"
            | "tab.rename"
            | "tab.nest"
            | "tab.unnest"
            | "pane.list"
            | "pane.split"
            | "pane.focus"
            | "pane.close"
            | "keyboard.send"
            | "keyboard.text"
            | "input.send"
            | "terminal.read"
            | "notification.send"
            | "notification.list"
            | "notification.jump"
            | "sidebar.toggle"
            | "state.get"
    )
}

/// Executa um método. `None` quando o app não atende o método.
pub fn handle_api_request(app: &mut AppState, method: &str, params: &Value) -> Option<ApiResult> {
    if !handles(method) {
        return None;
    }
    Some(match method {
        "tab.list" => Ok(Value::Array(tab_list(app))),
        "tab.create" | "tab.new" => tab_create(app, params),
        "tab.switch" | "tab.select" => tab_switch(app, params),
        "tab.close" => tab_close(app, params),
        "tab.rename" => tab_rename(app, params),
        "tab.nest" => tab_nest(app, params),
        "tab.unnest" => tab_unnest(app, params),
        "pane.list" => Ok(pane_list(app)),
        "pane.split" => pane_split(app, params),
        "pane.focus" => pane_focus(app, params),
        "pane.close" => pane_close(app, params),
        "keyboard.send" => send_text(app, params, "keys", true),
        "keyboard.text" | "input.send" => send_text(app, params, "text", false),
        "terminal.read" => terminal_read(app, params),
        "notification.send" => notification_send(app, params),
        "notification.list" => Ok(notification_list(app)),
        "notification.jump" => Ok(json!({ "jumped": app.jump_to_unread() })),
        "sidebar.toggle" => {
            app.toggle_sidebar();
            Ok(json!({ "visible": app.is_sidebar_visible() }))
        }
        "state.get" => Ok(state_get(app)),
        _ => Err(ApiError::MethodNotFound(method.to_string())),
    })
}

fn str_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str)
}

fn u64_param(params: &Value, key: &str) -> Option<u64> {
    params.get(key).and_then(Value::as_u64)
}

/// `command` pode ser lista (`["htop"]`) ou texto (rodado com `sh -c`).
fn command_param(params: &Value) -> Option<(String, Vec<String>)> {
    match params.get("command")? {
        Value::String(text) if !text.trim().is_empty() => {
            Some(("sh".to_string(), vec!["-c".to_string(), text.clone()]))
        }
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let (program, args) = parts.split_first()?;
            Some((program.clone(), args.to_vec()))
        }
        _ => None,
    }
}

fn tab_json(app: &AppState, index: usize) -> Option<Value> {
    let tab = app.tab_manager.tabs().get(index)?;
    let info = app.sidebar_info().get(index);
    let cwd = tab
        .buffer
        .as_ref()
        .and_then(|b| b.grid().working_dir().map(|p| p.display().to_string()))
        .or_else(|| info.and_then(|i| i.cwd.clone()));
    Some(json!({
        "id": tab.id,
        "index": index,
        "title": tab.title(),
        "is_active": index == app.active_tab_index(),
        "pid": tab.buffer.as_ref().and_then(|b| b.pty().pid()),
        "cwd": cwd,
        "panes": tab.pane_count(),
        "unread": app.notifications().unread_for_tab(tab.id),
        "custom_title": tab.custom_title(),
        "parent_id": tab.parent(),
        "collapsed": tab.is_collapsed(),
    }))
}

fn tab_list(app: &AppState) -> Vec<Value> {
    (0..app.tab_count())
        .filter_map(|index| tab_json(app, index))
        .collect()
}

fn tab_index(app: &AppState, params: &Value) -> Result<usize, ApiError> {
    if let Some(index) = u64_param(params, "index") {
        let index = index as usize;
        return (index < app.tab_count())
            .then_some(index)
            .ok_or_else(|| ApiError::NotFound(format!("tab index {index}")));
    }
    if let Some(id) = u64_param(params, "id").or_else(|| u64_param(params, "tab_id")) {
        return app
            .tab_manager
            .tabs()
            .iter()
            .position(|tab| u64::from(tab.id) == id)
            .ok_or_else(|| ApiError::NotFound(format!("tab id {id}")));
    }
    Ok(app.active_tab_index())
}

fn tab_create(app: &mut AppState, params: &Value) -> ApiResult {
    let result = match command_param(params) {
        Some((program, args)) => app.open_tab_running(&program, &args),
        None => app.create_new_tab(),
    };
    result.map_err(|e| ApiError::InternalError(e.to_string()))?;
    let index = app.tab_count() - 1;
    app.go_to_tab(index);
    // Título pedido na criação (`cli new-tab --title`) fica fixo.
    if let Some(title) = str_param(params, "title") {
        app.rename_tab(index, Some(title));
    }
    tab_json(app, index).ok_or_else(|| ApiError::InternalError("tab vanished".into()))
}

/// `tab.rename`: título fixo da tab (`index`/`id`, padrão a ativa). `title`
/// vazio ou nulo volta ao título automático.
fn tab_rename(app: &mut AppState, params: &Value) -> ApiResult {
    let index = tab_index(app, params)?;
    let title = match params.get("title") {
        Some(Value::String(title)) => Some(title.as_str()),
        Some(Value::Null) | None => None,
        Some(_) => return Err(ApiError::InvalidParams("'title' must be a string".into())),
    };
    app.rename_tab(index, title);
    tab_json(app, index).ok_or_else(|| ApiError::InternalError("tab vanished".into()))
}

/// Índice da tab de cima pedida em `parent_index`/`parent_id`.
fn parent_param(app: &AppState, params: &Value) -> Result<usize, ApiError> {
    let index = if let Some(index) = u64_param(params, "parent_index") {
        Some(index as usize).filter(|&index| index < app.tab_count())
    } else if let Some(id) = u64_param(params, "parent_id") {
        app.tab_manager
            .tabs()
            .iter()
            .position(|tab| u64::from(tab.id) == id)
    } else {
        return Err(ApiError::InvalidParams(
            "Missing 'parent_index' or 'parent_id'".into(),
        ));
    };
    index.ok_or_else(|| ApiError::NotFound("parent tab".into()))
}

/// `tab.nest`: põe a tab (`index`/`id`) no grupo da tab de cima pedida.
fn tab_nest(app: &mut AppState, params: &Value) -> ApiResult {
    let index = tab_index(app, params)?;
    let parent = parent_param(app, params)?;
    let id = app.tab_manager.tabs()[index].id;
    let changed = app.nest_tab(index, parent);
    Ok(json!({ "changed": changed, "tabs": tab_list(app), "id": id }))
}

/// `tab.unnest`: tira a tab (`index`/`id`) do grupo.
fn tab_unnest(app: &mut AppState, params: &Value) -> ApiResult {
    let index = tab_index(app, params)?;
    let id = app.tab_manager.tabs()[index].id;
    let changed = app.unnest_tab(index);
    Ok(json!({ "changed": changed, "tabs": tab_list(app), "id": id }))
}

fn tab_switch(app: &mut AppState, params: &Value) -> ApiResult {
    let index = tab_index(app, params)?;
    app.go_to_tab(index);
    Ok(json!({ "index": index }))
}

fn tab_close(app: &mut AppState, params: &Value) -> ApiResult {
    // Pela API não há quem confirme: fecha direto.
    let index = tab_index(app, params)?;
    let last = app.force_close_tab(index);
    Ok(json!({ "closed": index, "window_empty": last }))
}

fn pane_list(app: &AppState) -> Value {
    let Some(tab) = app.active_tab() else {
        return json!([]);
    };
    let focused = tab.focused_pane();
    let panes: Vec<Value> = tab
        .layout()
        .content_rects(app.pane_area())
        .into_iter()
        .enumerate()
        .map(|(index, (id, rect))| {
            json!({
                "id": id,
                "index": index,
                "is_active": id == focused,
                "cols": rect.cols,
                "rows": rect.rows,
                "unread": app.notifications().has_unread(id),
            })
        })
        .collect();
    Value::Array(panes)
}

fn split_direction(params: &Value) -> SplitDirection {
    // O CLI manda {"params": {"direction": ...}}; aceita também direto.
    let direction = str_param(params, "direction")
        .or_else(|| params.get("params").and_then(|p| str_param(p, "direction")))
        .unwrap_or("horizontal");
    match direction.to_ascii_lowercase().as_str() {
        "vertical" | "v" | "down" | "below" => SplitDirection::Vertical,
        _ => SplitDirection::Horizontal,
    }
}

fn pane_split(app: &mut AppState, params: &Value) -> ApiResult {
    let direction = split_direction(params);
    let result = match command_param(params) {
        Some((program, args)) => app.split_active_pane_running(direction, &program, &args),
        None => app.split_active_pane(direction),
    };
    result.map_err(|e| ApiError::InternalError(e.to_string()))?;
    let pane = app.active_tab().map(|tab| tab.focused_pane());
    Ok(json!({ "pane": pane }))
}

fn pane_focus(app: &mut AppState, params: &Value) -> ApiResult {
    let moved = if let Some(id) = u64_param(params, "id") {
        app.focus_pane(id)
    } else {
        let direction = match str_param(params, "direction").unwrap_or("") {
            "left" => FocusDirection::Left,
            "right" => FocusDirection::Right,
            "up" => FocusDirection::Up,
            "down" => FocusDirection::Down,
            "next" => return Ok(json!({ "focused": app.cycle_pane(true) })),
            "previous" | "prev" => return Ok(json!({ "focused": app.cycle_pane(false) })),
            other => {
                return Err(ApiError::InvalidParams(format!(
                    "need 'id' or 'direction' (left/right/up/down/next/previous), got '{other}'"
                )))
            }
        };
        app.focus_pane_direction(direction)
    };
    Ok(json!({ "focused": moved }))
}

fn pane_close(app: &mut AppState, params: &Value) -> ApiResult {
    if let Some(id) = u64_param(params, "id") {
        if !app.focus_pane(id) {
            return Err(ApiError::NotFound(format!("pane {id}")));
        }
    }
    let last = app.close_active_pane();
    Ok(json!({ "window_empty": last }))
}

/// `\n`/`\r` viram Enter e `\t` vira Tab (como o `keyboard.send` antigo).
fn unescape_keys(keys: &str) -> String {
    let mut out = String::with_capacity(keys.len());
    let mut chars = keys.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') | Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Painel alvo: `pane` (id) na tab ativa ou em qualquer tab; senão o foco.
fn target_pane(app: &AppState, params: &Value) -> Result<(usize, PaneId), ApiError> {
    // Aba persistente: a sessão do mux vale mais que o id do painel (que
    // muda a cada janela e fica velho no ambiente do shell).
    let by_session = str_param(params, "mux_session").and_then(|s| app.pane_of_mux_session(s));
    let pane = by_session
        .or_else(|| u64_param(params, "pane"))
        .or_else(|| u64_param(params, "pane_id"));
    match pane {
        Some(id) => app
            .tab_manager
            .tabs()
            .iter()
            .position(|tab| tab.pane_ids().contains(&id))
            .map(|index| (index, id))
            .ok_or_else(|| ApiError::NotFound(format!("pane {id}"))),
        None => {
            let index = tab_index(app, params)?;
            let tab = app
                .tab_manager
                .tabs()
                .get(index)
                .ok_or_else(|| ApiError::NotFound("no tab".into()))?;
            Ok((index, tab.focused_pane()))
        }
    }
}

fn send_text(app: &mut AppState, params: &Value, key: &str, escapes: bool) -> ApiResult {
    let text = str_param(params, key)
        .ok_or_else(|| ApiError::InvalidParams(format!("missing '{key}'")))?;
    let data = if escapes {
        unescape_keys(text)
    } else {
        text.to_string()
    };
    let (index, pane) = target_pane(app, params)?;
    let buffer = app
        .tab_manager
        .get_tab_mut(index)
        .and_then(|tab| tab.pane_buffer_mut(pane))
        .ok_or_else(|| ApiError::NotFound(format!("pane {pane}")))?;
    buffer
        .write(data.as_bytes())
        .map_err(|e| ApiError::InternalError(e.to_string()))?;
    Ok(json!({ "pane": pane, "bytes": data.len() }))
}

fn terminal_read(app: &mut AppState, params: &Value) -> ApiResult {
    let (index, pane) = target_pane(app, params)?;
    let lines = u64_param(params, "lines").unwrap_or(0) as usize;
    let tab = app
        .tab_manager
        .tabs()
        .get(index)
        .ok_or_else(|| ApiError::NotFound("no tab".into()))?;
    let buffer = tab
        .pane_buffer(pane)
        .ok_or_else(|| ApiError::NotFound(format!("pane {pane}")))?;
    let mut rows: Vec<String> = buffer
        .grid()
        .visible_lines()
        .iter()
        .map(|row| {
            row.iter()
                .filter(|cell| !cell.spacer)
                .map(|cell| if cell.char == '\0' { ' ' } else { cell.char })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    if lines > 0 && rows.len() > lines {
        rows.drain(..rows.len() - lines);
    }
    let (row, col) = buffer.grid().cursor_position();
    Ok(json!({ "pane": pane, "text": rows.join("\n"), "cursor": [row, col] }))
}

fn notification_send(app: &mut AppState, params: &Value) -> ApiResult {
    let title = str_param(params, "title").unwrap_or("").to_string();
    let body = str_param(params, "body")
        .or_else(|| str_param(params, "message"))
        .unwrap_or("")
        .to_string();
    if title.is_empty() && body.is_empty() {
        return Err(ApiError::InvalidParams("need 'title' or 'body'".into()));
    }
    let (_, pane) = target_pane(app, params)?;
    app.notify_pane(pane, &title, &body);
    Ok(json!({ "pane": pane }))
}

fn notification_list(app: &AppState) -> Value {
    let entries: Vec<Value> = app
        .notifications()
        .history()
        .take(50)
        .map(|entry| {
            json!({
                "tab_id": entry.tab_id,
                "pane": entry.pane,
                "title": entry.title,
                "body": entry.body,
                "bell": entry.bell,
                "unread": app.notifications().has_unread(entry.pane),
            })
        })
        .collect();
    json!({ "unread": app.notifications().total_unread(), "notifications": entries })
}

fn state_get(app: &AppState) -> Value {
    let (cols, rows) = app.terminal_size;
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "tabs": tab_list(app),
        "active_tab": app.active_tab().map(|tab| tab.id).unwrap_or(0),
        "terminal_size": { "cols": cols, "rows": rows },
        "sidebar_visible": app.is_sidebar_visible(),
        "unread": app.notifications().total_unread(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::config::Config;

    #[cfg(unix)]
    fn app() -> AppState {
        AppState::with_command(
            Config::default(),
            81,
            24,
            "/bin/sh",
            &["-c".into(), "sleep 30".into()],
        )
        .unwrap()
    }

    #[cfg(unix)]
    fn call(app: &mut AppState, method: &str, params: Value) -> Value {
        handle_api_request(app, method, &params)
            .expect("método atendido")
            .unwrap_or_else(|e| panic!("{method}: {e}"))
    }

    #[cfg(unix)]
    #[test]
    fn test_unhandled_methods_fall_through() {
        let mut app = app();
        assert!(handle_api_request(&mut app, "workspace.list", &json!({})).is_none());
        assert!(handles("tab.list"));
    }

    #[cfg(unix)]
    #[test]
    fn test_tabs_create_list_switch_close() {
        let mut app = app();
        let created = call(
            &mut app,
            "tab.create",
            json!({"command": ["sh", "-c", "sleep 30"]}),
        );
        assert_eq!(created["index"], 1);
        assert_eq!(created["is_active"], true);
        let tabs = call(&mut app, "tab.list", json!(null));
        assert_eq!(tabs.as_array().unwrap().len(), 2);
        call(&mut app, "tab.switch", json!({"index": 0}));
        assert_eq!(app.active_tab_index(), 0);
        let id = tabs[1]["id"].as_u64().unwrap();
        call(&mut app, "tab.close", json!({"id": id}));
        assert_eq!(app.tab_count(), 1);
        let err = handle_api_request(&mut app, "tab.switch", &json!({"index": 9})).unwrap();
        assert!(matches!(err, Err(ApiError::NotFound(_))));
    }

    #[cfg(unix)]
    #[test]
    fn test_tab_rename_and_groups() {
        let mut app = app();
        let created = call(
            &mut app,
            "tab.create",
            json!({"command": "sleep 30", "title": "logs"}),
        );
        assert_eq!(created["title"], "logs");
        assert_eq!(created["custom_title"], "logs");
        let renamed = call(&mut app, "tab.rename", json!({"index": 0, "title": "api"}));
        assert_eq!(renamed["title"], "api");
        let id0 = renamed["id"].as_u64().unwrap();

        let nested = call(&mut app, "tab.nest", json!({"index": 1, "parent_id": id0}));
        assert_eq!(nested["changed"], true);
        assert_eq!(nested["tabs"][1]["parent_id"], id0);
        let again = call(&mut app, "tab.nest", json!({"index": 1, "parent_index": 0}));
        assert_eq!(again["changed"], false);
        let out = call(&mut app, "tab.unnest", json!({"index": 1}));
        assert_eq!(out["tabs"][1]["parent_id"], Value::Null);

        // Título nulo volta ao automático.
        let cleared = call(&mut app, "tab.rename", json!({"index": 0, "title": null}));
        assert_eq!(cleared["custom_title"], Value::Null);
        assert_ne!(cleared["title"], "api");
        for (method, params) in [
            ("tab.rename", json!({"title": 3})),
            ("tab.nest", json!({"index": 0})),
            ("tab.nest", json!({"index": 0, "parent_index": 9})),
            ("tab.unnest", json!({"index": 9})),
        ] {
            let result = handle_api_request(&mut app, method, &params).unwrap();
            assert!(result.is_err(), "{method} {params}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_panes_split_focus_list_close() {
        let mut app = app();
        call(
            &mut app,
            "pane.split",
            json!({"params": {"direction": "vertical"}, "command": "sleep 30"}),
        );
        let panes = call(&mut app, "pane.list", json!(null));
        let panes = panes.as_array().unwrap();
        assert_eq!(panes.len(), 2);
        assert_eq!(
            panes[0]["rows"].as_u64().unwrap() + panes[1]["rows"].as_u64().unwrap(),
            // 24 linhas menos o separador e a barra de título de cada painel (#32).
            21
        );
        assert_eq!(panes[1]["is_active"], true);
        let focused = call(&mut app, "pane.focus", json!({"direction": "up"}));
        assert_eq!(focused["focused"], true);
        assert_eq!(
            app.active_tab().unwrap().focused_pane(),
            panes[0]["id"].as_u64().unwrap()
        );
        assert!(handle_api_request(&mut app, "pane.focus", &json!({}))
            .unwrap()
            .is_err());
        call(&mut app, "pane.close", json!({}));
        assert_eq!(app.active_tab().unwrap().pane_count(), 1);
    }

    /// Agente manda texto e lê a tela de volta.
    #[cfg(unix)]
    #[test]
    fn test_send_and_read_terminal() {
        let mut app = AppState::with_command(Config::default(), 40, 10, "/bin/cat", &[]).unwrap();
        call(&mut app, "keyboard.send", json!({"keys": "hello\\n"}));
        call(&mut app, "keyboard.text", json!({"text": "world\r"}));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut text = String::new();
        while std::time::Instant::now() < deadline {
            app.process_output();
            text = call(&mut app, "terminal.read", json!({}))["text"]
                .as_str()
                .unwrap()
                .to_string();
            if text.matches("world").count() >= 2 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            text.contains("hello") && text.matches("world").count() >= 2,
            "{text}"
        );
        let last = call(&mut app, "terminal.read", json!({"lines": 1}));
        assert_eq!(last["text"].as_str().unwrap().lines().count(), 1);
        assert!(handle_api_request(&mut app, "keyboard.send", &json!({}))
            .unwrap()
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_notifications_via_api_target_the_pane() {
        let mut app = app();
        let first = app.active_tab().unwrap().focused_pane();
        call(
            &mut app,
            "pane.split",
            json!({"direction": "horizontal", "command": "sleep 30"}),
        );
        call(
            &mut app,
            "notification.send",
            json!({"title": "Claude", "body": "done", "pane": first}),
        );
        assert!(app.notifications().has_unread(first));
        let list = call(&mut app, "notification.list", json!(null));
        assert_eq!(list["unread"], 1);
        assert_eq!(list["notifications"][0]["title"], "Claude");
        let state = call(&mut app, "state.get", json!(null));
        assert_eq!(state["unread"], 1);
        assert_eq!(state["tabs"][0]["panes"], 2);
        assert_eq!(
            call(&mut app, "notification.jump", json!(null))["jumped"],
            true
        );
        assert!(
            handle_api_request(&mut app, "notification.send", &json!({}))
                .unwrap()
                .is_err()
        );
        assert!(handle_api_request(
            &mut app,
            "notification.send",
            &json!({"body": "x", "pane": 999_999})
        )
        .unwrap()
        .is_err());
    }

    #[test]
    fn test_unescape_keys() {
        assert_eq!(unescape_keys("ls\\n"), "ls\r");
        assert_eq!(unescape_keys("a\\tb\\e\\\\\\x"), "a\tb\x1b\\\\x");
        assert_eq!(unescape_keys("end\\"), "end\\");
    }
}
