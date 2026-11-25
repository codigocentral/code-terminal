use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};
use tracing::{debug, error, info, warn};

/// Função chamada quando chega saída de qualquer PTY (ou ele termina).
type OutputWaker = Arc<dyn Fn() + Send + Sync>;

static OUTPUT_WAKER: std::sync::RwLock<Option<OutputWaker>> = std::sync::RwLock::new(None);

/// Registra quem deve ser acordado quando um PTY produz saída.
///
/// A UI usa isso para dormir até ter algo novo para desenhar, em vez de
/// redesenhar sem parar (o que deixava um núcleo da CPU em 100%).
pub fn set_output_waker(waker: impl Fn() + Send + Sync + 'static) {
    if let Ok(mut slot) = OUTPUT_WAKER.write() {
        *slot = Some(Arc::new(waker));
    }
}

/// Remove o despertador registrado.
pub fn clear_output_waker() {
    if let Ok(mut slot) = OUTPUT_WAKER.write() {
        *slot = None;
    }
}

/// Acorda o event loop para redesenhar (ex.: chegou a resposta da IA).
pub fn request_redraw() {
    wake_output_listener();
}

fn wake_output_listener() {
    let waker = OUTPUT_WAKER.read().ok().and_then(|slot| slot.clone());
    if let Some(waker) = waker {
        waker();
    }
}

/// Status do processo filho
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    /// Processo está rodando
    Running,
    /// Processo terminou normalmente
    Exited(i32),
    /// Processo foi terminado por sinal
    Signaled(i32),
    /// Status desconhecido
    Unknown,
}

impl std::fmt::Display for ProcessStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessStatus::Running => write!(f, "running"),
            ProcessStatus::Exited(code) => write!(f, "exited({})", code),
            ProcessStatus::Signaled(sig) => write!(f, "signaled({})", sig),
            ProcessStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Pedaço da saída do PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyChunk {
    pub data: Vec<u8>,
    /// Histórico reenviado ao anexar uma tab persistente: já foi visto, o
    /// parser não responde a consultas nem dispara avisos com ele.
    pub replay: bool,
}

/// Onde o processo roda.
enum Backend {
    /// PTY deste processo (tab comum).
    Local {
        master: Mutex<Box<dyn MasterPty + Send>>,
        /// Handle para o processo filho (Mutex para permitir &self nos métodos)
        child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
        /// Thread de leitura que dá para parar (para entregar o PTY ao daemon).
        #[cfg(target_os = "linux")]
        reader: Option<StoppableReader>,
    },
    /// PTY no daemon do multiplexer (tab persistente).
    #[cfg(unix)]
    Remote(super::remote_pty::RemoteLink),
}

/// Gerenciador de processo PTY
pub struct Pty {
    backend: Backend,
    writer: Mutex<Box<dyn Write + Send>>,
    /// Para a leitura que substitui a local (tab persistente).
    #[cfg_attr(not(unix), allow(dead_code))]
    output_tx: Sender<PtyChunk>,
    output_rx: Mutex<Receiver<PtyChunk>>,
    /// PID do processo filho
    pid: Option<u32>,
    /// Flag indicando se o PTY está ativo
    running: Arc<AtomicBool>,
    /// Canal para notificação de término
    exit_tx: Sender<ProcessStatus>,
    /// Canal para receber notificação de término
    exit_rx: Mutex<Receiver<ProcessStatus>>,
}

impl std::fmt::Debug for Pty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pty")
            .field("pid", &self.pid)
            .field("running", &self.running)
            .finish_non_exhaustive()
    }
}

