//! Hot Reload System para Plugins WASM
//!
//! Permite recarregar plugins sem reiniciar o terminal.
//! Monitora mudanças nos arquivos .wasm e recarrega automaticamente.

use crate::plugin::error::{PluginError, PluginResult};
use crate::plugin::lifecycle::{LifecycleEvent, LifecycleManager};
use crate::plugin::manifest::PluginManifest;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_DEBOUNCE_MS: u64 = 300;
pub const MAX_RELOAD_ATTEMPTS: u32 = 3;
pub const RELOAD_BACKOFF_MS: u64 = 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotReloadEvent {
    Detected {
        plugin_name: String,
        path: PathBuf,
    },
    Reloading {
        plugin_name: String,
    },
    Reloaded {
        plugin_name: String,
        duration_ms: u64,
    },
    Failed {
        plugin_name: String,
        error: String,
    },
    Disabled {
        plugin_name: String,
    },
}

pub type HotReloadCallback = Arc<dyn Fn(HotReloadEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct HotReloadConfig {
    pub enabled: bool,
    pub debounce_ms: u64,
    pub max_attempts: u32,
    pub backoff_ms: u64,
    pub watch_patterns: Vec<String>,
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            debounce_ms: DEFAULT_DEBOUNCE_MS,
            max_attempts: MAX_RELOAD_ATTEMPTS,
            backoff_ms: RELOAD_BACKOFF_MS,
            watch_patterns: vec!["*.wasm".to_string(), "plugin.toml".to_string()],
        }
    }
}

impl HotReloadConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    pub fn with_debounce(mut self, ms: u64) -> Self {
        self.debounce_ms = ms;
        self
    }

    pub fn with_max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts;
        self
    }
}

#[derive(Debug)]
struct WatchedPlugin {
    wasm_path: PathBuf,
    manifest_path: PathBuf,
    #[allow(dead_code)]
    last_reload: Instant,
    #[allow(dead_code)]
    reload_attempts: u32,
    #[allow(dead_code)]
    pending_reload: bool,
}

/// Índice caminho → nome do plugin, compartilhado com a thread do watcher
type PathIndex = Arc<Mutex<HashMap<PathBuf, String>>>;

pub struct HotReloadWatcher {
    config: HotReloadConfig,
    watcher: Option<RecommendedWatcher>,
    watched_plugins: HashMap<String, WatchedPlugin>,
    path_index: PathIndex,
    event_tx: Option<Sender<WatchEvent>>,
    stop_tx: Option<Sender<()>>,
    callback: Option<HotReloadCallback>,
}

#[derive(Debug)]
#[allow(dead_code)]
enum WatchEvent {
    FileChanged(PathBuf),
    Stop,
}

impl HotReloadWatcher {
    pub fn new(config: HotReloadConfig) -> Self {
        Self {
            config,
            watcher: None,
            watched_plugins: HashMap::new(),
            path_index: Arc::new(Mutex::new(HashMap::new())),
            event_tx: None,
            stop_tx: None,
            callback: None,
        }
    }

