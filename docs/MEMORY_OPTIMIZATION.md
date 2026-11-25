# Otimização de Memória - Codeterm

**Documento:** US-015 - Memory Optimization  
**Status:** ✅ Implementado  
**Última Atualização:** Março 2026

---

## 📋 Visão Geral

Este documento descreve as otimizações de memória implementadas no Codeterm para garantir:

- **Memory footprint < 30MB** base
- **Scrollback compacto** com delta encoding
- **Pool de buffers reutilizáveis**
- **Sem memory leaks**

---

## Histórico do grid (21/11/2026, #1)

O histórico de cada aba (as linhas que rolaram para fora da tela) fica em
`terminal::history::History`, não em células prontas para desenhar. Antes,
cada linha era um `Vec<Cell>` de 24 bytes por célula: uma aba de 160×40 com
10 mil linhas custava **~38,5 MB** privados, o maior gasto do produto.

**Formato:** uma alocação por linha com o texto em UTF-8 e as rodadas de
estilo (células seguidas com a mesma cor, atributos, destaque da busca,
seleção e marca de continuação de caractere largo), sem os brancos do fim:

```text
[rodadas: u16][rodada 0]...[rodada n-1][texto UTF-8]
rodada = [células: u16][frente: 4][fundo: 4][atributos: u8][marcas: u8]
```

Linha só com o estilo padrão não grava rodada; linha em branco não aloca.
A conversão é sem perda (cores 24-bit, atributos, largos e emoji, destaque
e seleção voltam iguais), e o histórico cresce conforme chega saída (não
reserva as 10 mil posições na criação).

**Na tela:** com a aba rolada, o grid guarda abertas só as linhas do
histórico que aparecem (`Grid::refresh_view`, refeito a cada rolagem ou
mudança do histórico). `visible_lines()` continua devolvendo `&Vec<Cell>`;
o renderer e a interface não mudaram. A linha que sobe para o histórico é
reaproveitada como a linha em branco de baixo.

| 160×40, 10 mil linhas por aba (`tests/persistent_memory.rs`) | Antes | Depois |
|---|---|---|
| 1 aba | 41,5 MB | 4,8 MB |
| 3 abas | 120,8 MB | 10,0 MB |
| 10 abas | 396,5 MB | 25,1 MB |
| Por aba | ~39,4 MB | ~2,2 MB |

| CPU (`tests/grid_cost.rs`, saída colorida com largos) | Antes | Depois |
|---|---|---|
| Encher 10 mil linhas | 31 MB/s | 55 MB/s |
| Rolar 3 linhas (abre a tela) | 0,07 µs | ~17 µs |
| Tecla com histórico cheio | 0,10 µs | 0,10 µs |
| Enter (linha vai para o histórico) | 0,4 µs | 0,9 µs |

Medir:

```text
cargo test --release --test persistent_memory -- --ignored --exact memoria_comum_10 --nocapture
cargo test --release --test grid_cost -- --ignored --nocapture
```

O `CompactScrollback` (`src/terminal/scrollback.rs`) descrito abaixo nunca
foi ligado ao grid e perde informação (corta linhas em 512 células, paleta
de 256 cores, sem largos nem destaque); o grid usa o `History`.

---

## 🏗️ Arquitetura

### Componentes

```
┌─────────────────────────────────────────────────────────────┐
│                    Memory Optimization Layer                 │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌──────────────────┐  ┌────────────┐ │
│  │ CompactScrollback│  │   BufferPool     │  │  Memory    │ │
│  │   (Delta+RLE)   │  │  (Reusable Bufs) │  │  Tracker   │ │
│  └────────┬────────┘  └────────┬─────────┘  └─────┬──────┘ │
│           │                    │                  │        │
│           ▼                    ▼                  ▼        │
│  ┌───────────────────────────────────────────────────────┐│
│  │                    Grid / Terminal                     ││
│  └───────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```

---

## 📦 Módulos

### 1. CompactScrollback (`src/terminal/scrollback.rs`)

Implementação de scrollback otimizado com compressão.

#### Features
- **Delta Encoding**: Armazena apenas diferenças entre linhas consecutivas
- **Run-Length Encoding (RLE)**: Comprime sequências de células idênticas
- **Paleta de Cores**: Reduz o tamanho de cada célula
- **Linhas Vazias**: Otimização especial para linhas em branco

#### Uso

