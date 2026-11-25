//! Plugin Context
//!
//! Fornece acesso controlado às funcionalidades do host para plugins.

use crate::config::theme::Theme;
use crate::plugin::api::{ApiCapability, PluginApi};
use crate::plugin::error::{PluginError, PluginResult};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

/// Contexto fornecido aos plugins
///
/// O contexto é a interface entre o plugin e o host. Toda interação
/// do plugin com o terminal deve passar pelo contexto.
#[derive(Clone)]
pub struct PluginContext {
    /// API disponível
    api: PluginApi,
    /// Capabilities habilitadas para este plugin
    enabled_capabilities: Vec<ApiCapability>,
    /// Diretório de dados do plugin
    data_dir: PathBuf,
    /// Diretório de configuração do plugin
    config_dir: PathBuf,
    /// Configurações do plugin
    config: HashMap<String, String>,
    /// Canal de notificações
    notification_tx: Option<std::sync::mpsc::Sender<PluginNotification>>,
    /// ID do plugin
    plugin_id: u64,
    /// Nome do plugin
    plugin_name: String,
}

impl PluginContext {
    /// Cria um novo contexto
    pub fn new(
        api: PluginApi,
        plugin_id: u64,
        plugin_name: impl Into<String>,
        data_dir: PathBuf,
        config_dir: PathBuf,
    ) -> Self {
        Self {
            api,
            enabled_capabilities: Vec::new(),
            data_dir,
            config_dir,
            config: HashMap::new(),
            notification_tx: None,
            plugin_id,
            plugin_name: plugin_name.into(),
        }
    }

    /// Retorna a versão da API
    pub fn api_version(&self) -> String {
        self.api.version.to_string()
    }

    /// Verifica se uma capability está disponível
    pub fn has_capability(&self, capability: ApiCapability) -> bool {
        self.api.has_capability(capability) && self.enabled_capabilities.contains(&capability)
    }

    /// Habilita uma capability
    pub fn enable_capability(&mut self, capability: ApiCapability) {
        if !self.enabled_capabilities.contains(&capability) {
            self.enabled_capabilities.push(capability);
        }
    }

    /// Desabilita uma capability
    pub fn disable_capability(&mut self, capability: ApiCapability) {
        self.enabled_capabilities.retain(|&c| c != capability);
    }

    /// Retorna o ID do plugin
    pub fn plugin_id(&self) -> u64 {
        self.plugin_id
    }

    /// Retorna o nome do plugin
    pub fn plugin_name(&self) -> &str {
        &self.plugin_name
    }

    /// Retorna o diretório de dados
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Retorna o diretório de configuração
    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    /// Obtém uma configuração
    pub fn get_config(&self, key: &str) -> Option<&String> {
        self.config.get(key)
    }

    /// Define uma configuração
    pub fn set_config(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.config.insert(key.into(), value.into());
    }

    /// Retorna todas as configurações
    pub fn config(&self) -> &HashMap<String, String> {
        &self.config
    }

    // === Métodos que requerem capabilities ===

    /// Envia uma notificação
    ///
    /// # Capability
    /// Requer `ApiCapability::Notifications`
    pub fn notify(&self, title: impl Into<String>, message: impl Into<String>) -> PluginResult<()> {
        if !self.has_capability(ApiCapability::Notifications) {
            return Err(PluginError::sandbox_violation(
                "notify",
                "Capability 'notifications' not enabled",
            ));
        }

        if let Some(ref tx) = self.notification_tx {
            let notification = PluginNotification {
                plugin_id: self.plugin_id,
                plugin_name: self.plugin_name.clone(),
                title: title.into(),
                message: message.into(),
                timestamp: std::time::SystemTime::now(),
            };
            tx.send(notification).map_err(|_| PluginError::Execution {
                reason: "Failed to send notification".to_string(),
            })?;
        }

        Ok(())
    }