    pub fn start(&mut self, callback: HotReloadCallback) -> PluginResult<()> {
        if !self.config.enabled {
            tracing::info!("Hot reload is disabled");
            return Ok(());
        }

        let (event_tx, event_rx) = channel();
        let (stop_tx, stop_rx) = channel();

        let callback_clone = callback.clone();
        let config_clone = self.config.clone();
        let event_tx_watcher = event_tx.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    if let Some(path) = event.paths.first() {
                        if is_watched_file(path, &config_clone.watch_patterns) {
                            let _ = event_tx_watcher.send(WatchEvent::FileChanged(path.clone()));
                        }
                    }
                }
            },
            Config::default().with_poll_interval(Duration::from_millis(200)),
        )
        .map_err(|e| PluginError::Other {
            message: format!("Failed to create file watcher: {}", e),
        })?;

        // Plugins registrados antes do start também passam a ser observados
        for plugin in self.watched_plugins.values() {
            for path in [&plugin.wasm_path, &plugin.manifest_path] {
                if path.exists() {
                    watcher
                        .watch(path, RecursiveMode::NonRecursive)
                        .map_err(|e| PluginError::Other {
                            message: format!("Failed to watch {}: {}", path.display(), e),
                        })?;
                }
            }
        }

        // A thread consulta o mesmo índice que watch_plugin/unwatch_plugin
        // atualizam (antes ela usava um mapa próprio, sempre vazio, e nenhuma
        // mudança era detectada).
        let path_index = self.path_index.clone();

        let _ = crate::utils::spawn_named("plugin-reload", move || {
            let mut pending_changes: HashMap<String, (Instant, PathBuf)> = HashMap::new();

            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }

                match event_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(WatchEvent::FileChanged(path)) => {
                        if let Some(plugin_name) = find_plugin_for_path(&path, &path_index) {
                            pending_changes.insert(plugin_name, (Instant::now(), path));
                        }
                    }
                    Ok(WatchEvent::Stop) => break,
                    Err(_) => {}
                }

                let now = Instant::now();
                let debounce = Duration::from_millis(config_clone.debounce_ms);

                let to_reload: Vec<String> = pending_changes
                    .iter()
                    .filter(|(_, (time, _))| now.duration_since(*time) >= debounce)
                    .map(|(name, _)| name.clone())
                    .collect();

                for plugin_name in to_reload {
                    if let Some((_, path)) = pending_changes.remove(&plugin_name) {
                        callback_clone(HotReloadEvent::Detected { plugin_name, path });
                    }
                }
            }
        });

        self.watcher = Some(watcher);
        self.event_tx = Some(event_tx);
        self.stop_tx = Some(stop_tx);
        self.callback = Some(callback);

        tracing::info!("Hot reload watcher started");
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }

        if let Some(ref mut watcher) = self.watcher {
            for plugin in self.watched_plugins.values() {
                let _ = watcher.unwatch(&plugin.wasm_path);
                let _ = watcher.unwatch(&plugin.manifest_path);
            }
        }

        self.watcher = None;
        self.event_tx = None;
        self.callback = None;

        tracing::info!("Hot reload watcher stopped");
    }

    pub fn watch_plugin(
        &mut self,
        name: impl Into<String>,
        wasm_path: impl Into<PathBuf>,
        manifest_path: impl Into<PathBuf>,
    ) -> PluginResult<()> {
        if !self.config.enabled {
            return Ok(());
        }

        let name = name.into();
        let wasm_path = wasm_path.into();
        let manifest_path = manifest_path.into();

        if let Some(ref mut watcher) = self.watcher {
            if wasm_path.exists() {
                watcher
                    .watch(&wasm_path, RecursiveMode::NonRecursive)
                    .map_err(|e| PluginError::Other {
                        message: format!("Failed to watch {}: {}", wasm_path.display(), e),
                    })?;
                tracing::debug!("Watching WASM file: {:?}", wasm_path);
            }

            if manifest_path.exists() {
                watcher
                    .watch(&manifest_path, RecursiveMode::NonRecursive)
                    .map_err(|e| PluginError::Other {
                        message: format!("Failed to watch {}: {}", manifest_path.display(), e),
                    })?;
                tracing::debug!("Watching manifest file: {:?}", manifest_path);
            }
        }

        if let Ok(mut index) = self.path_index.lock() {
            for path in [&wasm_path, &manifest_path] {
                index.insert(path.clone(), name.clone());
                if let Ok(canonical) = path.canonicalize() {
                    index.insert(canonical, name.clone());
                }
            }
        }

        self.watched_plugins.insert(
            name.clone(),
            WatchedPlugin {
                wasm_path,
                manifest_path,
                last_reload: Instant::now(),
                reload_attempts: 0,
                pending_reload: false,
            },
        );

        tracing::info!("Now watching plugin '{}' for changes", name);
        Ok(())
    }

    pub fn unwatch_plugin(&mut self, name: &str) -> PluginResult<()> {
        if let Some(plugin) = self.watched_plugins.remove(name) {
            if let Ok(mut index) = self.path_index.lock() {
                index.retain(|_, plugin_name| plugin_name != name);
            }
            if let Some(ref mut watcher) = self.watcher {
                let _ = watcher.unwatch(&plugin.wasm_path);
                let _ = watcher.unwatch(&plugin.manifest_path);
            }
            tracing::info!("Stopped watching plugin '{}'", name);
        }
        Ok(())
    }

    pub fn is_watching(&self, name: &str) -> bool {
        self.watched_plugins.contains_key(name)
    }

    pub fn watched_plugins(&self) -> Vec<&str> {
        self.watched_plugins.keys().map(|s| s.as_str()).collect()
    }

    pub fn config(&self) -> &HotReloadConfig {
        &self.config
    }
}

