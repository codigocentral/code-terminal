//! Tipos e estruturas de dados para chaves SSH

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Tipos de chaves SSH suportados
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyType {
    /// RSA (recomendado: 4096 bits)
    Rsa,
    /// Ed25519 (recomendado para novas chaves)
    Ed25519,
    /// ECDSA com curva P-256
    EcdsaP256,
    /// ECDSA com curva P-384
    EcdsaP384,
    /// ECDSA com curva P-521
    EcdsaP521,
    /// DSA (deprecated, não recomendado)
    Dsa,
}

impl KeyType {
    /// Retorna o nome do tipo de chave como string
    pub fn as_str(&self) -> &'static str {
        match self {
            KeyType::Rsa => "RSA",
            KeyType::Ed25519 => "ED25519",
            KeyType::EcdsaP256 => "ECDSA-P256",
            KeyType::EcdsaP384 => "ECDSA-P384",
            KeyType::EcdsaP521 => "ECDSA-P521",
            KeyType::Dsa => "DSA",
        }
    }

    /// Retorna o nome do arquivo padrão para o tipo de chave
    pub fn default_filename(&self) -> &'static str {
        match self {
            KeyType::Rsa => "id_rsa",
            KeyType::Ed25519 => "id_ed25519",
            KeyType::EcdsaP256 | KeyType::EcdsaP384 | KeyType::EcdsaP521 => "id_ecdsa",
            KeyType::Dsa => "id_dsa",
        }
    }

    /// Retorna o tamanho de bits padrão para o tipo de chave
    pub fn default_bits(&self) -> usize {
        match self {
            KeyType::Rsa => 4096,
            KeyType::Ed25519 => 256,
            KeyType::EcdsaP256 => 256,
            KeyType::EcdsaP384 => 384,
            KeyType::EcdsaP521 => 521,
            KeyType::Dsa => 1024,
        }
    }

    /// Verifica se o tipo de chave é recomendado
    pub fn is_recommended(&self) -> bool {
        matches!(self, KeyType::Ed25519 | KeyType::Rsa | KeyType::EcdsaP256)
    }

    /// Tenta parsear o tipo de chave a partir de uma string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "RSA" | "SSH-RSA" => Some(KeyType::Rsa),
            "ED25519" | "SSH-ED25519" => Some(KeyType::Ed25519),
            "ECDSA" | "ECDSA-P256" | "SSH-ECDSA" => Some(KeyType::EcdsaP256),
            "ECDSA-P384" => Some(KeyType::EcdsaP384),
            "ECDSA-P521" => Some(KeyType::EcdsaP521),
            "DSA" | "SSH-DSA" => Some(KeyType::Dsa),
            _ => None,
        }
    }
}

impl fmt::Display for KeyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Fingerprint de uma chave SSH
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyFingerprint {
    /// Hash SHA-256 em base64
    pub sha256: String,
    /// Hash MD5 em formato hex (legacy)
    pub md5: Option<String>,
}

impl KeyFingerprint {
    /// Cria um novo fingerprint
    pub fn new(sha256: String, md5: Option<String>) -> Self {
        Self { sha256, md5 }
    }

    /// Cria fingerprint apenas com SHA-256
    pub fn from_sha256(sha256: String) -> Self {
        Self { sha256, md5: None }
    }

    /// Retorna o fingerprint curto (primeiros 12 caracteres)
    pub fn short(&self) -> &str {
        let end = self.sha256.len().min(12);
        &self.sha256[..end]
    }
}

impl fmt::Display for KeyFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SHA256:{}", self.sha256)
    }
}

/// Chave pública SSH
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKey {
    /// Tipo da chave
    pub key_type: KeyType,
    /// Conteúdo da chave em formato OpenSSH
    pub data: String,
    /// Comentário da chave
    pub comment: Option<String>,
    /// Fingerprint da chave
    pub fingerprint: KeyFingerprint,
}

impl PublicKey {
    /// Cria uma nova chave pública
    pub fn new(
        key_type: KeyType,
        data: String,
        comment: Option<String>,
        fingerprint: KeyFingerprint,
    ) -> Self {
        Self {
            key_type,
            data,
            comment,
            fingerprint,
        }
    }

    /// Retorna a linha completa da chave pública para arquivo authorized_keys
    pub fn to_authorized_keys_line(&self) -> String {
        match &self.comment {
            Some(comment) => format!("{} {}", self.data, comment),
            None => self.data.clone(),
        }
    }

    /// Verifica se a chave tem um comentário
    pub fn has_comment(&self) -> bool {
        self.comment.is_some() && !self.comment.as_ref().unwrap().is_empty()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_authorized_keys_line())
    }
}

/// Chave privada SSH
#[derive(Debug, Clone)]
pub struct PrivateKey {
    /// Tipo da chave
    pub key_type: KeyType,
    /// Caminho do arquivo da chave
    pub path: PathBuf,
    /// Se a chave está criptografada
    pub encrypted: bool,
    /// Chave pública correspondente (se disponível)
    pub public_key: Option<PublicKey>,
    /// Comentário
    pub comment: Option<String>,
}

impl PrivateKey {
    /// Cria uma nova chave privada
    pub fn new(key_type: KeyType, path: PathBuf, encrypted: bool) -> Self {
        Self {
            key_type,
            path,
            encrypted,
            public_key: None,
            comment: None,
        }
    }

    /// Define a chave pública correspondente
    pub fn with_public_key(mut self, public_key: PublicKey) -> Self {
        self.public_key = Some(public_key);
        self
    }

    /// Define o comentário
    pub fn with_comment(mut self, comment: String) -> Self {
        self.comment = Some(comment);
        self
    }

    /// Retorna o nome do arquivo da chave
    pub fn filename(&self) -> &str {
        self.path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    }

