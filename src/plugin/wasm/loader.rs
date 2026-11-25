//! Loader de Plugins WASM
//!
//! Carrega e compila módulos WASM para o runtime.

use super::bindings::HostState;
use super::instance::WasmPluginInstance;
use crate::plugin::error::{PluginError, PluginResult};
use crate::plugin::manifest::Permission;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::{Engine, Module};

/// Configuração do loader
#[derive(Debug, Clone)]
pub struct WasmLoaderConfig {
    /// Diretório de plugins
    pub plugins_dir: PathBuf,
    /// Habilitar cache de módulos
    pub enable_cache: bool,
    /// Permissões padrão para plugins
    pub default_permissions: Vec<Permission>,
}

impl Default for WasmLoaderConfig {
    fn default() -> Self {
        Self {
            plugins_dir: PathBuf::from("plugins"),
            enable_cache: true,
            default_permissions: vec![
                Permission::ReadConfig,
                Permission::Logging,
            ],
        }
    }
}

impl WasmLoaderConfig {
    /// Cria configuração com diretório específico
    pub fn with_plugins_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            plugins_dir: dir.into(),
            ..Default::default()
        }
    }

    /// Define se cache está habilitado
    pub fn with_cache(mut self, enabled: bool) -> Self {
        self.enable_cache = enabled;
        self
    }

    /// Define permissões padrão
    pub fn with_permissions(mut self, permissions: Vec<Permission>) -> Self {
        self.default_permissions = permissions;
        self
    }
}

/// Cache de módulos WASM compilados
#[derive(Debug)]
pub struct ModuleCache {
    /// Módulos compilados (path -> Module)
    modules: RwLock<HashMap<PathBuf, Arc<Module>>>,
    /// Hits no cache
    hits: std::sync::atomic::AtomicU64,
    /// Misses no cache
    misses: std::sync::atomic::AtomicU64,
}

impl ModuleCache {
    /// Cria novo cache vazio
    pub fn new() -> Self {
        Self {
            modules: RwLock::new(HashMap::new()),
            hits: std::sync::atomic::AtomicU64::new(0),
            misses: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Obtém módulo do cache
    pub async fn get(&self, path: &Path) -> Option<Arc<Module>> {
        let modules = self.modules.read().await;
        if let Some(module) = modules.get(path) {
            self.hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(Arc::clone(module))
        } else {
            None
        }
    }

    /// Insere módulo no cache
    pub async fn insert(&self, path: PathBuf, module: Arc<Module>) {
        let mut modules = self.modules.write().await;
        modules.insert(path, module);
    }

    /// Remove módulo do cache
    pub async fn remove(&self, path: &Path) -> Option<Arc<Module>> {
        let mut modules = self.modules.write().await;
        modules.remove(path)
    }

    /// Limpa o cache
    pub async fn clear(&self) {
        let mut modules = self.modules.write().await;
        modules.clear();
    }

    /// Retorna estatísticas do cache
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: 0, // Calculado abaixo se necessário
            hits: self.hits.load(std::sync::atomic::Ordering::SeqCst),
            misses: self.misses.load(std::sync::atomic::Ordering::SeqCst),
        }
    }

    /// Registra um miss
    fn record_miss(&self) {
        self.misses.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Default for ModuleCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Estatísticas do cache
#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    /// Número de entradas
    pub entries: usize,
    /// Hits
    pub hits: u64,
    /// Misses
    pub misses: u64,
}

impl CacheStats {
    /// Taxa de hit (0.0 a 1.0)
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Loader de plugins WASM
#[derive(Debug)]
pub struct WasmPluginLoader {
    /// Engine WASM compartilhado
    engine: Arc<Engine>,
    /// Configuração
    config: WasmLoaderConfig,
    /// Cache de módulos
    cache: Arc<ModuleCache>,
}

impl WasmPluginLoader {
    /// Cria novo loader com engine
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            config: WasmLoaderConfig::default(),
            cache: Arc::new(ModuleCache::new()),
        }
    }

    /// Cria novo loader com configuração
    pub fn with_config(engine: Arc<Engine>, config: WasmLoaderConfig) -> Self {
        Self {
            engine,
            config,
            cache: Arc::new(ModuleCache::new()),
        }
    }