impl Pty {
    /// Cria um novo PTY com as dimensões especificadas
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        let shell = Self::get_default_shell();
        info!(shell = %shell, "Iniciando shell");
        Self::with_command(cols, rows, &shell, &[])
    }

    /// Cria um novo PTY com o shell padrão no diretório `cwd`.
    pub fn new_in(cols: u16, rows: u16, cwd: Option<&Path>) -> Result<Self> {
        let shell = Self::get_default_shell();
        info!(shell = %shell, ?cwd, "Iniciando shell");
        Self::spawn(cols, rows, &shell, &[], cwd)
    }

    /// Cria um novo PTY executando um comando específico.
    pub fn with_command(cols: u16, rows: u16, program: &str, args: &[String]) -> Result<Self> {
        Self::spawn(cols, rows, program, args, None)
    }

    /// Diretório inicial do processo: o pedido, senão o diretório atual do
    /// terminal. Sem isso o portable-pty usa o `$HOME`, e um `code-terminal`
    /// aberto dentro de um projeto começava fora dele.
    fn initial_dir(cwd: Option<&Path>) -> Option<PathBuf> {
        cwd.map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .filter(|dir| dir.is_dir())
    }

    /// Inicia `program` (ou o shell padrão, se `None`) com diretório e
    /// variáveis de ambiente extras.
    pub fn spawn_with(
        cols: u16,
        rows: u16,
        command: Option<(&str, &[String])>,
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> Result<Self> {
        let shell = Self::get_default_shell();
        let (program, args) = command.unwrap_or((shell.as_str(), &[]));
        Self::spawn_env(cols, rows, program, args, cwd, env)
    }

    fn spawn(
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
    ) -> Result<Self> {
        Self::spawn_env(cols, rows, program, args, cwd, &[])
    }

    fn spawn_env(
        cols: u16,
        rows: u16,
        program: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Falha ao criar par PTY")?;

        info!(program, args = ?args, "Iniciando comando no PTY");

        let mut cmd = CommandBuilder::new(program);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "code-terminal");
        for (key, value) in env {
            cmd.env(key, value);
        }
        for arg in args {
            cmd.arg(arg);
        }
        if let Some(dir) = Self::initial_dir(cwd) {
            cmd.cwd(dir);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .with_context(|| format!("Falha ao iniciar comando no PTY: {}", program))?;

        let pid = child.process_id();
        info!(pid = ?pid, program, "Processo iniciado no PTY");
        drop(pair.slave);

        let (output_tx, output_rx): (Sender<PtyChunk>, Receiver<PtyChunk>) = mpsc::channel();
        let (exit_tx, exit_rx): (Sender<ProcessStatus>, Receiver<ProcessStatus>) = mpsc::channel();
        let running = Arc::new(AtomicBool::new(true));

        // No Linux a leitura e a escrita usam cópias do descritor: a leitura
        // pode parar (para entregar o PTY ao daemon) e fechar o escritor não
        // manda `\n` + EOF ao shell (o que o escritor do portable-pty faz).
        #[cfg(target_os = "linux")]
        let (writer, reader): (Box<dyn Write + Send>, _) = {
            let fd = pair.master.as_raw_fd().context("PTY sem descritor")?;
            let writer = Box::new(dup_fd(fd).context("Falha ao duplicar o PTY")?);
            let reader = StoppableReader::spawn(
                dup_fd(fd).context("Falha ao duplicar o PTY")?,
                output_tx.clone(),
                pid,
            )
            .context("Falha ao iniciar a leitura do PTY")?;
            (writer, Some(reader))
        };
        #[cfg(not(target_os = "linux"))]
        let writer = {
            let reader = pair
                .master
                .try_clone_reader()
                .context("Falha ao clonar reader do PTY")?;
            spawn_blocking_reader(reader, output_tx.clone(), Arc::clone(&running), pid);
            pair.master
                .take_writer()
                .context("Falha ao obter writer do PTY")?
        };

        Ok(Self {
            backend: Backend::Local {
                master: Mutex::new(pair.master),
                child: Arc::new(Mutex::new(child)),
                #[cfg(target_os = "linux")]
                reader,
            },
            writer: Mutex::new(writer),
            output_tx,
            output_rx: Mutex::new(output_rx),
            pid,
            running,
            exit_tx,
            exit_rx: Mutex::new(exit_rx),
        })
    }
}

/// Leitura bloqueante numa thread (sistemas sem a leitura com parada).
#[cfg(not(target_os = "linux"))]
fn spawn_blocking_reader(
    mut reader: Box<dyn Read + Send>,
    output_tx: Sender<PtyChunk>,
    running_for_thread: Arc<AtomicBool>,
    pid_for_thread: Option<u32>,
) {
    let spawned = thread::Builder::new()
        .name("pty-reader".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                if !running_for_thread.load(Ordering::Relaxed) {
                    debug!("Thread de leitura encerrada (flag running=false)");
                    break;
                }

                match reader.read(&mut buf) {
                    Ok(0) => {
                        debug!("PTY EOF recebido");
                        break;
                    }
                    Ok(n) => {
                        let chunk = PtyChunk {
                            data: buf[..n].to_vec(),
                            replay: false,
                        };
                        if output_tx.send(chunk).is_err() {
                            debug!("Canal de output fechado");
                            break;
                        }
                        wake_output_listener();
                    }
                    Err(e) => {
                        error!(error = %e, pid = ?pid_for_thread, "Erro na leitura do PTY");
                        break;
                    }
                }
            }
            // Acorda a UI também no fim: ela precisa ver que o processo saiu.
            wake_output_listener();
            debug!("Thread de leitura do PTY encerrada");
        });
    if let Err(e) = spawned {
        error!(error = %e, "Failed to start the PTY reader");
    }
}

