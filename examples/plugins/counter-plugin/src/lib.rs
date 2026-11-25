//! Counter Plugin - Example WASM Plugin for Code Terminal
//! 
//! This plugin demonstrates:
//! - Command counting
//! - Host function calls (logging, terminal output)
//! - Configuration reading
//! - Special command handling (/counter-reset, /counter-status)

use std::sync::atomic::{AtomicU64, Ordering};

// =============================================================================
// Host Function Imports (provided by the Code Terminal runtime)
// =============================================================================

extern "C" {
    /// Write text to the terminal
    /// ptr: pointer to UTF-8 string in WASM memory
    /// len: length of string in bytes
    fn host_write(ptr: i32, len: i32);

    /// Log message at INFO level
    fn host_log_info(ptr: i32, len: i32);

    /// Log message at DEBUG level
    fn host_log_debug(ptr: i32, len: i32);

    /// Log message at WARN level
    fn host_log_warn(ptr: i32, len: i32);

    /// Log message at ERROR level
    fn host_log_error(ptr: i32, len: i32);

    /// Read plugin configuration value
    /// key_ptr: pointer to key string
    /// key_len: length of key string
    /// out_ptr: pointer to output buffer
    /// out_len: length of output buffer
    /// Returns: actual length of value (0 if not found, -1 on error)
    fn host_get_plugin_config(key_ptr: i32, key_len: i32, out_ptr: i32, out_len: i32) -> i32;

    /// Get terminal buffer size
    /// out_ptr: pointer to [rows: u32, cols: u32] output
    fn host_get_buffer_size(out_ptr: i32);

    /// Set timer for periodic updates
    /// interval_ms: timer interval in milliseconds
    fn host_set_timer(interval_ms: i32);

    /// Clear timer
    fn host_clear_timer();
}

// =============================================================================
// Plugin State
// =============================================================================

/// Global command counter (thread-safe)
static COMMAND_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Current threshold for notifications (read from config)
static mut THRESHOLD: u64 = 10;

/// Whether notifications are enabled
static mut NOTIFICATIONS_ENABLED: bool = true;

/// Cached notification message template
static mut NOTIFICATION_TEMPLATE: Option<String> = None;

// =============================================================================
// Utility Functions
// =============================================================================

/// Write a string to the terminal via host function
fn terminal_write(s: &str) {
    unsafe {
        host_write(s.as_ptr() as i32, s.len() as i32);
    }
}

/// Log at INFO level
fn log_info(s: &str) {
    unsafe {
        host_log_info(s.as_ptr() as i32, s.len() as i32);
    }
}

/// Log at DEBUG level
fn log_debug(s: &str) {
    unsafe {
        host_log_debug(s.as_ptr() as i32, s.len() as i32);
    }
}

/// Log at WARN level
fn log_warn(s: &str) {
    unsafe {
        host_log_warn(s.as_ptr() as i32, s.len() as i32);
    }
}

/// Log at ERROR level
fn log_error(s: &str) {
    unsafe {
        host_log_error(s.as_ptr() as i32, s.len() as i32);
    }
}

/// Read a string value from plugin config
fn get_config_string(key: &str) -> Option<String> {
    unsafe {
        let mut buffer = vec![0u8; 256];
        let result = host_get_plugin_config(
            key.as_ptr() as i32,
            key.len() as i32,
            buffer.as_mut_ptr() as i32,
            buffer.len() as i32,
        );

        if result > 0 {
            let len = result as usize;
            if len <= buffer.len() {
                String::from_utf8(buffer[..len].to_vec()).ok()
            } else {
                // Value too long, reallocate and retry
                buffer = vec![0u8; len];
                let result2 = host_get_plugin_config(
                    key.as_ptr() as i32,
                    key.len() as i32,
                    buffer.as_mut_ptr() as i32,
                    buffer.len() as i32,
                );
                if result2 > 0 {
                    String::from_utf8(buffer[..result2 as usize].to_vec()).ok()
                } else {
                    None
                }
            }
        } else {
            None
        }
    }
}

/// Get config value as u64 with default
fn get_config_u64(key: &str, default: u64) -> u64 {
    get_config_string(key)
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(default)
}

/// Get config value as bool with default
fn get_config_bool(key: &str, default: bool) -> bool {
    get_config_string(key)
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(default)
}

/// Load configuration from host
fn load_config() {
    unsafe {
        THRESHOLD = get_config_u64("threshold", 10);
        NOTIFICATIONS_ENABLED = get_config_bool("enable_notifications", true);
        NOTIFICATION_TEMPLATE = get_config_string("notification_message")
            .or_else(|| Some("🎉 {count} commands executed! (threshold: {threshold})".to_string()));
    }

    log_debug("Configuration loaded");
}

// =============================================================================
// Plugin Lifecycle Functions (called by host)
// =============================================================================

/// Plugin initialization - called once when plugin is loaded
/// 
/// # Safety
/// Called by the host runtime. Initializes plugin state and loads configuration.
#[no_mangle]
pub extern "C" fn init() {
    log_info("Counter Plugin initialized");
    load_config();

    // Reset counter
    COMMAND_COUNTER.store(0, Ordering::SeqCst);

    // Set up timer for periodic updates (every 5 seconds)
    unsafe {
        host_set_timer(5000);
    }

    // Welcome message
    terminal_write("\r\n[Counter Plugin] Active. Commands will be counted.\r\n");
    terminal_write("[Counter Plugin] Use /counter-status or /counter-reset\r\n\r\n");
}

