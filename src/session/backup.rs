//! Sistema avançado de backup de sessões
//!
//! Este módulo fornece funcionalidades completas para gerenciamento de backups:
//! - Criação de backups automáticos e manuais
//! - Rotação de backups com políticas configuráveis
//! - Restauração de backups específicos
//! - Compressão de backups
//! - Armazenamento em múltiplas localizações
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::session::{
//!     BackupConfig, BackupManager, BackupOptions, BackupRetentionPolicy, SessionManager,
//! };
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let manager = SessionManager::default()?;
//! let config = BackupConfig::default().with_retention_policy(BackupRetentionPolicy::KeepCount(10));
//! let backup_manager = BackupManager::new(config);
//!
//! // Criar backup manual dos dados da sessão salva
//! let session_data = manager.load_data("minha-sessao")?;
//! let options = BackupOptions::default().with_description("Antes da atualização");
//! let backup = backup_manager.create_backup("minha-sessao", &session_data, options)?;
//! println!("Backup criado: {:?}", backup.path);
//!
//! // Listar backups disponíveis
//! let backups = backup_manager.list_backups("minha-sessao")?;
//!
//! // Restaurar backup específico
//! if let Some(latest) = backups.first() {
//!     let restored = backup_manager.restore_backup(latest)?;
//!     manager.save("minha-sessao", &restored)?;
//! }
//! # Ok(())
//! # }
//! ```

use super::{
    error::{SessionError, SessionResult},
    serialization::SessionData,
    Session, SessionMetadata, SESSION_FORMAT_VERSION,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Política de retenção de backups
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupRetentionPolicy {
    /// Mantém um número fixo de backups
    KeepCount(usize),
    /// Mantém backups por um período de tempo
    KeepDuration(chrono::Duration),
    /// Combinação de contagem e duração
    KeepCountAndDuration(usize, chrono::Duration),
    /// Mantém backups infinitamente
    KeepAll,
}

impl Default for BackupRetentionPolicy {
    fn default() -> Self {
        Self::KeepCount(5)
    }
}

/// Opções de criação de backup
#[derive(Debug, Clone)]
pub struct BackupOptions {
    /// Nome do backup (opcional, gera timestamp se não fornecido)
    pub name: Option<String>,
    /// Descrição do backup
    pub description: Option<String>,
    /// Comprimir o backup
    pub compress: bool,
    /// Incluir metadados adicionais
    pub include_metadata: bool,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            name: None,
            description: None,
            compress: false,
            include_metadata: true,
        }
    }
}

impl BackupOptions {
    /// Cria opções com nome específico
    pub fn with_name(name: impl Into<String>) -> Self {
        Self {
            name: Some(name.into()),
            ..Default::default()
        }
    }

    /// Define a descrição
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Habilita compressão
    pub fn with_compression(mut self) -> Self {
        self.compress = true;
        self
    }

    /// Desabilita metadados
    pub fn without_metadata(mut self) -> Self {
        self.include_metadata = false;
        self
    }
}

/// Metadados de um backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Nome original da sessão
    pub session_name: String,
    /// Nome do backup
    pub backup_name: String,
    /// Timestamp de criação
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Descrição opcional
    pub description: Option<String>,
    /// Versão do formato
    pub version: u32,
    /// Tamanho em bytes
    pub size: u64,
    /// Se está comprimido
    pub compressed: bool,
    /// Informações adicionais
    #[serde(default)]
    pub extra: HashMap<String, String>,
}

/// Informações sobre um backup
#[derive(Debug, Clone)]
pub struct BackupInfo {
    /// Metadados do backup
    pub metadata: BackupMetadata,
    /// Caminho do arquivo
    pub path: PathBuf,
    /// Caminho do arquivo de metadados (se separado)
    pub metadata_path: Option<PathBuf>,
}

/// Estatísticas de backup
#[derive(Debug, Clone, Default)]
pub struct BackupStats {
    /// Total de backups
    pub total_backups: usize,
    /// Total de sessões com backup
    pub total_sessions: usize,
    /// Espaço total usado em bytes
    pub total_size: u64,
    /// Espaço usado por sessão
    pub size_per_session: HashMap<String, u64>,
    /// Backup mais antigo
    pub oldest_backup: Option<chrono::DateTime<chrono::Utc>>,
    /// Backup mais recente
    pub newest_backup: Option<chrono::DateTime<chrono::Utc>>,
}

