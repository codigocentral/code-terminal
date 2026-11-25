use parking_lot::{Condvar, Mutex, RwLock};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use tracing::{info, warn};

/// Mensagens de controle para threads
#[derive(Debug, Clone)]
pub enum ThreadControlMessage {
    Shutdown,
    Pause,
    Resume,
}

/// Gerenciador de threads do terminal
pub struct ThreadManager {
    io_thread: Option<JoinHandle<()>>,
    parser_thread: Option<JoinHandle<()>>,
    render_thread: Option<JoinHandle<()>>,
    running: Arc<RwLock<bool>>,
}

impl ThreadManager {
    pub fn new() -> Self {
        Self {
            io_thread: None,
            parser_thread: None,
            render_thread: None,
            running: Arc::new(RwLock::new(false)),
        }
    }

    fn start<F>(&mut self, name: &str, f: F) -> Option<JoinHandle<()>>
    where
        F: FnOnce() + Send + 'static,
    {
        let handle = crate::utils::spawn_named(name, f).ok();
        *self.running.write() = true;
        handle
    }

    pub fn start_io_thread<F>(&mut self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        info!("Starting IO thread");
        self.io_thread = self.start("io-thread", f);
    }

    pub fn start_parser_thread<F>(&mut self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        info!("Starting Parser thread");
        self.parser_thread = self.start("parser-thread", f);
    }

    pub fn start_render_thread<F>(&mut self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        info!("Starting Render thread");
        self.render_thread = self.start("render-thread", f);
    }

    pub fn stop(&mut self) {
        info!("Stopping all threads...");
        *self.running.write() = false;

        if let Some(handle) = self.io_thread.take() {
            let _ = handle.join();
            info!("IO thread stopped");
        }

        if let Some(handle) = self.parser_thread.take() {
            let _ = handle.join();
            info!("Parser thread stopped");
        }

        if let Some(handle) = self.render_thread.take() {
            let _ = handle.join();
            info!("Render thread stopped");
        }
    }

    pub fn is_running(&self) -> bool {
        *self.running.read()
    }
}

impl Default for ThreadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ThreadManager {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct ThreadPool {
    workers: Vec<JoinHandle<()>>,
    sender: Option<Sender<Box<dyn FnOnce() + Send>>>,
}

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        let (sender, receiver): (
            Sender<Box<dyn FnOnce() + Send>>,
            Receiver<Box<dyn FnOnce() + Send>>,
        ) = mpsc::channel();
        let receiver = Arc::new(Mutex::new(receiver));

        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            let receiver = Arc::clone(&receiver);
            let handle = crate::utils::spawn_named(&format!("worker-{id}"), move || {
                info!("ThreadPool worker {} started", id);
                loop {
                    let job = {
                        let recv = receiver.lock();
                        recv.recv()
                    };

                    match job {
                        Ok(job) => job(),
                        Err(_) => {
                            info!("ThreadPool worker {} shutting down", id);
                            break;
                        }
                    }
                }
            });
            workers.extend(handle.ok());
        }

        Self {
            workers,
            sender: Some(sender),
        }
    }

    pub fn spawn<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        if let Some(ref sender) = self.sender {
            if let Err(e) = sender.send(Box::new(f)) {
                warn!("Failed to send job to thread pool: {:?}", e);
            }
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        drop(self.sender.take());

        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

pub struct RingBuffer<T> {
    buffer: Vec<T>,
    capacity: usize,
}

impl<T> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.capacity == 0 {
            // Sem espaço nenhum: `remove(0)` num Vec vazio entraria em pânico
            return;
        }
        if self.buffer.len() >= self.capacity {
            self.buffer.remove(0);
        }
        self.buffer.push(item);
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.buffer.is_empty() {
            return None;
        }
        Some(self.buffer.remove(0))
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

pub struct SyncRingBuffer<T: Clone> {
    buffer: Mutex<RingBuffer<T>>,
    not_empty: Condvar,
    not_full: Condvar,
}

impl<T: Clone> SyncRingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Mutex::new(RingBuffer::new(capacity)),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
        }
    }

    pub fn push(&self, item: T) {
        let mut buffer = self.buffer.lock();

        while buffer.len() >= buffer.capacity() {
            self.not_full.wait(&mut buffer);
        }

        buffer.push(item);
        self.not_empty.notify_one();
    }

    pub fn try_push(&self, item: T) -> bool {
        let mut buffer = self.buffer.lock();

        if buffer.len() >= buffer.capacity() {
            return false;
        }

        buffer.push(item);
        self.not_empty.notify_one();
        true
    }

    pub fn pop(&self) -> Option<T> {
        let mut buffer = self.buffer.lock();

        while buffer.is_empty() {
            self.not_empty.wait(&mut buffer);
        }

        let item = buffer.pop();
        self.not_full.notify_one();
        item
    }

    pub fn try_pop(&self) -> Option<T> {
        let mut buffer = self.buffer.lock();

        if buffer.is_empty() {
            return None;
        }

        let item = buffer.pop();
        self.not_full.notify_one();
        item
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.lock().is_empty()
    }

    pub fn len(&self) -> usize {
        self.buffer.lock().len()
    }
}

