//! Memory Pool - Pool de Buffers Reutilizáveis
//!
//! Este módulo implementa um sistema de pooling de memória para reduzir
//! alocações frequentes e melhorar a performance.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// Pool de buffers reutilizáveis
pub struct BufferPool {
    /// Buffers disponíveis por tamanho
    inner: Arc<Mutex<BufferPoolInner>>,
}

struct BufferPoolInner {
    /// Tamanho máximo de buffer a manter no pool
    max_buffer_size: usize,
    /// Número máximo de buffers por categoria
    max_buffers_per_category: usize,
    /// Pools por categoria de tamanho
    small_buffers: VecDeque<Vec<u8>>, // < 1KB
    medium_buffers: VecDeque<Vec<u8>>, // 1KB - 16KB
    large_buffers: VecDeque<Vec<u8>>,  // 16KB - 64KB
    xlarge_buffers: VecDeque<Vec<u8>>, // 64KB - 256KB
    /// Estatísticas
    stats: PoolStats,
}

/// Estatísticas do pool
#[derive(Clone, Copy, Debug, Default)]
pub struct PoolStats {
    pub allocations: u64,
    pub deallocations: u64,
    pub hits: u64,
    pub misses: u64,
    pub total_bytes_pooled: usize,
    pub total_bytes_allocated: u64,
}

impl BufferPool {
    /// Cria um novo pool com configuração padrão
    pub fn new() -> Self {
        Self::with_config(256 * 1024, 32) // 256KB max, 32 buffers por categoria
    }

