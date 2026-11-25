//! Code Terminal CLI Tool
//!
//! Ferramenta de linha de comando para controle remoto do terminal.
//! Comunica-se com o terminal via JSON-RPC over socket.

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::{generate, shells};
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use code_terminal::cli::commands;
use code_terminal::cli::{
    CliConfig, CliResult, ConfigAction, OutputFormat, RemoteClient, SplitDirection,
};

/// Code Terminal CLI - Controle remoto do terminal
///
/// Ferramenta de linha de comando para controlar o Code Terminal remotamente
/// via JSON-RPC over socket ou TCP.
///
/// Exemplos:
///     code-terminal-cli list-tabs
///     code-terminal-cli new-tab --title "My Project"
///     code-terminal-cli send-keys "<C-c>"
///     code-terminal-cli notify "Build complete!"
#[derive(Parser, Debug)]
#[command(
    name = "code-terminal-cli",
    version,
    about = "CLI tool for controlling Code Terminal remotely",
    long_about = None,
    color = clap::ColorChoice::Auto
)]
struct Cli {
    /// Path to socket file (Unix) or TCP address
    #[arg(short, long, global = true)]
    socket: Option<String>,

    /// Configuration file path
    #[arg(short, long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Output format
    #[arg(short, long, global = true, value_enum, default_value = "text")]
    format: OutputFormat,

    /// Verbose output
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Quiet mode (suppress output)
    #[arg(short, long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Supported shells for completion
#[derive(Debug, Clone, Copy, ValueEnum)]
#[allow(clippy::enum_variant_names)]
enum Shell {
    /// Bash shell
    Bash,
    /// Zsh shell
    Zsh,
    /// Fish shell
    Fish,
    /// PowerShell
    PowerShell,
    /// Elvish shell
    Elvish,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// List all tabs
    #[command(alias = "ls")]
    ListTabs {
        /// Show all details
        #[arg(short, long)]
        long: bool,
    },

    /// Create a new tab
    ///
    /// Examples:
    ///     cli new-tab --title "My Project"
    ///     cli new -d ~/projects -- bash -l
    ///     cli new -- vim
    #[command(alias = "new")]
    NewTab {
        /// Tab title
        #[arg(short, long)]
        title: Option<String>,
        /// Working directory
        #[arg(short = 'd', long, value_name = "DIR")]
        cwd: Option<PathBuf>,
        /// Shell command to execute
        #[arg(trailing_var_arg = true, help = "Command and arguments to execute")]
        command: Vec<String>,
    },

    /// Close a tab
    #[command(alias = "close")]
    CloseTab {
        /// Tab index (0-based)
        #[arg(default_value = "0")]
        index: usize,
    },

    /// Rename a tab (the title stays fixed); without a title, back to automatic
    ///
    /// Examples:
    ///     cli rename-tab 0 "api"
    ///     cli rename-tab 0            # automatic title again
    RenameTab {
        /// Tab index (0-based)
        index: usize,
        /// New title
        title: Option<String>,
    },

    /// Switch to a specific tab
    #[command(alias = "switch")]
    GotoTab {
        /// Tab index (0-based)
        index: usize,
    },

    /// Send keys to the active tab (supports special keys like <C-c>, <Enter>, <Esc>)
    ///
    /// Examples:
    ///     cli send "<C-c>"              # Send Ctrl+C
    ///     cli send "<Esc>:wq<Enter>"    # Vim command
    ///     cli send "<Up><Up><Enter>"    # Navigate history
    #[command(alias = "send")]
    SendKeys {
        /// Keys to send (use special notation like <C-c>, <Enter>, <Esc>)
        #[arg(value_name = "KEYS")]
        keys: String,
    },

    /// Send text/input to the active tab
    Input {
        /// Text to input
        text: String,
    },

    /// Get or set terminal size
    Size {
        /// Set columns
        #[arg(short = 'w', long, requires = "rows")]
        cols: Option<u16>,
        /// Set rows
        #[arg(short = 'r', long, requires = "cols")]
        rows: Option<u16>,
    },

    /// Show terminal info
    Info,

    /// Send system notification
    ///
    /// Examples:
    ///     cli notify "Build complete!"
    ///     cli notify -t "Done" "Task finished"
    Notify {
        /// Notification title
        #[arg(short, long, default_value = "Code Terminal")]
        title: String,
        /// Notification message
        #[arg(value_name = "MESSAGE")]
        message: String,
    },

    /// Notify Code Terminal from an AI agent hook (reads the hook JSON on stdin).
    ///
    /// Example (Claude Code): see `agent-hook-config`.
    AgentHook {
        /// Agent name shown in the notification
        #[arg(long, default_value = "Claude Code")]
        agent: String,
    },

    /// Print the Claude Code settings.json snippet that wires its hooks to Code Terminal
    AgentHookConfig,

    /// Open a URL (or local file) in the Code Terminal browser window
    ///
    /// Examples:
    ///     code-terminal-cli browse localhost:3000
    ///     code-terminal-cli browse docs/index.html
    Browse {
        /// Address or path (empty opens a blank page)
        #[arg(default_value = "")]
        url: String,
    },

    /// Control the browser window
    ///
    /// Examples:
    ///     code-terminal-cli browser text
    ///     code-terminal-cli browser eval "document.title"
    ///     code-terminal-cli browser back
    Browser {
        #[command(subcommand)]
        action: BrowserAction,
    },

    /// Preview a file in the terminal: images inline, markdown and JSON formatted
    Preview {
        /// File to show
        file: PathBuf,
        /// Image width in cells
        #[arg(short, long)]
        width: Option<u32>,
    },

    /// Print the visible text of a pane (the current one by default)
    Read {
        /// Only the last N lines
        #[arg(short = 'n', long)]
        lines: Option<usize>,
        /// Pane id (see list-panes); defaults to $CODE_TERMINAL_PANE_ID
        #[arg(long)]
        pane: Option<u64>,
    },

    /// List all panes
    ListPanes,

    /// Split current pane
    Split {
        /// Split direction
        #[arg(value_enum, default_value = "horizontal")]
        direction: SplitDirection,
    },