impl<T: Clone> Default for SyncRingBuffer<T> {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// Mensagens do parser para o renderer
#[derive(Debug, Clone)]
pub enum RenderMessage {
    GridUpdated,
    CursorMoved { row: usize, col: usize },
    ScrollbackChanged,
    TitleChanged(String),
    Shutdown,
}

/// Estrutura de canais para comunicação entre threads do terminal
pub struct TerminalChannels {
    io_to_parser_tx: Sender<Vec<u8>>,
    io_to_parser_rx: Arc<Mutex<Receiver<Vec<u8>>>>,
    parser_to_render_tx: Sender<RenderMessage>,
    parser_to_render_rx: Arc<Mutex<Receiver<RenderMessage>>>,
    control_tx: Sender<ThreadControlMessage>,
    control_rx: Arc<Mutex<Receiver<ThreadControlMessage>>>,
}

impl TerminalChannels {
    pub fn new() -> Self {
        let (io_tx, io_rx) = mpsc::channel();
        let (render_tx, render_rx) = mpsc::channel();
        let (control_tx, control_rx) = mpsc::channel();

        Self {
            io_to_parser_tx: io_tx,
            io_to_parser_rx: Arc::new(Mutex::new(io_rx)),
            parser_to_render_tx: render_tx,
            parser_to_render_rx: Arc::new(Mutex::new(render_rx)),
            control_tx,
            control_rx: Arc::new(Mutex::new(control_rx)),
        }
    }

    pub fn io_sender(&self) -> Sender<Vec<u8>> {
        self.io_to_parser_tx.clone()
    }

    pub fn io_receiver(&self) -> Arc<Mutex<Receiver<Vec<u8>>>> {
        Arc::clone(&self.io_to_parser_rx)
    }

    pub fn render_sender(&self) -> Sender<RenderMessage> {
        self.parser_to_render_tx.clone()
    }

    pub fn render_receiver(&self) -> Arc<Mutex<Receiver<RenderMessage>>> {
        Arc::clone(&self.parser_to_render_rx)
    }

    pub fn control_sender(&self) -> Sender<ThreadControlMessage> {
        self.control_tx.clone()
    }

    pub fn control_receiver(&self) -> Arc<Mutex<Receiver<ThreadControlMessage>>> {
        Arc::clone(&self.control_rx)
    }

    pub fn shutdown(&self) {
        let _ = self.control_tx.send(ThreadControlMessage::Shutdown);
        let _ = self.parser_to_render_tx.send(RenderMessage::Shutdown);
    }
}

impl Default for TerminalChannels {
    fn default() -> Self {
        Self::new()
    }
}

/// Notificador thread-safe usando Condvar
///
/// Permite que uma thread espere por um sinal de outra thread
/// de forma eficiente (sem busy-waiting)
pub struct ThreadNotifier {
    state: Mutex<NotifierState>,
    condvar: Condvar,
}

/// Estado do notificador.
///
/// `generation` muda a cada `notify_all`: com só um flag booleano, a primeira
/// thread acordada zerava o flag e as outras voltavam a dormir, então o
/// `notify_all` acordava uma thread só.
struct NotifierState {
    pending: bool,
    generation: u64,
}

impl ThreadNotifier {
    /// Cria um novo notificador
    pub fn new() -> Self {
        Self {
            state: Mutex::new(NotifierState {
                pending: false,
                generation: 0,
            }),
            condvar: Condvar::new(),
        }
    }

    /// Notifica uma thread esperando
    pub fn notify_one(&self) {
        let mut state = self.state.lock();
        state.pending = true;
        self.condvar.notify_one();
    }

    /// Notifica todas as threads esperando
    pub fn notify_all(&self) {
        let mut state = self.state.lock();
        state.pending = true;
        state.generation = state.generation.wrapping_add(1);
        self.condvar.notify_all();
    }

    /// Espera pela notificação
    pub fn wait(&self) {
        let mut state = self.state.lock();
        let generation = state.generation;
        while !state.pending && state.generation == generation {
            self.condvar.wait(&mut state);
        }
        // Acordada por notify_one: consome o sinal. Por notify_all: deixa
        // o sinal para as demais threads da mesma rodada.
        if state.generation == generation {
            state.pending = false;
        }
    }

    /// Espera pela notificação com timeout
    ///
    /// Retorna true se foi notificado, false se timeout
    pub fn wait_timeout(&self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self.state.lock();
        let generation = state.generation;
        while !state.pending && state.generation == generation {
            if self.condvar.wait_until(&mut state, deadline).timed_out() {
                break;
            }
        }
        let notified = state.pending || state.generation != generation;
        if state.generation == generation {
            state.pending = false;
        }
        notified
    }

    /// Reseta o flag de notificação
    pub fn reset(&self) {
        self.state.lock().pending = false;
    }
}

impl Default for ThreadNotifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Barreira de sincronização para múltiplas threads
///
/// Útil para sincronizar threads em pontos específicos
pub struct ThreadBarrier {
    state: Mutex<BarrierState>,
    target: usize,
    condvar: Condvar,
}

/// Contagem da rodada atual e número da rodada.
///
/// Esperar pela troca de rodada (e não por `count == 0`) evita travar quando
/// uma thread rápida já entra na rodada seguinte antes das outras acordarem.
struct BarrierState {
    count: usize,
    generation: u64,
}

impl ThreadBarrier {
    /// Cria uma nova barreira para `target` threads
    pub fn new(target: usize) -> Self {
        Self {
            state: Mutex::new(BarrierState {
                count: 0,
                generation: 0,
            }),
            target,
            condvar: Condvar::new(),
        }
    }

