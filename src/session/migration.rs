//! Sistema de migração de sessões entre versões
//!
//! Este módulo fornece infraestrutura para migrar sessões salvas
//! em formatos antigos para a versão atual do formato.
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::session::{SessionManager, VersionManager};
//!
//! let manager = SessionManager::default().unwrap();
//! let version_manager = VersionManager::new();
//!
//! // Verificar se uma sessão precisa de migração
//! if let Some(session) = manager.load("old-session").ok() {
//!     if version_manager.needs_migration(&session) {
//!         let migrated = version_manager.migrate(session).unwrap();
//!         manager.save("migrated-session", &migrated.data).unwrap();
//!     }
//! }
//! ```

use super::{
    error::{SessionError, SessionResult},
    serialization::SessionData,
    Session, SESSION_FORMAT_VERSION,
};
use std::collections::HashMap;
use tracing::{debug, info, warn};

/// Trait para migradores de versão
pub trait VersionMigrator: Send + Sync {
    /// Versão de origem (de qual versão migra)
    fn from_version(&self) -> u32;
    /// Versão de destino (para qual versão migra)
    fn to_version(&self) -> u32;
    /// Descrição da migração
    fn description(&self) -> &str;
    /// Executa a migração
    fn migrate(&self, data: SessionData) -> SessionResult<SessionData>;
}

/// Migrador identidade (quando não há mudanças necessárias)
pub struct IdentityMigrator {
    version: u32,
}

impl IdentityMigrator {
    /// Cria um migrador identidade
    pub fn new(version: u32) -> Self {
        Self { version }
    }
}

impl VersionMigrator for IdentityMigrator {
    fn from_version(&self) -> u32 {
        self.version
    }

    fn to_version(&self) -> u32 {
        self.version
    }

    fn description(&self) -> &str {
        "Sem alterações necessárias"
    }

    fn migrate(&self, data: SessionData) -> SessionResult<SessionData> {
        Ok(data)
    }
}

/// Migração da versão 0 (pré-versão) para versão 1
pub struct V0ToV1Migrator;

impl V0ToV1Migrator {
    /// Cria o migrador
    pub fn new() -> Self {
        Self
    }
}

impl Default for V0ToV1Migrator {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionMigrator for V0ToV1Migrator {
    fn from_version(&self) -> u32 {
        0
    }

    fn to_version(&self) -> u32 {
        1
    }

    fn description(&self) -> &str {
        "Adiciona version field e estrutura de layout"
    }

    fn migrate(&self, mut data: SessionData) -> SessionResult<SessionData> {
        // Versão 0 não tinha o campo version definido corretamente
        // Garante que todos os campos obrigatórios da v1 existam
        data.version = 1;

        // Garante que layout existe
        if data.layout.panes.is_empty() {
            // Cria um layout padrão se não existir
            data.layout = super::serialization::LayoutData::new();
        }

        info!("Migrado da versão 0 para 1");
        Ok(data)
    }
}

/// Migração da versão 1 para versão 2 (exemplo futuro)
pub struct V1ToV2Migrator;

impl V1ToV2Migrator {
    /// Cria o migrador
    pub fn new() -> Self {
        Self
    }
}

impl Default for V1ToV2Migrator {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionMigrator for V1ToV2Migrator {
    fn from_version(&self) -> u32 {
        1
    }

    fn to_version(&self) -> u32 {
        2
    }

    fn description(&self) -> &str {
        "Exemplo: Adiciona campo de histórico de comandos"
    }

    fn migrate(&self, mut data: SessionData) -> SessionResult<SessionData> {
        // Exemplo: Migração futura
        // Neste exemplo, apenas atualiza a versão
        // Em uma migração real, faria conversões necessárias
        data.version = 2;

        debug!("Migrado da versão 1 para 2");
        Ok(data)
    }
}

/// Registro de migradores
pub struct MigrationRegistry {
    /// Mapa de (from_version, to_version) -> migrador
    migrators: HashMap<(u32, u32), Box<dyn VersionMigrator>>,
    /// Versão alvo (atual)
    target_version: u32,
}

impl std::fmt::Debug for MigrationRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MigrationRegistry")
            .field("migrators_count", &self.migrators.len())
            .field("target_version", &self.target_version)
            .field("supported_versions", &self.supported_versions())
            .finish()
    }
}

impl Default for MigrationRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationRegistry {
    /// Cria um novo registro com migradores padrão
    pub fn new() -> Self {
        let mut registry = Self {
            migrators: HashMap::new(),
            target_version: SESSION_FORMAT_VERSION,
        };

        // Registra migradores padrão
        registry.register(Box::new(V0ToV1Migrator::new()));
        registry.register(Box::new(V1ToV2Migrator::new()));

        registry
    }

    /// Cria registro com versão alvo específica
    pub fn with_target_version(target_version: u32) -> Self {
        Self {
            migrators: HashMap::new(),
            target_version,
        }
    }

    /// Registra um migrador
    pub fn register(&mut self, migrator: Box<dyn VersionMigrator>) {
        let key = (migrator.from_version(), migrator.to_version());
        self.migrators.insert(key, migrator);
    }

