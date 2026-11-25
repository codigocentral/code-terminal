//! Plugin Loader
//!
//! Carrega e instancia plugins WASM a partir de arquivos ou bytes.

use crate::plugin::error::{PluginError, PluginResult};
use crate::plugin::manifest::PluginManifest;
use crate::plugin::wasm_plugin::WasmPluginInstance;
use crate::plugin::wasm_runtime::WasmRuntime;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasmtime::{Instance, Module, Store};

/// Loader de plugins WASM
///
/// Responsável por:
/// - Carregar arquivos .wasm
/// - Compilar módulos
/// - Resolver imports
/// - Instanciar plugins
pub struct WasmPluginLoader {
    /// Runtime WASM
    runtime: Arc<WasmRuntime>,
    /// Módulos compilados em cache: nome -> (hash dos bytes, módulo)
    module_cache: HashMap<String, (u64, Arc<Module>)>,
    /// Diretório de plugins
    plugins_dir: PathBuf,
}

impl WasmPluginLoader {
    /// Cria um novo loader
    pub fn new(runtime: Arc<WasmRuntime>, plugins_dir: impl AsRef<Path>) -> Self {
        Self {
            runtime,
            module_cache: HashMap::new(),
            plugins_dir: plugins_dir.as_ref().to_path_buf(),
        }
    }

    /// Carrega um plugin a partir de um arquivo .wasm
    pub fn load_from_file(&mut self, path: impl AsRef<Path>) -> PluginResult<LoadedPlugin> {
        let path = path.as_ref();

        // Verificar se arquivo existe
        if !path.exists() {
            return Err(PluginError::io(
                "load plugin",
                format!("File not found: {}", path.display()),
            ));
        }

        // Compilar módulo
        let module = self.runtime.compile_file(path)?;

        // Extrair nome do plugin do path
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(LoadedPlugin {
            name,
            module,
            path: path.to_path_buf(),
        })
    }

    /// Carrega um plugin a partir de bytes
    pub fn load_from_bytes(
        &mut self,
        name: impl Into<String>,
        bytes: &[u8],
    ) -> PluginResult<LoadedPlugin> {
        let name = name.into();
        let hash = bytes_hash(bytes);

        // Verificar cache: só reaproveita se os bytes forem os mesmos (antes
        // bytes novos com o mesmo nome devolviam o módulo antigo)
        if let Some((cached_hash, module)) = self.module_cache.get(&name) {
            if *cached_hash == hash {
                return Ok(LoadedPlugin {
                    name,
                    module: Arc::clone(module),
                    path: PathBuf::new(),
                });
            }
        }

        // Compilar módulo
        let module = self.runtime.compile_bytes(bytes)?;

        // Adicionar ao cache
        self.module_cache
            .insert(name.clone(), (hash, Arc::clone(&module)));

        Ok(LoadedPlugin {
            name,
            module,
            path: PathBuf::new(),
        })
    }

    /// Carrega um plugin a partir do manifest
    pub fn load_from_manifest(
        &mut self,
        manifest: &PluginManifest,
        plugin_dir: impl AsRef<Path>,
    ) -> PluginResult<LoadedPlugin> {
        let plugin_dir = plugin_dir.as_ref();
        let entry_path = plugin_dir.join(&manifest.entry);

        self.load_from_file(&entry_path)
    }

    /// Remove um módulo do cache
    pub fn invalidate_cache(&mut self, name: &str) {
        self.module_cache.remove(name);
    }

    /// Limpa todo o cache de módulos
    pub fn clear_cache(&mut self) {
        self.module_cache.clear();
    }

    /// Retorna o tamanho do cache
    pub fn cache_size(&self) -> usize {
        self.module_cache.len()
    }

    /// Verifica se um plugin está em cache
    pub fn is_cached(&self, name: &str) -> bool {
        self.module_cache.contains_key(name)
    }

    /// Retorna referência ao runtime
    pub fn runtime(&self) -> &Arc<WasmRuntime> {
        &self.runtime
    }

    /// Retorna diretório de plugins
    pub fn plugins_dir(&self) -> &Path {
        &self.plugins_dir
    }

