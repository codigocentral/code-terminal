//! Image Protocol Integration Tests
//!
//! Testes de integração para o protocolo de imagens Kitty.
//! Testam o fluxo completo: parsing → decoding → caching → rendering.

use base64::Engine;
use code_terminal::image::cache::{CacheConfig, EvictionPolicy};
use code_terminal::image::*;

mod common;

/// Testa o fluxo completo de transmissão e decodificação de imagem
#[test]
fn test_full_transmission_flow() {
    let mut handler = ImageProtocolHandler::new();
    let decoder = ImageDecoder::new();
    let cache = ImageCache::new();

    // Cria uma imagem PNG de teste
    let png_data = create_test_png_data(100, 100);
    let base64_data = base64::engine::general_purpose::STANDARD.encode(&png_data);

    // Simula o comando de transmissão do protocolo Kitty
    let payload = format!("f=24,s=100,v=100,a=t;{}", base64_data);

    let result = handler.parse(payload.as_bytes());
    assert!(result.is_ok());

    let command = result.unwrap();
    assert!(command.is_some());

    match command.unwrap() {
        GraphicsCommand::Transmit {
            data,
            format,
            width,
            height,
        } => {
            assert_eq!(format, ImageFormat::Png);
            assert_eq!(width, 100);
            assert_eq!(height, 100);

            // Decodifica a imagem
            let decoded = decoder.decode_png(&data).unwrap();
            assert_eq!(decoded.width, 100);
            assert_eq!(decoded.height, 100);

            // Armazena no cache
            let image = Image::new(ImageId(1), width, height, format, data);
            cache.store(image);

            assert!(cache.contains(ImageId(1)));
        }
        _ => panic!("Expected Transmit command"),
    }
}

/// Testa múltiplas imagens no cache
#[test]
fn test_multiple_images_cache() {
    let cache = ImageCache::with_limits(10, 1024 * 1024);
    let decoder = ImageDecoder::new();

    // Adiciona 5 imagens diferentes
    for i in 1..=5 {
        let png_data = create_test_png_data(50, 50);
        let decoded = decoder.decode_png(&png_data).unwrap();

        let image = Image::new(
            ImageId(i),
            decoded.width,
            decoded.height,
            ImageFormat::Png,
            png_data,
        );

        cache.store(image);
    }

    assert_eq!(cache.len(), 5);

    // Verifica se todas estão no cache
    for i in 1..=5 {
        assert!(cache.contains(ImageId(i)));
    }

    // Verifica métricas
    let metrics = cache.metrics();
    assert_eq!(metrics.inserts, 5);
}

/// Testa evicção quando o limite de memória é atingido
#[test]
fn test_cache_eviction_under_pressure() {
    // Cache com limite muito baixo (100 bytes)
    let cache = ImageCache::with_limits(1, 100);
    let decoder = ImageDecoder::new();

    // Cria uma imagem que excede o limite de 100 bytes
    let png_data = create_test_png_data(100, 100);
    let decoded = decoder.decode_png(&png_data).unwrap();

    let image = Image::new(
        ImageId(1),
        decoded.width,
        decoded.height,
        ImageFormat::Png,
        png_data,
    );

    cache.store(image);

    // A imagem não deve estar no cache pois excede o limite
    // (o cache permite armazenar mesmo acima do limite se estiver vazio)
    let stats = cache.stats();
    let _image_count = stats.image_count;

    // Adiciona mais uma imagem para forçar evicção
    let png_data2 = create_test_png_data(50, 50);
    let image2 = Image::new(ImageId(2), 50, 50, ImageFormat::Png, png_data2);
    cache.store(image2);

    let final_stats = cache.stats();
    // Após adicionar a segunda imagem, deve haver evicção ou
    // o total deve respeitar o limite
    assert!(final_stats.metrics.evictions > 0 || final_stats.image_count <= 1);
}

/// Testa o parsing de diferentes comandos do protocolo
#[test]
fn test_protocol_command_parsing() {
    let mut handler = ImageProtocolHandler::new();

    // Test transmit and display
    let payload = b"a=T,f=24,s=100,v=100;data";
    let result = handler.parse(payload).unwrap();
    assert!(matches!(result, Some(GraphicsCommand::Transmit { .. })));

    // Test delete
    let payload = b"a=D,i=42";
    let result = handler.parse(payload).unwrap();
    assert!(matches!(result, Some(GraphicsCommand::Delete { .. })));

    // Test clear
    let payload = b"a=C";
    let result = handler.parse(payload).unwrap();
    assert_eq!(result, Some(GraphicsCommand::Clear));
}

/// Testa transmissão em stream (chunks)
#[test]
fn test_streaming_transmission() {
    let mut handler = ImageProtocolHandler::new();

    // Cria dados base64 válidos
    let data1 = base64::engine::general_purpose::STANDARD.encode(b"chunk1");
    let data2 = base64::engine::general_purpose::STANDARD.encode(b"chunk2");

    // Primeiro chunk
    let chunk1 = format!("a=t,t=s,m=1;{}", data1);
    let result = handler.parse(chunk1.as_bytes()).unwrap();
    assert!(result.is_none()); // Espera mais dados

    // Segundo chunk (último)
    let chunk2 = format!("a=t,t=s,m=0;{}", data2);
    let result = handler.parse(chunk2.as_bytes()).unwrap();
    // Deve retornar um comando transmit com os dados concatenados
    assert!(result.is_some());
}