    /// Obtém um migrador específico
    pub fn get_migrator(&self, from: u32, to: u32) -> Option<&dyn VersionMigrator> {
        self.migrators.get(&(from, to)).map(|b| b.as_ref())
    }

    /// Encontra o caminho de migração entre duas versões
    /// Retorna uma lista de (from, to) representando os passos
    pub fn find_migration_path(&self, from: u32, to: u32) -> Option<Vec<(u32, u32)>> {
        if from == to {
            return Some(Vec::new());
        }

        if from > to {
            // Downgrade não suportado
            warn!("Downgrade de versão {} para {} não suportado", from, to);
            return None;
        }

        // BFS para encontrar caminho
        let mut visited = HashMap::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(from);
        visited.insert(from, None);

        while let Some(current) = queue.pop_front() {
            if current == to {
                // Reconstrói o caminho
                let mut path = Vec::new();
                let mut node = to;
                while let Some(Some(prev)) = visited.get(&node) {
                    path.push((*prev, node));
                    node = *prev;
                }
                path.reverse();
                return Some(path);
            }

            // Encontra todas as versões alcançáveis a partir da atual
            for (from_ver, to_ver) in self.migrators.keys() {
                if *from_ver == current && !visited.contains_key(to_ver) {
                    visited.insert(*to_ver, Some(current));
                    queue.push_back(*to_ver);
                }
            }
        }

        None
    }

    /// Versão alvo
    pub fn target_version(&self) -> u32 {
        self.target_version
    }

    /// Define versão alvo
    pub fn set_target_version(&mut self, version: u32) {
        self.target_version = version;
    }

    /// Lista todas as versões suportadas
    pub fn supported_versions(&self) -> Vec<u32> {
        let mut versions: Vec<u32> = self
            .migrators
            .keys()
            .flat_map(|(from, to)| vec![*from, *to])
            .collect();
        versions.push(self.target_version);
        versions.sort_unstable();
        versions.dedup();
        versions
    }
}

/// Gerenciador de versões e migrações
#[derive(Debug)]
pub struct VersionManager {
    registry: MigrationRegistry,
}

impl Default for VersionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            registry: MigrationRegistry::new(),
        }
    }

    /// Cria com registro customizado
    pub fn with_registry(registry: MigrationRegistry) -> Self {
        Self { registry }
    }

    /// Verifica se uma sessão precisa de migração
    pub fn needs_migration(&self, session: &Session) -> bool {
        session.metadata.version != self.registry.target_version()
    }

    /// Obtém a versão de uma sessão
    pub fn session_version(&self, session: &Session) -> u32 {
        session.metadata.version
    }

    /// Verifica se uma versão é suportada
    pub fn is_version_supported(&self, version: u32) -> bool {
        self.registry.supported_versions().contains(&version)
    }

    /// Migra uma sessão para a versão atual
    pub fn migrate(&self, mut session: Session) -> SessionResult<Session> {
        let from_version = session.metadata.version;
        let to_version = self.registry.target_version();

        if from_version == to_version {
            debug!("Sessão já está na versão atual ({})", to_version);
            return Ok(session);
        }

        if from_version > to_version {
            return Err(SessionError::UnsupportedVersion {
                version: from_version,
            });
        }

        // Encontra caminho de migração
        let path = self
            .registry
            .find_migration_path(from_version, to_version)
            .ok_or_else(|| {
                SessionError::MigrationFailed(format!(
                    "Não há caminho de migração da versão {} para {}",
                    from_version, to_version
                ))
            })?;

        // Executa as migrações
        let mut current_data = session.data;

        for (from, to) in path {
            let migrator = self.registry.get_migrator(from, to).ok_or_else(|| {
                SessionError::MigrationFailed(format!(
                    "Migrador da versão {} para {} não encontrado",
                    from, to
                ))
            })?;

            info!(
                "Migrando da versão {} para {}: {}",
                from,
                to,
                migrator.description()
            );

            current_data = migrator.migrate(current_data)?;
        }

        // Atualiza metadados
        session.data = current_data;
        session.metadata.version = to_version;
        session.metadata.touch();

        info!("Migração completa: {} -> {}", from_version, to_version);

        Ok(session)
    }

    /// Tenta migrar dados brutos (JSON)
    pub fn migrate_raw(&self, json: &str) -> SessionResult<String> {
        let session: Session =
            serde_json::from_str(json).map_err(|e| SessionError::Deserialization(e.to_string()))?;

        let migrated = self.migrate(session)?;

        serde_json::to_string_pretty(&migrated)
            .map_err(|e| SessionError::Serialization(e.to_string()))
    }

    /// Retorna informações sobre versões suportadas
    pub fn version_info(&self) -> VersionInfo {
        VersionInfo {
            current_version: self.registry.target_version(),
            supported_versions: self.registry.supported_versions(),
        }
    }

    /// Define a versão alvo
    pub fn set_target_version(&mut self, version: u32) {
        self.registry.set_target_version(version);
    }
}

/// Informações sobre versões
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Versão atual do formato
    pub current_version: u32,
    /// Versões suportadas
    pub supported_versions: Vec<u32>,
}

