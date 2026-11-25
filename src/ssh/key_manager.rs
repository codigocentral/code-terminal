//! Gerenciador de chaves SSH
//!
//! Este módulo implementa o `KeyManager` responsável por descobrir, carregar,
//! gerar e gerenciar chaves SSH com integração ao SSH agent.
//!
//! # Exemplos
//!
//! ## Descobrir chaves existentes
//!
//! ```rust,no_run
//! use code_terminal::ssh::KeyManager;
//!
//! let mut manager = KeyManager::new();
//! let keys = manager.discover_keys().unwrap();
//!
//! for key in keys {
//!     println!("Chave: {} ({:?})", key.name, key.key_type);
//! }
//! ```
//!
//! ## Gerar nova chave Ed25519
//!
//! ```rust,no_run
//! use code_terminal::ssh::{KeyManager, KeyGenerationOptions, KeyType};
//!
//! let manager = KeyManager::new();
//! let options = KeyGenerationOptions::new(KeyType::Ed25519)
//!     .with_comment("usuario@exemplo.com".to_string());
//!
//! let key = manager.generate_key(options).unwrap();
//! println!("Chave gerada: {:?}", key.private_key_path);
//! ```
//!
//! ## Adicionar ao SSH agent
//!
//! ```rust,no_run
//! use code_terminal::ssh::KeyManager;
//!
//! let mut manager = KeyManager::new();
//! let keys = manager.discover_keys().unwrap();
//!
//! if let Some(key) = keys.first() {
//!     manager.add_to_agent(key, Some("passphrase")).unwrap();
//! }
//! ```

use crate::ssh::error::{SshKeyError, SshKeyResult};
use crate::ssh::key::{KeyFingerprint, KeyType, PublicKey, SshKey};
use std::collections::HashMap;
use std::fs;

use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

const SSH_DIR: &str = ".ssh";
const KNOWN_KEY_PREFIXES: &[&str] = &["id_rsa", "id_ed25519", "id_ecdsa", "id_dsa"];

#[derive(Debug, Clone)]
pub struct KeyGenerationOptions {
    pub key_type: KeyType,
    pub bits: Option<usize>,
    pub passphrase: Option<String>,
    pub comment: Option<String>,
    pub filename: Option<String>,
    pub overwrite: bool,
}

impl Default for KeyGenerationOptions {
    fn default() -> Self {
        Self {
            key_type: KeyType::Ed25519,
            bits: None,
            passphrase: None,
            comment: None,
            filename: None,
            overwrite: false,
        }
    }
}

impl KeyGenerationOptions {
    pub fn new(key_type: KeyType) -> Self {
        Self {
            key_type,
            ..Default::default()
        }
    }

    pub fn with_bits(mut self, bits: usize) -> Self {
        self.bits = Some(bits);
        self
    }

    pub fn with_passphrase(mut self, passphrase: String) -> Self {
        self.passphrase = Some(passphrase);
        self
    }

    pub fn with_comment(mut self, comment: String) -> Self {
        self.comment = Some(comment);
        self
    }

    pub fn with_filename(mut self, filename: String) -> Self {
        self.filename = Some(filename);
        self
    }

    pub fn with_overwrite(mut self, overwrite: bool) -> Self {
        self.overwrite = overwrite;
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct KeyManager {
    ssh_dir: Option<PathBuf>,
    keys: HashMap<String, SshKey>,
    agent_keys: Vec<String>,
    keygen_program: String,
    agent_program: String,
}

impl KeyManager {
    pub fn new() -> Self {
        Self {
            ssh_dir: None,
            keys: HashMap::new(),
            agent_keys: Vec::new(),
            keygen_program: "ssh-keygen".to_string(),
            agent_program: "ssh-add".to_string(),
        }
    }

    pub fn with_ssh_dir(path: PathBuf) -> Self {
        Self {
            ssh_dir: Some(path),
            keys: HashMap::new(),
            agent_keys: Vec::new(),
            keygen_program: "ssh-keygen".to_string(),
            agent_program: "ssh-add".to_string(),
        }
    }

    /// Substitui os executáveis `ssh-keygen` e `ssh-add` (útil para ambientes customizados e testes).
    pub fn with_programs(mut self, keygen: impl Into<String>, agent: impl Into<String>) -> Self {
        self.keygen_program = keygen.into();
        self.agent_program = agent.into();
        self
    }

    pub fn ssh_dir(&self) -> SshKeyResult<PathBuf> {
        if let Some(dir) = &self.ssh_dir {
            return Ok(dir.clone());
        }

        dirs::home_dir()
            .map(|h| h.join(SSH_DIR))
            .ok_or(SshKeyError::SshDirNotFound)
    }

    pub fn ensure_ssh_dir(&self) -> SshKeyResult<PathBuf> {
        let ssh_dir = self.ssh_dir()?;

        if !ssh_dir.exists() {
            fs::create_dir_all(&ssh_dir).map_err(SshKeyError::CreateDirFailed)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700))
                    .map_err(SshKeyError::CreateDirFailed)?;
            }

            info!("Created SSH directory: {:?}", ssh_dir);
        }

