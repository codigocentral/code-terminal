//! Socket Communication - Comunicação via socket local
//!
//! Usa Unix sockets em Unix/Linux e named pipes em Windows.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

/// Endereço do socket do servidor
pub fn server_socket_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("codeterm")
        .join("multiplexer.sock")
}

/// Diretório de runtime
pub fn runtime_dir() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("codeterm")
}

/// Verifica se o servidor está rodando
pub fn is_server_running() -> bool {
    server_socket_path().exists()
}

/// Trait para conexão de transporte
pub trait Transport: Send {
    /// Envia uma mensagem
    fn send(&mut self, data: &[u8]) -> std::io::Result<()>;
    /// Recebe uma mensagem (linha)
    fn recv(&mut self) -> std::io::Result<String>;
    /// Fecha a conexão
    fn close(&mut self) -> std::io::Result<()>;
}

/// Conexão TCP local (fallback universal)
pub struct TcpTransport {
    stream: std::net::TcpStream,
    reader: BufReader<std::net::TcpStream>,
}

impl TcpTransport {
    /// Conecta ao servidor local
    pub fn connect(port: u16) -> std::io::Result<Self> {
        let stream = std::net::TcpStream::connect(format!("127.0.0.1:{}", port))?;
        stream.set_nonblocking(false)?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self { stream, reader })
    }
}

impl Transport for TcpTransport {
    fn send(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.stream.write_all(data)?;
        self.stream.write_all(b"\n")?;
        self.stream.flush()
    }

    fn recv(&mut self) -> std::io::Result<String> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        Ok(line.trim_end().to_string())
    }

    fn close(&mut self) -> std::io::Result<()> {
        self.stream.shutdown(std::net::Shutdown::Both)
    }
}

#[cfg(unix)]
pub mod unix {
    use super::*;
    use std::os::unix::net::UnixStream;

    /// Conexão Unix socket
    pub struct UnixTransport {
        stream: UnixStream,
        reader: BufReader<UnixStream>,
    }

    impl UnixTransport {
        /// Conecta ao servidor via Unix socket
        pub fn connect(path: &std::path::Path) -> std::io::Result<Self> {
            let stream = UnixStream::connect(path)?;
            stream.set_nonblocking(false)?;
            let reader = BufReader::new(stream.try_clone()?);
            Ok(Self { stream, reader })
        }
    }

    impl Transport for UnixTransport {
        fn send(&mut self, data: &[u8]) -> std::io::Result<()> {
            self.stream.write_all(data)?;
            self.stream.write_all(b"\n")?;
            self.stream.flush()
        }

        fn recv(&mut self) -> std::io::Result<String> {
            let mut line = String::new();
            self.reader.read_line(&mut line)?;
            Ok(line.trim_end().to_string())
        }

        fn close(&mut self) -> std::io::Result<()> {
            self.stream.shutdown(std::net::Shutdown::Both)
        }
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;

    /// Transporte local no Windows.
    ///
    /// Named pipes ainda não foram implementados neste módulo. Para evitar panic
    /// em runtime, o cliente Windows usa o fallback TCP local já suportado.
    pub struct PipeTransport {
        inner: TcpTransport,
    }

    impl PipeTransport {
        /// Conecta ao servidor via transporte local.
        pub fn connect(_path: &std::path::Path) -> std::io::Result<Self> {
            Ok(Self {
                inner: TcpTransport::connect(7878)?,
            })
        }
    }

    impl Transport for PipeTransport {
        fn send(&mut self, data: &[u8]) -> std::io::Result<()> {
            self.inner.send(data)
        }

        fn recv(&mut self) -> std::io::Result<String> {
            self.inner.recv()
        }

        fn close(&mut self) -> std::io::Result<()> {
            self.inner.close()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_dir() {
        let dir = runtime_dir();
        assert!(dir.to_string_lossy().contains("codeterm"));
    }

    #[test]
    fn test_socket_path() {
        let path = server_socket_path();
        assert!(path.to_string_lossy().contains("multiplexer.sock"));
    }

    use std::io::{BufRead, BufReader, Write};

    /// Servidor de eco de uma linha: responde "eco:<linha>" e encerra.
    fn echo_once<S: std::io::Read + Write>(stream: S, reader: impl std::io::Read) -> S {
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let mut stream = stream;
        writeln!(stream, "eco:{}", line.trim_end()).unwrap();
        stream.flush().unwrap();
        stream
    }

    #[test]
    fn test_is_server_running_matches_socket_file() {
        assert_eq!(is_server_running(), server_socket_path().exists());
        assert!(server_socket_path().starts_with(runtime_dir()));
    }

    #[test]
    fn test_tcp_transport_round_trip() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let reader = stream.try_clone().unwrap();
            echo_once(stream, reader);
        });

        let mut transport = TcpTransport::connect(port).unwrap();
        transport.send(b"ping").unwrap();
        assert_eq!(transport.recv().unwrap(), "eco:ping");
        server.join().unwrap();
        // Servidor fechou: recv devolve linha vazia
        assert_eq!(transport.recv().unwrap(), "");
        // Com o par já fechado, shutdown pode devolver ENOTCONN em alguns SOs
        let _ = transport.close();
    }

    #[test]
    fn test_tcp_transport_connect_refused() {
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        assert!(TcpTransport::connect(port).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_transport_round_trip() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mux.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let reader = stream.try_clone().unwrap();
            echo_once(stream, reader);
        });

        let mut transport = unix::UnixTransport::connect(&path).unwrap();
        transport.send(b"oi").unwrap();
        assert_eq!(transport.recv().unwrap(), "eco:oi");
        server.join().unwrap();
        let _ = transport.close();

        assert!(unix::UnixTransport::connect(&dir.path().join("nada.sock")).is_err());
    }
}