    /// Cria uma instância de plugin a partir de um módulo carregado
    ///
    /// # Arguments
    /// * `loaded` - Plugin carregado (módulo compilado)
    /// * `manifest` - Manifesto do plugin
    ///
    /// # Returns
    /// Uma nova instância do plugin, pronta para ser inicializada
    pub fn create_instance(
        &self,
        loaded: &LoadedPlugin,
        manifest: PluginManifest,
    ) -> PluginResult<WasmPluginInstance> {
        let mut instance = WasmPluginInstance::new(
            loaded.name.clone(),
            Arc::clone(&loaded.module),
            manifest,
            if loaded.path.as_os_str().is_empty() {
                None
            } else {
                Some(loaded.path.clone())
            },
        );

        // Instanciar o módulo WASM
        instance.instantiate(&self.runtime)?;

        Ok(instance)
    }

    /// Carrega e cria uma instância de plugin a partir de arquivo
    ///
    /// Combina o carregamento do arquivo e criação da instância em uma única operação.
    pub fn load_and_instantiate(
        &mut self,
        path: impl AsRef<Path>,
    ) -> PluginResult<WasmPluginInstance> {
        let loaded = self.load_from_file(path.as_ref())?;

        // Tentar carregar manifest
        let manifest_path = path.as_ref().parent().map(|p| p.join("plugin.toml"));

        let manifest = if let Some(manifest_path) = manifest_path {
            if manifest_path.exists() {
                let content = std::fs::read_to_string(&manifest_path)
                    .map_err(|e| PluginError::io("read manifest", e.to_string()))?;
                PluginManifest::from_str(&content)?
            } else {
                PluginManifest::new(&loaded.name, "0.0.0", loaded.entry_point())
            }
        } else {
            PluginManifest::new(&loaded.name, "0.0.0", loaded.entry_point())
        };

        self.create_instance(&loaded, manifest)
    }
}

fn bytes_hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Plugin carregado (módulo compilado)
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// Nome do plugin
    pub name: String,
    /// Módulo WASM compilado
    pub module: Arc<Module>,
    /// Caminho do arquivo (se carregado de arquivo)
    pub path: PathBuf,
}

impl LoadedPlugin {
    /// Cria uma nova instância do plugin
    pub fn instantiate<T: Default>(
        &self,
        store: &mut Store<T>,
        imports: &[wasmtime::Extern],
    ) -> PluginResult<Instance> {
        let instance = Instance::new(store, &self.module, imports).map_err(|e| {
            PluginError::init_failed(&self.name, format!("Failed to instantiate: {}", e))
        })?;

        Ok(instance)
    }

    /// Retorna exports do módulo
    pub fn exports(&self) -> impl Iterator<Item = wasmtime::ExportType<'_>> {
        self.module.exports()
    }

    /// Verifica se exporta uma função específica
    pub fn has_export(&self, name: &str) -> bool {
        self.module.exports().any(|e| e.name() == name)
    }

    /// Verifica se é um plugin válido (tem exports necessários)
    pub fn is_valid_plugin(&self) -> bool {
        // Um plugin válido deve ter pelo menos init, update e shutdown
        self.has_export("init") && self.has_export("update") && self.has_export("shutdown")
    }

    /// Retorna o entry point do plugin (nome do arquivo .wasm)
    pub fn entry_point(&self) -> String {
        self.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("plugin.wasm")
            .to_string()
    }
}

/// Cache de módulos WASM
#[derive(Debug, Default)]
pub struct ModuleCache {
    modules: HashMap<String, (Arc<Module>, std::time::SystemTime)>,
    max_entries: usize,
}

impl ModuleCache {
    /// Cria novo cache com limite de entradas
    pub fn with_capacity(max_entries: usize) -> Self {
        Self {
            modules: HashMap::new(),
            max_entries,
        }
    }

    /// Adiciona módulo ao cache
    pub fn insert(&mut self, name: String, module: Arc<Module>) {
        if self.modules.len() >= self.max_entries {
            // Remove entrada mais antiga
            if let Some((oldest_key, _)) = self.modules.iter().min_by_key(|(_, (_, time))| time) {
                let key = oldest_key.clone();
                self.modules.remove(&key);
            }
        }

        self.modules
            .insert(name, (module, std::time::SystemTime::now()));
    }

    /// Obtém módulo do cache
    pub fn get(&self, name: &str) -> Option<Arc<Module>> {
        self.modules.get(name).map(|(module, _)| Arc::clone(module))
    }

    /// Remove módulo do cache
    pub fn remove(&mut self, name: &str) -> Option<Arc<Module>> {
        self.modules.remove(name).map(|(module, _)| module)
    }

    /// Limpa cache
    pub fn clear(&mut self) {
        self.modules.clear();
    }

    /// Retorna tamanho do cache
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Verifica se cache está vazio
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::Plugin;