    /// Configuration management
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Ping the server
    Ping,

    /// Show version information
    #[command(alias = "v")]
    Version,

    /// Generate shell completion scripts
    ///
    /// Examples:
    ///     code-terminal-cli completions bash > /etc/bash_completion.d/code-terminal-cli
    ///     code-terminal-cli completions zsh > /usr/local/share/zsh/site-functions/_code-terminal-cli
    ///     code-terminal-cli completions fish > ~/.config/fish/completions/code-terminal-cli.fish
    ///     code-terminal-cli completions power-shell > code-terminal-cli.ps1
    Completions {
        /// Shell to generate completions for (bash, zsh, fish, power-shell, elvish)
        shell: Shell,
    },

    /// List SSH hosts from ~/.ssh/config
    ///
    /// Examples:
    ///     code-terminal-cli ssh-hosts
    ///     code-terminal-cli ssh-hosts --query "prod"
    #[command(alias = "hosts")]
    SshHosts {
        /// Filter hosts by query
        #[arg(long)]
        query: Option<String>,
    },

    /// Connect to an SSH host
    ///
    /// Examples:
    ///     code-terminal-cli ssh-connect myserver
    ///     code-terminal-cli ssh user@host:port
    #[command(alias = "ssh")]
    SshConnect {
        /// Host to connect (can be user@host or host)
        host: String,
        /// Port number (default: 22)
        #[arg(short, long)]
        port: Option<u16>,
        /// User name (if not specified in host)
        #[arg(short, long)]
        user: Option<String>,
        /// Identity file path
        #[arg(short, long)]
        identity: Option<PathBuf>,
        /// Tab title
        #[arg(short, long)]
        title: Option<String>,
    },

    /// List SSH sessions
    ///
    /// Examples:
    ///     code-terminal-cli ssh-sessions
    ///     code-terminal-cli ssh-sessions -f json
    #[command(alias = "sessions")]
    SshSessions,