        Ok(ssh_dir)
    }

    pub fn discover_keys(&mut self) -> SshKeyResult<Vec<SshKey>> {
        let ssh_dir = self.ssh_dir()?;

        if !ssh_dir.exists() {
            debug!("SSH directory does not exist: {:?}", ssh_dir);
            return Ok(Vec::new());
        }

        let mut discovered = Vec::new();

        for entry in fs::read_dir(&ssh_dir).map_err(SshKeyError::ReadError)? {
            let entry = entry.map_err(SshKeyError::ReadError)?;
            let path = entry.path();

            let filename = entry.file_name().to_string_lossy().to_string();
            if is_private_key_file(&filename) {
                match self.load_key(&path) {
                    Ok(key) => {
                        self.keys.insert(key.name.clone(), key.clone());
                        discovered.push(key);
                    }
                    Err(e) => {
                        warn!("Failed to load key {:?}: {}", path, e);
                    }
                }
            }
        }

        debug!("Discovered {} SSH keys", discovered.len());
        Ok(discovered)
    }

    pub fn load_key(&self, path: &Path) -> SshKeyResult<SshKey> {
        if !path.exists() {
            return Err(SshKeyError::KeyNotFound(path.to_path_buf()));
        }

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let key_type = infer_key_type(&filename);
        let mut key = SshKey::new(filename.clone(), key_type, path.to_path_buf());

        let content = fs::read_to_string(path).map_err(SshKeyError::ReadError)?;

        key = key.with_encrypted(is_encrypted(&content));

        if let Some(bits) = extract_key_bits(&content) {
            key = key.with_bits(bits);
        }

        let pub_path = path.with_extension("pub");
        if pub_path.exists() {
            if let Ok(public_key) = self.load_public_key(&pub_path) {
                key = key.with_public_key(public_key);
            }
        }

        info!("Loaded SSH key: {} ({})", key.name, key.key_type);
        Ok(key)
    }

    fn load_public_key(&self, path: &Path) -> SshKeyResult<PublicKey> {
        let content = fs::read_to_string(path).map_err(SshKeyError::ReadError)?;

        parse_public_key(&content)
    }

    pub fn get_key(&self, name: &str) -> Option<&SshKey> {
        self.keys.get(name)
    }

    pub fn get_keys(&self) -> Vec<&SshKey> {
        self.keys.values().collect()
    }

    pub fn get_keys_by_type(&self, key_type: KeyType) -> Vec<&SshKey> {
        self.keys
            .values()
            .filter(|k| k.key_type == key_type)
            .collect()
    }

    pub fn get_default_key(&self) -> Option<&SshKey> {
        for preferred_type in [KeyType::Ed25519, KeyType::Rsa, KeyType::EcdsaP256] {
            if let Some(key) = self.keys.values().find(|k| k.key_type == preferred_type) {
                return Some(key);
            }
        }
        self.keys.values().next()
    }

    pub fn generate_key(&self, options: KeyGenerationOptions) -> SshKeyResult<SshKey> {
        let ssh_dir = self.ensure_ssh_dir()?;

        let filename = options
            .filename
            .unwrap_or_else(|| options.key_type.default_filename().to_string());

        let key_path = ssh_dir.join(&filename);

        if key_path.exists() && !options.overwrite {
            return Err(SshKeyError::KeyAlreadyExists(key_path));
        }

        let mut cmd = Command::new(&self.keygen_program);

        cmd.arg("-t").arg(options.key_type.as_str().to_lowercase());
        cmd.arg("-f").arg(&key_path);
        cmd.arg("-N")
            .arg(options.passphrase.as_deref().unwrap_or(""));

        if let Some(bits) = options.bits {
            if matches!(options.key_type, KeyType::Rsa | KeyType::Dsa) {
                cmd.arg("-b").arg(bits.to_string());
            }
        }

        if let Some(comment) = &options.comment {
            cmd.arg("-C").arg(comment);
        }

        let output = cmd
            .output()
            .map_err(|e| SshKeyError::GenerationFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshKeyError::GenerationFailed(stderr.to_string()));
        }

        info!("Generated SSH key: {:?}", key_path);
        self.load_key(&key_path)
    }

    pub fn delete_key(&mut self, key: &SshKey) -> SshKeyResult<()> {
        if key.private_key_path.exists() {
            fs::remove_file(&key.private_key_path).map_err(SshKeyError::WriteError)?;
            info!("Deleted private key: {:?}", key.private_key_path);
        }

        if key.public_key_path.exists() {
            fs::remove_file(&key.public_key_path).map_err(SshKeyError::WriteError)?;
            info!("Deleted public key: {:?}", key.public_key_path);
        }

        self.keys.remove(&key.name);
        Ok(())
    }

    pub fn export_public_key(&self, key: &SshKey) -> SshKeyResult<String> {
        key.public_key
            .as_ref()
            .map(|pk| pk.to_authorized_keys_line())
            .ok_or_else(|| SshKeyError::KeyNotFound(key.public_key_path.clone()))
    }

    pub fn add_to_agent(&mut self, key: &SshKey, passphrase: Option<&str>) -> SshKeyResult<()> {
        let mut cmd = Command::new(&self.agent_program);

        if let Some(_pass) = passphrase {
            // SSH_ASKPASS setup for passphrase - requires DISPLAY and SSH_ASKPASS
            cmd.env("SSH_ASKPASS", "/bin/sh");
            cmd.env("DISPLAY", ":0");
            // Note: Passphrase handling via SSH_ASKPASS is a simplified approach
            // In production, a custom askpass script would be needed
        }

        cmd.arg(&key.private_key_path);

        let output = cmd
            .output()
            .map_err(|e| SshKeyError::AgentAddFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshKeyError::AgentAddFailed(stderr.to_string()));
        }

        info!("Added key to SSH agent: {:?}", key.private_key_path);

        if let Some(k) = self.keys.get_mut(&key.name) {
            k.in_agent = true;
        }

        Ok(())
    }

    pub fn remove_from_agent(&mut self, key: &SshKey) -> SshKeyResult<()> {
        let output = Command::new(&self.agent_program)
            .arg("-d")
            .arg(&key.private_key_path)
            .output()
            .map_err(|e| SshKeyError::AgentRemoveFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshKeyError::AgentRemoveFailed(stderr.to_string()));
        }

        info!("Removed key from SSH agent: {:?}", key.private_key_path);

        if let Some(k) = self.keys.get_mut(&key.name) {
            k.in_agent = false;
        }

        Ok(())
    }

    pub fn list_agent_keys(&mut self) -> SshKeyResult<Vec<String>> {
        let output = Command::new(&self.agent_program)
            .arg("-l")
            .output()
            .map_err(|e| SshKeyError::AgentListFailed(e.to_string()))?;

        if !output.status.success() {
            self.agent_keys.clear();
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let keys: Vec<String> = stdout
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split_whitespace().collect();
                parts.last().map(|s| s.to_string())
            })
            .collect();

        self.agent_keys = keys.clone();
        Ok(keys)
    }

    pub fn is_key_in_agent(&self, key: &SshKey) -> bool {
        key.in_agent
            || self.agent_keys.iter().any(|k| {
                k.contains(&key.name)
                    || key
                        .fingerprint
                        .as_ref()
                        .map(|fp| k.contains(&fp.sha256))
                        .unwrap_or(false)
            })
    }

    pub fn clear_agent(&mut self) -> SshKeyResult<()> {
        let output = Command::new(&self.agent_program)
            .arg("-D")
            .output()
            .map_err(|e| SshKeyError::AgentRemoveFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshKeyError::AgentRemoveFailed(stderr.to_string()));
        }

        info!("Cleared all keys from SSH agent");

        for key in self.keys.values_mut() {
            key.in_agent = false;
        }
        self.agent_keys.clear();

        Ok(())
    }

    #[cfg_attr(not(unix), allow(unused_variables))]
    pub fn validate_key_permissions(&self, key: &SshKey) -> SshKeyResult<bool> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&key.private_key_path).map_err(SshKeyError::ReadError)?;
            let mode = metadata.permissions().mode();

            if mode & 0o077 != 0 {
                return Err(SshKeyError::InvalidPermissions);
            }

            Ok(true)
        }

        #[cfg(not(unix))]
        {
            Ok(true)
        }
    }

    #[cfg_attr(not(unix), allow(unused_variables))]
    pub fn fix_key_permissions(&self, key: &SshKey) -> SshKeyResult<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key.private_key_path, fs::Permissions::from_mode(0o600))
                .map_err(SshKeyError::WriteError)?;

            if key.public_key_path.exists() {
                fs::set_permissions(&key.public_key_path, fs::Permissions::from_mode(0o644))
                    .map_err(SshKeyError::WriteError)?;
            }

            info!("Fixed permissions for key: {:?}", key.private_key_path);
        }

        Ok(())
    }
}

