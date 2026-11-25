//! Validações e verificações de segurança para SSH
//!
//! Este módulo fornece funcionalidades para validar configurações SSH,
//! verificar segurança de chaves, algoritmos criptográficos e detectar
//! vulnerabilidades conhecidas.
//!
//! # Exemplos
//!
//! ## Verificar segurança de uma chave
//!
//! ```rust,no_run
//! use code_terminal::ssh::{KeyManager, security::SecurityValidator};
//!
//! let mut manager = KeyManager::new();
//! let validator = SecurityValidator::new();
//!
//! let keys = manager.discover_keys().unwrap();
//! for key in keys {
//!     let report = validator.validate_key(&key);
//!     println!("{}: {} vulnerabilidades encontradas", key.name, report.issues.len());
//! }
//! ```
//!
//! ## Verificar configuração de host
//!
//! ```rust,no_run
//! use code_terminal::ssh::{SshConfigParser, security::SecurityValidator};
//!
//! let parser = SshConfigParser::parse_default_config().unwrap();
//! let validator = SecurityValidator::new();
//!
//! if let Some(host) = parser.get_effective_config("myserver") {
//!     let report = validator.validate_host(&host);
//!     for warning in &report.warnings {
//!         println!("⚠️  {}", warning);
//!     }
//! }
//! ```

use crate::ssh::error::{SshError, SshKeyError, SshResult};
use crate::ssh::host::SshHost;
use crate::ssh::key::{KeyType, SshKey};
use std::collections::HashSet;
use std::path::Path;

/// Algoritmos criptográficos recomendados
pub mod algorithms {
    /// Key Exchange (KEX) algorithms recomendados
    pub const RECOMMENDED_KEX: &[&str] = &[
        "curve25519-sha256",
        "curve25519-sha256@libssh.org",
        "ecdh-sha2-nistp521",
        "ecdh-sha2-nistp384",
        "diffie-hellman-group16-sha512",
        "diffie-hellman-group18-sha512",
    ];

    /// Host key algorithms recomendados
    pub const RECOMMENDED_HOST_KEY: &[&str] = &[
        "ssh-ed25519",
        "ssh-ed25519-cert-v01@openssh.com",
        "ecdsa-sha2-nistp521",
        "ecdsa-sha2-nistp384",
        "rsa-sha2-512",
        "rsa-sha2-256",
    ];

    /// Ciphers recomendados
    pub const RECOMMENDED_CIPHERS: &[&str] = &[
        "aes256-gcm@openssh.com",
        "aes128-gcm@openssh.com",
        "aes256-ctr",
        "aes192-ctr",
        "aes128-ctr",
        "chacha20-poly1305@openssh.com",
    ];

    /// MACs recomendados
    pub const RECOMMENDED_MACS: &[&str] = &[
        "hmac-sha2-512-etm@openssh.com",
        "hmac-sha2-256-etm@openssh.com",
        "hmac-sha2-512",
        "hmac-sha2-256",
        "umac-128-etm@openssh.com",
    ];

    /// Algoritmos obsoletos ou inseguros
    pub const DEPRECATED_ALGORITHMS: &[&str] = &[
        "diffie-hellman-group1-sha1",
        "diffie-hellman-group14-sha1",
        "diffie-hellman-group-exchange-sha1",
        "rsa-sha",
        "ssh-dss",
        "ssh-rsa",
        "aes128-cbc",
        "aes192-cbc",
        "aes256-cbc",
        "3des-cbc",
        "blowfish-cbc",
        "cast128-cbc",
        "arcfour",
        "arcfour128",
        "arcfour256",
        "hmac-sha1",
        "hmac-sha1-96",
        "hmac-md5",
        "hmac-md5-96",
        "hmac-ripemd160",
        "umac-64@openssh.com",
        "umac-128@openssh.com",
    ];
}

/// Nível de severidade de uma vulnerabilidade
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informação, não requer ação
    Info,
    /// Baixa prioridade
    Low,
    /// Média prioridade
    Medium,
    /// Alta prioridade
    High,
    /// Crítico, requer ação imediata
    Critical,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Severity::Info => write!(f, "INFO"),
            Severity::Low => write!(f, "LOW"),
            Severity::Medium => write!(f, "MEDIUM"),
            Severity::High => write!(f, "HIGH"),
            Severity::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Problema de segurança encontrado
#[derive(Debug, Clone)]
pub struct SecurityIssue {
    /// Severidade do problema
    pub severity: Severity,
    /// Código do problema
    pub code: String,
    /// Descrição curta
    pub message: String,
    /// Descrição detalhada
    pub description: String,
    /// Recomendação de correção
    pub recommendation: String,
}

impl SecurityIssue {
    /// Cria um novo problema de segurança
    pub fn new(
        severity: Severity,
        code: impl Into<String>,
        message: impl Into<String>,
        description: impl Into<String>,
        recommendation: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            description: description.into(),
            recommendation: recommendation.into(),
        }
    }
}