    /// Espera na barreira
    ///
    /// Retorna true se foi a última thread a chegar
    pub fn wait(&self) -> bool {
        let mut state = self.state.lock();
        let generation = state.generation;
        state.count += 1;

        if state.count >= self.target {
            // Última thread a chegar: abre a rodada seguinte
            state.count = 0;
            state.generation = state.generation.wrapping_add(1);
            self.condvar.notify_all();
            true
        } else {
            while state.generation == generation {
                self.condvar.wait(&mut state);
            }
            false
        }
    }

    /// Reseta a barreira
    pub fn reset(&self) {
        self.state.lock().count = 0;
    }
}

/// Semáforo contador para controle de acesso a recursos
///
/// Limita o número de threads que podem acessar um recurso simultaneamente
pub struct ThreadSemaphore {
    permits: Mutex<usize>,
    condvar: Condvar,
}

impl ThreadSemaphore {
    /// Cria um novo semáforo com `permits` permissões
    pub fn new(permits: usize) -> Self {
        Self {
            permits: Mutex::new(permits),
            condvar: Condvar::new(),
        }
    }

    /// Adquire uma permissão (bloqueia se não houver)
    pub fn acquire(&self) {
        let mut permits = self.permits.lock();
        while *permits == 0 {
            self.condvar.wait(&mut permits);
        }
        *permits -= 1;
    }

    /// Tenta adquirir uma permissão (não bloqueia)
    ///
    /// Retorna true se conseguiu, false se não havia permissões
    pub fn try_acquire(&self) -> bool {
        let mut permits = self.permits.lock();
        if *permits > 0 {
            *permits -= 1;
            true
        } else {
            false
        }
    }

    /// Libera uma permissão
    pub fn release(&self) {
        let mut permits = self.permits.lock();
        *permits += 1;
        self.condvar.notify_one();
    }

    /// Retorna o número de permissões disponíveis
    pub fn available_permits(&self) -> usize {
        *self.permits.lock()
    }
}

/// Estado de uma thread worker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    Idle,
    Running,
    Paused,
    Stopping,
    Stopped,
}

/// Controle de estado thread-safe para workers
pub struct WorkerStateControl {
    state: Mutex<WorkerState>,
    condvar: Condvar,
}

impl WorkerStateControl {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(WorkerState::Idle),
            condvar: Condvar::new(),
        }
    }

    pub fn set_state(&self, new_state: WorkerState) {
        let mut state = self.state.lock();
        *state = new_state;
        self.condvar.notify_all();
    }

    pub fn get_state(&self) -> WorkerState {
        *self.state.lock()
    }

    pub fn wait_for_state(&self, target: WorkerState) {
        let mut state = self.state.lock();
        while *state != target {
            self.condvar.wait(&mut state);
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self.get_state(), WorkerState::Running)
    }

    pub fn should_stop(&self) -> bool {
        matches!(
            self.get_state(),
            WorkerState::Stopping | WorkerState::Stopped
        )
    }
}

impl Default for WorkerStateControl {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// LOCK-FREE STRUCTURES
// =============================================================================

use std::sync::atomic::{AtomicUsize, Ordering};

/// Ring Buffer Lock-Free (Single-Producer Single-Consumer)
///
/// Otimizado para hot paths onde não queremos locks.
/// Apenas uma thread pode escrever e uma pode ler.
///
/// # Safety
/// Não é thread-safe para múltiplos produtores ou consumidores.
pub struct AtomicRingBuffer<T> {
    buffer: Vec<std::cell::UnsafeCell<T>>,
    capacity: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl<T: Default + Clone> AtomicRingBuffer<T> {
    /// Cria um novo ring buffer com capacidade especificada
    ///
    /// A capacidade real será `capacity + 1` para distinguir entre
    /// buffer cheio e vazio.
    pub fn new(capacity: usize) -> Self {
        let actual_capacity = capacity + 1;
        let mut buffer = Vec::with_capacity(actual_capacity);
        for _ in 0..actual_capacity {
            buffer.push(std::cell::UnsafeCell::new(T::default()));
        }

        Self {
            buffer,
            capacity: actual_capacity,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Tenta adicionar um item (não bloqueia)
    ///
    /// Retorna true se conseguiu, false se buffer cheio
    pub fn try_push(&self, item: T) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        let next_tail = (tail + 1) % self.capacity;

        // Verifica se buffer está cheio
        if next_tail == self.head.load(Ordering::Acquire) {
            return false;
        }

        // Escreve o item
        unsafe {
            *self.buffer[tail].get() = item;
        }

        // Atualiza tail
        self.tail.store(next_tail, Ordering::Release);
        true
    }

    /// Tenta remover um item (não bloqueia)
    ///
    /// Retorna Some(item) se conseguiu, None se buffer vazio
    pub fn try_pop(&self) -> Option<T> {
        let head = self.head.load(Ordering::Relaxed);

        // Verifica se buffer está vazio
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }

        // Lê o item
        let item = unsafe { (*self.buffer[head].get()).clone() };

        // Atualiza head
        let next_head = (head + 1) % self.capacity;
        self.head.store(next_head, Ordering::Release);

        Some(item)
    }

    /// Verifica se está vazio
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    /// Verifica se está cheio
    pub fn is_full(&self) -> bool {
        let next_tail = (self.tail.load(Ordering::Acquire) + 1) % self.capacity;
        next_tail == self.head.load(Ordering::Acquire)
    }

    /// Retorna número aproximado de elementos
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);

