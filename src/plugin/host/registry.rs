//! Host Function Registry
//!
//! Registra as host functions no wasmtime Linker para que plugins WASM possam chamá-las.

use crate::plugin::bridge::{self, HostCommand};
use crate::plugin::error::{PluginError, PluginResult};
use crate::plugin::host::{HostFunctions, HOST_MODULE_NAME};
use crate::plugin::manifest::Permission;
use crate::plugin::wasm_plugin::PluginStoreData;
use std::sync::LazyLock;
use std::time::Instant;
use wasmtime::Linker;

static SYSTEM_REGISTRY_START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Registro de host functions para wasmtime
pub struct HostFunctionRegistry {
    functions: HostFunctions,
}

impl HostFunctionRegistry {
    /// Cria novo registro com todas as host functions
    pub fn new() -> Self {
        Self {
            functions: HostFunctions::new(),
        }
    }

    /// Registra todas as host functions no linker
    pub fn register_all(&self, linker: &mut Linker<PluginStoreData>) -> PluginResult<()> {
        debug_assert!(
            !self.functions.all().is_empty(),
            "host function registry must not be empty"
        );

        // Terminal functions
        self.register_terminal_functions(linker)?;

        // Logging functions
        self.register_logging_functions(linker)?;

        // Config functions
        self.register_config_functions(linker)?;

        // Event functions
        self.register_event_functions(linker)?;

        // UI functions
        self.register_ui_functions(linker)?;

        // System functions
        self.register_system_functions(linker)?;

        Ok(())
    }

