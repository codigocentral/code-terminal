//! Gerenciamento de conexões de clientes remotos
//!
//! Cada conexão é manipulada em uma task separada do tokio.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

#[cfg(unix)]
use tokio::net::UnixStream;

static CONNECTION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Identificador único de conexão
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub u64);

impl ConnectionId {
    /// Gerar novo ID único
    pub fn new() -> Self {
        Self(CONNECTION_ID_COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "conn-{}", self.0)
    }
}

/// Informações sobre uma conexão
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: ConnectionId,
    pub address: String,
    pub connected_at: std::time::Instant,
    pub bytes_received: u64,
    pub bytes_sent: u64,
}

impl ConnectionInfo {
    /// Criar info para conexão TCP
    pub fn from_tcp_addr(addr: SocketAddr) -> Self {
        Self {
            id: ConnectionId::new(),
            address: addr.to_string(),
            connected_at: std::time::Instant::now(),
            bytes_received: 0,
            bytes_sent: 0,
        }
    }

    /// Criar info para conexão Unix socket
    #[cfg(unix)]
    pub fn from_unix_socket() -> Self {
        Self {
            id: ConnectionId::new(),
            address: "unix".to_string(),
            connected_at: std::time::Instant::now(),
            bytes_received: 0,
            bytes_sent: 0,
        }
    }

    /// Retornar duração da conexão
    pub fn duration(&self) -> Duration {
        self.connected_at.elapsed()
    }
}

/// Handle para controlar uma conexão ativa
#[derive(Debug)]
pub struct ConnectionHandle {
    pub info: ConnectionInfo,
    shutdown: mpsc::Sender<()>,
}

impl ConnectionHandle {
    /// Criar novo handle
    fn new(info: ConnectionInfo) -> (Self, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel(1);
        (Self { info, shutdown: tx }, rx)
    }

    /// Solicitar desconexão gracefully
    pub async fn shutdown(&self) -> Result<(), ConnectionError> {
        self.shutdown
            .send(())
            .await
            .map_err(|_| ConnectionError::AlreadyClosed)
    }
}

/// Erros de conexão
#[derive(Debug)]
pub enum ConnectionError {
    Io(std::io::Error),
    Timeout,
    AlreadyClosed,
    InvalidMessage,
}

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionError::Io(e) => write!(f, "IO error: {}", e),
            ConnectionError::Timeout => write!(f, "Connection timeout"),
            ConnectionError::AlreadyClosed => write!(f, "Connection already closed"),
            ConnectionError::InvalidMessage => write!(f, "Invalid message format"),
        }
    }
}

impl std::error::Error for ConnectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConnectionError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ConnectionError {
    fn from(e: std::io::Error) -> Self {
        ConnectionError::Io(e)
    }
}

/// Gerenciador de conexões
#[derive(Debug)]
pub struct ConnectionManager {
    connections: Arc<parking_lot::Mutex<Vec<ConnectionHandle>>>,
    max_connections: usize,
}

impl ConnectionManager {
    /// Criar novo gerenciador
    pub fn new(max_connections: usize) -> Self {
        Self {
            connections: Arc::new(parking_lot::Mutex::new(Vec::new())),
            max_connections,
        }
    }

    /// Verificar se pode aceitar nova conexão
    pub fn can_accept(&self) -> bool {
        self.connections.lock().len() < self.max_connections
    }

    /// Número atual de conexões
    pub fn connection_count(&self) -> usize {
        self.connections.lock().len()
    }

    /// Verificar se há conexões ativas
    pub fn has_connections(&self) -> bool {
        !self.connections.lock().is_empty()
    }

