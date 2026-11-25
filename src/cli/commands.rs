//! CLI Commands Implementation
//!
//! Cada comando público escreve na saída padrão; a lógica fica em funções
//! `*_to` que recebem o cliente, as dependências e um `Write` de saída, para
//! poderem ser testadas com servidores falsos e buffers em memória.

use crate::cli::client::{RemoteClient, TabInfo};
use crate::cli::error::{CliError, CliResult};
use crate::cli::{ConfigAction, OutputFormat, SplitDirection};
use crate::multiplexer::{MultiplexerError, SessionClient, SessionInfo, SessionPersistence};
use crate::ssh::{SessionManager, SshConfigParser, SshHost, SshResult};
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// Corta `s` em no máximo `max` bytes sem partir um caractere UTF-8 ao meio.
fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn to_yaml<T: serde::Serialize + ?Sized>(value: &T) -> CliResult<String> {
    serde_yaml::to_string(value).map_err(|e| CliError::Serialization(e.to_string()))
}

fn mux_err(e: MultiplexerError) -> CliError {
    CliError::Connection {
        message: e.to_string(),
    }
}

fn connect_mux() -> CliResult<SessionClient> {
    SessionClient::connect().map_err(mux_err)
}

/// Conecta ao daemon, iniciando-o se ainda não estiver rodando.
fn connect_mux_starting_daemon() -> CliResult<SessionClient> {
    if let Ok(client) = SessionClient::connect() {
        return Ok(client);
    }
    start_mux_daemon()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match SessionClient::connect() {
            Ok(client) => return Ok(client),
            Err(e) if std::time::Instant::now() >= deadline => return Err(mux_err(e)),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// Sobe `code-terminal-cli mux-server` desligado deste terminal: grupo de
/// processos próprio e sem stdio, para sobreviver ao fechamento da janela.
#[cfg(unix)]
fn start_mux_daemon() -> CliResult<()> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("mux-server")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    Ok(())
}

#[cfg(not(unix))]
fn start_mux_daemon() -> CliResult<()> {
    Err(CliError::Connection {
        message: "the multiplexer daemon is not available on this platform yet".into(),
    })
}

/// Roda o daemon do multiplexer em primeiro plano até a última sessão sair.
#[cfg(unix)]
pub async fn mux_server(keep_alive: bool) -> CliResult<()> {
    use crate::multiplexer::daemon::MuxDaemon;
    use crate::multiplexer::tab_store::{default_dir, TabStore};
    use crate::multiplexer::transport::default_mux_socket_path;
    use std::sync::Arc;
    let config = crate::config::Config::load().unwrap_or_default();
    // Abas persistentes em disco (voltam depois do reboot), salvo se a
    // configuração desligar: aí o que houver é apagado.
    let store = match default_dir() {
        Some(dir) if config.session.save_scrollback => match TabStore::open(dir) {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!(error = %e, "Cannot save persistent tabs to disk");
                None
            }
        },
        Some(dir) => {
            TabStore::clear(&dir);
            None
        }
        None => None,
    };
    let daemon = Arc::new(
        MuxDaemon::new(default_mux_socket_path(), config)
            .with_keep_alive(keep_alive)
            .with_store(store),
    );
    let listener = daemon.bind()?;
    // SIGTERM (desligar a máquina, parar o serviço): grava as abas e sai.
    let on_signal = Arc::clone(&daemon);
    tokio::spawn(async move {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut term), Ok(mut int), Ok(mut hup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
            signal(SignalKind::hangup()),
        ) else {
            return;
        };
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
            _ = hup.recv() => {}
        }
        on_signal.shutdown();
    });
    tokio::task::spawn_blocking(move || daemon.run(listener))
        .await
        .map_err(|e| CliError::Connection {
            message: e.to_string(),
        })??;
    Ok(())
}

#[cfg(not(unix))]
pub async fn mux_server(_keep_alive: bool) -> CliResult<()> {
    start_mux_daemon()
}

/// List all tabs
pub async fn list_tabs(
    client: &mut RemoteClient,
    long: bool,
    format: OutputFormat,
) -> CliResult<()> {
    list_tabs_to(client, long, format, &mut std::io::stdout())
}

fn list_tabs_to(
    client: &mut RemoteClient,
    long: bool,
    format: OutputFormat,
    out: &mut dyn Write,
) -> CliResult<()> {
    let tabs = client.list_tabs()?;

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(&tabs)?)?;
        }
        OutputFormat::Yaml => {
            writeln!(out, "{}", to_yaml(&tabs)?)?;
        }
        OutputFormat::Text => write_tabs_text(&tabs, long, out)?,
    }

    Ok(())
}

/// Saída em texto de `list-tabs` (curta ou, com `long`, em tabela)
fn write_tabs_text(tabs: &[TabInfo], long: bool, out: &mut dyn Write) -> CliResult<()> {
    if tabs.is_empty() {
        writeln!(out, "No tabs")?;
        return Ok(());
    }

    if long {
        write_tabs_long(tabs, out)
    } else {
        write_tabs_short(tabs, out)
    }
}

fn write_tabs_long(tabs: &[TabInfo], out: &mut dyn Write) -> CliResult<()> {
    writeln!(
        out,
        "{:<5} {:<8} {:<10} {:<20} Title",
        "Index", "Active", "PID", "CWD"
    )?;
    writeln!(out, "{}", "-".repeat(80))?;
    for tab in tabs {
        let active = if tab.is_active { "*" } else { "" };
        let pid = tab
            .pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        let cwd = tab.cwd.as_deref().unwrap_or("-");
        let cwd_short = truncate_str(cwd, 18);
        writeln!(
            out,
            "{:<5} {:<8} {:<10} {:<20} {}",
            tab.index, active, pid, cwd_short, tab.title
        )?;
    }
    Ok(())
}

fn write_tabs_short(tabs: &[TabInfo], out: &mut dyn Write) -> CliResult<()> {
    for tab in tabs {
        let marker = if tab.is_active { "* " } else { "  " };
        writeln!(out, "{}{}: {}", marker, tab.index, tab.title)?;
    }
    Ok(())
}

/// Create a new tab
pub async fn new_tab(
    client: &mut RemoteClient,
    title: Option<String>,
    cwd: Option<PathBuf>,
    command: Vec<String>,
) -> CliResult<()> {
    new_tab_to(client, title, cwd, command, &mut std::io::stdout())
}

fn new_tab_to(
    client: &mut RemoteClient,
    title: Option<String>,
    cwd: Option<PathBuf>,
    command: Vec<String>,
    out: &mut dyn Write,
) -> CliResult<()> {
    let index = client.new_tab(title.as_deref(), cwd.as_deref(), command)?;
    writeln!(out, "Created tab {}", index)?;
    Ok(())
}

/// Close a tab
pub async fn close_tab(client: &mut RemoteClient, index: usize) -> CliResult<()> {
    close_tab_to(client, index, &mut std::io::stdout())
}

fn close_tab_to(client: &mut RemoteClient, index: usize, out: &mut dyn Write) -> CliResult<()> {
    client.close_tab(index)?;
    writeln!(out, "Closed tab {}", index)?;
    Ok(())
}

/// Rename a tab; no title (or an empty one) restores the automatic title
pub async fn rename_tab(
    client: &mut RemoteClient,
    index: usize,
    title: Option<String>,
) -> CliResult<()> {
    rename_tab_to(client, index, title, &mut std::io::stdout())
}

fn rename_tab_to(
    client: &mut RemoteClient,
    index: usize,
    title: Option<String>,
    out: &mut dyn Write,
) -> CliResult<()> {
    let title = title.filter(|title| !title.trim().is_empty());
    client.rename_tab(index, title.as_deref())?;
    match title {
        Some(title) => writeln!(out, "Renamed tab {} to '{}'", index, title.trim())?,
        None => writeln!(out, "Tab {} uses the automatic title again", index)?,
    }
    Ok(())
}

/// Switch to a tab
pub async fn goto_tab(client: &mut RemoteClient, index: usize) -> CliResult<()> {
    goto_tab_to(client, index, &mut std::io::stdout())
}

fn goto_tab_to(client: &mut RemoteClient, index: usize, out: &mut dyn Write) -> CliResult<()> {
    client.goto_tab(index)?;
    writeln!(out, "Switched to tab {}", index)?;
    Ok(())
}

/// Send keys to active tab
pub async fn send_keys(client: &mut RemoteClient, keys: &str) -> CliResult<()> {
    // Parse special keys
    let parsed = parse_keys(keys);
    client.send_keys(&parsed)?;
    Ok(())
}

/// Send text input to active tab
pub async fn send_input(client: &mut RemoteClient, text: &str) -> CliResult<()> {
    client.send_input(text)?;
    Ok(())
}

/// Get terminal size
pub async fn get_size(client: &mut RemoteClient, format: OutputFormat) -> CliResult<()> {
    get_size_to(client, format, &mut std::io::stdout())
}

fn get_size_to(
    client: &mut RemoteClient,
    format: OutputFormat,
    out: &mut dyn Write,
) -> CliResult<()> {
    let (cols, rows) = client.get_size()?;

    match format {
        OutputFormat::Json => {
            writeln!(out, "{{\"cols\": {}, \"rows\": {}}}", cols, rows)?;
        }
        _ => {
            writeln!(out, "{}x{}", cols, rows)?;
        }
    }

    Ok(())
}

/// Set terminal size
pub async fn set_size(client: &mut RemoteClient, cols: u16, rows: u16) -> CliResult<()> {
    set_size_to(client, cols, rows, &mut std::io::stdout())
}

fn set_size_to(
    client: &mut RemoteClient,
    cols: u16,
    rows: u16,
    out: &mut dyn Write,
) -> CliResult<()> {
    client.set_size(cols, rows)?;
    writeln!(out, "Set size to {}x{}", cols, rows)?;
    Ok(())
}

/// Show terminal info
pub async fn info(client: &mut RemoteClient, format: OutputFormat) -> CliResult<()> {
    info_to(client, format, &mut std::io::stdout())
}

fn info_to(client: &mut RemoteClient, format: OutputFormat, out: &mut dyn Write) -> CliResult<()> {
    let info = client.info()?;

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(&info)?)?;
        }
        OutputFormat::Yaml => {
            writeln!(out, "{}", to_yaml(&info)?)?;
        }
        OutputFormat::Text => {
            writeln!(out, "Version: {}", info.version)?;
            writeln!(
                out,
                "Tabs: {} (active: {})",
                info.tab_count, info.active_tab
            )?;
            writeln!(out, "Size: {}x{}", info.size.cols, info.size.rows)?;
        }
    }

    Ok(())
}

/// Send notification
pub async fn notify(client: &mut RemoteClient, title: &str, message: &str) -> CliResult<()> {
    notify_to(client, title, message, &mut std::io::stdout())
}