/// Cópia de um descritor como `File` próprio.
#[cfg(target_os = "linux")]
fn dup_fd(fd: std::os::fd::RawFd) -> std::io::Result<std::fs::File> {
    // SAFETY: `fd` é o descritor aberto do PTY, vivo enquanto o master
    // existir; o `BorrowedFd` só é usado para duplicá-lo.
    let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    Ok(std::fs::File::from(borrowed.try_clone_to_owned()?))
}

/// Thread que lê o PTY e pode ser parada: espera com `poll` no PTY e num
/// socket de aviso, em vez de ficar presa num `read`.
#[cfg(target_os = "linux")]
struct StoppableReader {
    stop: std::os::unix::net::UnixStream,
    thread: Option<thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl StoppableReader {
    fn spawn(
        mut file: std::fs::File,
        output_tx: Sender<PtyChunk>,
        pid: Option<u32>,
    ) -> std::io::Result<Self> {
        use std::os::fd::AsRawFd;
        let (stop, wake) = std::os::unix::net::UnixStream::pair()?;
        let thread = thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                let mut fds = [
                    libc::pollfd {
                        fd: file.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                    libc::pollfd {
                        fd: wake.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    },
                ];
                loop {
                    // SAFETY: `fds` é um vetor válido de 2 pollfd que vive
                    // durante a chamada.
                    let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
                    if ready < 0 {
                        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
                        {
                            continue;
                        }
                        break;
                    }
                    if fds[1].revents != 0 {
                        debug!("Leitura do PTY parada");
                        return;
                    }
                    if fds[0].revents == 0 {
                        continue;
                    }
                    match file.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let chunk = PtyChunk {
                                data: buf[..n].to_vec(),
                                replay: false,
                            };
                            if output_tx.send(chunk).is_err() {
                                break;
                            }
                            wake_output_listener();
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(e) => {
                            // EIO: o programa saiu (o lado dele do PTY fechou).
                            debug!(error = %e, ?pid, "PTY fechado");
                            break;
                        }
                    }
                }
                // Acorda a UI também no fim: ela precisa ver que o processo saiu.
                wake_output_listener();
            })?;
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Para a leitura e espera a thread sair (nenhum byte do PTY é lido
    /// depois disto).
    fn stop(mut self) {
        let _ = (&self.stop).write_all(&[1]);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Pty {
    /// Programa em primeiro plano que não é o próprio shell (ex.: `vim`, um
    /// build). Usado para pedir confirmação só quando fechar mataria algo.
    #[cfg(unix)]
    pub fn foreground_process(&self) -> Option<String> {
        let shell = self.pid?;
        let master = match &self.backend {
            Backend::Local { master, .. } => master,
            Backend::Remote(_) => return super::remote_pty::foreground_of(shell),
        };
        let leader = master.lock().ok()?.process_group_leader()?;
        if leader <= 0 || leader as u32 == shell {
            return None;
        }
        let name = std::fs::read_to_string(format!("/proc/{leader}/comm"))
            .map(|name| name.trim().to_string())
            .unwrap_or_else(|_| format!("pid {leader}"));
        Some(name)
    }

    /// Programa em primeiro plano (não suportado nesta plataforma).
    #[cfg(not(unix))]
    pub fn foreground_process(&self) -> Option<String> {
        None
    }

    /// Diretório atual do processo (para abrir a tab nova no mesmo lugar).
    #[cfg(target_os = "linux")]
    pub fn current_dir(&self) -> Option<PathBuf> {
        let pid = self.pid()?;
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }

    /// Diretório atual do processo (não suportado nesta plataforma).
    #[cfg(not(target_os = "linux"))]
    pub fn current_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Retorna o shell padrão do sistema
    fn get_default_shell() -> String {
        #[cfg(target_os = "windows")]
        {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        }

        #[cfg(not(target_os = "windows"))]
        {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        }
    }

    /// Retorna o PID do processo filho
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Verifica se o processo ainda está rodando
    pub fn is_running(&self) -> bool {
        // Primeiro verifica a flag
        if !self.running.load(Ordering::Relaxed) {
            return false;
        }

        // Verifica o estado real do processo
        match self.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => {
                self.running.store(false, Ordering::Relaxed);
                false
            }
            Err(_) => {
                // Em caso de erro, assume que não está rodando
                self.running.store(false, Ordering::Relaxed);
                false
            }
        }
    }

    /// Tenta obter o status de saída sem bloquear
    pub fn try_wait(&self) -> Result<Option<ProcessStatus>> {
        let child = match &self.backend {
            Backend::Local { child, .. } => child,
            #[cfg(unix)]
            Backend::Remote(link) => {
                if self.running.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                let status = ProcessStatus::Exited(link.exit_code().unwrap_or(0).max(0));
                return Ok(Some(status));
            }
        };
        let mut child = child.lock().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        match child.try_wait()? {
            Some(status) => {
                let process_status = Self::convert_exit_status(&status);
                drop(child); // Libera o lock antes de modificar running
                self.running.store(false, Ordering::Relaxed);
                self.emit_exit_status(process_status);
                Ok(Some(process_status))
            }
            None => Ok(None),
        }
    }

    /// Aguarda o processo terminar (bloqueante)
    pub fn wait(&self) -> Result<ProcessStatus> {
        let child = match &self.backend {
            Backend::Local { child, .. } => child,
            #[cfg(unix)]
            Backend::Remote(_) => loop {
                if let Some(status) = self.try_wait()? {
                    return Ok(status);
                }
                thread::sleep(Duration::from_millis(20));
            },
        };
        let mut child = child.lock().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        let status = child.wait()?;
        let process_status = Self::convert_exit_status(&status);
        drop(child); // Libera o lock antes de modificar running
        self.running.store(false, Ordering::Relaxed);
        self.emit_exit_status(process_status);
        Ok(process_status)
    }

    fn emit_exit_status(&self, process_status: ProcessStatus) {
        if let Err(error) = self.exit_tx.send(process_status) {
            tracing::debug!(
                error = %error,
                "PTY process exited but no exit status receiver is available"
            );
        }
    }

    /// Aguarda o processo terminar com timeout
    pub fn wait_timeout(&self, timeout: Duration) -> Result<Option<ProcessStatus>> {
        let deadline = std::time::Instant::now() + timeout;

        while std::time::Instant::now() < deadline {
            if let Some(status) = self.try_wait()? {
                return Ok(Some(status));
            }
            thread::sleep(Duration::from_millis(10));
        }

        Ok(None)
    }

    /// Converte o status de saída do portable-pty para nosso tipo
    fn convert_exit_status(status: &portable_pty::ExitStatus) -> ProcessStatus {
        // O ExitStatus do portable-pty tem métodos específicos
        // Usamos success() para verificar e mapeamos o resultado
        if status.success() {
            ProcessStatus::Exited(0)
        } else {
            // O código real (antes era sempre 1: "exit 3" virava Exited(1)).
            // O portable-pty não expõe o sinal; morte por sinal sai com 1.
            ProcessStatus::Exited(i32::try_from(status.exit_code()).unwrap_or(i32::MAX))
        }
    }

    /// Envia sinal SIGTERM (ou equivalente) para o processo
    #[cfg(unix)]
    pub fn terminate(&self) -> Result<()> {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        if let Some(pid) = self.pid {
            info!(pid, "Enviando SIGTERM para processo");
            kill(Pid::from_raw(pid as i32), Signal::SIGTERM).context("Falha ao enviar SIGTERM")?;
            Ok(())
        } else {
            anyhow::bail!("PID não disponível")
        }
    }

    /// Avisa o processo que o terminal fechou (SIGHUP no Unix).
    #[cfg(unix)]
    pub fn hang_up(&self) -> Result<()> {
        self.signal(nix::sys::signal::Signal::SIGHUP as i32)
    }

    /// Avisa o processo que o terminal fechou (no Windows, encerra).
    #[cfg(not(unix))]
    pub fn hang_up(&self) -> Result<()> {
        self.terminate()
    }

    /// Envia sinal SIGKILL (ou equivalente) para o processo
    #[cfg(unix)]
    pub fn kill(&self) -> Result<()> {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        if let Some(pid) = self.pid {
            info!(pid, "Enviando SIGKILL para processo");
            kill(Pid::from_raw(pid as i32), Signal::SIGKILL).context("Falha ao enviar SIGKILL")?;
            Ok(())
        } else {
            anyhow::bail!("PID não disponível")
        }
    }

    /// Envia sinal específico para o processo
    #[cfg(unix)]
    pub fn signal(&self, signal: i32) -> Result<()> {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        if let Some(pid) = self.pid {
            let sig = Signal::try_from(signal)
                .map_err(|_| anyhow::anyhow!("Sinal inválido: {}", signal))?;
            info!(pid, signal = signal, "Enviando sinal para processo");
            kill(Pid::from_raw(pid as i32), sig).context("Falha ao enviar sinal")?;
            Ok(())
        } else {
            anyhow::bail!("PID não disponível")
        }
    }

    /// Termina o processo (Windows - usa taskkill)
    #[cfg(windows)]
    pub fn terminate(&self) -> Result<()> {
        if let Some(pid) = self.pid {
            info!(pid, "Terminando processo Windows");
            let output = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string()])
                .output()
                .context("Falha ao executar taskkill")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("taskkill falhou: {}", stderr);
            }
            Ok(())
        } else {
            anyhow::bail!("PID não disponível")
        }
    }

    /// Mata o processo forçadamente (Windows)
    #[cfg(windows)]
    pub fn kill(&self) -> Result<()> {
        if let Some(pid) = self.pid {
            info!(pid, "Matando processo Windows (forçado)");
            let output = std::process::Command::new("taskkill")
                .args(["/F", "/PID", &pid.to_string()])
                .output()
                .context("Falha ao executar taskkill /F")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("taskkill /F falhou: {}", stderr);
            }
            Ok(())
        } else {
            anyhow::bail!("PID não disponível")
        }
    }

    /// Envia sinal específico (Windows - não suportado nativamente)
    #[cfg(windows)]
    pub fn signal(&self, _signal: i32) -> Result<()> {
        // Windows não tem sinais POSIX, mapeamos alguns comuns
        match _signal {
            15 => self.terminate(), // SIGTERM
            9 => self.kill(),       // SIGKILL
            _ => anyhow::bail!("Sinal {} não suportado no Windows", _signal),
        }
    }

    /// Envia dados para o PTY
    pub fn write(&self, data: &[u8]) -> Result<()> {
        if !self.is_running() {
            anyhow::bail!("PTY não está rodando");
        }
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Envia string para o PTY
    pub fn write_str(&self, s: &str) -> Result<()> {
        self.write(s.as_bytes())
    }

    /// Envia Ctrl+C (SIGINT)
    pub fn send_interrupt(&self) -> Result<()> {
        // Ctrl+C é enviado como byte 0x03 (ETX - End of Text)
        self.write(&[0x03])
    }

    /// Envia Ctrl+D (EOF)
    pub fn send_eof(&self) -> Result<()> {
        // Ctrl+D é enviado como byte 0x04 (EOT - End of Transmission)
        self.write(&[0x04])
    }

    /// Envia Ctrl+Z (suspend)
    #[cfg(unix)]
    pub fn send_suspend(&self) -> Result<()> {
        // Ctrl+Z é enviado como byte 0x1A (SUB - Substitute)
        self.write(&[0x1A])
    }

    /// Tenta ler output do PTY (não bloqueante)
    pub fn try_read(&self) -> Option<Vec<u8>> {
        self.try_read_chunk().map(|chunk| chunk.data)
    }

    /// Como [`Pty::try_read`], dizendo se o pedaço é histórico reenviado.
    pub fn try_read_chunk(&self) -> Option<PtyChunk> {
        self.output_rx.lock().ok()?.try_recv().ok()
    }

    /// Redimensiona o PTY
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        if !self.is_running() {
            anyhow::bail!("PTY não está rodando");
        }

        let master = match &self.backend {
            Backend::Local { master, .. } => master,
            #[cfg(unix)]
            Backend::Remote(link) => {
                link.resize(cols, rows)
                    .context("Falha ao redimensionar a tab persistente")?;
                return Ok(());
            }
        };
        master
            .lock()
            .map_err(|_| anyhow::anyhow!("Lock poisoned"))?
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Falha ao redimensionar PTY")?;
        debug!(cols, rows, "PTY redimensionado");
        Ok(())
    }

    /// Verifica se houve notificação de término
    pub fn check_exit_notification(&self) -> Option<ProcessStatus> {
        self.exit_rx.lock().ok()?.try_recv().ok()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        info!("Limpando PTY");
        #[cfg(unix)]
        if let Backend::Remote(link) = &mut self.backend {
            // Tab persistente: só desanexa, o processo segue no daemon.
            link.detach();
            return;
        }

        // Fecha como um terminal de verdade: SIGHUP (o bash interativo ignora
        // SIGTERM, e fechar uma tab travava a UI por 500 ms esperando à toa).
        if self.is_running() {
            info!("Encerrando processo do PTY");
            if let Err(e) = self.hang_up() {
                warn!(error = %e, "Falha ao enviar hangup, tentando kill");
            }
            if matches!(self.wait_timeout(Duration::from_millis(100)), Ok(None)) {
                if let Err(e) = self.kill() {
                    error!(error = %e, "Falha ao matar processo");
                }
            }
        }

        self.running.store(false, Ordering::Relaxed);

        match self.writer.lock() {
            Ok(mut writer) => {
                if let Err(e) = writer.flush() {
                    warn!(error = %e, "Falha ao flush writer no drop");
                }
            }
            Err(_) => {
                warn!("Lock do writer do PTY envenenado durante drop");
            }
        }

        info!("Limpeza do PTY completa");
    }
}

