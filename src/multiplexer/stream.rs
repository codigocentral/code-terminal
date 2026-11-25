//! Conexão de fluxo entre o app e o daemon (tabs persistentes).
//!
//! O app abre uma sessão com uma linha JSON ([`MessageType::OpenStream`]) e,
//! depois da resposta, a conexão passa a carregar quadros binários:
//! `[tipo: u8][tamanho: u32 LE][dados]`. Sem JSON nem varredura periódica no
//! caminho da tecla: cada byte digitado vira um quadro que o daemon escreve
//! no PTY na hora, e cada leitura do PTY vira um quadro para a janela.
//!
//! Também fica aqui a passagem do descritor do PTY (`SCM_RIGHTS`), usada para
//! tornar persistente uma tab que já está rodando sem matar o shell dela.
//!
//! [`MessageType::OpenStream`]: crate::multiplexer::protocol::MessageType::OpenStream

use std::io::{self, IoSlice, IoSliceMut, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

/// Bytes digitados (app → daemon).
pub const FRAME_INPUT: u8 = 1;
/// Novo tamanho: `cols: u16 LE`, `rows: u16 LE` (app → daemon).
pub const FRAME_RESIZE: u8 = 2;
/// Encerrar a sessão de vez (app → daemon).
pub const FRAME_KILL: u8 = 3;

/// Saída do programa (daemon → app).
pub const FRAME_OUTPUT: u8 = 1;
/// Fim do histórico reenviado no anexo; o que vem depois é saída nova.
pub const FRAME_REPLAY_DONE: u8 = 2;
/// O programa saiu: código `i32 LE` (`-1` se o daemon não souber).
pub const FRAME_EXITED: u8 = 3;

/// Maior quadro aceito (o histórico vai em pedaços bem menores que isso).
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// Pedaço do histórico mandado por quadro no anexo.
pub const REPLAY_CHUNK: usize = 64 * 1024;

/// Escreve um quadro inteiro (cabeçalho e dados num `write_all` só).
pub fn write_frame(out: &mut impl Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(kind);
    frame.extend_from_slice(&len.to_le_bytes());
    frame.extend_from_slice(payload);
    out.write_all(&frame)
}

/// Lê um quadro; `UnexpectedEof` quando a outra ponta fechou.
pub fn read_frame(input: &mut impl Read) -> io::Result<(u8, Vec<u8>)> {
    let mut header = [0u8; 5];
    input.read_exact(&mut header)?;
    let len = u32::from_le_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {len} bytes"),
        ));
    }
    let mut payload = vec![0u8; len];
    input.read_exact(&mut payload)?;
    Ok((header[0], payload))
}

/// Quadro de tamanho.
pub fn resize_payload(cols: u16, rows: u16) -> [u8; 4] {
    let (c, r) = (cols.to_le_bytes(), rows.to_le_bytes());
    [c[0], c[1], r[0], r[1]]
}

/// Lê o quadro de tamanho.
pub fn parse_resize(payload: &[u8]) -> Option<(u16, u16)> {
    let bytes: [u8; 4] = payload.get(..4)?.try_into().ok()?;
    Some((
        u16::from_le_bytes([bytes[0], bytes[1]]),
        u16::from_le_bytes([bytes[2], bytes[3]]),
    ))
}

/// Manda `data` levando junto o descritor `fd` (o outro processo recebe uma
/// cópia dele).
pub fn send_with_fd(stream: &UnixStream, data: &[u8], fd: RawFd) -> io::Result<()> {
    use nix::sys::socket::{sendmsg, ControlMessage, MsgFlags};
    let fds = [fd];
    let cmsg = [ControlMessage::ScmRights(&fds)];
    let mut sent = sendmsg::<()>(
        stream.as_raw_fd(),
        &[IoSlice::new(data)],
        &cmsg,
        MsgFlags::empty(),
        None,
    )
    .map_err(io::Error::from)?;
    // O descritor foi no primeiro pedaço; o resto vai normalmente.
    let mut rest = stream;
    while sent < data.len() {
        sent += rest.write(&data[sent..])?;
    }
    Ok(())
}

