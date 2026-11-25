//! Conexão com o daemon do multiplexer.
//!
//! No Unix o daemon escuta num socket `0600` em
//! `$XDG_RUNTIME_DIR/code-terminal-mux.sock`: só o próprio usuário anexa e
//! digita nas sessões. TCP fica para testes e para o Windows.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::net::UnixStream;

/// Porta TCP histórica do multiplexer (Windows e testes).
pub const DEFAULT_TCP_ADDR: &str = "127.0.0.1:9876";

/// Socket Unix padrão do daemon.
pub fn default_mux_socket_path() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("code-terminal-mux.sock")
}

/// Um stream com o daemon (Unix socket ou TCP).
#[derive(Debug)]
pub enum MuxConnection {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl MuxConnection {
    /// Conecta ao daemon padrão (socket Unix; TCP no Windows).
    pub fn connect_default() -> io::Result<Self> {
        #[cfg(unix)]
        {
            UnixStream::connect(default_mux_socket_path()).map(Self::Unix)
        }
        #[cfg(not(unix))]
        {
            TcpStream::connect(DEFAULT_TCP_ADDR).map(Self::Tcp)
        }
    }

    /// Conecta por TCP.
    pub fn connect_tcp(addr: &str) -> io::Result<Self> {
        TcpStream::connect(addr).map(Self::Tcp)
    }

    /// Conecta a um socket Unix.
    #[cfg(unix)]
    pub fn connect_unix(path: &std::path::Path) -> io::Result<Self> {
        UnixStream::connect(path).map(Self::Unix)
    }

    /// Segundo handle para o mesmo stream (ler numa thread, escrever noutra).
    pub fn try_clone(&self) -> io::Result<Self> {
        match self {
            Self::Tcp(s) => s.try_clone().map(Self::Tcp),
            #[cfg(unix)]
            Self::Unix(s) => s.try_clone().map(Self::Unix),
        }
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.set_read_timeout(timeout),
            #[cfg(unix)]
            Self::Unix(s) => s.set_read_timeout(timeout),
        }
    }

    /// Fecha os dois sentidos (acorda quem está lendo noutra thread).
    pub fn shutdown(&self) {
        let _ = match self {
            Self::Tcp(s) => s.shutdown(std::net::Shutdown::Both),
            #[cfg(unix)]
            Self::Unix(s) => s.shutdown(std::net::Shutdown::Both),
        };
    }
}

impl Read for MuxConnection {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(s) => s.read(buf),
            #[cfg(unix)]
            Self::Unix(s) => s.read(buf),
        }
    }
}

impl Write for MuxConnection {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(s) => s.write(buf),
            #[cfg(unix)]
            Self::Unix(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(s) => s.flush(),
            #[cfg(unix)]
            Self::Unix(s) => s.flush(),
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn test_unix_connection_clone_reads_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2];
            stream.read_exact(&mut buf).unwrap();
            stream.write_all(&buf).unwrap();
        });
        let mut conn = MuxConnection::connect_unix(&path).unwrap();
        let mut reader = conn.try_clone().unwrap();
        conn.write_all(b"oi").unwrap();
        let mut back = [0u8; 2];
        reader.read_exact(&mut back).unwrap();
        assert_eq!(&back, b"oi");
        server.join().unwrap();
        conn.shutdown();
    }

    #[test]
    fn test_default_socket_name() {
        assert!(default_mux_socket_path().ends_with("code-terminal-mux.sock"));
    }
}