fn notify_to(
    client: &mut RemoteClient,
    title: &str,
    message: &str,
    out: &mut dyn Write,
) -> CliResult<()> {
    client.notify(title, message)?;
    writeln!(out, "Notification sent")?;
    Ok(())
}

/// List all panes
pub async fn list_panes(client: &mut RemoteClient, format: OutputFormat) -> CliResult<()> {
    list_panes_to(client, format, &mut std::io::stdout())
}

fn list_panes_to(
    client: &mut RemoteClient,
    format: OutputFormat,
    out: &mut dyn Write,
) -> CliResult<()> {
    let panes = client.list_panes()?;

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(&panes)?)?;
        }
        _ => {
            if panes.is_empty() {
                writeln!(out, "No panes")?;
                return Ok(());
            }
            for pane in &panes {
                let marker = if pane.is_active { "* " } else { "  " };
                writeln!(out, "{}{}: pane-{}", marker, pane.index, pane.id)?;
            }
        }
    }

    Ok(())
}

/// Split current pane
pub async fn split(client: &mut RemoteClient, direction: SplitDirection) -> CliResult<()> {
    split_to(client, direction, &mut std::io::stdout())
}

fn split_to(
    client: &mut RemoteClient,
    direction: SplitDirection,
    out: &mut dyn Write,
) -> CliResult<()> {
    let dir = match direction {
        SplitDirection::Horizontal => "horizontal",
        SplitDirection::Vertical => "vertical",
    };
    client.split(dir)?;
    writeln!(out, "Split pane {}", dir)?;
    Ok(())
}

/// Config management
pub async fn config(client: &mut RemoteClient, action: ConfigAction) -> CliResult<()> {
    match action {
        ConfigAction::Show => {
            // This is handled in main.rs without connection
            unreachable!()
        }
        ConfigAction::Get { key } => {
            let value = client.get_config(&key)?;
            println!("{} = {}", key, value);
        }
        ConfigAction::Set { key, value } => {
            client.set_config(&key, &value)?;
            println!("Set {} = {}", key, value);
        }
        ConfigAction::Reset => {
            client.reset_config()?;
            println!("Configuration reset to defaults");
        }
    }
    Ok(())
}

/// Parse special keys notation
fn parse_keys(input: &str) -> String {
    input
        // Escape sequences
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
        .replace("\\\\", "\\")
        // Special keys
        .replace("<Enter>", "\r")
        .replace("<Return>", "\r")
        .replace("<Tab>", "\t")
        .replace("<Esc>", "\x1b")
        .replace("<Escape>", "\x1b")
        .replace("<Space>", " ")
        .replace("<Backspace>", "\x7f")
        .replace("<BS>", "\x7f")
        .replace("<Delete>", "\x1b[3~")
        .replace("<Del>", "\x1b[3~")
        .replace("<Insert>", "\x1b[2~")
        .replace("<Ins>", "\x1b[2~")
        .replace("<Home>", "\x1b[H")
        .replace("<End>", "\x1b[F")
        .replace("<PageUp>", "\x1b[5~")
        .replace("<PageDown>", "\x1b[6~")
        // Arrow keys
        .replace("<Up>", "\x1b[A")
        .replace("<Down>", "\x1b[B")
        .replace("<Right>", "\x1b[C")
        .replace("<Left>", "\x1b[D")
        // Function keys
        .replace("<F1>", "\x1bOP")
        .replace("<F2>", "\x1bOQ")
        .replace("<F3>", "\x1bOR")
        .replace("<F4>", "\x1bOS")
        .replace("<F5>", "\x1b[15~")
        .replace("<F6>", "\x1b[17~")
        .replace("<F7>", "\x1b[18~")
        .replace("<F8>", "\x1b[19~")
        .replace("<F9>", "\x1b[20~")
        .replace("<F10>", "\x1b[21~")
        .replace("<F11>", "\x1b[23~")
        .replace("<F12>", "\x1b[24~")
        // Ctrl+key combinations
        .replace("<C-a>", "\x01")
        .replace("<C-b>", "\x02")
        .replace("<C-c>", "\x03")
        .replace("<C-d>", "\x04")
        .replace("<C-e>", "\x05")
        .replace("<C-f>", "\x06")
        .replace("<C-g>", "\x07")
        .replace("<C-h>", "\x08")
        .replace("<C-i>", "\x09")
        .replace("<C-j>", "\x0a")
        .replace("<C-k>", "\x0b")
        .replace("<C-l>", "\x0c")
        .replace("<C-m>", "\x0d")
        .replace("<C-n>", "\x0e")
        .replace("<C-o>", "\x0f")
        .replace("<C-p>", "\x10")
        .replace("<C-q>", "\x11")
        .replace("<C-r>", "\x12")
        .replace("<C-s>", "\x13")
        .replace("<C-t>", "\x14")
        .replace("<C-u>", "\x15")
        .replace("<C-v>", "\x16")
        .replace("<C-w>", "\x17")
        .replace("<C-x>", "\x18")
        .replace("<C-y>", "\x19")
        .replace("<C-z>", "\x1a")
        // Alt+key combinations (simplified)
        .replace("<A-Esc>", "\x1b\x1b")
        .replace("<M-Esc>", "\x1b\x1b")
}

/// List SSH hosts from ~/.ssh/config
pub async fn ssh_hosts(query: Option<&str>, format: OutputFormat) -> CliResult<()> {
    ssh_hosts_to(
        SshConfigParser::parse_default_config(),
        query,
        format,
        &mut std::io::stdout(),
    )
}

fn ssh_hosts_to(
    parsed: SshResult<SshConfigParser>,
    query: Option<&str>,
    format: OutputFormat,
    out: &mut dyn Write,
) -> CliResult<()> {
    let parser = match parsed {
        Ok(p) => p,
        Err(e) => {
            writeln!(out, "No SSH config found or error parsing: {}", e)?;
            return Ok(());
        }
    };

    let hosts = filter_hosts(parser.hosts(), query);

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(&hosts)?)?;
        }
        OutputFormat::Yaml => {
            writeln!(out, "{}", to_yaml(&hosts)?)?;
        }
        OutputFormat::Text => write_hosts_text(&hosts, query, out)?,
    }

    Ok(())
}

/// Hosts cujo padrão ou hostname contém `query` (sem diferenciar caixa)
fn filter_hosts(hosts: &[SshHost], query: Option<&str>) -> Vec<SshHost> {
    let Some(q) = query else {
        return hosts.to_vec();
    };
    hosts
        .iter()
        .filter(|h| {
            h.pattern.to_lowercase().contains(&q.to_lowercase())
                || h.host_name
                    .as_ref()
                    .is_some_and(|hn| hn.to_lowercase().contains(&q.to_lowercase()))
        })
        .cloned()
        .collect()
}

/// Corta `s` em `max` bytes com ".." no fim quando passa do limite
fn shorten_with_dots(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}..", truncate_str(s, max))
    } else {
        s.to_string()
    }
}

/// Saída em texto de `ssh-hosts`: tabela com padrão, hostname, usuário e porta
fn write_hosts_text(hosts: &[SshHost], query: Option<&str>, out: &mut dyn Write) -> CliResult<()> {
    if hosts.is_empty() {
        writeln!(out, "No SSH hosts found")?;
        if let Some(query) = query {
            writeln!(out, "(filtered by query: '{}')", query)?;
        }
        return Ok(());
    }

    writeln!(
        out,
        "{:<20} {:<25} {:<15} {:<10}",
        "Pattern", "Hostname", "User", "Port"
    )?;
    writeln!(out, "{}", "-".repeat(80))?;

    for host in hosts {
        let pattern = shorten_with_dots(&host.pattern, 18);
        let hostname = shorten_with_dots(host.host_name.as_deref().unwrap_or("-"), 23);
        let user = host.user.as_deref().unwrap_or("-");
        let port = host
            .port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "22".to_string());

        writeln!(
            out,
            "{:<20} {:<25} {:<15} {:<10}",
            pattern, hostname, user, port
        )?;
    }

    writeln!(out, "\nTotal: {} host(s)", hosts.len())?;
    Ok(())
}

/// Connect to an SSH host
pub async fn ssh_connect(
    client: &mut RemoteClient,
    host: &str,
    port: Option<u16>,
    user: Option<&str>,
    identity: Option<&Path>,
    title: Option<&str>,
) -> CliResult<()> {
    let config = SshConfigParser::parse_default_config().ok();
    ssh_connect_to(
        client,
        config.as_ref(),
        host,
        port,
        user,
        identity,
        title,
        &mut std::io::stdout(),
    )
}

fn ssh_connect_to(
    client: &mut RemoteClient,
    config: Option<&SshConfigParser>,
    host: &str,
    port: Option<u16>,
    user: Option<&str>,
    identity: Option<&Path>,
    title: Option<&str>,
    out: &mut dyn Write,
) -> CliResult<()> {
    // Parse connection string (user@host or just host)
    let (parsed_user, hostname) = if let Some(at_pos) = host.find('@') {
        (Some(&host[..at_pos]), &host[at_pos + 1..])
    } else {
        (None, host)
    };

    // Try to find host in SSH config
    let effective_host = config.and_then(|parser| parser.get_effective_config(hostname));

    // Build SSH command
    let mut ssh_args = vec!["ssh".to_string()];

    // Add port if specified or from config
    let final_port = port
        .or(effective_host.as_ref().and_then(|h| h.port))
        .unwrap_or(22);
    if final_port != 22 {
        ssh_args.push("-p".to_string());
        ssh_args.push(final_port.to_string());
    }

    // Add identity if specified or from config
    let final_identity = identity.or(effective_host
        .as_ref()
        .and_then(|h| h.identity_file.as_deref()));
    if let Some(id) = final_identity {
        ssh_args.push("-i".to_string());
        ssh_args.push(id.to_string_lossy().to_string());
    }

    // Add user from: CLI arg > parsed from host string > config
    let final_user = user
        .or(parsed_user)
        .or(effective_host.as_ref().and_then(|h| h.user.as_deref()));

    // Build target string
    let target = if let Some(u) = final_user {
        format!("{}@{}", u, hostname)
    } else {
        hostname.to_string()
    };
    ssh_args.push(target);

    // Build tab title
    let tab_title = title
        .map(|t| t.to_string())
        .unwrap_or_else(|| format!("ssh:{}", hostname));

    // Execute SSH in new tab
    let index = client.new_tab(Some(&tab_title), None, ssh_args)?;

    writeln!(out, "Connected to {} in tab {}", hostname, index)?;

    Ok(())
}

/// List SSH sessions
pub async fn ssh_sessions(format: OutputFormat) -> CliResult<()> {
    ssh_sessions_to(&SessionManager::new(), format, &mut std::io::stdout())
}

