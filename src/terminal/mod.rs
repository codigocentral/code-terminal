pub mod buffer;
pub mod grid;
pub mod history;
pub mod images;
pub mod modes;
pub mod parser;
pub mod parser_thread;
pub mod pty;
#[cfg(unix)]
pub mod remote_pty;
pub mod scrollback;

// Re-exportações principais
pub use buffer::TerminalBuffer;
pub use grid::{Cell, Grid};
pub use history::History;
pub use modes::{AlternateBuffer, CursorKeysMode, MouseMode, TerminalModes};
pub use parser::{Attributes, Color, Parser};
pub use parser_thread::{ParserMessage, ParserThread, ParserThreadHandle, ThreadSafeGrid};
pub use pty::Pty;
pub use scrollback::{CompactLine, CompactScrollback, ScrollbackStats};