/// Lê a primeira linha da conexão juntando os descritores que vierem com
/// ela. Devolve a linha (sem o `\n`), o que já chegou depois dela e os
/// descritores.
pub fn recv_first_line(stream: &UnixStream) -> io::Result<(String, Vec<u8>, Vec<OwnedFd>)> {
    use nix::sys::socket::{recvmsg, ControlMessageOwned};
    const MAX_LINE: usize = 1024 * 1024;
    let mut data = Vec::new();
    let mut fds = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let mut cmsg = nix::cmsg_space!([RawFd; 4]);
        let (bytes, received) = {
            let mut iov = [IoSliceMut::new(&mut buf)];
            let msg = recvmsg::<()>(stream.as_raw_fd(), &mut iov, Some(&mut cmsg), recv_flags())
                .map_err(io::Error::from)?;
            let mut received = Vec::new();
            for control in msg.cmsgs() {
                if let ControlMessageOwned::ScmRights(list) = control {
                    received.extend(list);
                }
            }
            (msg.bytes, received)
        };
        for fd in received {
            // SAFETY: o kernel acabou de criar este descritor para nós
            // (SCM_RIGHTS) e ninguém mais o possui.
            fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
        }
        if bytes == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before the first line",
            ));
        }
        data.extend_from_slice(&buf[..bytes]);
        if let Some(end) = data.iter().position(|&b| b == b'\n') {
            let rest = data.split_off(end + 1);
            data.pop();
            let line = String::from_utf8(data)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            return Ok((line, rest, fds));
        }
        if data.len() > MAX_LINE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
        }
    }
}

/// Descritores recebidos já com close-on-exec (onde o sistema permite).
fn recv_flags() -> nix::sys::socket::MsgFlags {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        nix::sys::socket::MsgFlags::MSG_CMSG_CLOEXEC
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        nix::sys::socket::MsgFlags::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frames_round_trip() {
        let mut wire = Vec::new();
        write_frame(&mut wire, FRAME_INPUT, b"ls\r").unwrap();
        write_frame(&mut wire, FRAME_RESIZE, &resize_payload(120, 40)).unwrap();
        write_frame(&mut wire, FRAME_KILL, &[]).unwrap();
        let mut input = wire.as_slice();
        assert_eq!(
            read_frame(&mut input).unwrap(),
            (FRAME_INPUT, b"ls\r".to_vec())
        );
        let (kind, payload) = read_frame(&mut input).unwrap();
        assert_eq!(kind, FRAME_RESIZE);
        assert_eq!(parse_resize(&payload), Some((120, 40)));
        assert_eq!(read_frame(&mut input).unwrap(), (FRAME_KILL, Vec::new()));
        assert_eq!(
            read_frame(&mut input).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn test_oversized_frame_is_rejected() {
        let mut wire = vec![FRAME_OUTPUT];
        wire.extend_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            read_frame(&mut wire.as_slice()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(parse_resize(&[1, 2]), None);
    }

    /// A linha chega com o descritor; o que vem depois dela não se perde.
    #[test]
    fn test_line_carries_fd_and_keeps_the_rest() {
        let (a, b) = UnixStream::pair().unwrap();
        let file = tempfile::tempfile().unwrap();
        let mut message = b"{\"oi\":1}\nresto".to_vec();
        message.extend_from_slice(&[0u8; 3]);
        send_with_fd(&a, &message, file.as_raw_fd()).unwrap();
        let (line, rest, fds) = recv_first_line(&b).unwrap();
        assert_eq!(line, "{\"oi\":1}");
        assert!(rest.starts_with(b"resto"));
        assert_eq!(fds.len(), 1);
        // O descritor recebido aponta para o mesmo arquivo.
        let mut received = std::fs::File::from(fds.into_iter().next().unwrap());
        received.write_all(b"x").unwrap();
        drop(a);
        assert!(recv_first_line(&b).is_err());
    }
}
