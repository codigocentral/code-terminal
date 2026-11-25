//! Definições de erros para o módulo SSH
//!
//! Este módulo define os tipos de erro utilizados em todo o módulo SSH,
//! incluindo erros de configuração, chaves, sessões e armazenamento.

use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SshError {
    #[error("Failed to parse SSH config: {0}")]
    ParseError(String),

    #[error("Invalid SSH config: {0}")]
    InvalidConfig(String),

    #[error("Failed to read SSH config file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Invalid host pattern: {0}")]
    InvalidPattern(String),

    #[error("Unknown directive: {0}")]
    UnknownDirective(String),

    #[error("Missing required value for directive: {0}")]
    MissingValue(String),

    #[error("SSH config file not found")]
    ConfigNotFound,

    #[error("Home directory not found")]
    HomeNotFound,

    #[error("Invalid port number: {0}")]
    InvalidPort(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Host not found: {0}")]
    HostNotFound(String),
}

pub type SshResult<T> = Result<T, SshError>;

#[derive(Debug, Error)]
pub enum SshKeyError {
    #[error("Falha ao ler chave: {0}")]
    ReadError(#[source] std::io::Error),

    #[error("Falha ao escrever chave: {0}")]
    WriteError(#[source] std::io::Error),

    #[error("Formato de chave invalido: {0}")]
    InvalidFormat(String),

    #[error("Tipo de chave nao suportado: {0}")]
    UnsupportedType(String),

    #[error("Chave nao encontrada: {0}")]
    KeyNotFound(PathBuf),

    #[error("Senha incorreta para chave")]
    InvalidPassphrase,

    #[error("Chave esta criptografada e requer senha")]
    KeyEncrypted,

    #[error("Falha ao gerar chave: {0}")]
    GenerationFailed(String),

    #[error("Falha ao validar chave: {0}")]
    ValidationFailed(String),

    #[error("Falha ao calcular fingerprint: {0}")]
    FingerprintFailed(String),

    #[error("Agent SSH nao disponivel")]
    AgentNotAvailable,

    #[error("Falha na comunicacao com agent: {0}")]
    AgentCommunication(String),

    #[error("Falha ao adicionar chave ao agent: {0}")]
    AgentAddFailed(String),

    #[error("Falha ao remover chave do agent: {0}")]
    AgentRemoveFailed(String),

    #[error("Falha ao listar chaves do agent: {0}")]
    AgentListFailed(String),

    #[error("Armazenamento seguro nao disponivel")]
    SecureStorageNotAvailable,

    #[error("Falha no armazenamento seguro: {0}")]
    SecureStorageFailed(String),

    #[error("Chave ja existe: {0}")]
    KeyAlreadyExists(PathBuf),

    #[error("Permissoes invalidas para arquivo de chave")]
    InvalidPermissions,

    #[error("Diretorio ~/.ssh nao encontrado")]
    SshDirNotFound,

    #[error("Falha ao criar diretorio: {0}")]
    CreateDirFailed(#[source] std::io::Error),

    #[error("Comentario da chave invalido")]
    InvalidComment,

    #[error("Tamanho de chave invalido: {0}")]
    InvalidKeySize(usize),
}

pub type SshKeyResult<T> = Result<T, SshKeyError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_error_display() {
        let err = SshError::ParseError("test error".to_string());
        assert!(err.to_string().contains("test error"));

        let err = SshError::ConfigNotFound;
        assert!(err.to_string().contains("not found"));

        let err = SshError::InvalidPort("abc".to_string());
        assert!(err.to_string().contains("abc"));
    }

    #[test]
    fn test_ssh_key_error_display() {
        let err = SshKeyError::InvalidPassphrase;
        assert!(err.to_string().contains("Senha"));

        let err = SshKeyError::KeyEncrypted;
        assert!(err.to_string().contains("criptografada"));

        let path = PathBuf::from("/tmp/test");
        let err = SshKeyError::KeyNotFound(path.clone());
        assert!(err.to_string().contains("test"));
    }

    #[test]
    fn test_ssh_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let ssh_err: SshError = io_err.into();
        match ssh_err {
            SshError::IoError(_) => (),
            _ => panic!("Expected IoError variant"),
        }
    }

    #[test]
    fn test_ssh_error_variants() {
        // Testa que todas as variantes podem ser criadas
        let _ = SshError::ParseError("test".to_string());
        let _ = SshError::InvalidConfig("test".to_string());
        let _ = SshError::InvalidPattern("test".to_string());
        let _ = SshError::UnknownDirective("test".to_string());
        let _ = SshError::MissingValue("test".to_string());
        let _ = SshError::HomeNotFound;
        let _ = SshError::ConnectionFailed("test".to_string());
        let _ = SshError::AuthenticationFailed("test".to_string());
        let _ = SshError::HostNotFound("test".to_string());
    }

    #[test]
    fn test_ssh_key_error_variants() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let _ = SshKeyError::ReadError(io_err);
        let _ = SshKeyError::InvalidFormat("test".to_string());
        let _ = SshKeyError::UnsupportedType("test".to_string());
        let _ = SshKeyError::KeyNotFound(PathBuf::from("/tmp/test"));
        let _ = SshKeyError::GenerationFailed("test".to_string());
        let _ = SshKeyError::ValidationFailed("test".to_string());
        let _ = SshKeyError::FingerprintFailed("test".to_string());
        let _ = SshKeyError::AgentNotAvailable;
        let _ = SshKeyError::AgentCommunication("test".to_string());
        let _ = SshKeyError::SecureStorageNotAvailable;
        let _ = SshKeyError::SecureStorageFailed("test".to_string());
        let _ = SshKeyError::KeyAlreadyExists(PathBuf::from("/tmp/test"));
        let _ = SshKeyError::InvalidPermissions;
        let _ = SshKeyError::SshDirNotFound;
        let _ = SshKeyError::InvalidKeySize(1024);
    }
}
