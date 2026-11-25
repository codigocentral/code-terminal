//! Thread dedicada para processamento de escape sequences VT
//!
//! Isola o parsing CPU-bound do restante da aplicação,
//! permitindo melhor performance e responsividade.

use crate::terminal::grid::Grid;
use crate::terminal::parser::{Parser, ParserState};
use parking_lot::RwLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use tracing::{debug, trace, warn};

/// Mensagens enviadas para a Parser Thread
#[derive(Debug)]
pub enum ParserMessage {
    /// Dados brutos do PTY para processar
    Data(Vec<u8>),
    /// Sinaliza para a thread parar
    Shutdown,
}

/// Handle para comunicação com a Parser Thread
pub struct ParserThreadHandle {
    sender: Sender<ParserMessage>,
    thread: Option<JoinHandle<()>>,
}

impl ParserThreadHandle {
    /// Envia dados para serem processados pela thread
    pub fn send_data(&self, data: Vec<u8>) -> Result<(), mpsc::SendError<ParserMessage>> {
        self.sender.send(ParserMessage::Data(data))
    }

    /// Sinaliza shutdown para a thread
    pub fn shutdown(&self) {
        if let Err(error) = self.sender.send(ParserMessage::Shutdown) {
            tracing::debug!(error = %error, "Parser thread already stopped before shutdown signal");
        }
    }

    /// Aguarda a thread finalizar
    pub fn join(&mut self) {
        if let Some(handle) = self.thread.take() {
            if handle.join().is_err() {
                warn!("Parser thread panicked during join");
            }
        }
    }
}

impl Drop for ParserThreadHandle {
    fn drop(&mut self) {
        self.shutdown();
        self.join();
    }
}

/// Estrutura que gerencia a thread de parsing
pub struct ParserThread;

impl ParserThread {
    /// Cria uma nova Parser Thread associada a um Grid compartilhado
    ///
    /// Retorna o handle para comunicação e o Arc<RwLock<Grid>> compartilhado
    pub fn spawn(grid: Arc<RwLock<Grid>>) -> ParserThreadHandle {
        let (sender, receiver) = mpsc::channel::<ParserMessage>();

        let grid_clone = Arc::clone(&grid);

        let thread = thread::Builder::new()
            .name("vt-parser".into())
            .spawn(move || {
                let mut parser_thread = ParserThreadWorker::new(grid_clone, receiver);
                parser_thread.run();
            });
        if let Err(e) = &thread {
            tracing::error!(error = %e, "Failed to start the parser thread");
        }

        ParserThreadHandle {
            sender,
            thread: thread.ok(),
        }
    }
}

/// Worker interno que executa na thread de parsing
struct ParserThreadWorker {
    grid: Arc<RwLock<Grid>>,
    receiver: Receiver<ParserMessage>,
    stats: ParserStats,
    /// Estado do parser VT entre chunks
    parser_state: ParserState,
}

/// Estatísticas de parsing (para debug/performance)
#[derive(Debug, Default)]
struct ParserStats {
    bytes_processed: usize,
    chunks_processed: usize,
}

impl ParserThreadWorker {
    fn new(grid: Arc<RwLock<Grid>>, receiver: Receiver<ParserMessage>) -> Self {
        Self {
            grid,
            receiver,
            stats: ParserStats::default(),
            parser_state: ParserState::default(),
        }
    }

    /// Loop principal da thread de parsing
    fn run(&mut self) {
        debug!("Parser thread started");

        loop {
            match self.receiver.recv() {
                Ok(ParserMessage::Data(data)) => {
                    self.process_data(&data);
                }
                Ok(ParserMessage::Shutdown) => {
                    debug!("Parser thread received shutdown signal");
                    break;
                }
                Err(e) => {
                    warn!(error = %e, "Parser channel closed unexpectedly");
                    break;
                }
            }
        }

        debug!(stats = ?self.stats, "Parser thread stopping");
    }

    /// Processa um chunk de dados VT
    fn process_data(&mut self, data: &[u8]) {
        trace!(bytes = data.len(), "Processing data chunk");

        // Adquire lock de escrita no grid
        let mut grid = self.grid.write();

        // Retoma o parser do chunk anterior e processa os dados
        let state = std::mem::take(&mut self.parser_state);
        let mut parser = Parser::resume(&mut grid, state);
        parser.process(data);
        self.parser_state = parser.into_state();

        // Atualiza estatísticas
        self.stats.bytes_processed += data.len();
        self.stats.chunks_processed += 1;

        // Log periodicamente (a cada 100 chunks)
        if self.stats.chunks_processed % 100 == 0 {
            trace!(
                chunks = self.stats.chunks_processed,
                bytes = self.stats.bytes_processed,
                "Parser stats"
            );
        }

        // Lock é liberado automaticamente aqui
    }
}

/// Wrapper thread-safe para o Grid com Parser Thread integrada
pub struct ThreadSafeGrid {
    grid: Arc<RwLock<Grid>>,
    parser_handle: ParserThreadHandle,
}