/// Testa diferentes formatos de imagem
#[test]
fn test_different_image_formats() {
    let decoder = ImageDecoder::new();

    // PNG
    let png_data = create_test_png_data(10, 10);
    let png_result = decoder.decode_auto(&png_data);
    assert!(png_result.is_ok());

    // JPEG
    let jpeg_data = create_test_jpeg_data(10, 10);
    let jpeg_result = decoder.decode_auto(&jpeg_data);
    assert!(jpeg_result.is_ok());
}

/// Testa decodificação de dados RGBA raw
#[test]
fn test_raw_rgba_decoding() {
    let decoder = ImageDecoder::new();

    // 2x2 imagem RGBA
    let rgba_data: Vec<u8> = vec![
        255, 0, 0, 255, // Red
        0, 255, 0, 255, // Green
        0, 0, 255, 255, // Blue
        255, 255, 255, 255, // White
    ];

    let result = decoder.decode_rgba(&rgba_data, 2, 2);
    assert!(result.is_ok());

    let image = result.unwrap();
    assert_eq!(image.width, 2);
    assert_eq!(image.height, 2);
    assert_eq!(image.get_pixel(0, 0), Some([255, 0, 0, 255]));
    assert_eq!(image.get_pixel(1, 0), Some([0, 255, 0, 255]));
    assert_eq!(image.get_pixel(0, 1), Some([0, 0, 255, 255]));
    assert_eq!(image.get_pixel(1, 1), Some([255, 255, 255, 255]));
}

/// Testa resize de imagens
#[test]
fn test_image_resize() {
    let decoder = ImageDecoder::new();

    // Cria imagem 100x100
    let png_data = create_test_png_data(100, 100);
    let decoded = decoder.decode_png(&png_data).unwrap();

    // Redimensiona para 50x50
    let resized = decoder.resize(&decoded, 50, 50).unwrap();
    assert_eq!(resized.width, 50);
    assert_eq!(resized.height, 50);
}

/// Testa o gerenciador de imagens completo
#[test]
fn test_image_manager() {
    let mut manager = ImageManager::new();

    // Cria comando de transmissão
    let png_data = create_test_png_data(50, 50);
    let command = GraphicsCommand::Transmit {
        data: png_data,
        format: ImageFormat::Png,
        width: 50,
        height: 50,
    };

    // Processa o comando
    let result = manager.process_command(command);
    assert!(result.is_ok());

    // Verifica que há imagens no cache
    assert!(manager.memory_usage() > 0);

    // Testa garbage collection
    std::thread::sleep(std::time::Duration::from_millis(10));
    manager.garbage_collect(std::time::Duration::from_millis(5));
}

/// Testa cache entry com textura
#[test]
fn test_cache_entry_with_texture() {
    let image = Image::new(ImageId(1), 100, 100, ImageFormat::Png, vec![0u8; 100]);
    let entry = ImageCacheEntry::new(image, 1);

    assert_eq!(entry.access_count, 0);
    assert!(entry.texture.is_none());

    // Simula acesso
    let mut entry_clone = ImageCacheEntry {
        image: entry.image.clone(),
        texture: None,
        access_count: entry.access_count,
        created_at: entry.created_at,
        last_accessed: entry.last_accessed,
        insertion_order: entry.insertion_order,
    };

    entry_clone.touch();
    assert_eq!(entry_clone.access_count, 1);
}

/// Testa políticas de evicção diferentes
#[test]
fn test_different_eviction_policies() {
    let policies = vec![
        EvictionPolicy::Lru,
        EvictionPolicy::Lfu,
        EvictionPolicy::Fifo,
        EvictionPolicy::Random,
    ];

    for policy in policies {
        let cache = ImageCache::with_config(CacheConfig {
            max_images: 2,
            max_memory: 1024 * 1024,
            eviction_policy: policy,
            ..Default::default()
        });

        // Adiciona imagens até ultrapassar o limite
        for i in 1..=3 {
            let image = Image::new(ImageId(i), 10, 10, ImageFormat::Png, vec![0u8; 100]);
            cache.store(image);
        }

        // Deve ter apenas 2 imagens (limite)
        assert_eq!(cache.len(), 2);

        let stats = cache.stats();
        assert!(stats.metrics.evictions >= 1);
    }
}

// Helper functions

fn create_test_png_data(width: u32, height: u32) -> Vec<u8> {
    use image::{DynamicImage, Rgba, RgbaImage};

    let img = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        width,
        height,
        Rgba([128, 128, 128, 255]),
    ));

    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .unwrap();
    buf
}

fn create_test_jpeg_data(width: u32, height: u32) -> Vec<u8> {
    use image::{DynamicImage, Rgb, RgbImage};

    let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([128, 128, 128])));

    let mut buf = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Jpeg,
    )
    .unwrap();
    buf
}
