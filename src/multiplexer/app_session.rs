//! Sessões das tabs persistentes, do lado do daemon.
//!
//! Diferente das sessões do `mux attach` (que guardam um grid inteiro), a
//! sessão de uma tab do app guarda só o PTY e um **registro da saída**: os
//! bytes que o programa escreveu. Quem desenha é a janela; o daemon não
//! duplica o grid dela na memória. Ao anexar, a janela recebe o registro e o
//! passa pelo próprio parser, o que devolve a tela, o histórico (scrollback),
//! cores, título, links OSC 8 e marcas OSC 133 do jeito que estavam.
//!
//! Para o registro não crescer sem fim, de tempos em tempos ele é
//! **compactado**: o daemon passa os bytes por um grid temporário (do tamanho
//! do histórico configurado), escreve o resultado de volta como texto ANSI
//! ([`history_to_ansi`]) e joga o grid fora.
//!
//! Cada sessão tem uma thread que lê o PTY e repassa na hora aos clientes
//! anexados (sem varredura periódica no caminho da saída).

use crate::multiplexer::protocol::{AppSessionInfo, SessionFlagsInfo, SessionInfo, StreamCreate};
use crate::multiplexer::screen::history_to_ansi;
use crate::multiplexer::stream::{
    parse_resize, read_frame, write_frame, FRAME_EXITED, FRAME_INPUT, FRAME_KILL, FRAME_OUTPUT,
    FRAME_REPLAY_DONE, FRAME_RESIZE, REPLAY_CHUNK,
};
use crate::multiplexer::tab_store::{AgentInfo, TabRecord};
use crate::terminal::grid::Grid;
use crate::terminal::parser::Parser;
use parking_lot::Mutex;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// O registro é compactado quando passa disto (ou do dobro do tamanho que
/// tinha depois da última compactação, o que for maior).
pub const LOG_SOFT_LIMIT: usize = 2 * 1024 * 1024;

/// Sessão compartilhada entre a thread do PTY e as conexões.
pub type SharedAppSession = Arc<Mutex<AppSession>>;

/// Programa em primeiro plano é amostrado no máximo uma vez por isto.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// Aviso à thread que grava as abas em disco: alguma aba mudou. A thread
/// dorme no `Condvar` enquanto nada muda (não acorda à toa).
#[derive(Debug, Default)]
pub struct SaveSignal {
    pending: Mutex<bool>,
    cv: parking_lot::Condvar,
    stop: std::sync::atomic::AtomicBool,
}

impl SaveSignal {
    /// Alguma aba mudou.
    pub fn notify(&self) {
        let mut pending = self.pending.lock();
        if !*pending {
            *pending = true;
            self.cv.notify_one();
        }
    }

    /// Espera uma mudança. Devolve `false` se mandaram parar.
    pub fn wait(&self) -> bool {
        use std::sync::atomic::Ordering;
        let mut pending = self.pending.lock();
        while !*pending && !self.stop.load(Ordering::SeqCst) {
            self.cv.wait(&mut pending);
        }
        *pending = false;
        !self.stop.load(Ordering::SeqCst)
    }

    /// Acorda a thread para ela sair.
    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let _guard = self.pending.lock();
        self.cv.notify_all();
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Saída do programa guardada para quem anexar depois.
#[derive(Debug)]
pub struct ReplayLog {
    bytes: Vec<u8>,
    compacted_len: usize,
    soft_limit: usize,
    scrollback: usize,
}

impl ReplayLog {
    pub fn new(scrollback: usize, soft_limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            compacted_len: 0,
            soft_limit,
            scrollback,
        }
    }

    /// Acrescenta saída; compacta quando passa do limite.
    pub fn push(&mut self, data: &[u8], size: (u16, u16)) {
        self.bytes.extend_from_slice(data);
        if self.bytes.len() > self.soft_limit.max(self.compacted_len * 2) {
            self.compact(size);
        }
    }

