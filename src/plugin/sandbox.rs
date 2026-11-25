//! Plugin Sandbox
//!
//! Implementa sandbox de segurança para execução de plugins.
//! Limita o acesso de plugins a recursos do sistema.
//!
//! # Features
//! - Isolamento de recursos (memória, CPU, arquivos)
//! - Permissões granulares
//! - Rate limiting
//! - Network host whitelist/blacklist
//! - WASM-specific limits (fuel, epoch)
//!
//! # Exemplo
//! ```rust,ignore
//! use code_terminal::plugin::sandbox::{Sandbox, SandboxConfig, SandboxConfigBuilder};
//! use code_terminal::plugin::manifest::Permission;
//!
//! let config = SandboxConfigBuilder::new()
//!     .allow(Permission::Notifications)
//!     .max_memory(32 * 1024 * 1024)
//!     .max_fuel(1_000_000)
//!     .build();
//!
//! let sandbox = Sandbox::new(config);
//! sandbox.check(SandboxOperation::MemoryAllocation(1024))?;
//! ```

use crate::plugin::error::{PluginError, PluginResult};
use crate::plugin::manifest::Permission;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

/// Configuração do sandbox
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Permissões concedidas
    pub permissions: HashSet<Permission>,
    /// Diretório base do sandbox
    pub base_dir: std::path::PathBuf,
    /// Memória máxima (bytes)
    pub max_memory: usize,
    /// CPU time máximo (ms)
    pub max_cpu_time_ms: u64,
    /// Tamanho máximo de arquivo (bytes)
    pub max_file_size: usize,
    /// Número máximo de arquivos abertos
    pub max_open_files: usize,
    /// Permitir execução de código nativo
    pub allow_native_code: bool,
    /// Permitir carregamento de bibliotecas dinâmicas
    pub allow_dylibs: bool,
    /// WASM fuel máximo (unidades de computação)
    pub max_fuel: u64,
    /// Intervalo de epoch para interrupção (ms)
    pub epoch_interval_ms: u64,
    /// Rate limit: operações por segundo
    pub rate_limit_per_sec: u32,
    /// Hosts permitidos para rede (vazio = nenhum, ["*"] = todos)
    pub allowed_hosts: HashSet<String>,
    /// Hosts bloqueados para rede
    pub blocked_hosts: HashSet<String>,
    /// Paths permitidos para filesystem (além do base_dir)
    pub allowed_paths: HashSet<std::path::PathBuf>,
    /// Timeout máximo para operações (ms)
    pub operation_timeout_ms: u64,
}

impl SandboxConfig {
    /// Cria configuração restritiva (padrão seguro)
    pub fn restrictive() -> Self {
        let mut blocked_hosts = HashSet::new();
        blocked_hosts.insert("localhost".to_string());
        blocked_hosts.insert("127.0.0.1".to_string());
        blocked_hosts.insert("::1".to_string());
        blocked_hosts.insert("0.0.0.0".to_string());

        Self {
            permissions: HashSet::new(),
            base_dir: std::env::temp_dir().join("code-terminal-plugins"),
            max_memory: 64 * 1024 * 1024,
            max_cpu_time_ms: 1000,
            max_file_size: 10 * 1024 * 1024,
            max_open_files: 10,
            allow_native_code: false,
            allow_dylibs: false,
            max_fuel: 10_000_000,
            epoch_interval_ms: 100,
            rate_limit_per_sec: 100,
            allowed_hosts: HashSet::new(),
            blocked_hosts,
            allowed_paths: HashSet::new(),
            operation_timeout_ms: 5000,
        }
    }

    /// Cria configuração moderada
    pub fn moderate() -> Self {
        let mut config = Self::restrictive();
        config.permissions.insert(Permission::ReadConfig);
        config.permissions.insert(Permission::Notifications);
        config.permissions.insert(Permission::CustomUi);
        config.max_memory = 128 * 1024 * 1024;
        config.max_cpu_time_ms = 5000;
        config.max_fuel = 50_000_000;
        config.rate_limit_per_sec = 500;
        config.operation_timeout_ms = 10000;
        config
    }

