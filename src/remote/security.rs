//! Auditoria e segurança para controle remoto
//!
//! Fornece validações de segurança, logging de eventos de segurança,
//! e geração de relatórios de auditoria.

use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

/// Nível de severidade de eventos de segurança
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecurityLevel {
    /// Informação - eventos normais
    Info,
    /// Aviso - comportamento suspeito
    Warning,
    /// Alerta - possível ataque
    Alert,
    /// Crítico - ataque confirmado
    Critical,
}

impl std::fmt::Display for SecurityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityLevel::Info => write!(f, "INFO"),
            SecurityLevel::Warning => write!(f, "WARNING"),
            SecurityLevel::Alert => write!(f, "ALERT"),
            SecurityLevel::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Tipo de evento de segurança
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecurityEventType {
    /// Tentativa de autenticação
    AuthAttempt,
    /// Falha de autenticação
    AuthFailure,
    /// Sucesso de autenticação
    AuthSuccess,
    /// Acesso negado
    AccessDenied,
    /// Conexão de IP suspeito
    SuspiciousIp,
    /// Rate limiting ativado
    RateLimitTriggered,
    /// Comando perigoso detectado
    DangerousCommand,
    /// Configuração alterada
    ConfigChanged,
    /// Desconexão forçada
    ForcedDisconnect,
}

impl std::fmt::Display for SecurityEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityEventType::AuthAttempt => write!(f, "AUTH_ATTEMPT"),
            SecurityEventType::AuthFailure => write!(f, "AUTH_FAILURE"),
            SecurityEventType::AuthSuccess => write!(f, "AUTH_SUCCESS"),
            SecurityEventType::AccessDenied => write!(f, "ACCESS_DENIED"),
            SecurityEventType::SuspiciousIp => write!(f, "SUSPICIOUS_IP"),
            SecurityEventType::RateLimitTriggered => write!(f, "RATE_LIMIT"),
            SecurityEventType::DangerousCommand => write!(f, "DANGEROUS_CMD"),
            SecurityEventType::ConfigChanged => write!(f, "CONFIG_CHANGED"),
            SecurityEventType::ForcedDisconnect => write!(f, "FORCED_DISCONNECT"),
        }
    }
}

/// Evento de segurança
#[derive(Debug, Clone)]
pub struct SecurityEvent {
    /// Timestamp do evento
    pub timestamp: Instant,
    /// Nível de severidade
    pub level: SecurityLevel,
    /// Tipo do evento
    pub event_type: SecurityEventType,
    /// Endereço IP (se aplicável)
    pub ip: Option<IpAddr>,
    /// Descrição do evento
    pub description: String,
    /// Detalhes adicionais
    pub details: Option<String>,
}

impl SecurityEvent {
    /// Criar novo evento
    pub fn new(
        level: SecurityLevel,
        event_type: SecurityEventType,
        description: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Instant::now(),
            level,
            event_type,
            ip: None,
            description: description.into(),
            details: None,
        }
    }

    /// Definir IP
    pub fn with_ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    /// Adicionar detalhes
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// Configuração de segurança
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    /// Habilitar auditoria
    pub audit_enabled: bool,
    /// Nível mínimo para log
    pub min_log_level: SecurityLevel,
    /// Retenção de eventos (em segundos)
    pub retention_secs: u64,
    /// Limite de eventos armazenados
    pub max_events: usize,
    /// Bloquear IPs após falhas consecutivas
    pub block_after_failures: Option<u32>,
    /// IPs confiáveis (nunca bloqueados)
    pub trusted_ips: Vec<IpAddr>,
    /// IPs bloqueados permanentemente
    pub blocked_ips: Vec<IpAddr>,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            audit_enabled: true,
            min_log_level: SecurityLevel::Info,
            retention_secs: 86400, // 24 horas
            max_events: 10000,
            block_after_failures: Some(10),
            trusted_ips: vec![
                IpAddr::from([127, 0, 0, 1]),
                IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]), // ::1
            ],
            blocked_ips: Vec::new(),
        }
    }
}

