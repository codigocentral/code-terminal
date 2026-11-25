//! Common test utilities for integration tests

/// Setup function for tests that need tokio runtime
#[allow(dead_code)]
pub fn setup_tokio_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime")
}

/// Helper to create a temporary directory for tests
#[allow(dead_code)]
pub fn temp_dir() -> std::path::PathBuf {
    tempfile::tempdir()
        .expect("Failed to create temp dir")
        .keep()
}
