//! Plugins WASM dentro do app.
//!
//! Cada subpasta de `<config>/plugins/` com um `plugin.toml` é um plugin; o
//! `.wasm` apontado por `entry` é compilado, instanciado com as host
//! functions e iniciado. O app chama [`PluginHost::tick`] a cada
//! [`UPDATE_INTERVAL`], repassa a digitação por [`PluginHost::on_input`] e
//! aplica os pedidos que os plugins deixam na [`bridge`].

use crate::plugin::bridge;
use crate::plugin::loader::WasmPluginLoader;
use crate::plugin::manager::{PluginManager, PluginManagerConfig};
use crate::plugin::manifest::PluginManifest;
use crate::plugin::wasm_runtime::WasmRuntime;
use crate::plugin::PluginResult;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Intervalo entre chamadas de `update` nos plugins.
pub const UPDATE_INTERVAL: Duration = Duration::from_millis(250);

/// Limite de caracteres do texto de status de um plugin.
const MAX_STATUS_CHARS: usize = 60;

/// Plugin que não carregou, com o motivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLoadError {
    pub dir: PathBuf,
    pub reason: String,
}

/// Plugins carregados e o que eles mostram (status por plugin).
pub struct PluginHost {
    manager: PluginManager,
    runtime: Arc<WasmRuntime>,
    statuses: BTreeMap<String, String>,
    failed: Vec<PluginLoadError>,
    last_update: Instant,
}

impl PluginHost {
    /// Carrega todos os plugins de `dir`. Plugins com erro ficam em
    /// [`PluginHost::failed`] e não impedem os outros.
    pub fn load_dir(dir: &Path, data_root: &Path) -> PluginResult<Self> {
        let config = PluginManagerConfig {
            plugins_dir: dir.to_path_buf(),
            data_dir: data_root.join("data"),
            config_dir: data_root.join("config"),
            ..PluginManagerConfig::default()
        };
        let mut host = Self {
            manager: PluginManager::new(config),
            runtime: Arc::new(WasmRuntime::new()?),
            statuses: BTreeMap::new(),
            failed: Vec::new(),
            last_update: Instant::now(),
        };
        for plugin_dir in plugin_dirs(dir) {
            match host.load_one(&plugin_dir) {
                Ok(name) => tracing::info!(plugin = %name, "Plugin loaded"),
                Err(error) => {
                    tracing::warn!(dir = %plugin_dir.display(), error = %error, "Plugin failed to load");
                    host.failed.push(PluginLoadError {
                        dir: plugin_dir,
                        reason: error.to_string(),
                    });
                }
            }
        }
        Ok(host)
    }

    fn load_one(&mut self, dir: &Path) -> PluginResult<String> {
        let manifest = PluginManifest::from_file(dir.join("plugin.toml"))?;
        manifest.validate()?;
        let mut loader = WasmPluginLoader::new(Arc::clone(&self.runtime), dir);
        let loaded = loader.load_from_manifest(&manifest, dir)?;
        let instance = loader.create_instance(&loaded, manifest.clone())?;
        let name = manifest.plugin.name.clone();
        self.manager.load(manifest, Box::new(instance))?;
        Ok(name)
    }

    /// Nomes dos plugins carregados.
    pub fn names(&self) -> Vec<String> {
        let mut names = self.manager.list_names();
        names.sort();
        names
    }

    /// Plugins que não carregaram.
    pub fn failed(&self) -> &[PluginLoadError] {
        &self.failed
    }

    pub fn is_empty(&self) -> bool {
        self.manager.count() == 0
    }

    /// Chama `update` se já passou [`UPDATE_INTERVAL`]; `true` se chamou.
    pub fn tick(&mut self) -> bool {
        let elapsed = self.last_update.elapsed();
        if elapsed < UPDATE_INTERVAL {
            return false;
        }
        self.last_update = Instant::now();
        self.manager
            .update(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX));
        true
    }

    /// Texto digitado: fica disponível para `read_input` e vai para o
    /// `on_input` de cada plugin. `true` se algum plugin consumiu.
    pub fn on_input(&self, text: &str) -> bool {
        bridge::push_input(text);
        self.manager.on_input(text)
    }

    /// Guarda o status de um plugin (vazio apaga).
    pub fn set_status(&mut self, plugin: &str, text: &str) {
        let text: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_STATUS_CHARS)
            .collect();
        if text.trim().is_empty() {
            self.statuses.remove(plugin);
        } else {
            self.statuses.insert(plugin.to_string(), text);
        }
    }

    /// Status atuais, por nome do plugin.
    pub fn statuses(&self) -> Vec<(String, String)> {
        self.statuses
            .iter()
            .map(|(name, text)| (name.clone(), text.clone()))
            .collect()
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        let _ = self
            .manager
            .unload_all(crate::plugin::manager::UnloadOptions::graceful());
    }
}

/// Subpastas com `plugin.toml`, em ordem alfabética (carga previsível).
fn plugin_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.join("plugin.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// Pasta padrão dos plugins: `<config>/plugins`.
pub fn default_plugins_dir() -> Option<PathBuf> {
    crate::config::parser::get_config_dir()
        .ok()
        .map(|dir| dir.join("plugins"))
}

/// Sequência que escreve `text` em (coluna, linha) e devolve o cursor ao
/// lugar (DECSC/DECRC), sem atrapalhar o programa do painel.
pub fn write_at_sequence(col: u16, row: u16, text: &str) -> String {
    format!(
        "\x1b7\x1b[{};{}H{text}\x1b8",
        u32::from(row) + 1,
        u32::from(col) + 1
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_at_sequence_saves_and_restores_cursor() {
        assert_eq!(write_at_sequence(4, 0, "ok"), "\x1b7\x1b[1;5Hok\x1b8");
    }

    #[test]
    fn test_plugin_dirs_only_with_manifest() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("b")).unwrap();
        std::fs::write(dir.path().join("b/plugin.toml"), "").unwrap();
        std::fs::create_dir(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/plugin.toml"), "").unwrap();
        std::fs::create_dir(dir.path().join("sem-manifesto")).unwrap();
        let names: Vec<_> = plugin_dirs(dir.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["a", "b"]);
        assert!(plugin_dirs(&dir.path().join("nada")).is_empty());
    }

    #[test]
    fn test_bad_plugin_is_reported_and_others_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("quebrado");
        std::fs::create_dir(&bad).unwrap();
        std::fs::write(
            bad.join("plugin.toml"),
            "entry = \"x.wasm\"\n[plugin]\nname = \"quebrado\"\nversion = \"1.0.0\"\n",
        )
        .unwrap();
        let host = PluginHost::load_dir(dir.path(), &dir.path().join("estado")).unwrap();
        assert!(host.is_empty());
        assert_eq!(host.failed().len(), 1);
        assert_eq!(host.failed()[0].dir, bad);
    }

    #[test]
    fn test_status_is_sanitized_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = PluginHost::load_dir(dir.path(), dir.path()).unwrap();
        host.set_status("p", "linha\x1b[31m vermelha");
        assert_eq!(
            host.statuses(),
            vec![("p".into(), "linha[31m vermelha".into())]
        );
        host.set_status("p", &"x".repeat(200));
        assert_eq!(host.statuses()[0].1.len(), MAX_STATUS_CHARS);
        host.set_status("p", "  ");
        assert!(host.statuses().is_empty());
    }
}
