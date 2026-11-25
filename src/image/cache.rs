//! Image Cache
//!
//! Manages GPU textures and image data caching with configurable eviction
//! policies (LRU, LFU, FIFO, Random), memory limits, and performance metrics.

use super::renderer::ImageTexture;
use super::{Image, ImageId};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Cache entry with metadata
#[derive(Debug)]
pub struct ImageCacheEntry {
    pub image: Image,
    pub texture: Option<Arc<ImageTexture>>,
    pub access_count: u64,
    pub created_at: Instant,
    pub last_accessed: Instant,
    pub insertion_order: u64,
}

impl ImageCacheEntry {
    pub fn new(image: Image, insertion_order: u64) -> Self {
        let now = Instant::now();
        Self {
            image,
            texture: None,
            access_count: 0,
            created_at: now,
            last_accessed: now,
            insertion_order,
        }
    }

    pub fn with_texture(mut self, texture: Arc<ImageTexture>) -> Self {
        self.texture = Some(texture);
        self
    }

    pub fn touch(&mut self) {
        self.access_count += 1;
        self.last_accessed = Instant::now();
        self.image.touch();
    }

    pub fn score(&self, policy: EvictionPolicy, current_time: Instant) -> u64 {
        match policy {
            EvictionPolicy::Lru => {
                current_time.duration_since(self.last_accessed).as_millis() as u64
            }
            EvictionPolicy::Lfu => u64::MAX - self.access_count.min(u64::MAX - 1),
            EvictionPolicy::Fifo => self.insertion_order,
            EvictionPolicy::Random => 0,
        }
    }
}

/// Cache eviction policy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EvictionPolicy {
    #[default]
    Lru,
    Lfu,
    Fifo,
    Random,
}

impl EvictionPolicy {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "lru" => Some(Self::Lru),
            "lfu" => Some(Self::Lfu),
            "fifo" => Some(Self::Fifo),
            "random" => Some(Self::Random),
            _ => None,
        }
    }
}

/// Cache configuration
#[derive(Debug, Clone)]
pub struct CacheConfig {
    pub max_images: usize,
    pub max_memory: usize,
    pub default_ttl: Duration,
    pub eviction_policy: EvictionPolicy,
    pub enable_textures: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_images: 100,
            max_memory: 100 * 1024 * 1024,
            default_ttl: Duration::from_secs(300),
            eviction_policy: EvictionPolicy::Lru,
            enable_textures: true,
        }
    }
}

/// Cache performance metrics
#[derive(Debug, Clone, Default)]
pub struct CacheMetrics {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub inserts: u64,
    pub removes: u64,
    pub texture_uploads: u64,
    pub texture_evictions: u64,
}

impl CacheMetrics {
    pub fn hit_ratio(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0.0;
        }
        self.hits as f64 / total as f64
    }

    pub fn miss_ratio(&self) -> f64 {
        1.0 - self.hit_ratio()
    }

    pub fn total_requests(&self) -> u64 {
        self.hits + self.misses
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub image_count: usize,
    pub texture_count: usize,
    pub total_memory: usize,
    pub max_memory: usize,
    pub max_images: usize,
    pub metrics: CacheMetrics,
    pub config: CacheConfig,
}

impl CacheStats {
    pub fn memory_usage_percent(&self) -> f32 {
        if self.max_memory == 0 {
            return 0.0;
        }
        (self.total_memory as f32 / self.max_memory as f32) * 100.0
    }

    pub fn image_count_percent(&self) -> f32 {
        if self.max_images == 0 {
            return 0.0;
        }
        (self.image_count as f32 / self.max_images as f32) * 100.0
    }
}