/// Relatório de segurança
#[derive(Debug, Clone, Default)]
pub struct SecurityReport {
    /// Problemas encontrados
    pub issues: Vec<SecurityIssue>,
    /// Avisos (menos severos)
    pub warnings: Vec<String>,
    /// Recomendações
    pub recommendations: Vec<String>,
    /// Score de segurança (0-100)
    pub score: u8,
}

impl SecurityReport {
    /// Cria um novo relatório vazio
    pub fn new() -> Self {
        Self {
            issues: Vec::new(),
            warnings: Vec::new(),
            recommendations: Vec::new(),
            score: 100,
        }
    }

    /// Adiciona um problema ao relatório
    pub fn add_issue(&mut self, issue: SecurityIssue) {
        // Ajusta o score baseado na severidade
        let penalty = match issue.severity {
            Severity::Critical => 40,
            Severity::High => 25,
            Severity::Medium => 15,
            Severity::Low => 5,
            Severity::Info => 0,
        };
        self.score = self.score.saturating_sub(penalty);
        self.issues.push(issue);
    }

    /// Adiciona um aviso
    pub fn add_warning(&mut self, warning: impl Into<String>) {
        self.warnings.push(warning.into());
    }

    /// Adiciona uma recomendação
    pub fn add_recommendation(&mut self, recommendation: impl Into<String>) {
        self.recommendations.push(recommendation.into());
    }

    /// Retorna problemas por severidade
    pub fn issues_by_severity(&self, severity: Severity) -> Vec<&SecurityIssue> {
        self.issues
            .iter()
            .filter(|i| i.severity == severity)
            .collect()
    }

    /// Verifica se há problemas críticos
    pub fn has_critical(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Critical)
    }

    /// Verifica se há problemas de alta severidade
    pub fn has_high(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::High)
    }
}

/// Validador de segurança SSH
#[derive(Debug, Clone)]
pub struct SecurityValidator {
    /// Algoritmos KEX permitidos
    allowed_kex: HashSet<String>,
    /// Algoritmos de host key permitidos
    allowed_host_key: HashSet<String>,
    /// Ciphers permitidos
    allowed_ciphers: HashSet<String>,
    /// MACs permitidos
    allowed_macs: HashSet<String>,
    /// Algoritmos proibidos
    forbidden_algorithms: HashSet<String>,
}

impl Default for SecurityValidator {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityValidator {
    /// Cria um novo validador com configurações padrão seguras
    pub fn new() -> Self {
        let mut validator = Self {
            allowed_kex: HashSet::new(),
            allowed_host_key: HashSet::new(),
            allowed_ciphers: HashSet::new(),
            allowed_macs: HashSet::new(),
            forbidden_algorithms: HashSet::new(),
        };

        // Configura algoritmos recomendados
        for algo in algorithms::RECOMMENDED_KEX {
            validator.allowed_kex.insert(algo.to_string());
        }
        for algo in algorithms::RECOMMENDED_HOST_KEY {
            validator.allowed_host_key.insert(algo.to_string());
        }
        for algo in algorithms::RECOMMENDED_CIPHERS {
            validator.allowed_ciphers.insert(algo.to_string());
        }
        for algo in algorithms::RECOMMENDED_MACS {
            validator.allowed_macs.insert(algo.to_string());
        }
        for algo in algorithms::DEPRECATED_ALGORITHMS {
            validator.forbidden_algorithms.insert(algo.to_string());
        }

        validator
    }

    /// Valida uma chave SSH
    pub fn validate_key(&self, key: &SshKey) -> SecurityReport {
        let mut report = SecurityReport::new();

        // Verifica tipo de chave
        self.check_key_type(key, &mut report);

        // Verifica tamanho da chave
        self.check_key_size(key, &mut report);

        // Verifica idade da chave (se disponível)
        self.check_key_age(key, &mut report);

        // Verifica passphrase
        self.check_key_passphrase(key, &mut report);

        // Verifica permissões do arquivo
        self.check_key_permissions(key, &mut report);

        report
    }

    /// Valida configuração de um host
    pub fn validate_host(&self, host: &SshHost) -> SecurityReport {
        let mut report = SecurityReport::new();

        // Verifica algoritmos configurados
        self.check_host_algorithms(host, &mut report);

        // Verifica autenticação
        self.check_host_authentication(host, &mut report);

        // Verifica forwarding
        self.check_host_forwarding(host, &mut report);

        // Verifica StrictHostKeyChecking
        self.check_host_key_checking(host, &mut report);

        report
    }

