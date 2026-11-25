//! Armazenamento seguro de chaves SSH
//!
//! Este módulo fornece interfaces e implementações para armazenamento seguro
//! de chaves SSH, com suporte a criptografia e controle de acesso.
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::ssh::storage::KeyStorage;
//! use code_terminal::ssh::{KeyType, SshKey};
//! use std::path::PathBuf;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Criar armazenamento
//! let mut storage = KeyStorage::default_storage()?;
//!
//! // Armazenar uma chave
//! let key = SshKey::new(
//!     "minha-chave".to_string(),
//!     KeyType::Ed25519,
//!     PathBuf::from("/home/usuario/.ssh/id_ed25519"),
//! );
//! storage.store_key(&key)?;
//!
//! // Recuperar a chave
//! let stored = storage.retrieve_key("minha-chave")?;
//! println!("Tipo: {:?}", stored.key_type);
//!
//! // Listar chaves
//! for key_id in storage.list_keys() {
//!     println!("Chave armazenada: {}", key_id);
//! }
//! # Ok(())
//! # }
//! ```

use crate::ssh::error::{SshKeyError, SshKeyResult};
use crate::ssh::key::{KeyType, SshKey};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Interface para armazenamento seguro de chaves
pub trait SecureStorage: Send + Sync {
    /// Armazena uma chave de forma segura
    fn store(&mut self, key_id: &str, data: &[u8]) -> SshKeyResult<()>;

    /// Recupera uma chave armazenada
    fn retrieve(&self, key_id: &str) -> SshKeyResult<Vec<u8>>;

    /// Remove uma chave armazenada
    fn delete(&mut self, key_id: &str) -> SshKeyResult<()>;

    /// Lista todas as chaves armazenadas
    fn list(&self) -> SshKeyResult<Vec<String>>;

    /// Verifica se uma chave existe
    fn exists(&self, key_id: &str) -> bool;
}

/// Armazenamento de chaves baseado em arquivos
#[derive(Debug)]
pub struct KeyStorage {
    /// Diretório base para armazenamento
    base_dir: PathBuf,
    /// Chaves indexadas por ID
    index: HashMap<String, PathBuf>,
}