fn ssh_sessions_to(
    manager: &SessionManager,
    format: OutputFormat,
    out: &mut dyn Write,
) -> CliResult<()> {
    use crate::ssh::SessionState;

    let sessions = manager.list_sessions();

    let count = sessions.len();

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(&sessions)?)?;
        }
        OutputFormat::Yaml => {
            writeln!(out, "{}", to_yaml(&sessions)?)?;
        }
        OutputFormat::Text => {
            if sessions.is_empty() {
                writeln!(out, "No SSH sessions found")?;
                return Ok(());
            }

            writeln!(
                out,
                "{:<12} {:<15} {:<25} {:<15} {:<10} {:<12}",
                "ID", "Name", "Host", "User", "Port", "State"
            )?;
            writeln!(out, "{}", "-".repeat(95))?;

            for session in sessions {
                let state_str = match session.state {
                    SessionState::Connected => "connected",
                    SessionState::Disconnected => "disconnected",
                    SessionState::Connecting => "connecting",
                    SessionState::Error => "error",
                };

                let id_short = format!("{}", session.id);
                let host_short = if session.host.len() > 23 {
                    format!("{}..", truncate_str(&session.host, 21))
                } else {
                    session.host.clone()
                };

                writeln!(
                    out,
                    "{:<12} {:<15} {:<25} {:<15} {:<10} {:<12}",
                    id_short, session.name, host_short, session.user, session.port, state_str
                )?;
            }

            writeln!(out, "\nTotal: {} session(s)", count)?;
        }
    }

    Ok(())
}

/// Create a new SSH session
pub async fn ssh_session_create(
    name: &str,
    host: &str,
    user: &str,
    port: Option<u16>,
) -> CliResult<()> {
    ssh_session_create_to(
        &mut SessionManager::new(),
        name,
        host,
        user,
        port,
        &mut std::io::stdout(),
    )
    .map(|_| ())
}

fn ssh_session_create_to(
    manager: &mut SessionManager,
    name: &str,
    host: &str,
    user: &str,
    port: Option<u16>,
    out: &mut dyn Write,
) -> CliResult<crate::ssh::SessionId> {
    use crate::ssh::SshSession;

    let mut session = SshSession::new(name, host, user);

    if let Some(p) = port {
        session.port = p;
    }

    let id = manager.create_session(session);

    writeln!(out, "Created SSH session '{}' (ID: {})", name, id)?;
    Ok(id)
}

/// Remove an SSH session
pub async fn ssh_session_remove(id: &str) -> CliResult<()> {
    ssh_session_remove_to(&mut SessionManager::new(), id, &mut std::io::stdout())
}

fn ssh_session_remove_to(
    manager: &mut SessionManager,
    id: &str,
    out: &mut dyn Write,
) -> CliResult<()> {
    use crate::ssh::SessionId;
    use uuid::Uuid;

    let uuid = Uuid::parse_str(id)
        .map_err(|_| CliError::InvalidInput(format!("Invalid session ID: {}", id)))?;

    manager.remove_session(SessionId(uuid))?;

    writeln!(out, "Removed SSH session {}", id)?;
    Ok(())
}

// ============================================================================
// Multiplexer Commands
// ============================================================================

/// Lista sessões do multiplexer
pub async fn mux_list(format: OutputFormat) -> CliResult<()> {
    match connect_mux() {
        Ok(mut client) => mux_list_to(&mut client, format, &mut std::io::stdout()),
        // Sem daemon não há sessões (ele sai quando a última termina).
        Err(_) if matches!(format, OutputFormat::Text) => {
            println!("No multiplexer sessions");
            Ok(())
        }
        Err(_) => {
            println!("[]");
            Ok(())
        }
    }
}

fn mux_list_to(
    client: &mut SessionClient,
    format: OutputFormat,
    out: &mut dyn Write,
) -> CliResult<()> {
    let sessions = client.list_sessions().map_err(mux_err)?;

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(&sessions)?)?;
        }
        OutputFormat::Yaml => {
            writeln!(out, "{}", to_yaml(&sessions)?)?;
        }
        OutputFormat::Text => {
            if sessions.is_empty() {
                writeln!(out, "No multiplexer sessions")?;
                return Ok(());
            }

            writeln!(
                out,
                "{:<10} {:<15} {:<8} {:<12} {:<20}",
                "ID", "Name", "Clients", "State", "Working Dir"
            )?;
            writeln!(out, "{}", "-".repeat(80))?;

            for session in &sessions {
                let id_short = truncate_str(&session.id, 8);
                let name = truncate_str(&session.name, 14);
                let cwd = truncate_str(&session.working_dir, 18);

                writeln!(
                    out,
                    "{:<10} {:<15} {:<8} {:<12} {}",
                    id_short, name, session.clients, session.state, cwd
                )?;
            }

            writeln!(out, "\nTotal: {} session(s)", sessions.len())?;
        }
    }

    Ok(())
}

/// Cria uma nova sessão do multiplexer
pub async fn mux_create(name: Option<String>, cols: u16, rows: u16) -> CliResult<()> {
    mux_create_to(
        &mut connect_mux_starting_daemon()?,
        name,
        cols,
        rows,
        &mut std::io::stdout(),
    )
}

fn mux_create_to(
    client: &mut SessionClient,
    name: Option<String>,
    cols: u16,
    rows: u16,
    out: &mut dyn Write,
) -> CliResult<()> {
    let (id, session_name) = client.create_session(name, (cols, rows)).map_err(mux_err)?;

    writeln!(
        out,
        "Created multiplexer session '{}' (ID: {})",
        session_name, id
    )?;
    writeln!(
        out,
        "Use 'code-terminal-cli mux-attach {}' to attach",
        session_name
    )?;

    Ok(())
}

/// Anexa o terminal atual a uma sessão do multiplexer. `Ctrl+B d` desanexa;
/// o shell continua no daemon.
#[cfg(unix)]
pub async fn mux_attach(session_id: &str, read_only: bool, create: bool) -> CliResult<()> {
    use crate::multiplexer::attach::{
        run_attach, AttachEnd, AttachOptions, RawMode, StdoutSize, DEFAULT_PREFIX,
        RESET_CLIENT_MODES,
    };
    use crate::multiplexer::transport::MuxConnection;
    if create {
        // Garante o daemon de pé (a conexão de controle é descartada).
        drop(connect_mux_starting_daemon()?);
    }
    let conn = MuxConnection::connect_default().map_err(|e| CliError::Connection {
        message: format!("mux daemon not running ({e}); use --create"),
    })?;
    let opts = AttachOptions {
        session: session_id.to_string(),
        read_only,
        create,
        prefix: DEFAULT_PREFIX,
    };
    let session = opts.session.clone();
    let result = tokio::task::spawn_blocking(move || {
        let raw = RawMode::enable();
        let mut stdout = std::io::stdout();
        let result = run_attach(conn, &opts, std::io::stdin(), &mut stdout, &StdoutSize);
        drop(raw);
        let _ = stdout.write_all(RESET_CLIENT_MODES.as_bytes());
        result
    })
    .await
    .map_err(|e| CliError::Connection {
        message: e.to_string(),
    })?;
    let (name, end) = result.map_err(|e| CliError::Connection {
        message: format!("{session}: {e}"),
    })?;
    let note = match end {
        AttachEnd::Detached => format!("[detached from '{name}' - the shell keeps running]"),
        AttachEnd::SessionEnded => format!("[session '{name}' ended]"),
        AttachEnd::ConnectionLost => "[mux daemon closed the connection]".to_string(),
        AttachEnd::InputClosed => format!("[input closed - '{name}' keeps running]"),
    };
    println!("\r\n{note}");
    Ok(())
}

#[cfg(not(unix))]
pub async fn mux_attach(_session_id: &str, _read_only: bool, _create: bool) -> CliResult<()> {
    start_mux_daemon()
}

/// Desanexa da sessão atual
pub async fn mux_detach() -> CliResult<()> {
    mux_detach_to(&mut connect_mux()?, &mut std::io::stdout())
}

fn mux_detach_to(client: &mut SessionClient, out: &mut dyn Write) -> CliResult<()> {
    client.detach().map_err(mux_err)?;

    writeln!(out, "Detached from session")?;
    Ok(())
}

/// Renomeia uma sessão do multiplexer
pub async fn mux_rename(session_id: &str, new_name: &str) -> CliResult<()> {
    mux_rename_to(
        &mut connect_mux()?,
        session_id,
        new_name,
        &mut std::io::stdout(),
    )
}

fn mux_rename_to(
    client: &mut SessionClient,
    session_id: &str,
    new_name: &str,
    out: &mut dyn Write,
) -> CliResult<()> {
    client
        .rename_session(session_id, new_name.to_string())
        .map_err(mux_err)?;

    writeln!(out, "Renamed session {} to '{}'", session_id, new_name)?;
    Ok(())
}

/// Destrói uma sessão do multiplexer
pub async fn mux_destroy(session_id: &str) -> CliResult<()> {
    mux_destroy_to(&mut connect_mux()?, session_id, &mut std::io::stdout())
}

fn mux_destroy_to(
    client: &mut SessionClient,
    session_id: &str,
    out: &mut dyn Write,
) -> CliResult<()> {
    client.destroy_session(session_id).map_err(mux_err)?;

    writeln!(out, "Destroyed session {}", session_id)?;
    Ok(())
}

// ============================================================================
// Session Management Commands (TASK-148)
// ============================================================================

/// Salva uma sessão em disco
pub async fn mux_save(session_id: &str) -> CliResult<()> {
    mux_save_to(
        &mut connect_mux()?,
        &SessionPersistence::new(),
        session_id,
        &mut std::io::stdout(),
    )
}

fn mux_save_to(
    client: &mut SessionClient,
    persistence: &SessionPersistence,
    session_id: &str,
    out: &mut dyn Write,
) -> CliResult<()> {
    // Busca a sessão e salva
    let sessions = client.list_sessions().map_err(mux_err)?;

    if let Some(session) = sessions.iter().find(|s| s.id.starts_with(session_id)) {
        // Mantém o ID da sessão do servidor, para `mux restore <id>` achar o arquivo
        let mux_session = to_mux_session(session);

        persistence
            .save_session(&mux_session)
            .map_err(|e| CliError::Other(e.to_string()))?;

        writeln!(out, "Session '{}' saved to disk", session.name)?;
    } else {
        return Err(CliError::CommandNotFound(format!(
            "Session '{}' not found",
            session_id
        )));
    }

    Ok(())
}

/// Lista sessões salvas em disco
pub async fn mux_saved(format: OutputFormat) -> CliResult<()> {
    mux_saved_to(&SessionPersistence::new(), format, &mut std::io::stdout())
}