    /// Troca os bytes pelo que eles desenham num terminal `size` (histórico
    /// incluído).
    pub fn compact(&mut self, (cols, rows): (u16, u16)) {
        let started = Instant::now();
        let before = self.bytes.len();
        let mut grid = Grid::with_scrollback_limit(
            usize::from(cols.max(2)),
            usize::from(rows.max(2)),
            self.scrollback,
        );
        Parser::new(&mut grid).process(&self.bytes);
        self.bytes = history_to_ansi(&grid);
        self.bytes.shrink_to_fit();
        self.compacted_len = self.bytes.len();
        drop(grid);
        trim_heap();
        tracing::debug!(
            before,
            after = self.compacted_len,
            ms = started.elapsed().as_millis() as u64,
            "Replay log compacted"
        );
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// Devolve ao sistema a memória do grid temporário da compactação.
fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim só reorganiza o heap do glibc; sem pré-condições.
    unsafe {
        libc::malloc_trim(0);
    }
}

struct StreamClient {
    id: u64,
    writer: UnixStream,
}

/// Uma tab persistente: PTY, registro da saída e quem está anexado.
pub struct AppSession {
    pub id: String,
    pub name: String,
    master: File,
    pid: Option<u32>,
    cwd: PathBuf,
    created: SystemTime,
    last_activity: Instant,
    log: ReplayLog,
    clients: Vec<StreamClient>,
    size: (u16, u16),
    /// Dados da tab (título, grupo, posição), guardados como o app mandou.
    pub meta: String,
    /// "Só esconder": não volta sozinha ao abrir a janela.
    pub hidden: bool,
    /// Deixou de ser persistente: morre quando a janela desanexa.
    pub kill_on_detach: bool,
    /// Retomar o programa sozinho depois de reiniciar a máquina.
    pub auto_resume: bool,
    ended: bool,
    /// Mudou desde a última gravação em disco.
    dirty: bool,
    saver: Option<Arc<SaveSignal>>,
    /// Programa de IA da aba (dos hooks dele).
    agent: Option<AgentInfo>,
    foreground: Option<Vec<String>>,
    last_command: Option<Vec<String>>,
    last_sample: Option<Instant>,
    /// Comando a deixar digitado no prompt quando o shell novo aparecer.
    pending_prefill: Option<String>,
    /// O programa já escreveu algo (o prompt).
    saw_output: bool,
}

impl std::fmt::Debug for AppSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppSession")
            .field("name", &self.name)
            .field("pid", &self.pid)
            .field("clients", &self.clients.len())
            .field("log", &self.log.len())
            .finish_non_exhaustive()
    }
}

/// Sessão recém-criada e o que a thread do PTY precisa.
pub struct NewAppSession {
    pub session: SharedAppSession,
    reader: File,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
}

impl NewAppSession {
    /// Liga a thread que lê o PTY. `on_end` roda quando o programa sai,
    /// com a sessão (já marcada como encerrada).
    pub fn start(
        self,
        on_end: impl FnOnce(&AppSession) + Send + 'static,
    ) -> io::Result<SharedAppSession> {
        let session = Arc::clone(&self.session);
        let name = session.lock().name.clone();
        std::thread::Builder::new()
            .name(format!("mux-app-{name}"))
            .spawn(move || {
                pump(&self.session, self.reader, self.child);
                let ended = self.session.lock();
                on_end(&ended);
            })?;
        Ok(session)
    }

    /// Sessão ainda não ligada (para preparar antes da thread do PTY).
    pub fn session(&self) -> &SharedAppSession {
        &self.session
    }
}

