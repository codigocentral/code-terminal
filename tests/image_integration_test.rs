//! Integration tests for Image Protocol Module
//!
//! Tests the complete flow of the Kitty Graphics Protocol implementation:
//! - Protocol parsing
//! - Image decoding
//! - Cache management
//! - Image manager operations

use code_terminal::image::{
    DecodedImage, GraphicsCommand, ImageCache, ImageDecoder, ImageFormat, ImageId, ImageManager,
    ImagePlacement, ImageProtocolHandler, TransmissionMode,
};
use std::time::Duration;

// ============================================================================
// Image Protocol Integration Tests
// ============================================================================

#[test]
fn test_full_transmit_flow() {
    let mut manager = ImageManager::new();

    let command = GraphicsCommand::Transmit {
        data: vec![0u8; 1024],
        format: ImageFormat::Png,
        width: 100,
        height: 100,
    };

    let result = manager.process_command(command);
    assert!(result.is_ok());
}

#[test]
fn test_transmit_and_retrieve() {
    let mut manager = ImageManager::new();

    let command = GraphicsCommand::Transmit {
        data: vec![255u8; 4000],
        format: ImageFormat::Png,
        width: 100,
        height: 100,
    };

    manager.process_command(command).unwrap();

    let image = manager.get_image(ImageId(1));
    assert!(image.is_some());
    let image = image.unwrap();
    assert_eq!(image.width, 100);
    assert_eq!(image.height, 100);
}

#[test]
fn test_delete_image() {
    let mut manager = ImageManager::new();

    manager
        .process_command(GraphicsCommand::Transmit {
            data: vec![0u8; 1024],
            format: ImageFormat::Png,
            width: 100,
            height: 100,
        })
        .unwrap();

    assert!(manager.get_image(ImageId(1)).is_some());

    manager
        .process_command(GraphicsCommand::Delete {
            image_id: ImageId(1),
        })
        .unwrap();

    assert!(manager.get_image(ImageId(1)).is_none());
}

#[test]
fn test_clear_all_images() {
    let mut manager = ImageManager::new();

    for _ in 1..=5 {
        manager
            .process_command(GraphicsCommand::Transmit {
                data: vec![0u8; 1024],
                format: ImageFormat::Png,
                width: 100,
                height: 100,
            })
            .unwrap();
    }

    assert_eq!(manager.memory_usage(), 5 * 1024);

    manager.process_command(GraphicsCommand::Clear).unwrap();

    assert_eq!(manager.memory_usage(), 0);
}

// ============================================================================
// Protocol Parser Integration Tests
// ============================================================================

#[test]
fn test_protocol_parse_complete_sequence() {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let mut handler = ImageProtocolHandler::new();

    let image_data = vec![0xFF; 100];
    let base64_data = STANDARD.encode(&image_data);
    let payload = format!("f=24,s=100,v=100,a=t;{}", base64_data);

    let result = handler.parse(payload.as_bytes());
    assert!(result.is_ok());

    let command = result.unwrap();
    assert!(command.is_some());

    match command.unwrap() {
        GraphicsCommand::Transmit {
            format,
            width,
            height,
            data,
        } => {
            assert_eq!(format, ImageFormat::Png);
            assert_eq!(width, 100);
            assert_eq!(height, 100);
            assert_eq!(data, image_data);
        }
        _ => panic!("Expected Transmit command"),
    }
}

#[test]
fn test_protocol_extract_and_parse() {
    let mut handler = ImageProtocolHandler::new();

    let full_sequence = b"\x1b_Gf=24,s=50,v=50,a=t;dGVzdA==\x1b\\";

    let payload = ImageProtocolHandler::extract_payload(full_sequence);
    assert!(payload.is_some());

    let result = handler.parse(payload.unwrap());
    assert!(result.is_ok());
}

