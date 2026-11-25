#![allow(
    clippy::await_holding_lock,
    clippy::field_reassign_with_default,
    clippy::large_enum_variant,
    clippy::result_large_err,
    clippy::should_implement_trait,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::wrong_self_convention
)]

pub mod ai;
pub mod app;
pub mod browser;
pub mod cli;
pub mod config;
pub mod image;
pub mod multiplexer;
pub mod notification;
pub mod pane;
pub mod platform;
pub mod plugin;
pub mod remote;
pub mod renderer;
pub mod session;
pub mod ssh;
pub mod terminal;
pub mod types;
pub mod ui;
pub mod utils;

/// Executáveis falsos para os testes (sem a corrida do ETXTBSY).
#[cfg(all(test, unix))]
pub(crate) mod test_exec;
