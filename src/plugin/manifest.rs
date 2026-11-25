//! Plugin Manifest
//!
//! Define a estrutura do arquivo de manifesto de plugins (plugin.toml)

use crate::plugin::api::PluginApiVersion;
use crate::plugin::error::{PluginError, PluginResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// Manifesto de um plugin
///
/// Este é o formato do arquivo plugin.toml que todo plugin deve ter
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManifest {
    /// Informações básicas
    pub plugin: PluginMetadata,
    /// Versão da API requerida
    #[serde(default)]
    pub api: ApiRequirement,
    /// Dependências
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,
    /// Configurações
    #[serde(default)]
    pub config: HashMap<String, String>,
    /// Permissões solicitadas
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Scripts
    #[serde(default)]
    pub scripts: Scripts,
    /// Entrada do plugin (arquivo WASM ou shared lib)
    pub entry: String,
}

impl PluginManifest {
    /// Cria um manifesto mínimo
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        entry: impl Into<String>,
    ) -> Self {
        Self {
            plugin: PluginMetadata {
                name: name.into(),
                version: version.into(),
                description: String::new(),
                author: String::new(),
                homepage: None,
                repository: None,
                license: None,
                keywords: Vec::new(),
            },
            api: ApiRequirement::default(),
            dependencies: Vec::new(),
            config: HashMap::new(),
            permissions: Vec::new(),
            scripts: Scripts::default(),
            entry: entry.into(),
        }
    }

    /// Carrega manifesto de um arquivo TOML
    pub fn from_file(path: impl AsRef<Path>) -> PluginResult<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .map_err(|_e| PluginError::io("read manifest", path.display().to_string()))?;

        Self::from_str(&content)
    }

    /// Parse de string TOML
    pub fn from_str(content: &str) -> PluginResult<Self> {
        toml::from_str(content)
            .map_err(|e| PluginError::invalid_manifest(format!("Failed to parse manifest: {}", e)))
    }

    /// Salva manifesto em arquivo
    pub fn to_file(&self, path: impl AsRef<Path>) -> PluginResult<()> {
        let path = path.as_ref();
        let content = self.to_string_pretty()?;
        std::fs::write(path, content)
            .map_err(|_e| PluginError::io("write manifest", path.display().to_string()))
    }

    /// Converte para string TOML formatada
    pub fn to_string_pretty(&self) -> PluginResult<String> {
        toml::to_string_pretty(self).map_err(|e| {
            PluginError::invalid_manifest(format!("Failed to serialize manifest: {}", e))
        })
    }

    /// Verifica se o manifesto é válido
    pub fn validate(&self) -> PluginResult<()> {
        // Nome não pode ser vazio
        if self.plugin.name.is_empty() {
            return Err(PluginError::invalid_manifest("Plugin name is required"));
        }

        // Versão deve ser válida (semver básico)
        if !Self::is_valid_semver(&self.plugin.version) {
            return Err(PluginError::invalid_manifest(format!(
                "Invalid version: {}",
                self.plugin.version
            )));
        }

        // Entry point deve existir
        if self.entry.is_empty() {
            return Err(PluginError::invalid_manifest("Entry point is required"));
        }

        Ok(())
    }

    /// Retorna o nome do plugin
    pub fn name(&self) -> &str {
        &self.plugin.name
    }

    /// Retorna a versão do plugin
    pub fn version(&self) -> &str {
        &self.plugin.version
    }

    fn is_valid_semver(version: &str) -> bool {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() != 3 {
            return false;
        }

        parts.iter().all(|p| {
            p.parse::<u32>().is_ok() || p.split('-').next().unwrap().parse::<u32>().is_ok()
        })
    }
}

impl fmt::Display for PluginManifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} v{} (entry: {})",
            self.plugin.name, self.plugin.version, self.entry
        )
    }
}