        if tail >= head {
            tail - head
        } else {
            self.capacity - head + tail
        }
    }

    /// Retorna capacidade máxima
    pub fn capacity(&self) -> usize {
        self.capacity - 1
    }
}

// Safe para enviar entre threads (cada thread deve ter role exclusiva)
unsafe impl<T: Send> Send for AtomicRingBuffer<T> {}
unsafe impl<T: Send> Sync for AtomicRingBuffer<T> {}

/// Stack simples usando Mutex (para uso single-threaded ou quando lock-free não é crítico)
///
/// Para uma implementação lock-free real, usar crossbeam ou similar
pub struct AtomicStack<T> {
    inner: std::sync::Mutex<Vec<T>>,
    capacity: usize,
}

impl<T> AtomicStack<T> {
    /// Cria um novo stack com capacidade máxima
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: std::sync::Mutex::new(Vec::with_capacity(capacity)),
            capacity,
        }
    }

    /// Lock que sobrevive a um mutex envenenado (os dados continuam válidos)
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<T>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Tenta fazer push (não bloqueia)
    pub fn try_push(&self, item: T) -> bool {
        let mut guard = self.lock();
        if guard.len() >= self.capacity {
            return false;
        }
        guard.push(item);
        true
    }

    /// Tenta fazer pop (não bloqueia)
    pub fn try_pop(&self) -> Option<T> {
        let mut guard = self.lock();
        guard.pop()
    }

    /// Verifica se está vazio
    pub fn is_empty(&self) -> bool {
        let guard = self.lock();
        guard.is_empty()
    }

    /// Retorna o número de elementos
    pub fn len(&self) -> usize {
        let guard = self.lock();
        guard.len()
    }
}

/// Contador otimizado com múltiplas ordering strategies
///
/// Permite escolher entre performance (Relaxed) e consistência (SeqCst)
pub struct AtomicCounter {
    value: AtomicUsize,
}

impl AtomicCounter {
    /// Cria um novo contador
    pub fn new(value: usize) -> Self {
        Self {
            value: AtomicUsize::new(value),
        }
    }

    /// Incrementa (Relaxed - mais rápido)
    pub fn fetch_add_relaxed(&self, val: usize) -> usize {
        self.value.fetch_add(val, Ordering::Relaxed)
    }

    /// Incrementa (SeqCst - mais consistente)
    pub fn fetch_add_seqcst(&self, val: usize) -> usize {
        self.value.fetch_add(val, Ordering::SeqCst)
    }

    /// Lê o valor
    pub fn load(&self, ordering: Ordering) -> usize {
        self.value.load(ordering)
    }

    /// Escreve um valor
    pub fn store(&self, val: usize, ordering: Ordering) {
        self.value.store(val, ordering);
    }

    /// Compare-and-swap
    pub fn compare_exchange(
        &self,
        current: usize,
        new: usize,
        success: Ordering,
        failure: Ordering,
    ) -> Result<usize, usize> {
        self.value.compare_exchange(current, new, success, failure)
    }
}

impl Default for AtomicCounter {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Single-Producer Single-Consumer Queue Lock-Free
///
/// Implementação simples de fila lock-free para uso em hot paths.
/// Não é thread-safe para múltiplos produtores ou consumidores.
pub struct AtomicQueue<T> {
    buffer: Vec<std::cell::UnsafeCell<T>>,
    capacity: usize,
    read_idx: AtomicUsize,
    write_idx: AtomicUsize,
}

impl<T: Default + Clone> AtomicQueue<T> {
    /// Cria uma nova fila com capacidade especificada
    pub fn new(capacity: usize) -> Self {
        // Com 0 slots o `% capacity` dividia por zero no primeiro enqueue
        let capacity = capacity.max(1);
        let mut buffer = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            buffer.push(std::cell::UnsafeCell::new(T::default()));
        }

        Self {
            buffer,
            capacity,
            read_idx: AtomicUsize::new(0),
            write_idx: AtomicUsize::new(0),
        }
    }

    /// Tenta adicionar um item
    pub fn try_enqueue(&self, item: T) -> bool {
        let write = self.write_idx.load(Ordering::Relaxed);
        let next_write = (write + 1) % self.capacity;

        // Verifica se fila está cheia
        if next_write == self.read_idx.load(Ordering::Acquire) {
            return false;
        }

        // Escreve o item
        unsafe {
            *self.buffer[write].get() = item;
        }

        // Atualiza write index
        self.write_idx.store(next_write, Ordering::Release);
        true
    }