impl AppSession {
    /// Sessão nova rodando o comando de `spec` (ou o shell) num PTY do
    /// daemon, com o ambiente que o app mandou.
    pub fn spawn(
        name: &str,
        spec: &StreamCreate,
        size: (u16, u16),
        scrollback: usize,
    ) -> io::Result<NewAppSession> {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};
        let (cols, rows) = clamp_size(size);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(io::Error::other)?;
        let shell = spec
            .env
            .iter()
            .find(|(key, _)| key == "SHELL")
            .map(|(_, value)| value.clone())
            .unwrap_or_else(|| "/bin/sh".to_string());
        let (program, args) = match spec.command.as_deref() {
            Some([program, args @ ..]) => (program.clone(), args.to_vec()),
            _ => (shell, Vec::new()),
        };
        let mut cmd = CommandBuilder::new(&program);
        cmd.env_clear();
        for (key, value) in &spec.env {
            cmd.env(key, value);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "code-terminal");
        cmd.env(crate::multiplexer::daemon::SESSION_ENV, name);
        cmd.args(&args);
        let cwd = spec
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"));
        cmd.cwd(&cwd);
        let child = pair.slave.spawn_command(cmd).map_err(io::Error::other)?;
        drop(pair.slave);
        let fd = pair
            .master
            .as_raw_fd()
            .ok_or_else(|| io::Error::other("PTY without a descriptor"))?;
        let master = dup_file(fd)?;
        drop(pair.master);
        let pid = child.process_id();
        tracing::info!(
            session = name,
            ?pid,
            program,
            "Persistent tab session spawned"
        );
        Self::build(
            name,
            master,
            pid,
            cwd,
            (cols, rows),
            scrollback,
            spec,
            Some(child),
        )
    }

    /// Sessão para o PTY de uma tab que já existia (o descritor veio pelo
    /// socket). `seed` é a tela atual da tab, para quem anexar depois.
    pub fn adopt(
        name: &str,
        master: OwnedFd,
        spec: &StreamCreate,
        seed: &[u8],
        size: (u16, u16),
        scrollback: usize,
    ) -> io::Result<NewAppSession> {
        let pid = spec.adopt_pid;
        let cwd = spec.cwd.as_deref().map(PathBuf::from).unwrap_or_default();
        let new = Self::build(
            name,
            File::from(master),
            pid,
            cwd,
            clamp_size(size),
            scrollback,
            spec,
            None,
        )?;
        {
            let mut session = new.session.lock();
            let size = session.size;
            session.log.push(seed, size);
        }
        if let Some(pid) = pid {
            move_to_own_cgroup(pid);
        }
        tracing::info!(session = name, ?pid, "Running tab adopted as persistent");
        Ok(new)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(
        name: &str,
        master: File,
        pid: Option<u32>,
        cwd: PathBuf,
        size: (u16, u16),
        scrollback: usize,
        spec: &StreamCreate,
        child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    ) -> io::Result<NewAppSession> {
        let reader = master.try_clone()?;
        let session = AppSession {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            master,
            pid,
            cwd,
            created: SystemTime::now(),
            last_activity: Instant::now(),
            log: ReplayLog::new(scrollback, LOG_SOFT_LIMIT),
            clients: Vec::new(),
            size,
            meta: spec.meta.clone(),
            hidden: false,
            kill_on_detach: false,
            auto_resume: false,
            ended: false,
            dirty: true,
            saver: None,
            agent: None,
            foreground: None,
            last_command: None,
            last_sample: None,
            pending_prefill: None,
            saw_output: false,
        };
        Ok(NewAppSession {
            session: Arc::new(Mutex::new(session)),
            reader,
            child,
        })
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Liga a gravação em disco: avisa `saver` quando a aba mudar.
    pub fn set_saver(&mut self, saver: Arc<SaveSignal>) {
        self.dirty = true;
        saver.notify();
        self.saver = Some(saver);
    }

    /// Algo da aba mudou: grava na próxima rodada (avisa só na primeira
    /// mudança desde a última gravação).
    pub fn mark_dirty(&mut self) {
        let was_dirty = std::mem::replace(&mut self.dirty, true);
        if !was_dirty {
            if let Some(saver) = &self.saver {
                saver.notify();
            }
        }
    }

    /// Pega o que gravar (se mudou, ou sempre com `force`) e marca como
    /// gravado. O registro da saída é copiado: a gravação roda fora do lock.
    pub fn take_snapshot(&mut self, force: bool) -> Option<(TabRecord, Vec<u8>)> {
        if !self.dirty && !force {
            return None;
        }
        self.dirty = false;
        self.sample_foreground(true);
        Some((self.record(), self.log.bytes().to_vec()))
    }

    /// Dados da aba para o disco.
    pub fn record(&self) -> TabRecord {
        TabRecord {
            name: self.name.clone(),
            meta: self.meta.clone(),
            hidden: self.hidden,
            auto_resume: self.auto_resume,
            cwd: self.current_dir().display().to_string(),
            saved_at: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            foreground: self.foreground.clone(),
            last_command: self.last_command.clone(),
            agent: self.agent.clone(),
        }
    }

    /// Aba reaberta depois do reboot: o histórico de antes, os dados dela e
    /// o comando para retomar (digitado no prompt quando o shell aparecer;
    /// com Enter só se a aba pede para retomar sozinha).
    pub fn revive_from(&mut self, record: &TabRecord, history: &[u8]) {
        let size = self.size;
        let mut seed = history.to_vec();
        seed.extend_from_slice(&crate::multiplexer::tab_store::reopened_banner(
            record.saved_at,
        ));
        self.log.push(&seed, size);
        self.meta = record.meta.clone();
        self.hidden = record.hidden;
        self.auto_resume = record.auto_resume;
        self.agent = None;
        self.last_command = record.last_command.clone();
        self.pending_prefill = crate::multiplexer::tab_store::resume_command(record).map(|cmd| {
            if record.auto_resume {
                format!("{cmd}\r")
            } else {
                cmd
            }
        });
        self.mark_dirty();
    }

    /// Deixa o comando para retomar digitado no prompt quando o shell novo
    /// parar de escrever (o prompt pronto): escrito antes, o terminal o
    /// ecoaria antes do prompt e ele apareceria duas vezes.
    pub fn schedule_prefill(session: &SharedAppSession) {
        if session.lock().pending_prefill.is_none() {
            return;
        }
        let session = Arc::clone(session);
        let spawned = std::thread::Builder::new()
            .name("mux-app-resume".into())
            .spawn(move || {
                let started = Instant::now();
                loop {
                    std::thread::sleep(Duration::from_millis(50));
                    let mut guard = session.lock();
                    if guard.ended || guard.pending_prefill.is_none() {
                        return;
                    }
                    let quiet = guard.saw_output
                        && guard.last_activity.elapsed() >= Duration::from_millis(250);
                    if quiet || started.elapsed() >= Duration::from_secs(10) {
                        if let Some(prefill) = guard.pending_prefill.take() {
                            let _ = guard.master.write_all(prefill.as_bytes());
                        }
                        return;
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "Failed to schedule the resume command");
        }
    }

    /// Hook do agente: conversa aberta (ou encerrada, com `None`).
    pub fn set_agent(&mut self, agent: Option<AgentInfo>) {
        if self.agent != agent {
            self.agent = agent;
            self.mark_dirty();
        }
    }

    pub fn agent(&self) -> Option<&AgentInfo> {
        self.agent.as_ref()
    }

    /// Comando que ficou digitado no prompt (testes).
    pub fn pending_prefill(&self) -> Option<&str> {
        self.pending_prefill.as_deref()
    }

    /// Anota o programa em primeiro plano (no máximo uma vez por segundo,
    /// ou agora com `now`).
    fn sample_foreground(&mut self, now: bool) {
        if !now
            && self
                .last_sample
                .is_some_and(|at| at.elapsed() < SAMPLE_INTERVAL)
        {
            return;
        }
        self.last_sample = Some(Instant::now());
        // SAFETY: tcgetpgrp só consulta o descritor do PTY desta sessão.
        let group = unsafe { libc::tcgetpgrp(self.master.as_raw_fd()) };
        let shell = self.pid.and_then(|pid| i32::try_from(pid).ok());
        if group <= 0 || Some(group) == shell {
            self.foreground = None;
            return;
        }
        let argv = command_line(group);
        if argv.as_ref().is_some_and(|argv| !argv.is_empty()) {
            self.last_command.clone_from(&argv);
        }
        self.foreground = argv;
    }

    pub fn ended(&self) -> bool {
        self.ended
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// Diretório atual do programa (o do `/proc`, senão o inicial).
    pub fn current_dir(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        if let Some(pid) = self.pid {
            if let Ok(dir) = std::fs::read_link(format!("/proc/{pid}/cwd")) {
                return dir;
            }
        }
        self.cwd.clone()
    }

    pub fn info(&self) -> SessionInfo {
        let created = self
            .created
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();
        SessionInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            state: if self.clients.is_empty() {
                "detached".into()
            } else {
                "attached".into()
            },
            clients: self.clients.len(),
            size: self.size,
            working_dir: self.current_dir().display().to_string(),
            created_at: created,
            idle_seconds: self.last_activity.elapsed().as_secs(),
            flags: SessionFlagsInfo {
                destroy_on_exit: true,
                attachable: true,
                shared: self.clients.len() > 1,
                locked: false,
            },
            app: Some(AppSessionInfo {
                pid: self.pid,
                meta: self.meta.clone(),
                hidden: self.hidden,
                kill_on_detach: self.kill_on_detach,
                auto_resume: self.auto_resume,
            }),
        }
    }

    /// Saída nova do programa: guarda e repassa a quem está anexado.
    fn output(&mut self, data: &[u8]) {
        self.last_activity = Instant::now();
        // Repassa antes de compactar: a compactação não atrasa a tela.
        self.broadcast(FRAME_OUTPUT, data);
        let size = self.size;
        self.log.push(data, size);
        self.saw_output = true;
        self.sample_foreground(false);
        self.mark_dirty();
    }

    fn broadcast(&mut self, kind: u8, payload: &[u8]) {
        self.clients
            .retain_mut(|client| write_frame(&mut client.writer, kind, payload).is_ok());
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = clamp_size((cols, rows));
        let size = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: TIOCSWINSZ lê um `winsize` válido do ponteiro; o descritor
        // é do PTY desta sessão.
        let result = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ as _, &size) };
        if result == 0 {
            self.size = (cols, rows);
        }
    }

    /// Encerra o programa como um terminal que fecha: SIGHUP para o shell e
    /// para o que está em primeiro plano; SIGKILL se não sair logo.
    pub fn kill(&mut self) {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let Some(pid) = self.pid.and_then(|pid| i32::try_from(pid).ok()) else {
            return;
        };
        // SAFETY: tcgetpgrp só consulta o descritor do PTY desta sessão.
        let foreground = unsafe { libc::tcgetpgrp(self.master.as_raw_fd()) };
        if foreground > 0 && foreground != pid {
            let _ = kill(Pid::from_raw(-foreground), Signal::SIGHUP);
        }
        let _ = kill(Pid::from_raw(pid), Signal::SIGHUP);
        let spawned = std::thread::Builder::new()
            .name("mux-app-kill".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                if kill(Pid::from_raw(pid), None).is_ok() {
                    let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "Failed to spawn kill thread");
        }
    }

    fn detach_client(&mut self, client: u64) {
        self.clients.retain(|c| c.id != client);
        if self.clients.is_empty() && self.kill_on_detach {
            tracing::info!(session = %self.name, "Non-persistent tab detached, ending it");
            self.kill();
        }
    }
}

/// Linha de comando de um processo (`/proc/<pid>/cmdline`).
#[cfg(target_os = "linux")]
fn command_line(pid: i32) -> Option<Vec<String>> {
    let data = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(
        data.split(|&b| b == 0)
            .filter(|arg| !arg.is_empty())
            .map(|arg| String::from_utf8_lossy(arg).into_owned())
            .collect(),
    )
}

#[cfg(not(target_os = "linux"))]
fn command_line(_pid: i32) -> Option<Vec<String>> {
    None
}

fn clamp_size((cols, rows): (u16, u16)) -> (u16, u16) {
    (cols.clamp(2, 1000), rows.clamp(2, 1000))
}

fn dup_file(fd: std::os::fd::RawFd) -> io::Result<File> {
    // SAFETY: `fd` é um descritor aberto que pertence a quem chama; o
    // `BorrowedFd` não passa desta linha e a cópia vira um `File` nosso.
    let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    Ok(File::from(borrowed.try_clone_to_owned()?))
}

/// Lê o PTY até o programa sair e avisa os clientes.
fn pump(
    session: &SharedAppSession,
    mut reader: File,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => session.lock().output(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            // EIO: o último descritor do lado do programa fechou.
            Err(_) => break,
        }
    }
    let code = child
        .and_then(|mut child| child.wait().ok())
        .map(|status| i32::try_from(status.exit_code()).unwrap_or(i32::MAX))
        .unwrap_or(-1);
    let mut session = session.lock();
    tracing::info!(session = %session.name, code, "Persistent tab session ended");
    session.ended = true;
    session.broadcast(FRAME_EXITED, &code.to_le_bytes());
    session.clients.clear();
}

