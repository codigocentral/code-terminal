//! Daemon do multiplexer: mantém shells vivos sem janela.
//!
//! Cada sessão é um PTY com o seu grid (o mesmo [`TerminalBuffer`] das
//! tabs). Clientes anexam pelo socket, recebem a tela atual (redesenhada a
//! partir do grid) e depois a saída nova; o que digitam vai ao PTY. Fechar o
//! cliente (ou a janela onde ele roda) não mata o shell: basta anexar de novo.
//! O daemon sai sozinho quando a última sessão termina.
//!
//! Protocolo: uma [`Message`] JSON por linha, como o resto do multiplexer.

use crate::config::Config;
use crate::multiplexer::app_session::{serve_stream, AppSession, SaveSignal, SharedAppSession};
use crate::multiplexer::protocol::{
    ErrorCode, Message, MessageType, SessionFlagsInfo, SessionInfo, StreamCreate,
};
use crate::multiplexer::screen::screen_to_ansi;
use crate::multiplexer::stream::recv_first_line;
use crate::multiplexer::tab_store::{AgentInfo, TabRecord, TabStore};
use crate::multiplexer::transport::MuxConnection;
use crate::terminal::buffer::TerminalBuffer;
use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixListener;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Intervalo entre leituras dos PTYs.
const PUMP_INTERVAL: Duration = Duration::from_millis(8);
/// Cliente que não aceita escrita nesse tempo é desanexado.
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Daemon sem serviço que nunca recebeu sessão sai depois disto (quem o
/// subiu desistiu antes de ele abrir o socket).
const IDLE_EXIT: Duration = Duration::from_secs(60);
/// Intervalo entre gravações das abas persistentes em disco (só roda quando
/// alguma mudou).
const SAVE_INTERVAL: Duration = Duration::from_secs(30);
/// Variável que diz ao shell em que sessão ele roda.
pub const SESSION_ENV: &str = "CODE_TERMINAL_MUX_SESSION";

type Writer = Arc<Mutex<MuxConnection>>;

struct Attached {
    client: u64,
    writer: Writer,
    read_only: bool,
}

struct DaemonSession {
    id: String,
    name: String,
    buffer: TerminalBuffer,
    cwd: PathBuf,
    created: SystemTime,
    last_activity: Instant,
    clients: Vec<Attached>,
}

impl DaemonSession {
    fn info(&self) -> SessionInfo {
        let created = self
            .created
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        let grid = self.buffer.grid();
        SessionInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            state: if self.clients.is_empty() {
                "detached".into()
            } else {
                "attached".into()
            },
            clients: self.clients.len(),
            size: (
                u16::try_from(grid.cols()).unwrap_or(u16::MAX),
                u16::try_from(grid.rows()).unwrap_or(u16::MAX),
            ),
            working_dir: self.cwd.display().to_string(),
            created_at: created,
            idle_seconds: self.last_activity.elapsed().as_secs(),
            flags: SessionFlagsInfo {
                destroy_on_exit: true,
                attachable: true,
                shared: self.clients.len() > 1,
                locked: false,
            },
            app: None,
        }
    }

    /// Com cliente que digita, o terminal dele responde às consultas (DSR,
    /// DA); o grid do daemon fica quieto para não responder em dobro.
    fn update_reply_mode(&mut self) {
        let writable = self.clients.iter().any(|c| !c.read_only);
        self.buffer.set_mute_replies(writable);
    }

    /// Manda a todos os clientes; quem falhar é desanexado.
    fn broadcast(&mut self, msg: &Message) {
        let before = self.clients.len();
        self.clients.retain(|c| send(&c.writer, msg));
        if self.clients.len() != before {
            self.update_reply_mode();
        }
    }
}

/// Estado compartilhado entre as conexões e a thread que lê os PTYs.
struct DaemonState {
    sessions: Vec<DaemonSession>,
    /// Sessões das tabs persistentes do app (só PTY e registro da saída).
    apps: Vec<SharedAppSession>,
    /// Serviço do systemd: não sai quando a última sessão termina.
    keep_alive: bool,
    started: Instant,
    /// Abas gravadas em disco (`None`: gravação desligada).
    store: Option<TabStore>,
    /// Abas de antes do reboot, esperando uma janela reabri-las.
    dormant: Vec<TabRecord>,
    saver: Arc<SaveSignal>,
    /// Desligando: as abas que caírem agora não são apagadas do disco.
    shutting_down: Arc<AtomicBool>,
    /// Acorda a thread de varredura (que dorme sem sessões de grid).
    wake: Arc<parking_lot::Condvar>,
    /// Quantas vezes a varredura acordou (testes: daemon parado não acorda).
    pump_wakeups: u64,
    had_sessions: bool,
    next_client: u64,
    next_name: u64,
    config: Config,
    home: PathBuf,
}

/// Onde o cliente está anexado.
#[derive(Default)]
struct ClientState {
    session: Option<String>,
    read_only: bool,
}

impl DaemonState {
    /// Sessão de tab persistente pelo nome ou id.
    fn find_app(&self, key: &str) -> Option<SharedAppSession> {
        self.apps
            .iter()
            .find(|app| {
                let app = app.lock();
                !app.ended() && (app.name == key || app.id == key)
            })
            .cloned()
    }