    /// Cria um pool com configuração customizada
    pub fn with_config(max_buffer_size: usize, max_buffers: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(BufferPoolInner {
                max_buffer_size,
                max_buffers_per_category: max_buffers,
                small_buffers: VecDeque::with_capacity(max_buffers),
                medium_buffers: VecDeque::with_capacity(max_buffers),
                large_buffers: VecDeque::with_capacity(max_buffers),
                xlarge_buffers: VecDeque::with_capacity(max_buffers / 2),
                stats: PoolStats::default(),
            })),
        }
    }

    /// Obtém um buffer do pool ou aloca um novo
    pub fn acquire(&self, min_size: usize) -> PooledBuffer {
        let mut inner = self.inner.lock();
        inner.stats.allocations += 1;

        if min_size > inner.max_buffer_size {
            inner.stats.misses += 1;
            inner.stats.total_bytes_allocated += min_size as u64;
            return PooledBuffer::new(vec![0u8; min_size], self.inner.clone());
        }

        // Determina qual categoria usar
        let category = buffer_category(min_size);

        // Tenta obter do pool
        let buffer = match category {
            BufferCategory::Small => inner.small_buffers.pop_front(),
            BufferCategory::Medium => inner.medium_buffers.pop_front(),
            BufferCategory::Large => inner.large_buffers.pop_front(),
            BufferCategory::XLarge => inner.xlarge_buffers.pop_front(),
            BufferCategory::Huge => {
                // Buffers muito grandes não são guardados no pool
                inner.stats.misses += 1;
                return PooledBuffer::new(vec![0u8; min_size], self.inner.clone());
            }
        };

        if let Some(mut buffer) = buffer {
            inner.stats.hits += 1;
            inner.stats.total_bytes_pooled -= buffer.capacity();

            // Garante capacidade suficiente
            if buffer.len() < min_size {
                buffer.resize(min_size, 0);
            }

            PooledBuffer::new(buffer, self.inner.clone())
        } else {
            inner.stats.misses += 1;
            inner.stats.total_bytes_allocated += min_size as u64;
            PooledBuffer::new(vec![0u8; min_size], self.inner.clone())
        }
    }

    /// Obtém estatísticas do pool
    pub fn stats(&self) -> PoolStats {
        self.inner.lock().stats
    }

    /// Limpa buffers não usados do pool
    pub fn trim(&self) {
        let mut inner = self.inner.lock();
        let max_buffers_per_category = inner.max_buffers_per_category;

        while inner.small_buffers.len() > max_buffers_per_category / 2 {
            if let Some(buf) = inner.small_buffers.pop_back() {
                inner.stats.total_bytes_pooled -= buf.capacity();
            }
        }

        while inner.medium_buffers.len() > max_buffers_per_category / 2 {
            if let Some(buf) = inner.medium_buffers.pop_back() {
                inner.stats.total_bytes_pooled -= buf.capacity();
            }
        }

        while inner.large_buffers.len() > max_buffers_per_category / 2 {
            if let Some(buf) = inner.large_buffers.pop_back() {
                inner.stats.total_bytes_pooled -= buf.capacity();
            }
        }

        while inner.xlarge_buffers.len() > max_buffers_per_category / 4 {
            if let Some(buf) = inner.xlarge_buffers.pop_back() {
                inner.stats.total_bytes_pooled -= buf.capacity();
            }
        }
    }

    /// Retorna o número de buffers no pool
    pub fn pooled_count(&self) -> usize {
        let inner = self.inner.lock();
        inner.small_buffers.len()
            + inner.medium_buffers.len()
            + inner.large_buffers.len()
            + inner.xlarge_buffers.len()
    }

    /// Retorna o total de bytes no pool
    pub fn pooled_bytes(&self) -> usize {
        self.inner.lock().stats.total_bytes_pooled
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Categorias de buffer baseadas em tamanho
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BufferCategory {
    Small,  // < 1KB
    Medium, // 1KB - 16KB
    Large,  // 16KB - 64KB
    XLarge, // 64KB - 256KB
    Huge,   // > 256KB (não guardado no pool)
}

fn buffer_category(size: usize) -> BufferCategory {
    match size {
        0..=1024 => BufferCategory::Small,
        1025..=16384 => BufferCategory::Medium,
        16385..=65536 => BufferCategory::Large,
        65537..=262144 => BufferCategory::XLarge,
        _ => BufferCategory::Huge,
    }
}

/// Buffer que retorna automaticamente ao pool quando dropado
pub struct PooledBuffer {
    buffer: Option<Vec<u8>>,
    pool: Arc<Mutex<BufferPoolInner>>,
}

impl PooledBuffer {
    fn new(buffer: Vec<u8>, pool: Arc<Mutex<BufferPoolInner>>) -> Self {
        Self {
            buffer: Some(buffer),
            pool,
        }
    }

    /// Retorna uma referência ao buffer
    pub fn as_slice(&self) -> &[u8] {
        self.buffer.as_ref().unwrap().as_slice()
    }

    /// Retorna uma referência mutável ao buffer
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.buffer.as_mut().unwrap().as_mut_slice()
    }

    /// Retorna o tamanho do buffer
    pub fn len(&self) -> usize {
        self.buffer.as_ref().unwrap().len()
    }

    /// Verifica se o buffer está vazio
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Retorna a capacidade do buffer
    pub fn capacity(&self) -> usize {
        self.buffer.as_ref().unwrap().capacity()
    }

    /// Redimensiona o buffer
    pub fn resize(&mut self, new_len: usize, value: u8) {
        self.buffer.as_mut().unwrap().resize(new_len, value);
    }

    /// Converte para Vec<u8> (consome o buffer sem retornar ao pool)
    pub fn into_vec(mut self) -> Vec<u8> {
        self.buffer.take().unwrap()
    }

    /// Obtém uma referência ao buffer interno
    pub fn as_vec(&self) -> &Vec<u8> {
        self.buffer.as_ref().unwrap()
    }

    /// Obtém uma referência mutável ao buffer interno
    pub fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        self.buffer.as_mut().unwrap()
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        if let Some(mut buffer) = self.buffer.take() {
            let mut pool = self.pool.lock();

            // Só retorna ao pool se não for muito grande
            let category = buffer_category(buffer.capacity());
            let max_per_category = pool.max_buffers_per_category;
            let within_size_limit = buffer.capacity() <= pool.max_buffer_size;

            let should_return = within_size_limit
                && match category {
                    BufferCategory::Small => pool.small_buffers.len() < max_per_category,
                    BufferCategory::Medium => pool.medium_buffers.len() < max_per_category,
                    BufferCategory::Large => pool.large_buffers.len() < max_per_category,
                    BufferCategory::XLarge => pool.xlarge_buffers.len() < max_per_category / 2,
                    BufferCategory::Huge => false,
                };

            if should_return {
                buffer.clear(); // Zera o conteúdo mas mantém capacidade
                pool.stats.total_bytes_pooled += buffer.capacity();

                match category {
                    BufferCategory::Small => pool.small_buffers.push_back(buffer),
                    BufferCategory::Medium => pool.medium_buffers.push_back(buffer),
                    BufferCategory::Large => pool.large_buffers.push_back(buffer),
                    BufferCategory::XLarge => pool.xlarge_buffers.push_back(buffer),
                    BufferCategory::Huge => {}
                }
            } else {
                pool.stats.deallocations += 1;
            }
        }
    }
}

