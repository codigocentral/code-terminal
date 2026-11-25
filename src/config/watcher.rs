use crate::config::parser;
use anyhow::{Context, Result};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub type ConfigReloadCallback = Arc<dyn Fn(ConfigReloadEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum ConfigReloadEvent {
    Changed(PathBuf),
    Error(String),
}

pub struct ConfigWatcher {
    watcher: Option<RecommendedWatcher>,
    stop_tx: Option<Sender<()>>,
    /// Arquivo observado (para o `unwatch` no stop)
    watched_path: Option<PathBuf>,
}

impl ConfigWatcher {
    pub fn new() -> Self {
        Self {
            watcher: None,
            stop_tx: None,
            watched_path: None,
        }
    }

    pub fn start_watching<F>(callback: F) -> Result<Self>
    where
        F: Fn(ConfigReloadEvent) + Send + Sync + 'static,
    {
        Self::start_watching_path(parser::get_config_path()?, callback)
    }

    /// Observa um arquivo de configuração específico
    pub fn start_watching_path<F>(config_path: PathBuf, callback: F) -> Result<Self>
    where
        F: Fn(ConfigReloadEvent) + Send + Sync + 'static,
    {
        let callback = Arc::new(callback);
        let (stop_tx, stop_rx) = channel();
        let (event_tx, event_rx) = channel();

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Err(e) = event_tx.send(res) {
                    tracing::error!("Failed to send config watch event: {}", e);
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(1)),
        )
        .context("Failed to create config watcher")?;

        let config_path_clone = config_path.clone();
        let _ = crate::utils::spawn_named("config-watch", move || {
            run_debounce_loop(&config_path_clone, &stop_rx, &event_rx, callback.as_ref());
        });

        let mut watcher_instance = Self {
            watcher: Some(watcher),
            stop_tx: Some(stop_tx),
            watched_path: Some(config_path.clone()),
        };

        if let Some(ref mut w) = watcher_instance.watcher {
            w.watch(&config_path, RecursiveMode::NonRecursive)
                .context("Failed to watch config file")?;
            tracing::info!("Started watching config at {:?}", config_path);
        }

        Ok(watcher_instance)
    }

    pub fn stop(&mut self) {
        if let (Some(watcher), Some(path)) = (self.watcher.as_mut(), self.watched_path.take()) {
            let _ = watcher.unwatch(&path);
        }

        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }

        self.watcher = None;
        tracing::info!("Config watcher stopped");
    }
}

/// Intervalo sem eventos antes de avisar a mudança
const DEBOUNCE_DURATION: Duration = Duration::from_millis(500);

/// Estado do debounce: última mudança pendente e quando ela chegou
struct Debouncer {
    last_event_time: Instant,
    pending_change: Option<PathBuf>,
}

impl Debouncer {
    fn new() -> Self {
        Self {
            last_event_time: Instant::now(),
            pending_change: None,
        }
    }

    /// Registra o evento se ele for do arquivo observado
    fn on_event(&mut self, event: &Event, config_path: &Path) {
        if let Some(path) = event.paths.first().filter(|p| *p == config_path) {
            self.pending_change = Some(path.clone());
            self.last_event_time = Instant::now();
        }
    }

    /// Quanto esperar pelo próximo evento: `None` (para sempre) sem mudança
    /// pendente; com mudança, o que falta do debounce.
    fn wait(&self) -> Option<Duration> {
        self.pending_change.as_ref()?;
        Some(DEBOUNCE_DURATION.saturating_sub(self.last_event_time.elapsed()))
    }

    /// Dispara a mudança pendente quando o debounce venceu
    fn on_timeout(&mut self, callback: &dyn Fn(ConfigReloadEvent)) {
        if self.last_event_time.elapsed() < DEBOUNCE_DURATION {
            return;
        }
        if let Some(path) = self.pending_change.take() {
            callback(ConfigReloadEvent::Changed(path));
        }
    }
}