    /// Registra funções de terminal
    fn register_terminal_functions(
        &self,
        linker: &mut Linker<PluginStoreData>,
    ) -> PluginResult<()> {
        // host.write(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "write",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    match read_string_lossy(&caller, &memory, ptr, len) {
                        Some(text) => push_command(&caller, HostCommand::Write(text), len),
                        None => -1,
                    }
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register write: {}", e))
            })?;

        // host.read_input(buf_ptr: i32, buf_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "read_input",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 buf_ptr: i32,
                 buf_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    let Some(text) = bridge::pop_input() else {
                        return 0;
                    };
                    write_truncated(&mut caller, &memory, buf_ptr, buf_len, text.as_bytes())
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register read_input: {}", e))
            })?;

        // host.get_buffer_size(out_ptr: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_buffer_size",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, out_ptr: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    let (a, b) = bridge::with_view(|v| (v.cols, v.rows));
                    write_i32_pair(&mut caller, &memory, out_ptr, a.into(), b.into())
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_buffer_size: {}", e),
                )
            })?;

        // host.set_buffer_size(cols: i32, rows: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "set_buffer_size",
                |_caller: wasmtime::Caller<'_, PluginStoreData>, _cols: i32, _rows: i32| -> i32 {
                    // Placeholder - na implementação real, redimensionaria o terminal
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register set_buffer_size: {}", e),
                )
            })?;

        // host.get_cursor_position(out_ptr: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_cursor_position",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, out_ptr: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    let (a, b) = bridge::with_view(|v| (v.cursor_col, v.cursor_row));
                    write_i32_pair(&mut caller, &memory, out_ptr, a.into(), b.into())
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_cursor_position: {}", e),
                )
            })?;

        // host.set_cursor_position(col: i32, row: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "set_cursor_position",
                |caller: wasmtime::Caller<'_, PluginStoreData>, col: i32, row: i32| -> i32 {
                    // Posiciona o cursor do painel (CUP), como um programa faria.
                    if col < 0 || row < 0 {
                        return -1;
                    }
                    let seq = format!("\x1b[{};{}H", row.saturating_add(1), col.saturating_add(1));
                    push_command(&caller, HostCommand::Write(seq), 0)
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register set_cursor_position: {}", e),
                )
            })?;

        // host.read_buffer(start_col: i32, start_row: i32, end_col: i32, end_row: i32, out_ptr: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "read_buffer",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 start_col: i32,
                 start_row: i32,
                 end_col: i32,
                 end_row: i32,
                 out_ptr: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    let Some(region) = BufferRegion::new(start_col, start_row, end_col, end_row)
                    else {
                        return -1;
                    };
                    let text = bridge::with_view(|v| region.text(v));
                    write_truncated(
                        &mut caller,
                        &memory,
                        out_ptr,
                        region.capacity(),
                        text.as_bytes(),
                    )
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register read_buffer: {}", e))
            })?;

        // host.write_buffer(col: i32, row: i32, ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "write_buffer",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 col: i32,
                 row: i32,
                 ptr: i32,
                 len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    let (Ok(col), Ok(row)) = (u16::try_from(col), u16::try_from(row)) else {
                        return -1;
                    };
                    match read_string_lossy(&caller, &memory, ptr, len) {
                        Some(text) => {
                            push_command(&caller, HostCommand::WriteAt { col, row, text }, len)
                        }
                        None => -1,
                    }
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register write_buffer: {}", e))
            })?;

        // host.clear_buffer() -> i32
        linker
            .func_wrap(HOST_MODULE_NAME, "clear_buffer", || -> i32 {
                // Placeholder - na implementação real, limparia o buffer
                0
            })
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register clear_buffer: {}", e))
            })?;

        // host.scroll_buffer(lines: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "scroll_buffer",
                |_caller: wasmtime::Caller<'_, PluginStoreData>, _lines: i32| -> i32 {
                    // Placeholder - na implementação real, scrollaria o buffer
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register scroll_buffer: {}", e))
            })?;

        // host.get_tab_id() -> i64
        linker
            .func_wrap(HOST_MODULE_NAME, "get_tab_id", || -> i64 {
                bridge::with_view(|v| v.tab_id as i64)
            })
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register get_tab_id: {}", e))
            })?;

        // host.get_grid_id() -> i64
        linker
            .func_wrap(HOST_MODULE_NAME, "get_grid_id", || -> i64 {
                bridge::with_view(|v| v.pane_id as i64)
            })
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register get_grid_id: {}", e))
            })?;

        // host.send_key_event(key_ptr: i32, key_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "send_key_event",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    // Mandar texto ao shell é poder de digitar pelo usuário.
                    if !caller.data().has_permission(Permission::Pty) {
                        return -2;
                    }
                    match read_string_lossy(&caller, &memory, ptr, len) {
                        Some(text) => push_command(&caller, HostCommand::SendInput(text), len),
                        None => -1,
                    }
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register send_key_event: {}", e),
                )
            })?;

        // host.get_selection(out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_selection",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    let selection = bridge::with_view(|v| v.selection.clone());
                    write_truncated(&mut caller, &memory, out_ptr, out_len, selection.as_bytes())
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register get_selection: {}", e))
            })?;

        // host.set_selection(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "set_selection",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data =
                        match read_memory_caller(&caller, &memory, ptr as usize, len as usize) {
                            Ok(d) => d,
                            Err(_) => return -1,
                        };

                    // Placeholder - definiria a seleção
                    let _selection = String::from_utf8_lossy(&data);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register set_selection: {}", e))
            })?;

        Ok(())
    }

    /// Registra funções de logging
    fn register_logging_functions(
        &self,
        linker: &mut wasmtime::Linker<PluginStoreData>,
    ) -> PluginResult<()> {
        use crate::plugin::host::logging::{current_log_level, set_current_log_level, LogLevel};
        use tracing::{debug, error, info, trace, warn};

        // log_trace(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "log_trace",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data =
                        match read_memory_caller(&caller, &memory, ptr as usize, len as usize) {
                            Ok(d) => d,
                            Err(_) => return -1,
                        };

                    let msg = String::from_utf8_lossy(&data);
                    trace!(target: "plugin", "{}", msg);
                    len
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register log_trace: {}", e))
            })?;

        // log_debug(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "log_debug",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data =
                        match read_memory_caller(&caller, &memory, ptr as usize, len as usize) {
                            Ok(d) => d,
                            Err(_) => return -1,
                        };

                    let msg = String::from_utf8_lossy(&data);
                    debug!(target: "plugin", "{}", msg);
                    len
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register log_debug: {}", e))
            })?;

        // log_info(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "log_info",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data =
                        match read_memory_caller(&caller, &memory, ptr as usize, len as usize) {
                            Ok(d) => d,
                            Err(_) => return -1,
                        };

                    let msg = String::from_utf8_lossy(&data);
                    info!(target: "plugin", "{}", msg);
                    len
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register log_info: {}", e))
            })?;

        // log_warn(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "log_warn",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data =
                        match read_memory_caller(&caller, &memory, ptr as usize, len as usize) {
                            Ok(d) => d,
                            Err(_) => return -1,
                        };

                    let msg = String::from_utf8_lossy(&data);
                    warn!(target: "plugin", "{}", msg);
                    len
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register log_warn: {}", e))
            })?;

        // log_error(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "log_error",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data =
                        match read_memory_caller(&caller, &memory, ptr as usize, len as usize) {
                            Ok(d) => d,
                            Err(_) => return -1,
                        };

                    let msg = String::from_utf8_lossy(&data);
                    error!(target: "plugin", "{}", msg);
                    len
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register log_error: {}", e))
            })?;

        // log_with_metadata(level: i32, msg_ptr: i32, msg_len: i32, metadata_ptr: i32, metadata_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "log_with_metadata",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 level: i32,
                 msg_ptr: i32,
                 msg_len: i32,
                 metadata_ptr: i32,
                 metadata_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let msg_data = match read_memory_caller(
                        &caller,
                        &memory,
                        msg_ptr as usize,
                        msg_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let metadata_data = match read_memory_caller(
                        &caller,
                        &memory,
                        metadata_ptr as usize,
                        metadata_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let msg = String::from_utf8_lossy(&msg_data);
                    let metadata = String::from_utf8_lossy(&metadata_data);

                    match LogLevel::from_u32(level as u32) {
                        Some(LogLevel::Trace) => {
                            trace!(target: "plugin", metadata = %metadata, "{}", msg)
                        }
                        Some(LogLevel::Debug) => {
                            debug!(target: "plugin", metadata = %metadata, "{}", msg)
                        }
                        Some(LogLevel::Info) => {
                            info!(target: "plugin", metadata = %metadata, "{}", msg)
                        }
                        Some(LogLevel::Warn) => {
                            warn!(target: "plugin", metadata = %metadata, "{}", msg)
                        }
                        Some(LogLevel::Error) => {
                            error!(target: "plugin", metadata = %metadata, "{}", msg)
                        }
                        Some(LogLevel::Off) => {}
                        None => return -1,
                    }

                    msg_len
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register log_with_metadata: {}", e),
                )
            })?;

        // get_log_level() -> i32
        linker
            .func_wrap(HOST_MODULE_NAME, "get_log_level", || -> i32 {
                current_log_level() as i32
            })
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register get_log_level: {}", e))
            })?;

        // set_log_level(level: i32) -> i32
        linker
            .func_wrap(HOST_MODULE_NAME, "set_log_level", |level: i32| -> i32 {
                match LogLevel::from_u32(level as u32) {
                    Some(level) => {
                        set_current_log_level(level);
                        0
                    }
                    None => -1,
                }
            })
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register set_log_level: {}", e))
            })?;

        Ok(())
    }

    /// Registra funções de configuração
    fn register_config_functions(
        &self,
        linker: &mut wasmtime::Linker<PluginStoreData>,
    ) -> PluginResult<()> {
        // get_config(key_ptr: i32, key_len: i32, val_ptr: i32, val_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_config",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 key_ptr: i32,
                 key_len: i32,
                 val_ptr: i32,
                 val_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    // Lê a chave
                    let key_data = match read_memory_caller(
                        &caller,
                        &memory,
                        key_ptr as usize,
                        key_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let key = String::from_utf8_lossy(&key_data);

                    // Placeholder - retorna valor vazio
                    // Na implementação real, buscaria na configuração do terminal
                    let value = format!("config_{}_value", key);
                    let value_bytes = value.as_bytes();

                    let bytes_to_write = value_bytes.len().min(val_len as usize);
                    if write_memory_caller(
                        &mut caller,
                        &memory,
                        val_ptr as usize,
                        &value_bytes[..bytes_to_write],
                    )
                    .is_err()
                    {
                        return -1;
                    }

                    bytes_to_write as i32
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register get_config: {}", e))
            })?;

        // get_theme(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_theme",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let theme = b"default";
                    let bytes_to_write = theme.len().min(len as usize);

                    if write_memory_caller(
                        &mut caller,
                        &memory,
                        ptr as usize,
                        &theme[..bytes_to_write],
                    )
                    .is_err()
                    {
                        return -1;
                    }

                    bytes_to_write as i32
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register get_theme: {}", e))
            })?;

        // get_plugin_config(ptr: i32, len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_plugin_config",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, ptr: i32, len: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    // Retorna configuração do store
                    let data = caller
                        .data()
                        .get_data("config")
                        .cloned()
                        .unwrap_or_default();
                    let bytes_to_write = data.len().min(len as usize);

                    if write_memory_caller(
                        &mut caller,
                        &memory,
                        ptr as usize,
                        &data[..bytes_to_write],
                    )
                    .is_err()
                    {
                        return -1;
                    }

                    bytes_to_write as i32
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_plugin_config: {}", e),
                )
            })?;

        Ok(())
    }

    /// Registra funções de eventos
    fn register_event_functions(
        &self,
        linker: &mut wasmtime::Linker<PluginStoreData>,
    ) -> PluginResult<()> {
        // host.subscribe_event(event_type: i32, callback_id: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "subscribe_event",
                |_caller: wasmtime::Caller<'_, PluginStoreData>,
                 event_type: i32,
                 callback_id: i32|
                 -> i32 {
                    // Placeholder - registra assinatura de evento
                    // Na implementação real, registraria callback para o tipo de evento
                    tracing::debug!(
                        "Plugin subscribed to event type {} with callback {}",
                        event_type,
                        callback_id
                    );
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register subscribe_event: {}", e),
                )
            })?;

        // host.unsubscribe_event(event_type: i32, callback_id: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "unsubscribe_event",
                |_caller: wasmtime::Caller<'_, PluginStoreData>,
                 event_type: i32,
                 callback_id: i32|
                 -> i32 {
                    // Placeholder - remove assinatura de evento
                    tracing::debug!(
                        "Plugin unsubscribed from event type {} with callback {}",
                        event_type,
                        callback_id
                    );
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register unsubscribe_event: {}", e),
                )
            })?;

        // host.emit_event(event_type: i32, data_ptr: i32, data_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "emit_event",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 event_type: i32,
                 data_ptr: i32,
                 data_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data = match read_memory_caller(
                        &caller,
                        &memory,
                        data_ptr as usize,
                        data_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let event_data = String::from_utf8_lossy(&data);
                    tracing::debug!(
                        "Plugin emitted event type {} with data: {}",
                        event_type,
                        event_data
                    );
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register emit_event: {}", e))
            })?;

        // host.register_command(name_ptr: i32, name_len: i32, handler_id: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "register_command",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 name_ptr: i32,
                 name_len: i32,
                 handler_id: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data = match read_memory_caller(
                        &caller,
                        &memory,
                        name_ptr as usize,
                        name_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let name = String::from_utf8_lossy(&data);
                    tracing::debug!(
                        "Plugin registered command '{}' with handler {}",
                        name,
                        handler_id
                    );
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register register_command: {}", e),
                )
            })?;

        // host.unregister_command(name_ptr: i32, name_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "unregister_command",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 name_ptr: i32,
                 name_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data = match read_memory_caller(
                        &caller,
                        &memory,
                        name_ptr as usize,
                        name_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let name = String::from_utf8_lossy(&data);
                    tracing::debug!("Plugin unregistered command '{}'", name);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register unregister_command: {}", e),
                )
            })?;

        // host.execute_command(name_ptr: i32, name_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "execute_command",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 name_ptr: i32,
                 name_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data = match read_memory_caller(
                        &caller,
                        &memory,
                        name_ptr as usize,
                        name_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let name = String::from_utf8_lossy(&data);
                    tracing::debug!("Plugin executed command '{}'", name);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register execute_command: {}", e),
                )
            })?;

        // host.get_event_queue_size() -> i32
        linker
            .func_wrap(HOST_MODULE_NAME, "get_event_queue_size", || -> i32 {
                // Placeholder - retorna tamanho da fila de eventos
                0
            })
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_event_queue_size: {}", e),
                )
            })?;

        // host.flush_events() -> i32
        linker
            .func_wrap(HOST_MODULE_NAME, "flush_events", || -> i32 {
                // Placeholder - limpa a fila de eventos
                0
            })
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register flush_events: {}", e))
            })?;

        // host.set_timeout(ms: i32, callback_id: i32) -> i64
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "set_timeout",
                |_caller: wasmtime::Caller<'_, PluginStoreData>,
                 ms: i32,
                 callback_id: i32|
                 -> i64 {
                    tracing::debug!(
                        "Plugin set timeout for {}ms with callback {}",
                        ms,
                        callback_id
                    );
                    // Placeholder - retorna ID do timer
                    callback_id as i64
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register set_timeout: {}", e))
            })?;

        // host.set_interval(ms: i32, callback_id: i32) -> i64
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "set_interval",
                |_caller: wasmtime::Caller<'_, PluginStoreData>,
                 ms: i32,
                 callback_id: i32|
                 -> i64 {
                    tracing::debug!(
                        "Plugin set interval for {}ms with callback {}",
                        ms,
                        callback_id
                    );
                    // Placeholder - retorna ID do timer
                    callback_id as i64
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register set_interval: {}", e))
            })?;

        // host.clear_timer(timer_id: i64) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "clear_timer",
                |_caller: wasmtime::Caller<'_, PluginStoreData>, timer_id: i64| -> i32 {
                    tracing::debug!("Plugin cleared timer {}", timer_id);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register clear_timer: {}", e))
            })?;

        Ok(())
    }

    /// Registra funções de UI
    fn register_ui_functions(
        &self,
        linker: &mut wasmtime::Linker<PluginStoreData>,
    ) -> PluginResult<()> {
        // host.show_notification(level: i32, msg_ptr: i32, msg_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "show_notification",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 level: i32,
                 msg_ptr: i32,
                 msg_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    match read_string_lossy(&caller, &memory, msg_ptr, msg_len) {
                        Some(message) => {
                            push_command(&caller, HostCommand::Notify { level, message }, 0)
                        }
                        None => -1,
                    }
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register show_notification: {}", e),
                )
            })?;

        // host.show_alert_dialog(title_ptr: i32, title_len: i32, msg_ptr: i32, msg_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "show_alert_dialog",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 title_ptr: i32,
                 title_len: i32,
                 msg_ptr: i32,
                 msg_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let title_data = match read_memory_caller(
                        &caller,
                        &memory,
                        title_ptr as usize,
                        title_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let msg_data = match read_memory_caller(
                        &caller,
                        &memory,
                        msg_ptr as usize,
                        msg_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let title = String::from_utf8_lossy(&title_data);
                    let msg = String::from_utf8_lossy(&msg_data);
                    tracing::info!("Plugin alert dialog: [{}] {}", title, msg);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register show_alert_dialog: {}", e),
                )
            })?;

        // host.show_confirm_dialog(title_ptr: i32, title_len: i32, msg_ptr: i32, msg_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "show_confirm_dialog",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 title_ptr: i32,
                 title_len: i32,
                 msg_ptr: i32,
                 msg_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let title_data = match read_memory_caller(
                        &caller,
                        &memory,
                        title_ptr as usize,
                        title_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let msg_data = match read_memory_caller(
                        &caller,
                        &memory,
                        msg_ptr as usize,
                        msg_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let title = String::from_utf8_lossy(&title_data);
                    let msg = String::from_utf8_lossy(&msg_data);
                    tracing::info!("Plugin confirm dialog: [{}] {}", title, msg);
                    // Placeholder - retorna 1 (sim) como padrão
                    1
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register show_confirm_dialog: {}", e),
                )
            })?;

        // host.show_prompt_dialog(title_ptr: i32, title_len: i32, msg_ptr: i32, msg_len: i32,
        //                         default_ptr: i32, default_len: i32, out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "show_prompt_dialog",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 title_ptr: i32,
                 title_len: i32,
                 msg_ptr: i32,
                 msg_len: i32,
                 default_ptr: i32,
                 default_len: i32,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let title_data = match read_memory_caller(
                        &caller,
                        &memory,
                        title_ptr as usize,
                        title_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let msg_data = match read_memory_caller(
                        &caller,
                        &memory,
                        msg_ptr as usize,
                        msg_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let default_data = match read_memory_caller(
                        &caller,
                        &memory,
                        default_ptr as usize,
                        default_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let title = String::from_utf8_lossy(&title_data);
                    let msg = String::from_utf8_lossy(&msg_data);
                    let default_val = String::from_utf8_lossy(&default_data);
                    tracing::info!(
                        "Plugin prompt dialog: [{}] {} (default: {})",
                        title,
                        msg,
                        default_val
                    );

                    // Retorna o valor padrão
                    let bytes_to_write = default_data.len().min(out_len as usize);
                    if write_memory_caller(
                        &mut caller,
                        &memory,
                        out_ptr as usize,
                        &default_data[..bytes_to_write],
                    )
                    .is_err()
                    {
                        return -1;
                    }

                    bytes_to_write as i32
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register show_prompt_dialog: {}", e),
                )
            })?;

        // host.set_status_text(msg_ptr: i32, msg_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "set_status_text",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 msg_ptr: i32,
                 msg_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    match read_string_lossy(&caller, &memory, msg_ptr, msg_len) {
                        Some(text) => push_command(&caller, HostCommand::Status(text), 0),
                        None => -1,
                    }
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register set_status_text: {}", e),
                )
            })?;

        // host.clear_status_text() -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "clear_status_text",
                |caller: wasmtime::Caller<'_, PluginStoreData>| -> i32 {
                    push_command(&caller, HostCommand::Status(String::new()), 0)
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register clear_status_text: {}", e),
                )
            })?;

        // host.show_overlay(id: i32, x: i32, y: i32, w: i32, h: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "show_overlay",
                |_caller: wasmtime::Caller<'_, PluginStoreData>,
                 id: i32,
                 x: i32,
                 y: i32,
                 w: i32,
                 h: i32|
                 -> i32 {
                    tracing::debug!(
                        "Plugin show overlay {} at ({}, {}) size {}x{}",
                        id,
                        x,
                        y,
                        w,
                        h
                    );
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register show_overlay: {}", e))
            })?;

        // host.hide_overlay(id: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "hide_overlay",
                |_caller: wasmtime::Caller<'_, PluginStoreData>, id: i32| -> i32 {
                    tracing::debug!("Plugin hide overlay {}", id);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed("host", format!("Failed to register hide_overlay: {}", e))
            })?;

        // host.update_overlay(id: i32, x: i32, y: i32, w: i32, h: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "update_overlay",
                |_caller: wasmtime::Caller<'_, PluginStoreData>,
                 id: i32,
                 x: i32,
                 y: i32,
                 w: i32,
                 h: i32|
                 -> i32 {
                    tracing::debug!(
                        "Plugin update overlay {} to ({}, {}) size {}x{}",
                        id,
                        x,
                        y,
                        w,
                        h
                    );
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register update_overlay: {}", e),
                )
            })?;

        // host.get_window_position(out_ptr: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_window_position",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, out_ptr: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    // Posição padrão (0, 0)
                    let pos = [0u8; 8]; // x, y as i32

                    if write_memory_caller(&mut caller, &memory, out_ptr as usize, &pos).is_err() {
                        return -1;
                    }

                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_window_position: {}", e),
                )
            })?;

        // host.get_window_size(out_ptr: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_window_size",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, out_ptr: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    // Tamanho padrão (800x600)
                    let size = [0x20u8, 0x03, 0, 0, 0x58u8, 0x02, 0, 0]; // 800, 600 as i32

                    if write_memory_caller(&mut caller, &memory, out_ptr as usize, &size).is_err() {
                        return -1;
                    }

                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_window_size: {}", e),
                )
            })?;

        // host.set_window_title(title_ptr: i32, title_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "set_window_title",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 title_ptr: i32,
                 title_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };
                    match read_string_lossy(&caller, &memory, title_ptr, title_len) {
                        Some(text) => push_command(&caller, HostCommand::SetTitle(text), 0),
                        None => -1,
                    }
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register set_window_title: {}", e),
                )
            })?;

        // host.get_mouse_position(out_ptr: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_mouse_position",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, out_ptr: i32| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    // Posição padrão (0, 0)
                    let pos = [0u8; 8]; // x, y as i32

                    if write_memory_caller(&mut caller, &memory, out_ptr as usize, &pos).is_err() {
                        return -1;
                    }

                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_mouse_position: {}", e),
                )
            })?;

        // host.show_context_menu(x: i32, y: i32, items_ptr: i32, items_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "show_context_menu",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 x: i32,
                 y: i32,
                 items_ptr: i32,
                 items_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data = match read_memory_caller(
                        &caller,
                        &memory,
                        items_ptr as usize,
                        items_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let items = String::from_utf8_lossy(&data);
                    tracing::debug!("Plugin show context menu at ({}, {}): {}", x, y, items);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register show_context_menu: {}", e),
                )
            })?;

        // host.register_status_bar_item(id: i32, label_ptr: i32, label_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "register_status_bar_item",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 id: i32,
                 label_ptr: i32,
                 label_len: i32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -1,
                    };

                    let data = match read_memory_caller(
                        &caller,
                        &memory,
                        label_ptr as usize,
                        label_len as usize,
                    ) {
                        Ok(d) => d,
                        Err(_) => return -1,
                    };

                    let label = String::from_utf8_lossy(&data);
                    tracing::debug!("Plugin registered status bar item {}: {}", id, label);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register register_status_bar_item: {}", e),
                )
            })?;

        // host.unregister_status_bar_item(id: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "unregister_status_bar_item",
                |_caller: wasmtime::Caller<'_, PluginStoreData>, id: i32| -> i32 {
                    tracing::debug!("Plugin unregistered status bar item {}", id);
                    0
                },
            )
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register unregister_status_bar_item: {}", e),
                )
            })?;

        Ok(())
    }

    /// Registra funções de sistema
    fn register_system_functions(
        &self,
        linker: &mut wasmtime::Linker<PluginStoreData>,
    ) -> PluginResult<()> {
        use crate::plugin::host::system::Platform;

        macro_rules! register_error {
            ($name:literal, $err:expr) => {
                PluginError::init_failed("host", format!("Failed to register {}: {}", $name, $err))
            };
        }

        fn memory(caller: &mut wasmtime::Caller<'_, PluginStoreData>) -> Option<wasmtime::Memory> {
            match caller.get_export("memory") {
                Some(wasmtime::Extern::Memory(memory)) => Some(memory),
                _ => None,
            }
        }

        fn read_guest_string(
            caller: &wasmtime::Caller<'_, PluginStoreData>,
            memory: &wasmtime::Memory,
            ptr: i32,
            len: i32,
        ) -> Option<String> {
            read_string_caller(caller, memory, ptr as usize, len as usize).ok()
        }

        fn write_guest_bytes(
            caller: &mut wasmtime::Caller<'_, PluginStoreData>,
            memory: &wasmtime::Memory,
            ptr: i32,
            len: i32,
            data: &[u8],
        ) -> i32 {
            if len < 0 || ptr < 0 {
                return -1;
            }

            let bytes_to_write = data.len().min(len as usize);
            match write_memory_caller(caller, memory, ptr as usize, &data[..bytes_to_write]) {
                Ok(()) => bytes_to_write as i32,
                Err(_) => -1,
            }
        }

        fn write_guest_string(
            caller: &mut wasmtime::Caller<'_, PluginStoreData>,
            memory: &wasmtime::Memory,
            ptr: i32,
            len: i32,
            value: impl AsRef<str>,
        ) -> i32 {
            write_guest_bytes(caller, memory, ptr, len, value.as_ref().as_bytes())
        }

        // get_host_version(out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_host_version",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    write_guest_string(
                        &mut caller,
                        &memory,
                        out_ptr,
                        out_len,
                        env!("CARGO_PKG_VERSION"),
                    )
                },
            )
            .map_err(|e| register_error!("get_host_version", e))?;

        // get_api_version(out_ptr: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_api_version",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, out_ptr: i32| -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    let version =
                        [1u32.to_le_bytes(), 0u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
                    if write_guest_bytes(
                        &mut caller,
                        &memory,
                        out_ptr,
                        version.len() as i32,
                        &version,
                    ) < 0
                    {
                        -1
                    } else {
                        0
                    }
                },
            )
            .map_err(|e| register_error!("get_api_version", e))?;

        // get_platform() -> i32
        linker
            .func_wrap(HOST_MODULE_NAME, "get_platform", || -> i32 {
                Platform::current() as i32
            })
            .map_err(|e| register_error!("get_platform", e))?;

        // get_env_var(name_ptr: i32, name_len: i32, out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_env_var",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 name_ptr: i32,
                 name_len: i32,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    let Some(name) = read_guest_string(&caller, &memory, name_ptr, name_len) else {
                        return -1;
                    };
                    match std::env::var(name) {
                        Ok(value) => {
                            write_guest_string(&mut caller, &memory, out_ptr, out_len, value)
                        }
                        Err(_) => -1,
                    }
                },
            )
            .map_err(|e| register_error!("get_env_var", e))?;

        // get_current_time() -> i64
        linker
            .func_wrap(HOST_MODULE_NAME, "get_current_time", || -> i64 {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
            })
            .map_err(|e| register_error!("get_current_time", e))?;

        // get_uptime() -> i64
        linker
            .func_wrap(HOST_MODULE_NAME, "get_uptime", || -> i64 {
                SYSTEM_REGISTRY_START.elapsed().as_millis() as i64
            })
            .map_err(|e| register_error!("get_uptime", e))?;

        // get_memory_usage(out_ptr: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_memory_usage",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>, out_ptr: i32| -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    let usage = [0i64.to_le_bytes(), 0i64.to_le_bytes()].concat();
                    if write_guest_bytes(&mut caller, &memory, out_ptr, usage.len() as i32, &usage)
                        < 0
                    {
                        -1
                    } else {
                        0
                    }
                },
            )
            .map_err(|e| register_error!("get_memory_usage", e))?;

        // get_cpu_usage() -> f32
        linker
            .func_wrap(HOST_MODULE_NAME, "get_cpu_usage", || -> f32 { 0.0 })
            .map_err(|e| register_error!("get_cpu_usage", e))?;

        // get_hostname(out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_hostname",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    match hostname::get() {
                        Ok(hostname) => write_guest_string(
                            &mut caller,
                            &memory,
                            out_ptr,
                            out_len,
                            hostname.to_string_lossy(),
                        ),
                        Err(_) => -1,
                    }
                },
            )
            .map_err(|e| register_error!("get_hostname", e))?;

        // get_home_dir(out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_home_dir",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    match dirs::home_dir() {
                        Some(path) => write_guest_string(
                            &mut caller,
                            &memory,
                            out_ptr,
                            out_len,
                            path.to_string_lossy(),
                        ),
                        None => -1,
                    }
                },
            )
            .map_err(|e| register_error!("get_home_dir", e))?;

        // get_config_dir(out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_config_dir",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    match dirs::config_dir().map(|path| path.join("codeterm")) {
                        Some(path) => write_guest_string(
                            &mut caller,
                            &memory,
                            out_ptr,
                            out_len,
                            path.to_string_lossy(),
                        ),
                        None => -1,
                    }
                },
            )
            .map_err(|e| register_error!("get_config_dir", e))?;

        // get_data_dir(out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_data_dir",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    match dirs::data_dir().map(|path| path.join("codeterm")) {
                        Some(path) => write_guest_string(
                            &mut caller,
                            &memory,
                            out_ptr,
                            out_len,
                            path.to_string_lossy(),
                        ),
                        None => -1,
                    }
                },
            )
            .map_err(|e| register_error!("get_data_dir", e))?;

        // get_temp_dir(out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "get_temp_dir",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    write_guest_string(
                        &mut caller,
                        &memory,
                        out_ptr,
                        out_len,
                        std::env::temp_dir().to_string_lossy(),
                    )
                },
            )
            .map_err(|e| register_error!("get_temp_dir", e))?;

        // read_file(path_ptr: i32, path_len: i32, out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "read_file",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 path_ptr: i32,
                 path_len: i32,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    let Some(path) = read_guest_string(&caller, &memory, path_ptr, path_len) else {
                        return -1;
                    };
                    match std::fs::read(path) {
                        Ok(data) => {
                            write_guest_bytes(&mut caller, &memory, out_ptr, out_len, &data)
                        }
                        Err(_) => -1,
                    }
                },
            )
            .map_err(|e| register_error!("read_file", e))?;

        // write_file(path_ptr: i32, path_len: i32, data_ptr: i32, data_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "write_file",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 path_ptr: i32,
                 path_len: i32,
                 data_ptr: i32,
                 data_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    let Some(path) = read_guest_string(&caller, &memory, path_ptr, path_len) else {
                        return -1;
                    };
                    let Ok(data) =
                        read_memory_caller(&caller, &memory, data_ptr as usize, data_len as usize)
                    else {
                        return -1;
                    };
                    match std::fs::write(path, &data) {
                        Ok(()) => data.len() as i32,
                        Err(_) => -1,
                    }
                },
            )
            .map_err(|e| register_error!("write_file", e))?;

        // file_exists(path_ptr: i32, path_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "file_exists",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 path_ptr: i32,
                 path_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    let Some(path) = read_guest_string(&caller, &memory, path_ptr, path_len) else {
                        return -1;
                    };
                    if std::path::Path::new(&path).exists() {
                        1
                    } else {
                        0
                    }
                },
            )
            .map_err(|e| register_error!("file_exists", e))?;

        // list_dir(path_ptr: i32, path_len: i32, out_ptr: i32, out_len: i32) -> i32
        linker
            .func_wrap(
                HOST_MODULE_NAME,
                "list_dir",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>,
                 path_ptr: i32,
                 path_len: i32,
                 out_ptr: i32,
                 out_len: i32|
                 -> i32 {
                    let Some(memory) = memory(&mut caller) else {
                        return -1;
                    };
                    let Some(path) = read_guest_string(&caller, &memory, path_ptr, path_len) else {
                        return -1;
                    };

                    let Ok(read_dir) = std::fs::read_dir(path) else {
                        return -1;
                    };
                    let mut entries = Vec::new();
                    for entry in read_dir.flatten() {
                        if let Ok(file_type) = entry.file_type() {
                            entries.push(serde_json::json!({
                                "name": entry.file_name().to_string_lossy(),
                                "path": entry.path().to_string_lossy(),
                                "is_dir": file_type.is_dir(),
                                "is_file": file_type.is_file(),
                            }));
                        }
                    }
                    match serde_json::to_vec(&entries) {
                        Ok(data) => {
                            write_guest_bytes(&mut caller, &memory, out_ptr, out_len, &data)
                        }
                        Err(_) => -1,
                    }
                },
            )
            .map_err(|e| register_error!("list_dir", e))?;

        // Aliases legados mantidos por compatibilidade.
        // get_time_millis() -> i64
        linker
            .func_wrap(HOST_MODULE_NAME, "get_time_millis", || -> i64 {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
            })
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_time_millis: {}", e),
                )
            })?;

        // get_time_micros() -> i64
        linker
            .func_wrap(HOST_MODULE_NAME, "get_time_micros", || -> i64 {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_micros() as i64)
                    .unwrap_or(0)
            })
            .map_err(|e| {
                PluginError::init_failed(
                    "host",
                    format!("Failed to register get_time_micros: {}", e),
                )
            })?;

        Ok(())
    }
}