/// Atende uma janela anexada: manda a resposta (`reply`, uma linha JSON),
/// o histórico (se `replay`) e depois troca quadros até ela sair.
pub fn serve_stream(
    session: &SharedAppSession,
    client: u64,
    writer: UnixStream,
    mut reader: impl Read,
    reply: &str,
    replay: bool,
) -> io::Result<()> {
    let mut master = {
        let mut guard = session.lock();
        let mut writer = writer;
        writer.write_all(reply.as_bytes())?;
        writer.write_all(b"\n")?;
        // Sob o lock da sessão: nenhuma saída nova passa na frente do
        // histórico, nem se perde entre ele e o registro do cliente.
        if replay {
            for chunk in guard.log.bytes().chunks(REPLAY_CHUNK) {
                write_frame(&mut writer, FRAME_OUTPUT, chunk)?;
            }
        }
        write_frame(&mut writer, FRAME_REPLAY_DONE, &[])?;
        if guard.ended {
            write_frame(&mut writer, FRAME_EXITED, &(-1i32).to_le_bytes())?;
            return Ok(());
        }
        guard.clients.push(StreamClient { id: client, writer });
        guard.master.try_clone()?
    };
    while let Ok((kind, payload)) = read_frame(&mut reader) {
        match kind {
            FRAME_INPUT => {
                // Fora do lock: um programa que não lê a entrada não trava a
                // saída das outras conexões.
                if master.write_all(&payload).is_err() {
                    break;
                }
                session.lock().last_activity = Instant::now();
            }
            FRAME_RESIZE => {
                if let Some((cols, rows)) = parse_resize(&payload) {
                    session.lock().resize(cols, rows);
                }
            }
            FRAME_KILL => session.lock().kill(),
            other => tracing::debug!(kind = other, "Unknown stream frame"),
        }
    }
    session.lock().detach_client(client);
    Ok(())
}

