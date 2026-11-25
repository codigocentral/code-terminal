//! Attach interativo: liga o terminal atual a uma sessão do daemon.
//!
//! O terminal fica em modo raw; o que se digita vai à sessão e a saída dela
//! vem para a tela. `Ctrl+B d` desanexa (o shell continua vivo no daemon);
//! `Ctrl+B Ctrl+B` manda um `Ctrl+B` de verdade. Mudanças de tamanho do
//! terminal são repassadas.

use crate::multiplexer::protocol::{ErrorCode, Message, MessageType};
use crate::multiplexer::transport::MuxConnection;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::mpsc;
use std::time::Duration;

/// Tecla de prefixo padrão (Ctrl+B, como no tmux).
pub const DEFAULT_PREFIX: u8 = 0x02;

/// Separa o prefixo de desanexar do que vai para a sessão.
#[derive(Debug, Clone)]
pub struct PrefixFilter {
    prefix: u8,
    pending: bool,
}

impl PrefixFilter {
    pub fn new(prefix: u8) -> Self {
        Self {
            prefix,
            pending: false,
        }
    }

    /// Bytes a mandar e se o usuário pediu para desanexar.
    pub fn feed(&mut self, data: &[u8]) -> (Vec<u8>, bool) {
        let mut out = Vec::with_capacity(data.len());
        for &byte in data {
            if self.pending {
                self.pending = false;
                match byte {
                    b'd' | b'D' => return (out, true),
                    b if b == self.prefix => out.push(self.prefix),
                    other => {
                        out.push(self.prefix);
                        out.push(other);
                    }
                }
            } else if byte == self.prefix {
                self.pending = true;
            } else {
                out.push(byte);
            }
        }
        (out, false)
    }
}

/// Como o attach terminou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachEnd {
    /// O usuário desanexou; a sessão continua.
    Detached,
    /// O processo da sessão saiu (ou a sessão foi destruída).
    SessionEnded,
    /// O daemon fechou a conexão.
    ConnectionLost,
    /// A entrada acabou (EOF).
    InputClosed,
}

/// O que anexar.
#[derive(Debug, Clone)]
pub struct AttachOptions {
    pub session: String,
    pub read_only: bool,
    /// Cria a sessão com esse nome se ela não existir.
    pub create: bool,
    pub prefix: u8,
}

enum Event {
    Input(Vec<u8>),
    InputClosed,
    Server(MessageType),
    ServerClosed,
}

/// Tamanho atual do terminal (colunas, linhas).
pub trait TerminalSize: Send {
    fn size(&self) -> Option<(u16, u16)>;
}

/// Tamanho do terminal ligado ao stdout.
pub struct StdoutSize;

impl TerminalSize for StdoutSize {
    fn size(&self) -> Option<(u16, u16)> {
        stdout_size()
    }
}

fn send(writer: &mut MuxConnection, id: u64, msg_type: MessageType) -> io::Result<()> {
    let mut line = Message::new(id, msg_type)
        .encode()
        .map_err(io::Error::other)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

/// Anexa e fica repassando até desanexar ou a sessão acabar.
///
/// `input` e `output` são o terminal (stdin/stdout no CLI); os testes passam
/// pipes. O modo raw é responsabilidade de quem chama.
pub fn run_attach(
    conn: MuxConnection,
    opts: &AttachOptions,
    input: impl Read + Send + 'static,
    output: &mut dyn Write,
    size: &dyn TerminalSize,
) -> io::Result<(String, AttachEnd)> {
    let mut writer = conn.try_clone()?;
    let (tx, rx) = mpsc::channel();
    spawn_server_reader(conn, tx.clone())?;
    spawn_input_reader(input, tx)?;

    let initial = size.size().unwrap_or((80, 24));
    if opts.create {
        send(
            &mut writer,
            1,
            MessageType::CreateSession {
                name: Some(opts.session.clone()),
                size: initial,
            },
        )?;
    }
    send(
        &mut writer,
        2,
        MessageType::Attach {
            session_id: opts.session.clone(),
            read_only: opts.read_only,
        },
    )?;
    let name = wait_attached(&rx, opts.create, output)?;
    let mut last_size = Some(initial);
    send(
        &mut writer,
        3,
        MessageType::Resize {
            cols: initial.0,
            rows: initial.1,
        },
    )?;

    let mut filter = PrefixFilter::new(opts.prefix);
    let end = loop {
        let step = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Input(data)) => {
                forward_input(&mut writer, &mut filter, &data, opts.read_only)?
            }
            Ok(Event::InputClosed) => Some(AttachEnd::InputClosed),
            Ok(Event::Server(msg)) => show(msg, output)?,
            Ok(Event::ServerClosed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                Some(AttachEnd::ConnectionLost)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => None,
        };
        if let Some(end) = step {
            break end;
        }
        sync_size(&mut writer, size, &mut last_size)?;
    };
    writer.shutdown();
    Ok((name, end))
}