    /// Cria configuração permissiva (para desenvolvimento)
    pub fn permissive() -> Self {
        let mut config = Self::moderate();
        config.permissions.insert(Permission::Filesystem);
        config.permissions.insert(Permission::Network);
        config.permissions.insert(Permission::Clipboard);
        config.permissions.insert(Permission::Pty);
        config.max_memory = 256 * 1024 * 1024;
        config.max_cpu_time_ms = 30000;
        config.max_open_files = 100;
        config.max_fuel = 100_000_000;
        config.rate_limit_per_sec = 1000;
        config.allowed_hosts.insert("*".to_string());
        config.operation_timeout_ms = 30000;
        config
    }

    /// Adiciona uma permissão
    pub fn with_permission(mut self, permission: Permission) -> Self {
        self.permissions.insert(permission);
        self
    }

    /// Remove uma permissão
    pub fn without_permission(mut self, permission: Permission) -> Self {
        self.permissions.remove(&permission);
        self
    }

    /// Define o diretório base
    pub fn with_base_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.base_dir = dir.as_ref().to_path_buf();
        self
    }

    /// Verifica se tem uma permissão
    pub fn has_permission(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }

    /// Verifica se uma operação de filesystem é permitida
    pub fn check_filesystem_access(&self, path: impl AsRef<Path>) -> PluginResult<()> {
        if !self.has_permission(Permission::Filesystem) {
            return Err(PluginError::sandbox_violation(
                "filesystem",
                "Permission not granted",
            ));
        }

        let requested = path.as_ref();

        // `Path::starts_with` não resolve "..": "/base/../etc" passaria como se
        // estivesse dentro de "/base". Normaliza lexicamente antes de comparar.
        let path = normalize_lexically(requested).ok_or_else(|| {
            PluginError::sandbox_violation(
                "filesystem",
                format!("Path '{}' escapes the filesystem root", requested.display()),
            )
        })?;
        let path = path.as_path();

        let is_in_base_dir = path.starts_with(&self.base_dir);
        let is_in_allowed_paths = self.allowed_paths.iter().any(|p| path.starts_with(p));

        if !is_in_base_dir && !is_in_allowed_paths {
            return Err(PluginError::sandbox_violation(
                "filesystem",
                format!("Path '{}' is outside sandbox", path.display()),
            ));
        }

        Ok(())
    }

    /// Verifica se acesso à rede é permitido
    pub fn check_network_access(&self, host: Option<&str>) -> PluginResult<()> {
        if !self.has_permission(Permission::Network) {
            return Err(PluginError::sandbox_violation(
                "network",
                "Permission not granted",
            ));
        }

        if let Some(host) = host {
            if self.is_blocked_host(host) {
                return Err(PluginError::sandbox_violation(
                    "network",
                    format!("Host '{}' is blocked", host),
                ));
            }

            if !self.allowed_hosts.is_empty()
                && !self.allowed_hosts.contains("*")
                && !self.allowed_hosts.contains(host)
            {
                let host_parts: Vec<&str> = host.split('.').collect();
                let mut found = false;
                for i in 0..host_parts.len() {
                    let wildcard_host = host_parts[i..].join(".");
                    if self.allowed_hosts.contains(&format!("*.{}", wildcard_host)) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(PluginError::sandbox_violation(
                        "network",
                        format!("Host '{}' is not in whitelist", host),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Verifica se execução de shell é permitida
    pub fn check_shell_execution(&self) -> PluginResult<()> {
        if !self.has_permission(Permission::Shell) {
            return Err(PluginError::sandbox_violation(
                "shell",
                "Permission not granted",
            ));
        }
        Ok(())
    }

    /// Verifica se host está bloqueado
    fn is_blocked_host(&self, host: &str) -> bool {
        self.blocked_hosts
            .iter()
            .any(|blocked| host == blocked || host.ends_with(&format!(".{}", blocked)))
    }

    /// Adiciona host à whitelist
    pub fn allow_host(mut self, host: impl Into<String>) -> Self {
        self.allowed_hosts.insert(host.into());
        self
    }

    /// Adiciona host à blacklist
    pub fn block_host(mut self, host: impl Into<String>) -> Self {
        self.blocked_hosts.insert(host.into());
        self
    }

    /// Adiciona path permitido
    pub fn allow_path(mut self, path: impl AsRef<Path>) -> Self {
        self.allowed_paths.insert(path.as_ref().to_path_buf());
        self
    }

    /// Verifica se fuel está disponível
    pub fn check_fuel(&self, available: u64, required: u64) -> PluginResult<()> {
        if available < required {
            return Err(PluginError::sandbox_violation(
                "fuel",
                format!("Insufficient fuel: {} < {}", available, required),
            ));
        }
        Ok(())
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self::restrictive()
    }
}

/// Resolve `.` e `..` sem tocar no sistema de arquivos. Retorna `None` se o
/// caminho sobe acima da raiz (ou do início, se relativo).
pub(crate) fn normalize_lexically(path: &Path) -> Option<std::path::PathBuf> {
    use std::path::Component;

    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                );
                if !can_pop {
                    return None;
                }
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    Some(normalized)
}

/// Sandbox runtime para plugins
pub struct Sandbox {
    config: SandboxConfig,
    resource_usage: ResourceUsage,
    rate_limiter: RateLimiter,
    start_time: Instant,
    epoch: u64,
}

impl Sandbox {
    /// Cria um novo sandbox
    pub fn new(config: SandboxConfig) -> Self {
        Self {
            config,
            resource_usage: ResourceUsage::default(),
            rate_limiter: RateLimiter::new(),
            start_time: Instant::now(),
            epoch: 0,
        }
    }

    /// Verifica se uma operação é permitida
    pub fn check(&self, operation: SandboxOperation<'_>) -> PluginResult<()> {
        match operation {
            SandboxOperation::ReadFile(path) => self.config.check_filesystem_access(path),
            SandboxOperation::WriteFile(path) => self.config.check_filesystem_access(path),
            SandboxOperation::NetworkRequest { host } => self.config.check_network_access(host),
            SandboxOperation::ShellCommand => self.config.check_shell_execution(),
            SandboxOperation::MemoryAllocation(size) => self.check_memory(size),
            SandboxOperation::FileOpen => self.check_file_descriptors(),
            SandboxOperation::ConsumeFuel(amount) => self.check_fuel(amount),
            SandboxOperation::CpuTime(duration) => self.check_cpu_time(duration),
        }
    }

    /// Verifica se a operação está dentro do rate limit
    pub fn check_rate_limit(&mut self, operation: &str) -> PluginResult<()> {
        self.rate_limiter
            .check(operation, self.config.rate_limit_per_sec)
    }

    /// Consome fuel do sandbox
    pub fn consume_fuel(&mut self, amount: u64) -> PluginResult<()> {
        self.check_fuel(amount)?;
        self.resource_usage.fuel_consumed += amount;
        Ok(())
    }

    /// Retorna fuel restante
    pub fn remaining_fuel(&self) -> u64 {
        self.config
            .max_fuel
            .saturating_sub(self.resource_usage.fuel_consumed)
    }

    /// Incrementa epoch (para interrupção)
    pub fn increment_epoch(&mut self) {
        self.epoch += 1;
    }

    /// Retorna epoch atual
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Verifica se deve interromper baseado no epoch
    pub fn should_interrupt(&self) -> bool {
        let elapsed = self.start_time.elapsed().as_millis() as u64;
        elapsed >= self.config.epoch_interval_ms
    }

    /// Aloca memória no sandbox
    pub fn allocate_memory(&mut self, size: usize) -> PluginResult<()> {
        self.check_memory(size)?;
        self.resource_usage.memory_used += size;
        Ok(())
    }

    /// Libera memória do sandbox
    pub fn free_memory(&mut self, size: usize) {
        self.resource_usage.memory_used = self.resource_usage.memory_used.saturating_sub(size);
    }

    /// Abre um arquivo
    pub fn open_file(&mut self) -> PluginResult<()> {
        self.check_file_descriptors()?;
        self.resource_usage.open_files += 1;
        Ok(())
    }

    /// Fecha um arquivo
    pub fn close_file(&mut self) {
        self.resource_usage.open_files = self.resource_usage.open_files.saturating_sub(1);
    }

    /// Registra uso de CPU
    pub fn record_cpu_time(&mut self, duration: Duration) {
        self.resource_usage.cpu_time_ms += duration.as_millis() as u64;
    }

    /// Retorna o uso atual de recursos
    pub fn resource_usage(&self) -> &ResourceUsage {
        &self.resource_usage
    }

    /// Retorna a configuração do sandbox
    pub fn config(&self) -> &SandboxConfig {
        &self.config
    }

    /// Verifica se está dentro dos limites de memória
    fn check_memory(&self, requested: usize) -> PluginResult<()> {
        let new_usage = self.resource_usage.memory_used + requested;
        if new_usage > self.config.max_memory {
            return Err(PluginError::sandbox_violation(
                "memory",
                format!(
                    "Memory limit exceeded: {} > {}",
                    new_usage, self.config.max_memory
                ),
            ));
        }
        Ok(())
    }

    /// Verifica se está dentro dos limites de arquivos abertos
    fn check_file_descriptors(&self) -> PluginResult<()> {
        if self.resource_usage.open_files >= self.config.max_open_files {
            return Err(PluginError::sandbox_violation(
                "file_descriptors",
                format!(
                    "Too many open files: {} >= {}",
                    self.resource_usage.open_files, self.config.max_open_files
                ),
            ));
        }
        Ok(())
    }

    /// Verifica se há fuel suficiente
    fn check_fuel(&self, amount: u64) -> PluginResult<()> {
        let remaining = self.remaining_fuel();
        if remaining < amount {
            return Err(PluginError::sandbox_violation(
                "fuel",
                format!("Insufficient fuel: {} < {}", remaining, amount),
            ));
        }
        Ok(())
    }

    /// Verifica se está dentro do limite de CPU time
    fn check_cpu_time(&self, additional: Duration) -> PluginResult<()> {
        let new_total = self.resource_usage.cpu_time_ms + additional.as_millis() as u64;
        if new_total > self.config.max_cpu_time_ms {
            return Err(PluginError::sandbox_violation(
                "cpu_time",
                format!(
                    "CPU time limit exceeded: {}ms > {}ms",
                    new_total, self.config.max_cpu_time_ms
                ),
            ));
        }
        Ok(())
    }

    /// Finaliza o sandbox e limpa recursos
    pub fn shutdown(&mut self) {
        self.resource_usage = ResourceUsage::default();
        self.rate_limiter.reset();
        self.epoch = 0;
    }

    /// Reseta contadores mas mantém configuração
    pub fn reset(&mut self) {
        self.resource_usage = ResourceUsage::default();
        self.rate_limiter.reset();
        self.start_time = Instant::now();
    }
}

/// Operações que o sandbox pode verificar
#[derive(Debug, Clone)]
pub enum SandboxOperation<'a> {
    /// Ler arquivo
    ReadFile(&'a Path),
    /// Escrever arquivo
    WriteFile(&'a Path),
    /// Requisição de rede
    NetworkRequest { host: Option<&'a str> },
    /// Executar comando shell
    ShellCommand,
    /// Alocar memória
    MemoryAllocation(usize),
    /// Abrir arquivo
    FileOpen,
    /// Consumir fuel WASM
    ConsumeFuel(u64),
    /// Usar CPU time
    CpuTime(Duration),
}

/// Uso de recursos por um plugin
#[derive(Debug, Clone, Default)]
pub struct ResourceUsage {
    /// Memória usada (bytes)
    pub memory_used: usize,
    /// Arquivos abertos
    pub open_files: usize,
    /// Tempo de CPU usado (ms)
    pub cpu_time_ms: u64,
    /// Bytes lidos
    pub bytes_read: usize,
    /// Bytes escritos
    pub bytes_written: usize,
    /// Requisições de rede
    pub network_requests: usize,
    /// Fuel consumido (WASM)
    pub fuel_consumed: u64,
    /// Número de epochs passados
    pub epochs: u64,
}

/// Rate limiter para operações
#[derive(Debug, Clone, Default)]
pub struct RateLimiter {
    operations: HashMap<String, Vec<Instant>>,
}

impl RateLimiter {
    /// Cria novo rate limiter
    pub fn new() -> Self {
        Self {
            operations: HashMap::new(),
        }
    }

    /// Verifica se operação está dentro do rate limit
    pub fn check(&mut self, operation: &str, max_per_sec: u32) -> PluginResult<()> {
        let now = Instant::now();
        let cutoff = now - Duration::from_secs(1);

        let timestamps = self.operations.entry(operation.to_string()).or_default();

        timestamps.retain(|&t| t > cutoff);

        if timestamps.len() >= max_per_sec as usize {
            return Err(PluginError::sandbox_violation(
                "rate_limit",
                format!("Rate limit exceeded for operation: {}", operation),
            ));
        }

        timestamps.push(now);
        Ok(())
    }

    /// Reseta todos os contadores
    pub fn reset(&mut self) {
        self.operations.clear();
    }
}

/// Builder para SandboxConfig
pub struct SandboxConfigBuilder {
    config: SandboxConfig,
}

impl SandboxConfigBuilder {
    /// Cria novo builder com configuração restritiva
    pub fn new() -> Self {
        Self {
            config: SandboxConfig::restrictive(),
        }
    }

    /// Define permissões
    pub fn permissions(mut self, permissions: Vec<Permission>) -> Self {
        self.config.permissions = permissions.into_iter().collect();
        self
    }

    /// Adiciona permissão
    pub fn allow(mut self, permission: Permission) -> Self {
        self.config.permissions.insert(permission);
        self
    }

    /// Define diretório base
    pub fn base_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.config.base_dir = dir.as_ref().to_path_buf();
        self
    }

    /// Define limite de memória
    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.config.max_memory = bytes;
        self
    }

    /// Define limite de CPU
    pub fn max_cpu_time(mut self, ms: u64) -> Self {
        self.config.max_cpu_time_ms = ms;
        self
    }

    /// Define limite de fuel WASM
    pub fn max_fuel(mut self, fuel: u64) -> Self {
        self.config.max_fuel = fuel;
        self
    }

    /// Define intervalo de epoch
    pub fn epoch_interval(mut self, ms: u64) -> Self {
        self.config.epoch_interval_ms = ms;
        self
    }

    /// Define rate limit
    pub fn rate_limit(mut self, per_sec: u32) -> Self {
        self.config.rate_limit_per_sec = per_sec;
        self
    }

    /// Adiciona host permitido
    pub fn allow_host(mut self, host: impl Into<String>) -> Self {
        self.config.allowed_hosts.insert(host.into());
        self
    }

    /// Bloqueia host
    pub fn block_host(mut self, host: impl Into<String>) -> Self {
        self.config.blocked_hosts.insert(host.into());
        self
    }

    /// Adiciona path permitido
    pub fn allow_path(mut self, path: impl AsRef<Path>) -> Self {
        self.config
            .allowed_paths
            .insert(path.as_ref().to_path_buf());
        self
    }

    /// Define timeout de operação
    pub fn operation_timeout(mut self, ms: u64) -> Self {
        self.config.operation_timeout_ms = ms;
        self
    }

    /// Define limite de arquivos abertos
    pub fn max_open_files(mut self, count: usize) -> Self {
        self.config.max_open_files = count;
        self
    }

    /// Constrói a configuração
    pub fn build(self) -> SandboxConfig {
        self.config
    }
}

impl Default for SandboxConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_config_restrictive() {
        let config = SandboxConfig::restrictive();
        assert!(!config.has_permission(Permission::Filesystem));
        assert!(!config.has_permission(Permission::Network));
        assert!(!config.has_permission(Permission::Shell));
        assert_eq!(config.max_fuel, 10_000_000);
        assert!(config.blocked_hosts.contains("localhost"));
    }

    #[test]
    fn test_sandbox_config_moderate() {
        let config = SandboxConfig::moderate();
        assert!(!config.has_permission(Permission::Filesystem));
        assert!(config.has_permission(Permission::Notifications));
        assert!(config.has_permission(Permission::CustomUi));
        assert_eq!(config.max_fuel, 50_000_000);
    }

    #[test]
    fn test_sandbox_config_permissive() {
        let config = SandboxConfig::permissive();
        assert!(config.has_permission(Permission::Filesystem));
        assert!(config.has_permission(Permission::Clipboard));
        assert!(config.has_permission(Permission::Network));
        assert!(config.allowed_hosts.contains("*"));
    }

    #[test]
    fn test_sandbox_config_builder() {
        let config = SandboxConfigBuilder::new()
            .allow(Permission::Filesystem)
            .allow(Permission::Notifications)
            .max_memory(100 * 1024 * 1024)
            .max_fuel(1_000_000)
            .allow_host("api.example.com")
            .block_host("malicious.com")
            .build();

        assert!(config.has_permission(Permission::Filesystem));
        assert!(config.has_permission(Permission::Notifications));
        assert!(!config.has_permission(Permission::Network));
        assert_eq!(config.max_memory, 100 * 1024 * 1024);
        assert_eq!(config.max_fuel, 1_000_000);
        assert!(config.allowed_hosts.contains("api.example.com"));
        assert!(config.blocked_hosts.contains("malicious.com"));
    }

    #[test]
    fn test_sandbox_filesystem_check_denied() {
        let config = SandboxConfig::restrictive();
        let result = config.check_filesystem_access("/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_sandbox_network_check_denied() {
        let config = SandboxConfig::restrictive();
        let result = config.check_network_access(Some("example.com"));
        assert!(result.is_err());
    }

    #[test]
    fn test_sandbox_network_check_allowed() {
        let config = SandboxConfig::permissive();
        let result = config.check_network_access(Some("example.com"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_sandbox_blocked_host() {
        let config = SandboxConfig::permissive();
        let result = config.check_network_access(Some("localhost"));
        assert!(result.is_err());

        let result = config.check_network_access(Some("sub.localhost"));
        assert!(result.is_err());
    }

    #[test]
    fn test_sandbox_memory_allocation() {
        let config = SandboxConfig::restrictive();
        let mut sandbox = Sandbox::new(config);

        assert!(sandbox.allocate_memory(1024).is_ok());
        assert!(sandbox.allocate_memory(100 * 1024 * 1024).is_err());
    }

    #[test]
    fn test_resource_usage_default() {
        let usage = ResourceUsage::default();
        assert_eq!(usage.memory_used, 0);
        assert_eq!(usage.open_files, 0);
        assert_eq!(usage.cpu_time_ms, 0);
        assert_eq!(usage.fuel_consumed, 0);
    }

    #[test]
    fn test_sandbox_file_descriptors() {
        let config = SandboxConfig {
            max_open_files: 2,
            ..SandboxConfig::restrictive()
        };
        let mut sandbox = Sandbox::new(config);

        assert!(sandbox.open_file().is_ok());
        assert!(sandbox.open_file().is_ok());
        assert!(sandbox.open_file().is_err());

        sandbox.close_file();
        assert!(sandbox.open_file().is_ok());
    }

    #[test]
    fn test_sandbox_fuel_consumption() {
        let config = SandboxConfig {
            max_fuel: 1000,
            ..SandboxConfig::restrictive()
        };
        let mut sandbox = Sandbox::new(config);

        assert!(sandbox.consume_fuel(500).is_ok());
        assert_eq!(sandbox.remaining_fuel(), 500);

        assert!(sandbox.consume_fuel(300).is_ok());
        assert_eq!(sandbox.remaining_fuel(), 200);

        assert!(sandbox.consume_fuel(300).is_err());
        assert_eq!(sandbox.remaining_fuel(), 200);
    }

    #[test]
    fn test_sandbox_cpu_time_tracking() {
        let config = SandboxConfig {
            max_cpu_time_ms: 100,
            ..SandboxConfig::restrictive()
        };
        let sandbox = Sandbox::new(config);

        assert!(sandbox
            .check(SandboxOperation::CpuTime(Duration::from_millis(50)))
            .is_ok());
        assert!(sandbox
            .check(SandboxOperation::CpuTime(Duration::from_millis(150)))
            .is_err());
    }

    #[test]
    fn test_sandbox_epoch() {
        let config = SandboxConfig::restrictive();
        let mut sandbox = Sandbox::new(config);

        assert_eq!(sandbox.epoch(), 0);

        sandbox.increment_epoch();
        assert_eq!(sandbox.epoch(), 1);

        sandbox.increment_epoch();
        assert_eq!(sandbox.epoch(), 2);
    }

    #[test]
    fn test_rate_limiter() {
        let mut limiter = RateLimiter::new();

        for _ in 0..10 {
            assert!(limiter.check("test_op", 10).is_ok());
        }

        assert!(limiter.check("test_op", 10).is_err());
    }

    #[test]
    fn test_rate_limiter_reset() {
        let mut limiter = RateLimiter::new();

        for _ in 0..5 {
            assert!(limiter.check("test_op", 5).is_ok());
        }

        limiter.reset();

        for _ in 0..5 {
            assert!(limiter.check("test_op", 5).is_ok());
        }
    }

    #[test]
    fn test_sandbox_shutdown() {
        let config = SandboxConfig::restrictive();
        let mut sandbox = Sandbox::new(config);

        sandbox.allocate_memory(1024).unwrap();
        sandbox.consume_fuel(1000).unwrap();
        sandbox.increment_epoch();

        sandbox.shutdown();

        assert_eq!(sandbox.resource_usage().memory_used, 0);
        assert_eq!(sandbox.resource_usage().fuel_consumed, 0);
        assert_eq!(sandbox.epoch(), 0);
    }

    #[test]
    fn test_sandbox_reset() {
        let config = SandboxConfig::restrictive();
        let mut sandbox = Sandbox::new(config);

        sandbox.allocate_memory(1024).unwrap();
        sandbox.reset();

        assert_eq!(sandbox.resource_usage().memory_used, 0);
    }

    #[test]
    fn test_network_whitelist() {
        let config = SandboxConfigBuilder::new()
            .allow(Permission::Network)
            .allow_host("api.example.com")
            .allow_host("*.internal.company.com")
            .build();

        assert!(config.check_network_access(Some("api.example.com")).is_ok());
        assert!(config.check_network_access(Some("other.com")).is_err());
    }

    #[test]
    fn test_sandbox_check_operation() {
        let config = SandboxConfigBuilder::new()
            .allow(Permission::Network)
            .allow_host("example.com")
            .build();

        let sandbox = Sandbox::new(config);

        assert!(sandbox
            .check(SandboxOperation::NetworkRequest {
                host: Some("example.com")
            })
            .is_ok());

        assert!(sandbox
            .check(SandboxOperation::NetworkRequest {
                host: Some("localhost")
            })
            .is_err());

        assert!(sandbox.check(SandboxOperation::ShellCommand).is_err());
    }

    fn fs_config() -> SandboxConfig {
        SandboxConfig::restrictive()
            .with_permission(Permission::Filesystem)
            .with_base_dir("/sandbox/plugin")
            .allow_path("/tmp/shared")
    }

    /// Regressão: "/base/../etc/passwd" começava com "/base" e escapava do
    /// sandbox.
    #[test]
    fn test_filesystem_rejects_parent_dir_traversal() {
        let config = fs_config();
        assert!(config
            .check_filesystem_access("/sandbox/plugin/../../etc/passwd")
            .is_err());
        assert!(config
            .check_filesystem_access("/tmp/shared/../../root/.ssh/id_rsa")
            .is_err());
        assert!(config.check_filesystem_access("/../../etc").is_err());
        let err = config.check_filesystem_access("../x").unwrap_err();
        assert!(err.to_string().contains("escapes"), "{err}");

        // ".." que permanece dentro do sandbox continua permitido
        config
            .check_filesystem_access("/sandbox/plugin/a/../b/./c.txt")
            .unwrap();
        config.check_filesystem_access("/tmp/shared/x").unwrap();
        assert!(config.check_filesystem_access("/tmp/outro").is_err());
    }

    #[test]
    fn test_normalize_lexically() {
        assert_eq!(
            normalize_lexically(Path::new("/a/./b/../c")),
            Some(std::path::PathBuf::from("/a/c"))
        );
        assert_eq!(normalize_lexically(Path::new("/..")), None);
        assert_eq!(
            normalize_lexically(Path::new("a/../b")),
            Some(std::path::PathBuf::from("b"))
        );
        assert_eq!(normalize_lexically(Path::new("a/../..")), None);
    }

    #[test]
    fn test_permission_toggles_and_shell() {
        let config = SandboxConfig::default().with_permission(Permission::Shell);
        config.check_shell_execution().unwrap();
        let config = config.without_permission(Permission::Shell);
        let err = config.check_shell_execution().unwrap_err();
        assert!(err.to_string().contains("Permission not granted"), "{err}");
    }

    #[test]
    fn test_network_wildcards_and_blocking() {
        let config = SandboxConfig::restrictive()
            .with_permission(Permission::Network)
            .allow_host("*.example.com")
            .allow_host("api.test")
            .block_host("evil.com");
        config.check_network_access(Some("api.test")).unwrap();
        config
            .check_network_access(Some("a.b.example.com"))
            .unwrap();
        config.check_network_access(None).unwrap();
        assert!(config.check_network_access(Some("other.org")).is_err());
        // Subdomínio de host bloqueado
        let err = config.check_network_access(Some("x.evil.com")).unwrap_err();
        assert!(err.to_string().contains("blocked"), "{err}");
    }

    #[test]
    fn test_static_fuel_check_and_rate_limit_through_sandbox() {
        let config = SandboxConfig::restrictive();
        config.check_fuel(10, 5).unwrap();
        assert!(config.check_fuel(1, 5).is_err());

        let mut sandbox = Sandbox::new(SandboxConfigBuilder::new().rate_limit(2).build());
        sandbox.check_rate_limit("op").unwrap();
        sandbox.check_rate_limit("op").unwrap();
        assert!(sandbox.check_rate_limit("op").is_err());
        // Outra operação tem contador próprio
        sandbox.check_rate_limit("outra").unwrap();
    }

    #[test]
    fn test_should_interrupt_after_epoch_interval() {
        let sandbox = Sandbox::new(SandboxConfigBuilder::new().epoch_interval(0).build());
        assert!(sandbox.should_interrupt());
        let sandbox = Sandbox::new(SandboxConfigBuilder::new().epoch_interval(60_000).build());
        assert!(!sandbox.should_interrupt());
    }

    #[test]
    fn test_check_operations_cover_all_variants() {
        let config = SandboxConfigBuilder::default()
            .permissions(vec![Permission::Filesystem])
            .allow(Permission::Shell)
            .base_dir("/b")
            .max_memory(100)
            .max_cpu_time(50)
            .max_fuel(10)
            .operation_timeout(5)
            .max_open_files(1)
            .allow_host("h.test")
            .block_host("bad.test")
            .allow_path("/p")
            .build();
        assert_eq!(config.operation_timeout_ms, 5);
        assert!(config.allowed_paths.contains(Path::new("/p")));
        let mut sandbox = Sandbox::new(config);

        sandbox
            .check(SandboxOperation::ReadFile(Path::new("/b/x")))
            .unwrap();
        assert!(sandbox
            .check(SandboxOperation::WriteFile(Path::new("/b/../x")))
            .is_err());
        assert!(sandbox
            .check(SandboxOperation::NetworkRequest {
                host: Some("h.test")
            })
            .is_err()); // sem permissão de rede
        sandbox.check(SandboxOperation::ShellCommand).unwrap();
        assert!(sandbox
            .check(SandboxOperation::MemoryAllocation(101))
            .is_err());
        sandbox.check(SandboxOperation::FileOpen).unwrap();
        assert!(sandbox.check(SandboxOperation::ConsumeFuel(11)).is_err());
        assert!(sandbox
            .check(SandboxOperation::CpuTime(Duration::from_millis(51)))
            .is_err());

        sandbox.open_file().unwrap();
        let err = sandbox.open_file().unwrap_err();
        assert!(err.to_string().contains("Too many open files"), "{err}");
        sandbox.close_file();
        sandbox.close_file(); // saturante
        assert_eq!(sandbox.resource_usage().open_files, 0);
        assert_eq!(sandbox.config().max_memory, 100);
    }
}
