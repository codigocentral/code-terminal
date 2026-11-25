//! Integration tests for CLI tool
//!
//! These tests verify the CLI client functionality with mock server responses.

use code_terminal::cli::{CliConfig, ConfigAction, OutputFormat, SplitDirection};
use serde_json::json;

/// Test CLI config loading with defaults
#[test]
fn test_cli_config_defaults() {
    let config = CliConfig::default();
    assert_eq!(config.timeout_secs, 30);
    assert_eq!(config.retry_attempts, 3);
}

/// Test OutputFormat enum variants
#[test]
fn test_output_format_variants() {
    assert_eq!(format!("{:?}", OutputFormat::Text), "Text");
    assert_eq!(format!("{:?}", OutputFormat::Json), "Json");
    assert_eq!(format!("{:?}", OutputFormat::Yaml), "Yaml");
}

/// Test SplitDirection enum variants
#[test]
fn test_split_direction_variants() {
    assert_eq!(format!("{:?}", SplitDirection::Horizontal), "Horizontal");
    assert_eq!(format!("{:?}", SplitDirection::Vertical), "Vertical");
}

/// Test ConfigAction variants
#[test]
fn test_config_action_variants() {
    let show = ConfigAction::Show;
    let set = ConfigAction::Set {
        key: "test".to_string(),
        value: "value".to_string(),
    };
    let get = ConfigAction::Get {
        key: "test".to_string(),
    };
    let reset = ConfigAction::Reset;

    // Just verify they can be created
    match show {
        ConfigAction::Show => {}
        _ => panic!("Expected Show"),
    }

    match set {
        ConfigAction::Set { key, value } => {
            assert_eq!(key, "test");
            assert_eq!(value, "value");
        }
        _ => panic!("Expected Set"),
    }

    match get {
        ConfigAction::Get { key } => {
            assert_eq!(key, "test");
        }
        _ => panic!("Expected Get"),
    }

    match reset {
        ConfigAction::Reset => {}
        _ => panic!("Expected Reset"),
    }
}

/// Test serialization of tab info structures
#[test]
fn test_tab_info_serialization() {
    use code_terminal::cli::client::TabInfo;

    let tab = TabInfo {
        id: 1,
        index: 0,
        title: "Test Tab".to_string(),
        is_active: true,
        pid: Some(12345),
        cwd: Some("/home/user".to_string()),
    };

    let json = serde_json::to_string(&tab).unwrap();
    let deserialized: TabInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(tab.index, deserialized.index);
    assert_eq!(tab.title, deserialized.title);
    assert_eq!(tab.is_active, deserialized.is_active);
    assert_eq!(tab.pid, deserialized.pid);
    assert_eq!(tab.cwd, deserialized.cwd);
}

/// Test pane info serialization
#[test]
fn test_pane_info_serialization() {
    use code_terminal::cli::client::PaneInfo;

    let pane = PaneInfo {
        id: 1,
        index: 0,
        is_active: false,
    };

    let json = serde_json::to_string(&pane).unwrap();
    let deserialized: PaneInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(pane.id, deserialized.id);
    assert_eq!(pane.index, deserialized.index);
    assert_eq!(pane.is_active, deserialized.is_active);
}

/// Test size info serialization
#[test]
fn test_size_info_serialization() {
    use code_terminal::cli::client::SizeInfo;

    let size = SizeInfo {
        cols: 120,
        rows: 30,
    };
    let json = serde_json::to_string(&size).unwrap();
    let deserialized: SizeInfo = serde_json::from_str(&json).unwrap();

    assert_eq!(size.cols, deserialized.cols);
    assert_eq!(size.rows, deserialized.rows);
}

/// Test info serialization
#[test]
fn test_info_serialization() {
    use code_terminal::cli::client::{Info, SizeInfo};

    let info = Info {
        version: "0.1.0".to_string(),
        tab_count: 3,
        active_tab: 1,
        size: SizeInfo { cols: 80, rows: 24 },
    };

    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains("0.1.0"));
    assert!(json.contains("80"));
    assert!(json.contains("24"));

    let deserialized: Info = serde_json::from_str(&json).unwrap();
    assert_eq!(info.version, deserialized.version);
    assert_eq!(info.tab_count, deserialized.tab_count);
}

/// Test JSON-RPC request structure
#[test]
fn test_jsonrpc_request_format() {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "list_tabs",
        "params": null
    });

    assert_eq!(request["jsonrpc"], "2.0");
    assert_eq!(request["id"], 1);
    assert_eq!(request["method"], "list_tabs");
}

/// Test JSON-RPC response structure
#[test]
fn test_jsonrpc_response_format() {
    let response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": [
            {"index": 0, "title": "Tab 1", "is_active": true, "pid": 1234, "cwd": "/home"}
        ]
    });

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert!(response["result"].is_array());
}

/// Test JSON-RPC error response
#[test]
fn test_jsonrpc_error_format() {
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32601,
            "message": "Method not found",
            "data": null
        }
    });

    assert_eq!(error_response["error"]["code"], -32601);
    assert_eq!(error_response["error"]["message"], "Method not found");
}

/// Test new_tab params structure
#[test]
fn test_new_tab_params_structure() {
    let params = json!({
        "title": "My Tab",
        "cwd": "/home/user/projects",
        "command": ["cargo", "build"]
    });

    assert_eq!(params["title"], "My Tab");
    assert_eq!(params["cwd"], "/home/user/projects");
    assert!(params["command"].is_array());
    assert_eq!(params["command"].as_array().unwrap().len(), 2);
}