/// Configuração do gerenciador de backup
#[derive(Debug, Clone)]
pub struct BackupConfig {
    /// Diretório de backups
    pub backup_dir: PathBuf,
    /// Política de retenção
    pub retention_policy: BackupRetentionPolicy,
    /// Comprimir backups por padrão
    pub compress_by_default: bool,
    /// Tamanho máximo de backup não comprimido (em bytes)
    pub compress_threshold: usize,
    /// Habilitar backups automáticos
    pub auto_backup: bool,
    /// Intervalo entre backups automáticos (em segundos)
    pub auto_backup_interval: u64,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            backup_dir: default_backup_dir().unwrap_or_else(|_| PathBuf::from("backups")),
            retention_policy: BackupRetentionPolicy::default(),
            compress_by_default: false,
            compress_threshold: 1024 * 1024, // 1MB
            auto_backup: true,
            auto_backup_interval: 300, // 5 minutos
        }
    }
}

impl BackupConfig {
    /// Cria configuração com diretório específico
    pub fn with_directory(dir: impl Into<PathBuf>) -> Self {
        Self {
            backup_dir: dir.into(),
            ..Default::default()
        }
    }

    /// Define a política de retenção
    pub fn with_retention_policy(mut self, policy: BackupRetentionPolicy) -> Self {
        self.retention_policy = policy;
        self
    }
}

/// Gerenciador de backups
#[derive(Debug, Clone)]
pub struct BackupManager {
    config: BackupConfig,
}

impl BackupManager {
    /// Cria um novo gerenciador de backup
    pub fn new(config: BackupConfig) -> Self {
        Self { config }
    }

    /// Cria com configuração padrão
    pub fn default() -> SessionResult<Self> {
        Ok(Self::new(BackupConfig::default()))
    }

    /// Inicializa o diretório de backups
    pub fn initialize(&self) -> SessionResult<()> {
        if !self.config.backup_dir.exists() {
            info!("Criando diretório de backups: {:?}", self.config.backup_dir);
            fs::create_dir_all(&self.config.backup_dir).map_err(|_e| {
                SessionError::CreateDirectoryFailed(self.config.backup_dir.clone())
            })?;
        }
        Ok(())
    }

    /// Cria um backup de uma sessão
    pub fn create_backup(
        &self,
        session_name: impl AsRef<str>,
        session_data: &SessionData,
        options: BackupOptions,
    ) -> SessionResult<BackupInfo> {
        let session_name = session_name.as_ref();

        // Gera nome do backup
        let backup_name = options
            .name
            .clone()
            .unwrap_or_else(|| self.generate_backup_name(session_name));

        // Cria metadados
        let metadata = BackupMetadata {
            session_name: session_name.to_string(),
            backup_name: backup_name.clone(),
            created_at: chrono::Utc::now(),
            description: options.description,
            version: SESSION_FORMAT_VERSION,
            size: 0, // Será atualizado após salvar
            compressed: options.compress,
            extra: HashMap::new(),
        };

        // Cria a sessão para backup
        let session_metadata = SessionMetadata::new(&backup_name)
            .with_description(format!("Backup de {}", session_name));
        let session = Session {
            metadata: session_metadata,
            data: session_data.clone(),
        };

        // Determina o caminho do backup
        let backup_path = self.backup_path(&backup_name, options.compress);

        // Serializa e salva
        let json = serde_json::to_string_pretty(&session)
            .map_err(|e| SessionError::Serialization(e.to_string()))?;

        let data_to_save = if options.compress {
            // Comprime os dados
            compress_data(json.as_bytes())?
        } else {
            json.into_bytes()
        };

        fs::write(&backup_path, &data_to_save)
            .map_err(|e| SessionError::SaveFailed(e.to_string()))?;

        // Atualiza tamanho nos metadados
        let size = data_to_save.len() as u64;

        // Salva metadados separados se necessário
        let metadata_path = if options.include_metadata {
            let path = self.metadata_path(&backup_name);
            let metadata_with_size = BackupMetadata {
                size,
                ..metadata.clone()
            };
            let meta_json = serde_json::to_string_pretty(&metadata_with_size)
                .map_err(|e| SessionError::Serialization(e.to_string()))?;
            fs::write(&path, meta_json).map_err(|e| SessionError::SaveFailed(e.to_string()))?;
            Some(path)
        } else {
            None
        };

        info!(
            session = session_name,
            backup = backup_name,
            path = %backup_path.display(),
            size = size,
            "Backup criado com sucesso"
        );

        // Aplica política de retenção
        if let Err(e) = self.apply_retention_policy(session_name) {
            warn!(error = ?e, "Erro ao aplicar política de retenção");
        }

        Ok(BackupInfo {
            metadata: BackupMetadata { size, ..metadata },
            path: backup_path,
            metadata_path,
        })
    }

