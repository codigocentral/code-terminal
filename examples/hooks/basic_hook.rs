//! Exemplo básico de uso do sistema de hooks
//!
//! Este exemplo demonstra como registrar agentes e emitir eventos.

use code_terminal::ai::hooks::{
    AgentContext, AgentEvent, AgentInfo, AgentRegistration, AgentType,
    HookManager, HookConfig, EventType,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Exemplo Básico de Hooks ===\n");

    // Criar gerenciador com configuração padrão
    let config = HookConfig::default();
    let manager = HookManager::new(config);

    // Registrar um agente genérico
    let info = AgentInfo::new(AgentType::Generic, "Meu Agente")
        .with_capability("log_events")
        .with_metadata("version", "1.0.0");

    let registration = AgentRegistration::new(info);
    manager.register_agent(registration).await?;

    println!("✓ Agente registrado");

    // Listar agentes
    let agents = manager.list_agents().await?;
    println!("✓ Agentes ativos: {}", agents.len());
    for agent in &agents {
        println!("  - {} ({:?})", agent.info.name, agent.info.agent_type);
    }

    // Emitir um evento
    let event = AgentEvent::terminal_start();
    manager.emit_event(event).await?;
    println!("✓ Evento de inicialização emitido");

    // Criar contexto
    let context = AgentContext::new("session-1", 1)
        .with_workspace(code_terminal::ai::hooks::WorkspaceInfo::with_directory("/home/user"))
        .with_env_var("EDITOR", "vim");

    println!("✓ Contexto criado: {}", context.current_directory());

    // Estatísticas
    let stats = manager.statistics().await;
    println!("\n=== Estatísticas ===");
    println!("Total de agentes: {}", stats.total_agents);
    println!("Eventos processados: {}", stats.events_processed);
    println!("Erros: {}", stats.errors);

    // Desligar
    manager.shutdown().await?;
    println!("\n✓ Gerenciador desligado");

    Ok(())
}