/// Plugin update - called periodically by timer
/// 
/// # Safety
/// Called by the host runtime. Safe to call multiple times.
#[no_mangle]
pub extern "C" fn update() {
    // This is called every 5 seconds (as configured in init)
    // Could be used for periodic tasks like saving state
    log_debug("Counter plugin update tick");
}

/// Plugin shutdown - called when plugin is being unloaded
/// 
/// # Safety
/// Called by the host runtime. Perform cleanup here.
#[no_mangle]
pub extern "C" fn shutdown() {
    unsafe {
        host_clear_timer();
    }

    let count = COMMAND_COUNTER.load(Ordering::SeqCst);
    log_info(&format!("Counter Plugin shutting down. Total commands: {}", count));

    terminal_write("\r\n[Counter Plugin] Goodbye!\r\n");
}

// =============================================================================
// Event Handlers (called by host on specific events)
// =============================================================================

/// Called when user input is received
/// 
/// # Arguments
/// * `ptr` - Pointer to input string in WASM memory
/// * `len` - Length of input string in bytes
/// 
/// # Returns
/// 1 if the plugin handled the input (should not be passed to shell)
/// 0 if the input should be processed normally
/// 
/// # Safety
/// The caller must ensure ptr points to valid UTF-8 data of length len.
#[no_mangle]
pub unsafe extern "C" fn on_input(ptr: i32, len: i32) -> i32 {
    // Reconstruct input string from raw parts
    let input = std::str::from_utf8(
        std::slice::from_raw_parts(ptr as *const u8, len as usize)
    ).unwrap_or("");

    // Check for plugin commands
    if input.trim() == "/counter-reset" {
        COMMAND_COUNTER.store(0, Ordering::SeqCst);
        terminal_write("\r\n[Counter Plugin] Counter reset to 0!\r\n");
        log_info("Counter reset by user");
        return 1; // Handled
    }

    if input.trim() == "/counter-status" {
        let count = COMMAND_COUNTER.load(Ordering::SeqCst);
        let threshold = unsafe { THRESHOLD };
        terminal_write(&format!(
            "\r\n[Counter Plugin] Count: {} / Threshold: {}\r\n",
            count, threshold
        ));
        return 1; // Handled
    }

    // Count as a command (only if it looks like a command, not just Enter)
    if !input.trim().is_empty() && !input.starts_with('/') {
        let new_count = COMMAND_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
        let threshold = unsafe { THRESHOLD };

        // Log at debug level
        log_debug(&format!("Command count: {}", new_count));

        // Check if we hit the threshold
        if new_count % threshold == 0 {
            unsafe {
                if NOTIFICATIONS_ENABLED {
                    let template = NOTIFICATION_TEMPLATE
                        .as_ref()
                        .map(|s| s.as_str())
                        .unwrap_or("🎉 {count} commands!");

                    let message = template
                        .replace("{count}", &new_count.to_string())
                        .replace("{threshold}", &threshold.to_string());

                    terminal_write(&format!("\r\n[Counter Plugin] {}\r\n", message));
                    log_info(&format!("Threshold reached: {} commands", new_count));
                }
            }
        }
    }

    0 // Not handled, pass to shell
}

/// Called when a new tab is opened
/// 
/// # Safety
/// Called by the host runtime.
#[no_mangle]
pub extern "C" fn on_tab_opened() {
    log_debug("New tab opened");
}

/// Called when a tab is closed
/// 
/// # Safety
/// Called by the host runtime.
#[no_mangle]
pub extern "C" fn on_tab_closed() {
    log_debug("Tab closed");
}

/// Called when theme changes
/// 
/// # Arguments
/// * `is_dark` - 1 if dark theme, 0 if light theme
/// 
/// # Safety
/// Called by the host runtime.
#[no_mangle]
pub extern "C" fn on_theme_changed(is_dark: i32) {
    let theme = if is_dark != 0 { "dark" } else { "light" };
    log_debug(&format!("Theme changed to: {}", theme));
}

/// Called when configuration is updated
/// 
/// # Safety
/// Called by the host runtime. Reloads configuration from host.
#[no_mangle]
pub extern "C" fn on_config_updated() {
    log_info("Configuration updated, reloading...");
    load_config();

    let count = COMMAND_COUNTER.load(Ordering::SeqCst);
    let threshold = unsafe { THRESHOLD };

    terminal_write(&format!(
        "\r\n[Counter Plugin] Config reloaded. Count: {}, Threshold: {}\r\n",
        count, threshold
    ));
}

// =============================================================================
// WASI Compatibility
// =============================================================================

/// Provide a custom panic handler
#[cfg(target_arch = "wasm32")]
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // In a real implementation, we might want to log this
    core::arch::wasm32::unreachable()
}

/// Provide custom allocator for no_std environments if needed
// Note: WASI target includes a default allocator, so we don't need to provide one

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_increment() {
        COMMAND_COUNTER.store(0, Ordering::SeqCst);
        let count = COMMAND_COUNTER.fetch_add(1, Ordering::SeqCst);
        assert_eq!(count, 0);

        let new_count = COMMAND_COUNTER.load(Ordering::SeqCst);
        assert_eq!(new_count, 1);
    }

    #[test]
    fn test_threshold_calculation() {
        // Simulate counting up to threshold
        COMMAND_COUNTER.store(9, Ordering::SeqCst);
        let new_count = COMMAND_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
        assert_eq!(new_count, 10);
        assert_eq!(new_count % 10, 0); // Hit threshold
    }

    #[test]
    fn test_message_template() {
        let template = "Count: {count}, Threshold: {threshold}";
        let message = template
            .replace("{count}", "42")
            .replace("{threshold}", "10");
        assert_eq!(message, "Count: 42, Threshold: 10");
    }
}