    fn name_taken(&self, name: &str) -> bool {
        self.find(name).is_some() || self.find_app(name).is_some()
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.sessions
            .iter()
            .position(|s| s.id == key || s.name == key)
            .or_else(|| {
                // Prefixo do id (o `mux-list` mostra 8 caracteres).
                let matches: Vec<usize> = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| key.len() >= 4 && s.id.starts_with(key))
                    .map(|(i, _)| i)
                    .collect();
                (matches.len() == 1).then(|| matches[0])
            })
    }

    fn create(
        &mut self,
        name: Option<String>,
        size: (u16, u16),
    ) -> Result<(String, String), Message> {
        let name = match name.filter(|n| !n.trim().is_empty()) {
            Some(name) => name,
            None => loop {
                self.next_name += 1;
                let candidate = self.next_name.to_string();
                if !self.name_taken(&candidate) {
                    break candidate;
                }
            },
        };
        if self.name_taken(&name) {
            return Err(Message::error(
                0,
                ErrorCode::SessionAlreadyExists,
                format!("session '{name}' already exists"),
            ));
        }
        let (cols, rows) = (size.0.clamp(2, 1000), size.1.clamp(2, 1000));
        let env = vec![(SESSION_ENV.to_string(), name.clone())];
        let buffer =
            TerminalBuffer::for_pane(cols, rows, &self.config, None, Some(&self.home), &env)
                .map_err(|e| Message::error(0, ErrorCode::ServerError, e.to_string()))?;
        let id = uuid::Uuid::new_v4().to_string();
        self.sessions.push(DaemonSession {
            id: id.clone(),
            name: name.clone(),
            buffer,
            cwd: self.home.clone(),
            created: SystemTime::now(),
            last_activity: Instant::now(),
            clients: Vec::new(),
        });
        self.had_sessions = true;
        self.wake.notify_all();
        tracing::info!(session = %name, "Mux session created");
        Ok((id, name))
    }

    fn detach(&mut self, client: u64) -> Option<String> {
        for session in &mut self.sessions {
            let before = session.clients.len();
            session.clients.retain(|c| c.client != client);
            if session.clients.len() != before {
                session.update_reply_mode();
                return Some(session.id.clone());
            }
        }
        None
    }

    /// Trata uma mensagem; devolve as respostas na ordem.
    fn handle(
        &mut self,
        client: u64,
        writer: &Writer,
        me: &mut ClientState,
        msg: Message,
    ) -> Vec<Message> {
        let id = msg.id;
        let reply = |msg_type| vec![Message::new(id, msg_type)];
        let error = |code, text: String| vec![Message::error(id, code, text)];
        match msg.msg_type {
            MessageType::Ping => reply(MessageType::Pong),
            MessageType::CreateSession { name, size } => match self.create(name, size) {
                Ok((session_id, name)) => reply(MessageType::SessionCreated { session_id, name }),
                Err(mut err) => {
                    err.id = id;
                    vec![err]
                }
            },
            MessageType::ListSessions => reply(MessageType::SessionList {
                sessions: self
                    .sessions
                    .iter()
                    .map(DaemonSession::info)
                    .chain(self.apps.iter().map(|app| app.lock().info()))
                    .chain(self.dormant.iter().map(dormant_info))
                    .collect(),
            }),
            MessageType::ReportAgent {
                pid,
                kind,
                session_id,
                cwd,
                active,
            } => {
                let agent = session_id
                    .filter(|id| active && !id.is_empty())
                    .map(|session_id| AgentInfo {
                        kind,
                        session_id,
                        cwd,
                    });
                reply(MessageType::AgentReported {
                    session: self.report_agent(pid, agent),
                })
            }
            MessageType::SetSessionMeta {
                session_id,
                meta,
                hidden,
                kill_on_detach,
                auto_resume,
            } => match self.find_app(&session_id) {
                Some(app) => {
                    let mut app = app.lock();
                    app.meta = meta;
                    app.hidden = hidden;
                    app.kill_on_detach = kill_on_detach;
                    app.auto_resume = auto_resume;
                    app.mark_dirty();
                    reply(MessageType::Pong)
                }
                None => error(
                    ErrorCode::SessionNotFound,
                    format!("no persistent tab '{session_id}'"),
                ),
            },
            MessageType::Attach {
                session_id,
                read_only,
            } => self.attach(id, client, writer, me, &session_id, read_only),
            MessageType::Detach => match self.detach(client) {
                Some(session_id) => {
                    me.session = None;
                    reply(MessageType::Detached { session_id })
                }
                None => error(ErrorCode::NotAttached, "not attached".into()),
            },
            MessageType::Input { data } => self.input(id, me, &data),
            MessageType::Resize { cols, rows } => self.resize(id, me, cols, rows),
            MessageType::RenameSession {
                session_id,
                new_name,
            } => self.rename(id, &session_id, new_name),
            MessageType::DestroySession { session_id } => self.destroy(id, &session_id),
            other => error(
                ErrorCode::InvalidMessage,
                format!("unsupported message: {other:?}"),
            ),
        }
    }

    fn attach(
        &mut self,
        id: u64,
        client: u64,
        writer: &Writer,
        me: &mut ClientState,
        key: &str,
        read_only: bool,
    ) -> Vec<Message> {
        self.detach(client);
        let Some(index) = self.find(key) else {
            return vec![Message::error(
                id,
                ErrorCode::SessionNotFound,
                format!("no session '{key}'"),
            )];
        };
        let session = &mut self.sessions[index];
        session.clients.push(Attached {
            client,
            writer: Arc::clone(writer),
            read_only,
        });
        session.update_reply_mode();
        me.session = Some(session.id.clone());
        me.read_only = read_only;
        vec![
            Message::new(
                id,
                MessageType::Attached {
                    session_id: session.id.clone(),
                    name: session.name.clone(),
                },
            ),
            Message::new(
                0,
                MessageType::Output {
                    data: screen_to_ansi(session.buffer.grid()),
                },
            ),
        ]
    }

    fn attached_session(
        &mut self,
        me: &ClientState,
    ) -> Result<&mut DaemonSession, (ErrorCode, String)> {
        let Some(key) = me.session.as_deref() else {
            return Err((ErrorCode::NotAttached, "not attached".into()));
        };
        self.sessions
            .iter_mut()
            .find(|s| s.id == key)
            .ok_or((ErrorCode::SessionNotFound, "session ended".into()))
    }

    fn input(&mut self, id: u64, me: &ClientState, data: &[u8]) -> Vec<Message> {
        if me.read_only {
            return vec![Message::error(id, ErrorCode::PermissionDenied, "read-only")];
        }
        match self.attached_session(me) {
            Ok(session) => {
                session.last_activity = Instant::now();
                match session.buffer.write(data) {
                    Ok(()) => vec![Message::new(id, MessageType::Pong)],
                    Err(e) => vec![Message::error(id, ErrorCode::ServerError, e.to_string())],
                }
            }
            Err((code, text)) => vec![Message::error(id, code, text)],
        }
    }

    fn resize(&mut self, id: u64, me: &ClientState, cols: u16, rows: u16) -> Vec<Message> {
        let session = match self.attached_session(me) {
            Ok(session) => session,
            Err((code, text)) => return vec![Message::error(id, code, text)],
        };
        if me.read_only {
            return vec![Message::new(id, MessageType::Pong)];
        }
        let (cols, rows) = (cols.clamp(2, 1000), rows.clamp(2, 1000));
        if let Err(e) = session.buffer.resize(cols, rows) {
            return vec![Message::error(id, ErrorCode::ServerError, e.to_string())];
        }
        vec![Message::new(id, MessageType::Pong)]
    }

    fn rename(&mut self, id: u64, key: &str, new_name: String) -> Vec<Message> {
        if new_name.trim().is_empty() || self.name_taken(&new_name) {
            return vec![Message::error(
                id,
                ErrorCode::SessionAlreadyExists,
                format!("name '{new_name}' is empty or taken"),
            )];
        }
        if let Some(app) = self.find_app(key) {
            let mut app = app.lock();
            if let Some(store) = &self.store {
                store.remove(&app.name);
            }
            app.name = new_name.clone();
            app.mark_dirty();
            return vec![Message::new(
                id,
                MessageType::SessionRenamed {
                    session_id: app.id.clone(),
                    new_name,
                },
            )];
        }
        let Some(index) = self.find(key) else {
            return vec![Message::error(
                id,
                ErrorCode::SessionNotFound,
                format!("no session '{key}'"),
            )];
        };
        let session = &mut self.sessions[index];
        session.name = new_name.clone();
        vec![Message::new(
            id,
            MessageType::SessionRenamed {
                session_id: session.id.clone(),
                new_name,
            },
        )]
    }

    fn destroy(&mut self, id: u64, key: &str) -> Vec<Message> {
        if let Some(index) = self.dormant.iter().position(|r| r.name == key) {
            let record = self.dormant.remove(index);
            if let Some(store) = &self.store {
                store.remove(&record.name);
            }
            return vec![Message::new(
                id,
                MessageType::SessionDestroyed {
                    session_id: record.name,
                },
            )];
        }
        if let Some(app) = self.find_app(key) {
            let mut app = app.lock();
            app.kill();
            return vec![Message::new(
                id,
                MessageType::SessionDestroyed {
                    session_id: app.id.clone(),
                },
            )];
        }
        let Some(index) = self.find(key) else {
            return vec![Message::error(
                id,
                ErrorCode::SessionNotFound,
                format!("no session '{key}'"),
            )];
        };
        let mut session = self.sessions.remove(index);
        let session_id = session.id.clone();
        session.broadcast(&Message::new(
            0,
            MessageType::SessionDestroyedNotification {
                session_id: session_id.clone(),
            },
        ));
        vec![Message::new(
            id,
            MessageType::SessionDestroyed { session_id },
        )]
    }

    /// Lê os PTYs, repassa a saída e remove sessões cujo processo saiu.
    fn pump(&mut self) {
        // As tabs persistentes têm thread própria; aqui só sai quem acabou.
        self.apps.retain(|app| !app.lock().ended());
        self.sessions.retain_mut(|session| {
            let data = session.buffer.drain_output();
            if !data.is_empty() {
                session.last_activity = Instant::now();
                session.broadcast(&Message::new(0, MessageType::Output { data }));
                return true;
            }
            if session.buffer.pty().is_running() {
                return true;
            }
            tracing::info!(session = %session.name, "Mux session ended");
            session.broadcast(&Message::new(
                0,
                MessageType::SessionDestroyedNotification {
                    session_id: session.id.clone(),
                },
            ));
            false
        });
    }

    fn finished(&self) -> bool {
        !self.keep_alive
            && self.sessions.is_empty()
            && self.apps.is_empty()
            && (self.had_sessions || self.started.elapsed() >= IDLE_EXIT)
    }

    /// Guarda a conversa do agente na aba do processo `pid` (a aba cujo
    /// shell lidera a sessão de terminal do processo). Devolve a aba.
    fn report_agent(&mut self, pid: u32, agent: Option<AgentInfo>) -> Option<String> {
        let pid = i32::try_from(pid).ok()?;
        // SAFETY: getsid só consulta o id de sessão de um processo.
        let sid = unsafe { libc::getsid(pid) };
        let sid = u32::try_from(sid).ok().filter(|sid| *sid > 0)?;
        let app = self
            .apps
            .iter()
            .find(|app| {
                let app = app.lock();
                !app.ended() && app.pid() == Some(sid)
            })?
            .clone();
        let mut app = app.lock();
        app.set_agent(agent);
        Some(app.name.clone())
    }

    /// Grava as abas que mudaram (ou todas, com `force`). Os arquivos são
    /// escritos fora do lock da sessão.
    fn collect_snapshots(&self, force: bool) -> Vec<(TabRecord, Vec<u8>)> {
        self.apps
            .iter()
            .filter_map(|app| {
                let mut app = app.lock();
                if app.ended() {
                    return None;
                }
                app.take_snapshot(force)
            })
            .collect()
    }

    /// Acha ou cria a sessão de um `OpenStream`. Devolve a sessão e a
    /// resposta para o cliente.
    #[allow(clippy::too_many_arguments)]
    fn open_app(
        &mut self,
        id: u64,
        key: &str,
        size: (u16, u16),
        create: Option<StreamCreate>,
        master: Option<OwnedFd>,
        seed: &[u8],
    ) -> Result<(SharedAppSession, Message), Message> {
        let opened = |app: &SharedAppSession, created: bool| {
            let app = app.lock();
            Message::new(
                id,
                MessageType::StreamOpened {
                    session_id: app.id.clone(),
                    name: app.name.clone(),
                    pid: app.pid(),
                    created,
                },
            )
        };
        if let Some(app) = self.find_app(key) {
            let reply = opened(&app, false);
            return Ok((app, reply));
        }
        let fail = |code, text: String| Message::error(id, code, text);
        let Some(spec) = create else {
            return Err(fail(
                ErrorCode::SessionNotFound,
                format!("no persistent tab '{key}'"),
            ));
        };
        if key.trim().is_empty() || self.find(key).is_some() {
            return Err(fail(
                ErrorCode::SessionAlreadyExists,
                format!("name '{key}' is empty or taken"),
            ));
        }
        // Aba de antes do reboot: shell novo no diretório dela (o do agente,
        // se havia um), com o histórico de antes.
        let revived = self
            .dormant
            .iter()
            .position(|record| record.name == key)
            .map(|index| self.dormant.remove(index));
        let mut spec = spec;
        if let Some(record) = &revived {
            let dir = record
                .agent
                .as_ref()
                .and_then(|agent| agent.cwd.clone())
                .filter(|dir| Path::new(dir).is_dir())
                .unwrap_or_else(|| record.cwd.clone());
            if Path::new(&dir).is_dir() {
                spec.cwd = Some(dir);
            }
        }
        let scrollback = self.config.terminal.scrollback_limit;
        let store = self.store.clone();
        let shutting_down = Arc::clone(&self.shutting_down);
        let wake = Arc::clone(&self.wake);
        let on_end = move |ended: &AppSession| {
            // Encerrada de verdade (não pelo desligamento): sai do disco.
            if !shutting_down.load(Ordering::SeqCst) {
                if let Some(store) = &store {
                    store.remove(&ended.name);
                }
            }
            wake.notify_all();
        };
        let new = match (spec.adopt_pid, master) {
            (Some(_), Some(fd)) => AppSession::adopt(key, fd, &spec, seed, size, scrollback),
            (Some(_), None) => {
                return Err(fail(
                    ErrorCode::InvalidMessage,
                    "adopting a tab needs its PTY descriptor".into(),
                ))
            }
            (None, _) => AppSession::spawn(key, &spec, size, scrollback),
        }
        .and_then(|new| {
            {
                let mut session = new.session().lock();
                if let Some(record) = &revived {
                    let history = self
                        .store
                        .as_ref()
                        .and_then(|store| store.read_log(&record.name).ok())
                        .unwrap_or_default();
                    session.revive_from(record, &history);
                }
                if self.store.is_some() {
                    session.set_saver(Arc::clone(&self.saver));
                }
            }
            let started = new.start(on_end)?;
            AppSession::schedule_prefill(&started);
            Ok(started)
        })
        .map_err(|e| fail(ErrorCode::ServerError, e.to_string()))?;
        self.apps.push(std::sync::Arc::clone(&new));
        self.had_sessions = true;
        let reply = opened(&new, true);
        Ok((new, reply))
    }
}