/// Metadados do plugin
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginMetadata {
    /// Nome do plugin
    pub name: String,
    /// Versão (semver)
    pub version: String,
    /// Descrição
    #[serde(default)]
    pub description: String,
    /// Autor
    #[serde(default)]
    pub author: String,
    /// Homepage
    pub homepage: Option<String>,
    /// Repositório
    pub repository: Option<String>,
    /// Licença
    pub license: Option<String>,
    /// Keywords
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Requerimento de API
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiRequirement {
    /// Versão da API requerida
    pub version: String,
    /// Versão mínima (opcional)
    pub min_version: Option<String>,
}

impl ApiRequirement {
    /// Retorna a versão da API como struct
    pub fn parse_version(&self) -> Option<PluginApiVersion> {
        PluginApiVersion::parse(&self.version)
    }

    /// Retorna a versão mínima como struct
    pub fn parse_min_version(&self) -> Option<PluginApiVersion> {
        self.min_version
            .as_ref()
            .and_then(|v| PluginApiVersion::parse(v))
    }
}

impl Default for ApiRequirement {
    fn default() -> Self {
        Self {
            version: PluginApiVersion::CURRENT.to_string(),
            min_version: Some(PluginApiVersion::MINIMUM.to_string()),
        }
    }
}

/// Dependência de plugin
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginDependency {
    /// Nome do plugin dependência
    pub name: String,
    /// Versão requerida (range semver)
    pub version: String,
    /// Se é opcional
    #[serde(default)]
    pub optional: bool,
}

impl PluginDependency {
    /// Cria uma nova dependência obrigatória
    pub fn required(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            optional: false,
        }
    }

    /// Cria uma nova dependência opcional
    pub fn optional(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            optional: true,
        }
    }
}

/// Permissões que um plugin pode solicitar
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Acesso ao sistema de arquivos
    Filesystem,
    /// Acesso à rede
    Network,
    /// Executar comandos shell
    Shell,
    /// Acesso ao clipboard
    Clipboard,
    /// Ler configurações
    ReadConfig,
    /// Modificar configurações
    WriteConfig,
    /// Acesso a notificações
    Notifications,
    /// Acesso ao PTY
    Pty,
    /// UI customizada
    CustomUi,
}