```rust
use codeterm::terminal::scrollback::{CompactScrollback, ScrollbackStats};

// Cria scrollback com limite de 10000 linhas
let mut scrollback = CompactScrollback::with_limit(10000);

// Adiciona linha de células
let cells: Vec<Cell> = /* ... */;
scrollback.push_line(&cells);

// Obtém estatísticas
let stats: ScrollbackStats = scrollback.stats();
println!("Compressão: {:.2}%", stats.compression_ratio * 100.0);
```

#### Formatos de Linha

```rust
pub enum CompactLine {
    /// Linha completa (fallback)
    Full(Vec<CompactCell>),
    
    /// Delta: apenas mudanças da linha anterior
    Delta {
        back_ref: u16,           // Linhas para trás na referência
        changes: Vec<(u16, u16, Vec<CompactCell>)>, // (start, end, cells)
    },
    
    /// Linha vazia (otimização)
    Empty { width: u16 },
    
    /// RLE: (célula, contagem)
    Rle(Vec<(CompactCell, u16)>),
}
```

#### CompactCell

```rust
pub struct CompactCell {
    pub char_code: u32,    // 4 bytes
    pub fg_idx: u8,        // 1 byte (índice na paleta)
    pub bg_idx: u8,        // 1 byte
    pub attr_flags: u8,    // 1 byte (flags compactados)
}                          // Total: 9 bytes (vs ~32 bytes de Cell)
```

---

### 2. BufferPool (`src/utils/memory_pool.rs`)

Pool de buffers reutilizáveis para reduzir alocações.

#### Features
- Categorias por tamanho (Small, Medium, Large, XLarge)
- Retorno automático ao pool no Drop
- Thread-safe com parking_lot
- Estatísticas de hit/miss

#### Uso

```rust
use codeterm::utils::memory_pool::{BufferPool, PooledBuffer};

// Pool customizado
let pool = BufferPool::with_config(256 * 1024, 32);

// Adquire buffer
let mut buffer = pool.acquire(4096);
buffer[0] = 42;

// Retorna ao pool automaticamente no drop
drop(buffer);

// Reutiliza
let buffer2 = pool.acquire(4096); // Possivelmente o mesmo buffer
```

#### Pool Global

```rust
use codeterm::utils::memory_pool::{acquire_buffer, global_pool_stats};

// Usa pool global
let buf = acquire_buffer(8192);

// Estatísticas
let stats = global_pool_stats();
println!("Hits: {}, Misses: {}", stats.hits, stats.misses);
```

#### ObjectPool Genérico

```rust
use codeterm::utils::memory_pool::ObjectPool;

let pool = ObjectPool::new(
    32,                          // max objetos
    || Vec::with_capacity(100),  // factory
    |v| v.clear(),               // reset
);

{
    let mut obj = pool.acquire();
    obj.push(42);
} // Retorna ao pool, limpo

let obj2 = pool.acquire();
assert!(obj2.is_empty()); // Foi resetado
```

---

### 3. MemoryTracker (`src/utils/memory_tracker.rs`)

Rastreamento de memória e detecção de leaks.

#### Features
- Tracking de alocações por categoria
- Detecção de memory leaks
- Análise de crescimento
- Histórico de snapshots
- Alertas automáticos

#### Uso Básico

```rust
use codeterm::utils::memory_tracker::{MemoryTracker, AllocGuard};

let tracker = MemoryTracker::new();

// Registra alocação manual
tracker.record_alloc(0x1000, 1024, "grid");
tracker.record_alloc(0x2000, 2048, "scrollback");

// Snapshot
let snapshot = tracker.snapshot();
println!("Alocado: {} bytes", snapshot.total_allocated);

// Libera
tracker.record_free(0x1000);
```

#### Guard Automático

```rust
{
    let _guard = AllocGuard::new(1024, "parser", &tracker);
    // Memória rastreada
} // Libera automaticamente

let snapshot = tracker.snapshot();
assert_eq!(snapshot.total_allocated, 0);
```

#### Verificação de Leaks

```rust
// Após algum tempo...
let leaks = tracker.check_leaks();
for leak in leaks {
    println!("Leak: {} bytes em '{}' (há {}s)", 
        leak.size, leak.category, leak.age_seconds);
}
```

#### Análise de Crescimento

```rust
let analysis = tracker.analyze_growth(Duration::from_secs(60));

if analysis.is_concerning {
    println!("ALERTA: Crescimento de {:.2} bytes/segundo!",
        analysis.bytes_per_second);
}
```

