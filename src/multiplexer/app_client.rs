//! Lado do app das tabs persistentes: sobe o daemon se preciso, abre o fluxo
//! de uma sessão e troca os dados da tab (título, grupo, posição) com ele.

use crate::multiplexer::protocol::{Message, MessageType, SessionInfo, StreamCreate};
use crate::multiplexer::transport::default_mux_socket_path;
use std::io::{self, BufRead, BufReader, Write};
use std::os::fd::RawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Unidade systemd do daemon (`packaging/linux/code-terminal-mux.service`).
pub const MUX_UNIT: &str = "code-terminal-mux.service";

/// Com esta variável o app não usa o systemd para subir o daemon (testes).
pub const NO_SYSTEMD_ENV: &str = "CODE_TERMINAL_MUX_NO_SYSTEMD";

/// Fluxo aberto: a conexão (para escrever) e o leitor já posicionado depois
/// da resposta.
pub struct OpenedStream {
    pub writer: UnixStream,
    pub reader: BufReader<UnixStream>,
    pub name: String,
    pub pid: Option<u32>,
    /// A sessão foi criada agora (não existia).
    pub created: bool,
}

impl std::fmt::Debug for OpenedStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedStream")
            .field("name", &self.name)
            .field("pid", &self.pid)
            .field("created", &self.created)
            .finish_non_exhaustive()
    }
}

/// Socket do daemon.
pub fn daemon_socket() -> PathBuf {
    default_mux_socket_path()
}

fn connect() -> io::Result<UnixStream> {
    UnixStream::connect(daemon_socket())
}

/// O daemon está de pé?
pub fn daemon_running() -> bool {
    connect().is_ok()
}

/// Garante um daemon atendendo: o serviço do systemd, senão uma unidade
/// temporária (`systemd-run`), senão um processo solto. Nos dois primeiros
/// casos o daemon fica fora do cgroup do app, e reiniciar o
/// `code-terminal-server` não leva os shells junto.
pub fn ensure_daemon() -> io::Result<()> {
    if daemon_running() {
        return Ok(());
    }
    let use_systemd = std::env::var_os(NO_SYSTEMD_ENV).is_none();
    if use_systemd && run_quiet("systemctl", &["--user", "start", MUX_UNIT]) && wait_for_daemon() {
        return Ok(());
    }
    let cli = cli_path();
    let cli_str = cli.display().to_string();
    if use_systemd
        && run_quiet(
            "systemd-run",
            &[
                "--user",
                "--quiet",
                "--collect",
                "--unit",
                "code-terminal-mux-auto",
                // SIGTERM só para o daemon: ele grava as abas antes de os
                // shells caírem (como no code-terminal-mux.service).
                "-p",
                "KillMode=mixed",
                &cli_str,
                "mux-server",
            ],
        )
        && wait_for_daemon()
    {
        return Ok(());
    }
    use std::os::unix::process::CommandExt;
    std::process::Command::new(&cli)
        .arg("mux-server")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    if wait_for_daemon() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the multiplexer daemon did not start",
        ))
    }
}

fn run_quiet(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn wait_for_daemon() -> bool {
    // Folgado: numa máquina carregada (CI, build rodando) o daemon pode
    // levar alguns segundos para abrir o socket.
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if daemon_running() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// `code-terminal-cli` ao lado do executável atual, senão o do PATH.
pub fn cli_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("code-terminal-cli")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("code-terminal-cli"))
}

fn line_of(msg: MessageType) -> io::Result<Vec<u8>> {
    let mut line = Message::new(1, msg)
        .encode()
        .map_err(io::Error::other)?
        .into_bytes();
    line.push(b'\n');
    Ok(line)
}

fn read_reply(reader: &mut impl BufRead) -> io::Result<MessageType> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the daemon closed the connection",
        ));
    }
    let msg = Message::decode(line.trim_end()).map_err(io::Error::other)?;
    match msg.msg_type {
        MessageType::Error { message, .. } => Err(io::Error::other(message)),
        other => Ok(other),
    }
}

