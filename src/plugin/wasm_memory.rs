//! WASM Memory Management
//!
//! Gerenciamento de memória compartilhada entre host e plugins WASM.
//! Provê alocação, leitura, escrita e proteção de memória com validação de bounds.

use crate::plugin::error::{PluginError, PluginResult};
use std::sync::Arc;

/// Gerenciador de memória WASM
///
/// Provê operações seguras de memória entre host e guest WASM,
/// incluindo alocação, leitura, escrita e validação de bounds.
#[derive(Debug, Clone)]
pub struct WasmMemoryManager {
    /// Limite máximo de alocação por plugin (em bytes)
    max_allocation: usize,
    /// Memória total alocada pelo plugin
    allocated: Arc<std::sync::atomic::AtomicUsize>,
    /// Estatísticas de uso
    stats: Arc<std::sync::Mutex<MemoryStats>>,
}

/// Estatísticas de uso de memória
#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    /// Total de alocações realizadas
    pub allocations: u64,
    /// Total de liberações realizadas
    pub deallocations: u64,
    /// Pico de memória usada (bytes)
    pub peak_usage: usize,
    /// Falhas de alocação
    pub allocation_failures: u64,
}

/// Handle para uma alocação na memória WASM
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WasmAllocation {
    /// Offset na memória linear
    pub offset: usize,
    /// Tamanho alocado
    pub size: usize,
}

impl WasmMemoryManager {
    /// Cria novo gerenciador com limite de alocação
    pub fn new(max_allocation: usize) -> Self {
        Self {
            max_allocation,
            allocated: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            stats: Arc::new(std::sync::Mutex::new(MemoryStats::default())),
        }
    }

    /// Cria gerenciador com limite padrão (16 MB)
    pub fn with_default_limit() -> Self {
        Self::new(16 * 1024 * 1024) // 16 MB
    }

    /// Verifica se pode alocar o tamanho solicitado
    pub fn can_allocate(&self, size: usize) -> bool {
        let current = self.allocated.load(std::sync::atomic::Ordering::SeqCst);
        current + size <= self.max_allocation
    }

    /// Registra uma alocação bem-sucedida
    pub fn record_allocation(&self, size: usize) {
        let current = self
            .allocated
            .fetch_add(size, std::sync::atomic::Ordering::SeqCst);
        let new_total = current + size;

        let mut stats = self.stats.lock().unwrap();
        stats.allocations += 1;
        if new_total > stats.peak_usage {
            stats.peak_usage = new_total;
        }
    }

    /// Registra uma liberação
    pub fn record_free(&self, size: usize) {
        self.allocated
            .fetch_sub(size, std::sync::atomic::Ordering::SeqCst);
        let mut stats = self.stats.lock().unwrap();
        stats.deallocations += 1;
    }

    /// Registra falha de alocação
    pub fn record_allocation_failure(&self) {
        let mut stats = self.stats.lock().unwrap();
        stats.allocation_failures += 1;
    }

    /// Retorna estatísticas de memória
    pub fn stats(&self) -> MemoryStats {
        self.stats.lock().unwrap().clone()
    }

    /// Retorna memória atualmente alocada
    pub fn current_usage(&self) -> usize {
        self.allocated.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Retorna limite máximo de alocação
    pub fn max_allocation(&self) -> usize {
        self.max_allocation
    }

    /// Cria uma alocação registrada
    pub fn create_allocation(&self, offset: usize, size: usize) -> PluginResult<WasmAllocation> {
        if !self.can_allocate(size) {
            self.record_allocation_failure();
            return Err(PluginError::Execution {
                reason: format!(
                    "Allocation limit exceeded: current={} + size={} > max={}",
                    self.current_usage(),
                    size,
                    self.max_allocation
                ),
            });
        }

        self.record_allocation(size);

        Ok(WasmAllocation { offset, size })
    }

    /// Libera uma alocação
    pub fn release_allocation(&self, allocation: WasmAllocation) {
        self.record_free(allocation.size);
    }
}

impl Default for WasmMemoryManager {
    fn default() -> Self {
        Self::with_default_limit()
    }
}

/// Buffer compartilhado entre host e WASM
///
/// Gerencia um buffer de memória que pode ser acessado tanto pelo host
/// quanto pelo plugin WASM.
#[derive(Debug)]
pub struct SharedBuffer {
    allocation: WasmAllocation,
    manager: WasmMemoryManager,
}

impl SharedBuffer {
    /// Cria novo buffer compartilhado
    pub fn new(manager: WasmMemoryManager, offset: usize, size: usize) -> PluginResult<Self> {
        let allocation = manager.create_allocation(offset, size)?;

        Ok(Self {
            allocation,
            manager,
        })
    }

    /// Retorna tamanho do buffer
    pub fn size(&self) -> usize {
        self.allocation.size
    }

    /// Retorna offset na memória linear
    pub fn offset(&self) -> usize {
        self.allocation.offset
    }

    /// Retorna a alocação
    pub fn allocation(&self) -> WasmAllocation {
        self.allocation
    }