#### Watcher em Background

```rust
use codeterm::utils::memory_tracker::MemoryWatcher;

let watcher = MemoryWatcher::new(tracker, Duration::from_secs(30));

watcher.start(|report| {
    if report.potential_leaks > 0 {
        eprintln!("{} potenciais leaks detectados!", 
            report.potential_leaks);
    }
});

// Para quando necessário
watcher.stop();
```

---

## 📊 Benchmarks

### Executando Benchmarks de Memória

```bash
# Todos os benchmarks de memória
cargo bench --bench memory

# Benchmarks específicos
cargo bench --bench memory grid_allocation
cargo bench --bench memory scrollback_growth
cargo bench --bench memory cell_storage
```

### Interpretação de Resultados

```
grid_allocation/grid_80x24     time:   [1.234 µs 1.245 µs 1.256 µs]
scrollback_growth/add_1000     time:   [45.23 µs 45.67 µs 46.12 µs]
                               thrpt:  [21.69 Kelem/s 21.90 Kelem/s 22.11 Kelem/s]
```

### Metas de Performance

| Métrica | Meta | Status |
|---------|------|--------|
| Alocação grid 80x24 | < 2 µs | ✅ |
| Scrollback 1000 linhas | < 50 µs | ✅ |
| Compressão média | > 50% | ✅ |
| Memory footprint base | < 30 MB | ✅ |

---

## 🔧 Integração com Grid

Para usar o scrollback compacto no Grid existente:

```rust
// src/terminal/grid.rs
use crate::terminal::scrollback::CompactScrollback;

pub struct Grid {
    // ... campos existentes ...
    compact_scrollback: Option<CompactScrollback>,
}

impl Grid {
    pub fn enable_compact_scrollback(&mut self, limit: usize) {
        self.compact_scrollback = Some(
            CompactScrollback::with_limit(limit)
        );
    }
    
    pub fn push_to_compact_scrollback(&mut self, line: &[Cell]) {
        if let Some(ref mut sb) = self.compact_scrollback {
            sb.push_line(line);
        }
    }
}
```

---

## 🧪 Testes

### Testes Unitários

```bash
# Testes de scrollback
cargo test scrollback::

# Testes de memory pool
cargo test memory_pool::

# Testes de memory tracker
cargo test memory_tracker::
```

### Testes de Integração

```bash
# Teste de estresse de memória
cargo test --test memory_stress

# Verificação de leaks
cargo test --test leak_detection
```

---

## 📈 Monitoramento

### Métricas Exportadas

```rust
// Estatísticas do scrollback
pub struct ScrollbackStats {
    pub total_lines: usize,
    pub full_lines: usize,
    pub delta_lines: usize,
    pub empty_lines: usize,
    pub rle_lines: usize,
    pub estimated_bytes: usize,
    pub compression_ratio: f32,
}

// Estatísticas do pool
pub struct PoolStats {
    pub allocations: u64,
    pub deallocations: u64,
    pub hits: u64,
    pub misses: u64,
    pub total_bytes_pooled: usize,
    pub total_bytes_allocated: u64,
}
```

### Logging

```rust
use tracing::{info, debug};

// O tracker loga automaticamente
if analysis.is_concerning {
    tracing::warn!(
        "Memory growth detected: {:.2} bytes/sec",
        analysis.bytes_per_second
    );
}
```

---

## 🚀 Otimizações Futuras

1. **Compressão Zstd**: Para scrollback muito grande
2. **Offload para disco**: Linhas muito antigas
3. **Deduplicação**: Linhas idênticas
4. **Compressão de atributos**: RLE para atributos

---

## ✅ Checklist de Validação

- [ ] Memory footprint < 30MB em idle
- [ ] Scrollback compacto ativo
- [ ] Pool de buffers configurado
- [ ] Sem memory leaks detectados
- [ ] Benchmarks passando
- [ ] Documentação atualizada

---

## 📚 Referências

- [Alacritty Memory Layout](https://github.com/alacritty/alacritty/blob/master/docs/memory.md)
- [Rust Memory Optimization](https://nnethercote.github.io/perf-book/memory.html)
- [Delta Encoding](https://en.wikipedia.org/wiki/Delta_encoding)
- [Run-Length Encoding](https://en.wikipedia.org/wiki/Run-length_encoding)

---

**Maintained by:** Codeterm Team  
**Last Updated:** Março 2026