/// Aba de antes do reboot, como aparece na lista (`state = "dormant"`).
fn dormant_info(record: &TabRecord) -> SessionInfo {
    SessionInfo {
        id: record.name.clone(),
        name: record.name.clone(),
        state: "dormant".into(),
        clients: 0,
        size: (0, 0),
        working_dir: record.cwd.clone(),
        created_at: record.saved_at.to_string(),
        idle_seconds: 0,
        flags: SessionFlagsInfo {
            destroy_on_exit: true,
            attachable: true,
            shared: false,
            locked: false,
        },
        app: Some(crate::multiplexer::protocol::AppSessionInfo {
            pid: None,
            meta: record.meta.clone(),
            hidden: record.hidden,
            kill_on_detach: false,
            auto_resume: record.auto_resume,
        }),
    }
}

/// Grava em disco as abas que mudaram, no máximo a cada [`SAVE_INTERVAL`].
/// Sem mudança, dorme no `SaveSignal` (não acorda à toa).
fn save_loop(state: &Mutex<DaemonState>, saver: &SaveSignal, store: &TabStore, interval: Duration) {
    while saver.wait() {
        // Junta as mudanças do intervalo numa gravação só.
        let deadline = Instant::now() + interval;
        while Instant::now() < deadline && !saver.stopped() {
            std::thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
        }
        if saver.stopped() {
            return;
        }
        let snapshots = state.lock().collect_snapshots(false);
        write_snapshots(store, &snapshots);
    }
}