/// Tabs persistentes: o PTY no daemon do multiplexer.
#[cfg(unix)]
impl Pty {
    /// Abre a sessão `session` do daemon (criando-a com `create`, e subindo
    /// o daemon se preciso). A saída começa pelo histórico da sessão.
    pub fn connect_persistent(
        session: &str,
        cols: u16,
        rows: u16,
        create: Option<crate::multiplexer::protocol::StreamCreate>,
    ) -> Result<Self> {
        use crate::multiplexer::app_client;
        if create.is_some() {
            app_client::ensure_daemon()
                .context("Falha ao iniciar o daemon das tabs persistentes")?;
        }
        let opened = app_client::open_stream(session, (cols, rows), create, true, None)
            .with_context(|| format!("Falha ao abrir a tab persistente {session}"))?;
        let pid = opened.pid;
        let (output_tx, output_rx) = mpsc::channel();
        let (exit_tx, exit_rx) = mpsc::channel();
        let running = Arc::new(AtomicBool::new(true));
        let (link, writer) = super::remote_pty::RemoteLink::start(
            opened,
            output_tx.clone(),
            Arc::clone(&running),
            wake_output_listener,
        )?;
        // A sessão pode ter outro tamanho (outra janela): o programa redesenha
        // no tamanho desta tab.
        let _ = link.resize(cols, rows);
        info!(session, ?pid, "Tab persistente anexada");
        Ok(Self {
            backend: Backend::Remote(link),
            writer: Mutex::new(Box::new(writer)),
            output_tx,
            output_rx: Mutex::new(output_rx),
            pid,
            running,
            exit_tx,
            exit_rx: Mutex::new(exit_rx),
        })
    }