impl ThreadSafeGrid {
    /// Cria um novo ThreadSafeGrid com Parser Thread
    pub fn new(cols: usize, rows: usize) -> Self {
        let grid = Arc::new(RwLock::new(Grid::new(cols, rows)));
        let parser_handle = ParserThread::spawn(Arc::clone(&grid));

        Self {
            grid,
            parser_handle,
        }
    }

    /// Cria um novo ThreadSafeGrid com limite de scrollback configurável
    pub fn with_scrollback_limit(cols: usize, rows: usize, limit: usize) -> Self {
        let grid = Arc::new(RwLock::new(Grid::with_scrollback_limit(cols, rows, limit)));
        let parser_handle = ParserThread::spawn(Arc::clone(&grid));

        Self {
            grid,
            parser_handle,
        }
    }

    /// Envia dados para processamento na Parser Thread
    pub fn process_data(&self, data: Vec<u8>) -> Result<(), mpsc::SendError<ParserMessage>> {
        self.parser_handle.send_data(data)
    }

    /// Retorna referência ao Grid (para leitura pelo renderer)
    pub fn grid(&self) -> &Arc<RwLock<Grid>> {
        &self.grid
    }

    /// Retorna o Grid para leitura
    pub fn read_grid(&self) -> parking_lot::RwLockReadGuard<'_, Grid> {
        self.grid.read()
    }

    /// Retorna o Grid para escrita (use com cuidado!)
    pub fn write_grid(&self) -> parking_lot::RwLockWriteGuard<'_, Grid> {
        self.grid.write()
    }
}

impl Drop for ThreadSafeGrid {
    fn drop(&mut self) {
        debug!("ThreadSafeGrid dropping, shutting down parser thread");
        // ParserThreadHandle::drop cuida do shutdown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_parser_thread_creation() {
        let grid = Arc::new(RwLock::new(Grid::new(80, 24)));
        let handle = ParserThread::spawn(grid);

        // Dá tempo para a thread iniciar
        thread::sleep(Duration::from_millis(10));

        // Shutdown
        handle.shutdown();
    }

    #[test]
    fn test_parser_thread_processes_data() {
        let grid = Arc::new(RwLock::new(Grid::new(80, 24)));
        let handle = ParserThread::spawn(Arc::clone(&grid));

        // Envia dados para processar
        let data = b"Hello, World!".to_vec();
        handle.send_data(data).unwrap();

        // Aguarda processamento
        thread::sleep(Duration::from_millis(50));

        // Verifica se o grid foi atualizado
        let grid_lock = grid.read();
        let cells = grid_lock.get_cells();
        assert_eq!(cells[0][0].char, 'H');
        assert_eq!(cells[0][1].char, 'e');

        // Cleanup
        handle.shutdown();
    }

    #[test]
    fn test_parser_thread_escape_sequences() {
        let grid = Arc::new(RwLock::new(Grid::new(80, 24)));
        let handle = ParserThread::spawn(Arc::clone(&grid));

        // Envia escape sequence de cursor movement
        let data = b"\x1b[5;10HTest".to_vec();
        handle.send_data(data).unwrap();

        // Aguarda processamento
        thread::sleep(Duration::from_millis(50));

        // Verifica posição do cursor e conteúdo
        let grid_lock = grid.read();
        let (row, col) = grid_lock.cursor_position();
        assert_eq!(row, 4); // 0-indexed
        assert_eq!(col, 13); // After "Test" (col 10-1=9 + 4 chars = 13, 0-indexed)

        // Cleanup
        handle.shutdown();
    }

    #[test]
    fn test_thread_safe_grid() {
        let ts_grid = ThreadSafeGrid::new(80, 24);

        // Processa dados
        ts_grid.process_data(b"Hello".to_vec()).unwrap();
        thread::sleep(Duration::from_millis(50));

        // Lê o grid
        let grid = ts_grid.read_grid();
        let cells = grid.get_cells();
        assert_eq!(cells[0][0].char, 'H');
    }

    #[test]
    fn test_concurrent_read_write() {
        let ts_grid = Arc::new(ThreadSafeGrid::new(80, 24));

        // Thread que escreve
        let writer = thread::spawn({
            let ts_grid = Arc::clone(&ts_grid);
            move || {
                for i in 0..100 {
                    // O PTY entrega \r\n (onlcr); LF sozinho só desce a linha.
                    let data = format!("Line {}\r\n", i);
                    ts_grid.process_data(data.into_bytes()).unwrap();
                    thread::sleep(Duration::from_micros(100));
                }
            }
        });

        // Thread que lê
        let reader = thread::spawn({
            let ts_grid = Arc::clone(&ts_grid);
            move || {
                for _ in 0..50 {
                    let _grid = ts_grid.read_grid();
                    thread::sleep(Duration::from_micros(200));
                }
            }
        });

        // Aguarda ambas
        writer.join().unwrap();
        reader.join().unwrap();

        // Confirma que writer e reader usaram o mesmo grid compartilhado.
        thread::sleep(Duration::from_millis(50));
        let grid = ts_grid.read_grid();
        assert_eq!(grid.get_cells()[0][0].char, 'L');
    }
}