fn write_snapshots(store: &TabStore, snapshots: &[(TabRecord, Vec<u8>)]) {
    for (record, log) in snapshots {
        if let Err(e) = store.save(record, log) {
            tracing::warn!(tab = %record.name, error = %e, "Failed to save persistent tab");
        }
    }
    if !snapshots.is_empty() {
        tracing::debug!(tabs = snapshots.len(), "Persistent tabs saved");
    }
}

/// Uma mensagem por linha; `false` se o cliente não aceitou.
fn send(writer: &Writer, msg: &Message) -> bool {
    let Ok(mut line) = msg.encode() else {
        return false;
    };
    line.push('\n');
    let mut conn = writer.lock();
    conn.write_all(line.as_bytes())
        .and_then(|_| conn.flush())
        .is_ok()
}

/// O daemon: socket, estado e a thread que lê os PTYs.
pub struct MuxDaemon {
    state: Arc<Mutex<DaemonState>>,
    stop: Arc<AtomicBool>,
    socket: PathBuf,
    /// Acorda o `accept` para sair: um byte aqui. Conectar no próprio socket
    /// não serve se o arquivo dele sumiu, e `shutdown` na escuta não acorda o
    /// `accept` no macOS.
    wake: Option<(Arc<UnixStream>, UnixStream)>,
}