impl Permission {
    /// Retorna a string da permissão
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::Network => "network",
            Self::Shell => "shell",
            Self::Clipboard => "clipboard",
            Self::ReadConfig => "read_config",
            Self::WriteConfig => "write_config",
            Self::Notifications => "notifications",
            Self::Pty => "pty",
            Self::CustomUi => "custom_ui",
        }
    }

    /// Se esta permissão é considerada perigosa
    pub fn is_dangerous(&self) -> bool {
        matches!(self, Self::Shell | Self::Filesystem | Self::Network)
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Scripts do plugin
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Scripts {
    /// Script de build
    pub build: Option<String>,
    /// Script de test
    pub test: Option<String>,
    /// Script de lint
    pub lint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_new() {
        let manifest = PluginManifest::new("meu-plugin", "1.0.0", "plugin.wasm");
        assert_eq!(manifest.plugin.name, "meu-plugin");
        assert_eq!(manifest.plugin.version, "1.0.0");
        assert_eq!(manifest.entry, "plugin.wasm");
    }

    #[test]
    fn test_manifest_from_str() {
        let toml = r#"entry = "plugin.wasm"

[plugin]
name = "test-plugin"
version = "1.0.0"
description = "Plugin de teste"
author = "Test"
"#;

        let manifest = PluginManifest::from_str(toml).unwrap();
        assert_eq!(manifest.plugin.name, "test-plugin");
        assert_eq!(manifest.plugin.version, "1.0.0");
        assert_eq!(manifest.entry, "plugin.wasm");
    }

    #[test]
    fn test_manifest_validate_ok() {
        let manifest = PluginManifest::new("test", "1.0.0", "plugin.wasm");
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn test_manifest_validate_empty_name() {
        let mut manifest = PluginManifest::new("test", "1.0.0", "plugin.wasm");
        manifest.plugin.name = String::new();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_manifest_validate_invalid_version() {
        let manifest = PluginManifest::new("test", "invalid", "plugin.wasm");
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_dependency_required() {
        let dep = PluginDependency::required("other-plugin", "^1.0.0");
        assert_eq!(dep.name, "other-plugin");
        assert_eq!(dep.version, "^1.0.0");
        assert!(!dep.optional);
    }

    #[test]
    fn test_dependency_optional() {
        let dep = PluginDependency::optional("optional-plugin", ">=2.0.0");
        assert!(dep.optional);
    }

    #[test]
    fn test_permission_is_dangerous() {
        assert!(Permission::Shell.is_dangerous());
        assert!(Permission::Filesystem.is_dangerous());
        assert!(Permission::Network.is_dangerous());
        assert!(!Permission::Notifications.is_dangerous());
    }

    #[test]
    fn test_permission_display() {
        assert_eq!(Permission::Filesystem.to_string(), "filesystem");
        assert_eq!(Permission::Network.to_string(), "network");
    }

    #[test]
    fn test_api_requirement_parse() {
        let api_req = ApiRequirement {
            version: "1.0.0".to_string(),
            min_version: Some("1.0.0".to_string()),
        };

        assert!(api_req.parse_version().is_some());
        assert!(api_req.parse_min_version().is_some());
    }

    #[test]
    fn test_file_roundtrip_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugin.toml");
        let mut manifest = PluginManifest::new("arquivo", "1.2.3", "a.wasm");
        manifest.permissions = vec![Permission::Pty, Permission::CustomUi];
        manifest
            .dependencies
            .push(PluginDependency::optional("extra", "^1"));
        manifest.to_file(&path).unwrap();

        let loaded = PluginManifest::from_file(&path).unwrap();
        assert_eq!(loaded.name(), "arquivo");
        assert_eq!(loaded.version(), "1.2.3");
        assert_eq!(loaded.permissions, manifest.permissions);
        assert_eq!(loaded.dependencies, manifest.dependencies);
        assert_eq!(loaded.to_string(), "arquivo v1.2.3 (entry: a.wasm)");

        let err = PluginManifest::from_file(dir.path().join("nada.toml")).unwrap_err();
        assert!(err.to_string().contains("read manifest"), "{err}");
        let err = manifest
            .to_file(dir.path().join("nao/existe/plugin.toml"))
            .unwrap_err();
        assert!(err.to_string().contains("write manifest"), "{err}");
        let err = PluginManifest::from_str("[plugin\nname=").unwrap_err();
        assert!(
            err.to_string().contains("Failed to parse manifest"),
            "{err}"
        );
    }

    #[test]
    fn test_validate_entry_and_semver_variants() {
        let mut manifest = PluginManifest::new("p", "1.0.0-beta", "");
        let err = manifest.validate().unwrap_err();
        assert!(err.to_string().contains("Entry point"), "{err}");
        manifest.entry = "p.wasm".into();
        manifest.validate().unwrap();

        for bad in ["1.0", "1.0.0.0", "a.b.c", "1.x.0"] {
            manifest.plugin.version = bad.into();
            assert!(manifest.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn test_permission_names_and_api_requirement_defaults() {
        let names: Vec<_> = [
            Permission::Clipboard,
            Permission::ReadConfig,
            Permission::WriteConfig,
            Permission::Notifications,
            Permission::Pty,
            Permission::CustomUi,
        ]
        .iter()
        .map(|p| p.as_str())
        .collect();
        assert_eq!(
            names,
            vec![
                "clipboard",
                "read_config",
                "write_config",
                "notifications",
                "pty",
                "custom_ui"
            ]
        );
        assert!(!Permission::Pty.is_dangerous());

        let requirement = ApiRequirement::default();
        assert!(requirement.parse_version().is_some());
        assert!(requirement.parse_min_version().is_some());
        let none = ApiRequirement {
            version: "x".into(),
            min_version: None,
        };
        assert!(none.parse_version().is_none());
        assert!(none.parse_min_version().is_none());
    }
}