    /// Libera o buffer
    pub fn free(self) {
        self.manager.release_allocation(self.allocation);
    }
}

/// Pool de buffers reutilizáveis
#[derive(Debug)]
pub struct BufferPool {
    buffers: Vec<Vec<u8>>,
    buffer_size: usize,
    max_pool_size: usize,
}

impl BufferPool {
    /// Cria novo pool
    pub fn new(buffer_size: usize, max_pool_size: usize) -> Self {
        Self {
            buffers: Vec::with_capacity(max_pool_size),
            buffer_size,
            max_pool_size,
        }
    }

    /// Obtém um buffer do pool (ou cria novo)
    pub fn acquire(&mut self) -> Vec<u8> {
        self.buffers
            .pop()
            .unwrap_or_else(|| vec![0u8; self.buffer_size])
    }

    /// Retorna um buffer ao pool
    pub fn release(&mut self, mut buffer: Vec<u8>) {
        if self.buffers.len() < self.max_pool_size && buffer.len() == self.buffer_size {
            buffer.fill(0); // Limpa antes de reutilizar
            self.buffers.push(buffer);
        }
        // Caso contrário, deixa ser dropado
    }

    /// Retorna tamanho do pool atual
    pub fn size(&self) -> usize {
        self.buffers.len()
    }

    /// Limpa o pool
    pub fn clear(&mut self) {
        self.buffers.clear();
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new(4096, 16) // 4KB buffers, max 16
    }
}

/// Valida se um offset é válido para leitura na memória
pub fn validate_read_bounds(memory_size: usize, offset: usize, len: usize) -> PluginResult<()> {
    if offset >= memory_size {
        return Err(PluginError::Execution {
            reason: format!(
                "Read out of bounds: offset={} >= size={}",
                offset, memory_size
            ),
        });
    }

    if offset + len > memory_size {
        return Err(PluginError::Execution {
            reason: format!(
                "Read would exceed memory: offset={} + len={} > size={}",
                offset, len, memory_size
            ),
        });
    }

    Ok(())
}

/// Valida se um offset é válido para escrita na memória
pub fn validate_write_bounds(memory_size: usize, offset: usize, len: usize) -> PluginResult<()> {
    validate_read_bounds(memory_size, offset, len)
}

/// Informações sobre layout de memória linear WASM
#[derive(Debug, Clone, Copy)]
pub struct LinearMemoryLayout {
    /// Tamanho total da memória (bytes)
    pub size: usize,
    /// Tamanho de uma página (64KB)
    pub page_size: usize,
    /// Número de páginas atual
    pub pages: usize,
}

impl LinearMemoryLayout {
    /// Cria layout a partir de tamanho total
    pub fn from_size(size: usize) -> Self {
        const PAGE_SIZE: usize = 64 * 1024;
        Self {
            size,
            page_size: PAGE_SIZE,
            pages: size.div_ceil(PAGE_SIZE),
        }
    }

    /// Calcula offset seguro alinhado
    pub fn align_offset(&self, offset: usize, alignment: usize) -> usize {
        (offset + alignment - 1) & !(alignment - 1)
    }

    /// Verifica se offset está dentro dos limites
    pub fn is_valid(&self, offset: usize, len: usize) -> bool {
        offset + len <= self.size
    }
}

impl Default for LinearMemoryLayout {
    fn default() -> Self {
        Self::from_size(64 * 1024) // 1 página padrão
    }
}

/// Builder para configuração de memória
#[derive(Debug)]
pub struct MemoryConfigBuilder {
    max_allocation: usize,
    initial_pool_size: usize,
    buffer_size: usize,
}

impl MemoryConfigBuilder {
    /// Cria builder com valores padrão
    pub fn new() -> Self {
        Self {
            max_allocation: 16 * 1024 * 1024, // 16 MB
            initial_pool_size: 16,
            buffer_size: 4096, // 4 KB
        }
    }

    /// Define limite máximo de alocação
    pub fn max_allocation(mut self, bytes: usize) -> Self {
        self.max_allocation = bytes;
        self
    }

    /// Define tamanho inicial do pool
    pub fn initial_pool_size(mut self, count: usize) -> Self {
        self.initial_pool_size = count;
        self
    }

    /// Define tamanho dos buffers
    pub fn buffer_size(mut self, bytes: usize) -> Self {
        self.buffer_size = bytes;
        self
    }

    /// Constrói o gerenciador de memória
    pub fn build_manager(&self) -> WasmMemoryManager {
        WasmMemoryManager::new(self.max_allocation)
    }

    /// Constrói o pool de buffers
    pub fn build_pool(&self) -> BufferPool {
        BufferPool::new(self.buffer_size, self.initial_pool_size)
    }
}

impl Default for MemoryConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_manager_creation() {
        let manager = WasmMemoryManager::new(1024 * 1024);
        assert_eq!(manager.max_allocation(), 1024 * 1024);
        assert_eq!(manager.current_usage(), 0);
    }

