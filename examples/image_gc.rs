//! Image Garbage Collection Example
//!
//! Demonstra como usar o sistema de garbage collection para imagens
//! com execução em background e políticas de evicção configuráveis.

use code_terminal::image::{
    GcConfig, GcConfigBuilder, GcEvictionPolicy, Image, ImageCache, ImageFormat,
    ImageGarbageCollector, ImageId, ImageManager,
};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Image Garbage Collection Example ===\n");

    example_basic_gc().await?;
    example_config_builder().await?;
    example_with_manager().await?;

    Ok(())
}

async fn example_basic_gc() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Basic GC Usage ---");

    let cache = Arc::new(Mutex::new(ImageCache::with_limits(10, 10000)));

    {
        let cache_guard = cache.lock();
        cache_guard.store(create_test_image(1, 1000));
        cache_guard.store(create_test_image(2, 2000));
        cache_guard.store(create_test_image(3, 3000));
    }

    let config = GcConfig::debug();
    let mut gc = ImageGarbageCollector::new(cache.clone(), config);

    println!("Cache size before GC: {} images", cache.lock().len());

    gc.start()?;

    sleep(Duration::from_millis(100)).await;

    gc.force_collect()?;
    sleep(Duration::from_millis(100)).await;

    let stats = gc.stats();
    println!("GC Stats:");
    println!("  Cycles run: {}", stats.cycles_run);
    println!("  Images evicted: {}", stats.images_evicted);
    println!("  Bytes freed: {}", stats.bytes_freed);
    println!("  Avg cycle time: {:.2}ms", stats.avg_cycle_time_ms);

    gc.stop()?;

    println!("Cache size after GC: {} images\n", cache.lock().len());

    Ok(())
}

async fn example_config_builder() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Config Builder ---");

    let cache = Arc::new(Mutex::new(ImageCache::new()));

    let config = GcConfigBuilder::new()
        .interval(Duration::from_secs(15))
        .max_age(Duration::from_secs(120))
        .memory_threshold(0.7)
        .count_threshold(0.8)
        .eviction_policy(GcEvictionPolicy::Hybrid {
            lru_weight: 0.6,
            age_weight: 0.4,
        })
        .max_evict_per_cycle(5)
        .verbose(true)
        .build();

    println!("Custom GC Config:");
    println!("  Interval: {:?}", config.interval);
    println!("  Max age: {:?}", config.max_age);
    println!(
        "  Memory threshold: {:.0}%",
        config.memory_threshold * 100.0
    );
    println!("  Count threshold: {:.0}%", config.count_threshold * 100.0);

    let mut gc = ImageGarbageCollector::new(cache, config);
    gc.start()?;
    sleep(Duration::from_millis(50)).await;
    gc.stop()?;

    println!();

    Ok(())
}

async fn example_with_manager() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- Integration with ImageManager ---");

    let mut manager = ImageManager::new();

    manager.process_command(code_terminal::image::GraphicsCommand::Transmit {
        data: vec![0u8; 1000],
        format: ImageFormat::Png,
        width: 100,
        height: 100,
    })?;

    manager.process_command(code_terminal::image::GraphicsCommand::Transmit {
        data: vec![0u8; 2000],
        format: ImageFormat::Jpeg,
        width: 200,
        height: 200,
    })?;

    println!("Memory usage: {} bytes", manager.memory_usage());

    manager.garbage_collect(Duration::from_secs(0));

    println!("After GC: {} bytes\n", manager.memory_usage());

    Ok(())
}

fn create_test_image(id: u32, size: usize) -> Image {
    Image::new(ImageId(id), 100, 100, ImageFormat::Png, vec![0u8; size])
}