    /// Sessão do daemon desta tab, se ela for persistente.
    pub fn persistent_session(&self) -> Option<&str> {
        match &self.backend {
            Backend::Remote(link) => Some(link.session()),
            Backend::Local { .. } => None,
        }
    }

    /// Encerra a sessão persistente (o programa recebe SIGHUP).
    pub fn end_session(&self) -> Result<()> {
        match &self.backend {
            Backend::Remote(link) => link
                .end_session()
                .context("Falha ao encerrar a tab persistente"),
            Backend::Local { .. } => self.hang_up(),
        }
    }

    /// Ao fechar a tab, encerrar a sessão em vez de só desanexar.
    pub fn set_end_on_drop(&self, end: bool) {
        if let Backend::Remote(link) = &self.backend {
            link.set_end_on_drop(end);
        }
    }

    /// Entrega ao daemon o PTY desta tab, que já está rodando, sem matar o
    /// processo: a leitura daqui para, o descritor vai pelo socket junto com
    /// `seed` (a tela atual, para quem anexar depois) e a tab passa a
    /// conversar com o daemon. Se o daemon recusar, a tab segue como estava.
    #[cfg(target_os = "linux")]
    pub fn make_persistent(&mut self, session: &str, seed: &[u8], meta: String) -> Result<()> {
        use crate::multiplexer::app_client;
        use crate::multiplexer::protocol::StreamCreate;
        let cwd = self.current_dir().map(|dir| dir.display().to_string());
        let pid = self.pid;
        let Backend::Local { master, reader, .. } = &mut self.backend else {
            anyhow::bail!("a tab já é persistente");
        };
        let (fd, size) = {
            let master = master
                .get_mut()
                .map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
            let fd = master.as_raw_fd().context("PTY sem descritor")?;
            let size = master.get_size().unwrap_or_default();
            (fd, (size.cols, size.rows))
        };
        if let Some(reader) = reader.take() {
            reader.stop();
        }
        let spec = StreamCreate {
            cwd,
            adopt_pid: pid,
            meta,
            ..Default::default()
        };
        let opened = app_client::ensure_daemon().and_then(|()| {
            app_client::open_stream(session, size, Some(spec), false, Some((fd, seed)))
        });
        let opened = match opened {
            Ok(opened) => opened,
            Err(e) => {
                // Volta a ler daqui: nada mudou para o processo.
                *reader = Some(StoppableReader::spawn(
                    dup_fd(fd)?,
                    self.output_tx.clone(),
                    pid,
                )?);
                return Err(anyhow::Error::from(e).context("O daemon não adotou a tab"));
            }
        };
        let (link, writer) = super::remote_pty::RemoteLink::start(
            opened,
            self.output_tx.clone(),
            Arc::clone(&self.running),
            wake_output_listener,
        )?;
        // Daqui em diante o PTY é do daemon: o master e o escritor daqui
        // fecham, o shell continua filho deste processo (é colhido depois).
        let old = std::mem::replace(&mut self.backend, Backend::Remote(link));
        if let (Backend::Remote(link), Backend::Local { child, .. }) = (&mut self.backend, old) {
            if let Ok(child) = Arc::try_unwrap(child) {
                if let Ok(child) = child.into_inner() {
                    link.set_adopted_child(child);
                }
            }
        }
        if let Ok(mut current) = self.writer.lock() {
            *current = Box::new(writer);
        }
        info!(session, ?pid, "Tab entregue ao daemon (persistente)");
        Ok(())
    }