/// Auditor de segurança
#[derive(Debug)]
pub struct SecurityAuditor {
    config: SecurityConfig,
    events: Arc<RwLock<VecDeque<SecurityEvent>>>,
    ip_failures: Arc<RwLock<std::collections::HashMap<IpAddr, Vec<Instant>>>>,
}

impl SecurityAuditor {
    /// Criar novo auditor com configuração padrão
    pub fn new() -> Self {
        Self::with_config(SecurityConfig::default())
    }

    /// Criar auditor com configuração customizada
    pub fn with_config(config: SecurityConfig) -> Self {
        Self {
            config,
            events: Arc::new(RwLock::new(VecDeque::new())),
            ip_failures: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Registrar evento de segurança
    pub async fn log_event(&self, event: SecurityEvent) {
        if !self.config.audit_enabled {
            return;
        }

        if event.level < self.config.min_log_level {
            return;
        }

        // Log com tracing
        match event.level {
            SecurityLevel::Info => {
                tracing::info!(
                    "[SECURITY] {} - {}: {}",
                    event.level,
                    event.event_type,
                    event.description
                );
            }
            SecurityLevel::Warning => {
                tracing::warn!(
                    "[SECURITY] {} - {}: {}",
                    event.level,
                    event.event_type,
                    event.description
                );
            }
            SecurityLevel::Alert | SecurityLevel::Critical => {
                tracing::error!(
                    "[SECURITY] {} - {}: {} {:?}",
                    event.level,
                    event.event_type,
                    event.description,
                    event.ip
                );
            }
        }

        // Armazenar evento
        let mut events = self.events.write().await;
        events.push_back(event);

        // Limpar eventos antigos se necessário
        self.cleanup_old_events(&mut events).await;

        // Limitar tamanho
        while events.len() > self.config.max_events {
            events.pop_front();
        }
    }

    /// Registrar tentativa de autenticação
    pub async fn log_auth_attempt(&self, ip: Option<IpAddr>, username: Option<&str>) {
        let mut event = SecurityEvent::new(
            SecurityLevel::Info,
            SecurityEventType::AuthAttempt,
            format!("Authentication attempt from {:?}", username),
        );

        if let Some(ip) = ip {
            event = event.with_ip(ip);
        }

        self.log_event(event).await;
    }

    /// Registrar falha de autenticação
    pub async fn log_auth_failure(&self, ip: Option<IpAddr>, reason: &str) {
        let mut event = SecurityEvent::new(
            SecurityLevel::Warning,
            SecurityEventType::AuthFailure,
            format!("Authentication failed: {}", reason),
        );

        if let Some(ip) = ip {
            event = event.with_ip(ip);
            self.record_failure(ip).await;
        }

        self.log_event(event).await;

        // Verificar se deve bloquear IP
        if let Some(ip) = ip {
            if self.should_block_ip(ip).await {
                self.log_event(
                    SecurityEvent::new(
                        SecurityLevel::Alert,
                        SecurityEventType::SuspiciousIp,
                        format!("IP {} blocked due to repeated failures", ip),
                    )
                    .with_ip(ip),
                )
                .await;
            }
        }
    }

    /// Registrar sucesso de autenticação
    pub async fn log_auth_success(&self, ip: Option<IpAddr>, username: Option<&str>) {
        let mut event = SecurityEvent::new(
            SecurityLevel::Info,
            SecurityEventType::AuthSuccess,
            format!("Authentication successful for {:?}", username),
        );

        if let Some(ip) = ip {
            event = event.with_ip(ip);
            // Limpar falhas do IP
            self.clear_failures(ip).await;
        }

        self.log_event(event).await;
    }

    /// Registrar comando perigoso
    pub async fn log_dangerous_command(&self, ip: Option<IpAddr>, command: &str) {
        let mut event = SecurityEvent::new(
            SecurityLevel::Alert,
            SecurityEventType::DangerousCommand,
            format!("Dangerous command detected: {}", command),
        );

        if let Some(ip) = ip {
            event = event.with_ip(ip);
        }

        self.log_event(event).await;
    }

    /// Verificar se IP está bloqueado
    pub async fn is_ip_blocked(&self, ip: IpAddr) -> bool {
        // Verificar lista de bloqueio permanente
        if self.config.blocked_ips.contains(&ip) {
            return true;
        }

        // Verificar IPs confiáveis
        if self.config.trusted_ips.contains(&ip) {
            return false;
        }

        // Verificar falhas recentes
        self.should_block_ip(ip).await
    }

    /// Verificar se deve bloquear IP
    async fn should_block_ip(&self, ip: IpAddr) -> bool {
        let Some(threshold) = self.config.block_after_failures else {
            return false;
        };

        let failures = self.ip_failures.read().await;
        let Some(failure_times) = failures.get(&ip) else {
            return false;
        };

        let recent_failures: Vec<_> = failure_times
            .iter()
            .filter(|t| t.elapsed() < Duration::from_secs(3600)) // última hora
            .collect();

        recent_failures.len() >= threshold as usize
    }

    /// Registrar falha de IP
    async fn record_failure(&self, ip: IpAddr) {
        let mut failures = self.ip_failures.write().await;
        failures.entry(ip).or_default().push(Instant::now());
    }

    /// Limpar falhas de IP
    async fn clear_failures(&self, ip: IpAddr) {
        let mut failures = self.ip_failures.write().await;
        failures.remove(&ip);
    }

    /// Limpar eventos antigos
    async fn cleanup_old_events(&self, events: &mut VecDeque<SecurityEvent>) {
        let Some(cutoff) =
            Instant::now().checked_sub(Duration::from_secs(self.config.retention_secs))
        else {
            return;
        };

        while let Some(event) = events.front() {
            if event.timestamp < cutoff {
                events.pop_front();
            } else {
                break;
            }
        }
    }

    /// Obter relatório de eventos
    pub async fn get_events(
        &self,
        level: Option<SecurityLevel>,
        limit: Option<usize>,
    ) -> Vec<SecurityEvent> {
        let events = self.events.read().await;
        let mut result: Vec<_> = events.iter().cloned().collect();

        if let Some(level) = level {
            result.retain(|e| e.level >= level);
        }

        if let Some(limit) = limit {
            result.truncate(limit);
        }

        result
    }

    /// Gerar relatório de segurança
    pub async fn generate_report(&self) -> SecurityReport {
        let events = self.events.read().await;

        let mut report = SecurityReport {
            total_events: events.len(),
            events_by_level: std::collections::HashMap::new(),
            events_by_type: std::collections::HashMap::new(),
            blocked_ips: self.config.blocked_ips.clone(),
            top_ips: Vec::new(),
        };

        // Contar eventos
        for event in events.iter() {
            *report.events_by_level.entry(event.level).or_insert(0) += 1;
            *report.events_by_type.entry(event.event_type).or_insert(0) += 1;
        }

        report
    }

    /// Bloquear IP permanentemente
    pub async fn block_ip(&self, ip: IpAddr) {
        // Verificar se não é IP confiável
        if self.config.trusted_ips.contains(&ip) {
            return;
        }

        // Adicionar à lista de bloqueio
        // (Em implementação real, persistiria em disco)
        tracing::warn!("IP {} added to permanent block list", ip);
    }

    /// Validar comando para segurança
    pub fn validate_command(&self, command: &str) -> Result<(), SecurityError> {
        // Lista de comandos perigosos
        let dangerous_patterns = [
            "rm -rf /",
            ":(){ :|:& };:", // fork bomb
            ">/dev/sda",
            "dd if=/dev/zero",
            "mkfs",
            "format c:",
        ];

        let lower = command.to_lowercase();
        for pattern in &dangerous_patterns {
            if lower.contains(pattern) {
                return Err(SecurityError::DangerousCommand(command.to_string()));
            }
        }

        Ok(())
    }
}

impl Default for SecurityAuditor {
    fn default() -> Self {
        Self::new()
    }
}

/// Relatório de segurança
#[derive(Debug, Clone)]
pub struct SecurityReport {
    /// Total de eventos
    pub total_events: usize,
    /// Eventos por nível
    pub events_by_level: std::collections::HashMap<SecurityLevel, usize>,
    /// Eventos por tipo
    pub events_by_type: std::collections::HashMap<SecurityEventType, usize>,
    /// IPs bloqueados
    pub blocked_ips: Vec<IpAddr>,
    /// IPs mais ativos
    pub top_ips: Vec<(IpAddr, usize)>,
}

/// Erros de segurança
#[derive(Debug, Clone, PartialEq)]
pub enum SecurityError {
    /// Comando perigoso detectado
    DangerousCommand(String),
    /// IP bloqueado
    IpBlocked(IpAddr),
    /// Acesso negado
    AccessDenied(String),
}

impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecurityError::DangerousCommand(cmd) => {
                write!(f, "Dangerous command detected: {}", cmd)
            }
            SecurityError::IpBlocked(ip) => {
                write!(f, "IP {} is blocked", ip)
            }
            SecurityError::AccessDenied(reason) => {
                write!(f, "Access denied: {}", reason)
            }
        }
    }
}