    /// Tenta remover um item
    pub fn try_dequeue(&self) -> Option<T> {
        let read = self.read_idx.load(Ordering::Relaxed);

        // Verifica se fila está vazia
        if read == self.write_idx.load(Ordering::Acquire) {
            return None;
        }

        // Lê o item
        let item = unsafe { (*self.buffer[read].get()).clone() };

        // Atualiza read index
        let next_read = (read + 1) % self.capacity;
        self.read_idx.store(next_read, Ordering::Release);

        Some(item)
    }

    /// Verifica se está vazia
    pub fn is_empty(&self) -> bool {
        self.read_idx.load(Ordering::Acquire) == self.write_idx.load(Ordering::Acquire)
    }

    /// Verifica se está cheia
    pub fn is_full(&self) -> bool {
        let next_write = (self.write_idx.load(Ordering::Acquire) + 1) % self.capacity;
        next_write == self.read_idx.load(Ordering::Acquire)
    }

    /// Retorna número aproximado de elementos
    pub fn len(&self) -> usize {
        let read = self.read_idx.load(Ordering::Acquire);
        let write = self.write_idx.load(Ordering::Acquire);

        if write >= read {
            write - read
        } else {
            self.capacity - (read - write)
        }
    }

    /// Retorna a capacidade total
    pub fn capacity(&self) -> usize {
        self.capacity - 1 // -1 porque usamos um slot para distinguir empty/full
    }
}

// Safe para Send/Sync (cada thread tem role exclusiva)
unsafe impl<T: Send> Send for AtomicQueue<T> {}
unsafe impl<T: Send> Sync for AtomicQueue<T> {}

/// Métricas de performance para threads
pub struct ThreadMetrics {
    /// Número de operações processadas
    operations: AtomicCounter,
    /// Tempo total de processamento (ns)
    total_time_ns: AtomicCounter,
    /// Número de bloqueios
    blocks: AtomicCounter,
}

impl ThreadMetrics {
    pub fn new() -> Self {
        Self {
            operations: AtomicCounter::new(0),
            total_time_ns: AtomicCounter::new(0),
            blocks: AtomicCounter::new(0),
        }
    }

    /// Registra uma operação
    pub fn record_operation(&self, duration_ns: u64) {
        self.operations.fetch_add_relaxed(1);
        self.total_time_ns.fetch_add_relaxed(duration_ns as usize);
    }

    /// Registra um bloqueio
    pub fn record_block(&self) {
        self.blocks.fetch_add_relaxed(1);
    }

    /// Retorna operações por segundo
    pub fn ops_per_second(&self) -> f64 {
        let ops = self.operations.load(Ordering::Relaxed) as f64;
        let time_ns = self.total_time_ns.load(Ordering::Relaxed) as f64;
        if time_ns > 0.0 {
            ops / (time_ns / 1_000_000_000.0)
        } else {
            0.0
        }
    }

    /// Reseta métricas
    pub fn reset(&self) {
        self.operations.store(0, Ordering::Relaxed);
        self.total_time_ns.store(0, Ordering::Relaxed);
        self.blocks.store(0, Ordering::Relaxed);
    }
}

