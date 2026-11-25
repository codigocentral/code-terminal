//! API Versioning para Plugins
//!
//! Este módulo gerencia o versionamento da API de plugins,
//! garantindo compatibilidade entre diferentes versões do host e plugins.

use std::fmt;

/// Versão da API de plugins
///
/// Segue semantic versioning (MAJOR.MINOR.PATCH)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginApiVersion {
    /// Versão major - mudanças incompatíveis
    pub major: u32,
    /// Versão minor - funcionalidades adicionadas (compatível)
    pub minor: u32,
    /// Versão patch - bugfixes
    pub patch: u32,
}

impl PluginApiVersion {
    /// Versão atual da API
    pub const CURRENT: Self = Self::new(1, 0, 0);

    /// Versão mínima suportada
    pub const MINIMUM: Self = Self::new(1, 0, 0);

    /// Cria uma nova versão
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse de string no formato "MAJOR.MINOR.PATCH"
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return None;
        }

        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch = parts[2].parse().ok()?;

        Some(Self::new(major, minor, patch))
    }

    /// Verifica se esta versão é compatível com outra
    ///
    /// Regras de compatibilidade:
    /// - Major deve ser igual
    /// - Esta versão deve ser >= versão mínima requerida
    pub fn is_compatible_with(&self, required: &Self) -> bool {
        if self.major != required.major {
            return false;
        }
        self >= required
    }

    /// Verifica se o plugin pode ser carregado nesta versão da API
    pub fn can_load(&self, plugin_version: &Self) -> bool {
        // Major deve ser igual para compatibilidade
        if self.major != plugin_version.major {
            return false;
        }
        // Plugin requer versão <= versão atual do host
        plugin_version <= self
    }

    /// Retorna true se há breaking changes entre duas versões
    pub fn has_breaking_changes(&self, other: &Self) -> bool {
        self.major != other.major
    }
}

impl Default for PluginApiVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

impl fmt::Display for PluginApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Capabilities da API de plugins
///
/// Define quais funcionalidades estão disponíveis nesta versão
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginApi {
    /// Versão da API
    pub version: PluginApiVersion,
    /// Suporte a hot reload
    pub hot_reload: bool,
    /// Suporte a comunicação assíncrona
    pub async_support: bool,
    /// Suporte a UI customizada
    pub custom_ui: bool,
    /// Suporte a temas customizados
    pub custom_themes: bool,
    /// Suporte a keybindings customizados
    pub custom_keybindings: bool,
    /// Suporte a notificações
    pub notifications: bool,
    /// Suporte a acesso ao sistema de arquivos (sandboxed)
    pub filesystem_access: bool,
    /// Suporte a network (sandboxed)
    pub network_access: bool,
}

impl PluginApi {
    /// Cria a API atual com todas as capabilities
    pub fn current() -> Self {
        Self {
            version: PluginApiVersion::CURRENT,
            hot_reload: true,
            async_support: true,
            custom_ui: true,
            custom_themes: true,
            custom_keybindings: true,
            notifications: true,
            filesystem_access: true,
            network_access: false, // Desabilitado por padrão por segurança
        }
    }

    /// Cria API mínima (apenas funcionalidades básicas)
    pub fn minimal() -> Self {
        Self {
            version: PluginApiVersion::MINIMUM,
            hot_reload: false,
            async_support: false,
            custom_ui: false,
            custom_themes: false,
            custom_keybindings: true,
            notifications: false,
            filesystem_access: false,
            network_access: false,
        }
    }

    /// Verifica se uma capability específica está disponível
    pub fn has_capability(&self, capability: ApiCapability) -> bool {
        match capability {
            ApiCapability::HotReload => self.hot_reload,
            ApiCapability::Async => self.async_support,
            ApiCapability::CustomUi => self.custom_ui,
            ApiCapability::CustomThemes => self.custom_themes,
            ApiCapability::CustomKeybindings => self.custom_keybindings,
            ApiCapability::Notifications => self.notifications,
            ApiCapability::Filesystem => self.filesystem_access,
            ApiCapability::Network => self.network_access,
        }
    }
}