impl MuxDaemon {
    pub fn new(socket: PathBuf, config: Config) -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        Self {
            state: Arc::new(Mutex::new(DaemonState {
                sessions: Vec::new(),
                apps: Vec::new(),
                keep_alive: false,
                started: Instant::now(),
                store: None,
                dormant: Vec::new(),
                saver: Arc::new(SaveSignal::default()),
                shutting_down: Arc::new(AtomicBool::new(false)),
                wake: Arc::new(parking_lot::Condvar::new()),
                pump_wakeups: 0,
                had_sessions: false,
                next_client: 0,
                next_name: 0,
                config,
                home,
            })),
            stop: Arc::new(AtomicBool::new(false)),
            socket,
            wake: UnixStream::pair().ok().map(|(tx, rx)| (Arc::new(tx), rx)),
        }
    }

    /// Não sair quando a última sessão termina (serviço do systemd: as tabs
    /// persistentes voltam mesmo antes de abrir a janela).
    pub fn with_keep_alive(self, keep_alive: bool) -> Self {
        self.state.lock().keep_alive = keep_alive;
        self
    }

    /// Grava as abas persistentes em `store` (e reabre as de antes do
    /// reboot). Sem isto (testes, `save_scrollback = false`), nada vai ao disco.
    pub fn with_store(self, store: Option<TabStore>) -> Self {
        {
            let mut state = self.state.lock();
            state.dormant = store.as_ref().map(TabStore::load_all).unwrap_or_default();
            state.store = store;
        }
        self
    }

    /// Abas de antes do reboot ainda não reabertas.
    pub fn dormant_count(&self) -> usize {
        self.state.lock().dormant.len()
    }

    /// Quantas vezes a varredura acordou.
    pub fn pump_wakeups(&self) -> u64 {
        self.state.lock().pump_wakeups
    }

    /// Desligando (SIGTERM do systemd no reboot): grava todas as abas agora
    /// e para. As abas que caírem depois disso continuam no disco.
    pub fn shutdown(&self) {
        let (store, snapshots) = {
            let state = self.state.lock();
            state.shutting_down.store(true, Ordering::SeqCst);
            (state.store.clone(), state.collect_snapshots(true))
        };
        if let Some(store) = store {
            write_snapshots(&store, &snapshots);
            tracing::info!(tabs = snapshots.len(), "Persistent tabs saved for shutdown");
        }
        self.stop();
    }

    /// Escuta o socket (`0600`). Falha se outro daemon já atende nele;
    /// remove um socket órfão de um daemon que morreu.
    pub fn bind(&self) -> std::io::Result<UnixListener> {
        bind_socket(&self.socket)
    }

    /// Atende até a última sessão terminar (ou [`MuxDaemon::stop`]).
    pub fn run(&self, listener: UnixListener) -> std::io::Result<()> {
        let pump = {
            let state = Arc::clone(&self.state);
            let stop = Arc::clone(&self.stop);
            let socket = self.socket.clone();
            let wake = self.wake.as_ref().map(|(tx, _)| Arc::clone(tx));
            std::thread::Builder::new()
                .name("mux-pump".into())
                .spawn(move || pump_loop(&state, &stop, &socket, wake.as_deref()))?
        };
        let saver = {
            let state_guard = self.state.lock();
            state_guard.store.clone().map(|store| {
                let state = Arc::clone(&self.state);
                let signal = Arc::clone(&state_guard.saver);
                std::thread::Builder::new()
                    .name("mux-save".into())
                    .spawn(move || save_loop(&state, &signal, &store, SAVE_INTERVAL))
            })
        };
        let saver = saver.transpose()?;
        loop {
            if self.stop.load(Ordering::SeqCst) || !self.wait_for_client(&listener) {
                break;
            }
            if self.stop.load(Ordering::SeqCst) {
                break;
            }
            let Ok((stream, _)) = listener.accept() else {
                continue;
            };
            let _ = stream.set_write_timeout(Some(CLIENT_WRITE_TIMEOUT));
            let state = Arc::clone(&self.state);
            let spawned = std::thread::Builder::new()
                .name("mux-client".into())
                .spawn(move || serve_connection(&state, stream));
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "Failed to spawn mux client thread");
            }
        }
        self.stop.store(true, Ordering::SeqCst);
        self.state.lock().saver.stop();
        if let Some(saver) = saver {
            saver.thread().unpark();
            let _ = saver.join();
        }
        let _ = pump.join();
        let _ = std::fs::remove_file(&self.socket);
        Ok(())
    }

    /// Espera uma conexão ou o aviso para sair. `false` = sair.
    fn wait_for_client(&self, listener: &UnixListener) -> bool {
        use std::os::fd::AsRawFd;
        let Some((_, rx)) = &self.wake else {
            return true; // sem o par de sockets, o `accept` bloqueia direto
        };
        let mut fds = [
            libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: rx.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        loop {
            // SAFETY: `fds` são 2 pollfd válidos durante a chamada.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return false;
            }
            if fds[1].revents != 0 {
                return false;
            }
            if fds[0].revents != 0 {
                return true;
            }
        }
    }

    /// Pede para o daemon parar (testes, desligamento).
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        {
            let state = self.state.lock();
            state.wake.notify_all();
        }
        wake_accept(self.wake.as_ref().map(|(tx, _)| tx.as_ref()), &self.socket);
    }
}