    #[test]
    fn test_default_memory_manager() {
        let manager = WasmMemoryManager::with_default_limit();
        assert_eq!(manager.max_allocation(), 16 * 1024 * 1024);
    }

    #[test]
    fn test_memory_stats_default() {
        let stats = MemoryStats::default();
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.deallocations, 0);
        assert_eq!(stats.peak_usage, 0);
        assert_eq!(stats.allocation_failures, 0);
    }

    #[test]
    fn test_wasm_allocation() {
        let allocation = WasmAllocation {
            offset: 1024,
            size: 512,
        };
        assert_eq!(allocation.offset, 1024);
        assert_eq!(allocation.size, 512);
    }

    #[test]
    fn test_buffer_pool() {
        let mut pool = BufferPool::new(1024, 10);
        assert_eq!(pool.size(), 0);

        // Adquire buffer
        let buffer = pool.acquire();
        assert_eq!(buffer.len(), 1024);

        // Retorna ao pool
        pool.release(buffer);
        assert_eq!(pool.size(), 1);

        // Adquire novamente (deve vir do pool)
        let _ = pool.acquire();
        assert_eq!(pool.size(), 0);
    }

    #[test]
    fn test_buffer_pool_max_size() {
        let mut pool = BufferPool::new(100, 2);

        // Libera mais buffers que o máximo
        for _ in 0..5 {
            let buffer = vec![0u8; 100];
            pool.release(buffer);
        }

        // Pool deve ter no máximo 2
        assert_eq!(pool.size(), 2);
    }

    #[test]
    fn test_buffer_pool_wrong_size() {
        let mut pool = BufferPool::new(100, 10);

        // Tenta devolver buffer de tamanho errado
        let wrong_buffer = vec![0u8; 50];
        pool.release(wrong_buffer);

        // Não deve aceitar
        assert_eq!(pool.size(), 0);
    }

    #[test]
    fn test_buffer_pool_clear() {
        let mut pool = BufferPool::new(100, 10);

        for _ in 0..5 {
            pool.release(vec![0u8; 100]);
        }
        assert_eq!(pool.size(), 5);

        pool.clear();
        assert_eq!(pool.size(), 0);
    }

    #[test]
    fn test_validate_read_bounds_valid() {
        let result = validate_read_bounds(65536, 0, 100);
        assert!(result.is_ok());

        let result = validate_read_bounds(65536, 65000, 536);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_read_bounds_out_of_bounds() {
        let result = validate_read_bounds(65536, 65536, 100);
        assert!(result.is_err());

        let result = validate_read_bounds(65536, 65000, 1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_linear_memory_layout() {
        let layout = LinearMemoryLayout::from_size(128 * 1024);
        assert_eq!(layout.size, 128 * 1024);
        assert_eq!(layout.page_size, 64 * 1024);
        assert_eq!(layout.pages, 2);

        assert!(layout.is_valid(0, 100));
        assert!(layout.is_valid(128 * 1024 - 1, 1));
        assert!(!layout.is_valid(128 * 1024, 1));

        assert_eq!(layout.align_offset(0, 8), 0);
        assert_eq!(layout.align_offset(5, 8), 8);
        assert_eq!(layout.align_offset(16, 8), 16);
    }

    #[test]
    fn test_memory_config_builder() {
        let config = MemoryConfigBuilder::new()
            .max_allocation(32 * 1024 * 1024)
            .buffer_size(8192)
            .initial_pool_size(32);

        let manager = config.build_manager();
        assert_eq!(manager.max_allocation(), 32 * 1024 * 1024);

        let mut pool = config.build_pool();
        let buffer = pool.acquire();
        assert_eq!(buffer.len(), 8192);
    }

    #[test]
    fn test_allocation_tracking() {
        let manager = WasmMemoryManager::new(1024);

        // Cria alocação
        let allocation = manager.create_allocation(0, 100).unwrap();
        assert_eq!(manager.current_usage(), 100);

        let stats = manager.stats();
        assert_eq!(stats.allocations, 1);
        assert_eq!(stats.peak_usage, 100);

        // Libera
        manager.release_allocation(allocation);
        assert_eq!(manager.current_usage(), 0);

        let stats = manager.stats();
        assert_eq!(stats.deallocations, 1);
    }

    #[test]
    fn test_allocation_limit() {
        let manager = WasmMemoryManager::new(100);

        // Deve falhar - excede limite
        let result = manager.create_allocation(0, 200);
        assert!(result.is_err());

        let stats = manager.stats();
        assert_eq!(stats.allocation_failures, 1);
    }

    #[test]
    fn test_shared_buffer() {
        let manager = WasmMemoryManager::new(1024);
        let buffer = SharedBuffer::new(manager.clone(), 0, 256).unwrap();

        assert_eq!(buffer.size(), 256);
        assert_eq!(buffer.offset(), 0);
        assert_eq!(manager.current_usage(), 256);

        buffer.free();
        assert_eq!(manager.current_usage(), 0);
    }
}