impl Default for HotReloadWatcher {
    fn default() -> Self {
        Self::new(HotReloadConfig::default())
    }
}

impl Drop for HotReloadWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct HotReloadManager {
    watcher: HotReloadWatcher,
    reload_handlers: HashMap<String, Box<dyn ReloadHandler>>,
    lifecycle: LifecycleManager,
    stats: Arc<Mutex<HotReloadStats>>,
}

pub trait ReloadHandler: Send + Sync {
    fn reload(&mut self, plugin_name: &str) -> PluginResult<()>;
}

#[derive(Debug, Clone, Default)]
pub struct HotReloadStats {
    pub total_reloads: u64,
    pub successful_reloads: u64,
    pub failed_reloads: u64,
    pub total_reload_time_ms: u64,
}

impl HotReloadManager {
    pub fn new(config: HotReloadConfig) -> Self {
        Self {
            watcher: HotReloadWatcher::new(config),
            reload_handlers: HashMap::new(),
            lifecycle: LifecycleManager::new(),
            stats: Arc::new(Mutex::new(HotReloadStats::default())),
        }
    }

    pub fn start(&mut self) -> PluginResult<()> {
        // Mesmas estatísticas expostas por stats() (antes era uma cópia que
        // nunca voltava para o manager)
        let stats = self.stats.clone();

        let callback = Arc::new(move |event: HotReloadEvent| {
            let Ok(mut stats) = stats.lock() else {
                return;
            };
            match &event {
                HotReloadEvent::Reloading { .. } => {
                    stats.total_reloads += 1;
                }
                HotReloadEvent::Reloaded { duration_ms, .. } => {
                    stats.successful_reloads += 1;
                    stats.total_reload_time_ms += duration_ms;
                }
                HotReloadEvent::Failed { .. } => {
                    stats.failed_reloads += 1;
                }
                _ => {}
            }
            tracing::info!("Hot reload event: {:?}", event);
        });

        self.watcher.start(callback)
    }

    pub fn stop(&mut self) {
        self.watcher.stop();
    }

    pub fn register_handler(
        &mut self,
        plugin_name: impl Into<String>,
        handler: Box<dyn ReloadHandler>,
    ) {
        self.reload_handlers.insert(plugin_name.into(), handler);
    }

    pub fn unregister_handler(&mut self, plugin_name: &str) {
        self.reload_handlers.remove(plugin_name);
    }

    pub fn watch_plugin(
        &mut self,
        name: impl Into<String>,
        wasm_path: impl Into<PathBuf>,
        manifest_path: impl Into<PathBuf>,
    ) -> PluginResult<()> {
        self.watcher.watch_plugin(name, wasm_path, manifest_path)
    }

    pub fn unwatch_plugin(&mut self, name: &str) -> PluginResult<()> {
        self.watcher.unwatch_plugin(name)
    }