    /// Verifica se a chave tem arquivo público correspondente
    pub fn has_public_key_file(&self) -> bool {
        let pub_path = self.path.with_extension("pub");
        pub_path.exists()
    }
}

/// Chave SSH completa (par público/privado)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshKey {
    /// Nome/identificador da chave
    pub name: String,
    /// Tipo da chave
    pub key_type: KeyType,
    /// Caminho do arquivo da chave privada
    pub private_key_path: PathBuf,
    /// Caminho do arquivo da chave pública
    pub public_key_path: PathBuf,
    /// Chave pública (se carregada)
    #[serde(skip)]
    pub public_key: Option<PublicKey>,
    /// Se a chave privada está criptografada
    pub encrypted: bool,
    /// Comentário da chave
    pub comment: Option<String>,
    /// Fingerprint da chave
    pub fingerprint: Option<KeyFingerprint>,
    /// Tamanho em bits
    pub bits: Option<usize>,
    /// Se a chave está carregada no agent
    #[serde(skip)]
    pub in_agent: bool,
}

impl SshKey {
    /// Cria uma nova chave SSH
    pub fn new(name: String, key_type: KeyType, private_key_path: PathBuf) -> Self {
        let public_key_path = private_key_path.with_extension("pub");
        Self {
            name,
            key_type,
            private_key_path,
            public_key_path,
            public_key: None,
            encrypted: false,
            comment: None,
            fingerprint: None,
            bits: None,
            in_agent: false,
        }
    }

    /// Define a chave pública
    pub fn with_public_key(mut self, public_key: PublicKey) -> Self {
        self.fingerprint = Some(public_key.fingerprint.clone());
        self.comment = public_key.comment.clone();
        self.public_key = Some(public_key);
        self
    }

    /// Define se está criptografada
    pub fn with_encrypted(mut self, encrypted: bool) -> Self {
        self.encrypted = encrypted;
        self
    }

    /// Define o tamanho em bits
    pub fn with_bits(mut self, bits: usize) -> Self {
        self.bits = Some(bits);
        self
    }

    /// Define se está no agent
    pub fn with_in_agent(mut self, in_agent: bool) -> Self {
        self.in_agent = in_agent;
        self
    }

    /// Verifica se os arquivos da chave existem
    pub fn files_exist(&self) -> bool {
        self.private_key_path.exists()
    }

    /// Verifica se o arquivo público existe
    pub fn public_key_exists(&self) -> bool {
        self.public_key_path.exists()
    }

    /// Retorna o nome do arquivo da chave privada
    pub fn filename(&self) -> &str {
        self.private_key_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
    }
}

impl fmt::Display for SshKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encrypted = if self.encrypted { " [encrypted]" } else { "" };
        let agent = if self.in_agent { " [in agent]" } else { "" };
        write!(f, "{} ({}){}{}", self.name, self.key_type, encrypted, agent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_type_as_str() {
        assert_eq!(KeyType::Rsa.as_str(), "RSA");
        assert_eq!(KeyType::Ed25519.as_str(), "ED25519");
        assert_eq!(KeyType::EcdsaP256.as_str(), "ECDSA-P256");
    }

    #[test]
    fn test_key_type_default_filename() {
        assert_eq!(KeyType::Rsa.default_filename(), "id_rsa");
        assert_eq!(KeyType::Ed25519.default_filename(), "id_ed25519");
        assert_eq!(KeyType::EcdsaP256.default_filename(), "id_ecdsa");
    }

    #[test]
    fn test_key_type_default_bits() {
        assert_eq!(KeyType::Rsa.default_bits(), 4096);
        assert_eq!(KeyType::Ed25519.default_bits(), 256);
    }

    #[test]
    fn test_key_type_is_recommended() {
        assert!(KeyType::Ed25519.is_recommended());
        assert!(KeyType::Rsa.is_recommended());
        assert!(!KeyType::Dsa.is_recommended());
    }

    #[test]
    fn test_key_type_from_str() {
        assert_eq!(KeyType::from_str("RSA"), Some(KeyType::Rsa));
        assert_eq!(KeyType::from_str("ed25519"), Some(KeyType::Ed25519));
        assert_eq!(KeyType::from_str("unknown"), None);
    }

    #[test]
    fn test_key_fingerprint() {
        let fp = KeyFingerprint::from_sha256("abc123".to_string());
        assert_eq!(fp.sha256, "abc123");
        assert!(fp.md5.is_none());
        assert_eq!(format!("{}", fp), "SHA256:abc123");
    }

    #[test]
    fn test_public_key_to_authorized_keys_line() {
        let fp = KeyFingerprint::from_sha256("test".to_string());
        let pk = PublicKey::new(
            KeyType::Ed25519,
            "ssh-ed25519 AAAA...".to_string(),
            Some("user@host".to_string()),
            fp,
        );
        assert_eq!(
            pk.to_authorized_keys_line(),
            "ssh-ed25519 AAAA... user@host"
        );
    }

    #[test]
    fn test_ssh_key_creation() {
        let key = SshKey::new(
            "test-key".to_string(),
            KeyType::Ed25519,
            PathBuf::from("/home/user/.ssh/id_ed25519"),
        );
        assert_eq!(key.name, "test-key");
        assert_eq!(key.key_type, KeyType::Ed25519);
        assert!(!key.encrypted);
    }

    #[test]
    fn test_ssh_key_display() {
        let key = SshKey::new(
            "test-key".to_string(),
            KeyType::Ed25519,
            PathBuf::from("/home/user/.ssh/id_ed25519"),
        )
        .with_encrypted(true);

        let display = format!("{}", key);
        assert!(display.contains("test-key"));
        assert!(display.contains("ED25519"));
        assert!(display.contains("[encrypted]"));
    }
}