    /// Verifica tipo de chave
    fn check_key_type(&self, key: &SshKey, report: &mut SecurityReport) {
        match key.key_type {
            KeyType::Dsa => {
                report.add_issue(SecurityIssue::new(
                    Severity::Critical,
                    "KEY_DEPRECATED_TYPE",
                    "Chave DSA detectada",
                    "DSA é um algoritmo obsoleto e não é mais considerado seguro. Foi removido do OpenSSH 7.0+",
                    "Gere uma nova chave Ed25519 ou RSA-4096 e substitua esta chave.",
                ));
            }
            KeyType::Rsa => {
                if key.bits.map(|b| b < 2048).unwrap_or(false) {
                    report.add_issue(SecurityIssue::new(
                        Severity::High,
                        "KEY_SMALL_RSA",
                        "Chave RSA muito pequena",
                        format!(
                            "Chave RSA de {} bits detectada. RSA deve ter pelo menos 2048 bits.",
                            key.bits.unwrap_or(0)
                        ),
                        "Gere uma nova chave RSA de 4096 bits ou prefira Ed25519.",
                    ));
                }
            }
            KeyType::EcdsaP256 | KeyType::EcdsaP384 | KeyType::EcdsaP521 => {
                report.add_warning("ECDSA é seguro, mas Ed25519 é geralmente preferido por ser mais rápido e ter propriedades melhores.");
            }
            KeyType::Ed25519 => {
                // Ed25519 é o recomendado
            }
        }
    }

    /// Verifica tamanho da chave
    fn check_key_size(&self, key: &SshKey, report: &mut SecurityReport) {
        if let Some(bits) = key.bits {
            let min_bits = match key.key_type {
                KeyType::Rsa => 2048,
                KeyType::Ed25519 => 256,
                KeyType::EcdsaP256 => 256,
                KeyType::EcdsaP384 => 384,
                KeyType::EcdsaP521 => 521,
                KeyType::Dsa => 1024,
            };

            if bits < min_bits {
                report.add_issue(SecurityIssue::new(
                    Severity::High,
                    "KEY_INSUFFICIENT_BITS",
                    format!("Chave muito curta: {} bits", bits),
                    format!(
                        "A chave tem {} bits, mas o mínimo recomendado para {:?} é {} bits",
                        bits, key.key_type, min_bits
                    ),
                    "Gere uma nova chave com tamanho adequado.",
                ));
            }
        }
    }

    /// Verifica idade da chave
    fn check_key_age(&self, _key: &SshKey, _report: &mut SecurityReport) {
        // Nota: A verificação de idade da chave requer metadata do sistema de arquivos
        // ou um campo created_at na estrutura SshKey. Por padrão, esta validação
        // é um placeholder para implementação futura.
        //
        // Implementação futura:
        // if let Some(created) = key.created_at {
        //     if let Ok(age) = SystemTime::now().duration_since(created) {
        //         let years = age.as_secs() / (365 * 24 * 60 * 60);
        //         if years >= 5 { report.add_issue(...); }
        //     }
        // }
    }

    /// Verifica passphrase
    fn check_key_passphrase(&self, key: &SshKey, report: &mut SecurityReport) {
        if !key.encrypted {
            report.add_issue(SecurityIssue::new(
                Severity::High,
                "KEY_NO_PASSPHRASE",
                "Chave sem passphrase",
                "Chaves SSH sem passphrase estão desprotegidas se o arquivo for comprometido.",
                "Adicione uma passphrase à chave usando: ssh-keygen -p -f <chave>",
            ));
        }
    }