impl Default for HostFunctionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Retângulo pedido por `read_buffer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BufferRegion {
    start_col: usize,
    start_row: usize,
    end_col: usize,
    end_row: usize,
}

impl BufferRegion {
    fn new(start_col: i32, start_row: i32, end_col: i32, end_row: i32) -> Option<Self> {
        let region = Self {
            start_col: usize::try_from(start_col).ok()?,
            start_row: usize::try_from(start_row).ok()?,
            end_col: usize::try_from(end_col).ok()?,
            end_row: usize::try_from(end_row).ok()?,
        };
        (region.end_col >= region.start_col && region.end_row >= region.start_row).then_some(region)
    }

    fn text(&self, view: &bridge::HostView) -> String {
        view.region(self.start_col, self.start_row, self.end_col, self.end_row)
    }

    /// O buffer do plugin deve caber 4 bytes por célula e 1 por quebra de linha.
    fn capacity(&self) -> i32 {
        let rows = self.end_row - self.start_row + 1;
        let cells = (self.end_col - self.start_col).saturating_mul(rows);
        i32::try_from(cells.saturating_mul(4).saturating_add(rows)).unwrap_or(i32::MAX)
    }
}

/// Lê texto da memória do plugin (UTF-8 inválido vira U+FFFD).
fn read_string_lossy(
    caller: &wasmtime::Caller<'_, PluginStoreData>,
    memory: &wasmtime::Memory,
    ptr: i32,
    len: i32,
) -> Option<String> {
    let (Ok(ptr), Ok(len)) = (usize::try_from(ptr), usize::try_from(len)) else {
        return None;
    };
    read_memory_caller(caller, memory, ptr, len)
        .ok()
        .map(|data| String::from_utf8_lossy(&data).into_owned())
}

/// Pede algo ao app em nome do plugin; devolve `ok`, ou -3 se a fila encheu.
fn push_command(
    caller: &wasmtime::Caller<'_, PluginStoreData>,
    command: HostCommand,
    ok: i32,
) -> i32 {
    if bridge::push(caller.data().plugin_name(), command) {
        ok
    } else {
        -3
    }
}

/// Escreve até `capacity` bytes (sem cortar caractere UTF-8 no meio) e
/// devolve quantos escreveu.
fn write_truncated(
    caller: &mut wasmtime::Caller<'_, PluginStoreData>,
    memory: &wasmtime::Memory,
    ptr: i32,
    capacity: i32,
    data: &[u8],
) -> i32 {
    let (Ok(ptr), Ok(capacity)) = (usize::try_from(ptr), usize::try_from(capacity)) else {
        return -1;
    };
    let mut n = data.len().min(capacity);
    while n > 0 && n < data.len() && (data[n] & 0xC0) == 0x80 {
        n -= 1;
    }
    match write_memory_caller(caller, memory, ptr, &data[..n]) {
        Ok(()) => i32::try_from(n).unwrap_or(i32::MAX),
        Err(_) => -1,
    }
}

/// Escreve dois i32 little-endian (ex.: colunas e linhas).
fn write_i32_pair(
    caller: &mut wasmtime::Caller<'_, PluginStoreData>,
    memory: &wasmtime::Memory,
    ptr: i32,
    a: i32,
    b: i32,
) -> i32 {
    let Ok(ptr) = usize::try_from(ptr) else {
        return -1;
    };
    let mut bytes = [0u8; 8];
    bytes[..4].copy_from_slice(&a.to_le_bytes());
    bytes[4..].copy_from_slice(&b.to_le_bytes());
    match write_memory_caller(caller, memory, ptr, &bytes) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Lê dados da memória WASM usando Caller
pub fn read_memory_caller(
    caller: &wasmtime::Caller<'_, PluginStoreData>,
    memory: &wasmtime::Memory,
    offset: usize,
    len: usize,
) -> Result<Vec<u8>, String> {
    let mut buffer = vec![0u8; len];
    memory
        .read(caller, offset, &mut buffer)
        .map_err(|e| format!("Failed to read memory: {}", e))?;
    Ok(buffer)
}

/// Lê uma string da memória WASM usando Caller
pub fn read_string_caller(
    caller: &wasmtime::Caller<'_, PluginStoreData>,
    memory: &wasmtime::Memory,
    offset: usize,
    len: usize,
) -> Result<String, String> {
    let data = read_memory_caller(caller, memory, offset, len)?;
    String::from_utf8(data).map_err(|e| format!("Invalid UTF-8: {}", e))
}

/// Escreve dados na memória WASM usando Caller
pub fn write_memory_caller(
    caller: &mut wasmtime::Caller<'_, PluginStoreData>,
    memory: &wasmtime::Memory,
    offset: usize,
    data: &[u8],
) -> Result<(), String> {
    memory
        .write(caller, offset, data)
        .map_err(|e| format!("Failed to write memory: {}", e))
}

/// Escreve uma string na memória WASM usando Caller
pub fn write_string_caller(
    caller: &mut wasmtime::Caller<'_, PluginStoreData>,
    memory: &wasmtime::Memory,
    offset: usize,
    s: &str,
) -> Result<(), String> {
    write_memory_caller(caller, memory, offset, s.as_bytes())
}

/// Aloca memória no WASM (chama malloc do guest se disponível)
pub fn allocate_in_wasm(
    caller: &mut wasmtime::Caller<'_, PluginStoreData>,
    size: usize,
) -> Result<i32, String> {
    // Tenta chamar malloc do guest
    let malloc = caller.get_export("malloc").and_then(|e| e.into_func());

    if let Some(malloc_func) = malloc {
        let mut result = [wasmtime::Val::I32(0)];
        malloc_func
            .call(
                &mut *caller,
                &[wasmtime::Val::I32(size as i32)],
                &mut result,
            )
            .map_err(|e| format!("malloc failed: {}", e))?;
        Ok(result[0].unwrap_i32())
    } else {
        Err("malloc not found in WASM module".to_string())
    }
}

/// Libera memória no WASM (chama free do guest se disponível)
pub fn free_in_wasm(
    caller: &mut wasmtime::Caller<'_, PluginStoreData>,
    ptr: i32,
) -> Result<(), String> {
    let free = caller.get_export("free").and_then(|e| e.into_func());

    if let Some(free_func) = free {
        free_func
            .call(&mut *caller, &[wasmtime::Val::I32(ptr)], &mut [])
            .map_err(|e| format!("free failed: {}", e))?;
        Ok(())
    } else {
        // free é opcional
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::wasm_runtime::WasmRuntime;
    use wasmtime::{Module, Store};

    fn create_test_wasm() -> Vec<u8> {
        // WASM simples com memória exportada
        wat::parse_str(
            r#"
            (module
                (memory (export "memory") 1)
                (func (export "test") (result i32)
                    i32.const 42
                )
            )
        "#,
        )
        .unwrap()
    }

    #[test]
    fn test_registry_creation() {
        let registry = HostFunctionRegistry::new();
        assert!(!registry.functions.all().is_empty());
    }

    #[test]
    fn test_register_functions() {
        let runtime = WasmRuntime::new().unwrap();
        let mut linker = wasmtime::Linker::new(runtime.engine());

        let registry = HostFunctionRegistry::new();
        let result = registry.register_all(&mut linker);

        assert!(result.is_ok());
    }

    #[test]
    fn test_host_write() {
        let runtime = WasmRuntime::new().unwrap();
        let mut linker = wasmtime::Linker::new(runtime.engine());

        let registry = HostFunctionRegistry::new();
        registry.register_all(&mut linker).unwrap();

        // Compila e instancia um WASM de teste
        let wasm = create_test_wasm();
        let module = Module::new(runtime.engine(), &wasm).unwrap();

        let mut store = Store::new(runtime.engine(), PluginStoreData::new());
        let _instance = linker.instantiate(&mut store, &module).unwrap();
    }

    // ------------------------------------------------------------------
    // Harness: módulo WASM que importa UMA host function e a reexporta
    // através de uma função wasm ("call"), para que o Caller enxergue a
    // memória exportada pela instância.
    // ------------------------------------------------------------------

    /// Offset fora da memória de 1 página (64 KiB).
    const OOB: i32 = 70_000;

    struct Guest {
        store: Store<PluginStoreData>,
        instance: wasmtime::Instance,
    }

    impl Guest {
        fn new(name: &str, params: &[&str], result: Option<&str>) -> Self {
            Self::build(name, params, result, true, PluginStoreData::new())
        }

        fn with_data(
            name: &str,
            params: &[&str],
            result: Option<&str>,
            data: PluginStoreData,
        ) -> Self {
            Self::build(name, params, result, true, data)
        }

        fn without_memory(name: &str, params: &[&str], result: Option<&str>) -> Self {
            Self::build(name, params, result, false, PluginStoreData::new())
        }

        fn build(
            name: &str,
            params: &[&str],
            result: Option<&str>,
            with_memory: bool,
            data: PluginStoreData,
        ) -> Self {
            let param_decl = if params.is_empty() {
                String::new()
            } else {
                format!("(param {})", params.join(" "))
            };
            let result_decl = result.map(|r| format!("(result {r})")).unwrap_or_default();
            let locals: String = (0..params.len())
                .map(|i| format!("local.get {i} "))
                .collect();
            let memory = if with_memory {
                r#"(memory (export "memory") 1)"#
            } else {
                ""
            };
            let wat = format!(
                r#"(module
                    (import "host" "{name}" (func $h {param_decl} {result_decl}))
                    {memory}
                    (func (export "call") {param_decl} {result_decl}
                        {locals} call $h))"#
            );

            let runtime = WasmRuntime::new().unwrap();
            let mut linker = wasmtime::Linker::new(runtime.engine());
            HostFunctionRegistry::new()
                .register_all(&mut linker)
                .unwrap();
            let module = Module::new(runtime.engine(), wat::parse_str(&wat).unwrap()).unwrap();
            let mut store = Store::new(runtime.engine(), data);
            let instance = linker.instantiate(&mut store, &module).unwrap();
            Self { store, instance }
        }

        fn call(&mut self, args: &[wasmtime::Val]) -> Option<wasmtime::Val> {
            let func = self.instance.get_func(&mut self.store, "call").unwrap();
            let mut results = vec![wasmtime::Val::I32(0); func.ty(&self.store).results().len()];
            func.call(&mut self.store, args, &mut results).unwrap();
            results.into_iter().next()
        }

        fn call_i32(&mut self, args: &[i32]) -> i32 {
            let args: Vec<_> = args.iter().map(|a| wasmtime::Val::I32(*a)).collect();
            self.call(&args).unwrap().unwrap_i32()
        }

        fn call_i64(&mut self, args: &[i32]) -> i64 {
            let args: Vec<_> = args.iter().map(|a| wasmtime::Val::I32(*a)).collect();
            self.call(&args).unwrap().unwrap_i64()
        }

        fn write(&mut self, offset: usize, bytes: &[u8]) {
            let memory = self.instance.get_memory(&mut self.store, "memory").unwrap();
            memory.write(&mut self.store, offset, bytes).unwrap();
        }

        fn read(&mut self, offset: usize, len: usize) -> Vec<u8> {
            let memory = self.instance.get_memory(&mut self.store, "memory").unwrap();
            let mut buf = vec![0u8; len];
            memory.read(&self.store, offset, &mut buf).unwrap();
            buf
        }

        fn read_string(&mut self, offset: usize, len: usize) -> String {
            String::from_utf8(self.read(offset, len)).unwrap()
        }

        /// Escreve `s` no offset informado e devolve (ptr, len)
        fn put(&mut self, offset: usize, s: &[u8]) -> (i32, i32) {
            self.write(offset, s);
            (offset as i32, s.len() as i32)
        }
    }

    fn i32s(n: usize) -> Vec<&'static str> {
        vec!["i32"; n]
    }

    fn le_i32s(bytes: &[u8]) -> Vec<i32> {
        bytes
            .chunks_exact(4)
            .map(|c| i32::from_le_bytes(c.try_into().unwrap()))
            .collect()
    }

    /// Todas as funções que dependem da memória exportada: (nome, nº de params i32, resultado)
    const MEMORY_FUNCTIONS: &[(&str, usize, &str)] = &[
        ("write", 2, "i32"),
        ("get_buffer_size", 1, "i32"),
        ("get_cursor_position", 1, "i32"),
        ("write_buffer", 4, "i32"),
        ("send_key_event", 2, "i32"),
        ("get_selection", 2, "i32"),
        ("set_selection", 2, "i32"),
        ("log_trace", 2, "i32"),
        ("log_debug", 2, "i32"),
        ("log_info", 2, "i32"),
        ("log_warn", 2, "i32"),
        ("log_error", 2, "i32"),
        ("log_with_metadata", 5, "i32"),
        ("get_config", 4, "i32"),
        ("get_theme", 2, "i32"),
        ("get_plugin_config", 2, "i32"),
        ("emit_event", 3, "i32"),
        ("register_command", 3, "i32"),
        ("unregister_command", 2, "i32"),
        ("execute_command", 2, "i32"),
        ("show_notification", 3, "i32"),
        ("show_alert_dialog", 4, "i32"),
        ("show_confirm_dialog", 4, "i32"),
        ("show_prompt_dialog", 8, "i32"),
        ("set_status_text", 2, "i32"),
        ("get_window_position", 1, "i32"),
        ("get_window_size", 1, "i32"),
        ("set_window_title", 2, "i32"),
        ("get_mouse_position", 1, "i32"),
        ("show_context_menu", 4, "i32"),
        ("register_status_bar_item", 3, "i32"),
        ("get_host_version", 2, "i32"),
        ("get_api_version", 1, "i32"),
        ("get_env_var", 4, "i32"),
        ("get_memory_usage", 1, "i32"),
        ("get_hostname", 2, "i32"),
        ("get_home_dir", 2, "i32"),
        ("get_config_dir", 2, "i32"),
        ("get_data_dir", 2, "i32"),
        ("get_temp_dir", 2, "i32"),
        ("read_file", 4, "i32"),
        ("write_file", 4, "i32"),
        ("file_exists", 2, "i32"),
        ("list_dir", 4, "i32"),
    ];

    #[test]
    fn test_memory_functions_fail_without_exported_memory() {
        for (name, arity, result) in MEMORY_FUNCTIONS {
            let mut guest = Guest::without_memory(name, &i32s(*arity), Some(result));
            assert_eq!(
                guest.call_i32(&vec![0; *arity]),
                -1,
                "{name} deveria falhar sem memória exportada"
            );
        }
    }

    /// Cada argumento que é ponteiro, quando aponta para fora da memória, deve
    /// fazer a função devolver -1. (nome, aridade, índices dos ponteiros)
    const POINTER_ARGS: &[(&str, usize, &[usize])] = &[
        ("write", 2, &[0]),
        ("get_buffer_size", 1, &[0]),
        ("get_cursor_position", 1, &[0]),
        ("write_buffer", 4, &[2]),
        ("send_key_event", 2, &[0]),
        ("get_selection", 2, &[0]),
        ("set_selection", 2, &[0]),
        ("log_trace", 2, &[0]),
        ("log_debug", 2, &[0]),
        ("log_info", 2, &[0]),
        ("log_warn", 2, &[0]),
        ("log_error", 2, &[0]),
        ("log_with_metadata", 5, &[1, 3]),
        ("get_config", 4, &[0, 2]),
        ("get_theme", 2, &[0]),
        ("get_plugin_config", 2, &[0]),
        ("emit_event", 3, &[1]),
        ("register_command", 3, &[0]),
        ("unregister_command", 2, &[0]),
        ("execute_command", 2, &[0]),
        ("show_notification", 3, &[1]),
        ("show_alert_dialog", 4, &[0, 2]),
        ("show_confirm_dialog", 4, &[0, 2]),
        ("show_prompt_dialog", 8, &[0, 2, 4, 6]),
        ("set_status_text", 2, &[0]),
        ("get_window_position", 1, &[0]),
        ("get_window_size", 1, &[0]),
        ("set_window_title", 2, &[0]),
        ("get_mouse_position", 1, &[0]),
        ("show_context_menu", 4, &[2]),
        ("register_status_bar_item", 3, &[1]),
        ("get_host_version", 2, &[0]),
        ("get_api_version", 1, &[0]),
        ("get_memory_usage", 1, &[0]),
        ("get_hostname", 2, &[0]),
        ("get_home_dir", 2, &[0]),
        ("get_config_dir", 2, &[0]),
        ("get_data_dir", 2, &[0]),
        ("get_temp_dir", 2, &[0]),
        ("get_env_var", 4, &[0, 2]),
        ("read_file", 4, &[0]),
        ("write_file", 4, &[0, 2]),
        ("file_exists", 2, &[0]),
        ("list_dir", 4, &[0]),
    ];

    #[test]
    fn test_out_of_bounds_pointers_return_error() {
        for (name, arity, pointers) in POINTER_ARGS {
            for pointer in pointers.iter() {
                let data = PluginStoreData::for_plugin("oob", vec![Permission::Pty]);
                let mut guest = Guest::with_data(name, &i32s(*arity), Some("i32"), data);
                // Argumentos válidos: ponteiro 0, tamanho 4 (texto "abcd" na memória),
                // nível 0; apenas o ponteiro testado aponta para fora.
                guest.write(0, b"abcd");
                let mut args = vec![0; *arity];
                for (i, arg) in args.iter_mut().enumerate() {
                    if i > 0 && pointers.contains(&(i - 1)) {
                        *arg = 4; // tamanho do buffer apontado
                    }
                }
                if *name == "get_env_var" {
                    // nome "PATH" válido; saída é o que falha em outro teste
                    guest.write(0, b"PATH");
                }
                args[*pointer] = OOB;
                assert_eq!(
                    guest.call_i32(&args),
                    -1,
                    "{name}: ponteiro no argumento {pointer} fora da memória deveria falhar"
                );
            }
        }
    }

    #[test]
    fn test_registration_error_names_the_colliding_function() {
        // Descobre todos os nomes registrados
        let runtime = WasmRuntime::new().unwrap();
        let mut linker = wasmtime::Linker::new(runtime.engine());
        HostFunctionRegistry::default()
            .register_all(&mut linker)
            .unwrap();
        let mut store = Store::new(runtime.engine(), PluginStoreData::new());
        let names: Vec<String> = linker
            .iter(&mut store)
            .map(|(module, name, _)| {
                assert_eq!(module, HOST_MODULE_NAME);
                name.to_string()
            })
            .collect();
        assert!(names.len() >= 70, "poucas host functions: {}", names.len());
        for expected in ["write", "log_info", "get_config", "emit_event", "list_dir"] {
            assert!(
                names.iter().any(|n| n == expected),
                "{expected} não registrada"
            );
        }

        // Para cada nome, um linker que já o define faz o registro falhar
        // exatamente nele, com mensagem que identifica a função.
        for name in &names {
            let mut linker: wasmtime::Linker<PluginStoreData> =
                wasmtime::Linker::new(runtime.engine());
            linker.func_wrap(HOST_MODULE_NAME, name, || {}).unwrap();
            let err = HostFunctionRegistry::new()
                .register_all(&mut linker)
                .unwrap_err();
            let message = err.to_string();
            assert!(
                message.contains(&format!("Failed to register {name}:")),
                "mensagem de erro para {name} não identifica a função: {message}"
            );
        }
    }

    // ---------------- Terminal ----------------

    #[test]
    fn test_terminal_write_functions_return_length() {
        let mut guest = Guest::new("write", &i32s(2), Some("i32"));
        let (ptr, len) = guest.put(16, b"hello");
        assert_eq!(guest.call_i32(&[ptr, len]), 5);

        let mut guest = Guest::new("write_buffer", &i32s(4), Some("i32"));
        let (ptr, len) = guest.put(16, b"abc");
        assert_eq!(guest.call_i32(&[1, 2, ptr, len]), 3);
    }

    #[test]
    fn test_terminal_placeholders_return_zero() {
        let _guard = bridge::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        bridge::reset();
        let cases: &[(&str, usize)] = &[
            ("read_input", 2),
            ("set_buffer_size", 2),
            ("set_cursor_position", 2),
            ("read_buffer", 5),
            ("clear_buffer", 0),
            ("scroll_buffer", 1),
        ];
        for (name, arity) in cases {
            let mut guest = Guest::new(name, &i32s(*arity), Some("i32"));
            assert_eq!(guest.call_i32(&vec![1; *arity]), 0, "{name}");
        }

        let mut guest = Guest::new("set_selection", &i32s(2), Some("i32"));
        let (ptr, len) = guest.put(0, b"ctrl+c");
        assert_eq!(guest.call_i32(&[ptr, len]), 0, "set_selection");
    }

    /// Sem a permissão `pty` o plugin não digita no shell do usuário.
    #[test]
    fn test_send_key_event_requires_pty_permission() {
        let _guard = bridge::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        bridge::reset();
        let mut guest = Guest::with_data(
            "send_key_event",
            &i32s(2),
            Some("i32"),
            PluginStoreData::for_plugin("sem-pty", Vec::new()),
        );
        let (ptr, len) = guest.put(0, b"rm -rf\n");
        assert_eq!(guest.call_i32(&[ptr, len]), -2);

        let data = PluginStoreData::for_plugin("com-pty", vec![Permission::Pty]);
        let mut guest = Guest::with_data("send_key_event", &i32s(2), Some("i32"), data);
        let (ptr, len) = guest.put(0, b"ls\n");
        assert_eq!(guest.call_i32(&[ptr, len]), len);
        let sent: Vec<_> = bridge::drain()
            .into_iter()
            .filter(|r| r.plugin == "com-pty" || r.plugin == "sem-pty")
            .collect();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].command, HostCommand::SendInput("ls\n".into()));
        bridge::reset();
    }

    /// As funções de terminal leem a visão que o app publica e mandam
    /// pedidos ao app pela ponte.
    #[test]
    fn test_terminal_functions_use_app_bridge() {
        let _guard = bridge::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        bridge::reset();
        bridge::publish_view(bridge::HostView {
            cols: 120,
            rows: 40,
            cursor_col: 3,
            cursor_row: 2,
            lines: vec!["$ cargo test".into(), "ok: 10 passed".into()],
            tab_id: 7,
            pane_id: 9,
            selection: "passed".into(),
            theme: "dark".into(),
        });
        let mut size = Guest::new("get_buffer_size", &i32s(1), Some("i32"));
        assert_eq!(size.call_i32(&[32]), 0);
        assert_eq!(le_i32s(&size.read(32, 8)), vec![120, 40]);
        let mut cursor = Guest::new("get_cursor_position", &i32s(1), Some("i32"));
        assert_eq!(cursor.call_i32(&[32]), 0);
        assert_eq!(le_i32s(&cursor.read(32, 8)), vec![3, 2]);
        for (name, id) in [("get_tab_id", 7), ("get_grid_id", 9)] {
            let mut guest = Guest::new(name, &[], Some("i64"));
            assert_eq!(guest.call_i64(&[]), id, "{name}");
        }

        let mut read = Guest::new("read_buffer", &i32s(5), Some("i32"));
        let n = read.call_i32(&[0, 0, 80, 1, 64]);
        assert_eq!(read.read(64, n as usize), b"$ cargo test\nok: 10 passed");
        assert_eq!(read.call_i32(&[5, 0, 1, 0, 64]), -1, "retângulo invertido");

        let mut selection = Guest::new("get_selection", &i32s(2), Some("i32"));
        assert_eq!(
            selection.call_i32(&[64, 3]),
            3,
            "trunca no tamanho do buffer"
        );
        assert_eq!(selection.read(64, 3), b"pas");

        bridge::push_input("make\n");
        let mut input = Guest::new("read_input", &i32s(2), Some("i32"));
        assert_eq!(input.call_i32(&[64, 32]), 5);
        assert_eq!(input.read(64, 5), b"make\n");
        assert_eq!(input.call_i32(&[64, 32]), 0, "fila vazia");

        let data = PluginStoreData::for_plugin("ponte", Vec::new());
        let mut write = Guest::with_data("write", &i32s(2), Some("i32"), data);
        let (ptr, len) = write.put(0, b"oi");
        assert_eq!(write.call_i32(&[ptr, len]), 2);
        let data = PluginStoreData::for_plugin("ponte", Vec::new());
        let mut notify = Guest::with_data("show_notification", &i32s(3), Some("i32"), data);
        let (ptr, len) = notify.put(0, b"build ok");
        assert_eq!(notify.call_i32(&[1, ptr, len]), 0);
        let data = PluginStoreData::for_plugin("ponte", Vec::new());
        let mut status = Guest::with_data("set_status_text", &i32s(2), Some("i32"), data);
        let (ptr, len) = status.put(0, b"3 testes");
        assert_eq!(status.call_i32(&[ptr, len]), 0);

        let mine: Vec<_> = bridge::drain()
            .into_iter()
            .filter(|r| r.plugin == "ponte")
            .map(|r| r.command)
            .collect();
        assert_eq!(
            mine,
            vec![
                HostCommand::Write("oi".into()),
                HostCommand::Notify {
                    level: 1,
                    message: "build ok".into()
                },
                HostCommand::Status("3 testes".into()),
            ]
        );
        bridge::reset();
    }

    #[test]
    fn test_buffer_region_capacity_and_validation() {
        assert_eq!(
            BufferRegion::new(0, 0, 10, 1).map(|r| r.capacity()),
            Some(82)
        );
        assert!(BufferRegion::new(-1, 0, 10, 1).is_none());
        assert!(BufferRegion::new(0, 3, 10, 1).is_none());
    }

    #[test]
    fn test_get_buffer_size_writes_cols_and_rows() {
        let _guard = bridge::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        bridge::reset();
        let mut guest = Guest::new("get_buffer_size", &i32s(1), Some("i32"));
        assert_eq!(guest.call_i32(&[32]), 0);
        assert_eq!(le_i32s(&guest.read(32, 8)), vec![80, 24]);
    }

    #[test]
    fn test_position_functions_write_origin() {
        let _guard = bridge::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        bridge::reset();
        for name in [
            "get_cursor_position",
            "get_window_position",
            "get_mouse_position",
        ] {
            let mut guest = Guest::new(name, &i32s(1), Some("i32"));
            guest.write(8, &[0xAA; 8]);
            assert_eq!(guest.call_i32(&[8]), 0, "{name}");
            assert_eq!(le_i32s(&guest.read(8, 8)), vec![0, 0], "{name}");
        }
    }

    #[test]
    fn test_get_selection_is_empty() {
        let _guard = bridge::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        bridge::reset();
        let mut guest = Guest::new("get_selection", &i32s(2), Some("i32"));
        assert_eq!(guest.call_i32(&[0, 64]), 0);
    }

    // ---------------- Logging ----------------

    #[test]
    fn test_log_functions_return_message_length() {
        for name in [
            "log_trace",
            "log_debug",
            "log_info",
            "log_warn",
            "log_error",
        ] {
            let mut guest = Guest::new(name, &i32s(2), Some("i32"));
            let (ptr, len) = guest.put(100, "olá plugin".as_bytes());
            assert_eq!(guest.call_i32(&[ptr, len]), len, "{name}");
        }
    }

    #[test]
    fn test_log_with_metadata_levels() {
        for level in 0..=5 {
            let mut guest = Guest::new("log_with_metadata", &i32s(5), Some("i32"));
            let (msg_ptr, msg_len) = guest.put(0, b"mensagem");
            let (meta_ptr, meta_len) = guest.put(64, br#"{"k":"v"}"#);
            assert_eq!(
                guest.call_i32(&[level, msg_ptr, msg_len, meta_ptr, meta_len]),
                msg_len,
                "nível {level}"
            );
        }

        let mut guest = Guest::new("log_with_metadata", &i32s(5), Some("i32"));
        let (msg_ptr, msg_len) = guest.put(0, b"x");
        assert_eq!(guest.call_i32(&[42, msg_ptr, msg_len, 0, 0]), -1);
    }

    #[test]
    fn test_set_and_get_log_level() {
        // Usa o mesmo nível que o teste de logging.rs para não haver corrida
        let error = crate::plugin::host::logging::LogLevel::Error as i32;
        let mut set = Guest::new("set_log_level", &i32s(1), Some("i32"));
        assert_eq!(set.call_i32(&[error]), 0);
        assert_eq!(set.call_i32(&[99]), -1);

        let mut get = Guest::new("get_log_level", &[], Some("i32"));
        assert_eq!(get.call_i32(&[]), error);
    }

    // ---------------- Config ----------------

    #[test]
    fn test_get_config_formats_and_truncates_value() {
        let mut guest = Guest::new("get_config", &i32s(4), Some("i32"));
        let (key_ptr, key_len) = guest.put(0, b"font");
        let expected = b"config_font_value";
        assert_eq!(
            guest.call_i32(&[key_ptr, key_len, 100, 64]),
            expected.len() as i32
        );
        assert_eq!(guest.read(100, expected.len()), expected);

        // Buffer menor que o valor: trunca
        assert_eq!(guest.call_i32(&[key_ptr, key_len, 200, 6]), 6);
        assert_eq!(guest.read_string(200, 6), "config");
    }

    #[test]
    fn test_get_theme_returns_default() {
        let mut guest = Guest::new("get_theme", &i32s(2), Some("i32"));
        assert_eq!(guest.call_i32(&[0, 32]), 7);
        assert_eq!(guest.read_string(0, 7), "default");
        assert_eq!(guest.call_i32(&[40, 3]), 3);
        assert_eq!(guest.read_string(40, 3), "def");
    }

    #[test]
    fn test_get_plugin_config_reads_store_data() {
        let mut data = PluginStoreData::new();
        data.store_data("config", br#"{"a":1}"#.to_vec());
        let mut guest = Guest::build("get_plugin_config", &i32s(2), Some("i32"), true, data);
        assert_eq!(guest.call_i32(&[0, 64]), 7);
        assert_eq!(guest.read_string(0, 7), r#"{"a":1}"#);

        // Sem config no store: nada escrito
        let mut empty = Guest::new("get_plugin_config", &i32s(2), Some("i32"));
        assert_eq!(empty.call_i32(&[0, 64]), 0);
    }

    // ---------------- Events ----------------

    #[test]
    fn test_event_functions() {
        for name in ["subscribe_event", "unsubscribe_event"] {
            let mut guest = Guest::new(name, &i32s(2), Some("i32"));
            assert_eq!(guest.call_i32(&[3, 7]), 0, "{name}");
        }

        let mut guest = Guest::new("emit_event", &i32s(3), Some("i32"));
        let (ptr, len) = guest.put(0, br#"{"x":1}"#);
        assert_eq!(guest.call_i32(&[1, ptr, len]), 0);

        let mut guest = Guest::new("register_command", &i32s(3), Some("i32"));
        let (ptr, len) = guest.put(0, b"meu.comando");
        assert_eq!(guest.call_i32(&[ptr, len, 9]), 0);

        for name in ["unregister_command", "execute_command"] {
            let mut guest = Guest::new(name, &i32s(2), Some("i32"));
            let (ptr, len) = guest.put(0, b"meu.comando");
            assert_eq!(guest.call_i32(&[ptr, len]), 0, "{name}");
        }

        for name in ["get_event_queue_size", "flush_events"] {
            let mut guest = Guest::new(name, &[], Some("i32"));
            assert_eq!(guest.call_i32(&[]), 0, "{name}");
        }
    }

    #[test]
    fn test_timers_return_callback_id() {
        for name in ["set_timeout", "set_interval"] {
            let mut guest = Guest::new(name, &i32s(2), Some("i64"));
            assert_eq!(guest.call_i64(&[250, 42]), 42, "{name}");
        }

        let mut guest = Guest::new("clear_timer", &["i64"], Some("i32"));
        let result = guest.call(&[wasmtime::Val::I64(42)]).unwrap();
        assert_eq!(result.unwrap_i32(), 0);
    }

    // ---------------- UI ----------------

    #[test]
    fn test_show_notification_accepts_all_levels() {
        for level in 0..=4 {
            let mut guest = Guest::new("show_notification", &i32s(3), Some("i32"));
            let (ptr, len) = guest.put(0, b"aviso");
            assert_eq!(guest.call_i32(&[level, ptr, len]), 0, "nível {level}");
        }
    }

    #[test]
    fn test_dialogs() {
        let mut alert = Guest::new("show_alert_dialog", &i32s(4), Some("i32"));
        let (t, tl) = alert.put(0, b"Titulo");
        let (m, ml) = alert.put(32, b"Mensagem");
        assert_eq!(alert.call_i32(&[t, tl, m, ml]), 0);

        let mut confirm = Guest::new("show_confirm_dialog", &i32s(4), Some("i32"));
        let (t, tl) = confirm.put(0, b"Titulo");
        let (m, ml) = confirm.put(32, b"Confirma?");
        assert_eq!(confirm.call_i32(&[t, tl, m, ml]), 1);
    }

    #[test]
    fn test_prompt_dialog_returns_default_value() {
        let mut guest = Guest::new("show_prompt_dialog", &i32s(8), Some("i32"));
        let (t, tl) = guest.put(0, b"Nome");
        let (m, ml) = guest.put(16, b"Digite");
        let (d, dl) = guest.put(32, b"fulano");
        assert_eq!(guest.call_i32(&[t, tl, m, ml, d, dl, 128, 64]), 6);
        assert_eq!(guest.read_string(128, 6), "fulano");

        // Saída menor que o padrão: trunca
        assert_eq!(guest.call_i32(&[t, tl, m, ml, d, dl, 256, 3]), 3);
        assert_eq!(guest.read_string(256, 3), "ful");

        // Saída fora da memória
        assert_eq!(guest.call_i32(&[t, tl, m, ml, d, dl, OOB, 3]), -1);
    }

    #[test]
    fn test_status_and_window_functions() {
        let mut guest = Guest::new("set_status_text", &i32s(2), Some("i32"));
        let (ptr, len) = guest.put(0, b"pronto");
        assert_eq!(guest.call_i32(&[ptr, len]), 0);

        let mut guest = Guest::new("clear_status_text", &[], Some("i32"));
        assert_eq!(guest.call_i32(&[]), 0);

        let mut guest = Guest::new("set_window_title", &i32s(2), Some("i32"));
        let (ptr, len) = guest.put(0, b"codeterm");
        assert_eq!(guest.call_i32(&[ptr, len]), 0);

        let mut guest = Guest::new("get_window_size", &i32s(1), Some("i32"));
        assert_eq!(guest.call_i32(&[12]), 0);
        assert_eq!(le_i32s(&guest.read(12, 8)), vec![800, 600]);
    }

    #[test]
    fn test_overlay_and_status_bar_functions() {
        for name in ["show_overlay", "update_overlay"] {
            let mut guest = Guest::new(name, &i32s(5), Some("i32"));
            assert_eq!(guest.call_i32(&[1, 10, 20, 300, 200]), 0, "{name}");
        }
        for name in ["hide_overlay", "unregister_status_bar_item"] {
            let mut guest = Guest::new(name, &i32s(1), Some("i32"));
            assert_eq!(guest.call_i32(&[1]), 0, "{name}");
        }

        let mut guest = Guest::new("show_context_menu", &i32s(4), Some("i32"));
        let (ptr, len) = guest.put(0, br#"["copiar","colar"]"#);
        assert_eq!(guest.call_i32(&[5, 6, ptr, len]), 0);

        let mut guest = Guest::new("register_status_bar_item", &i32s(3), Some("i32"));
        let (ptr, len) = guest.put(0, b"git: main");
        assert_eq!(guest.call_i32(&[1, ptr, len]), 0);
    }

    // ---------------- System ----------------

    #[test]
    fn test_get_host_version() {
        let mut guest = Guest::new("get_host_version", &i32s(2), Some("i32"));
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(guest.call_i32(&[0, 64]), version.len() as i32);
        assert_eq!(guest.read_string(0, version.len()), version);

        // Tamanho/ponteiro negativos são rejeitados
        assert_eq!(guest.call_i32(&[0, -1]), -1);
        assert_eq!(guest.call_i32(&[-1, 8]), -1);
    }

    #[test]
    fn test_get_api_version_and_memory_usage() {
        let mut guest = Guest::new("get_api_version", &i32s(1), Some("i32"));
        assert_eq!(guest.call_i32(&[0]), 0);
        assert_eq!(le_i32s(&guest.read(0, 12)), vec![1, 0, 0]);

        let mut guest = Guest::new("get_memory_usage", &i32s(1), Some("i32"));
        guest.write(0, &[0xFF; 16]);
        assert_eq!(guest.call_i32(&[0]), 0);
        assert_eq!(guest.read(0, 16), vec![0; 16]);
    }

    #[test]
    fn test_platform_and_cpu() {
        use crate::plugin::host::system::Platform;
        let mut guest = Guest::new("get_platform", &[], Some("i32"));
        assert_eq!(guest.call_i32(&[]), Platform::current() as i32);

        let mut guest = Guest::new("get_cpu_usage", &[], Some("f32"));
        assert_eq!(guest.call(&[]).unwrap().unwrap_f32(), 0.0);
    }

    #[test]
    fn test_time_functions_are_monotonic_with_system_clock() {
        let now_ms = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
        };
        for name in ["get_current_time", "get_time_millis"] {
            let before = now_ms();
            let mut guest = Guest::new(name, &[], Some("i64"));
            let value = guest.call_i64(&[]);
            assert!(value >= before && value <= now_ms(), "{name}: {value}");
        }

        let before_us = now_ms() * 1000;
        let mut guest = Guest::new("get_time_micros", &[], Some("i64"));
        let micros = guest.call_i64(&[]);
        assert!(micros >= before_us && micros <= (now_ms() + 1) * 1000);

        let mut guest = Guest::new("get_uptime", &[], Some("i64"));
        let first = guest.call_i64(&[]);
        let second = guest.call_i64(&[]);
        assert!(first >= 0 && second >= first);
    }

    #[test]
    fn test_get_env_var() {
        let mut guest = Guest::new("get_env_var", &i32s(4), Some("i32"));
        let expected = std::env::var("PATH").unwrap();
        let (name_ptr, name_len) = guest.put(0, b"PATH");
        let written = guest.call_i32(&[name_ptr, name_len, 1024, 8192]);
        let expected_len = expected.len().min(8192);
        assert_eq!(written as usize, expected_len);
        assert_eq!(
            guest.read_string(1024, expected_len),
            expected[..expected_len]
        );

        // Variável inexistente
        let (name_ptr, name_len) = guest.put(0, b"CODETERM_VAR_QUE_NAO_EXISTE_123");
        assert_eq!(guest.call_i32(&[name_ptr, name_len, 1024, 64]), -1);

        // Nome com UTF-8 inválido
        let (name_ptr, name_len) = guest.put(0, &[0xff, 0xfe]);
        assert_eq!(guest.call_i32(&[name_ptr, name_len, 1024, 64]), -1);
    }

    #[test]
    fn test_directory_functions_match_host() {
        let cases: Vec<(&str, Option<String>)> = vec![
            (
                "get_hostname",
                hostname::get()
                    .ok()
                    .map(|h| h.to_string_lossy().into_owned()),
            ),
            (
                "get_home_dir",
                dirs::home_dir().map(|p| p.to_string_lossy().into_owned()),
            ),
            (
                "get_config_dir",
                dirs::config_dir().map(|p| p.join("codeterm").to_string_lossy().into_owned()),
            ),
            (
                "get_data_dir",
                dirs::data_dir().map(|p| p.join("codeterm").to_string_lossy().into_owned()),
            ),
            (
                "get_temp_dir",
                Some(std::env::temp_dir().to_string_lossy().into_owned()),
            ),
        ];
        for (name, expected) in cases {
            let mut guest = Guest::new(name, &i32s(2), Some("i32"));
            let written = guest.call_i32(&[0, 4096]);
            match expected {
                Some(expected) => {
                    assert_eq!(written as usize, expected.len(), "{name}");
                    assert_eq!(guest.read_string(0, expected.len()), expected, "{name}");
                }
                None => assert_eq!(written, -1, "{name}"),
            }
        }
    }

    #[test]
    fn test_file_functions_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("dados.txt");
        let file_str = file.to_string_lossy().into_owned();

        // write_file
        let mut guest = Guest::new("write_file", &i32s(4), Some("i32"));
        let (path_ptr, path_len) = guest.put(0, file_str.as_bytes());
        let (data_ptr, data_len) = guest.put(1024, b"conteudo do plugin");
        assert_eq!(
            guest.call_i32(&[path_ptr, path_len, data_ptr, data_len]),
            18
        );
        assert_eq!(std::fs::read(&file).unwrap(), b"conteudo do plugin");

        // write_file em diretório inexistente falha
        let bad = dir.path().join("nao/existe.txt");
        let (bad_ptr, bad_len) = guest.put(2048, bad.to_string_lossy().as_bytes());
        assert_eq!(guest.call_i32(&[bad_ptr, bad_len, data_ptr, data_len]), -1);

        // read_file
        let mut guest = Guest::new("read_file", &i32s(4), Some("i32"));
        let (path_ptr, path_len) = guest.put(0, file_str.as_bytes());
        assert_eq!(guest.call_i32(&[path_ptr, path_len, 1024, 4096]), 18);
        assert_eq!(guest.read_string(1024, 18), "conteudo do plugin");
        // buffer pequeno trunca
        assert_eq!(guest.call_i32(&[path_ptr, path_len, 2048, 8]), 8);
        assert_eq!(guest.read_string(2048, 8), "conteudo");
        // arquivo inexistente
        let (bad_ptr, bad_len) = guest.put(3000, bad.to_string_lossy().as_bytes());
        assert_eq!(guest.call_i32(&[bad_ptr, bad_len, 1024, 64]), -1);

        // file_exists
        let mut guest = Guest::new("file_exists", &i32s(2), Some("i32"));
        let (path_ptr, path_len) = guest.put(0, file_str.as_bytes());
        assert_eq!(guest.call_i32(&[path_ptr, path_len]), 1);
        let (bad_ptr, bad_len) = guest.put(2048, bad.to_string_lossy().as_bytes());
        assert_eq!(guest.call_i32(&[bad_ptr, bad_len]), 0);
    }

    #[test]
    fn test_list_dir_returns_json_entries() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"a").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();

        let mut guest = Guest::new("list_dir", &i32s(4), Some("i32"));
        let (ptr, len) = guest.put(0, dir_str.as_bytes());
        let written = guest.call_i32(&[ptr, len, 1024, 16384]);
        assert!(written > 0);
        let json: Vec<serde_json::Value> =
            serde_json::from_slice(&guest.read(1024, written as usize)).unwrap();
        assert_eq!(json.len(), 2);
        let file = json.iter().find(|e| e["name"] == "a.txt").unwrap();
        assert_eq!(file["is_file"], true);
        assert_eq!(file["is_dir"], false);
        let sub = json.iter().find(|e| e["name"] == "sub").unwrap();
        assert_eq!(sub["is_dir"], true);

        // Diretório inexistente
        let missing = dir.path().join("nada");
        let (ptr, len) = guest.put(0, missing.to_string_lossy().as_bytes());
        assert_eq!(guest.call_i32(&[ptr, len, 1024, 64]), -1);
    }

    // ---------------- Helpers públicos com Caller ----------------

    #[test]
    fn test_caller_helpers_allocate_and_strings() {
        let runtime = WasmRuntime::new().unwrap();
        let mut linker: wasmtime::Linker<PluginStoreData> = wasmtime::Linker::new(runtime.engine());

        // test.roundtrip(): aloca 16 bytes via malloc do guest, escreve "ok",
        // lê de volta e libera. Retorna o ponteiro alocado ou -1.
        linker
            .func_wrap(
                "test",
                "roundtrip",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>| -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(m)) => m,
                        _ => return -2,
                    };
                    let ptr = match allocate_in_wasm(&mut caller, 16) {
                        Ok(p) => p,
                        Err(e) => {
                            assert!(e.contains("malloc not found"), "{e}");
                            // free é opcional: sem export "free" é no-op
                            free_in_wasm(&mut caller, 0).unwrap();
                            return -1;
                        }
                    };
                    write_string_caller(&mut caller, &memory, ptr as usize, "ok").unwrap();
                    let s = read_string_caller(&caller, &memory, ptr as usize, 2).unwrap();
                    assert_eq!(s, "ok");
                    // Bytes inválidos em UTF-8 são rejeitados
                    write_memory_caller(&mut caller, &memory, ptr as usize, &[0xff]).unwrap();
                    let invalid = read_string_caller(&caller, &memory, ptr as usize, 1);
                    assert!(invalid.unwrap_err().contains("Invalid UTF-8"));
                    // Leitura/escrita fora da memória
                    assert!(read_memory_caller(&caller, &memory, 70_000, 4)
                        .unwrap_err()
                        .contains("Failed to read memory"));
                    assert!(write_memory_caller(&mut caller, &memory, 70_000, b"x")
                        .unwrap_err()
                        .contains("Failed to write memory"));
                    free_in_wasm(&mut caller, ptr).unwrap();
                    ptr
                },
            )
            .unwrap();

        let with_malloc = r#"(module
            (import "test" "roundtrip" (func $rt (result i32)))
            (memory (export "memory") 1)
            (global $freed (mut i32) (i32.const 0))
            (func (export "malloc") (param i32) (result i32) i32.const 256)
            (func (export "free") (param i32) local.get 0 global.set $freed)
            (func (export "freed") (result i32) global.get $freed)
            (func (export "call") (result i32) call $rt))"#;
        let module = Module::new(runtime.engine(), wat::parse_str(with_malloc).unwrap()).unwrap();
        let mut store = Store::new(runtime.engine(), PluginStoreData::new());
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let call = instance
            .get_typed_func::<(), i32>(&mut store, "call")
            .unwrap();
        assert_eq!(call.call(&mut store, ()).unwrap(), 256);
        let freed = instance
            .get_typed_func::<(), i32>(&mut store, "freed")
            .unwrap();
        assert_eq!(freed.call(&mut store, ()).unwrap(), 256);

        // Sem malloc: allocate falha; sem free: free é no-op
        let without_malloc = r#"(module
            (import "test" "roundtrip" (func $rt (result i32)))
            (memory (export "memory") 1)
            (func (export "call") (result i32) call $rt))"#;
        let module =
            Module::new(runtime.engine(), wat::parse_str(without_malloc).unwrap()).unwrap();
        let mut store = Store::new(runtime.engine(), PluginStoreData::new());
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let call = instance
            .get_typed_func::<(), i32>(&mut store, "call")
            .unwrap();
        assert_eq!(call.call(&mut store, ()).unwrap(), -1);
    }

    #[test]
    fn test_allocate_and_free_propagate_guest_traps() {
        let runtime = WasmRuntime::new().unwrap();
        let mut linker: wasmtime::Linker<PluginStoreData> = wasmtime::Linker::new(runtime.engine());
        linker
            .func_wrap(
                "test",
                "probe",
                |mut caller: wasmtime::Caller<'_, PluginStoreData>| -> i32 {
                    let alloc_err = allocate_in_wasm(&mut caller, 8).unwrap_err();
                    assert!(alloc_err.contains("malloc failed"), "{alloc_err}");
                    let free_err = free_in_wasm(&mut caller, 8).unwrap_err();
                    assert!(free_err.contains("free failed"), "{free_err}");
                    0
                },
            )
            .unwrap();

        let wat = r#"(module
            (import "test" "probe" (func $p (result i32)))
            (memory (export "memory") 1)
            (func (export "malloc") (param i32) (result i32) unreachable)
            (func (export "free") (param i32) unreachable)
            (func (export "call") (result i32) call $p))"#;
        let module = Module::new(runtime.engine(), wat::parse_str(wat).unwrap()).unwrap();
        let mut store = Store::new(runtime.engine(), PluginStoreData::new());
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let call = instance
            .get_typed_func::<(), i32>(&mut store, "call")
            .unwrap();
        assert_eq!(call.call(&mut store, ()).unwrap(), 0);
    }
}