    /// Cria backup simples (para compatibilidade)
    pub fn backup(
        &self,
        session_name: impl AsRef<str>,
        session_data: &SessionData,
    ) -> SessionResult<BackupInfo> {
        self.create_backup(session_name, session_data, BackupOptions::default())
    }

    /// Lista todos os backups de uma sessão
    ///
    /// Com metadados, a sessão dona vem de `session_name` (e não do prefixo
    /// do arquivo: "dev_" também casava com backups da sessão "dev_x", e a
    /// retenção de "dev" apagava backups dela). Sem metadados, cai no
    /// prefixo `<sessão>_` ou no nome exato.
    pub fn list_backups(&self, session_name: impl AsRef<str>) -> SessionResult<Vec<BackupInfo>> {
        let session_name = session_name.as_ref();
        let prefix = format!("{}_", session_name);

        let backups = self
            .list_all_backups()?
            .into_iter()
            .filter(|backup| {
                if backup.metadata_path.is_some() {
                    backup.metadata.session_name == session_name
                } else {
                    let name = &backup.metadata.backup_name;
                    name.starts_with(&prefix) || name == session_name
                }
            })
            .collect();

        Ok(backups)
    }

    /// Lista todos os backups disponíveis
    pub fn list_all_backups(&self) -> SessionResult<Vec<BackupInfo>> {
        let mut backups = Vec::new();

        if !self.config.backup_dir.exists() {
            return Ok(backups);
        }

        for entry in fs::read_dir(&self.config.backup_dir).map_err(SessionError::Io)? {
            let entry = entry.map_err(SessionError::Io)?;
            let path = entry.path();

            // Ignora arquivos de metadados (.meta.json)
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if stem.ends_with(".meta") {
                        continue;
                    }
                }
            }