    fn create_test_loader() -> WasmPluginLoader {
        let runtime = Arc::new(WasmRuntime::new().unwrap());
        WasmPluginLoader::new(runtime, "/tmp/plugins")
    }

    #[test]
    fn test_loader_creation() {
        let loader = create_test_loader();
        assert_eq!(loader.cache_size(), 0);
    }

    #[test]
    fn test_load_from_nonexistent_file() {
        let mut loader = create_test_loader();
        let result = loader.load_from_file("/nonexistent/plugin.wasm");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_bytes() {
        let mut loader = create_test_loader();

        // WASM mínimo
        let wasm = wat::parse_str("(module)").unwrap();

        let result = loader.load_from_bytes("test", &wasm);
        assert!(result.is_ok());

        let plugin = result.unwrap();
        assert_eq!(plugin.name, "test");
        assert!(loader.is_cached("test"));
    }

    #[test]
    fn test_cache_behavior() {
        let mut loader = create_test_loader();
        let wasm = wat::parse_str("(module)").unwrap();

        // Primeira carga
        let _ = loader.load_from_bytes("cached", &wasm);
        assert_eq!(loader.cache_size(), 1);

        // Segunda carga (deve vir do cache)
        let _ = loader.load_from_bytes("cached", &wasm);
        assert_eq!(loader.cache_size(), 1); // Não duplicou
    }

    #[test]
    fn test_clear_cache() {
        let mut loader = create_test_loader();
        let wasm = wat::parse_str("(module)").unwrap();

        let _ = loader.load_from_bytes("test1", &wasm);
        let _ = loader.load_from_bytes("test2", &wasm);

        assert_eq!(loader.cache_size(), 2);

        loader.clear_cache();
        assert_eq!(loader.cache_size(), 0);
    }

    #[test]
    fn test_module_cache() {
        let mut cache = ModuleCache::with_capacity(2);

        // Adicionar módulos mock (não podemos criar Module sem engine)
        assert_eq!(cache.len(), 0);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_create_instance() {
        let runtime = Arc::new(WasmRuntime::new().unwrap());
        let loader = WasmPluginLoader::new(Arc::clone(&runtime), "/tmp/plugins");
        let wasm = wat::parse_str("(module)").unwrap();

        let loaded = loader.runtime.compile_bytes(&wasm).unwrap();
        let loaded_plugin = LoadedPlugin {
            name: "test".to_string(),
            module: loaded,
            path: PathBuf::from("/tmp/test.wasm"),
        };

        let manifest = PluginManifest::new("test", "1.0.0", "test.wasm");
        let result = loader.create_instance(&loaded_plugin, manifest);

        assert!(result.is_ok());
        let instance = result.unwrap();
        assert_eq!(instance.info().name, "test");
    }

    #[test]
    fn test_load_and_instantiate_from_bytes() {
        let runtime = Arc::new(WasmRuntime::new().unwrap());
        let loader = WasmPluginLoader::new(Arc::clone(&runtime), "/tmp/plugins");

        let wasm = wat::parse_str("(module)").unwrap();
        let loaded = loader.runtime.compile_bytes(&wasm).unwrap();

        let loaded_plugin = LoadedPlugin {
            name: "test".to_string(),
            module: loaded,
            path: PathBuf::new(),
        };

        let manifest = PluginManifest::new("test", "1.0.0", "plugin.wasm");
        let result = loader.create_instance(&loaded_plugin, manifest);

        assert!(result.is_ok());
        assert!(result.unwrap().state().is_initializing_or_running());
    }

    fn wasm_exporting(name: &str) -> Vec<u8> {
        wat::parse_str(format!(r#"(module (func (export "{name}")))"#)).unwrap()
    }

    /// Regressão: bytes novos com o mesmo nome devolviam o módulo em cache.
    #[test]
    fn test_cache_is_invalidated_by_new_bytes() {
        let mut loader = create_test_loader();
        let first = loader.load_from_bytes("p", &wasm_exporting("a")).unwrap();
        assert!(first.has_export("a"));
        let second = loader.load_from_bytes("p", &wasm_exporting("b")).unwrap();
        assert!(second.has_export("b"), "módulo antigo reaproveitado");
        assert!(!second.has_export("a"));
        assert_eq!(loader.cache_size(), 1);

        // Mesmos bytes: reaproveita o mesmo módulo
        let third = loader.load_from_bytes("p", &wasm_exporting("b")).unwrap();
        assert!(Arc::ptr_eq(&second.module, &third.module));

        loader.invalidate_cache("p");
        assert!(!loader.is_cached("p"));
    }

    #[test]
    fn test_invalid_bytes_fail_to_compile() {
        let mut loader = create_test_loader();
        let err = loader.load_from_bytes("x", b"not wasm").unwrap_err();
        assert!(
            err.to_string().contains("compile") || err.to_string().contains("Compil"),
            "{err}"
        );
        assert!(!loader.is_cached("x"));
    }

    #[test]
    fn test_load_from_file_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("meu-plugin.wasm");
        let wasm = wat::parse_str(
            r#"(module (func (export "init")) (func (export "update")) (func (export "shutdown")))"#,
        )
        .unwrap();
        std::fs::write(&wasm_path, &wasm).unwrap();

        let mut loader = create_test_loader();
        assert_eq!(loader.plugins_dir(), Path::new("/tmp/plugins"));
        let loaded = loader.load_from_file(&wasm_path).unwrap();
        assert_eq!(loaded.name, "meu-plugin");
        assert_eq!(loaded.entry_point(), "meu-plugin.wasm");
        assert!(loaded.is_valid_plugin());
        assert_eq!(loaded.exports().count(), 3);

        let manifest = PluginManifest::new("meu-plugin", "1.0.0", "meu-plugin.wasm");
        let via_manifest = loader.load_from_manifest(&manifest, dir.path()).unwrap();
        assert_eq!(via_manifest.path, wasm_path);

        let missing = PluginManifest::new("x", "1.0.0", "nao-existe.wasm");
        assert!(loader.load_from_manifest(&missing, dir.path()).is_err());
    }

    #[test]
    fn test_load_and_instantiate_uses_manifest_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let wasm_path = dir.path().join("p.wasm");
        std::fs::write(&wasm_path, wat::parse_str("(module)").unwrap()).unwrap();

        let mut loader = create_test_loader();
        // Sem plugin.toml: manifesto padrão
        let instance = loader.load_and_instantiate(&wasm_path).unwrap();
        assert_eq!(instance.manifest().plugin.version, "0.0.0");
        assert_eq!(instance.manifest().entry, "p.wasm");
        assert_eq!(instance.path(), Some(&wasm_path));

        // Com plugin.toml
        let manifest = PluginManifest::new("p", "3.1.0", "p.wasm");
        std::fs::write(
            dir.path().join("plugin.toml"),
            manifest.to_string_pretty().unwrap(),
        )
        .unwrap();
        let instance = loader.load_and_instantiate(&wasm_path).unwrap();
        assert_eq!(instance.manifest().plugin.version, "3.1.0");

        // Manifesto inválido
        std::fs::write(dir.path().join("plugin.toml"), "isto não é toml [").unwrap();
        assert!(loader.load_and_instantiate(&wasm_path).is_err());
    }

    #[test]
    fn test_loaded_plugin_instantiate_and_validity() {
        let runtime = WasmRuntime::new().unwrap();
        let loaded = LoadedPlugin {
            name: "x".into(),
            module: runtime.compile_bytes(&wasm_exporting("init")).unwrap(),
            path: PathBuf::new(),
        };
        assert!(!loaded.is_valid_plugin());
        assert_eq!(loaded.entry_point(), "plugin.wasm");
        let mut store: Store<()> = runtime.create_store(());
        loaded.instantiate(&mut store, &[]).unwrap();

        // Módulo com import exige o extern
        let with_import = LoadedPlugin {
            name: "imp".into(),
            module: runtime
                .compile_bytes(&wat::parse_str(r#"(module (import "m" "f" (func)))"#).unwrap())
                .unwrap(),
            path: PathBuf::new(),
        };
        let err = with_import.instantiate(&mut store, &[]).unwrap_err();
        assert!(err.to_string().contains("Failed to instantiate"), "{err}");
    }

    #[test]
    fn test_module_cache_evicts_oldest() {
        let runtime = WasmRuntime::new().unwrap();
        let module = runtime
            .compile_bytes(&wat::parse_str("(module)").unwrap())
            .unwrap();
        let mut cache = ModuleCache::with_capacity(2);
        cache.insert("a".into(), module.clone());
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache.insert("b".into(), module.clone());
        std::thread::sleep(std::time::Duration::from_millis(5));
        cache.insert("c".into(), module.clone());
        assert_eq!(cache.len(), 2);
        assert!(cache.get("a").is_none());
        assert!(cache.get("c").is_some());
        assert!(cache.remove("b").is_some());
        assert!(cache.remove("b").is_none());
        assert!(!cache.is_empty());
    }
}