    /// Verifica permissões do arquivo
    #[cfg_attr(not(unix), allow(unused_variables))]
    fn check_key_permissions(&self, key: &SshKey, report: &mut SecurityReport) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Ok(metadata) = std::fs::metadata(&key.private_key_path) {
                let mode = metadata.permissions().mode() & 0o777;

                if mode != 0o600 {
                    if mode & 0o077 != 0 {
                        report.add_issue(SecurityIssue::new(
                            Severity::High,
                            "KEY_WORLD_ACCESSIBLE",
                            "Chave acessível por outros usuários",
                            format!("Permissões atuais: {:o}. A chave pode ser lida por outros usuários.", mode),
                            format!("Corrija as permissões: chmod 600 {}", key.private_key_path.display()),
                        ));
                    } else {
                        report.add_warning(format!(
                            "Permissões da chave são {:o}, recomendado: 600",
                            mode
                        ));
                    }
                }
            }
        }
    }

    /// Verifica algoritmos do host
    fn check_host_algorithms(&self, host: &SshHost, report: &mut SecurityReport) {
        // Verifica KEX algorithms
        if let Some(ref kex) = host.kex_algorithms {
            self.check_algorithm_list(kex, "KEX", &self.allowed_kex, report);
        }

        // Verifica host key algorithms
        if let Some(ref host_key) = host.host_key_algorithms {
            self.check_algorithm_list(host_key, "HostKey", &self.allowed_host_key, report);
        }

        // Verifica ciphers
        if let Some(ref ciphers) = host.ciphers {
            self.check_algorithm_list(ciphers, "Cipher", &self.allowed_ciphers, report);
        }

        // Verifica MACs
        if let Some(ref macs) = host.macs {
            self.check_algorithm_list(macs, "MAC", &self.allowed_macs, report);
        }
    }

    /// Verifica lista de algoritmos
    fn check_algorithm_list(
        &self,
        algorithms: &str,
        algo_type: &str,
        allowed: &HashSet<String>,
        report: &mut SecurityReport,
    ) {
        for algo in algorithms.split(',') {
            let algo = algo.trim();

            if self.forbidden_algorithms.contains(algo) {
                report.add_issue(SecurityIssue::new(
                    Severity::High,
                    format!("ALGO_DEPRECATED_{}", algo_type.to_uppercase()),
                    format!("Algoritmo {} inseguro: {}", algo_type, algo),
                    "Este algoritmo é conhecido por ter vulnerabilidades de segurança.",
                    format!(
                        "Remova '{}' da lista de {}.",
                        algo,
                        algo_type.to_lowercase()
                    ),
                ));
            } else if !allowed.contains(algo) {
                report.add_warning(format!(
                    "Algoritmo {} '{}' não está na lista de recomendados",
                    algo_type, algo
                ));
            }
        }
    }

    /// Verifica configurações de autenticação
    fn check_host_authentication(&self, host: &SshHost, report: &mut SecurityReport) {
        // Verifica se autenticação por senha está habilitada
        if host.password_authentication == Some(true) {
            report.add_warning(
                "Autenticação por senha está habilitada. Considere usar apenas chaves SSH."
                    .to_string(),
            );
        }

        // Verifica se autenticação por chave pública está habilitada
        if host.pub_key_authentication == Some(false) {
            report.add_issue(SecurityIssue::new(
                Severity::Medium,
                "AUTH_PUBKEY_DISABLED",
                "Autenticação por chave pública desabilitada",
                "Autenticação por chave pública é mais segura que por senha.",
                "Habilite PubkeyAuthentication no host.",
            ));
        }

        // Verifica PreferredAuthentications
        if let Some(ref preferred) = host.preferred_authentications {
            if preferred.contains("password") && !preferred.starts_with("publickey") {
                report.add_warning(
                    "Password authentication está antes de publickey em PreferredAuthentications"
                        .to_string(),
                );
            }
        }
    }

    /// Verifica configurações de forwarding
    fn check_host_forwarding(&self, host: &SshHost, report: &mut SecurityReport) {
        // Verifica agent forwarding
        if host.forward_agent == Some(true) {
            report.add_warning(
                "Agent forwarding está habilitado. Use com cautela em hosts não confiáveis."
                    .to_string(),
            );
        }

        // Verifica X11 forwarding
        if host.forward_x11 == Some(true) {
            report.add_issue(SecurityIssue::new(
                Severity::Medium,
                "FWD_X11_ENABLED",
                "X11 forwarding habilitado",
                "X11 forwarding pode expor o servidor a riscos de segurança.",
                "Desabilite X11 forwarding se não for necessário.",
            ));
        }

        // Verifica GatewayPorts
        if host.gateway_ports == Some(true) {
            report.add_warning(
                "GatewayPorts está habilitado. Port forwards estarão acessíveis externamente."
                    .to_string(),
            );
        }
    }

    /// Verifica StrictHostKeyChecking
    fn check_host_key_checking(&self, host: &SshHost, report: &mut SecurityReport) {
        use crate::ssh::host::StrictHostKeyChecking;

        match host.strict_host_key_checking {
            Some(StrictHostKeyChecking::No) => {
                report.add_issue(SecurityIssue::new(
                    Severity::Critical,
                    "HOSTKEY_CHECK_DISABLED",
                    "Verificação de host key desabilitada",
                    "StrictHostKeyChecking no desabilita a verificação de host keys, tornando vulnerável a ataques MITM.",
                    "Remova StrictHostKeyChecking=no ou use 'accept-new'.",
                ));
            }
            Some(StrictHostKeyChecking::AcceptNew) => {
                report.add_warning(
                    "StrictHostKeyChecking está em 'accept-new'. Hosts novos serão aceitos automaticamente.".to_string()
                );
            }
            _ => {}
        }
    }

    /// Valida permissões de arquivo
    #[cfg_attr(not(unix), allow(unused_variables))]
    pub fn validate_file_permissions(path: &Path) -> Result<bool, SshKeyError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let metadata = std::fs::metadata(path).map_err(SshKeyError::ReadError)?;
            let mode = metadata.permissions().mode() & 0o777;

            Ok(mode == 0o600)
        }

        #[cfg(not(unix))]
        {
            Ok(true)
        }
    }

    /// Verifica se um algoritmo é seguro
    pub fn is_algorithm_secure(&self, algorithm: &str) -> bool {
        !self.forbidden_algorithms.contains(algorithm)
    }

    /// Retorna lista de algoritmos recomendados
    pub fn get_recommended_algorithms(&self) -> RecommendedAlgorithms {
        RecommendedAlgorithms {
            kex: algorithms::RECOMMENDED_KEX.to_vec(),
            host_key: algorithms::RECOMMENDED_HOST_KEY.to_vec(),
            ciphers: algorithms::RECOMMENDED_CIPHERS.to_vec(),
            macs: algorithms::RECOMMENDED_MACS.to_vec(),
        }
    }
}

/// Algoritmos recomendados
#[derive(Debug, Clone)]
pub struct RecommendedAlgorithms {
    pub kex: Vec<&'static str>,
    pub host_key: Vec<&'static str>,
    pub ciphers: Vec<&'static str>,
    pub macs: Vec<&'static str>,
}

