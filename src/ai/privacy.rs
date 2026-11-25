use crate::config::ai::AIPrivacyConfig;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, warn};

const CONFIRMATION_TTL_MINUTES: i64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataSharingLevel {
    None,
    Minimal,
    Standard,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub action: String,
    pub data_type: String,
    pub redacted_count: usize,
    pub approved: bool,
    pub user_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PrivacyControls {
    config: AIPrivacyConfig,
    audit_log: Arc<RwLock<Vec<AuditLogEntry>>>,
    compiled_patterns: Vec<Regex>,
    pending_confirmations: Arc<RwLock<HashMap<String, PendingConfirmation>>>,
}

#[derive(Debug, Clone)]
struct PendingConfirmation {
    data_type: String,
    preview: String,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl PrivacyControls {
    pub fn new(config: AIPrivacyConfig) -> Self {
        let compiled_patterns = Self::compile_patterns(&config);

        Self {
            config,
            audit_log: Arc::new(RwLock::new(Vec::new())),
            compiled_patterns,
            pending_confirmations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn compile_patterns(config: &AIPrivacyConfig) -> Vec<Regex> {
        let mut patterns = Vec::new();

        if config.redact_secrets {
            let secret_patterns = vec![
                r"[A-Za-z0-9]{32,}",
                r"sk-[A-Za-z0-9]{20,}",
                r"password\s*[=:]\s*\S+",
                r"token\s*[=:]\s*\S+",
                r"secret\s*[=:]\s*\S+",
                r"api[_-]?key\s*[=:]\s*\S+",
            ];

            for pattern in secret_patterns {
                if let Ok(re) = Regex::new(pattern) {
                    patterns.push(re);
                } else {
                    warn!("Failed to compile secret pattern: {}", pattern);
                }
            }
        }

        for pattern in &config.exclude_patterns {
            let glob_pattern = glob_to_regex(pattern);
            if let Ok(re) = Regex::new(&glob_pattern) {
                patterns.push(re);
            } else {
                warn!("Failed to compile exclude pattern: {}", pattern);
            }
        }

        patterns
    }

    pub fn sanitize_for_ai(&self, input: &str, data_type: &str) -> Result<String, PrivacyError> {
        if !self.is_data_type_allowed(data_type) {
            return Err(PrivacyError::DataTypeNotAllowed(data_type.to_string()));
        }

        let mut sanitized = input.to_string();

        if self.config.anonymize_paths {
            sanitized = self.anonymize_paths(&sanitized);
        }

        if self.config.anonymize_usernames {
            sanitized = self.anonymize_usernames(&sanitized);
        }

        let mut redacted_count = 0;
        for pattern in &self.compiled_patterns {
            let matches: Vec<_> = pattern.find_iter(&sanitized).collect();
            redacted_count += matches.len();
            sanitized = pattern.replace_all(&sanitized, "[REDACTED]").to_string();
        }

        self.log_audit(AuditLogEntry {
            timestamp: chrono::Utc::now(),
            action: "sanitize".to_string(),
            data_type: data_type.to_string(),
            redacted_count,
            approved: true,
            user_message: None,
        });

        debug!(
            data_type,
            redacted_count, "Sanitized data for AI processing"
        );

        Ok(sanitized)
    }

    fn anonymize_paths(&self, input: &str) -> String {
        let path_pattern = Regex::new(r"/home/[^/\s]+|/Users/[^/\s]+|C:\\Users\\[^\\]+|~")
            .unwrap_or_else(|_| Regex::new(r"").unwrap());

        path_pattern.replace_all(input, "[PATH]").to_string()
    }

    fn anonymize_usernames(&self, input: &str) -> String {
        let username_patterns = vec![
            (r"user:\s*\w+", "user: [USERNAME]"),
            (r"username:\s*\w+", "username: [USERNAME]"),
            (r"login:\s*\w+", "login: [USERNAME]"),
        ];

        let mut result = input.to_string();
        for (pattern, replacement) in username_patterns {
            if let Ok(re) = Regex::new(pattern) {
                result = re.replace_all(&result, replacement).to_string();
            }
        }

        result
    }

    pub fn request_confirmation(
        &self,
        data_type: &str,
        preview: &str,
    ) -> Result<String, PrivacyError> {
        if !self.config.require_confirmation {
            return Ok("auto-approved".to_string());
        }

        let confirmation_id = uuid::Uuid::new_v4().to_string();

        let pending = PendingConfirmation {
            data_type: data_type.to_string(),
            preview: preview.chars().take(100).collect(),
            timestamp: chrono::Utc::now(),
        };

        self.pending_confirmations
            .write()
            .map_err(|_| PrivacyError::LockError)?
            .insert(confirmation_id.clone(), pending);

        debug!(confirmation_id, data_type, "Created pending confirmation");

        Ok(confirmation_id)
    }

    pub fn approve_confirmation(&self, confirmation_id: &str) -> Result<(), PrivacyError> {
        let mut pending = self
            .pending_confirmations
            .write()
            .map_err(|_| PrivacyError::LockError)?;
        Self::purge_expired_confirmations(&mut pending);

        if let Some(confirmation) = pending.remove(confirmation_id) {
            self.log_audit(AuditLogEntry {
                timestamp: chrono::Utc::now(),
                action: "approve".to_string(),
                data_type: confirmation.data_type,
                redacted_count: 0,
                approved: true,
                user_message: None,
            });

            debug!(confirmation_id, "Confirmation approved");
            Ok(())
        } else {
            Err(PrivacyError::InvalidConfirmationId)
        }
    }

    pub fn deny_confirmation(&self, confirmation_id: &str) -> Result<(), PrivacyError> {
        let mut pending = self
            .pending_confirmations
            .write()
            .map_err(|_| PrivacyError::LockError)?;
        Self::purge_expired_confirmations(&mut pending);

        if let Some(confirmation) = pending.remove(confirmation_id) {
            self.log_audit(AuditLogEntry {
                timestamp: chrono::Utc::now(),
                action: "deny".to_string(),
                data_type: confirmation.data_type,
                redacted_count: 0,
                approved: false,
                user_message: None,
            });

            debug!(confirmation_id, "Confirmation denied");
            Err(PrivacyError::ConfirmationDenied)
        } else {
            Err(PrivacyError::InvalidConfirmationId)
        }
    }

    pub fn get_pending_confirmations(&self) -> Result<Vec<(String, String, String)>, PrivacyError> {
        let mut pending = self
            .pending_confirmations
            .write()
            .map_err(|_| PrivacyError::LockError)?;
        Self::purge_expired_confirmations(&mut pending);

        Ok(pending
            .iter()
            .map(|(id, conf)| (id.clone(), conf.data_type.clone(), conf.preview.clone()))
            .collect())
    }

    pub fn is_data_type_allowed(&self, data_type: &str) -> bool {
        matches!(
            data_type,
            "terminal_output"
                | "command"
                | "error_message"
                | "file_paths"
                | "environment_vars"
                | "shell_history"
        )
    }

    pub fn update_config(&mut self, config: AIPrivacyConfig) {
        self.compiled_patterns = Self::compile_patterns(&config);
        self.config = config;
    }

    pub fn get_audit_log(&self) -> Result<Vec<AuditLogEntry>, PrivacyError> {
        self.audit_log
            .read()
            .map(|log| log.clone())
            .map_err(|_| PrivacyError::LockError)
    }

    pub fn clear_audit_log(&self) -> Result<(), PrivacyError> {
        self.audit_log
            .write()
            .map(|mut log| log.clear())
            .map_err(|_| PrivacyError::LockError)
    }

    fn log_audit(&self, entry: AuditLogEntry) {
        if self.config.log_requests {
            if let Ok(mut log) = self.audit_log.write() {
                log.push(entry);
            }
        }
    }

    fn purge_expired_confirmations(pending: &mut HashMap<String, PendingConfirmation>) {
        let cutoff = chrono::Utc::now() - chrono::Duration::minutes(CONFIRMATION_TTL_MINUTES);
        pending.retain(|_, confirmation| confirmation.timestamp >= cutoff);
    }

    pub fn get_data_sharing_level(&self) -> DataSharingLevel {
        if !self.config.anonymize_paths
            && !self.config.anonymize_usernames
            && !self.config.redact_secrets
        {
            DataSharingLevel::Full
        } else if self.config.anonymize_paths
            && self.config.anonymize_usernames
            && self.config.redact_secrets
        {
            DataSharingLevel::Minimal
        } else {
            DataSharingLevel::Standard
        }
    }

    pub fn is_env_var_excluded(&self, var_name: &str) -> bool {
        self.config
            .exclude_env_vars
            .iter()
            .any(|excluded| var_name.to_uppercase().contains(&excluded.to_uppercase()))
    }

    pub fn is_path_excluded(&self, path: &str) -> bool {
        self.config
            .exclude_patterns
            .iter()
            .any(|pattern| glob_match(pattern, path))
    }

    pub fn should_log_requests(&self) -> bool {
        self.config.log_requests
    }

    pub fn requires_confirmation(&self) -> bool {
        self.config.require_confirmation
    }
}

impl Default for PrivacyControls {
    fn default() -> Self {
        Self::new(AIPrivacyConfig::default())
    }
}

fn glob_to_regex(glob: &str) -> String {
    let mut regex = String::with_capacity(glob.len() * 2);
    regex.push('^');

    for c in glob.chars() {
        match c {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' | '|' | '+' | '\\' => {
                regex.push('\\');
                regex.push(c);
            }
            _ => regex.push(c),
        }
    }

    regex.push('$');
    regex
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();

    while ti < text.len() {
        if pi < pattern.len() && (pattern[pi] == text[ti] || pattern[pi] == '?') {
            pi += 1;
            ti += 1;
        } else if pi < pattern.len() && pattern[pi] == '*' {
            star_idx = Some(pi);
            match_idx = ti;
            pi += 1;
        } else if let Some(si) = star_idx {
            pi = si + 1;
            match_idx += 1;
            ti = match_idx;
        } else {
            return false;
        }
    }

    while pi < pattern.len() && pattern[pi] == '*' {
        pi += 1;
    }

    pi == pattern.len()
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum PrivacyError {
    #[error("Data type not allowed: {0}")]
    DataTypeNotAllowed(String),

    #[error("Invalid confirmation ID")]
    InvalidConfirmationId,

    #[error("Confirmation denied by user")]
    ConfirmationDenied,

    #[error("Failed to acquire lock")]
    LockError,

    #[error("Invalid regex pattern: {0}")]
    InvalidPattern(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_controls() -> PrivacyControls {
        let config = AIPrivacyConfig {
            anonymize_paths: true,
            anonymize_usernames: true,
            exclude_patterns: vec!["*.key".to_string()],
            exclude_env_vars: vec!["API_KEY".to_string()],
            require_confirmation: false,
            log_requests: true,
            redact_secrets: true,
        };
        PrivacyControls::new(config)
    }

    #[test]
    fn test_sanitize_paths() {
        let controls = create_test_controls();
        let input = "File at /home/user/secret.key contains data";
        let result = controls.sanitize_for_ai(input, "file_paths").unwrap();

        assert!(result.contains("[PATH]"));
        assert!(!result.contains("/home/user"));
    }

    #[test]
    fn test_sanitize_usernames() {
        let controls = create_test_controls();
        let input = "user: john login: jane";
        let result = controls.sanitize_for_ai(input, "terminal_output").unwrap();

        assert!(result.contains("[USERNAME]"));
    }

    #[test]
    fn test_data_type_allowed() {
        let controls = create_test_controls();

        assert!(controls.is_data_type_allowed("terminal_output"));
        assert!(controls.is_data_type_allowed("file_paths"));
        assert!(!controls.is_data_type_allowed("invalid_type"));
    }

    #[test]
    fn test_audit_log() {
        let controls = create_test_controls();

        controls
            .sanitize_for_ai("test data", "terminal_output")
            .unwrap();

        let log = controls.get_audit_log().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].action, "sanitize");
    }

    #[test]
    fn test_data_sharing_level() {
        let minimal_controls = PrivacyControls::new(AIPrivacyConfig {
            anonymize_paths: true,
            anonymize_usernames: true,
            redact_secrets: true,
            ..Default::default()
        });
        assert_eq!(
            minimal_controls.get_data_sharing_level(),
            DataSharingLevel::Minimal
        );

        let full_controls = PrivacyControls::new(AIPrivacyConfig {
            anonymize_paths: false,
            anonymize_usernames: false,
            redact_secrets: false,
            ..Default::default()
        });
        assert_eq!(
            full_controls.get_data_sharing_level(),
            DataSharingLevel::Full
        );
    }

    #[test]
    fn test_confirmation_flow() {
        let config = AIPrivacyConfig {
            require_confirmation: true,
            ..Default::default()
        };
        let controls = PrivacyControls::new(config);

        let confirmation_id = controls
            .request_confirmation("file_paths", "/home/user/secret")
            .unwrap();

        let pending = controls.get_pending_confirmations().unwrap();
        assert_eq!(pending.len(), 1);

        controls.approve_confirmation(&confirmation_id).unwrap();

        let pending = controls.get_pending_confirmations().unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_confirmation_deny() {
        let config = AIPrivacyConfig {
            require_confirmation: true,
            ..Default::default()
        };
        let controls = PrivacyControls::new(config);

        let confirmation_id = controls
            .request_confirmation("file_paths", "/home/user/secret")
            .unwrap();

        let result = controls.deny_confirmation(&confirmation_id);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PrivacyError::ConfirmationDenied
        ));
    }

    #[test]
    fn test_env_var_excluded() {
        let controls = create_test_controls();

        assert!(controls.is_env_var_excluded("API_KEY"));
        assert!(controls.is_env_var_excluded("MY_API_KEY"));
        assert!(!controls.is_env_var_excluded("PATH"));
    }

    #[test]
    fn test_path_excluded() {
        let controls = create_test_controls();

        assert!(controls.is_path_excluded("secret.key"));
        assert!(controls.is_path_excluded("/path/to/file.key"));
        assert!(!controls.is_path_excluded("file.txt"));
    }

    #[test]
    fn test_glob_to_regex() {
        assert_eq!(glob_to_regex("*.key"), "^.*\\.key$");
        assert_eq!(glob_to_regex("file?.txt"), "^file.\\.txt$");
        assert_eq!(glob_to_regex("test*"), "^test.*$");
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.key", "secret.key"));
        assert!(glob_match("*.key", "/path/to/secret.key"));
        assert!(!glob_match("*.key", "file.txt"));
        assert!(glob_match("file?.txt", "file1.txt"));
        assert!(!glob_match("file?.txt", "file.txt"));
    }
}