impl KeyStorage {
    /// Cria um novo armazenamento de chaves
    pub fn new(base_dir: PathBuf) -> SshKeyResult<Self> {
        if !base_dir.exists() {
            fs::create_dir_all(&base_dir).map_err(SshKeyError::CreateDirFailed)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&base_dir, fs::Permissions::from_mode(0o700))
                    .map_err(SshKeyError::WriteError)?;
            }
        }

        let mut storage = Self {
            base_dir,
            index: HashMap::new(),
        };

        storage.rebuild_index()?;
        Ok(storage)
    }

    /// Cria armazenamento no diretório padrão
    pub fn default_storage() -> SshKeyResult<Self> {
        let base_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codeterm")
            .join("keys");

        Self::new(base_dir)
    }

    /// Reconstrói o índice de chaves
    fn rebuild_index(&mut self) -> SshKeyResult<()> {
        self.index.clear();

        if !self.base_dir.exists() {
            return Ok(());
        }

        let entries = fs::read_dir(&self.base_dir).map_err(SshKeyError::ReadError)?;

        for entry in entries {
            let entry = entry.map_err(SshKeyError::ReadError)?;
            let path = entry.path();

            if path.extension().map(|e| e == "key").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    if let Some(id) = stem.to_str() {
                        self.index.insert(id.to_string(), path);
                    }
                }
            }
        }

        debug!("Rebuilt key index with {} entries", self.index.len());
        Ok(())
    }

    /// Armazena uma chave SSH
    pub fn store_key(&mut self, key: &SshKey) -> SshKeyResult<()> {
        let key_id = &key.name;
        let path = self.base_dir.join(format!("{}.key", key_id));

        let key_data = serde_json::to_string_pretty(key)
            .map_err(|e| SshKeyError::SecureStorageFailed(e.to_string()))?;

        fs::write(&path, key_data).map_err(SshKeyError::WriteError)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(SshKeyError::WriteError)?;
        }

        self.index.insert(key_id.clone(), path);
        info!("Stored key: {}", key_id);
        Ok(())
    }

    /// Recupera uma chave SSH
    pub fn retrieve_key(&self, key_id: &str) -> SshKeyResult<SshKey> {
        let path = self
            .index
            .get(key_id)
            .ok_or_else(|| SshKeyError::KeyNotFound(PathBuf::from(key_id)))?;

        let data = fs::read_to_string(path).map_err(SshKeyError::ReadError)?;

        let key: SshKey = serde_json::from_str(&data)
            .map_err(|e| SshKeyError::SecureStorageFailed(e.to_string()))?;

        Ok(key)
    }

    /// Remove uma chave
    pub fn delete_key(&mut self, key_id: &str) -> SshKeyResult<()> {
        let path = self
            .index
            .get(key_id)
            .ok_or_else(|| SshKeyError::KeyNotFound(PathBuf::from(key_id)))?;

        fs::remove_file(path).map_err(SshKeyError::WriteError)?;

        self.index.remove(key_id);
        info!("Deleted stored key: {}", key_id);
        Ok(())
    }

    /// Lista todas as chaves armazenadas
    pub fn list_keys(&self) -> Vec<String> {
        self.index.keys().cloned().collect()
    }

    /// Verifica se uma chave existe
    pub fn key_exists(&self, key_id: &str) -> bool {
        self.index.contains_key(key_id)
    }

    /// Exporta uma chave para o diretório ~/.ssh
    pub fn export_to_ssh_dir(&self, key_id: &str, ssh_dir: &Path) -> SshKeyResult<PathBuf> {
        let key = self.retrieve_key(key_id)?;

        if !ssh_dir.exists() {
            fs::create_dir_all(ssh_dir).map_err(SshKeyError::CreateDirFailed)?;
        }

        let dest_path = ssh_dir.join(key_id);

        if dest_path.exists() {
            return Err(SshKeyError::KeyAlreadyExists(dest_path));
        }

        let source_path = key.private_key_path.clone();
        fs::copy(&source_path, &dest_path).map_err(SshKeyError::WriteError)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dest_path, fs::Permissions::from_mode(0o600))
                .map_err(SshKeyError::WriteError)?;
        }

        let pub_source = source_path.with_extension("pub");
        let pub_dest = dest_path.with_extension("pub");

        if pub_source.exists() {
            fs::copy(&pub_source, &pub_dest).map_err(SshKeyError::WriteError)?;
        }

        info!("Exported key {} to {:?}", key_id, ssh_dir);
        Ok(dest_path)
    }

    /// Importa uma chave do diretório ~/.ssh
    pub fn import_from_ssh_dir(&mut self, key_path: &Path) -> SshKeyResult<String> {
        let key_name = key_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| SshKeyError::InvalidFormat("Invalid key filename".to_string()))?;

        let key_data = fs::read_to_string(key_path).map_err(SshKeyError::ReadError)?;

        let key_type = Self::detect_key_type_from_content(&key_data)
            .ok_or_else(|| SshKeyError::InvalidFormat("Unknown key type".to_string()))?;

        let encrypted = key_data.contains("ENCRYPTED")
            || openssh_cipher(&key_data).is_some_and(|cipher| cipher != "none");

        let key = SshKey::new(key_name.to_string(), key_type, key_path.to_path_buf())
            .with_encrypted(encrypted);

        self.store_key(&key)?;
        Ok(key_name.to_string())
    }

    /// Detecta o tipo de chave pelo conteúdo
    fn detect_key_type_from_content(content: &str) -> Option<KeyType> {
        // No formato OpenSSH (padrão do ssh-keygen desde a 7.8) o tipo fica
        // dentro do blob base64; antes toda chave nesse formato virava Ed25519
        openssh_blob(content)
            .and_then(|blob| key_type_from_blob(&blob))
            .or_else(|| key_type_from_text(content))
    }
}

/// Nome do algoritmo dentro do blob OpenSSH → tipo, na ordem de checagem
const BLOB_KEY_TYPES: &[(&[u8], KeyType)] = &[
    (b"ssh-ed25519", KeyType::Ed25519),
    (b"ssh-rsa", KeyType::Rsa),
    (b"ecdsa-sha2-nistp521", KeyType::EcdsaP521),
    (b"ecdsa-sha2-nistp384", KeyType::EcdsaP384),
    (b"ecdsa-sha2-nistp256", KeyType::EcdsaP256),
    (b"ssh-dss", KeyType::Dsa),
];