    /// Entregar uma tab que já roda ao daemon (não suportado aqui: só tabs
    /// novas podem ser persistentes).
    #[cfg(not(target_os = "linux"))]
    pub fn make_persistent(&mut self, _session: &str, _seed: &[u8], _meta: String) -> Result<()> {
        anyhow::bail!("tornar persistente uma tab que já roda só funciona no Linux")
    }
}

/// Sem daemon do multiplexer neste sistema: nenhuma tab é persistente.
#[cfg(not(unix))]
impl Pty {
    /// Sessão do daemon desta tab (nunca, aqui).
    pub fn persistent_session(&self) -> Option<&str> {
        None
    }

    /// Encerra o processo (aqui toda tab é local).
    pub fn end_session(&self) -> Result<()> {
        self.hang_up()
    }

    /// Sem efeito: toda tab daqui já morre ao fechar.
    pub fn set_end_on_drop(&self, _end: bool) {}

    /// Tabs persistentes ainda não existem neste sistema.
    pub fn make_persistent(&mut self, _session: &str, _seed: &[u8], _meta: String) -> Result<()> {
        anyhow::bail!("persistent tabs are not available on this system yet")
    }
}

#[cfg(test)]
mod tests {

    /// Regressão: fechar uma tab com bash interativo esperava 500 ms (o bash
    /// Regressão: sem `cwd` o portable-pty abria tudo no `$HOME`.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_spawn_starts_in_requested_dir_and_reports_it() {
        let dir = tempfile::tempdir().unwrap();
        let pty = Pty::spawn(80, 24, "sleep", &["5".to_string()], Some(dir.path())).unwrap();
        let reported = pty.current_dir().unwrap();
        assert_eq!(
            reported.canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
    }