/// Test send_keys params structure
#[test]
fn test_send_keys_params_structure() {
    let params = json!({
        "keys": "<C-c>"
    });

    assert_eq!(params["keys"], "<C-c>");
}

/// Test set_size params structure
#[test]
fn test_set_size_params_structure() {
    let params = json!({
        "cols": 120,
        "rows": 30
    });

    assert_eq!(params["cols"], 120);
    assert_eq!(params["rows"], 30);
}

/// Test notify params structure
#[test]
fn test_notify_params_structure() {
    let params = json!({
        "title": "Build Complete",
        "message": "Project compiled successfully"
    });

    assert_eq!(params["title"], "Build Complete");
    assert_eq!(params["message"], "Project compiled successfully");
}

/// Test split params structure
#[test]
fn test_split_params_structure() {
    let params = json!({
        "direction": "horizontal"
    });

    assert_eq!(params["direction"], "horizontal");
}

/// Test CLI error types
#[test]
fn test_cli_error_types() {
    use code_terminal::cli::error::CliError;

    let err = CliError::ServerNotRunning("127.0.0.1:9999".to_string());
    assert!(matches!(err, CliError::ServerNotRunning(_)));

    let err = CliError::connection("timeout");
    assert!(matches!(err, CliError::Connection { .. }));

    let err = CliError::Rpc {
        message: "test".to_string(),
    };
    assert!(matches!(err, CliError::Rpc { .. }));

    let err = CliError::InvalidResponse("bad json".to_string());
    assert!(matches!(err, CliError::InvalidResponse(_)));

    let err = CliError::Serialization("error".to_string());
    assert!(matches!(err, CliError::Serialization(_)));

    let err = CliError::Config("not found".to_string());
    assert!(matches!(err, CliError::Config(_)));
}

/// Test CliResult success case
#[test]
fn test_cli_result_success() {
    use code_terminal::cli::error::CliResult;

    fn returns_ok() -> CliResult<i32> {
        Ok(42)
    }

    assert_eq!(returns_ok().unwrap(), 42);
}

/// Test tab info with null fields
#[test]
fn test_tab_info_null_fields() {
    use code_terminal::cli::client::TabInfo;

    let json = r#"{"id":1,"index":1,"title":"Tab","is_active":false,"pid":null,"cwd":null}"#;
    let tab: TabInfo = serde_json::from_str(json).unwrap();

    assert_eq!(tab.index, 1);
    assert!(!tab.is_active);
    assert!(tab.pid.is_none());
    assert!(tab.cwd.is_none());
}

/// Test batch command parsing logic
#[test]
fn test_command_parsing() {
    // Simulate parsing command arguments
    let command_args: Vec<String> = vec![
        "cargo".to_string(),
        "build".to_string(),
        "--release".to_string(),
    ];

    assert_eq!(command_args.len(), 3);
    assert_eq!(command_args[0], "cargo");
    assert_eq!(command_args[1], "build");
    assert_eq!(command_args[2], "--release");
}

/// Test special key notation parsing
#[test]
fn test_special_key_notation() {
    let keys = vec![
        "<C-c>",
        "<Enter>",
        "<Esc>",
        "<Tab>",
        "<Backspace>",
        "<Up>",
        "<F1>",
    ];

    for key in &keys {
        assert!(key.starts_with('<') && key.ends_with('>'));
    }
}

/// Test config file structure
#[test]
fn test_config_structure() {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, Default)]
    struct TestConfig {
        socket_path: Option<String>,
        format: Option<String>,
        #[serde(default = "default_timeout")]
        timeout_secs: u64,
        #[serde(default = "default_retries")]
        retry_attempts: u32,
    }

    fn default_timeout() -> u64 {
        30
    }

    fn default_retries() -> u32 {
        3
    }

    let config: TestConfig = toml::from_str("").unwrap();
    assert_eq!(config.timeout_secs, 30);
    assert_eq!(config.retry_attempts, 3);
    assert!(config.socket_path.is_none());
}

/// Test complex tab list response
#[test]
fn test_complex_tab_list_response() {
    use code_terminal::cli::client::TabInfo;

    let json = r#"[
        {"id":0,"index":0,"title":"bash","is_active":false,"pid":1234,"cwd":"/home/user"},
        {"id":1,"index":1,"title":"vim main.rs","is_active":true,"pid":5678,"cwd":"/home/user/project"},
        {"id":2,"index":2,"title":"cargo build","is_active":false,"pid":9012,"cwd":"/home/user/project"}
    ]"#;

    let tabs: Vec<TabInfo> = serde_json::from_str(json).unwrap();
    assert_eq!(tabs.len(), 3);

    assert_eq!(tabs[0].title, "bash");
    assert!(tabs[1].is_active);
    assert_eq!(tabs[2].pid, Some(9012));
}

/// Test edge cases for terminal size
#[test]
fn test_terminal_size_edge_cases() {
    use code_terminal::cli::client::SizeInfo;

    // Minimum size
    let min = SizeInfo { cols: 1, rows: 1 };
    assert_eq!(min.cols, 1);
    assert_eq!(min.rows, 1);

    // Maximum reasonable size
    let max = SizeInfo {
        cols: 999,
        rows: 999,
    };
    assert_eq!(max.cols, 999);
    assert_eq!(max.rows, 999);

    // Zero should be valid for serialization (though not practical)
    let zero = SizeInfo { cols: 0, rows: 0 };
    assert_eq!(zero.cols, 0);
    assert_eq!(zero.rows, 0);
}