    /// Carrega um plugin de um arquivo
    pub async fn load_from_file(
        &self,
        path: impl AsRef<Path>,
        permissions: Option<Vec<Permission>>,
    ) -> PluginResult<WasmPluginInstance> {
        let path = path.as_ref();
        
        // Valida extensão
        if let Some(ext) = path.extension() {
            if ext != "wasm" {
                return Err(PluginError::invalid_manifest(format!(
                    "Invalid file extension: {:?}",
                    ext
                )));
            }
        }

        // Verifica se arquivo existe
        if !path.exists() {
            return Err(PluginError::io("load plugin", path.display().to_string()));
        }

        // Tenta obter do cache
        if self.config.enable_cache {
            if let Some(module) = self.cache.get(path).await {
                tracing::debug!("Module cache hit for {}", path.display());
                let plugin_name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                return WasmPluginInstance::new(
                    &self.engine,
                    &module,
                    plugin_name,
                    permissions.unwrap_or_else(|| self.config.default_permissions.clone()),
                ).await;
            }
        }

        // Compila o módulo
        tracing::debug!("Compiling module from {}", path.display());
        let module = Module::from_file(&self.engine, path).map_err(|e| {
            PluginError::Compilation {
                reason: format!("Failed to compile '{}': {}", path.display(), e),
            }
        })?;

        let module = Arc::new(module);

        // Adiciona ao cache
        if self.config.enable_cache {
            self.cache.insert(path.to_path_buf(), Arc::clone(&module)).await;
        }

        // Cria instância
        let plugin_name = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        WasmPluginInstance::new(
            &self.engine,
            &module,
            plugin_name,
            permissions.unwrap_or_else(|| self.config.default_permissions.clone()),
        ).await
    }

    /// Carrega um plugin de bytes
    pub async fn load_from_bytes(
        &self,
        name: impl Into<String>,
        bytes: &[u8],
        permissions: Option<Vec<Permission>>,
    ) -> PluginResult<WasmPluginInstance> {
        let name = name.into();
        
        // Compila o módulo
        let module = Module::new(&self.engine, bytes).map_err(|e| {
            PluginError::Compilation {
                reason: format!("Failed to compile module '{}': {}", name, e),
            }
        })?;

        // Cria instância
        WasmPluginInstance::new(
            &self.engine,
            &module,
            name,
            permissions.unwrap_or_else(|| self.config.default_permissions.clone()),
        ).await
    }

    /// Valida um arquivo WASM sem carregar
    pub fn validate(&self, path: impl AsRef<Path>) -> PluginResult<()> {
        let path = path.as_ref();
        
        let bytes = std::fs::read(path)
            .map_err(|e| PluginError::io("read wasm", format!("{}: {}", path.display(), e)))?;

        self.engine
            .validate(&bytes)
            .map_err(|e| PluginError::Compilation {
                reason: format!("Invalid WASM '{}': {}", path.display(), e),
            })
    }

    /// Lista plugins disponíveis no diretório
    pub async fn list_available_plugins(&self) -> Vec<PathBuf> {
        let mut plugins = Vec::new();

        if let Ok(mut entries) = tokio::fs::read_dir(&self.config.plugins_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.extension().map(|e| e == "wasm").unwrap_or(false) {
                    plugins.push(path);
                }
            }
        }

        plugins
    }

    /// Retorna estatísticas do cache
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    /// Limpa o cache
    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }

    /// Retorna referência ao engine
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    /// Retorna referência à configuração
    pub fn config(&self) -> &WasmLoaderConfig {
        &self.config
    }
}

/// Plugin carregado (wrapper para compatibilidade)
#[derive(Debug)]
pub struct LoadedPlugin {
    /// Instância do plugin
    pub instance: WasmPluginInstance,
    /// Caminho do arquivo (se carregado de arquivo)
    pub path: Option<PathBuf>,
    /// Hash do conteúdo
    pub content_hash: String,
}

impl LoadedPlugin {
    /// Cria novo plugin carregado
    pub fn new(instance: WasmPluginInstance, path: Option<PathBuf>, content_hash: String) -> Self {
        Self {
            instance,
            path,
            content_hash,
        }
    }

    /// Retorna nome do plugin
    pub fn name(&self) -> &str {
        self.instance.plugin_name()
    }

    /// Retorna ID da instância
    pub fn instance_id(&self) -> &str {
        self.instance.instance_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_config_default() {
        let config = WasmLoaderConfig::default();
        assert!(config.enable_cache);
        assert!(!config.default_permissions.is_empty());
    }

    #[test]
    fn test_loader_config_builder() {
        let config = WasmLoaderConfig::default()
            .with_cache(false)
            .with_permissions(vec![Permission::Filesystem]);

        assert!(!config.enable_cache);
        assert_eq!(config.default_permissions.len(), 1);
    }

    #[test]
    fn test_cache_stats() {
        let stats = CacheStats {
            entries: 10,
            hits: 80,
            misses: 20,
        };

        assert_eq!(stats.hit_rate(), 0.8);
    }

    #[test]
    fn test_cache_stats_zero() {
        let stats = CacheStats {
            entries: 0,
            hits: 0,
            misses: 0,
        };

        assert_eq!(stats.hit_rate(), 0.0);
    }
}