            if path
                .extension()
                .map(|e| e == "json" || e == "gz")
                .unwrap_or(false)
            {
                if let Ok(backup_info) = self.load_backup_info(&path) {
                    backups.push(backup_info);
                }
            }
        }

        // Ordena por data
        backups.sort_by_key(|b| std::cmp::Reverse(b.metadata.created_at));

        Ok(backups)
    }

    /// Carrega informações de um backup
    fn load_backup_info(&self, path: &Path) -> SessionResult<BackupInfo> {
        // Nome do backup sem ".json"/".json.gz". `with_extension` errava o
        // caminho dos metadados de backups comprimidos ("x.json.meta.json").
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let backup_name = file_name
            .strip_suffix(".json.gz")
            .or_else(|| file_name.strip_suffix(".json"))
            .unwrap_or(file_name)
            .to_string();
        let metadata_path = self.metadata_path(&backup_name);

        let metadata = if metadata_path.exists() {
            let content = fs::read_to_string(&metadata_path).map_err(SessionError::Io)?;
            serde_json::from_str(&content)
                .map_err(|e| SessionError::Deserialization(e.to_string()))?
        } else {
            // Extrai metadados do arquivo de backup
            let file_metadata = fs::metadata(path).ok();
            let size = file_metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            // Data do arquivo (não "agora"): ordenação e retenção dependem dela
            let created_at = file_metadata
                .and_then(|m| m.modified().ok())
                .map(chrono::DateTime::<chrono::Utc>::from)
                .unwrap_or_else(chrono::Utc::now);

            BackupMetadata {
                session_name: backup_name.clone(),
                backup_name: backup_name.clone(),
                created_at,
                description: None,
                version: SESSION_FORMAT_VERSION,
                size,
                compressed: path.extension().map(|e| e == "gz").unwrap_or(false),
                extra: HashMap::new(),
            }
        };

        Ok(BackupInfo {
            metadata,
            path: path.to_path_buf(),
            metadata_path: if metadata_path.exists() {
                Some(metadata_path)
            } else {
                None
            },
        })
    }

    /// Carrega os dados de um backup
    pub fn load_backup(&self, backup: &BackupInfo) -> SessionResult<Session> {
        let data = if backup.metadata.compressed {
            let compressed =
                fs::read(&backup.path).map_err(|e| SessionError::LoadFailed(e.to_string()))?;
            decompress_data(&compressed)?
        } else {
            fs::read_to_string(&backup.path).map_err(|e| SessionError::LoadFailed(e.to_string()))?
        };

        let session: Session = if backup.metadata.compressed {
            serde_json::from_str(&data).map_err(|e| SessionError::Deserialization(e.to_string()))?
        } else {
            serde_json::from_str(&data).map_err(|e| SessionError::Deserialization(e.to_string()))?
        };

        Ok(session)
    }

    /// Restaura um backup específico
    pub fn restore_backup(&self, backup: &BackupInfo) -> SessionResult<SessionData> {
        let session = self.load_backup(backup)?;
        info!(
            backup = backup.metadata.backup_name,
            session = backup.metadata.session_name,
            "Backup restaurado"
        );
        Ok(session.data)
    }

    /// Deleta um backup específico
    pub fn delete_backup(&self, backup: &BackupInfo) -> SessionResult<()> {
        // Deleta o arquivo de backup
        if backup.path.exists() {
            fs::remove_file(&backup.path).map_err(SessionError::Io)?;
        }

        // Deleta metadados se existirem
        if let Some(ref meta_path) = backup.metadata_path {
            if meta_path.exists() {
                fs::remove_file(meta_path).map_err(SessionError::Io)?;
            }
        }

        info!(backup = backup.metadata.backup_name, "Backup deletado");
        Ok(())
    }

    /// Deleta todos os backups de uma sessão
    pub fn delete_all_backups(&self, session_name: impl AsRef<str>) -> SessionResult<usize> {
        let backups = self.list_backups(session_name)?;
        let count = backups.len();

        for backup in backups {
            self.delete_backup(&backup)?;
        }

        info!(count = count, "Todos os backups deletados");
        Ok(count)
    }

    /// Obtém estatísticas de backup
    pub fn stats(&self) -> SessionResult<BackupStats> {
        let backups = self.list_all_backups()?;

        let total_size: u64 = backups.iter().map(|b| b.metadata.size).sum();
        let total_backups = backups.len();

        // Agrupa por sessão
        let mut size_per_session: HashMap<String, u64> = HashMap::new();
        for backup in &backups {
            *size_per_session
                .entry(backup.metadata.session_name.clone())
                .or_insert(0) += backup.metadata.size;
        }

        let oldest_backup = backups.last().map(|b| b.metadata.created_at);
        let newest_backup = backups.first().map(|b| b.metadata.created_at);

        Ok(BackupStats {
            total_backups,
            total_sessions: size_per_session.len(),
            total_size,
            size_per_session,
            oldest_backup,
            newest_backup,
        })
    }

    /// Aplica a política de retenção
    fn apply_retention_policy(&self, session_name: impl AsRef<str>) -> SessionResult<()> {
        let session_name = session_name.as_ref();

        match self.config.retention_policy {
            BackupRetentionPolicy::KeepCount(max_count) => {
                self.cleanup_by_count(session_name, max_count)?;
            }
            BackupRetentionPolicy::KeepDuration(duration) => {
                self.cleanup_by_duration(session_name, duration)?;
            }
            BackupRetentionPolicy::KeepCountAndDuration(max_count, duration) => {
                self.cleanup_by_count(session_name, max_count)?;
                self.cleanup_by_duration(session_name, duration)?;
            }
            BackupRetentionPolicy::KeepAll => {
                // Não faz nada
            }
        }

        Ok(())
    }

    /// Limpa backups mantendo apenas um número máximo
    fn cleanup_by_count(&self, session_name: &str, max_count: usize) -> SessionResult<()> {
        let mut backups = self.list_backups(session_name)?;

        if backups.len() <= max_count {
            return Ok(());
        }

        // Ordena por data (mais recente primeiro)
        backups.sort_by_key(|b| std::cmp::Reverse(b.metadata.created_at));

        // Remove os mais antigos
        for backup in backups.iter().skip(max_count) {
            debug!(
                backup = backup.metadata.backup_name,
                "Removendo backup antigo (excede contagem)"
            );
            self.delete_backup(backup)?;
        }

        Ok(())
    }

    /// Limpa backups mais antigos que uma duração
    fn cleanup_by_duration(
        &self,
        session_name: &str,
        duration: chrono::Duration,
    ) -> SessionResult<()> {
        let backups = self.list_backups(session_name)?;
        let cutoff = chrono::Utc::now() - duration;

        for backup in backups {
            if backup.metadata.created_at < cutoff {
                debug!(
                    backup = backup.metadata.backup_name,
                    created = %backup.metadata.created_at,
                    "Removendo backup antigo (excede duração)"
                );
                self.delete_backup(&backup)?;
            }
        }

        Ok(())
    }

    /// Verifica se existe backup para uma sessão
    pub fn has_backup(&self, session_name: impl AsRef<str>) -> SessionResult<bool> {
        let backups = self.list_backups(session_name)?;
        Ok(!backups.is_empty())
    }

    /// Obtém o backup mais recente de uma sessão
    pub fn get_latest_backup(
        &self,
        session_name: impl AsRef<str>,
    ) -> SessionResult<Option<BackupInfo>> {
        let backups = self.list_backups(session_name)?;
        Ok(backups.into_iter().next())
    }

    /// Obtém o backup mais antigo de uma sessão
    pub fn get_oldest_backup(
        &self,
        session_name: impl AsRef<str>,
    ) -> SessionResult<Option<BackupInfo>> {
        let mut backups = self.list_backups(session_name)?;
        Ok(backups.pop())
    }

    /// Gera um nome de backup único baseado no timestamp.
    ///
    /// O timestamp tem resolução de milissegundo: dois backups no mesmo
    /// milissegundo geravam o mesmo nome e o segundo sobrescrevia o primeiro.
    /// Se o nome já estiver em uso, acrescenta `_1`, `_2`, ...
    fn generate_backup_name(&self, session_name: &str) -> String {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S%3f");
        let base = format!("{}_{}", session_name, timestamp);
        let mut name = base.clone();
        let mut suffix = 1u32;
        while self.backup_name_in_use(&name) {
            name = format!("{}_{}", base, suffix);
            suffix += 1;
        }
        name
    }

    /// Indica se já existe backup (comprimido ou não) ou metadados com esse nome
    fn backup_name_in_use(&self, name: &str) -> bool {
        self.backup_path(name, false).exists()
            || self.backup_path(name, true).exists()
            || self.metadata_path(name).exists()
    }

    /// Retorna o caminho para um arquivo de backup
    fn backup_path(&self, name: &str, compressed: bool) -> PathBuf {
        let ext = if compressed { "json.gz" } else { "json" };
        self.config.backup_dir.join(format!("{}.{}", name, ext))
    }

    /// Retorna o caminho para o arquivo de metadados
    fn metadata_path(&self, name: &str) -> PathBuf {
        self.config.backup_dir.join(format!("{}.meta.json", name))
    }

    /// Exporta um backup para um arquivo externo
    pub fn export_backup(
        &self,
        backup: &BackupInfo,
        destination: impl AsRef<Path>,
    ) -> SessionResult<PathBuf> {
        let dest = destination.as_ref();
        let data = fs::read(&backup.path).map_err(SessionError::Io)?;

        fs::write(dest, data).map_err(SessionError::Io)?;

        info!(
            backup = backup.metadata.backup_name,
            destination = %dest.display(),
            "Backup exportado"
        );

        Ok(dest.to_path_buf())
    }

    /// Importa um backup de um arquivo externo
    pub fn import_backup(
        &self,
        source: impl AsRef<Path>,
        options: BackupOptions,
    ) -> SessionResult<BackupInfo> {
        let source = source.as_ref();

        // Lê e parseia o backup
        let data = fs::read_to_string(source).map_err(SessionError::Io)?;

        let session: Session = serde_json::from_str(&data)
            .map_err(|e| SessionError::Deserialization(e.to_string()))?;

        // Cria novo backup
        self.create_backup(&session.metadata.name, &session.data, options)
    }

    /// Retorna o diretório de backups
    pub fn backup_dir(&self) -> &Path {
        &self.config.backup_dir
    }

    /// Retorna a configuração
    pub fn config(&self) -> &BackupConfig {
        &self.config
    }
}

