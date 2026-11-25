//! Plugin de exemplo do Code Terminal.
//!
//! Mostra na sidebar há quanto tempo o terminal está aberto e quantos
//! comandos (Enter) foram digitados. Na primeira atualização escreve uma
//! saudação no painel.
//!
//! Compilar: `cargo build --release --target wasm32-unknown-unknown` e
//! copiar `plugin.toml` + o `.wasm` para `~/.config/code-terminal/plugins/session-timer/`.

use std::cell::Cell;

#[link(wasm_import_module = "host")]
extern "C" {
    fn write(ptr: *const u8, len: i32) -> i32;
    fn set_status_text(ptr: *const u8, len: i32) -> i32;
    fn get_current_time() -> i64;
}

thread_local! {
    static STARTED_MS: Cell<i64> = const { Cell::new(0) };
    static COMMANDS: Cell<u32> = const { Cell::new(0) };
    static GREETED: Cell<bool> = const { Cell::new(false) };
}

fn host_write(text: &str) {
    // SAFETY: ponteiro e tamanho de um &str válido durante a chamada.
    unsafe {
        write(text.as_ptr(), text.len() as i32);
    }
}

fn host_status(text: &str) {
    // SAFETY: idem.
    unsafe {
        set_status_text(text.as_ptr(), text.len() as i32);
    }
}

fn now_ms() -> i64 {
    // SAFETY: função do host sem argumentos.
    unsafe { get_current_time() }
}

/// O host copia o texto digitado para um buffer que o plugin aloca.
#[no_mangle]
pub extern "C" fn malloc(len: i32) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len.max(0) as usize);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn free(_ptr: *mut u8) {
    // Buffers pequenos e raros: deixar para o fim do plugin é suficiente.
}

#[no_mangle]
pub extern "C" fn init() {
    STARTED_MS.with(|s| s.set(now_ms()));
}

/// Formata a duração em minutos (ou horas e minutos).
fn format_duration(ms: i64) -> String {
    let minutes = (ms / 60_000).max(0);
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{}h{:02}", minutes / 60, minutes % 60)
    }
}

#[no_mangle]
pub extern "C" fn update(_delta_ms: i64) {
    if !GREETED.with(|g| g.replace(true)) {
        host_write("\r\n\x1b[36m[session-timer] plugin carregado\x1b[0m\r\n");
    }
    let elapsed = now_ms() - STARTED_MS.with(Cell::get);
    let commands = COMMANDS.with(Cell::get);
    host_status(&format!("{} · {commands} comandos", format_duration(elapsed)));
}

/// Conta os Enter; nunca consome o texto (o shell recebe tudo).
#[no_mangle]
pub extern "C" fn on_input(ptr: *const u8, len: i32) -> i32 {
    // SAFETY: o host escreveu `len` bytes em `ptr` (alocado por `malloc`).
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len.max(0) as usize) };
    let enters = bytes.iter().filter(|&&b| b == b'\r').count() as u32;
    COMMANDS.with(|c| c.set(c.get() + enters));
    0
}

#[no_mangle]
pub extern "C" fn shutdown() {}