    /// Lê um arquivo
    ///
    /// # Capability
    /// Requer `ApiCapability::Filesystem`
    pub fn read_file(&self, path: impl AsRef<std::path::Path>) -> PluginResult<Vec<u8>> {
        if !self.has_capability(ApiCapability::Filesystem) {
            return Err(PluginError::sandbox_violation(
                "read_file",
                "Capability 'filesystem' not enabled",
            ));
        }

        let path = path.as_ref();

        // Verifica se está dentro do sandbox
        if !self.is_path_allowed(path) {
            return Err(PluginError::sandbox_violation(
                "read_file",
                format!("Path '{}' is outside sandbox", path.display()),
            ));
        }

        std::fs::read(path).map_err(|_e| PluginError::io("read_file", path.display().to_string()))
    }

    /// Escreve um arquivo
    ///
    /// # Capability
    /// Requer `ApiCapability::Filesystem`
    pub fn write_file(&self, path: impl AsRef<std::path::Path>, data: &[u8]) -> PluginResult<()> {
        if !self.has_capability(ApiCapability::Filesystem) {
            return Err(PluginError::sandbox_violation(
                "write_file",
                "Capability 'filesystem' not enabled",
            ));
        }

        let path = path.as_ref();

        if !self.is_path_allowed(path) {
            return Err(PluginError::sandbox_violation(
                "write_file",
                format!("Path '{}' is outside sandbox", path.display()),
            ));
        }

        std::fs::write(path, data)
            .map_err(|_e| PluginError::io("write_file", path.display().to_string()))
    }

    /// Executa uma requisição HTTP
    ///
    /// # Capability
    /// Requer `ApiCapability::Network`
    pub async fn http_get(&self, _url: impl AsRef<str>) -> PluginResult<String> {
        if !self.has_capability(ApiCapability::Network) {
            return Err(PluginError::sandbox_violation(
                "http_get",
                "Capability 'network' not enabled",
            ));
        }

        // Implementação real usaria reqwest ou similar
        // Por enquanto retorna erro de não implementado
        Err(PluginError::Other {
            message: "HTTP not implemented yet".to_string(),
        })
    }

    /// Obtém o tema atual
    ///
    /// # Capability
    /// Requer `ApiCapability::CustomThemes`
    pub fn current_theme(&self) -> PluginResult<Theme> {
        if !self.has_capability(ApiCapability::CustomThemes) {
            return Err(PluginError::sandbox_violation(
                "current_theme",
                "Capability 'custom_themes' not enabled",
            ));
        }

        // Retornaria o tema atual do terminal
        Err(PluginError::Other {
            message: "Theme access not implemented yet".to_string(),
        })
    }

    /// Registra um keybinding
    ///
    /// # Capability
    /// Requer `ApiCapability::CustomKeybindings`
    pub fn register_keybinding(
        &self,
        _keys: impl Into<String>,
        _action: impl Into<String>,
    ) -> PluginResult<()> {
        if !self.has_capability(ApiCapability::CustomKeybindings) {
            return Err(PluginError::sandbox_violation(
                "register_keybinding",
                "Capability 'custom_keybindings' not enabled",
            ));
        }

        // Implementação real registraria o keybinding
        Ok(())
    }

    /// Verifica se um caminho é permitido (dentro do sandbox)
    fn is_path_allowed(&self, path: &std::path::Path) -> bool {
        // Permite acesso ao data_dir e config_dir do plugin. O caminho é
        // normalizado antes: "data_dir/../../etc" começava com data_dir e
        // passava no `starts_with`.
        let Some(normalized) = crate::plugin::sandbox::normalize_lexically(path) else {
            return false;
        };
        let allowed_prefixes = [&self.data_dir, &self.config_dir];

        allowed_prefixes.iter().any(|prefix| {
            normalized.starts_with(prefix)
                || path
                    .canonicalize()
                    .map(|p| p.starts_with(prefix))
                    .unwrap_or(false)
        })
    }

    /// Configura o canal de notificações
    pub fn set_notification_channel(&mut self, tx: std::sync::mpsc::Sender<PluginNotification>) {
        self.notification_tx = Some(tx);
    }
}

impl fmt::Debug for PluginContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginContext")
            .field("plugin_id", &self.plugin_id)
            .field("plugin_name", &self.plugin_name)
            .field("api_version", &self.api_version())
            .field("capabilities", &self.enabled_capabilities.len())
            .field("data_dir", &self.data_dir)
            .field("config_dir", &self.config_dir)
            .finish()
    }
}

