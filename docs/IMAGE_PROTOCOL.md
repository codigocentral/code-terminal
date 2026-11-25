# Image Protocol Documentation

## Overview

O módulo `image` implementa o **Kitty Graphics Protocol** para renderização de imagens inline no terminal. Este protocolo permite que aplicações exibam imagens diretamente no terminal usando escape sequences.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Terminal Grid                           │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│                ImageProtocolHandler                         │
│         (Parser de escape sequences APC)                    │
└────────────────────┬────────────────────────────────────────┘
                     │
         ┌───────────┼───────────┐
         ▼           ▼           ▼
┌─────────────┐ ┌─────────┐ ┌─────────────┐
│   Decoder   │ │  Cache  │ │   Renderer  │
│ (PNG,JPEG)  │ │ (GPU)   │ │   (wgpu)    │
└─────────────┘ └─────────┘ └─────────────┘
```

## Kitty Graphics Protocol

### Format

O protocolo usa **Application Program Command (APC)** sequences:

```
ESC _ G <key=value,...>;<payload> ESC \
```

- `ESC _` (0x1b 0x5f) - Inicia APC
- `G` - Identifica como comando gráfico
- Key-value pairs - Opções de configuração
- `;` - Separador entre metadados e dados
- `ESC \` (0x1b 0x5c) - Termina APC (ST)

### Key Options

| Key | Description | Values |
|-----|-------------|--------|
| `a` | Action | `t`=transmit, `T`=transmit+display, `d`=display, `D`=delete, `C`=clear |
| `f` | Format | `24`=PNG, `32`=RGBA, `100`=JPEG, etc. |
| `s` | Source width | Pixels |
| `v` | Source height | Pixels |
| `i` | Image ID | Number |
| `t` | Transmission | `d`=direct, `f`=file, `t`=temp, `s`=stream |
| `m` | More data | `0`=no, `1`=yes (for streaming) |

### Examples

#### Transmit PNG
```
ESC_Gf=24,s=100,v=100,a=t;<base64_data>ESC\`
```

#### Display Image
```
ESC_Ga=d,i=123,r=5,c=10;ESC\`
```

#### Delete Image
```
ESC_Ga=D,i=123;ESC\`
```

#### Clear All
```
ESC_Ga=C;ESC\`
```

## Implementation

### ImageManager

Coordena todas as operações de imagem:

```rust
use code_terminal::image::{ImageManager, GraphicsCommand, ImageFormat};

let mut manager = ImageManager::new();

// Processa comando de transmissão
let command = GraphicsCommand::Transmit {
    data: png_bytes,
    format: ImageFormat::Png,
    width: 100,
    height: 100,
};

manager.process_command(command)?;
```

### ImageCache

Cache thread-safe com políticas de evicção configuráveis:

```rust
use code_terminal::image::{ImageCache, CacheConfig, EvictionPolicy};

let config = CacheConfig {
    max_images: 100,
    max_memory: 100 * 1024 * 1024, // 100MB
    eviction_policy: EvictionPolicy::Lru,
    ..Default::default()
};

let cache = ImageCache::with_config(config);
```

#### Eviction Policies

- **LRU** (Least Recently Used) - Padrão
- **LFU** (Least Frequently Used)
- **FIFO** (First In, First Out)
- **Random**

### ImageDecoder

Suporta múltiplos formatos:

```rust
use code_terminal::image::ImageDecoder;

let decoder = ImageDecoder::new();

// Decodifica PNG
let decoded = decoder.decode_png(&png_data)?;

// Auto-detecta formato
let decoded = decoder.decode_auto(&image_data)?;

// Raw RGBA
let decoded = decoder.decode_rgba(&rgba_data, width, height)?;
```

Formatos suportados:
- PNG
- JPEG
- GIF
- WebP
- BMP
- TIFF
- Raw RGBA/RGB

### ImageRenderer

Renderização GPU via wgpu:

```rust
use code_terminal::image::ImageRenderer;

let renderer = ImageRenderer::new(device, queue, format)?;

// Cria textura GPU
renderer.create_texture(image_id, &decoded_image)?;

// Renderiza (dentro do render pass)
renderer.render(render_pass, &commands, screen_width, screen_height);
```

## Garbage Collection

Sistema de GC automático para gerenciar memória:

```rust
use code_terminal::image::{ImageGarbageCollector, GcConfig};

let cache = Arc::new(Mutex::new(ImageCache::new()));
let config = GcConfig::default();

let mut gc = ImageGarbageCollector::new(cache, config);
gc.start()?;

// ... uso do cache ...

gc.stop()?;
```

### Configurações Predefinidas

- **Default**: Intervalo 30s, TTL 5min, threshold 80%
- **Aggressive**: Intervalo 10s, TTL 1min, threshold 60%
- **Conservative**: Intervalo 60s, TTL 10min, threshold 90%

## Performance

### Cache Metrics

```rust
let stats = cache.stats();
println!("Hit ratio: {:.2}%", stats.metrics.hit_ratio() * 100.0);
println!("Memory: {} / {} bytes", stats.total_memory, stats.max_memory);
```

### Otimizações

- Texturas GPU mantidas no cache
- Upload lazy (só quando necessário)
- Evicção automática sob pressão de memória
- Suporte a streaming para imagens grandes

## Limites

| Parâmetro | Padrão | Máximo |
|-----------|--------|--------|
| Largura | 4096 | GPU limit |
| Altura | 4096 | GPU limit |
| Tamanho arquivo | 100MB | Configurável |
| Imagens cache | 100 | Configurável |
| Memória cache | 100MB | Configurável |

## Integration with Terminal

O parser de terminal detecta automaticamente sequences gráficas:

```rust
// No parser VT
if ImageProtocolHandler::is_graphics_sequence(data) {
    if let Some(payload) = ImageProtocolHandler::extract_payload(data) {
        let command = handler.parse(payload)?;
        image_manager.process_command(command)?;
    }
}
```

## Testing

```bash
# Todos os testes de imagem
cargo test image

# Testes de integração
cargo test --test image_protocol_tests

# Com logging
cargo test image -- --nocapture
```

## References

- [Kitty Graphics Protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/)
- [ECMA-48](https://ecma-international.org/publications-and-standards/standards/ecma-48/) - APC sequences