/// Comprime dados usando gzip
fn compress_data(data: &[u8]) -> SessionResult<Vec<u8>> {
    use std::io::Write;

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());

    encoder
        .write_all(data)
        .map_err(|e| SessionError::Serialization(format!("Erro de compressão: {}", e)))?;

    encoder
        .finish()
        .map_err(|e| SessionError::Serialization(format!("Erro ao finalizar compressão: {}", e)))
}

/// Descomprime dados gzip
fn decompress_data(data: &[u8]) -> SessionResult<String> {
    use std::io::Read;

    let mut decoder = flate2::read::GzDecoder::new(data);
    let mut result = String::new();

    decoder
        .read_to_string(&mut result)
        .map_err(|e| SessionError::Deserialization(format!("Erro de descompressão: {}", e)))?;

    Ok(result)
}

/// Retorna o diretório padrão de backups
fn default_backup_dir() -> SessionResult<PathBuf> {
    let home = dirs::home_dir().ok_or(SessionError::HomeNotFound)?;
    Ok(home.join(".config").join("code-terminal").join("backups"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_manager() -> (BackupManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config = BackupConfig::with_directory(temp_dir.path());
        let manager = BackupManager::new(config);
        manager.initialize().unwrap();
        (manager, temp_dir)
    }

    #[test]
    fn test_backup_config_default() {
        let config = BackupConfig::default();
        assert!(matches!(
            config.retention_policy,
            BackupRetentionPolicy::KeepCount(5)
        ));
        assert!(!config.compress_by_default);
        assert_eq!(config.compress_threshold, 1024 * 1024);
    }

    #[test]
    fn test_backup_options() {
        let opts = BackupOptions::with_name("test-backup")
            .with_description("Test description")
            .with_compression();

        assert_eq!(opts.name, Some("test-backup".to_string()));
        assert_eq!(opts.description, Some("Test description".to_string()));
        assert!(opts.compress);
    }

    #[test]
    fn test_create_and_list_backups() {
        let (manager, _temp) = create_test_manager();
        let data = SessionData::new();

        // Cria backup
        let backup = manager.backup("test-session", &data).unwrap();
        assert_eq!(backup.metadata.session_name, "test-session");
        assert!(backup.path.exists());

        // Lista backups
        let backups = manager.list_backups("test-session").unwrap();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn test_restore_backup() {
        let (manager, _temp) = create_test_manager();
        let mut data = SessionData::new();
        data.working_directory = Some(PathBuf::from("/test/dir"));

        // Cria e restaura backup
        let backup = manager.backup("test-session", &data).unwrap();
        let restored = manager.restore_backup(&backup).unwrap();

        assert_eq!(restored.working_directory, data.working_directory);
    }

    #[test]
    fn test_delete_backup() {
        let (manager, _temp) = create_test_manager();
        let data = SessionData::new();

        let backup = manager.backup("test-session", &data).unwrap();
        assert!(backup.path.exists());

        manager.delete_backup(&backup).unwrap();
        assert!(!backup.path.exists());
    }

    #[test]
    fn test_retention_policy_count() {
        let temp_dir = TempDir::new().unwrap();
        let config = BackupConfig::with_directory(temp_dir.path())
            .with_retention_policy(BackupRetentionPolicy::KeepCount(3));
        let manager = BackupManager::new(config);
        manager.initialize().unwrap();

        let data = SessionData::new();

        // Cria 5 backups usando nomes que incluem o session_name
        for i in 0..5 {
            let opts = BackupOptions::with_name(format!("test-session_backup-{}", i));
            manager.create_backup("test-session", &data, opts).unwrap();
        }

        // Deve manter apenas 3
        let backups = manager.list_backups("test-session").unwrap();
        assert_eq!(backups.len(), 3);
    }

    #[test]
    fn test_backup_stats() {
        let (manager, _temp) = create_test_manager();
        let data = SessionData::new();

        // Cria alguns backups
        manager.backup("session-1", &data).unwrap();
        manager.backup("session-1", &data).unwrap();
        manager.backup("session-2", &data).unwrap();

        let stats = manager.stats().unwrap();
        assert_eq!(stats.total_backups, 3);
        assert_eq!(stats.total_sessions, 2);
        assert!(stats.total_size > 0);
    }

    #[test]
    fn test_has_backup() {
        let (manager, _temp) = create_test_manager();
        let data = SessionData::new();

        assert!(!manager.has_backup("test-session").unwrap());

        manager.backup("test-session", &data).unwrap();

        assert!(manager.has_backup("test-session").unwrap());
    }

    #[test]
    fn test_get_latest_backup() {
        let (manager, _temp) = create_test_manager();
        let data = SessionData::new();

        assert!(manager.get_latest_backup("test-session").unwrap().is_none());

        let backup = manager.backup("test-session", &data).unwrap();
        let latest = manager.get_latest_backup("test-session").unwrap();

        assert!(latest.is_some());
        assert_eq!(
            latest.unwrap().metadata.backup_name,
            backup.metadata.backup_name
        );
    }

    fn data_with_size(cols: u16) -> SessionData {
        let mut data = SessionData::new();
        data.config.terminal_size = Some((cols, 24));
        data
    }

    #[test]
    fn test_compressed_backup_round_trip_keeps_metadata() {
        let (manager, _temp) = create_test_manager();
        let options = BackupOptions::with_name("proj_comprimido")
            .with_description("antes do deploy")
            .with_compression();
        let created = manager
            .create_backup("proj", &data_with_size(132), options)
            .unwrap();
        assert!(created.path.to_string_lossy().ends_with(".json.gz"));
        assert!(created.metadata.compressed);

        // Regressão: o caminho dos metadados de .json.gz era montado errado e a
        // listagem perdia descrição, sessão e data.
        let listed = manager.list_backups("proj").unwrap();
        assert_eq!(listed.len(), 1);
        let info = &listed[0];
        assert_eq!(
            info.metadata.description.as_deref(),
            Some("antes do deploy")
        );
        assert_eq!(info.metadata.session_name, "proj");
        assert!(info.metadata_path.is_some());

        let restored = manager.restore_backup(info).unwrap();
        assert_eq!(restored.config.terminal_size, Some((132, 24)));
    }

    /// Regressão: a listagem por prefixo de arquivo misturava sessões cujo
    /// nome começa igual ("dev" e "dev_api") e ignorava backups com nome
    /// próprio.
    #[test]
    fn test_list_backups_uses_session_from_metadata() {
        let (manager, _temp) = create_test_manager();
        let data = SessionData::new();
        manager.backup("dev", &data).unwrap();
        manager.backup("dev_api", &data).unwrap();
        manager
            .create_backup("dev", &data, BackupOptions::with_name("snapshot-manual"))
            .unwrap();

        let dev = manager.list_backups("dev").unwrap();
        assert_eq!(dev.len(), 2);
        assert!(dev.iter().all(|b| b.metadata.session_name == "dev"));
        assert_eq!(manager.list_backups("dev_api").unwrap().len(), 1);
        assert_eq!(manager.list_all_backups().unwrap().len(), 3);
    }

    #[test]
    fn test_backup_without_metadata_falls_back_to_file_name() {
        let (manager, _temp) = create_test_manager();
        let options = BackupOptions::with_name("solo_sem_meta").without_metadata();
        let created = manager
            .create_backup("solo", &SessionData::new(), options)
            .unwrap();
        assert!(created.metadata_path.is_none());

        let listed = manager.list_backups("solo").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].metadata.backup_name, "solo_sem_meta");
        assert!(listed[0].metadata.size > 0);
        assert!(listed[0].metadata.created_at <= chrono::Utc::now());

        // Arquivo comprimido sem metadados também é reconhecido como tal
        let gz = manager
            .create_backup(
                "solo",
                &SessionData::new(),
                BackupOptions::with_name("solo_gz")
                    .without_metadata()
                    .with_compression(),
            )
            .unwrap();
        let listed = manager.list_backups("solo").unwrap();
        let found = listed.iter().find(|b| b.path == gz.path).unwrap();
        assert!(found.metadata.compressed);
        assert!(manager.load_backup(found).is_ok());
    }

    #[test]
    fn test_delete_all_oldest_and_empty_dir() {
        let (manager, temp) = create_test_manager();
        let data = SessionData::new();
        for i in 0..3 {
            manager
                .create_backup("s", &data, BackupOptions::with_name(format!("s_{i}")))
                .unwrap();
        }
        assert!(manager.get_oldest_backup("s").unwrap().is_some());
        assert_eq!(manager.delete_all_backups("s").unwrap(), 3);
        assert!(!manager.has_backup("s").unwrap());
        assert!(manager.get_oldest_backup("s").unwrap().is_none());

        // Diretório inexistente: listas vazias, sem erro
        let missing = BackupManager::new(BackupConfig::with_directory(temp.path().join("nada")));
        assert!(missing.list_all_backups().unwrap().is_empty());
        assert!(missing.list_backups("s").unwrap().is_empty());
        assert_eq!(missing.stats().unwrap().total_backups, 0);
    }

    #[test]
    fn test_retention_by_duration_and_combined() {
        let temp = TempDir::new().unwrap();
        let config = BackupConfig::with_directory(temp.path()).with_retention_policy(
            BackupRetentionPolicy::KeepCountAndDuration(10, chrono::Duration::days(1)),
        );
        let manager = BackupManager::new(config);
        manager.initialize().unwrap();
        let data = SessionData::new();

        // Backup "antigo": metadados reescritos com data de 2 dias atrás
        let old = manager
            .create_backup("r", &data, BackupOptions::with_name("r_antigo"))
            .unwrap();
        let mut meta = old.metadata.clone();
        meta.created_at = chrono::Utc::now() - chrono::Duration::days(2);
        fs::write(
            old.metadata_path.as_ref().unwrap(),
            serde_json::to_string(&meta).unwrap(),
        )
        .unwrap();

        // O próximo backup aplica a retenção e remove o antigo
        manager
            .create_backup("r", &data, BackupOptions::with_name("r_novo"))
            .unwrap();
        let names: Vec<_> = manager
            .list_backups("r")
            .unwrap()
            .into_iter()
            .map(|b| b.metadata.backup_name)
            .collect();
        assert_eq!(names, vec!["r_novo"]);
        assert!(!old.path.exists());

        let only_duration = BackupManager::new(
            BackupConfig::with_directory(temp.path()).with_retention_policy(
                BackupRetentionPolicy::KeepDuration(chrono::Duration::days(1)),
            ),
        );
        only_duration
            .create_backup("r", &data, BackupOptions::with_name("r_mais_um"))
            .unwrap();
        assert_eq!(only_duration.list_backups("r").unwrap().len(), 2);
    }

    #[test]
    fn test_export_import_and_accessors() {
        let (manager, temp) = create_test_manager();
        let backup = manager.backup("exp", &data_with_size(100)).unwrap();
        let exported = temp.path().join("fora.json");
        assert_eq!(manager.export_backup(&backup, &exported).unwrap(), exported);

        let imported = manager
            .import_backup(&exported, BackupOptions::with_name("importado"))
            .unwrap();
        // O nome da sessão vem do arquivo importado (nome do backup original)
        assert_eq!(imported.metadata.session_name, backup.metadata.backup_name);
        let restored = manager.restore_backup(&imported).unwrap();
        assert_eq!(restored.config.terminal_size, Some((100, 24)));

        assert_eq!(manager.backup_dir(), temp.path());
        assert_eq!(manager.config().backup_dir, temp.path());

        let missing = temp.path().join("nao-existe.json");
        assert!(matches!(
            manager.import_backup(&missing, BackupOptions::default()),
            Err(SessionError::Io(_))
        ));
        fs::write(&missing, "{ quebrado").unwrap();
        assert!(matches!(
            manager.import_backup(&missing, BackupOptions::default()),
            Err(SessionError::Deserialization(_))
        ));
    }

    #[test]
    fn test_corrupted_backups_fail_to_load() {
        let (manager, _temp) = create_test_manager();
        let plain = manager.backup("c", &SessionData::new()).unwrap();
        fs::write(&plain.path, "lixo").unwrap();
        assert!(matches!(
            manager.load_backup(&plain),
            Err(SessionError::Deserialization(_))
        ));

        let gz = manager
            .create_backup(
                "c",
                &SessionData::new(),
                BackupOptions::default().with_compression(),
            )
            .unwrap();
        fs::write(&gz.path, "não é gzip").unwrap();
        assert!(matches!(
            manager.load_backup(&gz),
            Err(SessionError::Deserialization(_))
        ));

        fs::remove_file(&gz.path).unwrap();
        assert!(matches!(
            manager.load_backup(&gz),
            Err(SessionError::LoadFailed(_))
        ));
        assert!(decompress_data(b"\x1f\x8b").is_err());
        assert_eq!(
            decompress_data(&compress_data(b"ola").unwrap()).unwrap(),
            "ola"
        );
    }

    #[test]
    fn test_stats_group_size_by_session() {
        let (manager, _temp) = create_test_manager();
        manager.backup("a", &SessionData::new()).unwrap();
        manager.backup("a", &SessionData::new()).unwrap();
        manager.backup("b", &SessionData::new()).unwrap();
        let stats = manager.stats().unwrap();
        assert_eq!(stats.total_backups, 3);
        assert_eq!(stats.total_sessions, 2);
        assert_eq!(
            stats.total_size,
            stats.size_per_session.values().sum::<u64>()
        );
        assert!(stats.oldest_backup <= stats.newest_backup);
    }

    #[test]
    fn test_default_manager_uses_home_backup_dir() {
        let manager = BackupManager::default().unwrap();
        assert!(manager
            .backup_dir()
            .ends_with(std::path::Path::new(".config/code-terminal/backups")));
    }
}