/// Marcadores em texto (chave pública ou cabeçalho PEM) → tipo, na ordem de checagem
const TEXT_KEY_TYPES: &[(&str, &str, KeyType)] = &[
    ("ssh-ed25519", "BEGIN OPENSSH PRIVATE KEY", KeyType::Ed25519),
    ("ssh-rsa", "BEGIN RSA PRIVATE KEY", KeyType::Rsa),
    (
        "ecdsa-sha2-nistp256",
        "BEGIN EC PRIVATE KEY",
        KeyType::EcdsaP256,
    ),
    ("ssh-dss", "BEGIN DSA PRIVATE KEY", KeyType::Dsa),
];

/// Tipo da chave pelo nome do algoritmo gravado no blob OpenSSH
fn key_type_from_blob(blob: &[u8]) -> Option<KeyType> {
    BLOB_KEY_TYPES
        .iter()
        .find(|(needle, _)| blob.windows(needle.len()).any(|w| w == *needle))
        .map(|&(_, key_type)| key_type)
}

/// Tipo da chave pelos marcadores em texto
fn key_type_from_text(content: &str) -> Option<KeyType> {
    TEXT_KEY_TYPES
        .iter()
        .find(|(public, pem, _)| content.contains(public) || content.contains(pem))
        .map(|&(_, _, key_type)| key_type)
}

const OPENSSH_BEGIN: &str = "-----BEGIN OPENSSH PRIVATE KEY-----";
const OPENSSH_END: &str = "-----END OPENSSH PRIVATE KEY-----";
const OPENSSH_MAGIC: &[u8] = b"openssh-key-v1\0";

/// Decodifica o blob de uma chave privada no formato OpenSSH.
fn openssh_blob(content: &str) -> Option<Vec<u8>> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let start = content.find(OPENSSH_BEGIN)? + OPENSSH_BEGIN.len();
    let end = start + content[start..].find(OPENSSH_END)?;
    let body: String = content[start..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let blob = STANDARD.decode(body).ok()?;
    blob.starts_with(OPENSSH_MAGIC).then_some(blob)
}

/// Nome da cifra de uma chave OpenSSH ("none" quando não tem passphrase).
fn openssh_cipher(content: &str) -> Option<String> {
    let blob = openssh_blob(content)?;
    let rest = &blob[OPENSSH_MAGIC.len()..];
    let len = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    let name = rest.get(4..4 + len)?;
    Some(String::from_utf8_lossy(name).to_string())
}

/// Armazenamento seguro com criptografia
#[derive(Debug)]
pub struct EncryptedStorage {
    /// Armazenamento base
    base: KeyStorage,
    /// Senha mestre (hash)
    master_key_hash: Option<String>,
}

impl EncryptedStorage {
    /// Cria um novo armazenamento criptografado
    pub fn new(base_dir: PathBuf, master_password: &str) -> SshKeyResult<Self> {
        let master_key_hash = Self::hash_password(master_password);

        Ok(Self {
            base: KeyStorage::new(base_dir)?,
            master_key_hash: Some(master_key_hash),
        })
    }

