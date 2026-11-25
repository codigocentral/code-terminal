# Plugins WASM: guia rápido

Plugins são módulos WebAssembly que o Code Terminal carrega ao abrir. Eles
rodam isolados (wasmtime, sem acesso ao sistema além das funções do host),
podem ler a tela do painel em foco, escrever nele, mostrar um status na
sidebar, avisar e ver o texto digitado.

O exemplo completo e testado está em
[`examples/plugins/session-timer`](../examples/plugins/session-timer): tempo
de sessão e comandos digitados na sidebar.

## 1. Projeto

```
meu-plugin/
├── Cargo.toml
├── plugin.toml
└── src/lib.rs
```

`Cargo.toml`:

```toml
[package]
name = "meu-plugin"
version = "0.1.0"
edition = "2021"

[workspace]            # compila sozinho, fora de outro workspace

[lib]
crate-type = ["cdylib"]

[profile.release]
opt-level = "z"
lto = true
panic = "abort"
```

`plugin.toml` (`entry` e `permissions` ficam **antes** de `[plugin]`, senão o
TOML os coloca dentro da tabela):

```toml
entry = "meu_plugin.wasm"
permissions = []        # "pty" permite mandar texto ao shell

[plugin]
name = "meu-plugin"
version = "0.1.0"
description = "Meu primeiro plugin"
author = "Seu Nome"
```

## 2. Código

As funções do host vêm do módulo `host`:

```rust
#[link(wasm_import_module = "host")]
extern "C" {
    fn write(ptr: *const u8, len: i32) -> i32;
    fn set_status_text(ptr: *const u8, len: i32) -> i32;
}

#[no_mangle]
pub extern "C" fn malloc(len: i32) -> *mut u8 {
    // O host copia o texto digitado para cá antes de chamar on_input.
    let mut buf = Vec::<u8>::with_capacity(len.max(0) as usize);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn init() {}

#[no_mangle]
pub extern "C" fn update(_delta_ms: i64) {
    let status = "olá";
    unsafe { set_status_text(status.as_ptr(), status.len() as i32) };
}

/// Texto digitado (tecla a tecla). Devolva 1 para consumir (não vai ao shell).
#[no_mangle]
pub extern "C" fn on_input(_ptr: *const u8, _len: i32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn shutdown() {}
```

## 3. Compilar e instalar

```bash
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown

mkdir -p ~/.config/code-terminal/plugins/meu-plugin
cp plugin.toml ~/.config/code-terminal/plugins/meu-plugin/
cp target/wasm32-unknown-unknown/release/meu_plugin.wasm \
   ~/.config/code-terminal/plugins/meu-plugin/
```

Reabra o Code Terminal. O log mostra `Plugin loaded plugin=meu-plugin` (ou o
motivo da falha). Para ver sem abrir janela:

```bash
code-terminal --snapshot /tmp/p.png --plugins ~/.config/code-terminal/plugins -- sleep 1
```

## Ciclo de vida

| Export | Quando |
|---|---|
| `init()` | ao carregar |
| `update(delta_ms: i64)` | a cada 250 ms |
| `on_input(ptr, len) -> i32` | a cada tecla com texto; 1 consome |
| `shutdown()` | ao fechar |
| `malloc(len) -> ptr` | o host aloca o buffer de `on_input` |

## Funções do host (módulo `host`)

| Função | Efeito |
|---|---|
| `write(ptr, len) -> i32` | texto (com sequências VT) exibido no painel em foco; não vai ao shell |
| `write_buffer(col, row, ptr, len) -> i32` | texto na posição, preservando o cursor |
| `set_cursor_position(col, row) -> i32` | move o cursor do painel |
| `get_buffer_size(out_ptr) -> i32` | grava `cols, rows` (2 × i32) |
| `get_cursor_position(out_ptr) -> i32` | grava `col, row` (2 × i32) |
| `read_buffer(c0, r0, c1, r1, out_ptr) -> i32` | texto do retângulo; o buffer precisa de `(c1-c0)×(r1-r0+1)×4 + (r1-r0+1)` bytes |
| `read_input(ptr, cap) -> i32` | próximo texto digitado (0 se nada) |
| `get_selection(ptr, cap) -> i32` | texto selecionado |
| `get_tab_id() -> i64`, `get_grid_id() -> i64` | tab e painel em foco |
| `send_key_event(ptr, len) -> i32` | manda texto ao shell; exige `permissions = ["pty"]` (sem ela: -2) |
| `show_notification(level, ptr, len) -> i32` | aviso no painel em foco (anel, central, desktop) |
| `set_status_text(ptr, len)`, `clear_status_text()` | status do plugin no rodapé da sidebar |
| `set_window_title(ptr, len) -> i32` | título da janela |
| `log_info/log_warn/log_error(ptr, len)` | log do Code Terminal |
| `get_current_time() -> i64` | milissegundos desde 1970 |

Retornos negativos: -1 ponteiro/tamanho inválido, -2 sem permissão, -3 fila
de pedidos cheia (o plugin está pedindo mais rápido do que o app aplica).

## Limites

- Os pedidos dos plugins são aplicados no próximo frame, na ordem.
- Um plugin com erro de carga não impede os outros; o motivo vai para o log.
- `on_input` recebe tecla a tecla (o shell ainda não recebeu o texto).