impl RecommendedAlgorithms {
    /// Gera string de configuração SSH
    pub fn to_ssh_config(&self) -> String {
        format!(
            "KexAlgorithms {}\nHostKeyAlgorithms {}\nCiphers {}\nMACs {}",
            self.kex.join(","),
            self.host_key.join(","),
            self.ciphers.join(","),
            self.macs.join(",")
        )
    }
}

/// Auditor de segurança SSH
#[derive(Debug)]
pub struct SecurityAuditor {
    validator: SecurityValidator,
}

impl Default for SecurityAuditor {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityAuditor {
    /// Cria um novo auditor
    pub fn new() -> Self {
        Self {
            validator: SecurityValidator::new(),
        }
    }

    /// Retorna o validador usado pelas auditorias.
    pub fn validator(&self) -> &SecurityValidator {
        &self.validator
    }

    /// Executa auditoria completa
    pub fn audit(&self, ssh_dir: &Path) -> Result<SecurityAuditReport, SshError> {
        let mut report = SecurityAuditReport {
            overall_score: 100,
            key_reports: Vec::new(),
            config_issues: Vec::new(),
            recommendations: Vec::new(),
        };

        // Verifica diretório .ssh
        self.check_ssh_directory(ssh_dir, &mut report)?;

        // Verifica authorized_keys
        self.check_authorized_keys(ssh_dir, &mut report)?;

        // Verifica known_hosts
        self.check_known_hosts(ssh_dir, &mut report)?;

        // Verifica config
        self.check_config_file(ssh_dir, &mut report)?;

        // Calcula score geral
        report.calculate_overall_score();

        Ok(report)
    }

    /// Verifica diretório .ssh
    fn check_ssh_directory(
        &self,
        ssh_dir: &Path,
        report: &mut SecurityAuditReport,
    ) -> SshResult<()> {
        if !ssh_dir.exists() {
            report
                .config_issues
                .push("Diretório ~/.ssh não existe".to_string());
            return Ok(());
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let metadata = std::fs::metadata(ssh_dir).map_err(SshError::IoError)?;
            let mode = metadata.permissions().mode() & 0o777;

            if mode != 0o700 {
                report.config_issues.push(format!(
                    "Permissões do diretório ~/.ssh são {:o}, deveriam ser 700",
                    mode
                ));
            }
        }

        Ok(())
    }

    /// Verifica authorized_keys
    fn check_authorized_keys(
        &self,
        ssh_dir: &Path,
        report: &mut SecurityAuditReport,
    ) -> SshResult<()> {
        let auth_keys = ssh_dir.join("authorized_keys");

        if auth_keys.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let metadata = std::fs::metadata(&auth_keys).map_err(SshError::IoError)?;
                let mode = metadata.permissions().mode() & 0o777;

                if mode != 0o600 {
                    report.config_issues.push(format!(
                        "Permissões de authorized_keys são {:o}, deveriam ser 600",
                        mode
                    ));
                }
            }

            // Verifica por chaves obsoletas
            if let Ok(content) = std::fs::read_to_string(&auth_keys) {
                for (line_num, line) in content.lines().enumerate() {
                    if line.contains("ssh-dss") {
                        report.config_issues.push(format!(
                            "authorized_keys linha {}: Chave DSA encontrada (obsoleta)",
                            line_num + 1
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Verifica known_hosts
    #[cfg_attr(not(unix), allow(unused_variables))]
    fn check_known_hosts(&self, ssh_dir: &Path, report: &mut SecurityAuditReport) -> SshResult<()> {
        let known_hosts = ssh_dir.join("known_hosts");

        if known_hosts.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let metadata = std::fs::metadata(&known_hosts).map_err(SshError::IoError)?;
                let mode = metadata.permissions().mode() & 0o777;

                if mode != 0o644 && mode != 0o600 {
                    report.config_issues.push(format!(
                        "Permissões de known_hosts são {:o}, recomendado 644 ou 600",
                        mode
                    ));
                }
            }
        }

        Ok(())
    }

    /// Verifica arquivo de configuração
    #[cfg_attr(not(unix), allow(unused_variables))]
    fn check_config_file(&self, ssh_dir: &Path, report: &mut SecurityAuditReport) -> SshResult<()> {
        let config = ssh_dir.join("config");

        if config.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let metadata = std::fs::metadata(&config).map_err(SshError::IoError)?;
                let mode = metadata.permissions().mode() & 0o777;

                if mode != 0o600 {
                    report.config_issues.push(format!(
                        "Permissões de config são {:o}, deveriam ser 600",
                        mode
                    ));
                }
            }
        }

        Ok(())
    }
}

/// Relatório de auditoria de segurança
#[derive(Debug, Clone, Default)]
pub struct SecurityAuditReport {
    /// Score geral (0-100)
    pub overall_score: u8,
    /// Relatórios de chaves
    pub key_reports: Vec<(String, SecurityReport)>,
    /// Problemas de configuração
    pub config_issues: Vec<String>,
    /// Recomendações gerais
    pub recommendations: Vec<String>,
}

impl SecurityAuditReport {
    /// Calcula o score geral
    fn calculate_overall_score(&mut self) {
        let mut total = 100u8;

        // Penalidade por problemas de configuração
        // (calculado em usize: `len() as u8 * 5` estourava com 52+ problemas)
        let config_penalty = self.config_issues.len().saturating_mul(5).min(100) as u8;
        total = total.saturating_sub(config_penalty);

        // Penalidade por problemas nas chaves
        for (_, report) in &self.key_reports {
            total = total.saturating_sub((100 - report.score) / 4);
        }

        self.overall_score = total;
    }