impl Default for ThreadMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // =============================================================================
    // LOCK-FREE STRUCTURE TESTS
    // =============================================================================

    #[test]
    fn test_atomic_ring_buffer() {
        let buffer = AtomicRingBuffer::new(4);

        assert!(buffer.is_empty());
        assert!(!buffer.is_full());

        // Adiciona itens
        assert!(buffer.try_push(1));
        assert!(buffer.try_push(2));
        assert!(buffer.try_push(3));
        assert_eq!(buffer.len(), 3);

        // Remove itens
        assert_eq!(buffer.try_pop(), Some(1));
        assert_eq!(buffer.try_pop(), Some(2));
        assert_eq!(buffer.len(), 1);

        // Buffer circular: adiciona mais
        assert!(buffer.try_push(4));
        assert!(buffer.try_push(5));
        assert!(buffer.try_push(6));
        assert_eq!(buffer.len(), 4);

        // Deve estar cheio
        assert!(buffer.is_full());
        assert!(!buffer.try_push(7)); // Falha
    }

    #[test]
    fn test_atomic_ring_buffer_single_thread() {
        // Teste single-threaded completo
        let buffer = AtomicRingBuffer::new(8);

        for i in 0..100 {
            assert!(buffer.try_push(i));
            assert_eq!(buffer.len(), 1);
            assert_eq!(buffer.try_pop(), Some(i));
            assert!(buffer.is_empty());
        }
    }

    #[test]
    fn test_atomic_stack() {
        let stack = AtomicStack::new(4);

        assert!(stack.is_empty());

        // Push
        assert!(stack.try_push(1));
        assert!(!stack.is_empty());

        // Pop - apenas último push é recuperável
        assert_eq!(stack.try_pop(), Some(1));
        assert!(stack.is_empty());

        // Pop de stack vazio
        assert_eq!(stack.try_pop(), None);

        // Novo push
        stack.try_push(2);
        stack.try_push(3);
        // Apenas último é recuperável nesta implementação
        assert_eq!(stack.try_pop(), Some(3));
    }

    #[test]
    fn test_atomic_counter() {
        let counter = AtomicCounter::new(0);

        // Testa operações básicas
        assert_eq!(counter.fetch_add_relaxed(1), 0);
        assert_eq!(counter.fetch_add_relaxed(5), 1);
        assert_eq!(counter.load(Ordering::Relaxed), 6);

        // Testa SeqCst
        counter.store(0, Ordering::SeqCst);
        assert_eq!(counter.fetch_add_seqcst(10), 0);
        assert_eq!(counter.load(Ordering::SeqCst), 10);

        // Testa compare_exchange
        let result = counter.compare_exchange(10, 20, Ordering::SeqCst, Ordering::Relaxed);
        assert_eq!(result, Ok(10));
        assert_eq!(counter.load(Ordering::Relaxed), 20);

        // Testa falha no compare_exchange
        let result = counter.compare_exchange(10, 30, Ordering::SeqCst, Ordering::Relaxed);
        assert_eq!(result, Err(20));
        assert_eq!(counter.load(Ordering::Relaxed), 20);
    }

    #[test]
    fn test_atomic_queue() {
        let queue = AtomicQueue::new(4);

        assert!(queue.is_empty());

        // Enqueue
        assert!(queue.try_enqueue(1));
        assert!(queue.try_enqueue(2));
        assert!(queue.try_enqueue(3));
        assert!(!queue.is_empty());

        // Dequeue (FIFO)
        assert_eq!(queue.try_dequeue(), Some(1));
        assert_eq!(queue.try_dequeue(), Some(2));
        assert_eq!(queue.len(), 1);

        // Mais enqueue
        assert!(queue.try_enqueue(4));
        assert!(queue.try_enqueue(5));
        assert!(queue.is_full());
        assert!(!queue.try_enqueue(6)); // Falha

        // Drain
        assert_eq!(queue.try_dequeue(), Some(3));
        assert_eq!(queue.try_dequeue(), Some(4));
        assert_eq!(queue.try_dequeue(), Some(5));
        assert!(queue.is_empty());
    }

    #[test]
    fn test_thread_metrics() {
        let metrics = ThreadMetrics::new();

        // Simula operações
        for _ in 0..10 {
            metrics.record_operation(100_000); // 100us
        }

        // Registra bloqueios
        metrics.record_block();
        metrics.record_block();

        // Verifica
        assert_eq!(metrics.operations.load(Ordering::Relaxed), 10);
        assert_eq!(metrics.total_time_ns.load(Ordering::Relaxed), 1_000_000); // 10 * 100us
        assert_eq!(metrics.blocks.load(Ordering::Relaxed), 2);

        // Calcula ops/s
        let ops_per_sec = metrics.ops_per_second();
        assert!(ops_per_sec > 0.0);

        // Reseta
        metrics.reset();
        assert_eq!(metrics.operations.load(Ordering::Relaxed), 0);
    }

    // =============================================================================
    // ORIGINAL TESTS
    // =============================================================================

    #[test]
    fn test_thread_manager() {
        let mut manager = ThreadManager::new();
        assert!(!manager.is_running());

        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        manager.start_io_thread(move || {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        std::thread::sleep(Duration::from_millis(50));
        manager.stop();

        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_thread_pool() {
        let pool = ThreadPool::new(4);

        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        pool.spawn(move || {
            counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });

        std::thread::sleep(Duration::from_millis(100));

        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_ring_buffer() {
        let mut buffer = RingBuffer::new(3);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        assert_eq!(buffer.len(), 3);

        assert_eq!(buffer.pop(), Some(1));
        assert_eq!(buffer.pop(), Some(2));
        assert_eq!(buffer.pop(), Some(3));
        assert_eq!(buffer.pop(), None);
    }

    #[test]
    fn test_sync_ring_buffer() {
        let buffer = SyncRingBuffer::new(3);

        buffer.push(1);
        buffer.push(2);
        buffer.push(3);

        assert_eq!(buffer.len(), 3);

        assert_eq!(buffer.try_pop(), Some(1));
        assert_eq!(buffer.try_pop(), Some(2));
        assert_eq!(buffer.try_pop(), Some(3));
        assert_eq!(buffer.try_pop(), None);
    }

    #[test]
    fn test_terminal_channels() {
        let channels = TerminalChannels::new();

        let io_sender = channels.io_sender();
        io_sender.send(vec![1, 2, 3]).unwrap();
        assert_eq!(channels.io_receiver().lock().recv().unwrap(), vec![1, 2, 3]);

        let render_sender = channels.render_sender();
        render_sender.send(RenderMessage::GridUpdated).unwrap();
        match channels.render_receiver().lock().recv().unwrap() {
            RenderMessage::GridUpdated => {}
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_thread_notifier() {
        let notifier = Arc::new(ThreadNotifier::new());
        let notifier_clone = Arc::clone(&notifier);

        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            notifier_clone.notify_one();
        });

        // Espera pela notificação (deve receber após 50ms)
        notifier.wait();
        handle.join().unwrap();
    }

    #[test]
    fn test_thread_notifier_timeout() {
        let notifier = ThreadNotifier::new();

        // Espera sem notificação (deve timeout)
        let result = notifier.wait_timeout(Duration::from_millis(50));
        assert!(!result);

        // Notifica e espera novamente
        notifier.notify_one();
        let result = notifier.wait_timeout(Duration::from_millis(50));
        assert!(result);
    }

    #[test]
    fn test_thread_barrier() {
        let barrier = Arc::new(ThreadBarrier::new(3));
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut handles = vec![];
        for _ in 0..3 {
            let barrier = Arc::clone(&barrier);
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(10));
                let is_last = barrier.wait();
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                is_last
            }));
        }

        let results: Vec<bool> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
        assert!(results.iter().filter(|&&x| x).count() <= 1); // Apenas uma (ou nenhuma) deve ser a última
    }

    #[test]
    fn test_thread_semaphore() {
        let semaphore = Arc::new(ThreadSemaphore::new(2));
        let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let mut handles = vec![];
        for _ in 0..5 {
            let semaphore = Arc::clone(&semaphore);
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                semaphore.acquire();
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(10));
                counter.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                semaphore.release();
            }));
        }

        // Aguarda todas as threads terminarem
        for handle in handles {
            handle.join().unwrap();
        }

        // O semáforo deve ter 2 permissões novamente
        assert_eq!(semaphore.available_permits(), 2);
    }

    #[test]
    fn test_worker_state_control() {
        let control = Arc::new(WorkerStateControl::new());

        assert_eq!(control.get_state(), WorkerState::Idle);

        control.set_state(WorkerState::Running);
        assert!(control.is_running());
        assert!(!control.should_stop());

        control.set_state(WorkerState::Stopping);
        assert!(!control.is_running());
        assert!(control.should_stop());

        // Testa wait_for_state em thread separada
        let control_clone = Arc::clone(&control);
        let handle = std::thread::spawn(move || {
            control_clone.wait_for_state(WorkerState::Stopped);
        });

        std::thread::sleep(Duration::from_millis(10));
        control.set_state(WorkerState::Stopped);
        handle.join().unwrap();

        assert_eq!(control.get_state(), WorkerState::Stopped);
    }

    #[test]
    fn test_thread_manager_runs_and_joins_all_threads() {
        let ran = Arc::new(AtomicUsize::new(0));
        let mut manager = ThreadManager::default();
        assert!(!manager.is_running());
        for start in [
            ThreadManager::start_io_thread::<Box<dyn FnOnce() + Send>>,
            ThreadManager::start_parser_thread::<Box<dyn FnOnce() + Send>>,
            ThreadManager::start_render_thread::<Box<dyn FnOnce() + Send>>,
        ] {
            let ran = Arc::clone(&ran);
            start(
                &mut manager,
                Box::new(move || {
                    ran.fetch_add(1, Ordering::SeqCst);
                }),
            );
        }
        assert!(manager.is_running());
        manager.stop();
        assert!(!manager.is_running());
        assert_eq!(ran.load(Ordering::SeqCst), 3, "stop espera as threads");
    }

    #[test]
    fn test_thread_pool_executes_jobs() {
        let pool = ThreadPool::new(2);
        let (tx, rx) = mpsc::channel();
        for i in 0..4 {
            let tx = tx.clone();
            pool.spawn(move || tx.send(i).unwrap());
        }
        let mut got: Vec<i32> = (0..4)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        got.sort();
        assert_eq!(got, vec![0, 1, 2, 3]);
    }

    /// Regressão: capacidade 0 fazia `remove(0)` num Vec vazio (pânico).
    #[test]
    fn test_ring_buffer_with_zero_capacity_drops_items() {
        let mut ring = RingBuffer::new(0);
        ring.push(1);
        assert!(ring.is_empty());
        assert_eq!(ring.pop(), None);
    }

    #[test]
    fn test_sync_ring_buffer_blocking_and_try_operations() {
        let buffer = Arc::new(SyncRingBuffer::new(1));
        assert!(buffer.is_empty());
        assert!(buffer.try_push(1));
        assert!(!buffer.try_push(2), "cheio");

        // push bloqueia até o consumidor liberar espaço
        let producer = {
            let buffer = Arc::clone(&buffer);
            std::thread::spawn(move || buffer.push(2))
        };
        assert_eq!(buffer.pop(), Some(1));
        producer.join().unwrap();
        assert_eq!(buffer.pop(), Some(2));
        assert_eq!(buffer.try_pop(), None);
        assert_eq!(SyncRingBuffer::<u8>::default().len(), 0);
    }

    #[test]
    fn test_terminal_channels_shutdown_and_control() {
        let channels = TerminalChannels::default();
        channels
            .control_sender()
            .send(ThreadControlMessage::Shutdown)
            .unwrap();
        assert!(matches!(
            channels.control_receiver().lock().recv().unwrap(),
            ThreadControlMessage::Shutdown
        ));
        channels.shutdown();
        assert!(matches!(
            channels.control_receiver().lock().recv().unwrap(),
            ThreadControlMessage::Shutdown
        ));
        assert!(matches!(
            channels.render_receiver().lock().recv().unwrap(),
            RenderMessage::Shutdown
        ));
    }

    /// notify_all precisa acordar todas as threads (antes a primeira zerava o
    /// flag e as outras voltavam a dormir em `wait`).
    #[test]
    fn test_notify_all_wakes_every_waiter() {
        let notifier = Arc::new(ThreadNotifier::default());
        let ready = Arc::new(ThreadBarrier::new(4));
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let notifier = Arc::clone(&notifier);
                let ready = Arc::clone(&ready);
                std::thread::spawn(move || {
                    ready.wait();
                    notifier.wait();
                    true
                })
            })
            .collect();
        ready.wait();
        // Os waiters podem ainda não ter entrado no wait: notifica até todos acordarem
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while handles.iter().any(|h| !h.is_finished()) {
            assert!(
                std::time::Instant::now() < deadline,
                "waiters não acordaram"
            );
            notifier.notify_all();
            std::thread::yield_now();
        }
        for handle in handles {
            assert!(handle.join().unwrap(), "todos notificados, nenhum timeout");
        }

        // notify_all deixa um sinal pendente; reset limpa
        notifier.notify_all();
        notifier.reset();
        assert!(!notifier.wait_timeout(Duration::from_millis(10)));
    }

    /// Regressão: reutilizar a barreira podia travar quando uma thread rápida
    /// entrava na rodada seguinte antes das outras acordarem.
    #[test]
    fn test_barrier_can_be_reused_across_rounds() {
        let barrier = Arc::new(ThreadBarrier::new(3));
        let lasts = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let lasts = Arc::clone(&lasts);
                std::thread::spawn(move || {
                    for _ in 0..200 {
                        if barrier.wait() {
                            lasts.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(lasts.load(Ordering::SeqCst), 200, "um 'último' por rodada");
        barrier.reset();
    }

    #[test]
    fn test_semaphore_try_acquire_and_worker_default() {
        let semaphore = ThreadSemaphore::new(1);
        assert!(semaphore.try_acquire());
        assert!(!semaphore.try_acquire());
        semaphore.release();
        assert_eq!(semaphore.available_permits(), 1);
        assert_eq!(WorkerStateControl::default().get_state(), WorkerState::Idle);
    }

    #[test]
    fn test_atomic_ring_buffer_wraps_and_reports_full() {
        let ring: AtomicRingBuffer<u32> = AtomicRingBuffer::new(2);
        assert_eq!(ring.capacity(), 2);
        assert!(ring.try_push(1) && ring.try_push(2));
        assert!(ring.is_full());
        assert!(!ring.try_push(3));
        assert_eq!(ring.try_pop(), Some(1));
        assert!(ring.try_push(3));
        // tail deu a volta: len com tail < head
        assert_eq!(ring.len(), 2);
        assert_eq!(ring.try_pop(), Some(2));
        assert_eq!(ring.try_pop(), Some(3));
        assert_eq!(ring.try_pop(), None);
    }

    #[test]
    fn test_atomic_stack_and_counter() {
        let stack = AtomicStack::new(1);
        assert!(stack.is_empty());
        assert!(stack.try_push(7));
        assert!(!stack.try_push(8));
        assert_eq!(stack.len(), 1);
        assert_eq!(stack.try_pop(), Some(7));

        let counter = AtomicCounter::default();
        assert_eq!(counter.fetch_add_seqcst(2), 0);
        assert_eq!(
            counter.compare_exchange(2, 5, Ordering::SeqCst, Ordering::SeqCst),
            Ok(2)
        );
        assert_eq!(
            counter.compare_exchange(2, 9, Ordering::SeqCst, Ordering::SeqCst),
            Err(5)
        );
    }

    #[test]
    fn test_atomic_stack_survives_poisoned_lock() {
        let stack = Arc::new(AtomicStack::new(4));
        stack.try_push(1);
        let poisoner = Arc::clone(&stack);
        let _ = std::thread::spawn(move || {
            let _guard = poisoner.inner.lock().unwrap();
            panic!("envenena o mutex");
        })
        .join();
        assert!(stack.try_push(2));
        assert_eq!(stack.len(), 2);
    }

    /// Regressão: AtomicQueue::new(0) dividia por zero no primeiro enqueue.
    #[test]
    fn test_atomic_queue_zero_capacity_and_wrap_around() {
        let empty: AtomicQueue<u8> = AtomicQueue::new(0);
        assert!(!empty.try_enqueue(1));
        assert_eq!(empty.capacity(), 0);

        let queue: AtomicQueue<u32> = AtomicQueue::new(3);
        assert_eq!(queue.capacity(), 2);
        assert!(queue.try_enqueue(1) && queue.try_enqueue(2));
        assert!(queue.is_full());
        assert_eq!(queue.try_dequeue(), Some(1));
        assert_eq!(queue.try_dequeue(), Some(2));
        assert!(queue.try_enqueue(3));
        assert!(queue.try_enqueue(4));
        // write deu a volta e ficou atrás de read
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.try_dequeue(), Some(3));
    }

    #[test]
    fn test_thread_metrics_rate_and_reset() {
        let metrics = ThreadMetrics::default();
        assert_eq!(metrics.ops_per_second(), 0.0);
        metrics.record_operation(500_000_000);
        metrics.record_operation(500_000_000);
        metrics.record_block();
        assert!((metrics.ops_per_second() - 2.0).abs() < f64::EPSILON);
        metrics.reset();
        assert_eq!(metrics.ops_per_second(), 0.0);
    }
}