/// Extensão do SessionManager para suportar migração
pub trait MigrationExt {
    /// Carrega e migra uma sessão se necessário
    fn load_and_migrate(
        &self,
        name: impl AsRef<str>,
        version_manager: &VersionManager,
    ) -> SessionResult<Session>;

    /// Verifica se uma sessão precisa de migração
    fn check_migration_needed(&self, name: impl AsRef<str>) -> SessionResult<Option<u32>>;
}

impl MigrationExt for super::SessionManager {
    fn load_and_migrate(
        &self,
        name: impl AsRef<str>,
        version_manager: &VersionManager,
    ) -> SessionResult<Session> {
        let session = self.load(name)?;

        if version_manager.needs_migration(&session) {
            info!(
                "Sessão precisa de migração da versão {} para {}",
                session.metadata.version,
                version_manager.registry.target_version()
            );
            version_manager.migrate(session)
        } else {
            Ok(session)
        }
    }

    fn check_migration_needed(&self, name: impl AsRef<str>) -> SessionResult<Option<u32>> {
        let session = self.load(name)?;

        if session.metadata.version != SESSION_FORMAT_VERSION {
            Ok(Some(session.metadata.version))
        } else {
            Ok(None)
        }
    }
}

/// Macro para definir migradores simples
#[macro_export]
macro_rules! define_migrator {
    ($name:ident, $from:expr, $to:expr, $desc:expr, |$data:ident| $body:block) => {
        pub struct $name;

        impl $name {
            pub fn new() -> Self {
                Self
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl VersionMigrator for $name {
            fn from_version(&self) -> u32 {
                $from
            }

            fn to_version(&self) -> u32 {
                $to
            }

            fn description(&self) -> &str {
                $desc
            }

            fn migrate(&self, $data: SessionData) -> SessionResult<SessionData> {
                $body
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_migrator() {
        let migrator = IdentityMigrator::new(1);
        assert_eq!(migrator.from_version(), 1);
        assert_eq!(migrator.to_version(), 1);

        let data = SessionData::new();
        let result = migrator.migrate(data).unwrap();
        assert_eq!(result.version, 1);
    }

    #[test]
    fn test_v0_to_v1_migration() {
        let migrator = V0ToV1Migrator::new();
        assert_eq!(migrator.from_version(), 0);
        assert_eq!(migrator.to_version(), 1);

        let data = SessionData::new();
        let result = migrator.migrate(data).unwrap();
        assert_eq!(result.version, 1);
    }

    #[test]
    fn test_migration_registry() {
        let registry = MigrationRegistry::new();

        // Deve ter migradores registrados
        assert!(registry.get_migrator(0, 1).is_some());
        assert!(registry.get_migrator(1, 2).is_some());

        // Caminho 0 -> 1 deve existir
        let path = registry.find_migration_path(0, 1);
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec![(0, 1)]);

        // Caminho 0 -> 2 deve existir (via 1)
        let path = registry.find_migration_path(0, 2);
        assert!(path.is_some());
        assert_eq!(path.unwrap(), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn test_version_manager_needs_migration() {
        let manager = VersionManager::new();

        // Sessão antiga (versão 0) precisa de migração
        let old_session = {
            let mut s = Session::new("test", SessionData::new());
            s.metadata.version = 0; // Versão antiga
            s
        };
        assert!(manager.needs_migration(&old_session));

        // Sessão na versão atual não precisa de migração
        let current_session = Session::new("test", SessionData::new());
        assert!(!manager.needs_migration(&current_session));
    }

    #[test]
    fn test_version_manager_migrate() {
        let manager = VersionManager::new();

        // Cria sessão na versão 0
        let mut session = Session::new("test", SessionData::new());
        session.metadata.version = 0;

        // Migra
        let migrated = manager.migrate(session).unwrap();
        assert_eq!(migrated.metadata.version, SESSION_FORMAT_VERSION);
        assert_eq!(migrated.data.version, SESSION_FORMAT_VERSION);
    }

    #[test]
    fn test_version_info() {
        let manager = VersionManager::new();
        let info = manager.version_info();

        assert_eq!(info.current_version, SESSION_FORMAT_VERSION);
        assert!(!info.supported_versions.is_empty());
    }

    #[test]
    fn test_supported_versions() {
        let registry = MigrationRegistry::new();
        let versions = registry.supported_versions();

        assert!(versions.contains(&0));
        assert!(versions.contains(&1));
        assert!(versions.contains(&2));
        assert!(versions.contains(&SESSION_FORMAT_VERSION));
    }

    #[test]
    fn test_is_version_supported() {
        let manager = VersionManager::new();

        assert!(manager.is_version_supported(0));
        assert!(manager.is_version_supported(1));
        assert!(manager.is_version_supported(SESSION_FORMAT_VERSION));
    }

    #[test]
    fn test_downgrade_not_supported() {
        let registry = MigrationRegistry::new();

        // Tentar fazer downgrade não deve encontrar caminho
        let path = registry.find_migration_path(2, 1);
        assert!(path.is_none());
    }
}