    /// Aceitar e gerenciar conexão TCP
    pub async fn handle_tcp_connection<F, Fut>(
        &self,
        stream: TcpStream,
        idle_timeout: Duration,
        on_message: F,
    ) -> Result<ConnectionInfo, ConnectionError>
    where
        F: Fn(String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Option<String>> + Send,
    {
        if !self.can_accept() {
            return Err(ConnectionError::Io(std::io::Error::other(
                "Max connections reached",
            )));
        }

        let peer_addr = stream.peer_addr()?;
        let info = ConnectionInfo::from_tcp_addr(peer_addr);
        let (handle, mut shutdown_rx) = ConnectionHandle::new(info.clone());
        let id = info.id;

        // Registrar conexão
        self.connections.lock().push(handle);

        // Split stream para read/write
        let (read_half, mut write_half) = stream.into_split();
        let reader = BufReader::new(read_half);

        // Criar task para manipular a conexão
        let connections_clone = self.connections.clone();

        tokio::spawn(async move {
            if let Err(e) = connection_loop(
                reader,
                &mut write_half,
                idle_timeout,
                on_message,
                &mut shutdown_rx,
            )
            .await
            {
                tracing::debug!("Connection {} error: {}", id, e);
            }

            // Remover conexão da lista quando terminar
            let mut conns = connections_clone.lock();
            conns.retain(|c| c.info.id != id);
            tracing::info!("Connection {} closed", id);
        });

        Ok(info)
    }

    /// Aceitar e gerenciar conexão Unix socket
    #[cfg(unix)]
    pub async fn handle_unix_connection<F, Fut>(
        &self,
        stream: UnixStream,
        idle_timeout: Duration,
        on_message: F,
    ) -> Result<ConnectionInfo, ConnectionError>
    where
        F: Fn(String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Option<String>> + Send,
    {
        if !self.can_accept() {
            return Err(ConnectionError::Io(std::io::Error::other(
                "Max connections reached",
            )));
        }

        let info = ConnectionInfo::from_unix_socket();
        let (handle, mut shutdown_rx) = ConnectionHandle::new(info.clone());
        let id = info.id;

        self.connections.lock().push(handle);

        let (read_half, mut write_half) = stream.into_split();
        let reader = BufReader::new(read_half);
        let connections_clone = self.connections.clone();

        tokio::spawn(async move {
            if let Err(e) = unix_connection_loop(
                reader,
                &mut write_half,
                idle_timeout,
                on_message,
                &mut shutdown_rx,
            )
            .await
            {
                tracing::debug!("Unix connection {} error: {}", id, e);
            }

            let mut conns = connections_clone.lock();
            conns.retain(|c| c.info.id != id);
            tracing::info!("Unix connection {} closed", id);
        });

        Ok(info)
    }

    /// Desconectar todas as conexões
    pub async fn shutdown_all(&self) {
        let handles: Vec<_> = self.connections.lock().drain(..).collect();

        for handle in handles {
            let _ = handle.shutdown().await;
        }
    }

    /// Obter informações de todas as conexões
    pub fn list_connections(&self) -> Vec<ConnectionInfo> {
        self.connections
            .lock()
            .iter()
            .map(|h| h.info.clone())
            .collect()
    }
}

/// Loop principal de conexão TCP
async fn connection_loop<F, Fut>(
    mut reader: BufReader<OwnedReadHalf>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    idle_timeout: Duration,
    on_message: F,
    shutdown_rx: &mut mpsc::Receiver<()>,
) -> Result<(), ConnectionError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let mut line = String::new();

    loop {
        line.clear();

        tokio::select! {
            // Ler linha do cliente
            read_result = timeout(idle_timeout, reader.read_line(&mut line)) => {
                match read_result {
                    Ok(Ok(0)) => {
                        // EOF - conexão fechada
                        return Ok(());
                    }
                    Ok(Ok(_)) => {
                        // Processar mensagem
                        let line = line.trim_end();
                        if !line.is_empty() {
                            if let Some(response) = on_message(line.to_string()).await {
                                let response = format!("{}\n", response);
                                writer.write_all(response.as_bytes()).await?;
                                writer.flush().await?;
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        return Err(ConnectionError::Io(e));
                    }
                    Err(_) => {
                        return Err(ConnectionError::Timeout);
                    }
                }
            }

            // Sinal de shutdown
            _ = shutdown_rx.recv() => {
                tracing::debug!("Connection received shutdown signal");
                return Ok(());
            }
        }
    }
}

/// Loop principal de conexão Unix socket
#[cfg(unix)]
async fn unix_connection_loop<F, Fut>(
    mut reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    idle_timeout: Duration,
    on_message: F,
    shutdown_rx: &mut mpsc::Receiver<()>,
) -> Result<(), ConnectionError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let mut line = String::new();

    loop {
        line.clear();

        tokio::select! {
            read_result = timeout(idle_timeout, reader.read_line(&mut line)) => {
                match read_result {
                    Ok(Ok(0)) => return Ok(()),
                    Ok(Ok(_)) => {
                        let line = line.trim_end();
                        if !line.is_empty() {
                            if let Some(response) = on_message(line.to_string()).await {
                                let response = format!("{}\n", response);
                                writer.write_all(response.as_bytes()).await?;
                                writer.flush().await?;
                            }
                        }
                    }
                    Ok(Err(e)) => return Err(ConnectionError::Io(e)),
                    Err(_) => return Err(ConnectionError::Timeout),
                }
            }
            _ = shutdown_rx.recv() => {
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_id_generation() {
        let id1 = ConnectionId::new();
        let id2 = ConnectionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_connection_info_tcp() {
        let addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let info = ConnectionInfo::from_tcp_addr(addr);
        assert_eq!(info.address, "127.0.0.1:12345");
        assert_eq!(info.bytes_received, 0);
        assert_eq!(info.bytes_sent, 0);
    }

    #[test]
    fn test_connection_manager_creation() {
        let manager = ConnectionManager::new(5);
        assert_eq!(manager.connection_count(), 0);
        assert!(manager.can_accept());
    }

    #[test]
    fn test_connection_manager_max_connections() {
        let manager = ConnectionManager::new(2);
        assert!(manager.can_accept());

        // Simular conexões (não podemos testar async facilmente aqui)
        // Testamos a lógica de can_accept
    }

    #[test]
    fn test_connection_error_display() {
        let err = ConnectionError::Timeout;
        assert_eq!(err.to_string(), "Connection timeout");

        let err = ConnectionError::AlreadyClosed;
        assert_eq!(err.to_string(), "Connection already closed");
    }

    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    /// Espera (com limite) até a condição valer; evita sleeps fixos.
    async fn wait_until(mut condition: impl FnMut() -> bool) {
        timeout(Duration::from_secs(5), async {
            while !condition() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("condição não foi atingida a tempo");
    }

    /// Par de streams TCP conectados no loopback (porta efêmera).
    async fn tcp_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (client, accepted) = tokio::join!(TcpStream::connect(addr), listener.accept());
        (accepted.unwrap().0, client.unwrap())
    }

    async fn echo(message: String) -> Option<String> {
        if message == "silent" {
            None
        } else {
            Some(format!("echo:{}", message))
        }
    }

    #[test]
    fn test_connection_id_display_and_default() {
        let id = ConnectionId(42);
        assert_eq!(id.to_string(), "conn-42");
        let a = ConnectionId::default();
        let b = ConnectionId::default();
        assert!(b.0 > a.0);
    }

    #[test]
    fn test_connection_error_display_source_and_from() {
        use std::error::Error;

        let io = std::io::Error::other("falhou");
        let err: ConnectionError = io.into();
        assert_eq!(err.to_string(), "IO error: falhou");
        assert!(err.source().is_some());

        assert_eq!(
            ConnectionError::InvalidMessage.to_string(),
            "Invalid message format"
        );
        assert!(ConnectionError::Timeout.source().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_connection_info_unix_and_duration() {
        let info = ConnectionInfo::from_unix_socket();
        assert_eq!(info.address, "unix");
        assert_eq!(info.bytes_received, 0);
        assert!(info.duration() < Duration::from_secs(60));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_handle_shutdown_fails_when_receiver_is_gone() {
        let (handle, rx) = ConnectionHandle::new(ConnectionInfo::from_unix_socket());
        drop(rx);
        assert!(matches!(
            handle.shutdown().await,
            Err(ConnectionError::AlreadyClosed)
        ));
    }

    #[tokio::test]
    async fn test_tcp_connection_echoes_and_cleans_up_on_eof() {
        let manager = ConnectionManager::new(4);
        let (server, client) = tcp_pair().await;
        let client_addr = client.local_addr().unwrap();

        let info = manager
            .handle_tcp_connection(server, Duration::from_secs(10), echo)
            .await
            .unwrap();
        assert_eq!(info.address, client_addr.to_string());
        assert!(manager.has_connections());
        assert_eq!(manager.connection_count(), 1);
        assert_eq!(manager.list_connections()[0].id, info.id);

        let (read_half, mut write_half) = client.into_split();
        let mut reader = BufReader::new(read_half);
        // linha vazia é ignorada; "silent" não gera resposta
        write_half
            .write_all(b"hello\n\nsilent\nbye\r\n")
            .await
            .unwrap();

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(line, "echo:hello\n");
        line.clear();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(line, "echo:bye\n");

        drop(write_half);
        drop(reader);
        wait_until(|| !manager.has_connections()).await;
    }

    #[tokio::test]
    async fn test_tcp_connection_rejected_when_full() {
        let manager = ConnectionManager::new(0);
        assert!(!manager.can_accept());
        let (server, _client) = tcp_pair().await;
        let error = manager
            .handle_tcp_connection(server, Duration::from_secs(1), echo)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "IO error: Max connections reached");
        assert_eq!(manager.connection_count(), 0);
    }

    #[tokio::test]
    async fn test_tcp_connection_times_out_when_idle() {
        let manager = ConnectionManager::new(1);
        let (server, mut client) = tcp_pair().await;
        manager
            .handle_tcp_connection(server, Duration::from_millis(20), echo)
            .await
            .unwrap();
        wait_until(|| manager.connection_count() == 0).await;
        // O servidor fechou o socket: o cliente lê EOF
        let mut buf = [0u8; 8];
        assert_eq!(client.read(&mut buf).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_tcp_connection_closes_on_invalid_utf8() {
        let manager = ConnectionManager::new(1);
        let (server, mut client) = tcp_pair().await;
        manager
            .handle_tcp_connection(server, Duration::from_secs(10), echo)
            .await
            .unwrap();
        client.write_all(b"\xff\xfe\n").await.unwrap();
        wait_until(|| manager.connection_count() == 0).await;
    }

    #[tokio::test]
    async fn test_shutdown_all_disconnects_tcp_clients() {
        let manager = ConnectionManager::new(2);
        let (server, mut client) = tcp_pair().await;
        manager
            .handle_tcp_connection(server, Duration::from_secs(10), echo)
            .await
            .unwrap();
        manager.shutdown_all().await;
        assert_eq!(manager.connection_count(), 0);
        let mut buf = [0u8; 8];
        let read = timeout(Duration::from_secs(5), client.read(&mut buf))
            .await
            .expect("cliente deveria ver o fechamento");
        assert_eq!(read.unwrap(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unix_connection_echo_timeout_and_shutdown() {
        let manager = ConnectionManager::new(3);

        // Eco e limpeza no EOF
        let (server, client) = UnixStream::pair().unwrap();
        let info = manager
            .handle_unix_connection(server, Duration::from_secs(10), echo)
            .await
            .unwrap();
        assert_eq!(info.address, "unix");
        let (read_half, mut write_half) = client.into_split();
        let mut reader = BufReader::new(read_half);
        write_half.write_all(b"\nsilent\nping\n").await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(line, "echo:ping\n");
        drop(write_half);
        drop(reader);
        wait_until(|| manager.connection_count() == 0).await;

        // Timeout por inatividade
        let (server, _idle_client) = UnixStream::pair().unwrap();
        manager
            .handle_unix_connection(server, Duration::from_millis(20), echo)
            .await
            .unwrap();
        wait_until(|| manager.connection_count() == 0).await;

        // UTF-8 inválido encerra a conexão
        let (server, mut bad_client) = UnixStream::pair().unwrap();
        manager
            .handle_unix_connection(server, Duration::from_secs(10), echo)
            .await
            .unwrap();
        bad_client.write_all(b"\xff\n").await.unwrap();
        wait_until(|| manager.connection_count() == 0).await;

        // shutdown_all
        let (server, mut client) = UnixStream::pair().unwrap();
        manager
            .handle_unix_connection(server, Duration::from_secs(10), echo)
            .await
            .unwrap();
        assert_eq!(manager.connection_count(), 1);
        manager.shutdown_all().await;
        let mut buf = [0u8; 4];
        let read = timeout(Duration::from_secs(5), client.read(&mut buf))
            .await
            .expect("fechamento");
        assert_eq!(read.unwrap(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_unix_connection_rejected_when_full() {
        let manager = ConnectionManager::new(0);
        let (server, _client) = UnixStream::pair().unwrap();
        let error = manager
            .handle_unix_connection(server, Duration::from_secs(1), echo)
            .await
            .unwrap_err();
        assert!(matches!(error, ConnectionError::Io(_)));
    }
}