    pub fn reload_plugin(&mut self, plugin_name: &str) -> PluginResult<()> {
        let start = Instant::now();

        tracing::info!("Reloading plugin: {}", plugin_name);

        self.lifecycle.notify(plugin_name, LifecycleEvent::Stopping);

        let result = match self.reload_handlers.get_mut(plugin_name) {
            Some(handler) => handler.reload(plugin_name),
            None => {
                tracing::warn!("No reload handler registered for plugin: {}", plugin_name);
                Ok(())
            }
        };

        let duration = start.elapsed();
        if let Ok(mut stats) = self.stats.lock() {
            stats.total_reloads += 1;
            if result.is_ok() {
                stats.successful_reloads += 1;
                stats.total_reload_time_ms += duration.as_millis() as u64;
            } else {
                stats.failed_reloads += 1;
            }
        }
        result?;

        self.lifecycle.notify(plugin_name, LifecycleEvent::Started);

        tracing::info!(
            "Plugin '{}' reloaded in {}ms",
            plugin_name,
            duration.as_millis()
        );

        Ok(())
    }

    pub fn stats(&self) -> HotReloadStats {
        self.stats
            .lock()
            .map(|stats| stats.clone())
            .unwrap_or_default()
    }

    pub fn reset_stats(&mut self) {
        if let Ok(mut stats) = self.stats.lock() {
            *stats = HotReloadStats::default();
        }
    }

    /// Registra um observador dos eventos de lifecycle emitidos no reload
    pub fn register_lifecycle_handler(
        &mut self,
        handler: Box<dyn crate::plugin::lifecycle::LifecycleHandler>,
    ) {
        self.lifecycle.register(handler);
    }

    pub fn is_enabled(&self) -> bool {
        self.watcher.config().enabled
    }
}

fn is_watched_file(path: &Path, patterns: &[String]) -> bool {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    patterns.iter().any(|pattern| {
        if pattern.starts_with('*') {
            let ext = pattern.trim_start_matches('*');
            file_name.ends_with(ext)
        } else {
            file_name == pattern
        }
    })
}

fn find_plugin_for_path(path: &Path, path_index: &PathIndex) -> Option<String> {
    let index = path_index.lock().ok()?;
    if let Some(name) = index.get(path) {
        return Some(name.clone());
    }
    let canonical = path.canonicalize().ok()?;
    index.get(&canonical).cloned()
}

#[derive(Debug, Clone)]
pub struct ReloadContext {
    pub plugin_name: String,
    pub old_version: String,
    pub new_version: String,
    pub preserve_state: bool,
    pub state_data: Option<Vec<u8>>,
}

impl ReloadContext {
    pub fn new(plugin_name: impl Into<String>) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            old_version: String::new(),
            new_version: String::new(),
            preserve_state: false,
            state_data: None,
        }
    }

    pub fn with_versions(mut self, old: impl Into<String>, new: impl Into<String>) -> Self {
        self.old_version = old.into();
        self.new_version = new.into();
        self
    }

    pub fn preserve_state(mut self, data: Option<Vec<u8>>) -> Self {
        self.preserve_state = data.is_some();
        self.state_data = data;
        self
    }
}

pub fn validate_plugin_for_reload(
    old_manifest: &PluginManifest,
    new_manifest: &PluginManifest,
) -> PluginResult<()> {
    if old_manifest.plugin.name != new_manifest.plugin.name {
        return Err(PluginError::Other {
            message: format!(
                "Plugin name mismatch: {} != {}",
                old_manifest.plugin.name, new_manifest.plugin.name
            ),
        });
    }

    Ok(())
}