#[test]
fn test_protocol_display_command() {
    let mut handler = ImageProtocolHandler::new();

    let payload = b"a=d,i=42,r=10,c=5,w=20,h=10";

    let result = handler.parse(payload).unwrap();

    match result.unwrap() {
        GraphicsCommand::Display {
            image_id,
            row,
            col,
            width,
            height,
        } => {
            assert_eq!(image_id, ImageId(42));
            assert_eq!(row, 10);
            assert_eq!(col, 5);
            assert_eq!(width, 20);
            assert_eq!(height, 10);
        }
        _ => panic!("Expected Display command"),
    }
}

// ============================================================================
// Decoder Integration Tests
// ============================================================================

#[test]
fn test_decoder_rgba_roundtrip() {
    let decoder = ImageDecoder::new();

    let original_data: Vec<u8> = (0..400).map(|i| i as u8).collect();

    let decoded = decoder.decode_rgba(&original_data, 10, 10).unwrap();

    assert_eq!(decoded.width, 10);
    assert_eq!(decoded.height, 10);
    assert_eq!(decoded.rgba_data, original_data);
}

#[test]
fn test_decoder_pixel_access() {
    let decoder = ImageDecoder::new();

    // Cria dados RGBA para uma imagem 2x1 (8 bytes = 2 pixels * 4 canais)
    let mut data = vec![0u8; 8];
    data[0..4].copy_from_slice(&[255, 0, 0, 255]);
    data[4..8].copy_from_slice(&[0, 255, 0, 255]);

    let decoded = decoder.decode_rgba(&data, 2, 1).unwrap();

    assert_eq!(decoded.get_pixel(0, 0), Some([255, 0, 0, 255]));
    assert_eq!(decoded.get_pixel(1, 0), Some([0, 255, 0, 255]));
}

#[test]
fn test_decoder_limits() {
    let decoder = ImageDecoder::with_limits(100, 100, 1000);

    let data = vec![255u8; 400];
    let result = decoder.decode_rgba(&data, 10, 10);
    assert!(result.is_ok());

    let large_data = vec![255u8; 40401];
    let result = decoder.decode_rgba(&large_data, 101, 100);
    assert!(result.is_err());
}

#[test]
fn test_decoder_format_detection() {
    assert_eq!(ImageFormat::from_extension("png"), Some(ImageFormat::Png));
    assert_eq!(ImageFormat::from_extension("jpg"), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_extension("jpeg"), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_extension("gif"), Some(ImageFormat::Gif));
    assert_eq!(ImageFormat::from_extension("webp"), Some(ImageFormat::Webp));
    assert_eq!(ImageFormat::from_extension("unknown"), None);
}

#[test]
fn test_decoder_kitty_format() {
    assert_eq!(ImageFormat::from_kitty_format(24), Some(ImageFormat::Png));
    assert_eq!(ImageFormat::from_kitty_format(32), Some(ImageFormat::Rgba));
    assert_eq!(ImageFormat::from_kitty_format(100), Some(ImageFormat::Jpeg));
    assert_eq!(ImageFormat::from_kitty_format(101), Some(ImageFormat::Gif));
    assert_eq!(ImageFormat::from_kitty_format(999), None);
}

#[test]
fn test_decoder_rgb_to_rgba() {
    let decoder = ImageDecoder::new();

    let rgb_data = vec![255, 0, 0, 0, 255, 0];
    let decoded = decoder.decode_rgb(&rgb_data, 2, 1).unwrap();

    assert_eq!(decoded.rgba_data.len(), 8);
    assert_eq!(decoded.get_pixel(0, 0), Some([255, 0, 0, 255]));
    assert_eq!(decoded.get_pixel(1, 0), Some([0, 255, 0, 255]));
}

// ============================================================================
// Cache Integration Tests
// ============================================================================

fn create_test_image(id: u32, size: usize) -> code_terminal::image::Image {
    code_terminal::image::Image::new(ImageId(id), 10, 10, ImageFormat::Png, vec![0u8; size])
}