fn is_private_key_file(filename: &str) -> bool {
    !filename.ends_with(".pub")
        && !filename.ends_with(".pem")
        && !filename.ends_with(".ppk")
        && (filename.starts_with("id_")
            || filename.contains("_key")
            || KNOWN_KEY_PREFIXES
                .iter()
                .any(|prefix| filename.starts_with(prefix)))
}

fn infer_key_type(filename: &str) -> KeyType {
    if filename.contains("rsa") {
        KeyType::Rsa
    } else if filename.contains("ed25519") {
        KeyType::Ed25519
    } else if filename.contains("ecdsa") {
        KeyType::EcdsaP256
    } else if filename.contains("dsa") {
        KeyType::Dsa
    } else {
        KeyType::Ed25519
    }
}

fn is_encrypted(content: &str) -> bool {
    content.contains("ENCRYPTED") || content.contains("Proc-Type: 4,ENCRYPTED")
}

fn extract_key_bits(content: &str) -> Option<usize> {
    if content.contains("BEGIN RSA PRIVATE KEY") {
        return Some(4096);
    }
    if content.contains("BEGIN OPENSSH PRIVATE KEY") {
        return Some(256);
    }
    None
}

fn parse_public_key(content: &str) -> SshKeyResult<PublicKey> {
    let content = content.trim();
    let parts: Vec<&str> = content.split_whitespace().collect();

    if parts.len() < 2 {
        return Err(SshKeyError::InvalidFormat(
            "Invalid public key format".to_string(),
        ));
    }

    let key_type_str = parts[0];
    let key_data = parts[1];
    let comment = parts.get(2).map(|s| s.to_string());

    let key_type = KeyType::from_str(key_type_str)
        .ok_or_else(|| SshKeyError::UnsupportedType(key_type_str.to_string()))?;

    let full_key = format!("{} {}", key_type_str, key_data);
    let fingerprint = calculate_fingerprint(&full_key)?;

    Ok(PublicKey::new(key_type, full_key, comment, fingerprint))
}