fn mux_saved_to(
    persistence: &SessionPersistence,
    format: OutputFormat,
    out: &mut dyn Write,
) -> CliResult<()> {
    let sessions = persistence
        .list_saved_sessions()
        .map_err(|e| CliError::Other(e.to_string()))?;

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(&sessions)?)?;
        }
        OutputFormat::Yaml => {
            writeln!(out, "{}", to_yaml(&sessions)?)?;
        }
        OutputFormat::Text => {
            if sessions.is_empty() {
                writeln!(out, "No saved sessions")?;
                return Ok(());
            }

            writeln!(
                out,
                "{:<10} {:<15} {:<20} {:<15}",
                "ID", "Name", "Working Dir", "Last Activity"
            )?;
            writeln!(out, "{}", "-".repeat(70))?;

            for session in &sessions {
                let id_full = session.id.to_string();
                let id_short = truncate_str(&id_full, 8);
                let name = truncate_str(&session.name, 14);
                let cwd = truncate_str(&session.working_dir, 18);
                let last = session.last_activity.format("%Y-%m-%d %H:%M").to_string();

                writeln!(
                    out,
                    "{:<10} {:<15} {:<20} {:<15}",
                    id_short, name, cwd, last
                )?;
            }

            writeln!(out, "\nTotal: {} saved session(s)", sessions.len())?;
        }
    }

    Ok(())
}

/// Restaura uma sessão salva
pub async fn mux_restore(session_id: &str) -> CliResult<()> {
    mux_restore_to(
        &SessionPersistence::new(),
        session_id,
        &mut std::io::stdout(),
    )
}

fn mux_restore_to(
    persistence: &SessionPersistence,
    session_id: &str,
    out: &mut dyn Write,
) -> CliResult<()> {
    use crate::multiplexer::SessionId;
    use uuid::Uuid;

    let uuid = Uuid::parse_str(session_id)
        .map_err(|_| CliError::InvalidInput(format!("Invalid session ID: {}", session_id)))?;

    let data = persistence
        .load_session(SessionId(uuid))
        .map_err(|e| CliError::Other(e.to_string()))?;

    let restored = persistence.restore_session(&data);

    writeln!(
        out,
        "Restored session '{}' (ID: {})",
        restored.name, restored.id
    )?;
    writeln!(out, "Working directory: {}", restored.working_dir)?;
    writeln!(out, "Shell: {}", restored.shell)?;

    Ok(())
}

/// Mostra estatísticas do multiplexer
pub async fn mux_stats(format: OutputFormat) -> CliResult<()> {
    mux_stats_to(
        &mut connect_mux()?,
        &SessionPersistence::new(),
        format,
        &mut std::io::stdout(),
    )
}

fn mux_stats_to(
    client: &mut SessionClient,
    persistence: &SessionPersistence,
    format: OutputFormat,
    out: &mut dyn Write,
) -> CliResult<()> {
    let sessions = client.list_sessions().map_err(mux_err)?;

    let saved_stats = persistence
        .stats()
        .map_err(|e| CliError::Other(e.to_string()))?;

    #[derive(serde::Serialize)]
    struct StatsData {
        active_sessions: usize,
        saved_sessions: usize,
        persistence_size_bytes: u64,
    }

    let data = StatsData {
        active_sessions: sessions.len(),
        saved_sessions: saved_stats.total_sessions,
        persistence_size_bytes: saved_stats.total_size_bytes,
    };

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(&data)?)?;
        }
        OutputFormat::Yaml => {
            writeln!(out, "{}", to_yaml(&data)?)?;
        }
        OutputFormat::Text => {
            writeln!(out, "Multiplexer Statistics")?;
            writeln!(out, "{}", "=".repeat(40))?;
            writeln!(out, "Active sessions: {}", data.active_sessions)?;
            writeln!(out, "Saved sessions:  {}", data.saved_sessions)?;
            writeln!(
                out,
                "Persistence size: {} bytes",
                data.persistence_size_bytes
            )?;

            if !sessions.is_empty() {
                writeln!(out, "\nActive Sessions:")?;
                for s in &sessions {
                    writeln!(out, "  - {} ({} clients, {})", s.name, s.clients, s.state)?;
                }
            }
        }
    }

    Ok(())
}

/// Converte o `SessionInfo` recebido do servidor numa `MultiplexerSession`
/// local com o mesmo ID, estado, clientes e diretório.
fn to_mux_session(s: &SessionInfo) -> crate::multiplexer::MultiplexerSession {
    let mut session = crate::multiplexer::MultiplexerSession::new(Some(s.name.clone()), s.size);
    session.id = crate::multiplexer::SessionId(
        s.id.parse::<uuid::Uuid>()
            .unwrap_or_else(|_| uuid::Uuid::new_v4()),
    );
    session.state = parse_session_state(&s.state);
    session.client_count = s.clients;
    session.working_dir = s.working_dir.clone();
    session
}

/// Lista sessões com interface interativa (TASK-151)
pub async fn mux_session_list(format: OutputFormat, sort: Option<String>) -> CliResult<()> {
    mux_session_list_to(&mut connect_mux()?, format, sort, &mut std::io::stdout())
}

fn mux_session_list_to(
    client: &mut SessionClient,
    format: OutputFormat,
    sort: Option<String>,
    out: &mut dyn Write,
) -> CliResult<()> {
    use crate::multiplexer::{SessionList, SortOrder};

    let sessions = client.list_sessions().map_err(mux_err)?;

    let mut list = SessionList::new();

    // Converte SessionInfo para MultiplexerSession
    list.set_sessions(sessions.iter().map(to_mux_session).collect());

    // Aplica ordenação
    if let Some(sort_by) = sort {
        let order = match sort_by.as_str() {
            "name" => SortOrder::Name,
            "created" => SortOrder::Created,
            "activity" => SortOrder::Activity,
            "clients" => SortOrder::Clients,
            _ => SortOrder::Activity,
        };
        list.set_sort_order(order);
    }

    match format {
        OutputFormat::Json => {
            writeln!(out, "{}", serde_json::to_string_pretty(list.sessions())?)?;
        }
        OutputFormat::Yaml => {
            writeln!(out, "{}", to_yaml(list.sessions())?)?;
        }
        OutputFormat::Text => {
            writeln!(out, "{}", list.format_text())?;

            let stats = list.stats();
            writeln!(
                out,
                "\nTotal: {} sessions, {} active, {} attachable",
                stats.total, stats.active, stats.attachable
            )?;
            writeln!(
                out,
                "Use 'mux-switch <name>' or 'mux-switch --next/--prev' to change sessions"
            )?;
        }
    }

    Ok(())
}

/// Alterna entre sessões (TASK-152)
pub async fn mux_switch(target: Option<String>, next: bool, prev: bool) -> CliResult<()> {
    mux_switch_to(
        &mut connect_mux()?,
        target,
        next,
        prev,
        &mut std::io::stdout(),
    )
}

fn mux_switch_to(
    client: &mut SessionClient,
    target: Option<String>,
    next: bool,
    prev: bool,
    out: &mut dyn Write,
) -> CliResult<()> {
    use crate::multiplexer::SessionSwitcher;

    let sessions = client.list_sessions().map_err(mux_err)?;

    let mut switcher = SessionSwitcher::new();

    // Converte SessionInfo para MultiplexerSession
    switcher.set_sessions(sessions.iter().map(to_mux_session).collect());

    let target_id = if next {
        switcher.switch_next()
    } else if prev {
        switcher.switch_previous()
    } else if let Some(name) = target {
        switch_to_named(&mut switcher, &name)?
    } else {
        // Se não especificado, lista as opções
        return write_switch_choices(&switcher, out);
    };

    if let Some(id) = target_id {
        // Anexa à sessão selecionada pelo UUID completo (o Display de
        // SessionId mostra só o primeiro bloco)
        client.attach(&id.0.to_string(), false).map_err(mux_err)?;

        if let Some(current) = switcher.current() {
            writeln!(out, "Switched to session '{}' ({})", current.name, id)?;
        }
    }

    Ok(())
}

/// Seleciona a sessão `name` no switcher; erro se ela não existir
fn switch_to_named(
    switcher: &mut crate::multiplexer::SessionSwitcher,
    name: &str,
) -> CliResult<Option<crate::multiplexer::SessionId>> {
    if !switcher.switch_to_name(name) {
        return Err(CliError::CommandNotFound(format!(
            "Session '{}' not found",
            name
        )));
    }
    Ok(switcher.current_id())
}

/// Lista as sessões disponíveis quando `mux-switch` não recebe alvo
fn write_switch_choices(
    switcher: &crate::multiplexer::SessionSwitcher,
    out: &mut dyn Write,
) -> CliResult<()> {
    writeln!(out, "Available sessions:")?;
    for (i, session) in switcher.list().sessions().iter().enumerate() {
        let marker = if i == 0 { "►" } else { " " };
        writeln!(out, "{} {}: {}", marker, i + 1, session.name)?;
    }
    writeln!(out, "\nUse 'mux-switch <name>' or 'mux-switch --next'")?;
    Ok(())
}

/// Navega para a última sessão usada
pub async fn mux_last() -> CliResult<()> {
    mux_last_to(&mut connect_mux()?, &mut std::io::stdout())
}

fn mux_last_to(client: &mut SessionClient, out: &mut dyn Write) -> CliResult<()> {
    use crate::multiplexer::SessionSwitcher;

    let sessions = client.list_sessions().map_err(mux_err)?;

    let mut switcher = SessionSwitcher::new();

    switcher.set_sessions(sessions.iter().map(to_mux_session).collect());

    if let Some(id) = switcher.switch_last() {
        client.attach(&id.0.to_string(), false).map_err(mux_err)?;

        writeln!(out, "Switched to last session")?;
    } else {
        writeln!(out, "No previous session")?;
    }

    Ok(())
}

// Helper function
fn parse_session_state(state: &str) -> crate::multiplexer::SessionState {
    match state {
        "creating" => crate::multiplexer::SessionState::Creating,
        "active" => crate::multiplexer::SessionState::Active,
        "background" => crate::multiplexer::SessionState::Background,
        "destroying" => crate::multiplexer::SessionState::Destroying,
        _ => crate::multiplexer::SessionState::Background,
    }
}

/// Gerencia grupos de sessões
pub async fn mux_groups(action: &str, args: &[String]) -> CliResult<()> {
    mux_groups_to(action, args, &mut std::io::stdout())
}

fn mux_groups_to(action: &str, args: &[String], out: &mut dyn Write) -> CliResult<()> {
    use crate::multiplexer::GroupManager;

    let mut manager = GroupManager::new();

    match action {
        "list" => {
            let groups = manager.list_groups();
            if groups.is_empty() {
                writeln!(out, "No groups defined")?;
            } else {
                writeln!(out, "Groups:")?;
                for group in groups {
                    writeln!(out, "  {} ({} sessions)", group.name, group.session_count())?;
                }
            }
        }
        "create" => {
            if args.is_empty() {
                return Err(CliError::InvalidInput("Group name required".to_string()));
            }
            let id = manager.create_group(&args[0]);
            writeln!(out, "Created group '{}' (ID: {})", args[0], id)?;
        }
        "delete" => {
            if args.is_empty() {
                return Err(CliError::InvalidInput("Group ID required".to_string()));
            }
            // Simplificado - em uma implementação real, parsear o UUID
            writeln!(out, "Group deletion requires full implementation")?;
        }
        _ => {
            return Err(CliError::InvalidInput(format!(
                "Unknown action: {}",
                action
            )));
        }
    }

    Ok(())
}