/// Acorda o `accept` do `run`: pelo par de sockets, senão conectando no
/// próprio socket.
fn wake_accept(wake: Option<&UnixStream>, socket: &Path) {
    match wake {
        Some(mut tx) => {
            let _ = tx.write_all(&[1]);
        }
        None => {
            let _ = UnixStream::connect(socket);
        }
    }
}

fn pump_loop(
    state: &Mutex<DaemonState>,
    stop: &AtomicBool,
    socket: &Path,
    wake: Option<&UnixStream>,
) {
    let mut guard = state.lock();
    while !stop.load(Ordering::SeqCst) {
        guard.pump_wakeups += 1;
        guard.pump();
        if guard.finished() {
            tracing::info!("Last mux session ended, daemon exiting");
            stop.store(true, Ordering::SeqCst);
            drop(guard);
            wake_accept(wake, socket);
            return;
        }
        if guard.sessions.is_empty() {
            // Só tabs persistentes (thread própria) ou nada: dorme até criar
            // uma sessão de grid, uma tab terminar ou mandarem parar. Sem
            // sessão nenhuma ainda, acorda para a saída por ociosidade.
            let wake = Arc::clone(&guard.wake);
            if guard.had_sessions || guard.keep_alive {
                wake.wait(&mut guard);
            } else {
                let left = IDLE_EXIT.saturating_sub(guard.started.elapsed());
                wake.wait_for(&mut guard, left.max(Duration::from_millis(10)));
            }
            continue;
        }
        drop(guard);
        std::thread::sleep(PUMP_INTERVAL);
        guard = state.lock();
    }
}