fn calculate_fingerprint(public_key: &str) -> SshKeyResult<KeyFingerprint> {
    let parts: Vec<&str> = public_key.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(SshKeyError::FingerprintFailed(
            "Invalid key format".to_string(),
        ));
    }

    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use sha2::{Digest, Sha256};

    let key_data = STANDARD
        .decode(parts[1])
        .map_err(|e| SshKeyError::FingerprintFailed(e.to_string()))?;

    let mut hasher = Sha256::new();
    hasher.update(&key_data);
    let hash = hasher.finalize();

    let sha256_fingerprint = base64::engine::general_purpose::STANDARD_NO_PAD.encode(hash);

    Ok(KeyFingerprint::from_sha256(sha256_fingerprint))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn test_key_manager_creation() {
        let manager = KeyManager::new();
        assert!(manager.ssh_dir.is_none());
        assert!(manager.get_keys().is_empty());
    }

    #[test]
    fn test_key_generation_options_default() {
        let opts = KeyGenerationOptions::default();
        assert_eq!(opts.key_type, KeyType::Ed25519);
        assert!(opts.bits.is_none());
        assert!(opts.passphrase.is_none());
    }

    #[test]
    fn test_key_generation_options_builder() {
        let opts = KeyGenerationOptions::new(KeyType::Rsa)
            .with_bits(4096)
            .with_comment("test@example.com".to_string());

        assert_eq!(opts.key_type, KeyType::Rsa);
        assert_eq!(opts.bits, Some(4096));
        assert_eq!(opts.comment, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_is_private_key_file() {
        assert!(is_private_key_file("id_rsa"));
        assert!(is_private_key_file("id_ed25519"));
        assert!(is_private_key_file("id_ecdsa"));
        assert!(!is_private_key_file("id_rsa.pub"));
        assert!(!is_private_key_file("id_rsa.pem"));
        assert!(!is_private_key_file("id_rsa.ppk"));
    }

    #[test]
    fn test_infer_key_type() {
        assert_eq!(infer_key_type("id_rsa"), KeyType::Rsa);
        assert_eq!(infer_key_type("id_ed25519"), KeyType::Ed25519);
        assert_eq!(infer_key_type("my_ecdsa_key"), KeyType::EcdsaP256);
        assert_eq!(infer_key_type("id_dsa"), KeyType::Dsa);
    }

    #[test]
    fn test_is_encrypted() {
        let encrypted = "-----BEGIN RSA PRIVATE KEY-----
Proc-Type: 4,ENCRYPTED
DEK-Info: AES-256-CBC";
        assert!(is_encrypted(encrypted));

        let not_encrypted = "-----BEGIN RSA PRIVATE KEY-----
MIIEpAIBAAKCAQEA";
        assert!(!is_encrypted(not_encrypted));
    }

    #[test]
    fn test_get_default_key_empty() {
        let manager = KeyManager::new();
        assert!(manager.get_default_key().is_none());
    }

    #[test]
    fn test_get_keys_by_type_empty() {
        let manager = KeyManager::new();
        let rsa_keys = manager.get_keys_by_type(KeyType::Rsa);
        assert!(rsa_keys.is_empty());
    }
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    const PUB: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGq0 user@host\n";

    fn script(dir: &TempDir, name: &str, body: &str) -> String {
        crate::test_exec::write_fake_executable(&dir.path().join(name), body)
    }

    fn manager_with(dir: &TempDir, keygen: &str, agent: &str) -> KeyManager {
        KeyManager::with_ssh_dir(dir.path().join("ssh")).with_programs(keygen, agent)
    }

    fn sample_key(dir: &TempDir, name: &str) -> SshKey {
        let path = dir.path().join(name);
        fs::write(&path, "k").unwrap();
        SshKey::new(name.to_string(), KeyType::Ed25519, path)
    }

    #[test]
    fn test_ssh_dir_explicit_and_default() {
        let m = KeyManager::with_ssh_dir(PathBuf::from("/x/y"));
        assert_eq!(m.ssh_dir().unwrap(), PathBuf::from("/x/y"));
        let d = KeyManager::new().ssh_dir();
        assert!(d.map(|p| p.ends_with(".ssh")).unwrap_or(true));
    }

    #[test]
    fn test_ensure_ssh_dir_creates_with_0700() {
        let t = TempDir::new().unwrap();
        let m = manager_with(&t, "x", "y");
        let dir = m.ensure_ssh_dir().unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(m.ensure_ssh_dir().unwrap(), dir);
    }

    #[test]
    fn test_ensure_ssh_dir_failure() {
        let t = TempDir::new().unwrap();
        let blocker = t.path().join("f");
        fs::write(&blocker, "").unwrap();
        let m = KeyManager::with_ssh_dir(blocker.join("sub"));
        assert!(matches!(
            m.ensure_ssh_dir(),
            Err(SshKeyError::CreateDirFailed(_))
        ));
    }

    #[test]
    fn test_discover_keys_variants() {
        let t = TempDir::new().unwrap();
        let mut m = manager_with(&t, "x", "y");
        assert!(m.discover_keys().unwrap().is_empty());

        let dir = m.ensure_ssh_dir().unwrap();
        fs::write(
            dir.join("id_rsa"),
            "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\n",
        )
        .unwrap();
        fs::write(
            dir.join("id_ed25519"),
            "-----BEGIN OPENSSH PRIVATE KEY-----\n",
        )
        .unwrap();
        fs::write(dir.join("id_ed25519.pub"), PUB).unwrap();
        fs::write(dir.join("id_bad"), [0xff, 0xfe, 0xfd]).unwrap();
        fs::write(dir.join("config"), "x").unwrap();
        let mut keys = m.discover_keys().unwrap();
        keys.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<_> = keys.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, vec!["id_ed25519", "id_rsa"]);
        assert!(keys[0].public_key.is_some());
        assert!(keys[1].public_key.is_none());
        assert!(m.get_key("id_rsa").is_some());
        assert_eq!(m.get_keys().len(), 2);
        assert_eq!(m.get_keys_by_type(KeyType::Rsa).len(), 1);
        assert_eq!(m.get_default_key().unwrap().key_type, KeyType::Ed25519);
        assert_eq!(
            export_line(&m, &keys[0]),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGq0 user@host"
        );
        assert!(matches!(
            m.export_public_key(&keys[1]),
            Err(SshKeyError::KeyNotFound(_))
        ));
    }

    fn export_line(m: &KeyManager, k: &SshKey) -> String {
        m.export_public_key(k).unwrap()
    }

    #[test]
    fn test_discover_keys_read_dir_error() {
        let t = TempDir::new().unwrap();
        let f = t.path().join("file");
        fs::write(&f, "").unwrap();
        let mut m = KeyManager::with_ssh_dir(f);
        assert!(matches!(m.discover_keys(), Err(SshKeyError::ReadError(_))));
    }

    #[test]
    fn test_load_key_errors_and_bad_pub() {
        let t = TempDir::new().unwrap();
        let m = manager_with(&t, "x", "y");
        assert!(matches!(
            m.load_key(&t.path().join("nope")),
            Err(SshKeyError::KeyNotFound(_))
        ));
        let dir_as_key = t.path().join("id_dir");
        fs::create_dir(&dir_as_key).unwrap();
        assert!(matches!(
            m.load_key(&dir_as_key),
            Err(SshKeyError::ReadError(_))
        ));
        let k = t.path().join("id_rsa");
        fs::write(&k, "x").unwrap();
        fs::write(t.path().join("id_rsa.pub"), "bad").unwrap();
        assert!(m.load_key(&k).unwrap().public_key.is_none());
    }

    #[test]
    fn test_get_default_key_fallbacks() {
        let t = TempDir::new().unwrap();
        let mut m = KeyManager::new();
        let put = |m: &mut KeyManager, name: &str, ty: KeyType| {
            let k = SshKey::new(name.to_string(), ty, t.path().join(name));
            m.keys.insert(name.to_string(), k);
        };
        put(&mut m, "d", KeyType::Dsa);
        assert_eq!(m.get_default_key().unwrap().name, "d");
        put(&mut m, "e", KeyType::EcdsaP256);
        assert_eq!(m.get_default_key().unwrap().name, "e");
        put(&mut m, "r", KeyType::Rsa);
        assert_eq!(m.get_default_key().unwrap().name, "r");
    }

    #[test]
    fn test_generate_key_paths() {
        let t = TempDir::new().unwrap();
        let ok = script(&t, "kg_ok", "touch \"$4\"");
        let m = manager_with(&t, &ok, "y");
        let k = m
            .generate_key(
                KeyGenerationOptions::new(KeyType::Rsa)
                    .with_bits(2048)
                    .with_comment("c".into())
                    .with_passphrase("p".into())
                    .with_filename("custom".into()),
            )
            .unwrap();
        assert_eq!(k.name, "custom");
        assert!(matches!(
            m.generate_key(KeyGenerationOptions::new(KeyType::Rsa).with_filename("custom".into())),
            Err(SshKeyError::KeyAlreadyExists(_))
        ));
        let again = m
            .generate_key(
                KeyGenerationOptions::new(KeyType::Ed25519)
                    .with_bits(1)
                    .with_filename("custom".into())
                    .with_overwrite(true),
            )
            .unwrap();
        assert_eq!(again.name, "custom");
        assert!(m.generate_key(KeyGenerationOptions::default()).is_ok());

        let bad = script(&t, "kg_bad", "echo boom >&2; exit 1");
        let m2 = manager_with(&t, &bad, "y");
        let r = m2.generate_key(
            KeyGenerationOptions::new(KeyType::Dsa)
                .with_bits(1024)
                .with_filename("z".into()),
        );
        assert!(matches!(r, Err(SshKeyError::GenerationFailed(ref m)) if m.contains("boom")));
        let m3 = manager_with(&t, "/nonexistent/ssh-keygen", "y");
        assert!(matches!(
            m3.generate_key(KeyGenerationOptions::new(KeyType::Rsa).with_filename("n3".into())),
            Err(SshKeyError::GenerationFailed(_))
        ));
        let m4 = KeyManager::with_ssh_dir(PathBuf::from("/proc/nope/x")).with_programs(&ok, "y");
        let _ = m4.generate_key(KeyGenerationOptions::default());
    }

    #[test]
    fn test_delete_key() {
        let t = TempDir::new().unwrap();
        let mut m = manager_with(&t, "x", "y");
        let key = sample_key(&t, "id_x");
        fs::write(&key.public_key_path, PUB).unwrap();
        m.keys.insert(key.name.clone(), key.clone());
        m.delete_key(&key).unwrap();
        assert!(!key.private_key_path.exists());
        assert!(!key.public_key_path.exists());
        assert!(m.get_key("id_x").is_none());
        m.delete_key(&key).unwrap();
    }

    #[test]
    fn test_delete_key_write_error() {
        let t = TempDir::new().unwrap();
        let mut m = manager_with(&t, "x", "y");
        let d = t.path().join("dir_key");
        fs::create_dir(&d).unwrap();
        fs::write(d.join("inner"), "").unwrap();
        let key = SshKey::new("dir_key".into(), KeyType::Rsa, d.clone());
        assert!(matches!(
            m.delete_key(&key),
            Err(SshKeyError::WriteError(_))
        ));
        let mut key2 = sample_key(&t, "id_y");
        let pubdir = t.path().join("pubdir");
        fs::create_dir(&pubdir).unwrap();
        fs::write(pubdir.join("inner"), "").unwrap();
        key2.public_key_path = pubdir;
        assert!(matches!(
            m.delete_key(&key2),
            Err(SshKeyError::WriteError(_))
        ));
    }

    #[test]
    fn test_agent_add_remove_list_clear() {
        let t = TempDir::new().unwrap();
        let ok = script(
            &t,
            "add_ok",
            "echo \"256 SHA256:abc comment (ED25519)\"; echo \"256 SHA256:def id_x (ED25519)\"",
        );
        let mut m = manager_with(&t, "x", &ok);
        let key = sample_key(&t, "id_x");
        m.keys.insert(key.name.clone(), key.clone());

        m.add_to_agent(&key, Some("pw")).unwrap();
        assert!(m.get_key("id_x").unwrap().in_agent);
        m.remove_from_agent(&key).unwrap();
        assert!(!m.get_key("id_x").unwrap().in_agent);

        let keys = m.list_agent_keys().unwrap();
        assert_eq!(keys, vec!["(ED25519)", "(ED25519)"]);
        assert!(!m.is_key_in_agent(&key));
        m.agent_keys = vec!["xx id_x yy".into()];
        assert!(m.is_key_in_agent(&key));
        let mut fp_key = sample_key(&t, "other");
        fp_key.fingerprint = Some(KeyFingerprint::from_sha256("SHA256:zz".into()));
        assert!(!m.is_key_in_agent(&fp_key));
        m.agent_keys = vec!["SHA256:zz".into()];
        assert!(m.is_key_in_agent(&fp_key));
        assert!(m.is_key_in_agent(&sample_key(&t, "q").with_in_agent(true)));

        m.keys.get_mut("id_x").unwrap().in_agent = true;
        m.clear_agent().unwrap();
        assert!(!m.get_key("id_x").unwrap().in_agent);
        assert!(m.agent_keys.is_empty());
    }

    #[test]
    fn test_agent_failures() {
        let t = TempDir::new().unwrap();
        let bad = script(&t, "add_bad", "echo nope >&2; exit 1");
        let mut m = manager_with(&t, "x", &bad);
        let key = sample_key(&t, "id_x");
        assert!(matches!(
            m.add_to_agent(&key, None),
            Err(SshKeyError::AgentAddFailed(_))
        ));
        assert!(matches!(
            m.remove_from_agent(&key),
            Err(SshKeyError::AgentRemoveFailed(_))
        ));
        m.agent_keys = vec!["a".into()];
        assert!(m.list_agent_keys().unwrap().is_empty());
        assert!(m.agent_keys.is_empty());
        assert!(matches!(
            m.clear_agent(),
            Err(SshKeyError::AgentRemoveFailed(_))
        ));

        let mut m2 = manager_with(&t, "x", "/nonexistent/ssh-add");
        assert!(matches!(
            m2.add_to_agent(&key, None),
            Err(SshKeyError::AgentAddFailed(_))
        ));
        assert!(matches!(
            m2.remove_from_agent(&key),
            Err(SshKeyError::AgentRemoveFailed(_))
        ));
        assert!(matches!(
            m2.list_agent_keys(),
            Err(SshKeyError::AgentListFailed(_))
        ));
        assert!(matches!(
            m2.clear_agent(),
            Err(SshKeyError::AgentRemoveFailed(_))
        ));
    }

    #[test]
    fn test_permissions() {
        let t = TempDir::new().unwrap();
        let m = manager_with(&t, "x", "y");
        let key = sample_key(&t, "id_p");
        fs::write(&key.public_key_path, PUB).unwrap();
        fs::set_permissions(&key.private_key_path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            m.validate_key_permissions(&key),
            Err(SshKeyError::InvalidPermissions)
        ));
        m.fix_key_permissions(&key).unwrap();
        assert!(m.validate_key_permissions(&key).unwrap());
        let mut foreign = sample_key(&t, "id_f");
        foreign.public_key_path = PathBuf::from("/etc/hosts");
        assert_eq!(
            m.fix_key_permissions(&foreign).is_ok(),
            nix::unistd::geteuid().is_root()
        );
        let missing = SshKey::new("m".into(), KeyType::Rsa, t.path().join("missing"));
        assert!(matches!(
            m.validate_key_permissions(&missing),
            Err(SshKeyError::ReadError(_))
        ));
        assert!(matches!(
            m.fix_key_permissions(&missing),
            Err(SshKeyError::WriteError(_))
        ));
    }

    #[test]
    fn test_helpers_remaining() {
        assert_eq!(extract_key_bits("BEGIN RSA PRIVATE KEY"), Some(4096));
        assert_eq!(extract_key_bits("BEGIN OPENSSH PRIVATE KEY"), Some(256));
        assert_eq!(extract_key_bits("x"), None);
        assert_eq!(infer_key_type("weird"), KeyType::Ed25519);
        assert!(is_private_key_file("my_key"));
        assert!(!is_private_key_file("config"));
        assert!(parse_public_key("only").is_err());
        assert!(matches!(
            parse_public_key("foo AAAA c"),
            Err(SshKeyError::UnsupportedType(_))
        ));
        assert!(matches!(
            parse_public_key("ssh-rsa !!!"),
            Err(SshKeyError::FingerprintFailed(_))
        ));
        assert!(matches!(
            calculate_fingerprint("one"),
            Err(SshKeyError::FingerprintFailed(_))
        ));
        let pk = parse_public_key("ssh-rsa AAAA").unwrap();
        assert!(pk.comment.is_none());
    }
}