/// Limpa sessões antigas
pub async fn mux_cleanup(days: u32) -> CliResult<()> {
    mux_cleanup_to(&SessionPersistence::new(), days, &mut std::io::stdout())
}

fn mux_cleanup_to(
    persistence: &SessionPersistence,
    days: u32,
    out: &mut dyn Write,
) -> CliResult<()> {
    let removed = persistence
        .cleanup_old_sessions(days)
        .map_err(|e| CliError::Other(e.to_string()))?;

    writeln!(out, "Cleaned up {} old session(s)", removed)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::test_support::{terminal_handler, FakeMux, FakeRpc, RpcReply};
    use crate::multiplexer::protocol::SessionFlagsInfo;
    use crate::multiplexer::{ErrorCode, MessageType, PersistenceConfig, SessionId as MuxId};
    use crate::ssh::SessionPersistenceConfig;
    use serde_json::{json, Value};
    use tempfile::TempDir;

    // ------------------------------------------------------------------
    // Auxiliares
    // ------------------------------------------------------------------

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn rpc_client(server: &FakeRpc) -> RemoteClient {
        rt().block_on(RemoteClient::connect(&server.addr)).unwrap()
    }

    fn run<F>(f: F) -> (CliResult<()>, String)
    where
        F: FnOnce(&mut dyn Write) -> CliResult<()>,
    {
        let mut buf = Vec::new();
        let r = f(&mut buf);
        (r, String::from_utf8(buf).unwrap())
    }

    fn empty_lists(method: &str, params: &Value) -> RpcReply {
        match method {
            "tab.list" | "pane.list" => Ok(json!([])),
            _ => terminal_handler(method, params),
        }
    }

    const ID_A: &str = "11111111-1111-4111-8111-111111111111";
    const ID_B: &str = "22222222-2222-4222-8222-222222222222";

    fn sinfo(id: &str, name: &str, state: &str, clients: usize, cwd: &str) -> SessionInfo {
        SessionInfo {
            id: id.to_string(),
            name: name.to_string(),
            state: state.to_string(),
            clients,
            size: (80, 24),
            working_dir: cwd.to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            idle_seconds: 0,
            flags: SessionFlagsInfo {
                destroy_on_exit: false,
                attachable: true,
                shared: false,
                locked: false,
            },
            app: None,
        }
    }

    fn two_sessions() -> Vec<SessionInfo> {
        vec![
            sinfo(ID_A, "alpha", "active", 2, "/srv/a"),
            sinfo(ID_B, "beta", "background", 0, "/srv/b"),
        ]
    }

    /// Servidor do multiplexer que responde como um servidor real simples.
    fn mux_server(sessions: Vec<SessionInfo>) -> FakeMux {
        FakeMux::start(move |req| match req {
            MessageType::ListSessions => MessageType::SessionList {
                sessions: sessions.clone(),
            },
            MessageType::CreateSession { name, .. } => MessageType::SessionCreated {
                session_id: ID_A.to_string(),
                name: name.clone().unwrap_or_else(|| "0".to_string()),
            },
            MessageType::Attach { session_id, .. } => {
                match sessions.iter().find(|s| &s.id == session_id) {
                    Some(s) => MessageType::Attached {
                        session_id: s.id.clone(),
                        name: s.name.clone(),
                    },
                    None => MessageType::Error {
                        code: ErrorCode::SessionNotFound,
                        message: session_id.clone(),
                    },
                }
            }
            MessageType::Detach => MessageType::Detached {
                session_id: ID_A.to_string(),
            },
            MessageType::RenameSession {
                session_id,
                new_name,
            } => MessageType::SessionRenamed {
                session_id: session_id.clone(),
                new_name: new_name.clone(),
            },
            MessageType::DestroySession { session_id } => MessageType::SessionDestroyed {
                session_id: session_id.clone(),
            },
            _ => MessageType::Pong,
        })
    }

    fn failing_mux() -> FakeMux {
        FakeMux::start(|_| MessageType::Error {
            code: ErrorCode::ServerError,
            message: "boom".to_string(),
        })
    }

    fn persistence(dir: &TempDir) -> SessionPersistence {
        SessionPersistence::with_config(PersistenceConfig {
            persistence_dir: dir.path().join("mux"),
            ..PersistenceConfig::default()
        })
    }

    fn ssh_manager(dir: &TempDir) -> SessionManager {
        SessionManager::with_config(SessionPersistenceConfig {
            persistence_dir: dir.path().join("ssh"),
            ..SessionPersistenceConfig::default()
        })
    }

    // ------------------------------------------------------------------
    // truncate_str
    // ------------------------------------------------------------------

    #[test]
    fn test_truncate_str_keeps_short_and_ascii() {
        assert_eq!(truncate_str("abc", 10), "abc");
        assert_eq!(truncate_str("abcdef", 3), "abc");
        assert_eq!(truncate_str("", 0), "");
    }

    #[test]
    fn test_truncate_str_never_splits_utf8() {
        // 'é' ocupa os bytes 17..19; cortar em 18 entraria no meio do caractere
        let s = format!("{}é-resto", "a".repeat(17));
        assert_eq!(truncate_str(&s, 18), "a".repeat(17));
        assert_eq!(truncate_str("ção", 1), "");
        assert_eq!(truncate_str("ção", 2), "ç");
    }

    // ------------------------------------------------------------------
    // Comandos via JSON-RPC
    // ------------------------------------------------------------------

    #[test]
    fn test_list_tabs_text_short_marks_active() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| list_tabs_to(&mut c, false, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "  0: shell\n* 1: vim\n");
        assert_eq!(srv.methods(), vec!["tab.list"]);
    }

    #[test]
    fn test_list_tabs_text_long_shows_pid_and_cwd() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| list_tabs_to(&mut c, true, OutputFormat::Text, o));
        r.unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("Index"));
        assert_eq!(lines[1], "-".repeat(80));
        assert!(lines[2].contains("100") && lines[2].contains("/home/u"));
        assert!(lines[2].ends_with("shell"));
        // aba sem pid nem cwd usa "-"
        assert!(lines[3].starts_with("1     *        -          -"));
    }

    #[test]
    fn test_list_tabs_long_with_multibyte_cwd_does_not_panic() {
        // Regressão: `&cwd[..18]` entrava em pânico com caracteres de 2 bytes
        let srv = FakeRpc::tcp(|m, _| match m {
            "tab.list" => Ok(json!([{
                "id": 1, "index": 0, "title": "t", "is_active": true,
                "pid": 1, "cwd": "/srv/projetos/açõespendentes"
            }])),
            _ => Ok(Value::Null),
        });
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| list_tabs_to(&mut c, true, OutputFormat::Text, o));
        r.unwrap();
        assert!(out.contains("/srv/projetos/aç "));
        assert!(!out.contains("açõ"));
    }

    #[test]
    fn test_list_tabs_json_yaml_and_empty() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| list_tabs_to(&mut c, false, OutputFormat::Json, o));
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert_eq!(v[1]["title"], "vim");

        let (r, out) = run(|o| list_tabs_to(&mut c, false, OutputFormat::Yaml, o));
        r.unwrap();
        let y: serde_yaml::Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(y[0]["title"].as_str(), Some("shell"));

        let srv = FakeRpc::tcp(empty_lists);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| list_tabs_to(&mut c, true, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "No tabs\n");
    }

    #[test]
    fn test_new_tab_sends_params_and_switches() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| {
            new_tab_to(
                &mut c,
                Some("build".into()),
                Some(PathBuf::from("/tmp/w")),
                vec!["make".into(), "all".into()],
                o,
            )
        });
        r.unwrap();
        assert_eq!(out, "Created tab 3\n");
        assert_eq!(srv.methods(), vec!["tab.create", "tab.switch"]);
        assert_eq!(
            srv.params_of("tab.create").unwrap(),
            json!({"title": "build", "cwd": "/tmp/w", "command": ["make", "all"]})
        );
        assert_eq!(srv.params_of("tab.switch").unwrap(), json!({"id": 7}));
    }

    #[test]
    fn test_new_tab_without_options_sends_empty_object() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, _) = run(|o| new_tab_to(&mut c, None, None, vec![], o));
        r.unwrap();
        assert_eq!(srv.params_of("tab.create").unwrap(), json!({}));
    }

    #[test]
    fn test_close_tab_resolves_index_to_id() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| close_tab_to(&mut c, 1, o));
        r.unwrap();
        assert_eq!(out, "Closed tab 1\n");
        assert_eq!(srv.methods(), vec!["tab.list", "tab.close"]);
        assert_eq!(srv.params_of("tab.close").unwrap(), json!({"id": 11}));

        let (r, out) = run(|o| close_tab_to(&mut c, 9, o));
        assert!(matches!(r, Err(CliError::InvalidResponse(m)) if m.contains("9")));
        assert!(out.is_empty());
    }

    #[test]
    fn test_rename_tab() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| rename_tab_to(&mut c, 1, Some("api".into()), o));
        r.unwrap();
        assert_eq!(out, "Renamed tab 1 to 'api'\n");
        assert_eq!(
            srv.params_of("tab.rename").unwrap(),
            json!({"index": 1, "title": "api"})
        );
        let (r, out) = run(|o| rename_tab_to(&mut c, 0, Some("  ".into()), o));
        r.unwrap();
        assert_eq!(out, "Tab 0 uses the automatic title again\n");
        assert_eq!(
            srv.params_of("tab.rename").unwrap(),
            json!({"index": 0, "title": null})
        );
    }

    #[test]
    fn test_goto_tab_and_keys_and_input() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| goto_tab_to(&mut c, 2, o));
        r.unwrap();
        assert_eq!(out, "Switched to tab 2\n");
        assert_eq!(srv.params_of("tab.switch").unwrap(), json!({"index": 2}));

        rt().block_on(send_keys(&mut c, "ls<Enter><C-c>")).unwrap();
        assert_eq!(
            srv.params_of("keyboard.send").unwrap(),
            json!({"keys": "ls\r\x03"})
        );
        rt().block_on(send_input(&mut c, "echo <Enter>")).unwrap();
        // send_input não interpreta a notação de teclas
        assert_eq!(
            srv.params_of("keyboard.text").unwrap(),
            json!({"text": "echo <Enter>"})
        );
    }

    #[test]
    fn test_get_and_set_size() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| get_size_to(&mut c, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "120x40\n");
        let (r, out) = run(|o| get_size_to(&mut c, OutputFormat::Json, o));
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v, json!({"cols": 120, "rows": 40}));

        let (r, out) = run(|o| set_size_to(&mut c, 100, 30, o));
        r.unwrap();
        assert_eq!(out, "Set size to 100x30\n");
        assert_eq!(
            srv.params_of("set_size").unwrap(),
            json!({"cols": 100, "rows": 30})
        );
    }

    #[test]
    fn test_info_all_formats() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| info_to(&mut c, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "Version: 1.2.3\nTabs: 2 (active: 1)\nSize: 120x40\n");

        let (r, out) = run(|o| info_to(&mut c, OutputFormat::Json, o));
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["tab_count"], 2);
        assert_eq!(v["size"]["cols"], 120);

        let (r, out) = run(|o| info_to(&mut c, OutputFormat::Yaml, o));
        r.unwrap();
        assert!(out.contains("version: 1.2.3"));
    }

    #[test]
    fn test_notify_and_split() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| notify_to(&mut c, "t", "m", o));
        r.unwrap();
        assert_eq!(out, "Notification sent\n");
        let params = srv.params_of("notification.send").unwrap();
        assert_eq!(params["title"], "t");
        assert_eq!(params["body"], "m");

        for (dir, name) in [
            (SplitDirection::Horizontal, "horizontal"),
            (SplitDirection::Vertical, "vertical"),
        ] {
            let (r, out) = run(|o| split_to(&mut c, dir, o));
            r.unwrap();
            assert_eq!(out, format!("Split pane {}\n", name));
            assert_eq!(
                srv.params_of("pane.split").unwrap(),
                json!({"tab_id": 11, "params": {"direction": name}})
            );
        }
    }

    #[test]
    fn test_list_panes_formats() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| list_panes_to(&mut c, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "* 0: pane-5\n  1: pane-6\n");
        let (r, out) = run(|o| list_panes_to(&mut c, OutputFormat::Json, o));
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[1]["id"], 6);

        let srv = FakeRpc::tcp(empty_lists);
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| list_panes_to(&mut c, OutputFormat::Yaml, o));
        r.unwrap();
        assert_eq!(out, "No panes\n");
    }

    #[test]
    fn test_rpc_error_propagates_without_output() {
        let srv = FakeRpc::tcp(|_, _| Err((-32601, "Method not found".into(), None)));
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| list_tabs_to(&mut c, false, OutputFormat::Text, o));
        match r {
            Err(CliError::Rpc { message }) => assert!(message.contains("-32601")),
            other => panic!("esperava erro RPC, veio {:?}", other),
        }
        assert!(out.is_empty());
    }

    #[test]
    fn test_public_rpc_wrappers_hit_server() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let rt = rt();
        rt.block_on(async {
            list_tabs(&mut c, false, OutputFormat::Json).await.unwrap();
            new_tab(&mut c, None, None, vec![]).await.unwrap();
            close_tab(&mut c, 0).await.unwrap();
            goto_tab(&mut c, 0).await.unwrap();
            get_size(&mut c, OutputFormat::Json).await.unwrap();
            set_size(&mut c, 80, 24).await.unwrap();
            info(&mut c, OutputFormat::Json).await.unwrap();
            notify(&mut c, "a", "b").await.unwrap();
            list_panes(&mut c, OutputFormat::Json).await.unwrap();
            split(&mut c, SplitDirection::Vertical).await.unwrap();
        });
        assert_eq!(
            srv.methods(),
            vec![
                "tab.list",
                "tab.create",
                "tab.switch",
                "tab.list",
                "tab.close",
                "tab.switch",
                "state.get",
                "set_size",
                "state.get",
                "notification.send",
                "pane.list",
                "state.get",
                "pane.split",
            ]
        );
    }

    // ------------------------------------------------------------------
    // SSH
    // ------------------------------------------------------------------

    const SSH_CONFIG: &str = "\
Host prod
    HostName prod.example.com
    User deploy
    Port 2222
    IdentityFile /keys/prod

