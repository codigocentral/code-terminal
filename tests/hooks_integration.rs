//! Smoke tests de integração para o sistema de hooks.

use code_terminal::ai::hooks::api::ResponseData;
use code_terminal::ai::hooks::{
    AgentEvent, AgentInfo, AgentRegistration, AgentRequest, AgentResponse, AgentStatus, AgentType,
    HookManager, HookPriority, HookRegistry,
};
use std::time::Duration;

#[tokio::test]
async fn test_hook_lifecycle_smoke() {
    let manager = HookManager::default();

    let info = AgentInfo::new(AgentType::Generic, "TestAgent").with_capability("test");
    let reg = AgentRegistration::new(info);
    manager.register_agent(reg.clone()).await.unwrap();

    let agents = manager.list_agents().await.unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].info.name, "TestAgent");

    manager
        .emit_event(AgentEvent::terminal_start())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let removed = manager.unregister_agent(&reg.id).await.unwrap();
    assert_eq!(removed.info.name, "TestAgent");
    assert!(manager.list_agents().await.unwrap().is_empty());

    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_hook_manager_type_lookup_and_status() {
    let manager = HookManager::default();

    let reg = AgentRegistration::new(AgentInfo::new(AgentType::ClaudeCode, "Claude"));
    manager.register_agent(reg.clone()).await.unwrap();
    manager
        .update_agent_status(&reg.id, AgentStatus::Active)
        .await
        .unwrap();

    let agent = manager
        .get_agent_by_type(AgentType::ClaudeCode)
        .await
        .unwrap();
    assert!(agent.is_some());
    assert_eq!(agent.unwrap().info.status, AgentStatus::Active);

    manager.shutdown().await.unwrap();
}

#[test]
fn test_hook_registry_smoke() {
    let registry = HookRegistry::new();

    let reg = AgentRegistration::new(AgentInfo::new(AgentType::Aider, "Aider"));
    registry.register_agent(reg.clone()).unwrap();

    assert_eq!(registry.list_agents().unwrap().len(), 1);
    assert_eq!(
        registry
            .list_agents_by_type(AgentType::Aider)
            .unwrap()
            .len(),
        1
    );

    registry.unregister_agent(&reg.id).unwrap();
    assert!(registry.list_agents().unwrap().is_empty());
}

#[test]
fn test_agent_request_and_response_smoke() {
    let request = AgentRequest::execute_command("cargo test")
        .with_timeout(Duration::from_secs(1))
        .with_data("cwd", "/project");
    assert_eq!(request.timeout, Duration::from_secs(1));

    let response = AgentResponse::success(
        request.id,
        ResponseData::Text {
            text: "ok".to_string(),
        },
    )
    .with_processing_time(42);

    assert!(response.success);
    assert_eq!(response.processing_time_ms, 42);
}

#[test]
fn test_hook_priority_ordering() {
    assert!(HookPriority::Critical < HookPriority::Normal);
    assert!(HookPriority::Normal < HookPriority::Background);
    assert_eq!(HookPriority::Critical.value(), 0);
}