/// Loop da thread do watcher: junta eventos em rajada e avisa uma vez só
fn run_debounce_loop(
    config_path: &Path,
    stop_rx: &Receiver<()>,
    event_rx: &Receiver<Result<Event, notify::Error>>,
    callback: &dyn Fn(ConfigReloadEvent),
) {
    let mut debouncer = Debouncer::new();

    loop {
        if stop_rx.try_recv().is_ok() {
            tracing::info!("Config watcher stopped");
            break;
        }

        // Sem mudança pendente, dorme até chegar um evento (o `stop` derruba
        // o watcher, que fecha o canal); com mudança, só até o debounce
        // vencer. Antes acordava a cada 100 ms para nada (#30).
        let event = match debouncer.wait() {
            None => event_rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(wait) => event_rx.recv_timeout(wait),
        };
        match event {
            Ok(Ok(event)) => debouncer.on_event(&event, config_path),
            Ok(Err(e)) => callback(ConfigReloadEvent::Error(e.to_string())),
            Err(RecvTimeoutError::Timeout) => debouncer.on_timeout(callback),
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

impl Default for ConfigWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_watcher_creation() {
        let watcher = ConfigWatcher::new();
        assert!(watcher.watcher.is_none());
    }

    #[test]
    fn test_config_reload_event() {
        let event = ConfigReloadEvent::Changed(PathBuf::from("/test/path"));
        match event {
            ConfigReloadEvent::Changed(path) => {
                assert_eq!(path, PathBuf::from("/test/path"));
            }
            _ => panic!("Wrong variant"),
        }

        let event = ConfigReloadEvent::Error("test error".to_string());
        match event {
            ConfigReloadEvent::Error(msg) => {
                assert_eq!(msg, "test error");
            }
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_watcher_drop() {
        let mut watcher = ConfigWatcher::new();
        watcher.stop();
        assert!(watcher.watcher.is_none());
    }

    use std::sync::Mutex;

    /// Espera (com limite) até o predicado valer para os eventos recebidos.
    fn wait_for(
        events: &Mutex<Vec<ConfigReloadEvent>>,
        check: impl Fn(&[ConfigReloadEvent]) -> bool,
    ) {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !check(&events.lock().unwrap()) {
            assert!(
                std::time::Instant::now() < deadline,
                "evento não chegou: {:?}",
                events.lock().unwrap()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Sem mudança pendente a thread dorme sem prazo; com mudança, só até o
    /// debounce vencer (#30: antes acordava a cada 100 ms).
    #[test]
    fn test_debouncer_sleeps_until_an_event_or_the_debounce() {
        let mut debouncer = Debouncer::new();
        assert_eq!(debouncer.wait(), None);
        let path = PathBuf::from("/tmp/config.toml");
        let event = Event::new(notify::EventKind::Any).add_path(path.clone());
        debouncer.on_event(&event, &path);
        let wait = debouncer.wait().unwrap();
        assert!(wait <= DEBOUNCE_DURATION && wait > Duration::from_millis(400));
        debouncer.last_event_time = Instant::now() - DEBOUNCE_DURATION;
        assert_eq!(debouncer.wait(), Some(Duration::ZERO));
        let fired = std::cell::Cell::new(false);
        debouncer.on_timeout(&|_| fired.set(true));
        assert!(fired.get());
        assert_eq!(debouncer.wait(), None);
    }

    /// Parar o watcher acorda a thread bloqueada (o canal fecha).
    #[test]
    fn test_stop_wakes_the_blocked_loop() {
        let (_stop_tx, stop_rx) = channel();
        let (event_tx, event_rx) = channel::<Result<Event, notify::Error>>();
        let path = PathBuf::from("/tmp/config.toml");
        let thread = std::thread::spawn(move || {
            run_debounce_loop(&path, &stop_rx, &event_rx, &|_| {});
        });
        std::thread::sleep(Duration::from_millis(50));
        assert!(!thread.is_finished());
        drop(event_tx);
        thread.join().unwrap();
    }

    #[test]
    fn test_watching_a_file_reports_debounced_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "a = 1\n").unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let mut watcher = ConfigWatcher::start_watching_path(path.clone(), move |event| {
            sink.lock().unwrap().push(event);
        })
        .unwrap();
        assert!(watcher.watcher.is_some());

        // Várias escritas seguidas viram um único "Changed" depois do debounce
        for i in 0..3 {
            std::fs::write(&path, format!("a = {}\n", i)).unwrap();
        }
        wait_for(&events, |events| {
            events
                .iter()
                .any(|e| matches!(e, ConfigReloadEvent::Changed(p) if p == &path))
        });

        watcher.stop();
        assert!(watcher.watcher.is_none());
        assert!(watcher.watched_path.is_none());
        // Parar de novo é seguro
        watcher.stop();
    }

    #[test]
    fn test_watching_a_missing_file_fails() {
        let dir = tempfile::tempdir().unwrap();
        let result = ConfigWatcher::start_watching_path(dir.path().join("nao-existe.toml"), |_| {});
        let error = result
            .err()
            .expect("watch de arquivo inexistente deveria falhar");
        assert!(
            error.to_string().contains("Failed to watch config file"),
            "{error}"
        );
        assert!(ConfigWatcher::default().watched_path.is_none());
    }
}