    /// Create a new SSH session
    ///
    /// Examples:
    ///     code-terminal-cli ssh-session-create prod-server prod.example.com admin -p 2222
    #[command(alias = "session-new")]
    SshSessionCreate {
        /// Session name
        name: String,
        /// Host to connect
        host: String,
        /// User name
        user: String,
        /// Port number (default: 22)
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Remove an SSH session
    ///
    /// Examples:
    ///     code-terminal-cli ssh-session-remove <id>
    #[command(alias = "session-rm")]
    SshSessionRemove {
        /// Session ID
        id: String,
    },

    // Multiplexer commands
    /// List multiplexer sessions
    ///
    /// Examples:
    ///     code-terminal-cli mux-list
    ///     code-terminal-cli mux-list -f json
    #[command(alias = "ml")]
    MuxList,

    /// Create a new multiplexer session
    ///
    /// Examples:
    ///     code-terminal-cli mux-create
    ///     code-terminal-cli mux-create --name mysession --cols 120 --rows 40
    #[command(alias = "mc")]
    MuxCreate {
        /// Session name
        #[arg(short, long)]
        name: Option<String>,
        /// Terminal columns (`-w`, como em `size`: `-c` é o `--config` global)
        #[arg(short = 'w', long, default_value = "80")]
        cols: u16,
        /// Terminal rows
        #[arg(short, long, default_value = "24")]
        rows: u16,
    },

    /// Attach to a multiplexer session (Ctrl+B d detaches; the shell keeps running)
    ///
    /// Examples:
    ///     code-terminal-cli mux-attach <id-or-name>
    ///     code-terminal-cli mux-attach work --create
    ///     code-terminal-cli mux-attach work --read-only
    #[command(alias = "ma")]
    MuxAttach {
        /// Session ID (or prefix) or name
        session: String,
        /// Read-only mode
        #[arg(short, long)]
        read_only: bool,
        /// Create the session (and start the daemon) if it does not exist
        #[arg(long)]
        create: bool,
    },

    /// Run the multiplexer daemon in the foreground (started on demand by
    /// mux-create and mux-attach --create)
    #[command(hide = true)]
    MuxServer {
        /// Keep running with no sessions (systemd service: persistent tabs
        /// come back even before a window opens)
        #[arg(long)]
        keep_alive: bool,
    },

    /// Detach from current multiplexer session
    ///
    /// Examples:
    ///     code-terminal-cli mux-detach
    #[command(alias = "md")]
    MuxDetach,

    /// Rename a multiplexer session
    ///
    /// Examples:
    ///     code-terminal-cli mux-rename <id> <new-name>
    MuxRename {
        /// Session ID
        id: String,
        /// New name
        name: String,
    },

    /// Destroy a multiplexer session
    ///
    /// Examples:
    ///     code-terminal-cli mux-destroy <id>
    #[command(alias = "mux-rm")]
    MuxDestroy {
        /// Session ID
        id: String,
    },

    // Session Management commands (TASK-148)
    /// Save a session to disk
    ///
    /// Examples:
    ///     code-terminal-cli mux-save <id>
    #[command(alias = "ms")]
    MuxSave {
        /// Session ID
        id: String,
    },

    /// List saved sessions
    ///
    /// Examples:
    ///     code-terminal-cli mux-saved
    ///     code-terminal-cli mux-saved -f json
    #[command(alias = "msd")]
    MuxSaved,

    /// Restore a saved session
    ///
    /// Examples:
    ///     code-terminal-cli mux-restore <id>
    MuxRestore {
        /// Session ID
        id: String,
    },

    /// Show multiplexer statistics
    ///
    /// Examples:
    ///     code-terminal-cli mux-stats
    ///     code-terminal-cli mux-stats -f json
    MuxStats,

    /// Manage session groups
    ///
    /// Examples:
    ///     code-terminal-cli mux-groups list
    ///     code-terminal-cli mux-groups create mygroup
    MuxGroups {
        /// Action (list, create, delete)
        action: String,
        /// Additional arguments
        args: Vec<String>,
    },

    /// Cleanup old sessions
    ///
    /// Examples:
    ///     code-terminal-cli mux-cleanup --days 30
    MuxCleanup {
        /// Days to keep
        #[arg(short, long, default_value = "30")]
        days: u32,
    },

    // Session List and Switch (TASK-151 & TASK-152)
    /// Interactive session list
    ///
    /// Examples:
    ///     code-terminal-cli mux-session-list
    ///     code-terminal-cli mux-session-list -f json --sort name
    #[command(alias = "sl")]
    MuxSessionList {
        /// Sort by (name, created, activity, clients) (`-o`: `-s` é o `--socket` global)
        #[arg(short = 'o', long)]
        sort: Option<String>,
    },

    /// Switch to another session
    ///
    /// Examples:
    ///     code-terminal-cli mux-switch mysession
    ///     code-terminal-cli mux-switch --next
    ///     code-terminal-cli mux-switch --prev
    #[command(alias = "sw")]
    MuxSwitch {
        /// Session name or index
        target: Option<String>,
        /// Switch to next session
        #[arg(short, long)]
        next: bool,
        /// Switch to previous session
        #[arg(short, long)]
        prev: bool,
    },

    /// Switch to last used session
    ///
    /// Examples:
    ///     code-terminal-cli mux-last
    #[command(alias = "L")]
    MuxLast,
}

/// Hook de agente: nunca falha (um erro aqui atrapalharia o agente). Fora
/// do Code Terminal, ou sem nada a avisar, não faz nada.
async fn run_agent_hook(agent: &str, socket: Option<&str>) {
    use std::io::Read;
    let mut input = String::new();
    let _ = io::stdin().read_to_string(&mut input);
    let payload: serde_json::Value = serde_json::from_str(&input).unwrap_or_default();
    // Aba persistente: o daemon guarda a conversa (para `claude --resume`
    // depois do reboot) e diz em que aba o agente está.
    let mux_session = report_agent_to_daemon(agent, &payload)
        .or_else(|| std::env::var("CODE_TERMINAL_MUX_SESSION").ok());
    let Some(note) = code_terminal::cli::agent_hook::notification_from_hook(agent, &payload) else {
        return;
    };
    let address = socket
        .map(str::to_string)
        .or_else(|| std::env::var("CODE_TERMINAL_SOCKET").ok());
    let Some(address) = address else {
        return;
    };
    if let Ok(mut client) = RemoteClient::connect(&address).await {
        let _ = client.notify_in(&note.title, &note.body, mux_session.as_deref());
    }
}

#[cfg(unix)]
fn report_agent_to_daemon(agent: &str, payload: &serde_json::Value) -> Option<String> {
    let report = code_terminal::cli::agent_hook::agent_report(agent, payload)?;
    code_terminal::multiplexer::app_client::report_agent(
        std::process::id(),
        &report.kind,
        report.session_id.as_deref(),
        report.cwd.as_deref(),
        report.active,
    )
}

#[cfg(not(unix))]
fn report_agent_to_daemon(_agent: &str, _payload: &serde_json::Value) -> Option<String> {
    None
}

/// Endereço padrão da Socket API: o socket Unix no Linux/macOS (o servidor
/// não abre TCP nesses sistemas) e TCP local no Windows.
fn default_socket_address() -> String {
    #[cfg(unix)]
    {
        code_terminal::multiplexer::socket_api::server::default_unix_socket_path()
            .display()
            .to_string()
    }
    #[cfg(not(unix))]
    {
        "127.0.0.1:9123".to_string()
    }
}

/// Ações do navegador em janela própria.
#[derive(Subcommand, Debug, Clone)]
enum BrowserAction {
    /// Previous page
    Back,
    /// Next page
    Forward,
    /// Reload the page
    Reload,
    /// Current URL and title
    Info,
    /// Visible text of the page
    Text,
    /// Run JavaScript and print the result (JSON)
    Eval { js: String },
    /// Close the browser window
    Close,
}

/// Mostra a resposta do navegador: texto/resultado, ou URL e título.
fn print_browser_reply(reply: &code_terminal::browser::remote::BrowserReply) {
    if let Some(error) = &reply.error {
        eprintln!("browser: {error}");
        return;
    }
    if let Some(result) = &reply.result {
        println!("{result}");
        return;
    }
    if let Some(url) = &reply.url {
        println!("{url}");
    }
    if let Some(title) = &reply.title {
        println!("{title}");
    }
}

/// Idioma da CLI: o `language` do config.toml do app (o mesmo da tela de
/// Configurações) e, sem ele ou com "auto", o do sistema.
fn cli_language() -> code_terminal::ui::i18n::Language {
    let configured = code_terminal::config::Config::config_path()
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("language")?.as_str().map(str::to_string))
        .unwrap_or_default();
    code_terminal::ui::i18n::language_from_config(&configured)
}

#[tokio::main]
async fn main() -> ExitCode {
    // Ajuda e erros no idioma da interface (#32).
    let language = cli_language();
    code_terminal::ui::i18n::set_language(language);
    let matches = code_terminal::cli::help_pt::localize(Cli::command(), language).get_matches();
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let quiet = cli.quiet;

    // Setup logging based on verbose level
    setup_logging(cli.verbose);

    if let Err(e) = run(cli).await {
        if !quiet {
            eprintln!(
                "{} {}",
                code_terminal::cli::CliError::prefix(),
                e.localized()
            );
        }
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

async fn run(cli: Cli) -> CliResult<()> {
    // Handle commands that don't need connection
    match &cli.command {
        Commands::Version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Commands::Config {
            action: ConfigAction::Show,
        } => {
            let config = CliConfig::load(cli.config.as_deref())?;
            println!("{:#?}", config);
            return Ok(());
        }
        Commands::Browse { url } => {
            let reply = code_terminal::browser::remote::open_in_browser(url)
                .map_err(code_terminal::cli::CliError::InvalidInput)?;
            print_browser_reply(&reply);
            return Ok(());
        }
        Commands::Browser { action } => {
            use code_terminal::browser::remote::{request, BrowserRequest};
            let req = match action {
                BrowserAction::Back => BrowserRequest::Back,
                BrowserAction::Forward => BrowserRequest::Forward,
                BrowserAction::Reload => BrowserRequest::Reload,
                BrowserAction::Info => BrowserRequest::Info,
                BrowserAction::Text => BrowserRequest::Text,
                BrowserAction::Eval { js } => BrowserRequest::Eval { js: js.clone() },
                BrowserAction::Close => BrowserRequest::Close,
            };
            let reply = request(req).map_err(|e| code_terminal::cli::CliError::Connection {
                message: format!("browser window not running ({e}); use 'browse <url>'"),
            })?;
            print_browser_reply(&reply);
            return Ok(());
        }
        Commands::Preview { file, width } => {
            match code_terminal::cli::preview::preview(file, *width) {
                Ok(output) => print!("{output}"),
                Err(e) => return Err(code_terminal::cli::CliError::InvalidInput(e)),
            }
            return Ok(());
        }
        Commands::AgentHookConfig => {
            let snippet = code_terminal::cli::agent_hook::claude_settings_snippet();
            println!("{}", serde_json::to_string_pretty(&snippet)?);
            return Ok(());
        }
        Commands::AgentHook { agent } => {
            run_agent_hook(agent, cli.socket.as_deref()).await;
            return Ok(());
        }
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            let bin_name = cmd.get_name().to_string();
            match shell {
                Shell::Bash => generate(shells::Bash, &mut cmd, bin_name, &mut io::stdout()),
                Shell::Zsh => generate(shells::Zsh, &mut cmd, bin_name, &mut io::stdout()),
                Shell::Fish => generate(shells::Fish, &mut cmd, bin_name, &mut io::stdout()),
                Shell::PowerShell => {
                    generate(shells::PowerShell, &mut cmd, bin_name, &mut io::stdout())
                }
                Shell::Elvish => generate(shells::Elvish, &mut cmd, bin_name, &mut io::stdout()),
            }
            return Ok(());
        }
        _ => {}
    }

    // Comandos que não usam o servidor JSON-RPC do terminal: SSH local e
    // multiplexer (que fala com o próprio servidor de sessões)
    let format = cli.format;
    let command = match cli.command {
        Commands::SshHosts { query } => return commands::ssh_hosts(query.as_deref(), format).await,
        Commands::SshSessions => return commands::ssh_sessions(format).await,
        Commands::SshSessionCreate {
            name,
            host,
            user,
            port,
        } => {
            return commands::ssh_session_create(name.as_str(), host.as_str(), user.as_str(), port)
                .await
        }
        Commands::SshSessionRemove { id } => {
            return commands::ssh_session_remove(id.as_str()).await
        }
        Commands::MuxList => return commands::mux_list(format).await,
        Commands::MuxCreate { name, cols, rows } => {
            return commands::mux_create(name, cols, rows).await
        }
        Commands::MuxAttach {
            session,
            read_only,
            create,
        } => return commands::mux_attach(session.as_str(), read_only, create).await,
        Commands::MuxServer { keep_alive } => return commands::mux_server(keep_alive).await,
        Commands::MuxDetach => return commands::mux_detach().await,
        Commands::MuxRename { id, name } => {
            return commands::mux_rename(id.as_str(), name.as_str()).await
        }
        Commands::MuxDestroy { id } => return commands::mux_destroy(id.as_str()).await,
        // Session Management commands (TASK-148)
        Commands::MuxSave { id } => return commands::mux_save(id.as_str()).await,
        Commands::MuxSaved => return commands::mux_saved(format).await,
        Commands::MuxRestore { id } => return commands::mux_restore(id.as_str()).await,
        Commands::MuxStats => return commands::mux_stats(format).await,
        Commands::MuxGroups { action, args } => {
            return commands::mux_groups(action.as_str(), args.as_slice()).await
        }
        Commands::MuxCleanup { days } => return commands::mux_cleanup(days).await,
        // Session List and Switch (TASK-151 & TASK-152)
        Commands::MuxSessionList { sort } => return commands::mux_session_list(format, sort).await,
        Commands::MuxSwitch { target, next, prev } => {
            return commands::mux_switch(target, next, prev).await
        }
        Commands::MuxLast => return commands::mux_last().await,
        other => other,
    };

    // Load configuration
    let config = CliConfig::load(cli.config.as_deref())?;

    // Determine socket path/address
    let socket_addr = cli
        .socket
        .or_else(|| {
            config
                .socket_path
                .map(|p: PathBuf| p.to_string_lossy().to_string())
        })
        .or_else(|| std::env::var("CODE_TERMINAL_SOCKET").ok())
        .unwrap_or_else(default_socket_address);

    // Create client
    let mut client = RemoteClient::connect(&socket_addr).await?;

    // Execute command
    match command {
        Commands::ListTabs { long } => {
            commands::list_tabs(&mut client, long, format).await?;
        }
        Commands::NewTab {
            title,
            cwd,
            command,
        } => {
            commands::new_tab(&mut client, title, cwd, command).await?;
        }
        Commands::CloseTab { index } => {
            commands::close_tab(&mut client, index).await?;
        }
        Commands::GotoTab { index } => {
            commands::goto_tab(&mut client, index).await?;
        }
        Commands::RenameTab { index, title } => {
            commands::rename_tab(&mut client, index, title).await?;
        }
        Commands::SendKeys { keys } => {
            commands::send_keys(&mut client, &keys).await?;
        }
        Commands::Input { text } => {
            commands::send_input(&mut client, &text).await?;
        }
        Commands::Size { cols, rows } => {
            if let (Some(c), Some(r)) = (cols, rows) {
                commands::set_size(&mut client, c, r).await?;
            } else {
                commands::get_size(&mut client, format).await?;
            }
        }
        Commands::Info => {
            commands::info(&mut client, format).await?;
        }
        Commands::Notify { title, message } => {
            commands::notify(&mut client, &title, &message).await?;
        }
        Commands::Read { lines, pane } => {
            println!("{}", client.read_screen(lines, pane)?);
        }
        Commands::ListPanes => {
            commands::list_panes(&mut client, format).await?;
        }
        Commands::Split { direction } => {
            commands::split(&mut client, direction).await?;
        }
        Commands::Config { action } => {
            commands::config(&mut client, action).await?;
        }
        Commands::Ping => {
            let start = std::time::Instant::now();
            client.ping().await?;
            let elapsed = start.elapsed();
            if !cli.quiet {
                println!("Pong! ({}ms)", elapsed.as_millis());
            }
        }
        Commands::SshConnect {
            host,
            port,
            user,
            identity,
            title,
        } => {
            commands::ssh_connect(
                &mut client,
                host.as_str(),
                port,
                user.as_deref(),
                identity.as_deref().map(std::path::Path::new),
                title.as_deref(),
            )
            .await?;
        }
        _ => unreachable!(),
    }

    Ok(())
}

fn setup_logging(verbose: u8) {
    let level = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(format!("code_terminal_cli={}", level))
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse_list_tabs() {
        let cli = Cli::parse_from(["code-terminal-cli", "list-tabs"]);
        match cli.command {
            Commands::ListTabs { long } => assert!(!long),
            _ => panic!("Expected ListTabs command"),
        }
    }

    #[test]
    fn test_cli_parse_list_tabs_long() {
        let cli = Cli::parse_from(["code-terminal-cli", "list-tabs", "-l"]);
        match cli.command {
            Commands::ListTabs { long } => assert!(long),
            _ => panic!("Expected ListTabs command"),
        }
    }

    #[test]
    fn test_cli_parse_new_tab_with_title() {
        let cli = Cli::parse_from(["code-terminal-cli", "new-tab", "-t", "test-tab"]);
        match cli.command {
            Commands::NewTab {
                title,
                cwd,
                command,
            } => {
                assert_eq!(title, Some("test-tab".to_string()));
                assert!(cwd.is_none());
                assert!(command.is_empty());
            }
            _ => panic!("Expected NewTab command"),
        }
    }

    #[test]
    fn test_cli_parse_new_tab_full() {
        let cli = Cli::parse_from([
            "code-terminal-cli",
            "new",
            "--title",
            "test",
            "-d",
            "/home/user",
            "--",
            "vim",
            "file.txt",
        ]);
        match cli.command {
            Commands::NewTab {
                title,
                cwd,
                command,
            } => {
                assert_eq!(title, Some("test".to_string()));
                assert_eq!(cwd, Some(PathBuf::from("/home/user")));
                assert_eq!(command, vec!["vim".to_string(), "file.txt".to_string()]);
            }
            _ => panic!("Expected NewTab command"),
        }
    }

    #[test]
    fn test_cli_parse_send_keys() {
        let cli = Cli::parse_from(["code-terminal-cli", "send", "<C-c>"]);
        match cli.command {
            Commands::SendKeys { keys } => {
                assert_eq!(keys, "<C-c>".to_string());
            }
            _ => panic!("Expected SendKeys command"),
        }
    }

    #[test]
    fn test_cli_parse_close_tab() {
        let cli = Cli::parse_from(["code-terminal-cli", "close-tab", "5"]);
        match cli.command {
            Commands::CloseTab { index } => assert_eq!(index, 5),
            _ => panic!("Expected CloseTab command"),
        }
    }

    #[test]
    fn test_cli_parse_close_tab_default() {
        let cli = Cli::parse_from(["code-terminal-cli", "close"]); // default index
        match cli.command {
            Commands::CloseTab { index } => assert_eq!(index, 0),
            _ => panic!("Expected CloseTab command"),
        }
    }

    #[test]
    fn test_cli_parse_goto_tab() {
        let cli = Cli::parse_from(["code-terminal-cli", "goto-tab", "3"]);
        match cli.command {
            Commands::GotoTab { index } => assert_eq!(index, 3),
            _ => panic!("Expected GotoTab command"),
        }
    }

    #[test]
    fn test_cli_parse_rename_tab() {
        let cli = Cli::parse_from(["code-terminal-cli", "rename-tab", "2", "api"]);
        match cli.command {
            Commands::RenameTab { index, title } => {
                assert_eq!((index, title.as_deref()), (2, Some("api")))
            }
            _ => panic!("Expected RenameTab command"),
        }
        let cli = Cli::parse_from(["code-terminal-cli", "rename-tab", "2"]);
        assert!(matches!(
            cli.command,
            Commands::RenameTab { title: None, .. }
        ));
    }

    #[test]
    fn test_cli_parse_input() {
        let cli = Cli::parse_from(["code-terminal-cli", "input", "hello world"]);
        match cli.command {
            Commands::Input { text } => assert_eq!(text, "hello world".to_string()),
            _ => panic!("Expected Input command"),
        }
    }

    #[test]
    fn test_cli_parse_size_get() {
        let cli = Cli::parse_from(["code-terminal-cli", "size"]);
        match cli.command {
            Commands::Size { cols, rows } => {
                assert!(cols.is_none());
                assert!(rows.is_none());
            }
            _ => panic!("Expected Size command"),
        }
    }

    #[test]
    fn test_cli_parse_size_set() {
        let cli = Cli::parse_from(["code-terminal-cli", "size", "-w", "120", "-r", "40"]);
        match cli.command {
            Commands::Size { cols, rows } => {
                assert_eq!(cols, Some(120));
                assert_eq!(rows, Some(40));
            }
            _ => panic!("Expected Size command"),
        }
    }

    #[test]
    fn test_cli_parse_info() {
        let cli = Cli::parse_from(["code-terminal-cli", "info"]);
        assert!(matches!(cli.command, Commands::Info));
    }

    #[test]
    fn test_cli_parse_notify() {
        let cli = Cli::parse_from([
            "code-terminal-cli",
            "notify",
            "--title",
            "Test",
            "Hello World",
        ]);
        match cli.command {
            Commands::Notify { title, message } => {
                assert_eq!(title, "Test".to_string());
                assert_eq!(message, "Hello World".to_string());
            }
            _ => panic!("Expected Notify command"),
        }
    }

    #[test]
    fn test_cli_parse_notify_default_title() {
        let cli = Cli::parse_from(["code-terminal-cli", "notify", "Hello"]);
        match cli.command {
            Commands::Notify { title, message } => {
                assert_eq!(title, "Code Terminal".to_string()); // default
                assert_eq!(message, "Hello".to_string());
            }
            _ => panic!("Expected Notify command"),
        }
    }

    #[test]
    fn test_cli_parse_list_panes() {
        let cli = Cli::parse_from(["code-terminal-cli", "list-panes"]);
        assert!(matches!(cli.command, Commands::ListPanes));
    }

    #[test]
    fn test_cli_parse_split_horizontal() {
        let cli = Cli::parse_from(["code-terminal-cli", "split", "horizontal"]);
        match cli.command {
            Commands::Split { direction } => {
                assert!(matches!(direction, SplitDirection::Horizontal));
            }
            _ => panic!("Expected Split command"),
        }
    }

    #[test]
    fn test_cli_parse_split_vertical() {
        let cli = Cli::parse_from(["code-terminal-cli", "split", "vertical"]);
        match cli.command {
            Commands::Split { direction } => {
                assert!(matches!(direction, SplitDirection::Vertical));
            }
            _ => panic!("Expected Split command"),
        }
    }

    #[test]
    fn test_cli_parse_config_get() {
        let cli = Cli::parse_from(["code-terminal-cli", "config", "get", "font.size"]);
        match cli.command {
            Commands::Config { action } => {
                assert!(matches!(action, ConfigAction::Get { key } if key == "font.size"));
            }
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_cli_parse_config_set() {
        let cli = Cli::parse_from(["code-terminal-cli", "config", "set", "theme", "dark"]);
        match cli.command {
            Commands::Config { action } => {
                assert!(
                    matches!(action, ConfigAction::Set { key, value } if key == "theme" && value == "dark")
                );
            }
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_cli_parse_config_show() {
        let cli = Cli::parse_from(["code-terminal-cli", "config", "show"]);
        match cli.command {
            Commands::Config { action } => {
                assert!(matches!(action, ConfigAction::Show));
            }
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_cli_parse_ping() {
        let cli = Cli::parse_from(["code-terminal-cli", "ping"]);
        assert!(matches!(cli.command, Commands::Ping));
    }

    #[test]
    fn test_cli_parse_version() {
        let cli = Cli::parse_from(["code-terminal-cli", "version"]);
        assert!(matches!(cli.command, Commands::Version));
    }

    #[test]
    fn test_cli_parse_completions_bash() {
        let cli = Cli::parse_from(["code-terminal-cli", "completions", "bash"]);
        match cli.command {
            Commands::Completions { shell } => {
                assert!(matches!(shell, Shell::Bash));
            }
            _ => panic!("Expected Completions command"),
        }
    }

    #[test]
    fn test_cli_parse_completions_zsh() {
        let cli = Cli::parse_from(["code-terminal-cli", "completions", "zsh"]);
        match cli.command {
            Commands::Completions { shell } => {
                assert!(matches!(shell, Shell::Zsh));
            }
            _ => panic!("Expected Completions command"),
        }
    }

    #[test]
    fn test_cli_parse_completions_fish() {
        let cli = Cli::parse_from(["code-terminal-cli", "completions", "fish"]);
        match cli.command {
            Commands::Completions { shell } => {
                assert!(matches!(shell, Shell::Fish));
            }
            _ => panic!("Expected Completions command"),
        }
    }

    #[test]
    fn test_cli_parse_completions_powershell() {
        let cli = Cli::parse_from(["code-terminal-cli", "completions", "power-shell"]);
        match cli.command {
            Commands::Completions { shell } => {
                assert!(matches!(shell, Shell::PowerShell));
            }
            _ => panic!("Expected Completions command"),
        }
    }

    #[test]
    fn test_cli_parse_completions_elvish() {
        let cli = Cli::parse_from(["code-terminal-cli", "completions", "elvish"]);
        match cli.command {
            Commands::Completions { shell } => {
                assert!(matches!(shell, Shell::Elvish));
            }
            _ => panic!("Expected Completions command"),
        }
    }

    #[test]
    fn test_cli_global_flags() {
        let cli = Cli::parse_from([
            "code-terminal-cli",
            "-s",
            "/tmp/socket.sock",
            "-v",
            "-f",
            "json",
            "ping",
        ]);
        assert_eq!(cli.socket, Some("/tmp/socket.sock".to_string()));
        assert_eq!(cli.verbose, 1);
        assert!(matches!(cli.format, OutputFormat::Json));
    }

    #[test]
    fn test_cli_multiple_verbose_flags() {
        let cli = Cli::parse_from(["code-terminal-cli", "-vvv", "ping"]);
        assert_eq!(cli.verbose, 3);
    }

    #[test]
    fn test_cli_default_socket_unix() {
        let cli = Cli::parse_from(["code-terminal-cli", "ping"]);
        #[cfg(unix)]
        assert_eq!(cli.socket, None); // Uses default internally
        #[cfg(windows)]
        assert_eq!(cli.socket, None); // Uses default internally
    }

    #[test]
    fn test_cli_default_format() {
        let cli = Cli::parse_from(["code-terminal-cli", "ping"]);
        assert!(matches!(cli.format, OutputFormat::Text));
    }

    #[test]
    fn test_cli_quiet_flag() {
        let cli = Cli::parse_from(["code-terminal-cli", "-q", "ping"]);
        assert!(cli.quiet);
    }

    #[test]
    fn test_cli_alias_ls() {
        let cli = Cli::parse_from(["code-terminal-cli", "ls"]);
        match cli.command {
            Commands::ListTabs { .. } => {}
            _ => panic!("Expected ListTabs command via ls alias"),
        }
    }

    #[test]
    fn test_cli_alias_new() {
        let cli = Cli::parse_from(["code-terminal-cli", "new"]);
        match cli.command {
            Commands::NewTab { .. } => {}
            _ => panic!("Expected NewTab command via new alias"),
        }
    }

    #[test]
    fn test_cli_alias_close() {
        let cli = Cli::parse_from(["code-terminal-cli", "close", "2"]);
        match cli.command {
            Commands::CloseTab { index } => assert_eq!(index, 2),
            _ => panic!("Expected CloseTab command via close alias"),
        }
    }

    #[test]
    fn test_cli_alias_switch() {
        let cli = Cli::parse_from(["code-terminal-cli", "switch", "1"]);
        match cli.command {
            Commands::GotoTab { index } => assert_eq!(index, 1),
            _ => panic!("Expected GotoTab command via switch alias"),
        }
    }

    #[test]
    fn test_cli_alias_send() {
        let cli = Cli::parse_from(["code-terminal-cli", "send", "test"]);
        match cli.command {
            Commands::SendKeys { keys } => assert_eq!(keys, "test"),
            _ => panic!("Expected SendKeys command via send alias"),
        }
    }

    #[test]
    fn test_cli_alias_v() {
        let cli = Cli::parse_from(["code-terminal-cli", "v"]);
        match cli.command {
            Commands::Version => {}
            _ => panic!("Expected Version command via v alias"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_hosts() {
        let cli = Cli::parse_from(["code-terminal-cli", "ssh-hosts"]);
        match cli.command {
            Commands::SshHosts { query } => assert!(query.is_none()),
            _ => panic!("Expected SshHosts command"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_hosts_with_query() {
        let cli = Cli::parse_from(["code-terminal-cli", "ssh-hosts", "--query", "prod"]);
        match cli.command {
            Commands::SshHosts { query } => assert_eq!(query, Some("prod".to_string())),
            _ => panic!("Expected SshHosts command"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_hosts_alias() {
        let cli = Cli::parse_from(["code-terminal-cli", "hosts"]);
        match cli.command {
            Commands::SshHosts { .. } => {}
            _ => panic!("Expected SshHosts command via hosts alias"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_connect() {
        let cli = Cli::parse_from(["code-terminal-cli", "ssh-connect", "myserver"]);
        match cli.command {
            Commands::SshConnect {
                host,
                port,
                user,
                identity,
                title,
            } => {
                assert_eq!(host, "myserver");
                assert!(port.is_none());
                assert!(user.is_none());
                assert!(identity.is_none());
                assert!(title.is_none());
            }
            _ => panic!("Expected SshConnect command"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_connect_full() {
        let cli = Cli::parse_from([
            "code-terminal-cli",
            "ssh",
            "user@example.com",
            "-p",
            "2222",
            "-u",
            "admin",
            "-i",
            "~/.ssh/id_rsa",
            "-t",
            "Production",
        ]);
        match cli.command {
            Commands::SshConnect {
                host,
                port,
                user,
                identity,
                title,
            } => {
                assert_eq!(host, "user@example.com");
                assert_eq!(port, Some(2222));
                assert_eq!(user, Some("admin".to_string()));
                assert_eq!(identity, Some(PathBuf::from("~/.ssh/id_rsa")));
                assert_eq!(title, Some("Production".to_string()));
            }
            _ => panic!("Expected SshConnect command"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_connect_alias() {
        let cli = Cli::parse_from(["code-terminal-cli", "ssh", "myserver"]);
        match cli.command {
            Commands::SshConnect { host, .. } => assert_eq!(host, "myserver"),
            _ => panic!("Expected SshConnect command via ssh alias"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_sessions() {
        let cli = Cli::parse_from(["code-terminal-cli", "ssh-sessions"]);
        assert!(matches!(cli.command, Commands::SshSessions));
    }

    #[test]
    fn test_cli_parse_ssh_sessions_alias() {
        let cli = Cli::parse_from(["code-terminal-cli", "sessions"]);
        assert!(matches!(cli.command, Commands::SshSessions));
    }

    #[test]
    fn test_cli_parse_ssh_session_create() {
        let cli = Cli::parse_from([
            "code-terminal-cli",
            "ssh-session-create",
            "prod",
            "prod.example.com",
            "admin",
        ]);
        match cli.command {
            Commands::SshSessionCreate {
                name,
                host,
                user,
                port,
            } => {
                assert_eq!(name, "prod");
                assert_eq!(host, "prod.example.com");
                assert_eq!(user, "admin");
                assert!(port.is_none());
            }
            _ => panic!("Expected SshSessionCreate command"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_session_create_with_port() {
        let cli = Cli::parse_from([
            "code-terminal-cli",
            "session-new",
            "prod",
            "prod.example.com",
            "admin",
            "-p",
            "2222",
        ]);
        match cli.command {
            Commands::SshSessionCreate {
                name,
                host,
                user,
                port,
            } => {
                assert_eq!(name, "prod");
                assert_eq!(host, "prod.example.com");
                assert_eq!(user, "admin");
                assert_eq!(port, Some(2222));
            }
            _ => panic!("Expected SshSessionCreate command via session-new alias"),
        }
    }

    #[test]
    fn test_cli_parse_ssh_session_remove() {
        let cli = Cli::parse_from(["code-terminal-cli", "ssh-session-remove", "abc123"]);
        match cli.command {
            Commands::SshSessionRemove { id } => {
                assert_eq!(id, "abc123");
            }
            _ => panic!("Expected SshSessionRemove command"),
        }
    }

    // ------------------------------------------------------------------
    // run(): execução de ponta a ponta contra um terminal falso
    // ------------------------------------------------------------------

    use code_terminal::cli::CliError;
    use serde_json::{json, Value};
    use std::io::{BufRead, BufReader, Write};
    use std::sync::{Arc, Mutex};

    /// Terminal falso: aceita conexões em 127.0.0.1 e responde JSON-RPC,
    /// gravando os métodos chamados.
    fn fake_terminal() -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let calls: Arc<Mutex<Vec<String>>> = Arc::default();
        let c = calls.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let calls = c.clone();
                std::thread::spawn(move || serve_fake_connection(stream, &calls));
            }
        });
        (addr, calls)
    }

    /// Atende uma conexão do terminal falso, uma requisição JSON-RPC por linha.
    fn serve_fake_connection(stream: std::net::TcpStream, calls: &Mutex<Vec<String>>) {
        let mut writer = stream.try_clone().unwrap();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            let req: Value = serde_json::from_str(&line).unwrap();
            let method = req["method"].as_str().unwrap().to_string();
            let result = fake_rpc_result(&method);
            calls.lock().unwrap().push(method);
            let resp = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
            if writeln!(writer, "{}", resp).is_err() {
                break;
            }
        }
    }

    /// Resultado que o terminal falso devolve para cada método.
    fn fake_rpc_result(method: &str) -> Value {
        let tab = json!({"id": 1, "index": 0, "title": "sh", "is_active": true, "pid": null, "cwd": null});
        match method {
            "tab.list" => json!([tab]),
            "tab.create" => tab,
            "pane.list" => json!([{"id": 1, "index": 0, "is_active": true}]),
            "state.get" => json!({
                "version": "0.0.1",
                "tabs": [tab],
                "active_tab": 1,
                "terminal_size": {"cols": 80, "rows": 24}
            }),
            _ => Value::Null,
        }
    }

    fn run_args(args: &[&str]) -> CliResult<()> {
        let mut full = vec!["code-terminal-cli"];
        full.extend_from_slice(args);
        let cli = Cli::parse_from(full);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(run(cli))
    }

    fn free_addr() -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().to_string()
    }

    #[test]
    fn test_run_local_commands_without_server() {
        let dead = free_addr();
        run_args(&["-s", &dead, "version"]).unwrap();
        for shell in ["bash", "zsh", "fish", "power-shell", "elvish"] {
            run_args(&["-s", &dead, "completions", shell]).unwrap();
        }
    }

    #[test]
    fn test_run_config_show_reads_given_file() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("cli.toml");
        std::fs::write(&ok, "socket_path = \"/tmp/x.sock\"\ntimeout_secs = 5\n").unwrap();
        run_args(&["-c", ok.to_str().unwrap(), "config", "show"]).unwrap();

        let bad = dir.path().join("bad.toml");
        std::fs::write(&bad, "timeout_secs = \"nao\"").unwrap();
        assert!(matches!(
            run_args(&["-c", bad.to_str().unwrap(), "config", "show"]),
            Err(CliError::Config(_))
        ));
        // o mesmo erro aparece antes de conectar num comando remoto
        assert!(matches!(
            run_args(&["-c", bad.to_str().unwrap(), "info"]),
            Err(CliError::Config(_))
        ));
    }

    #[test]
    fn test_run_remote_commands_against_fake_terminal() {
        let (addr, calls) = fake_terminal();
        let cmds: &[&[&str]] = &[
            &["list-tabs"],
            &["list-tabs", "-l"],
            &["new-tab", "--title", "t"],
            &["close-tab", "0"],
            &["goto-tab", "0"],
            &["send-keys", "<C-c>"],
            &["input", "ls"],
            &["size"],
            &["size", "-w", "100", "-r", "30"],
            &["info"],
            &["notify", "pronto"],
            &["list-panes"],
            &["split", "vertical"],
            &["ping"],
            &["-q", "ping"],
            &["ssh-connect", "user@host.invalid", "-p", "2200"],
        ];
        for args in cmds {
            let mut full = vec!["-s", addr.as_str(), "-f", "json"];
            full.extend_from_slice(args);
            run_args(&full).unwrap_or_else(|e| panic!("{:?}: {}", args, e));
        }
        let calls = calls.lock().unwrap();
        for m in [
            "tab.list",
            "tab.create",
            "tab.switch",
            "tab.close",
            "keyboard.send",
            "keyboard.text",
            "state.get",
            "set_size",
            "notification.send",
            "pane.list",
            "pane.split",
            "state.api_info",
        ] {
            assert!(calls.iter().any(|c| c == m), "faltou {m}: {calls:?}");
        }
    }

    #[test]
    fn test_run_uses_socket_from_config_file() {
        let (addr, calls) = fake_terminal();
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("cli.toml");
        std::fs::write(&cfg, format!("socket_path = \"{}\"\n", addr)).unwrap();
        run_args(&["-c", cfg.to_str().unwrap(), "ping"]).unwrap();
        assert_eq!(*calls.lock().unwrap(), vec!["state.api_info".to_string()]);
    }

    #[test]
    fn test_run_reports_server_not_running() {
        let dead = free_addr();
        assert!(matches!(
            run_args(&["-s", &dead, "list-tabs"]),
            Err(CliError::ServerNotRunning(a)) if a == dead
        ));
    }

    #[test]
    fn test_run_mux_groups_does_not_need_terminal_server() {
        // Regressão: comandos locais (ssh-*/mux-*) exigiam o servidor JSON-RPC
        // do terminal e falhavam com "Server not running"
        let dead = free_addr();
        run_args(&["-s", &dead, "mux-groups", "list"]).unwrap();
        assert!(matches!(
            run_args(&["-s", &dead, "mux-groups", "explodir"]),
            Err(CliError::InvalidInput(_))
        ));
    }

    #[test]
    fn test_cli_definition_has_no_conflicting_options() {
        // Regressão: `mux-create -c` (cols) e `mux-session-list -s` (sort)
        // colidiam com as opções globais `-c/--config` e `-s/--socket`; o clap
        // entrava em pânico ao montar o comando (completions) em debug
        Cli::command().debug_assert();
    }

    /// #32: todo texto da ajuda tem tradução; a ajuda em português monta
    /// sem conflito de argumentos.
    #[test]
    fn every_help_text_has_a_translation() {
        let missing = code_terminal::cli::help_pt::untranslated(&Cli::command());
        assert!(
            missing.is_empty(),
            "sem tradução em help_pt.rs: {missing:#?}"
        );
        code_terminal::cli::help_pt::localize(
            Cli::command(),
            code_terminal::ui::i18n::Language::PortugueseBr,
        )
        .debug_assert();
        let cli = Cli::parse_from(["code-terminal-cli", "mux-create", "-w", "100", "-r", "30"]);
        assert!(matches!(
            cli.command,
            Commands::MuxCreate {
                cols: 100,
                rows: 30,
                ..
            }
        ));
        let cli = Cli::parse_from(["code-terminal-cli", "-s", "/tmp/s", "sl", "-o", "name"]);
        assert_eq!(cli.socket.as_deref(), Some("/tmp/s"));
        assert!(matches!(cli.command, Commands::MuxSessionList { sort: Some(s) } if s == "name"));
    }

    #[test]
    fn test_setup_logging_accepts_every_level() {
        for v in 0..4 {
            setup_logging(v);
        }
    }
}