/// Pedido simples: uma linha, uma resposta.
fn request(msg: MessageType) -> io::Result<MessageType> {
    let mut stream = connect()?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(&line_of(msg)?)?;
    read_reply(&mut BufReader::new(stream))
}

/// Abre o fluxo da sessão `session`, criando-a com `create` se não existir.
/// Com `adopt`, manda junto o descritor do PTY (tab que já roda) e `seed`
/// (a tela dela) logo depois da linha. `replay` pede o histórico da sessão.
pub fn open_stream(
    session: &str,
    size: (u16, u16),
    create: Option<StreamCreate>,
    replay: bool,
    adopt: Option<(RawFd, &[u8])>,
) -> io::Result<OpenedStream> {
    let writer = connect()?;
    let mut create = create;
    if let (Some(spec), Some((_, seed))) = (create.as_mut(), adopt) {
        spec.seed_len = seed.len();
    }
    let line = line_of(MessageType::OpenStream {
        session: session.to_string(),
        cols: size.0,
        rows: size.1,
        create,
        replay,
    })?;
    match adopt {
        Some((fd, seed)) => {
            crate::multiplexer::stream::send_with_fd(&writer, &line, fd)?;
            (&writer).write_all(seed)?;
        }
        None => (&writer).write_all(&line)?,
    }
    let read_half = writer.try_clone()?;
    read_half.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(read_half);
    let reply = read_reply(&mut reader)?;
    reader.get_ref().set_read_timeout(None)?;
    match reply {
        MessageType::StreamOpened {
            name, pid, created, ..
        } => Ok(OpenedStream {
            writer,
            reader,
            name,
            pid,
            created,
        }),
        other => Err(io::Error::other(format!("unexpected reply {other:?}"))),
    }
}

/// Sessões das tabs persistentes que o daemon tem (vazio sem daemon).
pub fn list_app_sessions() -> Vec<SessionInfo> {
    if !daemon_running() {
        return Vec::new();
    }
    match request(MessageType::ListSessions) {
        Ok(MessageType::SessionList { sessions }) => {
            sessions.into_iter().filter(|s| s.app.is_some()).collect()
        }
        _ => Vec::new(),
    }
}

/// Marcas de uma tab persistente gravadas no daemon.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabFlags {
    /// "Só esconder": não volta sozinha ao abrir a janela.
    pub hidden: bool,
    /// Deixou de ser persistente: morre quando a janela desanexa.
    pub kill_on_detach: bool,
    /// Retomar o programa sozinho depois de reiniciar a máquina.
    pub auto_resume: bool,
}

/// Grava os dados da tab e as marcas da sessão no daemon.
pub fn set_meta(session: &str, meta: &str, flags: TabFlags) -> io::Result<()> {
    request(MessageType::SetSessionMeta {
        session_id: session.to_string(),
        meta: meta.to_string(),
        hidden: flags.hidden,
        kill_on_detach: flags.kill_on_detach,
        auto_resume: flags.auto_resume,
    })
    .map(|_| ())
}

/// Sobe o daemon se há abas gravadas de antes do reboot (para a janela
/// reabri-las). Sem nada gravado, não sobe nada.
pub fn ensure_daemon_for_saved_tabs() {
    if !daemon_running() && crate::multiplexer::tab_store::has_saved_tabs() {
        if let Err(e) = ensure_daemon() {
            tracing::warn!(error = %e, "Could not start the daemon for saved tabs");
        }
    }
}

/// Hook de agente: conta ao daemon (se houver) a conversa do processo
/// `pid`. Devolve a tab persistente onde ele roda.
pub fn report_agent(
    pid: u32,
    kind: &str,
    session_id: Option<&str>,
    cwd: Option<&str>,
    active: bool,
) -> Option<String> {
    if !daemon_running() {
        return None;
    }
    match request(MessageType::ReportAgent {
        pid,
        kind: kind.to_string(),
        session_id: session_id.map(str::to_string),
        cwd: cwd.map(str::to_string),
        active,
    }) {
        Ok(MessageType::AgentReported { session }) => session,
        _ => None,
    }
}
