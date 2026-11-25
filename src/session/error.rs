//! Erros do sistema de sessão

use std::path::PathBuf;
use thiserror::Error;

/// Erros que podem ocorrer no sistema de sessão
#[derive(Error, Debug)]
pub enum SessionError {
    /// Erro de IO
    #[error("Erro de IO: {0}")]
    Io(#[from] std::io::Error),

    /// Erro de serialização
    #[error("Erro de serialização: {0}")]
    Serialization(String),

    /// Erro de desserialização
    #[error("Erro de desserialização: {0}")]
    Deserialization(String),

    /// Sessão não encontrada
    #[error("Sessão não encontrada: {0}")]
    NotFound(String),

    /// Diretório de sessões não encontrado
    #[error("Diretório de sessões não encontrado: {0}")]
    DirectoryNotFound(PathBuf),

    /// Falha ao criar diretório
    #[error("Falha ao criar diretório: {0}")]
    CreateDirectoryFailed(PathBuf),

    /// Versão de formato não suportada
    #[error("Versão de formato não suportada: {version}")]
    UnsupportedVersion { version: u32 },

    /// Sessão corrompida
    #[error("Sessão corrompida: {0}")]
    Corrupted(String),

    /// Falha ao salvar sessão
    #[error("Falha ao salvar sessão: {0}")]
    SaveFailed(String),

    /// Falha ao carregar sessão
    #[error("Falha ao carregar sessão: {0}")]
    LoadFailed(String),

    /// Falha ao restaurar sessão
    #[error("Falha ao restaurar sessão: {0}")]
    RestoreFailed(String),

    /// Diretório home não encontrado
    #[error("Diretório home não encontrado")]
    HomeNotFound,

    /// Sessão já existe
    #[error("Sessão já existe: {0}")]
    AlreadyExists(String),

    /// Nome de sessão inválido
    #[error("Nome de sessão inválido: {0}")]
    InvalidName(String),

    /// Falha na migração
    #[error("Falha na migração: {0}")]
    MigrationFailed(String),
}

/// Resultado de operações de sessão
pub type SessionResult<T> = Result<T, SessionError>;
