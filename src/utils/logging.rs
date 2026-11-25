pub fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    #[cfg(all(windows, not(debug_assertions)))]
    {
        use std::sync::OnceLock;
        use tracing_appender::non_blocking::WorkerGuard;

        static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

        let log_dir = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Code Terminal")
            .join("logs");

        if let Err(error) = std::fs::create_dir_all(&log_dir) {
            eprintln!(
                "Failed to create log directory {}: {error}",
                log_dir.display()
            );
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::sink)
                .try_init();
            return;
        }

        let file_appender = tracing_appender::rolling::daily(log_dir, "code-terminal.log");
        let (writer, guard) = tracing_appender::non_blocking(file_appender);
        let _ = LOG_GUARD.set(guard);
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(writer)
            .with_ansi(false)
            .try_init();
    }

    #[cfg(not(all(windows, not(debug_assertions))))]
    {
        let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
    }
}
