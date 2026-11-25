//! PTY de uma tab persistente: o processo vive no daemon do multiplexer e a
//! tab conversa com ele pelo socket (quadros binários, ver
//! `multiplexer::stream`).

use super::pty::PtyChunk;
use crate::multiplexer::app_client::OpenedStream;
use crate::multiplexer::stream::{
    read_frame, resize_payload, write_frame, FRAME_EXITED, FRAME_INPUT, FRAME_KILL, FRAME_OUTPUT,
    FRAME_REPLAY_DONE, FRAME_RESIZE,
};
use portable_pty::Child;
use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// Ponta do app de uma sessão do daemon.
pub struct RemoteLink {
    session: String,
    stream: Arc<Mutex<UnixStream>>,
    /// Código de saída, quando o daemon avisa que o programa saiu.
    exit: Arc<Mutex<Option<i32>>>,
    /// Shell que nasceu neste processo e foi adotado pelo daemon: continua
    /// filho daqui e precisa ser colhido (senão vira zumbi).
    adopted_child: Option<Box<dyn Child + Send + Sync>>,
    /// Encerrar a sessão quando a tab sair (em vez de só desanexar).
    end_on_drop: AtomicBool,
}

impl std::fmt::Debug for RemoteLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteLink")
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

/// Escreve o que a tab digita como quadros de entrada.
pub struct FrameWriter(Arc<Mutex<UnixStream>>);

impl Write for FrameWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut stream = self
            .0
            .lock()
            .map_err(|_| io::Error::other("stream lock poisoned"))?;
        write_frame(&mut *stream, FRAME_INPUT, buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl RemoteLink {
    /// Liga o fluxo aberto: uma thread lê os quadros do daemon e põe a saída
    /// em `output` (o histórico do anexo marcado como `replay`), chamando
    /// `wake` a cada pedaço. Devolve a ponta e o escritor da entrada.
    pub fn start(
        opened: OpenedStream,
        output: Sender<PtyChunk>,
        running: Arc<AtomicBool>,
        wake: fn(),
    ) -> io::Result<(Self, FrameWriter)> {
        let OpenedStream {
            writer,
            mut reader,
            name,
            ..
        } = opened;
        let stream = Arc::new(Mutex::new(writer));
        let exit = Arc::new(Mutex::new(None));
        let exit_for_thread = Arc::clone(&exit);
        std::thread::Builder::new()
            .name(format!("pty-remote-{name}"))
            .spawn(move || {
                let mut replaying = true;
                // Erro de leitura: o daemon caiu, para a tab é como o
                // processo sair.
                while let Ok((kind, payload)) = read_frame(&mut reader) {
                    match kind {
                        FRAME_OUTPUT => {
                            let chunk = PtyChunk {
                                data: payload,
                                replay: replaying,
                            };
                            if output.send(chunk).is_err() {
                                break;
                            }
                            wake();
                        }
                        FRAME_REPLAY_DONE => replaying = false,
                        FRAME_EXITED => {
                            let code = payload
                                .get(..4)
                                .and_then(|b| b.try_into().ok())
                                .map(i32::from_le_bytes)
                                .unwrap_or(-1);
                            if let Ok(mut exit) = exit_for_thread.lock() {
                                *exit = Some(code);
                            }
                            break;
                        }
                        _ => {}
                    }
                }
                running.store(false, Ordering::Relaxed);
                wake();
            })?;
        let writer = FrameWriter(Arc::clone(&stream));
        Ok((
            Self {
                session: name,
                stream,
                exit,
                adopted_child: None,
                end_on_drop: AtomicBool::new(false),
            },
            writer,
        ))
    }

    /// Guarda o shell adotado (para colher quando ele sair).
    pub fn set_adopted_child(&mut self, child: Box<dyn Child + Send + Sync>) {
        self.adopted_child = Some(child);
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    fn send(&self, kind: u8, payload: &[u8]) -> io::Result<()> {
        let mut stream = self
            .stream
            .lock()
            .map_err(|_| io::Error::other("stream lock poisoned"))?;
        write_frame(&mut *stream, kind, payload)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        self.send(FRAME_RESIZE, &resize_payload(cols, rows))
    }

    /// Encerra a sessão no daemon (o programa recebe SIGHUP).
    pub fn end_session(&self) -> io::Result<()> {
        self.send(FRAME_KILL, &[])
    }

    /// Ao sair da tab, encerrar a sessão em vez de só desanexar.
    pub fn set_end_on_drop(&self, end: bool) {
        self.end_on_drop.store(end, Ordering::Relaxed);
    }

    /// Código de saída, se o programa já saiu.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit.lock().ok().and_then(|exit| *exit)
    }

    /// A tab saiu: desanexa (ou encerra, se pedido). O shell adotado ganha
    /// uma thread que só espera ele sair, para não virar zumbi.
    pub fn detach(&mut self) {
        if self.end_on_drop.load(Ordering::Relaxed) {
            let _ = self.end_session();
        }
        if let Ok(stream) = self.stream.lock() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        if let Some(mut child) = self.adopted_child.take() {
            let spawned = std::thread::Builder::new()
                .name("pty-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                });
            if let Err(e) = spawned {
                tracing::warn!(error = %e, "Failed to spawn reaper thread");
            }
        }
    }
}

/// Programa em primeiro plano no terminal do shell `pid` (lido do `/proc`:
/// a tab persistente não tem o descritor do PTY).
#[cfg(target_os = "linux")]
pub fn foreground_of(pid: u32) -> Option<String> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.rsplit_once(')')?;
    // estado, ppid, pgrp, sid, tty_nr, tpgid
    let tpgid: i64 = rest.split_whitespace().nth(5)?.parse().ok()?;
    if tpgid <= 0 || tpgid == i64::from(pid) {
        return None;
    }
    let name = std::fs::read_to_string(format!("/proc/{tpgid}/comm"))
        .map(|name| name.trim().to_string())
        .unwrap_or_else(|_| format!("pid {tpgid}"));
    Some(name)
}

#[cfg(not(target_os = "linux"))]
pub fn foreground_of(_pid: u32) -> Option<String> {
    None
}