/// Capabilities individuais da API
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApiCapability {
    /// Hot reload de plugins
    HotReload,
    /// Suporte assíncrono
    Async,
    /// UI customizada
    CustomUi,
    /// Temas customizados
    CustomThemes,
    /// Keybindings customizados
    CustomKeybindings,
    /// Notificações
    Notifications,
    /// Acesso ao filesystem
    Filesystem,
    /// Acesso à rede
    Network,
}

/// Verifica compatibilidade entre versões
pub fn check_compatibility(
    host_version: &PluginApiVersion,
    plugin_version: &PluginApiVersion,
) -> CompatibilityResult {
    if host_version.major != plugin_version.major {
        return CompatibilityResult::Incompatible {
            reason: format!(
                "Major version mismatch: host={}, plugin={}",
                host_version.major, plugin_version.major
            ),
        };
    }

    if host_version < plugin_version {
        return CompatibilityResult::HostTooOld {
            host: *host_version,
            required: *plugin_version,
        };
    }

    if host_version.minor > plugin_version.minor {
        CompatibilityResult::Compatible {
            warnings: vec![format!(
                "Plugin uses older minor version ({} < {})",
                plugin_version.minor, host_version.minor
            )],
        }
    } else {
        CompatibilityResult::FullyCompatible
    }
}

/// Resultado de verificação de compatibilidade
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompatibilityResult {
    /// Totalmente compatível
    FullyCompatible,
    /// Compatível com avisos
    Compatible { warnings: Vec<String> },
    /// Host muito antigo
    HostTooOld {
        host: PluginApiVersion,
        required: PluginApiVersion,
    },
    /// Incompatível
    Incompatible { reason: String },
}

impl CompatibilityResult {
    /// Retorna true se é compatível (mesmo com avisos)
    pub fn is_compatible(&self) -> bool {
        matches!(self, Self::FullyCompatible | Self::Compatible { .. })
    }

    /// Retorna true se é totalmente compatível
    pub fn is_fully_compatible(&self) -> bool {
        matches!(self, Self::FullyCompatible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        let v = PluginApiVersion::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn test_version_parsing_invalid() {
        assert!(PluginApiVersion::parse("1.2").is_none());
        assert!(PluginApiVersion::parse("1.2.3.4").is_none());
        assert!(PluginApiVersion::parse("abc").is_none());
    }

    #[test]
    fn test_version_display() {
        let v = PluginApiVersion::new(1, 2, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn test_compatibility_same_version() {
        let v1 = PluginApiVersion::new(1, 0, 0);
        let v2 = PluginApiVersion::new(1, 0, 0);
        assert!(v1.is_compatible_with(&v2));
    }

    #[test]
    fn test_compatibility_minor_difference() {
        let host = PluginApiVersion::new(1, 5, 0);
        let plugin = PluginApiVersion::new(1, 3, 0);
        assert!(host.is_compatible_with(&plugin));
    }

    #[test]
    fn test_compatibility_major_mismatch() {
        let v1 = PluginApiVersion::new(1, 0, 0);
        let v2 = PluginApiVersion::new(2, 0, 0);
        assert!(!v1.is_compatible_with(&v2));
    }

    #[test]
    fn test_compatibility_host_too_old() {
        let host = PluginApiVersion::new(1, 0, 0);
        let plugin = PluginApiVersion::new(1, 5, 0);
        assert!(!host.is_compatible_with(&plugin));
    }

    #[test]
    fn test_check_compatibility_fully() {
        let host = PluginApiVersion::new(1, 5, 0);
        let plugin = PluginApiVersion::new(1, 5, 0);
        let result = check_compatibility(&host, &plugin);
        assert!(result.is_fully_compatible());
    }

    #[test]
    fn test_check_compatibility_host_too_old() {
        let host = PluginApiVersion::new(1, 0, 0);
        let plugin = PluginApiVersion::new(1, 5, 0);
        let result = check_compatibility(&host, &plugin);
        assert!(!result.is_compatible());
        assert!(matches!(result, CompatibilityResult::HostTooOld { .. }));
    }

    #[test]
    fn test_api_capabilities() {
        let api = PluginApi::current();
        assert!(api.has_capability(ApiCapability::HotReload));
        assert!(api.has_capability(ApiCapability::CustomKeybindings));
        assert!(!api.has_capability(ApiCapability::Network));
    }
}