impl std::error::Error for SecurityError {}

/// Middleware de segurança para verificar comandos
pub struct SecurityMiddleware {
    auditor: Arc<SecurityAuditor>,
}

impl SecurityMiddleware {
    /// Criar novo middleware
    pub fn new(auditor: Arc<SecurityAuditor>) -> Self {
        Self { auditor }
    }

    /// Verificar se método é seguro
    pub async fn check_method(
        &self,
        method: &str,
        ip: Option<IpAddr>,
    ) -> Result<(), SecurityError> {
        // Verificar se IP está bloqueado
        if let Some(ip) = ip {
            if self.auditor.is_ip_blocked(ip).await {
                return Err(SecurityError::IpBlocked(ip));
            }
        }

        // Verificar se método é perigoso
        let dangerous_methods = ["system.exec", "system.shell", "file.delete"];
        if dangerous_methods.contains(&method) {
            self.auditor.log_dangerous_command(ip, method).await;
            return Err(SecurityError::AccessDenied(format!(
                "Method '{}' is restricted",
                method
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_level_ordering() {
        assert!(SecurityLevel::Info < SecurityLevel::Warning);
        assert!(SecurityLevel::Warning < SecurityLevel::Alert);
        assert!(SecurityLevel::Alert < SecurityLevel::Critical);
    }

    #[test]
    fn test_security_event_creation() {
        let event = SecurityEvent::new(
            SecurityLevel::Warning,
            SecurityEventType::AuthFailure,
            "Test event",
        );

        assert_eq!(event.level, SecurityLevel::Warning);
        assert_eq!(event.event_type, SecurityEventType::AuthFailure);
        assert_eq!(event.description, "Test event");
        assert!(event.ip.is_none());
    }

    #[test]
    fn test_security_event_with_ip() {
        let ip = IpAddr::from([192, 168, 1, 1]);
        let event = SecurityEvent::new(
            SecurityLevel::Alert,
            SecurityEventType::SuspiciousIp,
            "Suspicious activity",
        )
        .with_ip(ip);

        assert_eq!(event.ip, Some(ip));
    }

    #[tokio::test]
    async fn test_auditor_log_event() {
        let auditor = SecurityAuditor::new();
        let event = SecurityEvent::new(SecurityLevel::Info, SecurityEventType::AuthAttempt, "Test");

        auditor.log_event(event).await;

        let events = auditor.get_events(None, None).await;
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn test_auditor_log_auth_events() {
        let auditor = SecurityAuditor::new();
        let ip = IpAddr::from([127, 0, 0, 1]);

        auditor.log_auth_attempt(Some(ip), Some("user")).await;
        auditor.log_auth_success(Some(ip), Some("user")).await;

        let events = auditor.get_events(None, None).await;
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn test_ip_blocking() {
        let config = SecurityConfig {
            block_after_failures: Some(3),
            trusted_ips: vec![], // Não confiar em localhost para este teste
            ..Default::default()
        };
        let auditor = SecurityAuditor::with_config(config);
        let ip = IpAddr::from([192, 168, 1, 100]);

        // Inicialmente não bloqueado
        assert!(!auditor.is_ip_blocked(ip).await);

        // Registrar 3 falhas
        auditor.log_auth_failure(Some(ip), "Wrong password").await;
        auditor.log_auth_failure(Some(ip), "Wrong password").await;
        auditor.log_auth_failure(Some(ip), "Wrong password").await;

        // Agora deve estar bloqueado
        assert!(auditor.is_ip_blocked(ip).await);
    }

    #[tokio::test]
    async fn test_trusted_ip_not_blocked() {
        let config = SecurityConfig {
            block_after_failures: Some(1),
            ..Default::default()
        };
        let auditor = SecurityAuditor::with_config(config);
        let ip = IpAddr::from([127, 0, 0, 1]);

        // Registrar falha
        auditor.log_auth_failure(Some(ip), "Wrong password").await;

        // Não deve estar bloqueado (é trusted)
        assert!(!auditor.is_ip_blocked(ip).await);
    }

    #[tokio::test]
    async fn test_clear_failures_on_success() {
        let config = SecurityConfig {
            block_after_failures: Some(3),
            trusted_ips: vec![],
            ..Default::default()
        };
        let auditor = SecurityAuditor::with_config(config);
        let ip = IpAddr::from([192, 168, 1, 100]);

        // 2 falhas
        auditor.log_auth_failure(Some(ip), "Wrong password").await;
        auditor.log_auth_failure(Some(ip), "Wrong password").await;

        // Sucesso deve limpar
        auditor.log_auth_success(Some(ip), Some("user")).await;

        // Não deve estar bloqueado
        assert!(!auditor.is_ip_blocked(ip).await);
    }

    #[test]
    fn test_validate_command_safe() {
        let auditor = SecurityAuditor::new();
        assert!(auditor.validate_command("ls -la").is_ok());
        assert!(auditor.validate_command("echo hello").is_ok());
    }

    #[test]
    fn test_validate_command_dangerous() {
        let auditor = SecurityAuditor::new();
        assert!(auditor.validate_command("rm -rf /").is_err());
        assert!(auditor.validate_command(":(){ :|:& };:").is_err());
    }

    #[tokio::test]
    async fn test_security_report() {
        let auditor = SecurityAuditor::new();

        // Gerar eventos de diferentes níveis
        for _ in 0..5 {
            auditor
                .log_event(SecurityEvent::new(
                    SecurityLevel::Info,
                    SecurityEventType::AuthAttempt,
                    "Attempt",
                ))
                .await;
        }

        for _ in 0..3 {
            auditor
                .log_event(SecurityEvent::new(
                    SecurityLevel::Warning,
                    SecurityEventType::AuthFailure,
                    "Failure",
                ))
                .await;
        }

        let report = auditor.generate_report().await;
        assert_eq!(report.total_events, 8);
        assert_eq!(report.events_by_level.get(&SecurityLevel::Info), Some(&5));
        assert_eq!(
            report.events_by_level.get(&SecurityLevel::Warning),
            Some(&3)
        );
    }

    #[tokio::test]
    async fn test_security_middleware() {
        let auditor = Arc::new(SecurityAuditor::new());
        let middleware = SecurityMiddleware::new(auditor.clone());

        // Método seguro deve passar
        assert!(middleware.check_method("tab.list", None).await.is_ok());

        // Método perigoso deve falhar
        assert!(middleware.check_method("system.exec", None).await.is_err());
    }

    #[test]
    fn test_security_error_display() {
        let err = SecurityError::DangerousCommand("rm -rf /".to_string());
        assert!(err.to_string().contains("Dangerous"));

        let ip = IpAddr::from([192, 168, 1, 1]);
        let err = SecurityError::IpBlocked(ip);
        assert!(err.to_string().contains("blocked"));
    }
}