impl std::ops::Deref for PooledBuffer {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl std::ops::DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl AsRef<[u8]> for PooledBuffer {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for PooledBuffer {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

/// Pool global de buffers (lazy initialization)
use std::sync::LazyLock;

static GLOBAL_BUFFER_POOL: LazyLock<BufferPool> = LazyLock::new(BufferPool::new);

/// Obtém um buffer do pool global
pub fn acquire_buffer(min_size: usize) -> PooledBuffer {
    GLOBAL_BUFFER_POOL.acquire(min_size)
}

/// Retorna estatísticas do pool global
pub fn global_pool_stats() -> PoolStats {
    GLOBAL_BUFFER_POOL.stats()
}

/// Pool de objetos genérico
pub struct ObjectPool<T> {
    objects: Arc<Mutex<VecDeque<T>>>,
    max_size: usize,
    factory: Arc<dyn Fn() -> T + Send + Sync>,
    reset: Arc<dyn Fn(&mut T) + Send + Sync>,
}

impl<T> ObjectPool<T> {
    /// Cria um novo pool de objetos
    pub fn new(
        max_size: usize,
        factory: impl Fn() -> T + Send + Sync + 'static,
        reset: impl Fn(&mut T) + Send + Sync + 'static,
    ) -> Self {
        Self {
            objects: Arc::new(Mutex::new(VecDeque::with_capacity(max_size))),
            max_size,
            factory: Arc::new(factory),
            reset: Arc::new(reset),
        }
    }

    /// Obtém um objeto do pool
    pub fn acquire(&self) -> PooledObject<T> {
        let mut objects = self.objects.lock();

        if let Some(mut obj) = objects.pop_front() {
            (self.reset)(&mut obj);
            PooledObject::new(obj, self.objects.clone(), self.max_size, self.reset.clone())
        } else {
            PooledObject::new(
                (self.factory)(),
                self.objects.clone(),
                self.max_size,
                self.reset.clone(),
            )
        }
    }

    /// Retorna o número de objetos no pool
    pub fn len(&self) -> usize {
        self.objects.lock().len()
    }

    /// Verifica se o pool está vazio
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Limpa objetos do pool
    pub fn clear(&self) {
        self.objects.lock().clear();
    }
}

/// Objeto que retorna ao pool quando dropado
pub struct PooledObject<T> {
    object: Option<T>,
    pool: Arc<Mutex<VecDeque<T>>>,
    max_size: usize,
    reset: Arc<dyn Fn(&mut T) + Send + Sync>,
}

impl<T> PooledObject<T> {
    fn new(
        object: T,
        pool: Arc<Mutex<VecDeque<T>>>,
        max_size: usize,
        reset: Arc<dyn Fn(&mut T) + Send + Sync>,
    ) -> Self {
        Self {
            object: Some(object),
            pool,
            max_size,
            reset,
        }
    }

    /// Obtém uma referência ao objeto
    pub fn get(&self) -> &T {
        self.object.as_ref().unwrap()
    }

    /// Obtém uma referência mutável ao objeto
    pub fn get_mut(&mut self) -> &mut T {
        self.object.as_mut().unwrap()
    }