#[test]
fn test_cache_lru_behavior() {
    let cache = ImageCache::new();

    for i in 1..=3 {
        let image = create_test_image(i, 100);
        cache.store(image);
    }

    cache.get(ImageId(1));
    cache.get(ImageId(2));

    let image4 = create_test_image(4, 100);
    cache.store(image4);

    assert!(cache.contains(ImageId(1)));
    assert!(cache.contains(ImageId(2)));
    assert!(cache.contains(ImageId(4)));
}

#[test]
fn test_cache_clear() {
    let cache = ImageCache::new();

    for i in 1..=5 {
        let image = create_test_image(i, 100);
        cache.store(image);
    }

    assert_eq!(cache.len(), 5);

    cache.clear();

    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
    assert_eq!(cache.total_size(), 0);
}

#[test]
fn test_cache_remove() {
    let cache = ImageCache::new();

    cache.store(create_test_image(1, 100));
    cache.store(create_test_image(2, 100));

    assert!(cache.contains(ImageId(1)));

    let removed = cache.remove(ImageId(1));
    assert!(removed.is_some());
    assert!(!cache.contains(ImageId(1)));
    assert!(cache.contains(ImageId(2)));
}

#[test]
fn test_cache_image_ids() {
    let cache = ImageCache::new();

    cache.store(create_test_image(1, 100));
    cache.store(create_test_image(2, 100));
    cache.store(create_test_image(3, 100));

    let ids = cache.image_ids();
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&ImageId(1)));
    assert!(ids.contains(&ImageId(2)));
    assert!(ids.contains(&ImageId(3)));
}

// ============================================================================
// Image Placement Tests
// ============================================================================

#[test]
fn test_image_placement() {
    let placement = ImagePlacement {
        image_id: ImageId(42),
        row: 10,
        col: 5,
        width_cells: 20,
        height_cells: 10,
        z_index: 0,
    };

    assert_eq!(placement.image_id, ImageId(42));
    assert_eq!(placement.row, 10);
    assert_eq!(placement.col, 5);
    assert_eq!(placement.width_cells, 20);
    assert_eq!(placement.height_cells, 10);
}