    /// Shell parado não conta como "processo rodando"; um filho em primeiro
    /// plano conta.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_foreground_process_ignores_idle_shell() {
        let pty = Pty::spawn(80, 24, "/bin/bash", &["--norc".into(), "-i".into()], None).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(pty.foreground_process(), None);
        pty.write_str("sleep 5\n").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut found = None;
        while found.is_none() && std::time::Instant::now() < deadline {
            found = pty.foreground_process();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(found.as_deref(), Some("sleep"));
    }

    #[test]
    fn test_initial_dir_defaults_to_current_dir() {
        assert_eq!(Pty::initial_dir(None), std::env::current_dir().ok());
        let missing = Path::new("/definitely/not/here");
        assert_eq!(Pty::initial_dir(Some(missing)), None);
    }

    /// ignora SIGTERM). Com SIGHUP o drop termina rápido e o shell sai.
    #[cfg(unix)]
    #[test]
    fn test_drop_hangs_up_interactive_shell_quickly() {
        let pty = Pty::with_command(80, 24, "/bin/bash", &["-i".into()]).unwrap();
        let pid = pty.pid().unwrap() as i32;
        thread::sleep(Duration::from_millis(200));
        let started = std::time::Instant::now();
        drop(pty);
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "{:?}",
            started.elapsed()
        );
        thread::sleep(Duration::from_millis(100));
        let alive = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
            && std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .map(|stat| !stat.contains(") Z "))
                .unwrap_or(false);
        assert!(!alive, "bash continuou vivo depois do drop");
    }

    /// O despertador é chamado quando o PTY escreve algo.
    #[cfg(unix)]
    #[test]
    fn test_output_waker_fires_on_output() {
        use std::sync::atomic::AtomicUsize;
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        set_output_waker(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });
        let pty = Pty::with_command(80, 24, "/bin/sh", &["-c".into(), "echo wake".into()]).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while hits.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        clear_output_waker();
        assert!(hits.load(Ordering::SeqCst) > 0);
        drop(pty);
    }

    use super::*;

    #[test]
    fn test_process_status_display() {
        assert_eq!(ProcessStatus::Running.to_string(), "running");
        assert_eq!(ProcessStatus::Exited(0).to_string(), "exited(0)");
        assert_eq!(ProcessStatus::Signaled(9).to_string(), "signaled(9)");
        assert_eq!(ProcessStatus::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_pty_creation() {
        let pty = Pty::new(80, 24);
        assert!(pty.is_ok());

        let pty = pty.unwrap();
        assert!(pty.pid().is_some());
        assert!(pty.is_running());
    }

    #[test]
    fn test_pty_write_read() {
        let pty = Pty::new(80, 24).unwrap();

        // Envia um comando simples
        pty.write_str("echo teste123\n").unwrap();

        // Aguarda um pouco para o comando executar
        thread::sleep(Duration::from_millis(100));

        // Lê o output
        let mut output = String::new();
        for _ in 0..10 {
            if let Some(data) = pty.try_read() {
                output.push_str(&String::from_utf8_lossy(&data));
                if output.contains("teste123") {
                    break;
                }
            }
            thread::sleep(Duration::from_millis(50));
        }

        assert!(
            output.contains("teste123"),
            "Output não contém 'teste123': {}",
            output
        );
    }

    #[test]
    fn test_pty_resize() {
        let pty = Pty::new(80, 24).unwrap();
        assert!(pty.resize(100, 30).is_ok());
    }

    #[test]
    fn test_send_interrupt() {
        let pty = Pty::new(80, 24).unwrap();
        // Apenas verifica se não panic
        assert!(pty.send_interrupt().is_ok());
    }

    #[cfg(unix)]
    fn spawn(program: &str, args: &[&str]) -> Pty {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        Pty::with_command(80, 24, program, &args).expect("spawn")
    }

    /// Regressão: qualquer saída diferente de 0 virava Exited(1).
    #[test]
    fn test_convert_exit_status_keeps_exit_code() {
        use portable_pty::ExitStatus;
        assert_eq!(
            Pty::convert_exit_status(&ExitStatus::with_exit_code(0)),
            ProcessStatus::Exited(0)
        );
        assert_eq!(
            Pty::convert_exit_status(&ExitStatus::with_exit_code(3)),
            ProcessStatus::Exited(3)
        );
        assert_eq!(
            Pty::convert_exit_status(&ExitStatus::with_signal("Killed")),
            ProcessStatus::Exited(1)
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_wait_reports_exit_code_and_notifies() {
        let pty = spawn("sh", &["-c", "exit 3"]);
        assert!(format!("{:?}", pty).contains("Pty"));
        assert_eq!(pty.wait().unwrap(), ProcessStatus::Exited(3));
        assert!(!pty.is_running());
        assert_eq!(
            pty.check_exit_notification(),
            Some(ProcessStatus::Exited(3))
        );
        assert_eq!(pty.check_exit_notification(), None);
        assert!(
            pty.write(b"x").is_err(),
            "escrever em processo encerrado falha"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_signals_and_kill() {
        let pty = spawn("sleep", &["30"]);
        assert!(pty.is_running());
        assert!(pty.signal(9999).is_err(), "sinal inválido");
        // Ctrl+Z (SIGTSTP) para o processo; SIGKILL mata mesmo parado
        pty.send_suspend().unwrap();
        pty.kill().unwrap();
        let status = pty
            .wait_timeout(Duration::from_secs(10))
            .unwrap()
            .expect("processo morto deveria terminar");
        assert_ne!(status, ProcessStatus::Exited(0));

        let pty = spawn("sleep", &["30"]);
        pty.terminate().unwrap();
        assert!(pty.wait_timeout(Duration::from_secs(10)).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn test_send_eof_ends_cat() {
        let pty = spawn("cat", &[]);
        pty.send_eof().unwrap();
        let status = pty
            .wait_timeout(Duration::from_secs(10))
            .unwrap()
            .expect("cat deveria sair no EOF");
        assert_eq!(status, ProcessStatus::Exited(0));
    }
}
