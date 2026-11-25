use code_terminal::app::state::AppState;
use code_terminal::config::parser::CONFIG_DIR_ENV;

#[test]
fn test_app_state_creation() {
    // Isola a configuração: AppState::new cria/migra o config.toml.
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var(CONFIG_DIR_ENV, dir.path());

    let state = AppState::new(80, 24);
    assert!(state.is_ok());
    assert!(dir.path().join("config.toml").exists());
    assert!(!dir.path().join("config.toml.toml").exists());
}