    /// Converte para o objeto interno (não retorna ao pool)
    pub fn into_inner(mut self) -> T {
        self.object.take().unwrap()
    }
}

impl<T> std::ops::Deref for PooledObject<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl<T> std::ops::DerefMut for PooledObject<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

impl<T> Drop for PooledObject<T> {
    fn drop(&mut self) {
        if let Some(mut object) = self.object.take() {
            let mut pool = self.pool.lock();
            if pool.len() < self.max_size {
                (self.reset)(&mut object);
                pool.push_back(object);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool_basic() {
        let pool = BufferPool::new();

        // Adquire um buffer
        let mut buf1 = pool.acquire(1024);
        assert_eq!(buf1.len(), 1024);
        buf1[0] = 42;

        // Drop retorna ao pool
        drop(buf1);

        // Próxima aquisição deve reusar (se hit)
        let buf2 = pool.acquire(1024);
        assert_eq!(buf2.len(), 1024);

        let stats = pool.stats();
        assert_eq!(stats.allocations, 2);
        assert_eq!(stats.hits, 1);
    }

    #[test]
    fn test_buffer_pool_categories() {
        let pool = BufferPool::new();

        // Diferentes tamanhos
        let small = pool.acquire(512);
        let medium = pool.acquire(8192);
        let large = pool.acquire(32768);

        assert_eq!(small.len(), 512);
        assert_eq!(medium.len(), 8192);
        assert_eq!(large.len(), 32768);

        drop(small);
        drop(medium);
        drop(large);

        // Verifica que estão em categorias diferentes
        let stats = pool.stats();
        assert_eq!(stats.allocations, 3);
    }

    #[test]
    fn test_pooled_buffer_resize() {
        let pool = BufferPool::new();

        let mut buf = pool.acquire(100);
        assert_eq!(buf.len(), 100);

        buf.resize(200, 0);
        assert_eq!(buf.len(), 200);
    }

    #[test]
    fn test_object_pool() {
        let pool = ObjectPool::new(10, || Vec::<u8>::with_capacity(100), |v| v.clear());

        {
            let mut obj = pool.acquire();
            obj.get_mut().push(1);
            obj.get_mut().push(2);
            assert_eq!(obj.get().len(), 2);
        }

        // Objeto deve ter sido retornado ao pool
        assert_eq!(pool.len(), 1);

        // Próxima aquisição deve reusar
        let obj2 = pool.acquire();
        assert!(obj2.get().is_empty()); // Foi resetado
    }

    #[test]
    fn test_global_pool() {
        let buf = acquire_buffer(4096);
        assert_eq!(buf.len(), 4096);
        drop(buf);

        let stats = global_pool_stats();
        assert!(stats.allocations > 0);
    }

    #[test]
    fn test_pool_trim() {
        let pool = BufferPool::with_config(1024 * 1024, 10);

        // Cria muitos buffers
        let mut buffers = Vec::new();
        for _ in 0..20 {
            buffers.push(pool.acquire(512));
        }

        // Libera todos
        drop(buffers);

        // Antes do trim, pode ter mais de 10
        let count_before = pool.pooled_count();

        // Trim reduz para metade do max
        pool.trim();

        let count_after = pool.pooled_count();
        assert!(count_after <= 5 || count_after <= count_before);
    }

    #[test]
    fn test_reuse_counts_hits_and_zeroes_contents() {
        let pool = BufferPool::default();
        let mut buf = pool.acquire(2000); // médio
        buf.as_mut_slice()[0] = 7;
        let capacity = buf.capacity();
        drop(buf);
        assert_eq!(pool.pooled_count(), 1);
        assert_eq!(pool.pooled_bytes(), capacity);

        let reused = pool.acquire(1500);
        assert_eq!(reused.len(), 1500);
        assert!(reused.iter().all(|b| *b == 0), "conteúdo antigo vazou");
        let stats = pool.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.allocations, 2);
        assert_eq!(pool.pooled_bytes(), 0);
    }

    #[test]
    fn test_every_category_round_trips() {
        let pool = BufferPool::new();
        for size in [10, 5_000, 40_000, 100_000] {
            drop(pool.acquire(size));
        }
        assert_eq!(pool.pooled_count(), 4);
        for size in [10, 5_000, 40_000, 100_000] {
            let buf = pool.acquire(size);
            assert_eq!(buf.len(), size);
        }
        assert_eq!(pool.stats().hits, 4);
    }

    #[test]
    fn test_oversized_buffers_are_not_pooled() {
        // Acima do limite do pool
        let pool = BufferPool::with_config(1024, 4);
        let big = pool.acquire(4096);
        assert_eq!(big.len(), 4096);
        drop(big);
        assert_eq!(pool.pooled_count(), 0);
        assert_eq!(pool.stats().deallocations, 1);
        assert_eq!(pool.stats().total_bytes_allocated, 4096);

        // Categoria Huge dentro de um limite grande
        let pool = BufferPool::with_config(10 * 1024 * 1024, 4);
        let huge = pool.acquire(300_000);
        drop(huge);
        assert_eq!(pool.pooled_count(), 0);
        assert_eq!(pool.stats().misses, 1);
    }

    #[test]
    fn test_category_limits_on_return() {
        let pool = BufferPool::with_config(256 * 1024, 2);
        let buffers: Vec<_> = (0..3).map(|_| pool.acquire(100)).collect();
        drop(buffers);
        assert_eq!(pool.pooled_count(), 2);
        assert_eq!(pool.stats().deallocations, 1);

        // XLarge guarda só metade do limite (1)
        let xl: Vec<_> = (0..2).map(|_| pool.acquire(100_000)).collect();
        drop(xl);
        assert_eq!(pool.pooled_count(), 3);
    }

    #[test]
    fn test_trim_reduces_each_category() {
        let pool = BufferPool::with_config(256 * 1024, 8);
        for size in [100, 5_000, 40_000] {
            let held: Vec<_> = (0..8).map(|_| pool.acquire(size)).collect();
            drop(held);
        }
        let xl: Vec<_> = (0..4).map(|_| pool.acquire(100_000)).collect();
        drop(xl);
        assert_eq!(pool.pooled_count(), 8 * 3 + 4);

        pool.trim();
        // metade por categoria (4) e um quarto para XLarge (2)
        assert_eq!(pool.pooled_count(), 4 * 3 + 2);
        let inner = pool.inner.lock();
        let expected: usize = inner
            .small_buffers
            .iter()
            .chain(&inner.medium_buffers)
            .chain(&inner.large_buffers)
            .chain(&inner.xlarge_buffers)
            .map(|b| b.capacity())
            .sum();
        assert_eq!(inner.stats.total_bytes_pooled, expected);
    }

    #[test]
    fn test_pooled_buffer_accessors() {
        let pool = BufferPool::new();
        let mut buf = pool.acquire(4);
        assert!(!buf.is_empty());
        buf.as_mut_vec().push(9);
        assert_eq!(buf.as_vec().len(), 5);
        let as_ref: &[u8] = buf.as_ref();
        assert_eq!(as_ref[4], 9);
        AsMut::<[u8]>::as_mut(&mut buf)[0] = 1;
        assert_eq!(buf[0], 1);

        // into_vec não devolve ao pool
        let vec = buf.into_vec();
        assert_eq!(vec, vec![1, 0, 0, 0, 9]);
        assert_eq!(pool.pooled_count(), 0);

        let empty = pool.acquire(0);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_global_pool_helpers() {
        let before = global_pool_stats().allocations;
        let buf = acquire_buffer(64);
        assert_eq!(buf.len(), 64);
        assert!(global_pool_stats().allocations > before);
    }

    #[test]
    fn test_object_pool_reset_limit_and_into_inner() {
        let created = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = created.clone();
        let pool = ObjectPool::new(
            1,
            move || {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Vec::<u8>::new()
            },
            |v: &mut Vec<u8>| v.clear(),
        );
        assert!(pool.is_empty());

        let mut a = pool.acquire();
        a.push(1);
        let mut b = pool.acquire();
        b.get_mut().push(2);
        assert_eq!(*b.get(), vec![2]);
        drop(a);
        drop(b); // pool cheio (max 1): descartado
        assert_eq!(pool.len(), 1);

        // Reaproveitado e resetado
        let reused = pool.acquire();
        assert!(reused.is_empty());
        assert_eq!(created.load(std::sync::atomic::Ordering::SeqCst), 2);

        let owned = reused.into_inner();
        assert!(owned.is_empty());
        assert!(pool.is_empty());

        drop(pool.acquire());
        pool.clear();
        assert!(pool.is_empty());
    }
}