#[test]
fn test_image_placement_equality() {
    let p1 = ImagePlacement {
        image_id: ImageId(1),
        row: 0,
        col: 0,
        width_cells: 10,
        height_cells: 10,
        z_index: 0,
    };

    let p2 = ImagePlacement {
        image_id: ImageId(1),
        row: 0,
        col: 0,
        width_cells: 10,
        height_cells: 10,
        z_index: 0,
    };

    assert_eq!(p1, p2);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_decoder_error_invalid_size() {
    let decoder = ImageDecoder::new();
    let data = vec![0u8; 100];

    let result = decoder.decode_rgba(&data, 10, 10);
    assert!(result.is_err());
}

#[test]
fn test_decoder_error_dimensions() {
    let decoder = ImageDecoder::with_limits(50, 50, 1000000);
    let data = vec![255u8; 40000];

    let result = decoder.decode_rgba(&data, 100, 100);
    assert!(result.is_err());
}

// ============================================================================
// Concurrency Tests
// ============================================================================

#[test]
fn test_cache_concurrent_access() {
    use std::sync::{Arc, Mutex};
    use std::thread;

    let cache = Arc::new(Mutex::new(ImageCache::new()));
    let mut handles = vec![];

    for i in 0..10 {
        let cache_clone = Arc::clone(&cache);
        let handle = thread::spawn(move || {
            let cache = cache_clone.lock().unwrap();
            let image = create_test_image(i, 100);
            cache.store(image);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    let cache = cache.lock().unwrap();
    assert_eq!(cache.len(), 10);
}

// ============================================================================
// Performance Tests
// ============================================================================

#[test]
fn test_cache_performance_large_dataset() {
    let cache = ImageCache::new();

    let start = std::time::Instant::now();

    for i in 0..1000 {
        let image = create_test_image(i, 1000);
        cache.store(image);
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 100,
        "Cache should handle 1000 inserts in < 100ms"
    );
}

#[test]
fn test_decoder_performance() {
    let decoder = ImageDecoder::new();
    let data = vec![255u8; 40000];

    let start = std::time::Instant::now();

    for _ in 0..100 {
        let _ = decoder.decode_rgba(&data, 100, 100);
    }

    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 50,
        "Decoder should handle 100 decodes in < 50ms"
    );
}

// ============================================================================
// Protocol Edge Cases
// ============================================================================

#[test]
fn test_protocol_empty_payload() {
    let mut handler = ImageProtocolHandler::new();

    let result = handler.parse(b"");
    assert!(result.is_ok());
}

#[test]
fn test_protocol_missing_values() {
    let mut handler = ImageProtocolHandler::new();

    let result = handler.parse(b"a=t");
    assert!(result.is_ok());

    let command = result.unwrap();
    assert!(command.is_some());

    match command.unwrap() {
        GraphicsCommand::Transmit { width, height, .. } => {
            assert_eq!(width, 0);
            assert_eq!(height, 0);
        }
        _ => panic!("Expected Transmit command"),
    }
}

#[test]
fn test_protocol_invalid_base64() {
    let mut handler = ImageProtocolHandler::new();

    let result = handler.parse(b"a=t;!!!invalid-base64!!!");
    assert!(result.is_err());
}

#[test]
fn test_protocol_is_graphics_sequence() {
    assert!(ImageProtocolHandler::is_graphics_sequence(b"\x1b_G"));
    assert!(!ImageProtocolHandler::is_graphics_sequence(b"hello"));
    assert!(!ImageProtocolHandler::is_graphics_sequence(b"\x1b["));
}

// ============================================================================
// Image Manager Tests
// ============================================================================

#[test]
fn test_manager_id_generation() {
    let mut manager = ImageManager::new();

    for _ in 1..=100 {
        let command = GraphicsCommand::Transmit {
            data: vec![0u8; 100],
            format: ImageFormat::Png,
            width: 10,
            height: 10,
        };
        manager.process_command(command).unwrap();
    }

    for i in 1..=100 {
        assert!(manager.get_image(ImageId(i)).is_some());
    }
}

#[test]
fn test_manager_memory_tracking() {
    let mut manager = ImageManager::new();

    let initial_memory = manager.memory_usage();
    assert_eq!(initial_memory, 0);

    manager
        .process_command(GraphicsCommand::Transmit {
            data: vec![0u8; 1000],
            format: ImageFormat::Png,
            width: 10,
            height: 10,
        })
        .unwrap();

    assert_eq!(manager.memory_usage(), 1000);
}

#[test]
fn test_manager_garbage_collection() {
    use std::thread;

    let mut manager = ImageManager::new();

    manager
        .process_command(GraphicsCommand::Transmit {
            data: vec![0u8; 100],
            format: ImageFormat::Png,
            width: 10,
            height: 10,
        })
        .unwrap();

    thread::sleep(Duration::from_millis(10));

    manager.get_image(ImageId(1));

    manager.garbage_collect(Duration::from_millis(5));

    assert!(manager.get_image(ImageId(1)).is_some());
}

// ============================================================================
// Resize Utility Tests
// ============================================================================

#[test]
fn test_resize_to_fit_no_resize_needed() {
    let (w, h) = code_terminal::image::decoder::resize_to_fit(100, 100, 200, 200);
    assert_eq!((w, h), (100, 100));
}

#[test]
fn test_resize_to_fit_width_limited() {
    let (w, h) = code_terminal::image::decoder::resize_to_fit(400, 200, 200, 200);
    assert_eq!(w, 200);
    assert_eq!(h, 100);
}

#[test]
fn test_resize_to_fit_height_limited() {
    let (w, h) = code_terminal::image::decoder::resize_to_fit(200, 400, 200, 200);
    assert_eq!(w, 100);
    assert_eq!(h, 200);
}

#[test]
fn test_resize_to_fit_both_limited() {
    let (w, h) = code_terminal::image::decoder::resize_to_fit(800, 600, 200, 200);
    assert_eq!(w, 200);
    assert_eq!(h, 150);
}

#[test]
fn test_resize_to_fit_zero_dimensions() {
    let (w, h) = code_terminal::image::decoder::resize_to_fit(0, 100, 200, 200);
    assert_eq!((w, h), (0, 0));

    let (w, h) = code_terminal::image::decoder::resize_to_fit(100, 0, 200, 200);
    assert_eq!((w, h), (0, 0));
}

// ============================================================================
// Transmission Mode Tests
// ============================================================================

#[test]
fn test_transmission_mode_from_char() {
    assert_eq!(
        TransmissionMode::from_char('d'),
        Some(TransmissionMode::Direct)
    );
    assert_eq!(
        TransmissionMode::from_char('f'),
        Some(TransmissionMode::File)
    );
    assert_eq!(
        TransmissionMode::from_char('t'),
        Some(TransmissionMode::TempFile)
    );
    assert_eq!(
        TransmissionMode::from_char('s'),
        Some(TransmissionMode::Stream)
    );
    assert_eq!(TransmissionMode::from_char('x'), None);
}

// ============================================================================
// Decoded Image Tests
// ============================================================================

#[test]
fn test_decoded_image_size_bytes() {
    let image = DecodedImage::new(10, 10, vec![255u8; 400]);
    assert_eq!(image.size_bytes(), 400);
}

#[test]
fn test_decoded_image_out_of_bounds() {
    let image = DecodedImage::new(2, 2, vec![0u8; 16]);

    assert!(image.get_pixel(0, 0).is_some());
    assert!(image.get_pixel(1, 1).is_some());
    assert!(image.get_pixel(2, 0).is_none());
    assert!(image.get_pixel(0, 2).is_none());
}

// ============================================================================
// Image Format Tests
// ============================================================================

#[test]
fn test_image_format_mime_types() {
    assert_eq!(ImageFormat::Png.mime_type(), "image/png");
    assert_eq!(ImageFormat::Jpeg.mime_type(), "image/jpeg");
    assert_eq!(ImageFormat::Gif.mime_type(), "image/gif");
    assert_eq!(ImageFormat::Webp.mime_type(), "image/webp");
    assert_eq!(ImageFormat::Bmp.mime_type(), "image/bmp");
    assert_eq!(ImageFormat::Tiff.mime_type(), "image/tiff");
}

// ============================================================================
// Complex Integration Tests
// ============================================================================

#[test]
fn test_full_workflow_transmit_display_delete() {
    let mut manager = ImageManager::new();

    let transmit_cmd = GraphicsCommand::Transmit {
        data: vec![255u8; 10000],
        format: ImageFormat::Png,
        width: 50,
        height: 50,
    };
    manager.process_command(transmit_cmd).unwrap();

    assert!(manager.get_image(ImageId(1)).is_some());

    let display_cmd = GraphicsCommand::Display {
        image_id: ImageId(1),
        row: 5,
        col: 10,
        width: 50,
        height: 25,
    };
    manager.process_command(display_cmd).unwrap();

    let delete_cmd = GraphicsCommand::Delete {
        image_id: ImageId(1),
    };
    manager.process_command(delete_cmd).unwrap();

    assert!(manager.get_image(ImageId(1)).is_none());
}

#[test]
fn test_multiple_images_management() {
    let mut manager = ImageManager::new();

    for i in 1..=10 {
        let cmd = GraphicsCommand::Transmit {
            data: vec![i as u8; 1000],
            format: ImageFormat::Png,
            width: 10,
            height: 10,
        };
        manager.process_command(cmd).unwrap();
    }

    assert_eq!(manager.memory_usage(), 10000);

    for i in 1..=10 {
        let image = manager.get_image(ImageId(i));
        assert!(image.is_some());
        assert_eq!(image.unwrap().data[0], i as u8);
    }

    manager.process_command(GraphicsCommand::Clear).unwrap();
    assert_eq!(manager.memory_usage(), 0);
}