    /// Verifica se a auditoria passou
    pub fn passed(&self) -> bool {
        self.overall_score >= 70 && !self.has_critical_issues()
    }

    /// Verifica se há problemas críticos
    pub fn has_critical_issues(&self) -> bool {
        self.key_reports.iter().any(|(_, r)| r.has_critical())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_security_validator_creation() {
        let validator = SecurityValidator::new();
        assert!(!validator.allowed_kex.is_empty());
        assert!(!validator.forbidden_algorithms.is_empty());
    }

    #[test]
    fn test_is_algorithm_secure() {
        let validator = SecurityValidator::new();
        assert!(validator.is_algorithm_secure("ssh-ed25519"));
        assert!(!validator.is_algorithm_secure("ssh-dss"));
    }

    #[test]
    fn test_security_report() {
        let mut report = SecurityReport::new();
        assert_eq!(report.score, 100);
        assert!(!report.has_critical());

        report.add_issue(SecurityIssue::new(
            Severity::High,
            "TEST",
            "Test issue",
            "Description",
            "Fix it",
        ));

        assert!(report.score < 100);
    }

    #[test]
    fn test_recommended_algorithms() {
        let validator = SecurityValidator::new();
        let algos = validator.get_recommended_algorithms();

        assert!(!algos.kex.is_empty());
        assert!(!algos.host_key.is_empty());
        assert!(!algos.ciphers.is_empty());
        assert!(!algos.macs.is_empty());

        let config = algos.to_ssh_config();
        assert!(config.contains("KexAlgorithms"));
        assert!(config.contains("HostKeyAlgorithms"));
    }

    #[test]
    fn test_key_type_check_dsa() {
        let validator = SecurityValidator::new();
        let key = SshKey::new("test".to_string(), KeyType::Dsa, PathBuf::from("/tmp/test"))
            .with_bits(1024);

        let report = validator.validate_key(&key);
        assert!(report.has_critical());
    }

    #[test]
    fn test_key_type_check_rsa_small() {
        let validator = SecurityValidator::new();
        let key = SshKey::new("test".to_string(), KeyType::Rsa, PathBuf::from("/tmp/test"))
            .with_bits(1024);

        let report = validator.validate_key(&key);
        assert!(report.has_high());
    }

    #[test]
    fn test_key_without_passphrase() {
        let validator = SecurityValidator::new();
        let key = SshKey::new(
            "test".to_string(),
            KeyType::Ed25519,
            PathBuf::from("/tmp/test"),
        )
        .with_bits(256);

        let report = validator.validate_key(&key);
        assert!(report.has_high());
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        assert!(Severity::Medium > Severity::Low);
        assert!(Severity::Low > Severity::Info);
    }

    #[test]
    fn test_security_audit_report() {
        let mut report = SecurityAuditReport::default();
        report.config_issues.push("Test issue".to_string());
        report
            .key_reports
            .push(("key1".to_string(), SecurityReport::new()));

        report.calculate_overall_score();
        assert!(report.overall_score <= 100);
    }

    fn key(key_type: KeyType, bits: Option<usize>, encrypted: bool, path: PathBuf) -> SshKey {
        let k = SshKey::new("k".to_string(), key_type, path).with_encrypted(encrypted);
        match bits {
            Some(b) => k.with_bits(b),
            None => k,
        }
    }

    fn codes(report: &SecurityReport) -> Vec<&str> {
        report.issues.iter().map(|i| i.code.as_str()).collect()
    }

    #[test]
    fn test_severity_display() {
        let all = [
            (Severity::Info, "INFO"),
            (Severity::Low, "LOW"),
            (Severity::Medium, "MEDIUM"),
            (Severity::High, "HIGH"),
            (Severity::Critical, "CRITICAL"),
        ];
        for (sev, text) in all {
            assert_eq!(sev.to_string(), text);
        }
    }

    #[test]
    fn test_report_penalties_and_queries() {
        let mut r = SecurityReport::new();
        let penalties = [
            (Severity::Info, 100),
            (Severity::Low, 95),
            (Severity::Medium, 80),
            (Severity::High, 55),
            (Severity::Critical, 15),
            (Severity::Critical, 0), // satura em zero
        ];
        for (sev, expected) in penalties {
            r.add_issue(SecurityIssue::new(sev, "C", "m", "d", "r"));
            assert_eq!(r.score, expected, "após {:?}", sev);
        }
        assert!(r.has_critical() && r.has_high());
        assert_eq!(r.issues_by_severity(Severity::Critical).len(), 2);
        assert_eq!(r.issues_by_severity(Severity::Low).len(), 1);
        r.add_warning("w");
        r.add_recommendation("rec");
        assert_eq!(r.warnings, vec!["w"]);
        assert_eq!(r.recommendations, vec!["rec"]);
        // Default não é o mesmo que new(): score começa em 0
        assert_eq!(SecurityReport::default().score, 0);
    }

    #[test]
    fn test_validate_key_types_and_sizes() {
        let v = SecurityValidator::default();
        let missing = PathBuf::from("/nao/existe/id");

        let r = v.validate_key(&key(KeyType::Ed25519, Some(256), true, missing.clone()));
        assert!(r.issues.is_empty() && r.warnings.is_empty());
        assert_eq!(r.score, 100);

        let r = v.validate_key(&key(KeyType::EcdsaP256, Some(256), true, missing.clone()));
        assert!(r.issues.is_empty());
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("ECDSA"));

        let r = v.validate_key(&key(KeyType::EcdsaP384, Some(256), true, missing.clone()));
        assert_eq!(codes(&r), vec!["KEY_INSUFFICIENT_BITS"]);
        assert!(r.issues[0].description.contains("384"));

        let r = v.validate_key(&key(KeyType::Rsa, Some(1024), true, missing.clone()));
        assert_eq!(codes(&r), vec!["KEY_SMALL_RSA", "KEY_INSUFFICIENT_BITS"]);
        assert!(r.issues[0].description.contains("1024"));

        let r = v.validate_key(&key(KeyType::Rsa, Some(4096), true, missing.clone()));
        assert!(r.issues.is_empty());
        // sem bits conhecidos não há checagem de tamanho
        let r = v.validate_key(&key(KeyType::Rsa, None, true, missing.clone()));
        assert!(r.issues.is_empty());

        let r = v.validate_key(&key(KeyType::Dsa, Some(1024), false, missing.clone()));
        assert_eq!(codes(&r), vec!["KEY_DEPRECATED_TYPE", "KEY_NO_PASSPHRASE"]);
        assert_eq!(r.score, 100 - 40 - 25);

        let r = v.validate_key(&key(KeyType::EcdsaP521, Some(521), true, missing));
        assert!(r.issues.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_key_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id");
        std::fs::write(&path, "x").unwrap();
        let v = SecurityValidator::new();
        let set = |mode| std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode));