    /// Hash da senha usando SHA-256
    fn hash_password(password: &str) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        let hash = hasher.finalize();
        format!("{:x}", hash)
    }

    /// Verifica a senha mestre
    pub fn verify_password(&self, password: &str) -> bool {
        if let Some(ref stored_hash) = self.master_key_hash {
            let hash = Self::hash_password(password);
            return hash == *stored_hash;
        }
        false
    }

    /// Altera a senha mestre
    pub fn change_password(&mut self, old_password: &str, new_password: &str) -> SshKeyResult<()> {
        if !self.verify_password(old_password) {
            return Err(SshKeyError::InvalidPassphrase);
        }

        self.master_key_hash = Some(Self::hash_password(new_password));
        info!("Changed master password");
        Ok(())
    }

    /// Armazena uma chave de forma criptografada
    pub fn store_encrypted(&mut self, key: &SshKey, password: &str) -> SshKeyResult<()> {
        if !self.verify_password(password) {
            return Err(SshKeyError::InvalidPassphrase);
        }

        let key_data =
            serde_json::to_vec(key).map_err(|e| SshKeyError::SecureStorageFailed(e.to_string()))?;

        let encrypted = self.encrypt_data(&key_data, password)?;

        let key_id = format!("{}_encrypted", key.name);
        let path = self.base.base_dir.join(format!("{}.enc", key_id));

        fs::write(&path, encrypted).map_err(SshKeyError::WriteError)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(SshKeyError::WriteError)?;
        }

        info!("Stored encrypted key: {}", key.name);
        Ok(())
    }

    /// Recupera uma chave criptografada
    pub fn retrieve_encrypted(&self, key_id: &str, password: &str) -> SshKeyResult<SshKey> {
        if !self.verify_password(password) {
            return Err(SshKeyError::InvalidPassphrase);
        }

        let enc_key_id = format!("{}_encrypted", key_id);
        let path = self.base.base_dir.join(format!("{}.enc", enc_key_id));

        if !path.exists() {
            return Err(SshKeyError::KeyNotFound(path));
        }

        let encrypted = fs::read(&path).map_err(SshKeyError::ReadError)?;

        let decrypted = self.decrypt_data(&encrypted, password)?;

        let key: SshKey = serde_json::from_slice(&decrypted)
            .map_err(|e| SshKeyError::SecureStorageFailed(e.to_string()))?;

        Ok(key)
    }

    /// Criptografa dados (XOR simples para demonstração)
    fn encrypt_data(&self, data: &[u8], password: &str) -> SshKeyResult<Vec<u8>> {
        let key = password.as_bytes();
        if key.is_empty() {
            // `i % key.len()` dividiria por zero
            return Err(SshKeyError::SecureStorageFailed(
                "Empty password cannot be used for encryption".to_string(),
            ));
        }
        let encrypted: Vec<u8> = data
            .iter()
            .enumerate()
            .map(|(i, &byte)| byte ^ key[i % key.len()])
            .collect();
        Ok(encrypted)
    }

    /// Descriptografa dados
    fn decrypt_data(&self, data: &[u8], password: &str) -> SshKeyResult<Vec<u8>> {
        self.encrypt_data(data, password)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_key_storage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = KeyStorage::new(temp_dir.path().to_path_buf());
        assert!(storage.is_ok());
    }

    #[test]
    fn test_key_storage_store_and_retrieve() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = KeyStorage::new(temp_dir.path().to_path_buf()).unwrap();

        let key = SshKey::new(
            "test-key".to_string(),
            KeyType::Ed25519,
            PathBuf::from("/tmp/id_ed25519"),
        );

        storage.store_key(&key).unwrap();
        assert!(storage.key_exists("test-key"));

        let retrieved = storage.retrieve_key("test-key").unwrap();
        assert_eq!(retrieved.name, "test-key");
        assert_eq!(retrieved.key_type, KeyType::Ed25519);
    }

    #[test]
    fn test_key_storage_delete() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = KeyStorage::new(temp_dir.path().to_path_buf()).unwrap();

        let key = SshKey::new(
            "test-key".to_string(),
            KeyType::Ed25519,
            PathBuf::from("/tmp/id_ed25519"),
        );

        storage.store_key(&key).unwrap();
        assert!(storage.key_exists("test-key"));

        storage.delete_key("test-key").unwrap();
        assert!(!storage.key_exists("test-key"));
    }

    #[test]
    fn test_key_storage_list() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = KeyStorage::new(temp_dir.path().to_path_buf()).unwrap();

        let keys = storage.list_keys();
        assert!(keys.is_empty());

        let key1 = SshKey::new(
            "key1".to_string(),
            KeyType::Ed25519,
            PathBuf::from("/tmp/id_ed25519"),
        );
        let key2 = SshKey::new(
            "key2".to_string(),
            KeyType::Rsa,
            PathBuf::from("/tmp/id_rsa"),
        );

        storage.store_key(&key1).unwrap();
        storage.store_key(&key2).unwrap();

        let keys = storage.list_keys();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_encrypted_storage_password() {
        let hash = EncryptedStorage::hash_password("test123");
        assert!(!hash.is_empty());

        let hash2 = EncryptedStorage::hash_password("test123");
        assert_eq!(hash, hash2);
    }

    /// Monta uma chave privada no formato OpenSSH (sem material real de
    /// chave) com a cifra e o tipo dados.
    fn openssh_key(cipher: &str, key_type: &str) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        fn string(out: &mut Vec<u8>, s: &[u8]) {
            out.extend_from_slice(&(s.len() as u32).to_be_bytes());
            out.extend_from_slice(s);
        }
        let mut pubkey = Vec::new();
        string(&mut pubkey, key_type.as_bytes());
        string(&mut pubkey, &[0u8; 32]);
        let mut blob = OPENSSH_MAGIC.to_vec();
        string(&mut blob, cipher.as_bytes());
        string(
            &mut blob,
            if cipher == "none" { b"none" } else { b"bcrypt" },
        );
        string(&mut blob, b"");
        blob.extend_from_slice(&1u32.to_be_bytes());
        string(&mut blob, &pubkey);
        let b64 = STANDARD.encode(&blob);
        let mut body = String::new();
        for chunk in b64.as_bytes().chunks(70) {
            body.push_str(std::str::from_utf8(chunk).unwrap());
            body.push('\n');
        }
        format!("{OPENSSH_BEGIN}\n{body}{OPENSSH_END}\n")
    }

    #[test]
    fn test_detect_key_type_openssh_format_reads_embedded_type() {
        // Regressão: toda chave "BEGIN OPENSSH PRIVATE KEY" virava Ed25519
        let cases = [
            ("ssh-ed25519", KeyType::Ed25519),
            ("ssh-rsa", KeyType::Rsa),
            ("ecdsa-sha2-nistp256", KeyType::EcdsaP256),
            ("ecdsa-sha2-nistp384", KeyType::EcdsaP384),
            ("ecdsa-sha2-nistp521", KeyType::EcdsaP521),
            ("ssh-dss", KeyType::Dsa),
        ];
        for (name, expected) in cases {
            let pem = openssh_key("none", name);
            assert_eq!(
                KeyStorage::detect_key_type_from_content(&pem),
                Some(expected),
                "{name}"
            );
        }
        // blob sem tipo conhecido ou ilegível: mantém o palpite antigo
        let unknown = openssh_key("none", "sk-desconhecido");
        assert_eq!(
            KeyStorage::detect_key_type_from_content(&unknown),
            Some(KeyType::Ed25519)
        );
        let garbage = format!("{OPENSSH_BEGIN}\n!!!não é base64!!!\n{OPENSSH_END}\n");
        assert_eq!(
            KeyStorage::detect_key_type_from_content(&garbage),
            Some(KeyType::Ed25519)
        );
    }

    #[test]
    fn test_detect_key_type_legacy_and_public_formats() {
        let d = KeyStorage::detect_key_type_from_content;
        assert_eq!(d("-----BEGIN RSA PRIVATE KEY-----"), Some(KeyType::Rsa));
        assert_eq!(d("ssh-rsa AAAA x"), Some(KeyType::Rsa));
        assert_eq!(d("ssh-ed25519 AAAA x"), Some(KeyType::Ed25519));
        assert_eq!(
            d("-----BEGIN EC PRIVATE KEY-----"),
            Some(KeyType::EcdsaP256)
        );
        assert_eq!(d("ecdsa-sha2-nistp256 AAAA"), Some(KeyType::EcdsaP256));
        assert_eq!(d("-----BEGIN DSA PRIVATE KEY-----"), Some(KeyType::Dsa));
        assert_eq!(d("ssh-dss AAAA"), Some(KeyType::Dsa));
        assert_eq!(d("qualquer coisa"), None);
    }

    #[test]
    fn test_openssh_cipher_parsing() {
        assert_eq!(
            openssh_cipher(&openssh_key("none", "ssh-ed25519")).as_deref(),
            Some("none")
        );
        assert_eq!(
            openssh_cipher(&openssh_key("aes256-ctr", "ssh-ed25519")).as_deref(),
            Some("aes256-ctr")
        );
        assert_eq!(openssh_cipher("-----BEGIN RSA PRIVATE KEY-----"), None);
        // blob truncado logo após o magic
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let short = format!(
            "{OPENSSH_BEGIN}\n{}\n{OPENSSH_END}",
            STANDARD.encode(OPENSSH_MAGIC)
        );
        assert_eq!(openssh_cipher(&short), None);
        // magic errado
        let wrong = format!(
            "{OPENSSH_BEGIN}\n{}\n{OPENSSH_END}",
            STANDARD.encode(b"outra-coisa")
        );
        assert!(openssh_blob(&wrong).is_none());
        // sem marcador final
        assert!(openssh_blob(OPENSSH_BEGIN).is_none());
    }

    #[test]
    fn test_import_from_ssh_dir_detects_type_and_encryption() {
        let temp = TempDir::new().unwrap();
        let ssh = temp.path().join("ssh");
        std::fs::create_dir(&ssh).unwrap();
        let mut storage = KeyStorage::new(temp.path().join("store")).unwrap();

        let rsa = ssh.join("id_rsa");
        std::fs::write(&rsa, openssh_key("none", "ssh-rsa")).unwrap();
        assert_eq!(storage.import_from_ssh_dir(&rsa).unwrap(), "id_rsa");
        let k = storage.retrieve_key("id_rsa").unwrap();
        assert_eq!(k.key_type, KeyType::Rsa);
        assert!(!k.encrypted);
        assert_eq!(k.private_key_path, rsa);

        let enc = ssh.join("id_ed25519");
        std::fs::write(&enc, openssh_key("aes256-ctr", "ssh-ed25519")).unwrap();
        storage.import_from_ssh_dir(&enc).unwrap();
        assert!(storage.retrieve_key("id_ed25519").unwrap().encrypted);

        let pem = ssh.join("legacy");
        std::fs::write(
            &pem,
            "-----BEGIN RSA PRIVATE KEY-----\nProc-Type: 4,ENCRYPTED\n-----END RSA PRIVATE KEY-----\n",
        )
        .unwrap();
        storage.import_from_ssh_dir(&pem).unwrap();
        let k = storage.retrieve_key("legacy").unwrap();
        assert_eq!(k.key_type, KeyType::Rsa);
        assert!(k.encrypted);

        let junk = ssh.join("junk");
        std::fs::write(&junk, "nada aqui").unwrap();
        assert!(matches!(
            storage.import_from_ssh_dir(&junk),
            Err(SshKeyError::InvalidFormat(m)) if m == "Unknown key type"
        ));
        assert!(matches!(
            storage.import_from_ssh_dir(&ssh.join("faltando")),
            Err(SshKeyError::ReadError(_))
        ));
        assert!(matches!(
            storage.import_from_ssh_dir(Path::new("/")),
            Err(SshKeyError::InvalidFormat(m)) if m == "Invalid key filename"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn test_new_creates_private_dir_and_rebuilds_index() {
        use std::os::unix::fs::PermissionsExt;
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("a").join("b");
        let mut storage = KeyStorage::new(dir.clone()).unwrap();
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let key = SshKey::new("k1".into(), KeyType::Ed25519, PathBuf::from("/x"));
        storage.store_key(&key).unwrap();
        assert_eq!(
            std::fs::metadata(dir.join("k1.key"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        // arquivos que não são .key ficam fora do índice
        std::fs::write(dir.join("notas.txt"), "x").unwrap();
        std::fs::write(dir.join("k2.key"), "{}").unwrap();

        let reopened = KeyStorage::new(dir).unwrap();
        let mut ids = reopened.list_keys();
        ids.sort();
        assert_eq!(ids, vec!["k1", "k2"]);
        assert_eq!(reopened.retrieve_key("k1").unwrap().name, "k1");
        // k2 existe no índice mas não é um SshKey válido
        assert!(matches!(
            reopened.retrieve_key("k2"),
            Err(SshKeyError::SecureStorageFailed(_))
        ));
    }

    #[test]
    fn test_missing_keys_and_failed_creation() {
        let temp = TempDir::new().unwrap();
        let mut storage = KeyStorage::new(temp.path().to_path_buf()).unwrap();
        assert!(matches!(
            storage.retrieve_key("nao"),
            Err(SshKeyError::KeyNotFound(p)) if p == Path::new("nao")
        ));
        assert!(matches!(
            storage.delete_key("nao"),
            Err(SshKeyError::KeyNotFound(_))
        ));

        // diretório base sob um arquivo comum não pode ser criado
        let file = temp.path().join("arquivo");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(
            KeyStorage::new(file.join("sub")),
            Err(SshKeyError::CreateDirFailed(_))
        ));
        // se o diretório some depois, o índice reconstruído fica vazio
        let gone = temp.path().join("some");
        let mut s2 = KeyStorage::new(gone.clone()).unwrap();
        std::fs::remove_dir(&gone).unwrap();
        s2.rebuild_index().unwrap();
        assert!(s2.list_keys().is_empty());
    }

    #[test]
    fn test_export_to_ssh_dir_copies_private_and_public() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let private = src.join("id_x");
        std::fs::write(&private, "PRIVADA").unwrap();
        std::fs::write(src.join("id_x.pub"), "PUBLICA").unwrap();

        let mut storage = KeyStorage::new(temp.path().join("store")).unwrap();
        let key = SshKey::new("deploy".into(), KeyType::Ed25519, private.clone());
        storage.store_key(&key).unwrap();

        let ssh = temp.path().join("novo-ssh");
        let dest = storage.export_to_ssh_dir("deploy", &ssh).unwrap();
        assert_eq!(dest, ssh.join("deploy"));
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "PRIVADA");
        assert_eq!(
            std::fs::read_to_string(ssh.join("deploy.pub")).unwrap(),
            "PUBLICA"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert!(matches!(
            storage.export_to_ssh_dir("deploy", &ssh),
            Err(SshKeyError::KeyAlreadyExists(p)) if p == dest
        ));
        assert!(matches!(
            storage.export_to_ssh_dir("outra", &ssh),
            Err(SshKeyError::KeyNotFound(_))
        ));

        // sem .pub na origem, só a privada é copiada
        std::fs::remove_file(src.join("id_x.pub")).unwrap();
        let ssh2 = temp.path().join("ssh2");
        std::fs::create_dir(&ssh2).unwrap();
        storage.export_to_ssh_dir("deploy", &ssh2).unwrap();
        assert!(!ssh2.join("deploy.pub").exists());

        // origem apagada: erro de escrita na cópia
        std::fs::remove_file(&private).unwrap();
        let ssh3 = temp.path().join("ssh3");
        assert!(matches!(
            storage.export_to_ssh_dir("deploy", &ssh3),
            Err(SshKeyError::WriteError(_))
        ));
    }

    #[test]
    fn test_encrypted_storage_round_trip_and_password_checks() {
        let temp = TempDir::new().unwrap();
        let mut es = EncryptedStorage::new(temp.path().to_path_buf(), "mestra").unwrap();
        assert!(es.verify_password("mestra"));
        assert!(!es.verify_password("outra"));

        let key = SshKey::new("prod".into(), KeyType::Rsa, PathBuf::from("/k/prod"))
            .with_bits(4096)
            .with_encrypted(true);
        es.store_encrypted(&key, "mestra").unwrap();
        let file = temp.path().join("prod_encrypted.enc");
        let raw = std::fs::read(&file).unwrap();
        // o conteúdo gravado não é o JSON em claro
        assert!(!String::from_utf8_lossy(&raw).contains("/k/prod"));

        let back = es.retrieve_encrypted("prod", "mestra").unwrap();
        assert_eq!(back.name, "prod");
        assert_eq!(back.bits, Some(4096));
        assert!(back.encrypted);

        assert!(matches!(
            es.store_encrypted(&key, "errada"),
            Err(SshKeyError::InvalidPassphrase)
        ));
        assert!(matches!(
            es.retrieve_encrypted("prod", "errada"),
            Err(SshKeyError::InvalidPassphrase)
        ));
        assert!(matches!(
            es.retrieve_encrypted("nao", "mestra"),
            Err(SshKeyError::KeyNotFound(_))
        ));

        // arquivo corrompido não decodifica
        std::fs::write(&file, b"\x00\x01lixo").unwrap();
        assert!(matches!(
            es.retrieve_encrypted("prod", "mestra"),
            Err(SshKeyError::SecureStorageFailed(_))
        ));

        assert!(matches!(
            es.change_password("errada", "nova"),
            Err(SshKeyError::InvalidPassphrase)
        ));
        es.change_password("mestra", "nova").unwrap();
        assert!(es.verify_password("nova"));
        assert!(!es.verify_password("mestra"));

        es.master_key_hash = None;
        assert!(!es.verify_password("nova"));
    }

    #[test]
    fn test_encrypted_storage_with_empty_password_does_not_panic() {
        // Regressão: a cifra XOR fazia `i % 0` com senha vazia
        let temp = TempDir::new().unwrap();
        let mut es = EncryptedStorage::new(temp.path().to_path_buf(), "").unwrap();
        let key = SshKey::new("k".into(), KeyType::Ed25519, PathBuf::from("/k"));
        assert!(matches!(
            es.store_encrypted(&key, ""),
            Err(SshKeyError::SecureStorageFailed(_))
        ));
        assert!(!temp.path().join("k_encrypted.enc").exists());
    }
}