/// Notificação de plugin
#[derive(Debug, Clone)]
pub struct PluginNotification {
    /// ID do plugin que enviou
    pub plugin_id: u64,
    /// Nome do plugin
    pub plugin_name: String,
    /// Título da notificação
    pub title: String,
    /// Mensagem
    pub message: String,
    /// Timestamp
    pub timestamp: std::time::SystemTime,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_context() -> PluginContext {
        PluginContext::new(
            PluginApi::current(),
            1,
            "test-plugin",
            PathBuf::from("/tmp/plugin-data"),
            PathBuf::from("/tmp/plugin-config"),
        )
    }

    #[test]
    fn test_context_new() {
        let ctx = create_test_context();
        assert_eq!(ctx.plugin_id(), 1);
        assert_eq!(ctx.plugin_name(), "test-plugin");
    }

    #[test]
    fn test_context_capabilities() {
        let mut ctx = create_test_context();

        assert!(!ctx.has_capability(ApiCapability::Notifications));

        ctx.enable_capability(ApiCapability::Notifications);
        assert!(ctx.has_capability(ApiCapability::Notifications));

        ctx.disable_capability(ApiCapability::Notifications);
        assert!(!ctx.has_capability(ApiCapability::Notifications));
    }

    #[test]
    fn test_context_config() {
        let mut ctx = create_test_context();

        assert!(ctx.get_config("key").is_none());

        ctx.set_config("key", "value");
        assert_eq!(ctx.get_config("key"), Some(&"value".to_string()));
    }

    #[test]
    fn test_context_notify_without_capability() {
        let ctx = create_test_context();
        let result = ctx.notify("Title", "Message");
        assert!(result.is_err());
    }

    #[test]
    fn test_context_read_file_without_capability() {
        let ctx = create_test_context();
        let result = ctx.read_file("/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_context_write_file_without_capability() {
        let ctx = create_test_context();
        let result = ctx.write_file("/tmp/test.txt", b"data");
        assert!(result.is_err());
    }

    #[test]
    fn test_context_notify_with_capability_but_no_channel() {
        let mut ctx = create_test_context();
        ctx.enable_capability(ApiCapability::Notifications);

        let result = ctx.notify("Title", "Message");
        assert!(result.is_ok());
    }

    #[test]
    fn test_context_api_version() {
        let ctx = create_test_context();
        assert_eq!(ctx.api_version(), "1.0.0");
    }

    #[test]
    fn test_context_debug_format() {
        let ctx = create_test_context();
        let debug_str = format!("{:?}", ctx);
        assert!(debug_str.contains("test-plugin"));
        assert!(debug_str.contains("plugin_id"));
    }

    #[test]
    fn test_context_data_dir() {
        let ctx = create_test_context();
        assert_eq!(ctx.data_dir(), &PathBuf::from("/tmp/plugin-data"));
    }

    #[test]
    fn test_context_config_dir() {
        let ctx = create_test_context();
        assert_eq!(ctx.config_dir(), &PathBuf::from("/tmp/plugin-config"));
    }

    #[test]
    fn test_context_register_keybinding_without_capability() {
        let ctx = create_test_context();
        let result = ctx.register_keybinding("Ctrl+A", "action");
        assert!(result.is_err());
    }

    #[test]
    fn test_context_register_keybinding_with_capability() {
        let mut ctx = create_test_context();
        ctx.enable_capability(ApiCapability::CustomKeybindings);
        let result = ctx.register_keybinding("Ctrl+A", "action");
        assert!(result.is_ok());
    }

    #[test]
    fn test_context_multiple_capabilities() {
        let mut ctx = create_test_context();

        ctx.enable_capability(ApiCapability::Notifications);
        ctx.enable_capability(ApiCapability::CustomKeybindings);
        ctx.enable_capability(ApiCapability::CustomThemes);

        assert!(ctx.has_capability(ApiCapability::Notifications));
        assert!(ctx.has_capability(ApiCapability::CustomKeybindings));
        assert!(ctx.has_capability(ApiCapability::CustomThemes));
    }

    #[test]
    fn test_context_config_all() {
        let mut ctx = create_test_context();
        ctx.set_config("key1", "value1");
        ctx.set_config("key2", "value2");

        let config = ctx.config();
        assert_eq!(config.len(), 2);
    }

    fn sandboxed_ctx(dir: &std::path::Path) -> PluginContext {
        let data = dir.join("data");
        let config = dir.join("config");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        let mut ctx = PluginContext::new(PluginApi::current(), 3, "fs", data, config);
        ctx.enable_capability(ApiCapability::Filesystem);
        ctx
    }

    #[test]
    fn test_file_roundtrip_inside_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = sandboxed_ctx(dir.path());
        let file = ctx.data_dir().join("estado.json");
        ctx.write_file(&file, b"{}").unwrap();
        assert_eq!(ctx.read_file(&file).unwrap(), b"{}");
        let cfg = ctx.config_dir().join("c.toml");
        ctx.write_file(&cfg, b"x=1").unwrap();
        assert_eq!(ctx.read_file(&cfg).unwrap(), b"x=1");

        // Fora do sandbox
        let outside = dir.path().join("fora.txt");
        std::fs::write(&outside, b"segredo").unwrap();
        let err = ctx.read_file(&outside).unwrap_err();
        assert!(err.to_string().contains("outside sandbox"), "{err}");
        assert!(ctx.write_file(&outside, b"x").is_err());

        // Arquivo inexistente dentro do sandbox: erro de IO
        assert!(ctx.read_file(ctx.data_dir().join("nada")).is_err());
        // Diretório pai inexistente: escrita falha
        assert!(ctx
            .write_file(ctx.data_dir().join("a/b/c.txt"), b"x")
            .is_err());
    }