        set(0o600).unwrap();
        let r = v.validate_key(&key(KeyType::Ed25519, None, true, path.clone()));
        assert!(r.issues.is_empty() && r.warnings.is_empty());
        assert!(SecurityValidator::validate_file_permissions(&path).unwrap());

        set(0o644).unwrap();
        let r = v.validate_key(&key(KeyType::Ed25519, None, true, path.clone()));
        assert_eq!(codes(&r), vec!["KEY_WORLD_ACCESSIBLE"]);
        assert!(r.issues[0].description.contains("644"));
        assert!(r.issues[0].recommendation.contains("chmod 600"));
        assert!(!SecurityValidator::validate_file_permissions(&path).unwrap());

        set(0o400).unwrap();
        let r = v.validate_key(&key(KeyType::Ed25519, None, true, path.clone()));
        assert!(r.issues.is_empty());
        assert_eq!(
            r.warnings,
            vec!["Permissões da chave são 400, recomendado: 600"]
        );

        assert!(matches!(
            SecurityValidator::validate_file_permissions(&dir.path().join("nada")),
            Err(SshKeyError::ReadError(_))
        ));
    }

    #[test]
    fn test_validate_host_algorithms() {
        let v = SecurityValidator::new();
        let mut h = SshHost::new("srv");
        h.kex_algorithms = Some("curve25519-sha256, diffie-hellman-group1-sha1".into());
        h.host_key_algorithms = Some("ssh-ed25519,ssh-rsa".into());
        h.ciphers = Some("aes256-gcm@openssh.com,aes128-cbc,inventado-ctr".into());
        h.macs = Some("hmac-md5".into());
        let r = v.validate_host(&h);
        assert_eq!(
            codes(&r),
            vec![
                "ALGO_DEPRECATED_KEX",
                "ALGO_DEPRECATED_HOSTKEY",
                "ALGO_DEPRECATED_CIPHER",
                "ALGO_DEPRECATED_MAC"
            ]
        );
        assert_eq!(r.issues[2].message, "Algoritmo Cipher inseguro: aes128-cbc");
        assert_eq!(
            r.issues[2].recommendation,
            "Remova 'aes128-cbc' da lista de cipher."
        );
        assert_eq!(
            r.warnings,
            vec!["Algoritmo Cipher 'inventado-ctr' não está na lista de recomendados"]
        );
    }