/// Thread-safe image cache with configurable eviction
pub struct ImageCache {
    entries: RwLock<HashMap<ImageId, ImageCacheEntry>>,
    config: RwLock<CacheConfig>,
    current_memory: RwLock<usize>,
    next_insertion_order: AtomicU64,
    metrics: RwLock<CacheMetrics>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self::with_config(CacheConfig::default())
    }

    pub fn with_limits(max_images: usize, max_memory: usize) -> Self {
        Self::with_config(CacheConfig {
            max_images,
            max_memory,
            ..Default::default()
        })
    }

    pub fn with_config(config: CacheConfig) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            config: RwLock::new(config),
            current_memory: RwLock::new(0),
            next_insertion_order: AtomicU64::new(0),
            metrics: RwLock::new(CacheMetrics::default()),
        }
    }

    fn next_insertion_order(&self) -> u64 {
        self.next_insertion_order.fetch_add(1, Ordering::Relaxed)
    }

    pub fn store(&self, image: Image) {
        self.store_with_texture(image, None)
    }

    pub fn store_with_texture(&self, image: Image, texture: Option<Arc<ImageTexture>>) {
        let size = image.size_bytes();
        let id = image.id;
        let order = self.next_insertion_order();

        {
            let mut entries = self.entries.write();
            let mut current_memory = self.current_memory.write();

            loop {
                let (max_memory, max_images) = {
                    let config = self.config.read();
                    (config.max_memory, config.max_images)
                };

                if *current_memory + size <= max_memory && entries.len() < max_images {
                    break;
                }

                if !self.evict_one_internal(&mut entries, &mut current_memory) {
                    break;
                }
            }

            let entry = ImageCacheEntry::new(image, order);
            let entry = if let Some(tex) = texture {
                self.metrics.write().texture_uploads += 1;
                entry.with_texture(tex)
            } else {
                entry
            };

            if let Some(old) = entries.insert(id, entry) {
                *current_memory -= old.image.size_bytes();
            }

            *current_memory += size;
            self.metrics.write().inserts += 1;
        }
    }

    pub fn get(&self, id: ImageId) -> Option<Image> {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(&id) {
            entry.touch();
            self.metrics.write().hits += 1;
            Some(entry.image.clone())
        } else {
            self.metrics.write().misses += 1;
            None
        }
    }

    pub fn get_entry(&self, id: ImageId) -> Option<ImageCacheEntry> {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(&id) {
            entry.touch();
            self.metrics.write().hits += 1;
            Some(ImageCacheEntry {
                image: entry.image.clone(),
                texture: entry.texture.clone(),
                access_count: entry.access_count,
                created_at: entry.created_at,
                last_accessed: entry.last_accessed,
                insertion_order: entry.insertion_order,
            })
        } else {
            self.metrics.write().misses += 1;
            None
        }
    }

    pub fn get_texture(&self, id: ImageId) -> Option<Arc<ImageTexture>> {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(&id) {
            entry.touch();
            self.metrics.write().hits += 1;
            entry.texture.clone()
        } else {
            self.metrics.write().misses += 1;
            None
        }
    }

    pub fn set_texture(&self, id: ImageId, texture: Arc<ImageTexture>) -> bool {
        let mut entries = self.entries.write();
        if let Some(entry) = entries.get_mut(&id) {
            entry.texture = Some(texture);
            self.metrics.write().texture_uploads += 1;
            true
        } else {
            false
        }
    }

    pub fn contains(&self, id: ImageId) -> bool {
        self.entries.read().contains_key(&id)
    }

    pub fn remove(&self, id: ImageId) -> Option<Image> {
        let mut entries = self.entries.write();
        let mut current_memory = self.current_memory.write();

        entries.remove(&id).map(|entry| {
            *current_memory -= entry.image.size_bytes();
            if entry.texture.is_some() {
                self.metrics.write().texture_evictions += 1;
            }
            self.metrics.write().removes += 1;
            entry.image
        })
    }

    pub fn clear(&self) {
        let mut entries = self.entries.write();
        let mut current_memory = self.current_memory.write();

        let texture_count = entries.values().filter(|e| e.texture.is_some()).count();
        self.metrics.write().texture_evictions += texture_count as u64;

        entries.clear();
        *current_memory = 0;
    }

    pub fn remove_old(&self, max_age: Duration) {
        let now = Instant::now();
        let to_remove: Vec<ImageId> = {
            let entries = self.entries.read();
            entries
                .iter()
                .filter(|(_, entry)| now.duration_since(entry.last_accessed) > max_age)
                .map(|(id, _)| *id)
                .collect()
        };

        for id in to_remove {
            self.remove(id);
        }
    }

    fn evict_one_internal(
        &self,
        entries: &mut HashMap<ImageId, ImageCacheEntry>,
        current_memory: &mut usize,
    ) -> bool {
        if entries.is_empty() {
            return false;
        }

        let config = self.config.read();
        let eviction_policy = config.eviction_policy;
        drop(config);

        let victim_id = match eviction_policy {
            EvictionPolicy::Random => {
                use rand::seq::IteratorRandom;
                entries.keys().choose(&mut rand::thread_rng()).copied()
            }
            EvictionPolicy::Fifo => {
                // FIFO: remove oldest (smallest insertion_order)
                entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.insertion_order)
                    .map(|(id, _)| *id)
            }
            EvictionPolicy::Lru => {
                // LRU: remove least recently used (smallest last_accessed time)
                entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_accessed)
                    .map(|(id, _)| *id)
            }
            EvictionPolicy::Lfu => {
                // LFU: remove least frequently used (smallest access_count)
                entries
                    .iter()
                    .min_by_key(|(_, entry)| entry.access_count)
                    .map(|(id, _)| *id)
            }
        };

        if let Some(id) = victim_id {
            if let Some(entry) = entries.remove(&id) {
                *current_memory -= entry.image.size_bytes();
                if entry.texture.is_some() {
                    self.metrics.write().texture_evictions += 1;
                }
                self.metrics.write().evictions += 1;
                return true;
            }
        }

        false
    }

    pub fn evict_one(&self) -> bool {
        let mut entries = self.entries.write();
        let mut current_memory = self.current_memory.write();
        self.evict_one_internal(&mut entries, &mut current_memory)
    }

    pub fn stats(&self) -> CacheStats {
        let entries = self.entries.read();
        let current_memory = self.current_memory.read();
        let metrics = self.metrics.read();
        let config = self.config.read();

        let texture_count = entries.values().filter(|e| e.texture.is_some()).count();

        CacheStats {
            image_count: entries.len(),
            texture_count,
            total_memory: *current_memory,
            max_memory: config.max_memory,
            max_images: config.max_images,
            metrics: metrics.clone(),
            config: config.clone(),
        }
    }

    pub fn total_size(&self) -> usize {
        *self.current_memory.read()
    }

    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    pub fn image_ids(&self) -> Vec<ImageId> {
        self.entries.read().keys().copied().collect()
    }

    /// `(id, last_accessed, created_at)` de cada entrada, sem registrar
    /// acesso: não altera a ordem LRU nem as métricas de hit/miss.
    pub fn access_times(&self) -> Vec<(ImageId, Instant, Instant)> {
        self.entries
            .read()
            .iter()
            .map(|(id, entry)| (*id, entry.last_accessed, entry.created_at))
            .collect()
    }

    pub fn metrics(&self) -> CacheMetrics {
        self.metrics.read().clone()
    }

    pub fn reset_metrics(&self) {
        *self.metrics.write() = CacheMetrics::default();
    }

    pub fn config(&self) -> CacheConfig {
        self.config.read().clone()
    }

    pub fn resize(&self, max_images: usize, max_memory: usize) {
        let current_config = self.config.read();
        let config = CacheConfig {
            max_images,
            max_memory,
            ..current_config.clone()
        };
        drop(current_config);
        self.resize_with_config(config);
    }

    pub fn resize_with_config(&self, config: CacheConfig) {
        let mut entries = self.entries.write();
        let mut current_memory = self.current_memory.write();

        while entries.len() > config.max_images || *current_memory > config.max_memory {
            if !self.evict_one_internal(&mut entries, &mut current_memory) {
                break;
            }
        }

        *self.config.write() = config;
    }

    pub fn set_eviction_policy(&self, policy: EvictionPolicy) {
        let mut config = self.config.write();
        config.eviction_policy = policy;
    }
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::ImageFormat;

    fn create_test_image(id: u32, size: usize) -> Image {
        Image::new(ImageId(id), 100, 100, ImageFormat::Png, vec![0u8; size])
    }

    #[test]
    fn test_cache_store_and_get() {
        let cache = ImageCache::new();
        let image = create_test_image(1, 1024);

        cache.store(image);

        assert_eq!(cache.len(), 1);
        assert!(cache.contains(ImageId(1)));

        let retrieved = cache.get(ImageId(1));
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id.0, 1);

        let metrics = cache.metrics();
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.inserts, 1);
    }

    #[test]
    fn test_cache_miss() {
        let cache = ImageCache::new();

        let result = cache.get(ImageId(999));
        assert!(result.is_none());

        let metrics = cache.metrics();
        assert_eq!(metrics.misses, 1);
    }

    #[test]
    fn test_cache_remove() {
        let cache = ImageCache::new();
        let image = create_test_image(1, 1024);

        cache.store(image);
        assert_eq!(cache.len(), 1);

        let removed = cache.remove(ImageId(1));
        assert!(removed.is_some());
        assert_eq!(cache.len(), 0);
        assert!(!cache.contains(ImageId(1)));
    }

    #[test]
    fn test_cache_clear() {
        let cache = ImageCache::new();

        cache.store(create_test_image(1, 1024));
        cache.store(create_test_image(2, 1024));
        cache.store(create_test_image(3, 1024));

        assert_eq!(cache.len(), 3);

        cache.clear();
        assert_eq!(cache.len(), 0);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cache_memory_limit() {
        let cache = ImageCache::with_limits(100, 2000);

        cache.store(create_test_image(1, 1024));
        cache.store(create_test_image(2, 1024));

        assert!(!cache.contains(ImageId(1)));
        assert!(cache.contains(ImageId(2)));

        let metrics = cache.metrics();
        assert!(metrics.evictions >= 1);
    }

    #[test]
    fn test_cache_count_limit() {
        let cache = ImageCache::with_limits(2, 100 * 1024 * 1024);

        cache.store(create_test_image(1, 100));
        cache.store(create_test_image(2, 100));
        cache.store(create_test_image(3, 100));

        assert_eq!(cache.len(), 2);
        assert!(!cache.contains(ImageId(1)));
        assert!(cache.contains(ImageId(2)));
        assert!(cache.contains(ImageId(3)));
    }

    #[test]
    fn test_cache_lru_eviction() {
        let cache = ImageCache::with_config(CacheConfig {
            max_images: 2,
            max_memory: 100 * 1024 * 1024,
            eviction_policy: EvictionPolicy::Lru,
            ..Default::default()
        });

        cache.store(create_test_image(1, 100));
        cache.store(create_test_image(2, 100));

        std::thread::sleep(Duration::from_millis(5));
        cache.get(ImageId(1));

        std::thread::sleep(Duration::from_millis(5));
        cache.store(create_test_image(3, 100));

        assert!(cache.contains(ImageId(1)));
        assert!(!cache.contains(ImageId(2)));
        assert!(cache.contains(ImageId(3)));
    }

    #[test]
    fn test_cache_lfu_eviction() {
        let cache = ImageCache::with_config(CacheConfig {
            max_images: 2,
            max_memory: 100 * 1024 * 1024,
            eviction_policy: EvictionPolicy::Lfu,
            ..Default::default()
        });

        cache.store(create_test_image(1, 100));
        cache.store(create_test_image(2, 100));

        cache.get(ImageId(1));
        cache.get(ImageId(1));
        cache.get(ImageId(2));

        cache.store(create_test_image(3, 100));

        assert!(cache.contains(ImageId(1)));
        assert!(!cache.contains(ImageId(2)));
        assert!(cache.contains(ImageId(3)));
    }

    #[test]
    fn test_cache_fifo_eviction() {
        let cache = ImageCache::with_config(CacheConfig {
            max_images: 2,
            max_memory: 100 * 1024 * 1024,
            eviction_policy: EvictionPolicy::Fifo,
            ..Default::default()
        });

        cache.store(create_test_image(1, 100));
        cache.store(create_test_image(2, 100));

        cache.get(ImageId(1));

        cache.store(create_test_image(3, 100));

        assert!(!cache.contains(ImageId(1)));
        assert!(cache.contains(ImageId(2)));
        assert!(cache.contains(ImageId(3)));
    }

    #[test]
    fn test_cache_stats() {
        let cache = ImageCache::with_limits(10, 10000);

        cache.store(create_test_image(1, 1000));
        cache.store(create_test_image(2, 2000));

        let stats = cache.stats();
        assert_eq!(stats.image_count, 2);
        assert_eq!(stats.total_memory, 3000);
        assert_eq!(stats.max_memory, 10000);
        assert_eq!(stats.max_images, 10);

        assert!((stats.memory_usage_percent() - 30.0).abs() < 0.1);
        assert!((stats.image_count_percent() - 20.0).abs() < 0.1);
    }

    #[test]
    fn test_cache_metrics() {
        let cache = ImageCache::new();

        cache.store(create_test_image(1, 100));

        cache.get(ImageId(1));
        cache.get(ImageId(1));
        cache.get(ImageId(999));

        let metrics = cache.metrics();
        assert_eq!(metrics.hits, 2);
        assert_eq!(metrics.misses, 1);
        assert!((metrics.hit_ratio() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_remove_old() {
        let cache = ImageCache::new();

        cache.store(create_test_image(1, 100));
        cache.store(create_test_image(2, 100));

        std::thread::sleep(Duration::from_millis(10));
        cache.get(ImageId(1));

        cache.remove_old(Duration::from_millis(5));

        assert!(cache.contains(ImageId(1)));
        assert!(!cache.contains(ImageId(2)));
    }

    #[test]
    fn test_eviction_policy_from_str() {
        assert_eq!(EvictionPolicy::from_str("lru"), Some(EvictionPolicy::Lru));
        assert_eq!(EvictionPolicy::from_str("LFU"), Some(EvictionPolicy::Lfu));
        assert_eq!(EvictionPolicy::from_str("fifo"), Some(EvictionPolicy::Fifo));
        assert_eq!(
            EvictionPolicy::from_str("random"),
            Some(EvictionPolicy::Random)
        );
        assert_eq!(EvictionPolicy::from_str("unknown"), None);
    }

    #[test]
    fn test_cache_entry_score() {
        use std::thread;

        let entry_lru = ImageCacheEntry::new(create_test_image(1, 100), 1);
        thread::sleep(Duration::from_millis(10));
        let now = Instant::now();

        let lru_score = entry_lru.score(EvictionPolicy::Lru, now);
        assert!(lru_score >= 10);

        let lfu_score = entry_lru.score(EvictionPolicy::Lfu, now);
        assert_eq!(lfu_score, u64::MAX);

        let fifo_score = entry_lru.score(EvictionPolicy::Fifo, now);
        assert_eq!(fifo_score, 1);
    }

    #[test]
    fn test_reset_metrics() {
        let cache = ImageCache::new();

        cache.store(create_test_image(1, 100));
        cache.get(ImageId(1));
        cache.get(ImageId(999));

        let metrics = cache.metrics();
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.misses, 1);

        cache.reset_metrics();

        let metrics = cache.metrics();
        assert_eq!(metrics.hits, 0);
        assert_eq!(metrics.misses, 0);
    }

    use crate::image::renderer::test_support::test_texture;

    #[test]
    fn test_textures_are_tracked_in_metrics() {
        let Some(texture) = test_texture(ImageId(1)) else {
            eprintln!("sem adaptador wgpu: teste de textura pulado");
            return;
        };
        let cache = ImageCache::new();

        cache.store_with_texture(create_test_image(1, 10), Some(texture.clone()));
        assert_eq!(cache.metrics().texture_uploads, 1);
        assert_eq!(cache.stats().texture_count, 1);
        assert!(cache.get_texture(ImageId(1)).is_some());

        let entry = cache.get_entry(ImageId(1)).unwrap();
        assert!(entry.texture.is_some());
        assert_eq!(entry.access_count, 2);

        // set_texture só funciona em imagem existente
        cache.store(create_test_image(2, 10));
        assert!(cache.set_texture(ImageId(2), texture.clone()));
        assert!(!cache.set_texture(ImageId(99), texture.clone()));
        assert_eq!(cache.metrics().texture_uploads, 2);

        // Remover/limpar contabiliza texturas descartadas
        cache.remove(ImageId(1));
        assert_eq!(cache.metrics().texture_evictions, 1);
        cache.clear();
        assert_eq!(cache.metrics().texture_evictions, 2);

        // Evicção também
        let small = ImageCache::with_limits(1, usize::MAX);
        small.store_with_texture(create_test_image(1, 1), Some(texture.clone()));
        small.store(create_test_image(2, 1));
        assert_eq!(small.metrics().texture_evictions, 1);
        assert_eq!(small.metrics().evictions, 1);

        let entry = ImageCacheEntry::new(create_test_image(5, 1), 0).with_texture(texture);
        assert!(entry.texture.is_some());
    }

    #[test]
    fn test_get_entry_and_texture_miss() {
        let cache = ImageCache::default();
        assert!(cache.get_entry(ImageId(1)).is_none());
        assert!(cache.get_texture(ImageId(1)).is_none());
        cache.store(create_test_image(1, 4));
        assert!(cache.get_texture(ImageId(1)).is_none()); // sem textura
        let metrics = cache.metrics();
        assert_eq!(metrics.misses, 2);
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.total_requests(), 3);
        assert!((metrics.hit_ratio() - 1.0 / 3.0).abs() < 1e-9);
        assert!((metrics.miss_ratio() - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(CacheMetrics::default().hit_ratio(), 0.0);
        assert_eq!(CacheMetrics::default().miss_ratio(), 1.0);
    }

    #[test]
    fn test_access_times_do_not_count_as_access() {
        let cache = ImageCache::new();
        cache.store(create_test_image(1, 4));
        cache.store(create_test_image(2, 4));
        let mut times = cache.access_times();
        times.sort_by_key(|(id, _, _)| id.0);
        assert_eq!(times.len(), 2);
        assert_eq!(times[0].0, ImageId(1));
        assert_eq!(cache.metrics().total_requests(), 0);
    }

    #[test]
    fn test_restore_same_id_replaces_and_keeps_memory_consistent() {
        let cache = ImageCache::new();
        cache.store(create_test_image(1, 100));
        cache.store(create_test_image(1, 40));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.total_size(), 40);
    }

    #[test]
    fn test_resize_evicts_and_updates_config() {
        let cache = ImageCache::with_limits(10, 10_000);
        cache.set_eviction_policy(EvictionPolicy::Fifo);
        for i in 1..=5 {
            cache.store(create_test_image(i, 100));
        }
        cache.resize(2, 10_000);
        assert_eq!(cache.len(), 2);
        // FIFO: sobram as duas últimas
        assert!(cache.contains(ImageId(4)) && cache.contains(ImageId(5)));
        let config = cache.config();
        assert_eq!(config.max_images, 2);
        assert_eq!(config.eviction_policy, EvictionPolicy::Fifo);

        // Limite de memória também dispara evicção
        cache.resize(10, 150);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.metrics().evictions, 4);
    }

    #[test]
    fn test_evict_one_and_random_policy() {
        let cache = ImageCache::new();
        assert!(!cache.evict_one());
        cache.set_eviction_policy(EvictionPolicy::Random);
        cache.store(create_test_image(1, 1));
        cache.store(create_test_image(2, 1));
        assert!(cache.evict_one());
        assert_eq!(cache.len(), 1);
        assert!(cache.evict_one());
        assert!(cache.is_empty());
        assert_eq!(cache.total_size(), 0);
    }

    #[test]
    fn test_lfu_evicts_least_used_and_scores() {
        let cache = ImageCache::with_limits(2, usize::MAX);
        cache.set_eviction_policy(EvictionPolicy::Lfu);
        cache.store(create_test_image(1, 1));
        cache.store(create_test_image(2, 1));
        cache.get(ImageId(1));
        cache.get(ImageId(1));
        cache.store(create_test_image(3, 1));
        assert!(cache.contains(ImageId(1)));
        assert!(!cache.contains(ImageId(2)));

        let mut entry = ImageCacheEntry::new(create_test_image(9, 1), 7);
        let now = Instant::now();
        assert_eq!(entry.score(EvictionPolicy::Fifo, now), 7);
        assert_eq!(entry.score(EvictionPolicy::Random, now), 0);
        let fresh = entry.score(EvictionPolicy::Lfu, now);
        entry.touch();
        assert_eq!(entry.score(EvictionPolicy::Lfu, now), fresh - 1);
    }

    #[test]
    fn test_stats_percentages_with_zero_limits() {
        let cache = ImageCache::with_limits(0, 0);
        let stats = cache.stats();
        assert_eq!(stats.memory_usage_percent(), 0.0);
        assert_eq!(stats.image_count_percent(), 0.0);

        let cache = ImageCache::with_limits(4, 1000);
        cache.store(create_test_image(1, 250));
        let stats = cache.stats();
        assert_eq!(stats.memory_usage_percent(), 25.0);
        assert_eq!(stats.image_count_percent(), 25.0);
    }
}