pub fn can_hot_reload(old_manifest: &PluginManifest, new_manifest: &PluginManifest) -> bool {
    old_manifest.plugin.name == new_manifest.plugin.name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hot_reload_config_default() {
        let config = HotReloadConfig::default();
        assert!(config.enabled);
        assert_eq!(config.debounce_ms, DEFAULT_DEBOUNCE_MS);
        assert_eq!(config.max_attempts, MAX_RELOAD_ATTEMPTS);
    }

    #[test]
    fn test_hot_reload_config_disabled() {
        let config = HotReloadConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_hot_reload_config_builder() {
        let config = HotReloadConfig::new()
            .with_debounce(500)
            .with_max_attempts(5);

        assert_eq!(config.debounce_ms, 500);
        assert_eq!(config.max_attempts, 5);
    }

    #[test]
    fn test_is_watched_file() {
        let patterns = vec!["*.wasm".to_string(), "plugin.toml".to_string()];

        assert!(is_watched_file(Path::new("plugin.wasm"), &patterns));
        assert!(is_watched_file(Path::new("my-plugin.wasm"), &patterns));
        assert!(is_watched_file(Path::new("plugin.toml"), &patterns));
        assert!(!is_watched_file(Path::new("README.md"), &patterns));
        assert!(!is_watched_file(Path::new("lib.rs"), &patterns));
    }

    #[test]
    fn test_hot_reload_event_equality() {
        let event1 = HotReloadEvent::Reloading {
            plugin_name: "test".to_string(),
        };
        let event2 = HotReloadEvent::Reloading {
            plugin_name: "test".to_string(),
        };
        let event3 = HotReloadEvent::Reloading {
            plugin_name: "other".to_string(),
        };

        assert_eq!(event1, event2);
        assert_ne!(event1, event3);
    }

    #[test]
    fn test_hot_reload_watcher_creation() {
        let config = HotReloadConfig::disabled();
        let watcher = HotReloadWatcher::new(config);
        assert!(!watcher.config.enabled);
    }

    #[test]
    fn test_reload_context() {
        let ctx = ReloadContext::new("test-plugin")
            .with_versions("1.0.0", "1.1.0")
            .preserve_state(Some(vec![1, 2, 3]));

        assert_eq!(ctx.plugin_name, "test-plugin");
        assert_eq!(ctx.old_version, "1.0.0");
        assert_eq!(ctx.new_version, "1.1.0");
        assert!(ctx.preserve_state);
        assert_eq!(ctx.state_data, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_hot_reload_stats_default() {
        let stats = HotReloadStats::default();
        assert_eq!(stats.total_reloads, 0);
        assert_eq!(stats.successful_reloads, 0);
        assert_eq!(stats.failed_reloads, 0);
    }

    #[test]
    fn test_hot_reload_manager_creation() {
        let manager = HotReloadManager::new(HotReloadConfig::disabled());
        assert!(!manager.is_enabled());
    }

    #[test]
    fn test_watched_plugins_list() {
        let watcher = HotReloadWatcher::new(HotReloadConfig::disabled());
        let plugins = watcher.watched_plugins();
        assert!(plugins.is_empty());
    }

    use crate::plugin::lifecycle::LifecycleHandler;

    type Events = Arc<Mutex<Vec<HotReloadEvent>>>;

    fn recorder() -> (Events, HotReloadCallback) {
        let events: Events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let callback: HotReloadCallback = Arc::new(move |event| sink.lock().unwrap().push(event));
        (events, callback)
    }

    /// Reescreve o arquivo até o watcher emitir Detected para o plugin
    fn wait_for_detection(events: &Events, plugin: &str, file: &Path) -> Option<HotReloadEvent> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut n = 0u32;
        while Instant::now() < deadline {
            n += 1;
            std::fs::write(file, format!("versao {n}")).unwrap();
            std::thread::sleep(Duration::from_millis(150));
            let found = events.lock().unwrap().iter().find(|e| {
                matches!(e, HotReloadEvent::Detected { plugin_name, .. } if plugin_name == plugin)
            }).cloned();
            if found.is_some() {
                return found;
            }
        }
        None
    }

    /// Regressão: a thread do watcher usava um mapa próprio (sempre vazio) e
    /// nenhuma alteração de arquivo chegava ao callback.
    #[test]
    fn test_watcher_detects_changes_of_watched_plugins() {
        let dir = tempfile::tempdir().unwrap();
        let early_wasm = dir.path().join("early.wasm");
        let late_wasm = dir.path().join("late.wasm");
        let manifest = dir.path().join("plugin.toml");
        std::fs::write(&early_wasm, b"v0").unwrap();
        std::fs::write(&late_wasm, b"v0").unwrap();
        std::fs::write(&manifest, b"[plugin]").unwrap();

        let mut watcher = HotReloadWatcher::new(HotReloadConfig::new().with_debounce(20));
        // Registrado antes do start: o start precisa passar a observá-lo
        watcher
            .watch_plugin("early", &early_wasm, &manifest)
            .unwrap();
        let (events, callback) = recorder();
        watcher.start(callback).unwrap();
        watcher
            .watch_plugin("late", &late_wasm, dir.path().join("late.toml"))
            .unwrap();

        let mut watched = watcher.watched_plugins();
        watched.sort();
        assert_eq!(watched, vec!["early", "late"]);

        match wait_for_detection(&events, "late", &late_wasm) {
            Some(HotReloadEvent::Detected { path, .. }) => {
                assert_eq!(path.file_name(), late_wasm.file_name());
            }
            other => panic!("mudança em late.wasm não detectada: {other:?}"),
        }
        assert!(
            wait_for_detection(&events, "early", &early_wasm).is_some(),
            "plugin registrado antes do start não foi observado"
        );

        // Depois de deixar de observar, o índice não aponta mais para o plugin
        watcher.unwatch_plugin("late").unwrap();
        assert!(!watcher.is_watching("late"));
        assert!(find_plugin_for_path(&late_wasm, &watcher.path_index).is_none());
        assert_eq!(
            find_plugin_for_path(&early_wasm, &watcher.path_index).as_deref(),
            Some("early")
        );

        watcher.stop();
        assert!(watcher.watcher.is_none());
        // stop é idempotente e o Drop chama stop de novo
        watcher.stop();
    }

    #[test]
    fn test_disabled_watcher_is_a_no_op() {
        let mut watcher = HotReloadWatcher::new(HotReloadConfig::disabled());
        let (_events, callback) = recorder();
        watcher.start(callback).unwrap();
        assert!(watcher.watcher.is_none());
        watcher
            .watch_plugin("p", "/x/p.wasm", "/x/plugin.toml")
            .unwrap();
        assert!(!watcher.is_watching("p"));
        // unwatch de plugin desconhecido não falha
        watcher.unwatch_plugin("p").unwrap();
    }

    #[test]
    fn test_watch_without_start_only_records() {
        let mut watcher = HotReloadWatcher::default();
        assert!(watcher.config().enabled);
        watcher
            .watch_plugin("p", "/nao/existe/p.wasm", "/nao/existe/plugin.toml")
            .unwrap();
        assert!(watcher.is_watching("p"));
        assert_eq!(
            find_plugin_for_path(Path::new("/nao/existe/p.wasm"), &watcher.path_index).as_deref(),
            Some("p")
        );
        assert!(find_plugin_for_path(Path::new("/outro.wasm"), &watcher.path_index).is_none());
    }

    struct CountingHandler {
        calls: Arc<Mutex<Vec<String>>>,
        fail: bool,
    }

    impl ReloadHandler for CountingHandler {
        fn reload(&mut self, plugin_name: &str) -> PluginResult<()> {
            self.calls.lock().unwrap().push(plugin_name.to_string());
            if self.fail {
                Err(PluginError::Other {
                    message: "falha no reload".into(),
                })
            } else {
                Ok(())
            }
        }
    }

    struct EventLog(Arc<Mutex<Vec<(String, LifecycleEvent)>>>);

    impl LifecycleHandler for EventLog {
        fn on_lifecycle_event(&mut self, plugin_name: &str, event: LifecycleEvent) {
            self.0
                .lock()
                .unwrap()
                .push((plugin_name.to_string(), event));
        }
    }

    #[test]
    fn test_manager_reload_updates_stats_and_lifecycle() {
        let mut manager = HotReloadManager::new(HotReloadConfig::disabled());
        manager.start().unwrap();

        let calls = Arc::new(Mutex::new(Vec::new()));
        let lifecycle = Arc::new(Mutex::new(Vec::new()));
        manager.register_lifecycle_handler(Box::new(EventLog(lifecycle.clone())));
        manager.register_handler(
            "ok",
            Box::new(CountingHandler {
                calls: calls.clone(),
                fail: false,
            }),
        );
        manager.register_handler(
            "ruim",
            Box::new(CountingHandler {
                calls: calls.clone(),
                fail: true,
            }),
        );

        manager.reload_plugin("ok").unwrap();
        let err = manager.reload_plugin("ruim").unwrap_err();
        assert!(err.to_string().contains("falha no reload"), "{err}");
        // Sem handler: só registra aviso
        manager.reload_plugin("sem-handler").unwrap();

        assert_eq!(*calls.lock().unwrap(), vec!["ok", "ruim"]);
        let stats = manager.stats();
        assert_eq!(stats.total_reloads, 3);
        assert_eq!(stats.successful_reloads, 2);
        assert_eq!(stats.failed_reloads, 1);

        let events = lifecycle.lock().unwrap().clone();
        assert_eq!(
            events,
            vec![
                ("ok".to_string(), LifecycleEvent::Stopping),
                ("ok".to_string(), LifecycleEvent::Started),
                ("ruim".to_string(), LifecycleEvent::Stopping),
                ("sem-handler".to_string(), LifecycleEvent::Stopping),
                ("sem-handler".to_string(), LifecycleEvent::Started),
            ]
        );

        manager.reset_stats();
        assert_eq!(manager.stats().total_reloads, 0);

        manager.unregister_handler("ok");
        manager.reload_plugin("ok").unwrap();
        assert_eq!(calls.lock().unwrap().len(), 2);
        manager.stop();
    }

    #[test]
    fn test_manager_watch_delegates_to_watcher() {
        let mut manager = HotReloadManager::new(HotReloadConfig::default());
        assert!(manager.is_enabled());
        manager.start().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let wasm = dir.path().join("m.wasm");
        std::fs::write(&wasm, b"x").unwrap();
        manager
            .watch_plugin("m", &wasm, dir.path().join("plugin.toml"))
            .unwrap();
        assert!(manager.watcher.is_watching("m"));
        manager.unwatch_plugin("m").unwrap();
        assert!(!manager.watcher.is_watching("m"));
        manager.stop();
    }

    #[test]
    fn test_reload_validation_requires_same_name() {
        let old = PluginManifest::new("meu-plugin", "1.0.0", "p.wasm");
        let same = PluginManifest::new("meu-plugin", "1.1.0", "p.wasm");
        let other = PluginManifest::new("outro", "1.0.0", "p.wasm");

        assert!(validate_plugin_for_reload(&old, &same).is_ok());
        assert!(can_hot_reload(&old, &same));

        let err = validate_plugin_for_reload(&old, &other).unwrap_err();
        assert!(err.to_string().contains("Plugin name mismatch"), "{err}");
        assert!(!can_hot_reload(&old, &other));
    }

    #[test]
    fn test_is_watched_file_edge_cases() {
        let patterns = vec!["*.wasm".to_string()];
        assert!(!is_watched_file(Path::new("/"), &patterns));
        assert!(!is_watched_file(Path::new("plugin.wasm.bak"), &patterns));
        assert!(!is_watched_file(Path::new("x.wasm"), &[]));
        assert!(!ReloadContext::new("p").preserve_state(None).preserve_state);
    }
}