    #[test]
    fn test_validate_host_auth_forwarding_and_host_keys() {
        use crate::ssh::host::StrictHostKeyChecking;
        let v = SecurityValidator::new();

        let clean = v.validate_host(&SshHost::new("ok"));
        assert!(clean.issues.is_empty() && clean.warnings.is_empty());

        let mut h = SshHost::new("arriscado");
        h.password_authentication = Some(true);
        h.pub_key_authentication = Some(false);
        h.preferred_authentications = Some("password,publickey".into());
        h.forward_agent = Some(true);
        h.forward_x11 = Some(true);
        h.gateway_ports = Some(true);
        h.strict_host_key_checking = Some(StrictHostKeyChecking::No);
        let r = v.validate_host(&h);
        assert_eq!(
            codes(&r),
            vec![
                "AUTH_PUBKEY_DISABLED",
                "FWD_X11_ENABLED",
                "HOSTKEY_CHECK_DISABLED"
            ]
        );
        assert_eq!(r.warnings.len(), 4);
        assert!(r.has_critical());
        assert_eq!(r.score, 100 - 15 - 15 - 40);

        let mut h = SshHost::new("meio");
        h.preferred_authentications = Some("publickey,password".into());
        h.strict_host_key_checking = Some(StrictHostKeyChecking::AcceptNew);
        let r = v.validate_host(&h);
        assert!(r.issues.is_empty());
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("accept-new"));

        h.strict_host_key_checking = Some(StrictHostKeyChecking::Yes);
        assert!(v.validate_host(&h).warnings.is_empty());
    }

    #[test]
    fn test_auditor_on_missing_directory() {
        let auditor = SecurityAuditor::default();
        assert!(auditor.validator().is_algorithm_secure("ssh-ed25519"));
        let dir = tempfile::tempdir().unwrap();
        let report = auditor.audit(&dir.path().join("nao-existe")).unwrap();
        assert_eq!(report.config_issues, vec!["Diretório ~/.ssh não existe"]);
        assert_eq!(report.overall_score, 95);
        assert!(report.passed());
    }

    #[cfg(unix)]
    #[test]
    fn test_auditor_checks_permissions_and_dsa_keys() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir(&ssh).unwrap();
        let perm = |p: &Path, m| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m));

        // tudo correto
        perm(&ssh, 0o700).unwrap();
        for (name, mode) in [
            ("authorized_keys", 0o600),
            ("known_hosts", 0o644),
            ("config", 0o600),
        ] {
            let p = ssh.join(name);
            std::fs::write(&p, "ssh-ed25519 AAAA x\n").unwrap();
            perm(&p, mode).unwrap();
        }
        let r = SecurityAuditor::new().audit(&ssh).unwrap();
        assert!(r.config_issues.is_empty(), "{:?}", r.config_issues);
        assert_eq!(r.overall_score, 100);
        assert!(r.passed() && !r.has_critical_issues());

        // tudo errado
        perm(&ssh, 0o755).unwrap();
        let ak = ssh.join("authorized_keys");
        std::fs::write(&ak, "ssh-ed25519 AAAA a\nssh-dss AAAA b\nssh-dss AAAA c\n").unwrap();
        perm(&ak, 0o644).unwrap();
        perm(&ssh.join("known_hosts"), 0o666).unwrap();
        perm(&ssh.join("config"), 0o644).unwrap();
        let r = SecurityAuditor::new().audit(&ssh).unwrap();
        assert_eq!(
            r.config_issues,
            vec![
                "Permissões do diretório ~/.ssh são 755, deveriam ser 700",
                "Permissões de authorized_keys são 644, deveriam ser 600",
                "authorized_keys linha 2: Chave DSA encontrada (obsoleta)",
                "authorized_keys linha 3: Chave DSA encontrada (obsoleta)",
                "Permissões de known_hosts são 666, recomendado 644 ou 600",
                "Permissões de config são 644, deveriam ser 600",
            ]
        );
        assert_eq!(r.overall_score, 70);
        assert!(r.passed());
        perm(&ssh, 0o700).unwrap();
    }

    #[test]
    fn test_audit_score_counts_key_reports_and_critical() {
        let v = SecurityValidator::new();
        let dsa = v.validate_key(&key(
            KeyType::Dsa,
            None,
            false,
            PathBuf::from("/nao/existe"),
        ));
        assert_eq!(dsa.score, 35);
        let mut report = SecurityAuditReport {
            key_reports: vec![("id_dsa".into(), dsa)],
            ..Default::default()
        };
        report.calculate_overall_score();
        // (100 - 35) / 4 = 16
        assert_eq!(report.overall_score, 84);
        assert!(report.has_critical_issues());
        assert!(!report.passed());
    }

    #[test]
    fn test_audit_score_with_many_config_issues_does_not_overflow() {
        // Regressão: `len() as u8 * 5` estourava (pânico em debug) com 52+
        // problemas e, com 256, virava zero de penalidade
        for n in [52usize, 256, 1000] {
            let mut report = SecurityAuditReport {
                config_issues: vec!["x".to_string(); n],
                ..Default::default()
            };
            report.calculate_overall_score();
            assert_eq!(report.overall_score, 0, "{} problemas", n);
            assert!(!report.passed());
        }
    }
}
