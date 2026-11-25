//! Gerenciador de chaves SSH

use crate::ssh::error::{SshKeyError, SshKeyResult};
use crate::ssh::key::{KeyType, KeyFingerprint, PublicKey, PrivateKey, SshKey};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

/// Configuração do gerenciador de chaves
#[derive(Debug, Clone)]
pub struct KeyManagerConfig {
    /// Diretório SSH (~/.ssh)
    pub ssh_dir: PathBuf,
    /// Tipos de chave para buscar automaticamente
    pub auto_detect_types: Vec<KeyType>,
    /// Verificar permissões de arquivos
    pub check_permissions: bool,
    /// Comando ssh-keygen (para geração)
    pub ssh_keygen_path: PathBuf,
}

impl Default for KeyManagerConfig {
    fn default() -> Self {
        Self {
            ssh_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".ssh"),
            auto_detect_types: vec![
                KeyType::Ed25519,
                KeyType::Rsa,
                KeyType::EcdsaP256,
            ],
            check_permissions: true,
            ssh_keygen_path: PathBuf::from("ssh-keygen"),
        }
    }
}

impl KeyManagerConfig {
    /// Cria configuração com diretório SSH customizado
    pub fn with_ssh_dir(path: PathBuf) -> Self {
        Self {
            ssh_dir: path,
            ..Self::default()
        }
    }
}

/// Gerenciador de chaves SSH
#[derive(Debug)]
pub struct KeyManager {
    config: KeyManagerConfig,
    keys: HashMap<String, SshKey>,
}

impl KeyManager {
    /// Cria um novo gerenciador de chaves
    pub fn new() -> SshKeyResult<Self> {
        Self::with_config(KeyManagerConfig::default())
    }

    /// Cria um gerenciador com configuração customizada
    pub fn with_config(config: KeyManagerConfig) -> SshKeyResult<Self> {
        let mut manager = Self {
            config,
            keys: HashMap::new(),
        };
        manager.ensure_ssh_dir()?;
        Ok(manager)
    }