Host um-alias-bem-comprido-demais
    HostName servidor.muito.comprido.example.com

Host dev
    HostName 10.0.0.5
";

    #[test]
    fn test_ssh_hosts_without_config_reports_error() {
        let (r, out) = run(|o| {
            ssh_hosts_to(
                Err(crate::ssh::SshError::ConfigNotFound),
                None,
                OutputFormat::Text,
                o,
            )
        });
        r.unwrap();
        assert!(out.starts_with("No SSH config found or error parsing:"));
    }

    #[test]
    fn test_ssh_hosts_text_table_truncates_and_defaults() {
        let (r, out) = run(|o| {
            ssh_hosts_to(
                SshConfigParser::from_str(SSH_CONFIG),
                None,
                OutputFormat::Text,
                o,
            )
        });
        r.unwrap();
        assert!(out.contains("prod.example.com"));
        assert!(out.contains("deploy"));
        assert!(out.contains("2222"));
        // padrão longo é cortado em 18 + ".."
        assert!(out.contains("um-alias-bem-compr.."));
        assert!(out.contains("servidor.muito.comprido..")); // 23 + ".."
                                                            // sem User nem Port: "-" e 22
        let dev = out.lines().find(|l| l.starts_with("dev")).unwrap();
        assert!(dev.contains(" - ") && dev.contains("22"));
        assert!(out.ends_with("Total: 3 host(s)\n"));
    }

    #[test]
    fn test_ssh_hosts_query_filters_by_pattern_or_hostname() {
        let (r, out) = run(|o| {
            ssh_hosts_to(
                SshConfigParser::from_str(SSH_CONFIG),
                Some("EXAMPLE"),
                OutputFormat::Json,
                o,
            )
        });
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);

        let (r, out) = run(|o| {
            ssh_hosts_to(
                SshConfigParser::from_str(SSH_CONFIG),
                Some("dev"),
                OutputFormat::Yaml,
                o,
            )
        });
        r.unwrap();
        assert!(out.contains("10.0.0.5"));
        assert!(!out.contains("prod.example.com"));

        let (r, out) = run(|o| {
            ssh_hosts_to(
                SshConfigParser::from_str(SSH_CONFIG),
                Some("zzz"),
                OutputFormat::Text,
                o,
            )
        });
        r.unwrap();
        assert_eq!(out, "No SSH hosts found\n(filtered by query: 'zzz')\n");

        let (r, out) =
            run(|o| ssh_hosts_to(SshConfigParser::from_str(""), None, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "No SSH hosts found\n");
    }

    #[test]
    fn test_ssh_hosts_multibyte_pattern_does_not_panic() {
        let cfg = format!("Host {}ção\n    HostName h\n", "a".repeat(17));
        let (r, out) =
            run(|o| ssh_hosts_to(SshConfigParser::from_str(&cfg), None, OutputFormat::Text, o));
        r.unwrap();
        assert!(out.contains(&format!("{}..", "a".repeat(17))));
    }

    fn ssh_command(srv: &FakeRpc) -> Vec<String> {
        let p = srv.params_of("tab.create").unwrap();
        p["command"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn test_ssh_connect_uses_config_values() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let cfg = SshConfigParser::from_str(SSH_CONFIG).unwrap();
        let (r, out) =
            run(|o| ssh_connect_to(&mut c, Some(&cfg), "prod", None, None, None, None, o));
        r.unwrap();
        assert_eq!(out, "Connected to prod in tab 3\n");
        assert_eq!(
            ssh_command(&srv),
            vec!["ssh", "-p", "2222", "-i", "/keys/prod", "deploy@prod"]
        );
        assert_eq!(srv.params_of("tab.create").unwrap()["title"], "ssh:prod");
    }

    #[test]
    fn test_ssh_connect_cli_args_override_config() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let cfg = SshConfigParser::from_str(SSH_CONFIG).unwrap();
        let (r, _) = run(|o| {
            ssh_connect_to(
                &mut c,
                Some(&cfg),
                "root@prod",
                Some(22),
                Some("admin"),
                Some(Path::new("/k/other")),
                Some("Produção"),
                o,
            )
        });
        r.unwrap();
        // porta 22 é omitida; usuário da CLI vence o "root@" e o do config
        assert_eq!(
            ssh_command(&srv),
            vec!["ssh", "-i", "/k/other", "admin@prod"]
        );
        assert_eq!(srv.params_of("tab.create").unwrap()["title"], "Produção");
    }

    #[test]
    fn test_ssh_connect_without_config_uses_user_from_host_string() {
        let srv = FakeRpc::tcp(terminal_handler);
        let mut c = rpc_client(&srv);
        let (r, _) =
            run(|o| ssh_connect_to(&mut c, None, "joe@box", Some(2200), None, None, None, o));
        r.unwrap();
        assert_eq!(ssh_command(&srv), vec!["ssh", "-p", "2200", "joe@box"]);

        let (r, _) = run(|o| ssh_connect_to(&mut c, None, "box", None, None, None, None, o));
        r.unwrap();
        assert_eq!(ssh_command(&srv), vec!["ssh", "box"]);
    }

    #[test]
    fn test_ssh_connect_propagates_rpc_error() {
        let srv = FakeRpc::tcp(|_, _| Err((-1, "no".into(), Some(json!("x")))));
        let mut c = rpc_client(&srv);
        let (r, out) = run(|o| ssh_connect_to(&mut c, None, "h", None, None, None, None, o));
        assert!(matches!(r, Err(CliError::Rpc { message }) if message.contains("data: \"x\"")));
        assert!(out.is_empty());
    }

    #[test]
    fn test_ssh_session_create_list_remove() {
        let dir = TempDir::new().unwrap();
        let mut m = ssh_manager(&dir);

        let (r, out) = run(|o| ssh_sessions_to(&m, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "No SSH sessions found\n");

        let mut buf = Vec::new();
        let id = ssh_session_create_to(
            &mut m,
            "web",
            "web.example.com",
            "ana",
            Some(2022),
            &mut buf,
        )
        .unwrap();
        let created = String::from_utf8(buf).unwrap();
        assert!(created.starts_with("Created SSH session 'web' (ID: "));
        assert_eq!(m.get_session(id).unwrap().port, 2022);

        let mut buf = Vec::new();
        let id2 = ssh_session_create_to(
            &mut m,
            "longo",
            "um.host.com.nome.bem.comprido.example",
            "bob",
            None,
            &mut buf,
        )
        .unwrap();
        assert_eq!(m.get_session(id2).unwrap().port, 22);

        let (r, out) = run(|o| ssh_sessions_to(&m, OutputFormat::Text, o));
        r.unwrap();
        assert!(out.contains("web.example.com"));
        assert!(out.contains("disconnected"));
        assert!(out.contains("um.host.com.nome.bem..."));
        assert!(out.ends_with("Total: 2 session(s)\n"));

        let (r, out) = run(|o| ssh_sessions_to(&m, OutputFormat::Json, o));
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 2);
        let (r, out) = run(|o| ssh_sessions_to(&m, OutputFormat::Yaml, o));
        r.unwrap();
        assert!(out.contains("ana"));

        // persistido com auto_save: um novo manager no mesmo diretório vê a sessão
        let reloaded = ssh_manager(&dir);
        assert!(reloaded.get_session(id).is_some());

        let (r, out) = run(|o| ssh_session_remove_to(&mut m, &id.0.to_string(), o));
        r.unwrap();
        assert!(out.starts_with("Removed SSH session "));
        assert!(m.get_session(id).is_none());

        let (r, _) = run(|o| ssh_session_remove_to(&mut m, "nao-e-uuid", o));
        assert!(matches!(r, Err(CliError::InvalidInput(msg)) if msg.contains("nao-e-uuid")));
    }

    #[test]
    fn test_ssh_sessions_shows_every_state() {
        use crate::ssh::SessionState;
        let dir = TempDir::new().unwrap();
        let mut m = ssh_manager(&dir);
        for (i, st) in [
            SessionState::Connected,
            SessionState::Connecting,
            SessionState::Error,
        ]
        .into_iter()
        .enumerate()
        {
            let mut buf = Vec::new();
            let id =
                ssh_session_create_to(&mut m, &format!("s{i}"), "h", "u", None, &mut buf).unwrap();
            m.get_session_mut(id).unwrap().state = st;
        }
        let (r, out) = run(|o| ssh_sessions_to(&m, OutputFormat::Text, o));
        r.unwrap();
        for word in ["connected", "connecting", "error"] {
            assert!(out.contains(word), "faltou {word} em {out}");
        }
    }

    // ------------------------------------------------------------------
    // Multiplexer
    // ------------------------------------------------------------------

    #[test]
    fn test_mux_list_text_json_yaml_and_empty() {
        let srv = mux_server(two_sessions());
        let mut c = srv.client();
        let (r, out) = run(|o| mux_list_to(&mut c, OutputFormat::Text, o));
        r.unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("ID"));
        assert!(lines[2].starts_with("11111111   alpha"));
        assert!(lines[2].contains("active") && lines[2].ends_with("/srv/a"));
        assert!(out.ends_with("Total: 2 session(s)\n"));

        let (r, out) = run(|o| mux_list_to(&mut c, OutputFormat::Json, o));
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[1]["name"], "beta");
        let (r, out) = run(|o| mux_list_to(&mut c, OutputFormat::Yaml, o));
        r.unwrap();
        assert!(out.contains("name: alpha"));

        let srv = mux_server(vec![]);
        let mut c = srv.client();
        let (r, out) = run(|o| mux_list_to(&mut c, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "No multiplexer sessions\n");
    }

    #[test]
    fn test_mux_list_truncates_long_multibyte_fields() {
        // Regressão: nome/diretório com acentos além do limite causavam pânico
        // ("é" ocupa os bytes 13..15 do nome; "õ" os bytes 17..19 do diretório)
        let name = format!("{}é-fim", "a".repeat(13));
        let srv = mux_server(vec![sinfo(
            "abc",
            &name,
            "active",
            1,
            "/srv/projetos/açõespendentes",
        )]);
        let mut c = srv.client();
        let (r, out) = run(|o| mux_list_to(&mut c, OutputFormat::Text, o));
        r.unwrap();
        let row = out.lines().nth(2).unwrap();
        assert!(
            row.starts_with(&format!("abc        {}  ", "a".repeat(13))),
            "{row}"
        );
        assert!(row.ends_with("/srv/projetos/aç"), "{row}");
    }

    #[test]
    fn test_mux_server_error_becomes_connection_error() {
        let srv = failing_mux();
        let mut c = srv.client();
        let (r, out) = run(|o| mux_list_to(&mut c, OutputFormat::Text, o));
        assert!(matches!(r, Err(CliError::Connection { message }) if message.contains("boom")));
        assert!(out.is_empty());
    }

    #[test]
    fn test_mux_create_detach_rename_destroy() {
        let srv = mux_server(two_sessions());
        let mut c = srv.client();

        let (r, out) = run(|o| mux_create_to(&mut c, Some("work".into()), 100, 30, o));
        r.unwrap();
        assert_eq!(
            out,
            "Created multiplexer session 'work' (ID: 11111111)\n\
             Use 'code-terminal-cli mux-attach work' to attach\n"
        );

        let (r, out) = run(|o| mux_detach_to(&mut c, o));
        r.unwrap();
        assert_eq!(out, "Detached from session\n");

        let (r, out) = run(|o| mux_rename_to(&mut c, ID_A, "novo", o));
        r.unwrap();
        assert_eq!(out, format!("Renamed session {} to 'novo'\n", ID_A));

        let (r, out) = run(|o| mux_destroy_to(&mut c, ID_B, o));
        r.unwrap();
        assert_eq!(out, format!("Destroyed session {}\n", ID_B));

        assert_eq!(
            srv.requests(),
            vec![
                MessageType::CreateSession {
                    name: Some("work".into()),
                    size: (100, 30)
                },
                MessageType::Detach,
                MessageType::RenameSession {
                    session_id: ID_A.into(),
                    new_name: "novo".into()
                },
                MessageType::DestroySession {
                    session_id: ID_B.into()
                },
            ]
        );
    }

    #[test]
    fn test_mux_mutations_report_server_errors() {
        let srv = failing_mux();
        let mut c = srv.client();
        let (r, _) = run(|o| mux_create_to(&mut c, None, 80, 24, o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
        let (r, _) = run(|o| mux_detach_to(&mut c, o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
        let (r, _) = run(|o| mux_rename_to(&mut c, "x", "y", o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
        let (r, _) = run(|o| mux_destroy_to(&mut c, "x", o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
        let dir = TempDir::new().unwrap();
        let p = persistence(&dir);
        let (r, _) = run(|o| mux_save_to(&mut c, &p, "x", o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
        let (r, _) = run(|o| mux_stats_to(&mut c, &p, OutputFormat::Text, o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
        let (r, _) = run(|o| mux_session_list_to(&mut c, OutputFormat::Text, None, o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
        let (r, _) = run(|o| mux_switch_to(&mut c, None, true, false, o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
        let (r, _) = run(|o| mux_last_to(&mut c, o));
        assert!(matches!(r, Err(CliError::Connection { .. })));
    }

    #[test]
    fn test_mux_save_keeps_server_session_id_and_restore_finds_it() {
        // Regressão: o save criava uma sessão com ID novo e o restore pelo ID
        // original falhava
        let dir = TempDir::new().unwrap();
        let p = persistence(&dir);
        let srv = mux_server(two_sessions());
        let mut c = srv.client();

        let (r, out) = run(|o| mux_save_to(&mut c, &p, "2222", o));
        r.unwrap();
        assert_eq!(out, "Session 'beta' saved to disk\n");
        let id_b = MuxId(ID_B.parse().unwrap());
        assert!(p.session_exists(id_b));
        let data = p.load_session(id_b).unwrap();
        assert_eq!(data.name, "beta");
        assert_eq!(data.working_dir, "/srv/b");

        let (r, out) = run(|o| mux_restore_to(&p, ID_B, o));
        r.unwrap();
        assert!(out.starts_with("Restored session 'beta' (ID: 22222222)\n"));
        assert!(out.contains("Working directory: /srv/b\n"));

        let (r, _) = run(|o| mux_save_to(&mut c, &p, "ffff", o));
        assert!(matches!(r, Err(CliError::CommandNotFound(m)) if m.contains("ffff")));
    }

    #[test]
    fn test_mux_restore_errors() {
        let dir = TempDir::new().unwrap();
        let p = persistence(&dir);
        let (r, _) = run(|o| mux_restore_to(&p, "xyz", o));
        assert!(matches!(r, Err(CliError::InvalidInput(m)) if m.contains("xyz")));
        let (r, _) = run(|o| mux_restore_to(&p, ID_A, o));
        assert!(matches!(r, Err(CliError::Other(_))));
    }

    #[test]
    fn test_mux_saved_formats() {
        let dir = TempDir::new().unwrap();
        let p = persistence(&dir);
        let (r, out) = run(|o| mux_saved_to(&p, OutputFormat::Text, o));
        r.unwrap();
        assert_eq!(out, "No saved sessions\n");

        // Regressão: cortes no meio de caracteres multibyte causavam pânico
        let name = format!("{}ção", "a".repeat(13));
        let srv = mux_server(vec![sinfo(
            ID_A,
            &name,
            "active",
            0,
            "/srv/projetos/açõespendentes",
        )]);
        let mut c = srv.client();
        run(|o| mux_save_to(&mut c, &p, ID_A, o)).0.unwrap();

        let (r, out) = run(|o| mux_saved_to(&p, OutputFormat::Text, o));
        r.unwrap();
        let row = out.lines().nth(2).unwrap();
        assert!(
            row.starts_with(&format!("11111111   {}  ", "a".repeat(13))),
            "{row}"
        );
        assert!(row.contains("/srv/projetos/aç "), "{row}");
        assert!(out.ends_with("Total: 1 saved session(s)\n"));

        let (r, out) = run(|o| mux_saved_to(&p, OutputFormat::Json, o));
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v[0]["name"], name.as_str());
        let (r, out) = run(|o| mux_saved_to(&p, OutputFormat::Yaml, o));
        r.unwrap();
        assert!(out.contains("working_dir: /srv/projetos/açõespendentes"));
    }

    #[test]
    fn test_mux_saved_and_cleanup_fail_without_directory() {
        let dir = TempDir::new().unwrap();
        let p = persistence(&dir);
        std::fs::remove_dir_all(dir.path().join("mux")).unwrap();
        let (r, _) = run(|o| mux_saved_to(&p, OutputFormat::Text, o));
        assert!(matches!(r, Err(CliError::Other(_))));
        let (r, _) = run(|o| mux_cleanup_to(&p, 1, o));
        assert!(matches!(r, Err(CliError::Other(_))));
        let srv = mux_server(vec![]);
        let mut c = srv.client();
        let (r, _) = run(|o| mux_stats_to(&mut c, &p, OutputFormat::Text, o));
        assert!(matches!(r, Err(CliError::Other(_))));
    }

    #[test]
    fn test_mux_stats_formats() {
        let dir = TempDir::new().unwrap();
        let p = persistence(&dir);
        let srv = mux_server(two_sessions());
        let mut c = srv.client();
        run(|o| mux_save_to(&mut c, &p, ID_A, o)).0.unwrap();

        let (r, out) = run(|o| mux_stats_to(&mut c, &p, OutputFormat::Text, o));
        r.unwrap();
        assert!(out.contains("Active sessions: 2\n"));
        assert!(out.contains("Saved sessions:  1\n"));
        assert!(out.contains("  - alpha (2 clients, active)\n"));
        assert!(out.contains("  - beta (0 clients, background)\n"));

        let (r, out) = run(|o| mux_stats_to(&mut c, &p, OutputFormat::Json, o));
        r.unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["active_sessions"], 2);
        assert_eq!(v["saved_sessions"], 1);
        assert!(v["persistence_size_bytes"].as_u64().unwrap() > 0);
        let (r, out) = run(|o| mux_stats_to(&mut c, &p, OutputFormat::Yaml, o));
        r.unwrap();
        assert!(out.contains("active_sessions: 2"));

        let srv = mux_server(vec![]);
        let mut c = srv.client();
        let (r, out) = run(|o| mux_stats_to(&mut c, &p, OutputFormat::Text, o));
        r.unwrap();
        assert!(!out.contains("Active Sessions:"));
    }

    #[test]
    fn test_mux_cleanup_removes_only_old_sessions() {
        let dir = TempDir::new().unwrap();
        let p = persistence(&dir);
        let srv = mux_server(two_sessions());
        let mut c = srv.client();
        run(|o| mux_save_to(&mut c, &p, ID_A, o)).0.unwrap();

        let (r, out) = run(|o| mux_cleanup_to(&p, 30, o));
        r.unwrap();
        assert_eq!(out, "Cleaned up 0 old session(s)\n");

        // envelhece a sessão salva
        let id = MuxId(ID_A.parse().unwrap());
        let mut data = p.load_session(id).unwrap();
        data.last_activity -= chrono::Duration::days(40);
        std::fs::write(
            dir.path().join("mux").join(format!("{}.json", ID_A)),
            serde_json::to_string(&data).unwrap(),
        )
        .unwrap();

        let (r, out) = run(|o| mux_cleanup_to(&p, 30, o));
        r.unwrap();
        assert_eq!(out, "Cleaned up 1 old session(s)\n");
        assert!(!p.session_exists(id));
    }

    #[test]
    fn test_mux_session_list_sorting_and_formats() {
        let sessions = vec![
            sinfo(ID_B, "zeta", "background", 0, "/z"),
            sinfo(ID_A, "alpha", "active", 3, "/a"),
        ];
        let srv = mux_server(sessions);
        let mut c = srv.client();

        let names = |out: &str| -> Vec<String> {
            let v: Value = serde_json::from_str(out).unwrap();
            v.as_array()
                .unwrap()
                .iter()
                .map(|s| s["name"].as_str().unwrap().to_string())
                .collect()
        };

        let (r, out) = run(|o| mux_session_list_to(&mut c, OutputFormat::Json, None, o));
        r.unwrap();
        assert_eq!(names(&out), vec!["zeta", "alpha"]);
        for (sort, expected) in [
            ("name", vec!["alpha", "zeta"]),
            ("clients", vec!["alpha", "zeta"]),
            ("created", vec!["alpha", "zeta"]),
            ("activity", vec!["zeta", "alpha"]),
            ("qualquer", vec!["zeta", "alpha"]),
        ] {
            let (r, out) =
                run(|o| mux_session_list_to(&mut c, OutputFormat::Json, Some(sort.to_string()), o));
            r.unwrap();
            assert_eq!(names(&out), expected, "ordenação {sort}");
        }

        let (r, out) =
            run(|o| mux_session_list_to(&mut c, OutputFormat::Text, Some("name".into()), o));
        r.unwrap();
        assert!(out.contains("alpha") && out.contains("zeta"));
        assert!(out.contains("Total: 2 sessions, 1 active, 2 attachable"));

        let (r, out) = run(|o| mux_session_list_to(&mut c, OutputFormat::Yaml, None, o));
        r.unwrap();
        assert!(out.contains("name: zeta"));
    }

    fn attached_ids(srv: &FakeMux) -> Vec<String> {
        srv.requests()
            .into_iter()
            .filter_map(|m| match m {
                MessageType::Attach { session_id, .. } => Some(session_id),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_mux_switch_by_name_attaches_with_full_uuid() {
        let srv = mux_server(two_sessions());
        let mut c = srv.client();
        let (r, out) = run(|o| mux_switch_to(&mut c, Some("BET".into()), false, false, o));
        r.unwrap();
        assert_eq!(out, "Switched to session 'beta' (22222222)\n");
        // o servidor recebe o UUID completo, não só o primeiro bloco
        assert_eq!(attached_ids(&srv), vec![ID_B.to_string()]);

        let (r, _) = run(|o| mux_switch_to(&mut c, Some("gamma".into()), false, false, o));
        assert!(matches!(r, Err(CliError::CommandNotFound(m)) if m.contains("gamma")));
    }

    #[test]
    fn test_mux_switch_next_prev_and_listing() {
        let three = vec![
            sinfo(ID_A, "a", "active", 1, "/"),
            sinfo(ID_B, "b", "active", 1, "/"),
            sinfo(
                "33333333-3333-4333-8333-333333333333",
                "c",
                "active",
                1,
                "/",
            ),
        ];
        let srv = mux_server(three);
        let mut c = srv.client();
        let (r, out) = run(|o| mux_switch_to(&mut c, None, true, false, o));
        r.unwrap();
        assert_eq!(out, "Switched to session 'b' (22222222)\n");
        let (r, out) = run(|o| mux_switch_to(&mut c, None, false, true, o));
        r.unwrap();
        assert_eq!(out, "Switched to session 'c' (33333333)\n");
        assert_eq!(
            attached_ids(&srv),
            vec![
                ID_B.to_string(),
                "33333333-3333-4333-8333-333333333333".to_string()
            ]
        );

        let (r, out) = run(|o| mux_switch_to(&mut c, None, false, false, o));
        r.unwrap();
        assert_eq!(
            out,
            "Available sessions:\n► 1: a\n  2: b\n  3: c\n\nUse 'mux-switch <name>' or 'mux-switch --next'\n"
        );
        assert_eq!(attached_ids(&srv).len(), 2);
    }

    #[test]
    fn test_mux_switch_with_no_sessions_does_nothing() {
        let srv = mux_server(vec![]);
        let mut c = srv.client();
        let (r, out) = run(|o| mux_switch_to(&mut c, None, true, false, o));
        r.unwrap();
        assert!(out.is_empty());
        assert!(attached_ids(&srv).is_empty());
    }

    #[test]
    fn test_mux_last_without_history() {
        let srv = mux_server(two_sessions());
        let mut c = srv.client();
        let (r, out) = run(|o| mux_last_to(&mut c, o));
        r.unwrap();
        assert_eq!(out, "No previous session\n");
        assert!(attached_ids(&srv).is_empty());
    }

    #[test]
    fn test_mux_groups_actions() {
        let (r, out) = run(|o| mux_groups_to("list", &[], o));
        r.unwrap();
        assert_eq!(out, "No groups defined\n");

        let (r, out) = run(|o| mux_groups_to("create", &["dev".into()], o));
        r.unwrap();
        assert!(out.starts_with("Created group 'dev' (ID: "));

        let (r, _) = run(|o| mux_groups_to("create", &[], o));
        assert!(matches!(r, Err(CliError::InvalidInput(m)) if m == "Group name required"));
        let (r, _) = run(|o| mux_groups_to("delete", &[], o));
        assert!(matches!(r, Err(CliError::InvalidInput(m)) if m == "Group ID required"));
        let (r, out) = run(|o| mux_groups_to("delete", &["x".into()], o));
        r.unwrap();
        assert!(out.contains("requires full implementation"));
        let (r, _) = run(|o| mux_groups_to("rename", &[], o));
        assert!(matches!(r, Err(CliError::InvalidInput(m)) if m.contains("rename")));

        rt().block_on(mux_groups("list", &[])).unwrap();
    }

    #[test]
    fn test_parse_session_state_mapping() {
        use crate::multiplexer::SessionState as S;
        assert_eq!(parse_session_state("creating"), S::Creating);
        assert_eq!(parse_session_state("active"), S::Active);
        assert_eq!(parse_session_state("background"), S::Background);
        assert_eq!(parse_session_state("destroying"), S::Destroying);
        assert_eq!(parse_session_state("???"), S::Background);
    }

    #[test]
    fn test_to_mux_session_keeps_fields_and_tolerates_bad_id() {
        let s = to_mux_session(&sinfo(ID_A, "x", "active", 4, "/w"));
        assert_eq!(s.id.0.to_string(), ID_A);
        assert_eq!(s.client_count, 4);
        assert_eq!(s.working_dir, "/w");
        assert_eq!(s.size, (80, 24));
        let s = to_mux_session(&sinfo("curto", "x", "active", 0, "/"));
        assert_ne!(s.id.0.to_string(), "curto");
    }

    #[test]
    fn test_mux_err_wraps_message() {
        let e = mux_err(MultiplexerError::NotAttached);
        assert!(matches!(e, CliError::Connection { message } if !message.is_empty()));
    }

    // ------------------------------------------------------------------
    // parse_keys
    // ------------------------------------------------------------------

    #[test]
    fn test_parse_keys_basic() {
        assert_eq!(parse_keys("\\n"), "\n");
        assert_eq!(parse_keys("\\t"), "\t");
        assert_eq!(parse_keys("\\\\"), "\\");
    }

    #[test]
    fn test_parse_keys_special() {
        assert_eq!(parse_keys("<Enter>"), "\r");
        assert_eq!(parse_keys("<Return>"), "\r");
        assert_eq!(parse_keys("<Tab>"), "\t");
        assert_eq!(parse_keys("<Esc>"), "\x1b");
        assert_eq!(parse_keys("<Space>"), " ");
        assert_eq!(parse_keys("<Backspace>"), "\x7f");
        assert_eq!(parse_keys("<Home>"), "\x1b[H");
        assert_eq!(parse_keys("<End>"), "\x1b[F");
    }

    #[test]
    fn test_parse_keys_arrows() {
        assert_eq!(parse_keys("<Up>"), "\x1b[A");
        assert_eq!(parse_keys("<Down>"), "\x1b[B");
        assert_eq!(parse_keys("<Right>"), "\x1b[C");
        assert_eq!(parse_keys("<Left>"), "\x1b[D");
    }

    #[test]
    fn test_parse_keys_ctrl() {
        assert_eq!(parse_keys("<C-c>"), "\x03");
        assert_eq!(parse_keys("<C-d>"), "\x04");
        assert_eq!(parse_keys("<C-z>"), "\x1a");
        assert_eq!(parse_keys("<C-l>"), "\x0c");
        assert_eq!(parse_keys("<C-a>"), "\x01");
    }

    #[test]
    fn test_parse_keys_function() {
        assert_eq!(parse_keys("<F1>"), "\x1bOP");
        assert_eq!(parse_keys("<F2>"), "\x1bOQ");
        assert_eq!(parse_keys("<F12>"), "\x1b[24~");
    }

    #[test]
    fn test_parse_keys_combined() {
        let result = parse_keys("hello\\nworld<Enter>");
        assert_eq!(result, "hello\nworld\r");

        let result = parse_keys("<Esc>:wq<Enter>");
        assert_eq!(result, "\x1b:wq\r");

        let result = parse_keys("<C-c>");
        assert_eq!(result, "\x03");
    }
}