/// Manda o que foi digitado (menos o prefixo); `Some` se o usuário desanexou.
fn forward_input(
    writer: &mut MuxConnection,
    filter: &mut PrefixFilter,
    data: &[u8],
    read_only: bool,
) -> io::Result<Option<AttachEnd>> {
    let (bytes, detach) = filter.feed(data);
    if !bytes.is_empty() && !read_only {
        send(writer, 0, MessageType::Input { data: bytes })?;
    }
    if !detach {
        return Ok(None);
    }
    let _ = send(writer, 0, MessageType::Detach);
    Ok(Some(AttachEnd::Detached))
}

/// Repassa ao daemon uma mudança de tamanho do terminal.
fn sync_size(
    writer: &mut MuxConnection,
    size: &dyn TerminalSize,
    last: &mut Option<(u16, u16)>,
) -> io::Result<()> {
    let current = size.size();
    let Some((cols, rows)) = current.filter(|_| current != *last) else {
        return Ok(());
    };
    *last = current;
    send(writer, 0, MessageType::Resize { cols, rows })
}

/// Espera a resposta do attach, mostrando a tela que vier junto.
fn wait_attached(
    rx: &mpsc::Receiver<Event>,
    create: bool,
    output: &mut dyn Write,
) -> io::Result<String> {
    loop {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Server(MessageType::Attached { name, .. })) => return Ok(name),
            // Com --create, "já existe" só quer dizer que vamos anexar.
            Ok(Event::Server(MessageType::Error {
                code: ErrorCode::SessionAlreadyExists,
                ..
            })) if create => {}
            Ok(Event::Server(MessageType::SessionCreated { .. })) => {}
            Ok(Event::Server(MessageType::Error { message, .. })) => {
                return Err(io::Error::new(io::ErrorKind::NotFound, message))
            }
            Ok(Event::Server(MessageType::Output { data })) => output.write_all(&data)?,
            Ok(Event::Server(_)) | Ok(Event::Input(_)) => {}
            Ok(Event::InputClosed) => {}
            Ok(Event::ServerClosed) | Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "mux daemon closed the connection",
                ))
            }
        }
    }
}

/// Mostra uma mensagem do daemon; `Some` quando o attach acabou.
fn show(msg: MessageType, output: &mut dyn Write) -> io::Result<Option<AttachEnd>> {
    match msg {
        MessageType::Output { data } => {
            output.write_all(&data)?;
            output.flush()?;
            Ok(None)
        }
        MessageType::SessionDestroyedNotification { .. } => Ok(Some(AttachEnd::SessionEnded)),
        MessageType::Detached { .. } => Ok(Some(AttachEnd::Detached)),
        _ => Ok(None),
    }
}

fn spawn_server_reader(conn: MuxConnection, tx: mpsc::Sender<Event>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("mux-attach-server".into())
        .spawn(move || {
            for line in BufReader::new(conn).lines() {
                let Ok(line) = line else { break };
                if let Ok(msg) = Message::decode(&line) {
                    if tx.send(Event::Server(msg.msg_type)).is_err() {
                        return;
                    }
                }
            }
            let _ = tx.send(Event::ServerClosed);
        })?;
    Ok(())
}

fn spawn_input_reader(
    mut input: impl Read + Send + 'static,
    tx: mpsc::Sender<Event>,
) -> io::Result<()> {
    std::thread::Builder::new()
        .name("mux-attach-input".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match input.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Event::InputClosed);
                        return;
                    }
                    Ok(n) => {
                        if tx.send(Event::Input(buf[..n].to_vec())).is_err() {
                            return;
                        }
                    }
                }
            }
        })?;
    Ok(())
}

/// Sequência que devolve o terminal do cliente a um estado normal depois de
/// desanexar (o programa da sessão pode ter ligado tela alternativa, mouse...).
pub const RESET_CLIENT_MODES: &str =
    "\x1b[?1049l\x1b[0m\x1b[?25h\x1b[?1l\x1b>\x1b[?2004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?1004l";

/// Tamanho do terminal no stdout.
#[cfg(unix)]
pub fn stdout_size() -> Option<(u16, u16)> {
    // SAFETY: TIOCGWINSZ só escreve na struct winsize passada, que vive
    // durante a chamada; o fd 1 pode não ser um terminal (aí volta erro).
    let mut ws: nix::libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { nix::libc::ioctl(1, nix::libc::TIOCGWINSZ, &mut ws) } == 0;
    (ok && ws.ws_col > 0 && ws.ws_row > 0).then_some((ws.ws_col, ws.ws_row))
}

#[cfg(not(unix))]
pub fn stdout_size() -> Option<(u16, u16)> {
    None
}

/// Deixa o stdin em modo raw enquanto existir (Unix).
#[cfg(unix)]
pub struct RawMode {
    original: nix::sys::termios::Termios,
}