    /// Regressão: "data_dir/../fora.txt" passava no `starts_with(data_dir)`.
    #[test]
    fn test_parent_dir_traversal_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = sandboxed_ctx(dir.path());
        std::fs::write(dir.path().join("fora.txt"), b"segredo").unwrap();

        let sneaky = ctx.data_dir().join("..").join("fora.txt");
        assert!(ctx.read_file(&sneaky).is_err());
        assert!(ctx.write_file(&sneaky, b"x").is_err());
        assert_eq!(
            std::fs::read(dir.path().join("fora.txt")).unwrap(),
            b"segredo"
        );

        // ".." que continua dentro é aceito
        std::fs::create_dir(ctx.data_dir().join("sub")).unwrap();
        let fine = ctx.data_dir().join("sub").join("..").join("ok.txt");
        ctx.write_file(&fine, b"ok").unwrap();
        assert_eq!(std::fs::read(ctx.data_dir().join("ok.txt")).unwrap(), b"ok");
    }

    #[test]
    fn test_notify_through_channel() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut ctx = create_test_context();
        ctx.enable_capability(ApiCapability::Notifications);
        ctx.set_notification_channel(tx);
        ctx.notify("Titulo", "Corpo").unwrap();
        let n = rx.try_recv().unwrap();
        assert_eq!(n.title, "Titulo");
        assert_eq!(n.message, "Corpo");
        assert_eq!(n.plugin_name, ctx.plugin_name());
        assert_eq!(n.plugin_id, ctx.plugin_id());

        // Receptor fechado: erro de execução
        drop(rx);
        let err = ctx.notify("a", "b").unwrap_err();
        assert!(
            err.to_string().contains("Failed to send notification"),
            "{err}"
        );
    }

    #[test]
    fn test_unimplemented_capabilities_and_disable() {
        let mut ctx = create_test_context();
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        assert!(rt.block_on(ctx.http_get("https://a.test")).is_err());
        assert!(ctx.current_theme().is_err());

        // A API atual não oferece rede: habilitar no contexto não basta
        ctx.enable_capability(ApiCapability::Network);
        let err = rt.block_on(ctx.http_get("https://a.test")).unwrap_err();
        assert!(err.to_string().contains("not enabled"), "{err}");

        ctx.enable_capability(ApiCapability::CustomThemes);
        let err = ctx.current_theme().err().unwrap();
        assert!(err.to_string().contains("not implemented"), "{err}");

        ctx.enable_capability(ApiCapability::CustomThemes); // idempotente
        ctx.disable_capability(ApiCapability::CustomThemes);
        assert!(!ctx.has_capability(ApiCapability::CustomThemes));
    }
}