    /// Garante que o diretório SSH existe
    fn ensure_ssh_dir(&self) -> SshKeyResult<()> {
        if !self.config.ssh_dir.exists() {
            std::fs::create_dir_all(&self.config.ssh_dir)
                .map_err(SshKeyError::CreateDirFailed)?;
            info!("Created SSH directory: {:?}", self.config.ssh_dir);
            
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&self.config.ssh_dir, 
                    std::fs::Permissions::from_mode(0o700))
                    .map_err(SshKeyError::WriteError)?;
            }
        }
        Ok(())
    }

    /// Lista todas as chaves SSH do diretório ~/.ssh
    pub fn list_keys(&mut self) -> SshKeyResult<Vec<SshKey>> {
        self.keys.clear();
        
        if !self.config.ssh_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&self.config.ssh_dir)
            .map_err(SshKeyError::ReadError)?;

        for entry in entries {
            let entry = entry.map_err(SshKeyError::ReadError)?;
            let path = entry.path();
            
            if let Some(key) = self.try_load_key(&path)? {
                self.keys.insert(key.name.clone(), key);
            }
        }

        Ok(self.keys.values().cloned().collect())
    }

    /// Tenta carregar uma chave de um arquivo
    fn try_load_key(&self, path: &Path) -> SshKeyResult<Option<SshKey>> {
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if filename.starts_with('.')
            || filename.ends_with(".pub")
            || filename.ends_with(".pem")
            || filename.contains("known_hosts")
            || filename.contains("authorized_keys")
            || filename.contains("config")
        {
            return Ok(None);
        }

        let key_type = self.detect_key_type(path)?;
        
        if let Some(kt) = key_type {
            let encrypted = self.is_key_encrypted(path)?;
            let public_key = self.load_public_key(&path.with_extension("pub")).ok();
            
            let name = filename.to_string();
            let mut key = SshKey::new(name, kt, path.to_path_buf())
                .with_encrypted(encrypted);

            if let Some(pk) = public_key {
                key = key.with_public_key(pk);
            }

            debug!("Loaded SSH key: {} ({})", key.name, key.key_type);
            return Ok(Some(key));
        }

        Ok(None)
    }

    /// Detecta o tipo de uma chave SSH
    fn detect_key_type(&self, path: &Path) -> SshKeyResult<Option<KeyType>> {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        if content.contains("BEGIN OPENSSH PRIVATE KEY") {
            if content.contains("ssh-ed25519") {
                return Ok(Some(KeyType::Ed25519));
            }
            if content.contains("ssh-rsa") {
                return Ok(Some(KeyType::Rsa));
            }
            if content.contains("ecdsa-sha2-nistp256") {
                return Ok(Some(KeyType::EcdsaP256));
            }
            if content.contains("ecdsa-sha2-nistp384") {
                return Ok(Some(KeyType::EcdsaP384));
            }
            if content.contains("ecdsa-sha2-nistp521") {
                return Ok(Some(KeyType::EcdsaP521));
            }
        }

        if content.contains("BEGIN RSA PRIVATE KEY") {
            return Ok(Some(KeyType::Rsa));
        }

        if content.contains("BEGIN EC PRIVATE KEY") {
            return Ok(Some(KeyType::EcdsaP256));
        }

        if content.contains("BEGIN DSA PRIVATE KEY") {
            return Ok(Some(KeyType::Dsa));
        }

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if filename.contains("id_ed25519") {
            return Ok(Some(KeyType::Ed25519));
        }
        if filename.contains("id_rsa") {
            return Ok(Some(KeyType::Rsa));
        }
        if filename.contains("id_ecdsa") {
            return Ok(Some(KeyType::EcdsaP256));
        }
        if filename.contains("id_dsa") {
            return Ok(Some(KeyType::Dsa));
        }

        Ok(None)
    }

    /// Verifica se uma chave privada está criptografada
    fn is_key_encrypted(&self, path: &Path) -> SshKeyResult<bool> {
        let content = std::fs::read_to_string(path)
            .map_err(SshKeyError::ReadError)?;

        Ok(content.contains("ENCRYPTED") || content.contains("Proc-Type: 4,ENCRYPTED"))
    }

    /// Carrega uma chave pública de um arquivo
    fn load_public_key(&self, path: &Path) -> SshKeyResult<PublicKey> {
        let content = std::fs::read_to_string(path)
            .map_err(SshKeyError::ReadError)?;

        let parts: Vec<&str> = content.trim().splitn(3, ' ').collect();
        if parts.is_empty() {
            return Err(SshKeyError::InvalidFormat("Empty public key".to_string()));
        }

        let key_data = parts[0];
        let key_type = self.detect_public_key_type(key_data)?;
        let comment = parts.get(2).map(|s| s.to_string());
        
        let fingerprint = self.calculate_fingerprint(key_data)?;

        Ok(PublicKey::new(key_type, content.trim().to_string(), comment, fingerprint))
    }

    /// Detecta o tipo de chave a partir do dado público
    fn detect_public_key_type(&self, data: &str) -> SshKeyResult<KeyType> {
        if data.starts_with("ssh-ed25519") {
            return Ok(KeyType::Ed25519);
        }
        if data.starts_with("ssh-rsa") {
            return Ok(KeyType::Rsa);
        }
        if data.starts_with("ecdsa-sha2-nistp256") {
            return Ok(KeyType::EcdsaP256);
        }
        if data.starts_with("ecdsa-sha2-nistp384") {
            return Ok(KeyType::EcdsaP384);
        }
        if data.starts_with("ecdsa-sha2-nistp521") {
            return Ok(KeyType::EcdsaP521);
        }
        if data.starts_with("ssh-dss") {
            return Ok(KeyType::Dsa);
        }
        
        Err(SshKeyError::InvalidFormat(format!("Unknown key type: {}", data)))
    }

    /// Calcula o fingerprint de uma chave
    fn calculate_fingerprint(&self, key_data: &str) -> SshKeyResult<KeyFingerprint> {
        let parts: Vec<&str> = key_data.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(SshKeyError::FingerprintFailed("Invalid key data".to_string()));
        }

        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let decoded = STANDARD
            .decode(parts[1])
            .map_err(|e| SshKeyError::FingerprintFailed(e.to_string()))?;

        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&decoded);
        let hash = hasher.finalize();

        let sha256 = STANDARD.encode(&hash)
            .trim_end_matches('=')
            .to_string();

        Ok(KeyFingerprint::from_sha256(sha256))
    }

    /// Gera uma nova chave SSH
    pub fn generate_key(
        &self,
        name: &str,
        key_type: KeyType,
        passphrase: Option<&str>,
    ) -> SshKeyResult<SshKey> {
        let key_path = self.config.ssh_dir.join(name);

        if key_path.exists() {
            return Err(SshKeyError::KeyAlreadyExists(key_path));
        }

        let mut cmd = Command::new(&self.config.ssh_keygen_path);
        cmd.arg("-t").arg(key_type.as_str().to_lowercase());
        cmd.arg("-f").arg(&key_path);
        cmd.arg("-N").arg(passphrase.unwrap_or(""));

        if key_type == KeyType::Rsa {
            cmd.arg("-b").arg("4096");
        }

        let output = cmd.output()
            .map_err(|e| SshKeyError::GenerationFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshKeyError::GenerationFailed(stderr.to_string()));
        }

        info!("Generated SSH key: {} ({})", name, key_type);

        let public_key = self.load_public_key(&key_path.with_extension("pub")).ok();
        let encrypted = passphrase.is_some();

        let mut key = SshKey::new(name.to_string(), key_type, key_path)
            .with_encrypted(encrypted);

        if let Some(pk) = public_key {
            key = key.with_public_key(pk);
        }

        Ok(key)
    }

    /// Remove uma chave SSH
    pub fn delete_key(&self, name: &str) -> SshKeyResult<()> {
        let key = self.keys.get(name)
            .ok_or_else(|| SshKeyError::KeyNotFound(PathBuf::from(name)))?;

        std::fs::remove_file(&key.private_key_path)
            .map_err(SshKeyError::WriteError)?;

        if key.public_key_path.exists() {
            std::fs::remove_file(&key.public_key_path)
                .map_err(SshKeyError::WriteError)?;
        }

        info!("Deleted SSH key: {}", name);
        Ok(())
    }

    /// Retorna uma chave pelo nome
    pub fn get_key(&self, name: &str) -> Option<&SshKey> {
        self.keys.get(name)
    }

    /// Retorna todas as chaves
    pub fn get_all_keys(&self) -> Vec<&SshKey> {
        self.keys.values().collect()
    }

    /// Exporta a chave pública para o clipboard ou arquivo
    pub fn export_public_key(&self, name: &str) -> SshKeyResult<String> {
        let key = self.keys.get(name)
            .ok_or_else(|| SshKeyError::KeyNotFound(PathBuf::from(name)))?;

        if !key.public_key_path.exists() {
            return Err(SshKeyError::KeyNotFound(key.public_key_path.clone()));
        }

        std::fs::read_to_string(&key.public_key_path)
            .map_err(SshKeyError::ReadError)
    }

    /// Valida uma chave SSH
    pub fn validate_key(&self, name: &str) -> SshKeyResult<bool> {
        let key = self.keys.get(name)
            .ok_or_else(|| SshKeyError::KeyNotFound(PathBuf::from(name)))?;

        let mut cmd = Command::new(&self.config.ssh_keygen_path);
        cmd.arg("-l").arg("-f").arg(&key.private_key_path);

        let output = cmd.output()
            .map_err(|e| SshKeyError::ValidationFailed(e.to_string()))?;

        Ok(output.status.success())
    }

    /// Muda a senha de uma chave
    pub fn change_passphrase(
        &self,
        name: &str,
        old_passphrase: Option<&str>,
        new_passphrase: Option<&str>,
    ) -> SshKeyResult<()> {
        let key = self.keys.get(name)
            .ok_or_else(|| SshKeyError::KeyNotFound(PathBuf::from(name)))?;

        let mut cmd = Command::new(&self.config.ssh_keygen_path);
        cmd.arg("-p").arg("-f").arg(&key.private_key_path);

        if let Some(old) = old_passphrase {
            cmd.arg("-P").arg(old);
        }

        if let Some(new) = new_passphrase {
            cmd.arg("-N").arg(new);
        }

        let output = cmd.output()
            .map_err(|e| SshKeyError::GenerationFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshKeyError::InvalidPassphrase);
        }

        info!("Changed passphrase for key: {}", name);
        Ok(())
    }

    /// Verifica as permissões do arquivo de chave
    #[cfg(unix)]
    pub fn check_key_permissions(&self, name: &str) -> SshKeyResult<bool> {
        use std::os::unix::fs::PermissionsExt;
        
        let key = self.keys.get(name)
            .ok_or_else(|| SshKeyError::KeyNotFound(PathBuf::from(name)))?;

        let metadata = std::fs::metadata(&key.private_key_path)
            .map_err(SshKeyError::ReadError)?;

        let mode = metadata.permissions().mode();
        let perms = mode & 0o777;

        Ok(perms == 0o600)
    }

    #[cfg(windows)]
    pub fn check_key_permissions(&self, _name: &str) -> SshKeyResult<bool> {
        Ok(true)
    }

    /// Corrige as permissões do arquivo de chave
    #[cfg(unix)]
    pub fn fix_key_permissions(&self, name: &str) -> SshKeyResult<()> {
        use std::os::unix::fs::PermissionsExt;

        let key = self.keys.get(name)
            .ok_or_else(|| SshKeyError::KeyNotFound(PathBuf::from(name)))?;

        std::fs::set_permissions(
            &key.private_key_path,
            std::fs::Permissions::from_mode(0o600),
        ).map_err(SshKeyError::WriteError)?;

        info!("Fixed permissions for key: {}", name);
        Ok(())
    }

    #[cfg(windows)]
    pub fn fix_key_permissions(&self, _name: &str) -> SshKeyResult<()> {
        Ok(())
    }
}

impl Default for KeyManager {
    fn default() -> Self {
        Self::new().expect("Failed to create KeyManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_manager_config_default() {
        let config = KeyManagerConfig::default();
        assert!(config.ssh_dir.ends_with(".ssh"));
        assert!(config.check_permissions);
    }

    #[test]
    fn test_key_manager_creation() {
        let manager = KeyManager::new();
        assert!(manager.is_ok());
    }

    #[test]
    fn test_list_keys_empty() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = KeyManagerConfig::with_ssh_dir(temp_dir.path().to_path_buf());
        let mut manager = KeyManager::with_config(config).unwrap();
        
        let keys = manager.list_keys().unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_detect_key_type_from_filename() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = KeyManagerConfig::with_ssh_dir(temp_dir.path().to_path_buf());
        let manager = KeyManager::with_config(config).unwrap();

        let rsa_path = temp_dir.path().join("id_rsa");
        std::fs::write(&rsa_path, "").unwrap();
        
        let key_type = manager.detect_key_type(&rsa_path).unwrap();
        assert_eq!(key_type, Some(KeyType::Rsa));
    }
}