#[cfg(unix)]
impl RawMode {
    /// `None` quando o stdin não é um terminal.
    pub fn enable() -> Option<Self> {
        use nix::sys::termios::{cfmakeraw, tcgetattr, tcsetattr, SetArg};
        let stdin = nix::libc::STDIN_FILENO;
        let original = tcgetattr(stdin).ok()?;
        let mut raw = original.clone();
        cfmakeraw(&mut raw);
        tcsetattr(stdin, SetArg::TCSANOW, &raw).ok()?;
        Some(Self { original })
    }
}

#[cfg(unix)]
impl Drop for RawMode {
    fn drop(&mut self) {
        use nix::sys::termios::{tcsetattr, SetArg};
        let _ = tcsetattr(nix::libc::STDIN_FILENO, SetArg::TCSANOW, &self.original);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::multiplexer::daemon::MuxDaemon;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_prefix_filter() {
        let mut f = PrefixFilter::new(DEFAULT_PREFIX);
        assert_eq!(f.feed(b"ls\r"), (b"ls\r".to_vec(), false));
        assert_eq!(f.feed(b"a\x02\x02b"), (b"a\x02b".to_vec(), false));
        assert_eq!(f.feed(b"\x02x"), (b"\x02x".to_vec(), false));
        // Prefixo no fim de uma leitura e `d` na próxima.
        assert_eq!(f.feed(b"ok\x02"), (b"ok".to_vec(), false));
        assert_eq!(f.feed(b"d"), (Vec::new(), true));
    }

    /// Saída escrita por `run_attach`, lida noutra thread.
    #[derive(Clone, Default)]
    struct Screen(Arc<Mutex<Vec<u8>>>);

    impl Write for Screen {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Screen {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
        }
    }

    struct Fixed;
    impl TerminalSize for Fixed {
        fn size(&self) -> Option<(u16, u16)> {
            Some((70, 12))
        }
    }

    /// Entrada que solta os bytes aos poucos, com pausas (como alguém digitando).
    struct Typing(std::vec::IntoIter<(u64, Vec<u8>)>);
    impl Read for Typing {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.next() {
                Some((pause, bytes)) => {
                    std::thread::sleep(Duration::from_millis(pause));
                    buf[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                None => {
                    // Fica "aberta" para o attach terminar por outro motivo.
                    std::thread::sleep(Duration::from_secs(30));
                    Ok(0)
                }
            }
        }
    }

    /// Fluxo do usuário: cria e anexa, roda um comando, desanexa com
    /// Ctrl+B d; anexa de novo e vê o resultado na tela; `exit` encerra.
    #[test]
    fn test_attach_detach_reattach_and_exit() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("mux.sock");
        let daemon = Arc::new(MuxDaemon::new(socket.clone(), Config::default()));
        let listener = daemon.bind().unwrap();
        let runner = Arc::clone(&daemon);
        let thread = std::thread::spawn(move || runner.run(listener).unwrap());

        let opts = AttachOptions {
            session: "trabalho".into(),
            read_only: false,
            create: true,
            prefix: DEFAULT_PREFIX,
        };
        let screen = Screen::default();
        let typing = Typing(
            vec![
                (300, b"echo valor-$((6*7))\r".to_vec()),
                (700, b"\x02".to_vec()),
                (10, b"d".to_vec()),
            ]
            .into_iter(),
        );
        let conn = MuxConnection::connect_unix(&socket).unwrap();
        let (name, end) = run_attach(conn, &opts, typing, &mut screen.clone(), &Fixed).unwrap();
        assert_eq!(name, "trabalho");
        assert_eq!(end, AttachEnd::Detached);
        assert!(screen.text().contains("valor-42"), "{}", screen.text());

        // Reanexa sem --create: a tela volta com o resultado anterior.
        let again = Screen::default();
        let typing = Typing(vec![(500, b"exit\r".to_vec())].into_iter());
        let opts = AttachOptions {
            create: false,
            ..opts
        };
        let conn = MuxConnection::connect_unix(&socket).unwrap();
        let (_, end) = run_attach(conn, &opts, typing, &mut again.clone(), &Fixed).unwrap();
        assert_eq!(end, AttachEnd::SessionEnded);
        assert!(again.text().contains("valor-42"), "{}", again.text());

        // Última sessão terminou: o daemon sai.
        thread.join().unwrap();

        // Sessão inexistente sem --create é erro.
        let daemon = Arc::new(MuxDaemon::new(socket.clone(), Config::default()));
        let listener = daemon.bind().unwrap();
        let runner = Arc::clone(&daemon);
        let thread = std::thread::spawn(move || runner.run(listener).unwrap());
        let conn = MuxConnection::connect_unix(&socket).unwrap();
        let typing = Typing(Vec::new().into_iter());
        let err = run_attach(conn, &opts, typing, &mut Screen::default(), &Fixed).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        daemon.stop();
        thread.join().unwrap();
    }
}