/// Leva o shell adotado (e o que ele já rodou) para o cgroup do daemon. No
/// systemd, reiniciar o `code-terminal-server` mata tudo o que está no
/// cgroup dele; um shell que nasceu lá precisa sair de lá para sobreviver.
#[cfg(target_os = "linux")]
fn move_to_own_cgroup(leader: u32) {
    let Some(target) = own_cgroup_procs() else {
        return;
    };
    let mut moved = 0;
    for pid in session_members(leader) {
        if std::fs::write(&target, pid.to_string()).is_ok() {
            moved += 1;
        }
    }
    tracing::info!(
        leader,
        moved,
        cgroup = %target.display(),
        "Adopted processes moved to the daemon cgroup"
    );
}

#[cfg(not(target_os = "linux"))]
fn move_to_own_cgroup(_leader: u32) {}

/// `cgroup.procs` do cgroup (v2) deste processo.
#[cfg(target_os = "linux")]
fn own_cgroup_procs() -> Option<PathBuf> {
    let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let path = text.lines().find_map(|line| line.strip_prefix("0::"))?;
    let procs = PathBuf::from(format!("/sys/fs/cgroup{}", path.trim())).join("cgroup.procs");
    procs.exists().then_some(procs)
}

/// Processos da sessão (do terminal) liderada por `leader`.
#[cfg(target_os = "linux")]
fn session_members(leader: u32) -> Vec<u32> {
    let mut members = vec![leader];
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return members;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == leader {
            continue;
        }
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        // Campos depois do nome: estado, ppid, pgrp, sid...
        let fields: Vec<&str> = stat
            .rsplit_once(')')
            .map(|(_, rest)| rest.split_whitespace().collect())
            .unwrap_or_default();
        if fields.get(3).and_then(|sid| sid.parse::<u32>().ok()) == Some(leader) {
            members.push(pid);
        }
    }
    members
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen_of(bytes: &[u8], cols: usize, rows: usize) -> (Vec<String>, Vec<String>) {
        let mut grid = Grid::with_scrollback_limit(cols, rows, 1000);
        Parser::new(&mut grid).process(bytes);
        let text = |row: &Vec<crate::terminal::grid::Cell>| {
            row.iter()
                .map(|c| c.char)
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        let history = grid
            .scrollback_lines()
            .iter()
            .map(|row| text(&row))
            .collect();
        let screen = grid.visible_lines().into_iter().map(text).collect();
        (history, screen)
    }

    /// Compactar não muda o que quem anexa vê: mesmo histórico, mesma tela.
    #[test]
    fn test_compaction_keeps_history_and_screen() {
        let mut log = ReplayLog::new(1000, 1024);
        let mut expected = Vec::new();
        for i in 0..300 {
            let line = format!("\x1b[1;3{}mlinha {i}\x1b[0m\r\n", i % 7);
            expected.extend_from_slice(line.as_bytes());
            log.push(line.as_bytes(), (40, 10));
        }
        assert!(log.compacted_len > 0, "log was compacted");
        let (history, screen) = screen_of(log.bytes(), 40, 10);
        let (want_history, want_screen) = screen_of(&expected, 40, 10);
        assert_eq!(history, want_history);
        assert_eq!(screen, want_screen);
        assert!(history.first().is_some_and(|l| l == "linha 0"));
    }

    /// Histórico além do limite configurado cai na compactação.
    #[test]
    fn test_compaction_respects_scrollback_limit() {
        let mut log = ReplayLog::new(50, 512);
        for i in 0..2000 {
            log.push(format!("{i}\r\n").as_bytes(), (20, 5));
        }
        log.compact((20, 5));
        let (history, screen) = screen_of(log.bytes(), 20, 5);
        assert!(history.len() <= 50 + 5, "{}", history.len());
        assert!(screen.iter().any(|l| l == "1999"));
        assert!(!history.iter().any(|l| l == "10"));
    }

    #[test]
    fn test_spawned_session_runs_command_and_reports_exit() {
        let spec = StreamCreate {
            command: Some(vec![
                "/bin/sh".into(),
                "-c".into(),
                "echo ola; exit 3".into(),
            ]),
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
            ..Default::default()
        };
        let new = AppSession::spawn("t", &spec, (40, 5), 100).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let session = new
            .start(move |_| {
                let _ = tx.send(());
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let guard = session.lock();
        assert!(guard.ended());
        assert!(String::from_utf8_lossy(guard.log.bytes()).contains("ola"));
        assert!(guard.info().app.unwrap().pid.is_some());
    }

    /// A thread de gravação só acorda quando alguma aba muda (e para quando
    /// mandam).
    #[test]
    fn test_save_signal_sleeps_until_change() {
        let signal = Arc::new(SaveSignal::default());
        let woke = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let thread = {
            let (signal, woke) = (Arc::clone(&signal), Arc::clone(&woke));
            std::thread::spawn(move || {
                while signal.wait() {
                    woke.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
            })
        };
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(woke.load(std::sync::atomic::Ordering::SeqCst), 0);
        signal.notify();
        signal.notify();
        std::thread::sleep(Duration::from_millis(100));
        // Acordou pela mudança (uma ou duas vezes, conforme a corrida)...
        let after = woke.load(std::sync::atomic::Ordering::SeqCst);
        assert!((1..=2).contains(&after), "{after}");
        // ...e volta a dormir: sem mudança nova, não acorda mais.
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(woke.load(std::sync::atomic::Ordering::SeqCst), after);
        signal.stop();
        thread.join().unwrap();
    }

    /// Aba reaberta: histórico de antes, a marca e o comando para retomar
    /// esperando o prompt (sem Enter, salvo "retomar automaticamente").
    #[test]
    fn test_revive_seeds_history_and_queues_resume() {
        let spec = StreamCreate {
            command: Some(vec!["/bin/sh".into(), "-c".into(), "sleep 5".into()]),
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
            ..Default::default()
        };
        let new = AppSession::spawn("t", &spec, (40, 5), 100).unwrap();
        let record = TabRecord {
            name: "t".into(),
            saved_at: 1_700_000_000,
            agent: Some(AgentInfo {
                kind: "claude".into(),
                session_id: "abc".into(),
                cwd: None,
            }),
            ..Default::default()
        };
        {
            let mut session = new.session().lock();
            session.revive_from(&record, b"antes do reboot\r\n");
            assert_eq!(session.pending_prefill(), Some("claude --resume abc"));
            let log = String::from_utf8_lossy(session.log.bytes()).to_string();
            assert!(log.starts_with("antes do reboot"), "{log}");
            assert!(log.contains("aba reaberta"));
            let mut auto = record.clone();
            auto.auto_resume = true;
            session.revive_from(&auto, b"");
            assert_eq!(session.pending_prefill(), Some("claude --resume abc\r"));
            session.kill();
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_session_members_include_leader() {
        let me = std::process::id();
        assert!(session_members(me).contains(&me));
    }
}
