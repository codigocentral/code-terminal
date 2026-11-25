use crate::ai::{PrivacyControls, PrivacyError};
use crate::config::ai::AIPrivacyConfig;

pub struct PrivacyManager {
    controls: PrivacyControls,
}

impl PrivacyManager {
    pub fn new(config: AIPrivacyConfig) -> Self {
        Self {
            controls: PrivacyControls::new(config),
        }
    }

    pub fn sanitize_command(&self, command: &str) -> Result<String, PrivacyError> {
        self.controls.sanitize_for_ai(command, "command")
    }

    pub fn sanitize_terminal_output(&self, output: &str) -> Result<String, PrivacyError> {
        self.controls.sanitize_for_ai(output, "terminal_output")
    }

    pub fn sanitize_error_message(&self, error: &str) -> Result<String, PrivacyError> {
        self.controls.sanitize_for_ai(error, "error_message")
    }

    pub fn should_send_file_path(&self, path: &str) -> bool {
        !self.controls.is_path_excluded(path)
    }

    pub fn should_send_env_var(&self, var_name: &str) -> bool {
        !self.controls.is_env_var_excluded(var_name)
    }

    pub fn request_user_confirmation(
        &self,
        data_type: &str,
        preview: &str,
    ) -> Result<String, PrivacyError> {
        self.controls.request_confirmation(data_type, preview)
    }

    pub fn approve_confirmation(&self, confirmation_id: &str) -> Result<(), PrivacyError> {
        self.controls.approve_confirmation(confirmation_id)
    }

    pub fn deny_confirmation(&self, confirmation_id: &str) -> Result<(), PrivacyError> {
        self.controls.deny_confirmation(confirmation_id)
    }

    pub fn get_pending_confirmations(&self) -> Result<Vec<(String, String, String)>, PrivacyError> {
        self.controls.get_pending_confirmations()
    }

    pub fn get_audit_log(&self) -> Result<Vec<crate::ai::AuditLogEntry>, PrivacyError> {
        self.controls.get_audit_log()
    }

    pub fn update_privacy_config(&mut self, config: AIPrivacyConfig) {
        self.controls.update_config(config);
    }

    pub fn get_data_sharing_level(&self) -> crate::ai::DataSharingLevel {
        self.controls.get_data_sharing_level()
    }

    pub fn requires_confirmation(&self) -> bool {
        self.controls.requires_confirmation()
    }

    pub fn should_log_requests(&self) -> bool {
        self.controls.should_log_requests()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_manager() -> PrivacyManager {
        PrivacyManager::new(AIPrivacyConfig::default())
    }

    #[test]
    fn test_sanitize_command() {
        let manager = create_test_manager();
        // Pattern matches lowercase api_key (case sensitive)
        let command = "export api_key=secret123";
        let result = manager.sanitize_command(command).unwrap();

        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("secret123"));
    }

    #[test]
    fn test_sanitize_paths() {
        let manager = create_test_manager();
        let output = "File at /home/user/secret.txt";
        let result = manager.sanitize_terminal_output(output).unwrap();

        assert!(result.contains("[PATH]"));
        assert!(!result.contains("/home/user"));
    }

    #[test]
    fn test_file_path_exclusion() {
        let manager = create_test_manager();

        assert!(!manager.should_send_file_path("secret.key"));
        assert!(!manager.should_send_file_path("config.env"));
        assert!(manager.should_send_file_path("readme.md"));
    }

    #[test]
    fn test_env_var_exclusion() {
        let manager = create_test_manager();

        assert!(!manager.should_send_env_var("API_KEY"));
        assert!(!manager.should_send_env_var("MY_SECRET"));
        assert!(manager.should_send_env_var("PATH"));
    }

    #[test]
    fn test_confirmation_flow() {
        let mut config = AIPrivacyConfig::default();
        config.require_confirmation = true;
        let manager = PrivacyManager::new(config);

        let confirmation_id = manager
            .request_user_confirmation("file_paths", "/home/user/data")
            .unwrap();

        let pending = manager.get_pending_confirmations().unwrap();
        assert_eq!(pending.len(), 1);

        manager.approve_confirmation(&confirmation_id).unwrap();

        let pending = manager.get_pending_confirmations().unwrap();
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn test_data_sharing_level() {
        let manager = create_test_manager();
        let level = manager.get_data_sharing_level();

        assert_eq!(level, crate::ai::DataSharingLevel::Minimal);
    }
}