fn serve_connection(state: &Mutex<DaemonState>, stream: UnixStream) {
    let client = {
        let mut state = state.lock();
        state.next_client += 1;
        state.next_client
    };
    // A primeira linha pode trazer o descritor de um PTY (adotar uma tab).
    let Ok((first, rest, mut fds)) = recv_first_line(&stream) else {
        return;
    };
    let (Ok(read_half), Ok(write_half)) = (stream.try_clone(), stream.try_clone()) else {
        return;
    };
    let mut reader = BufReader::new(std::io::Cursor::new(rest).chain(read_half));
    let writer: Writer = Arc::new(Mutex::new(MuxConnection::Unix(write_half)));
    let mut me = ClientState::default();
    let mut pending = Some(first);
    loop {
        let line = match pending.take() {
            Some(line) => line,
            None => {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => line,
                }
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let msg = match Message::decode(line.trim_end()) {
            Ok(Message {
                msg_type: MessageType::Quit,
                ..
            }) => break,
            Ok(msg) => msg,
            Err(e) => Message::error(0, ErrorCode::InvalidMessage, e.to_string()),
        };
        if let MessageType::OpenStream {
            session,
            cols,
            rows,
            create,
            replay,
        } = msg.msg_type
        {
            // A conexão vira o fluxo da tab até a janela sair.
            let master = fds.pop();
            open_stream(
                state,
                client,
                msg.id,
                (&session, (cols, rows)),
                create,
                replay,
                master,
                &stream,
                reader,
            );
            return;
        }
        // Trata e responde sob o mesmo lock: a tela do attach chega antes de
        // qualquer saída nova que a thread dos PTYs mande.
        let mut guard = state.lock();
        let replies = match msg.msg_type {
            MessageType::Error { .. } => vec![msg],
            _ => guard.handle(client, &writer, &mut me, msg),
        };
        let delivered = replies.iter().all(|reply| send(&writer, reply));
        drop(guard);
        if !delivered {
            break;
        }
    }
    state.lock().detach(client);
}

/// `OpenStream`: acha ou cria a sessão e atende o fluxo da janela.
#[allow(clippy::too_many_arguments)]
fn open_stream(
    state: &Mutex<DaemonState>,
    client: u64,
    id: u64,
    (session, size): (&str, (u16, u16)),
    create: Option<StreamCreate>,
    replay: bool,
    master: Option<OwnedFd>,
    stream: &UnixStream,
    mut reader: impl BufRead,
) {
    let refuse = |msg: Message| {
        if let Ok(mut line) = msg.encode() {
            line.push('\n');
            let mut out = stream;
            let _ = out.write_all(line.as_bytes());
        }
    };
    // A tela da tab adotada vem logo depois da linha (fora do lock).
    let seed_len = create.as_ref().map_or(0, |spec| spec.seed_len);
    let mut seed = vec![0u8; seed_len.min(crate::multiplexer::stream::MAX_FRAME)];
    if reader.read_exact(&mut seed).is_err() {
        return;
    }
    let opened = state
        .lock()
        .open_app(id, session, size, create, master, &seed);
    let (app, reply) = match opened {
        Ok(found) => found,
        Err(msg) => return refuse(msg),
    };
    let (Ok(line), Ok(writer)) = (reply.encode(), stream.try_clone()) else {
        return;
    };
    let _ = writer.set_write_timeout(Some(CLIENT_WRITE_TIMEOUT));
    if let Err(e) = serve_stream(&app, client, writer, reader, &line, replay) {
        tracing::debug!(error = %e, "Persistent tab stream ended");
    }
}

/// Cria o socket com permissão `0600`.
pub fn bind_socket(path: &Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                format!("a mux daemon is already listening on {}", path.display()),
            ));
        }
        std::fs::remove_file(path)?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::SessionClient;

    struct Running {
        daemon: Arc<MuxDaemon>,
        socket: PathBuf,
        thread: Option<std::thread::JoinHandle<()>>,
        _dir: tempfile::TempDir,
    }

    impl Running {
        fn start() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let socket = dir.path().join("mux.sock");
            let daemon = Arc::new(MuxDaemon::new(socket.clone(), Config::default()));
            let listener = daemon.bind().unwrap();
            let runner = Arc::clone(&daemon);
            let thread = std::thread::spawn(move || runner.run(listener).unwrap());
            Self {
                daemon,
                socket,
                thread: Some(thread),
                _dir: dir,
            }
        }

        fn client(&self) -> SessionClient {
            SessionClient::with_connection(MuxConnection::connect_unix(&self.socket).unwrap())
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.daemon.stop();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// Lê mensagens até `pred` aceitar uma ou o tempo acabar.
    fn read_until(
        conn: &mut BufReader<MuxConnection>,
        mut pred: impl FnMut(&MessageType) -> bool,
    ) -> Vec<MessageType> {
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let mut line = String::new();
            match conn.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let msg = Message::decode(line.trim_end()).unwrap().msg_type;
                    let done = pred(&msg);
                    seen.push(msg);
                    if done {
                        break;
                    }
                }
                Err(_) => continue,
            }
        }
        seen
    }

    fn output_text(messages: &[MessageType]) -> String {
        messages
            .iter()
            .filter_map(|m| match m {
                MessageType::Output { data } => Some(String::from_utf8_lossy(data).to_string()),
                _ => None,
            })
            .collect()
    }

    fn line(msg: MessageType) -> Vec<u8> {
        let mut bytes = Message::new(1, msg).encode().unwrap().into_bytes();
        bytes.push(b'\n');
        bytes
    }

    /// O shell continua vivo depois que o cliente sai; quem anexa de novo vê
    /// a tela redesenhada com o que foi feito antes.
    #[test]
    fn test_session_survives_client_and_screen_is_restored() {
        let running = Running::start();
        let mut control = running.client();
        let (id, name) = control
            .create_session(Some("dev".into()), (60, 10))
            .unwrap();
        assert_eq!(name, "dev");
        assert!(control
            .create_session(Some("dev".into()), (60, 10))
            .is_err());

        // Primeiro cliente: anexa, roda um comando e cai sem se despedir.
        {
            let conn = MuxConnection::connect_unix(&running.socket).unwrap();
            conn.set_read_timeout(Some(Duration::from_millis(200)))
                .unwrap();
            let mut writer = conn.try_clone().unwrap();
            let mut reader = BufReader::new(conn);
            writer
                .write_all(&line(MessageType::Attach {
                    session_id: "dev".into(),
                    read_only: false,
                }))
                .unwrap();
            let seen = read_until(&mut reader, |m| matches!(m, MessageType::Output { .. }));
            assert!(matches!(seen[0], MessageType::Attached { .. }), "{seen:?}");
            writer
                .write_all(&line(MessageType::Input {
                    data: b"echo marca-$((40+2))\r".to_vec(),
                }))
                .unwrap();
            let mut text = String::new();
            let seen = read_until(&mut reader, |m| {
                if let MessageType::Output { data } = m {
                    text.push_str(&String::from_utf8_lossy(data));
                }
                text.contains("marca-42")
            });
            assert!(
                output_text(&seen).contains("marca-42"),
                "{}",
                output_text(&seen)
            );
        }

        // A sessão segue viva, sem cliente.
        let sessions = control.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, id.0.to_string());
        let deadline = Instant::now() + Duration::from_secs(3);
        while control.list_sessions().unwrap()[0].clients != 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(control.list_sessions().unwrap()[0].state, "detached");

        // Segundo cliente (pelo prefixo do id): recebe a tela com o resultado.
        let conn = MuxConnection::connect_unix(&running.socket).unwrap();
        conn.set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let mut writer = conn.try_clone().unwrap();
        let mut reader = BufReader::new(conn);
        writer
            .write_all(&line(MessageType::Attach {
                session_id: id.0.to_string()[..8].to_string(),
                read_only: true,
            }))
            .unwrap();
        let seen = read_until(&mut reader, |m| matches!(m, MessageType::Output { .. }));
        assert!(output_text(&seen).contains("marca-42"));
        // Somente leitura não digita.
        writer
            .write_all(&line(MessageType::Input {
                data: b"x".to_vec(),
            }))
            .unwrap();
        let seen = read_until(&mut reader, |m| matches!(m, MessageType::Error { .. }));
        assert!(seen.iter().any(|m| matches!(
            m,
            MessageType::Error {
                code: ErrorCode::PermissionDenied,
                ..
            }
        )));

        // Renomear e destruir avisam quem está anexado.
        control.rename_session("dev", "prod".into()).unwrap();
        control.destroy_session("prod").unwrap();
        let seen = read_until(&mut reader, |m| {
            matches!(m, MessageType::SessionDestroyedNotification { .. })
        });
        assert!(seen
            .iter()
            .any(|m| matches!(m, MessageType::SessionDestroyedNotification { .. })));
    }

    /// `exit` no shell encerra a sessão e, sendo a última, o daemon.
    #[test]
    fn test_daemon_exits_after_last_session_ends() {
        let mut running = Running::start();
        let mut control = running.client();
        control
            .create_session(Some("curta".into()), (40, 5))
            .unwrap();
        let conn = MuxConnection::connect_unix(&running.socket).unwrap();
        let mut writer = conn.try_clone().unwrap();
        writer
            .write_all(&line(MessageType::Attach {
                session_id: "curta".into(),
                read_only: false,
            }))
            .unwrap();
        writer
            .write_all(&line(MessageType::Input {
                data: b"exit\r".to_vec(),
            }))
            .unwrap();
        let thread = running.thread.take().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(thread.is_finished(), "daemon should exit with no sessions");
        thread.join().unwrap();
        assert!(!running.socket.exists(), "socket removed on exit");
    }

    /// Sem o arquivo do socket (apagaram o diretório), o daemon ainda sai
    /// quando a última sessão termina, em vez de ficar preso no `accept`.
    #[test]
    fn test_daemon_exits_even_if_socket_file_is_gone() {
        let mut running = Running::start();
        let mut control = running.client();
        control
            .create_session(Some("sem-socket".into()), (40, 5))
            .unwrap();
        let conn = MuxConnection::connect_unix(&running.socket).unwrap();
        let mut writer = conn.try_clone().unwrap();
        writer
            .write_all(&line(MessageType::Attach {
                session_id: "sem-socket".into(),
                read_only: false,
            }))
            .unwrap();
        std::fs::remove_file(&running.socket).unwrap();
        writer
            .write_all(&line(MessageType::Input {
                data: b"exit\r".to_vec(),
            }))
            .unwrap();
        let thread = running.thread.take().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(thread.is_finished(), "daemon stuck in accept");
        thread.join().unwrap();
    }

    /// Sem sessões de grid a varredura dorme (antes acordava a cada 8 ms
    /// mesmo parado).
    #[test]
    fn test_idle_daemon_does_not_poll() {
        let running = Running::start();
        std::thread::sleep(Duration::from_millis(200));
        let before = running.daemon.pump_wakeups();
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            running.daemon.pump_wakeups() - before <= 1,
            "woke {} times",
            running.daemon.pump_wakeups() - before
        );
    }

    #[test]
    fn test_second_daemon_refuses_live_socket_and_replaces_stale_one() {
        let running = Running::start();
        assert_eq!(
            bind_socket(&running.socket).unwrap_err().kind(),
            std::io::ErrorKind::AddrInUse
        );
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join("velho.sock");
        drop(UnixListener::bind(&stale).unwrap());
        assert!(bind_socket(&stale).is_ok());
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&stale).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn test_messages_before_attach_are_rejected() {
        let running = Running::start();
        let mut control = running.client();
        assert!(control.send_input(b"ls").is_err());
        assert!(control.attach("nada", false).is_err());
        assert!(control.list_sessions().unwrap().is_empty());
    }
}
