//! Exemplo de tratamento de eventos
//!
//! Este exemplo demonstra como criar handlers para diferentes tipos de eventos.

use code_terminal::ai::hooks::{
    HookManager, HookConfig, AgentEvent, EventType, EventPriority,
    AgentContext, AgentInfo, AgentRegistration, AgentType,
    HookHandler, HookRegistry, HookPriority,
};
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Exemplo de Tratamento de Eventos ===\n");

    // Criar gerenciador
    let manager = HookManager::default();

    // Registrar agente
    let info = AgentInfo::new(AgentType::Generic, "EventLogger")
        .with_capability("event_logging");
    let reg = AgentRegistration::new(info);
    manager.register_agent(reg).await?;

    println!("✓ Agente registrado");

    // Emitir diferentes tipos de eventos
    println!("\n=== Emitindo Eventos ===");

    // Evento de inicialização
    let event1 = AgentEvent::terminal_start();
    manager.emit_event(event1).await?;
    println!("✓ Evento: TerminalStart");

    // Evento de criação de aba
    let event2 = AgentEvent::new(EventType::TabCreated {
        tab_id: 1,
        title: "bash".to_string(),
    })
    .with_tab_id(1)
    .with_priority(EventPriority::Normal);
    manager.emit_event(event2).await?;
    println!("✓ Evento: TabCreated (tab_id: 1)");

    // Evento de comando
    let event3 = AgentEvent::command_executed(1, "cargo build");
    manager.emit_event(event3).await?;
    println!("✓ Evento: CommandExecuted (cargo build)");

    // Evento de erro
    let event4 = AgentEvent::error_detected(
        1,
        "CompileError",
        "error[E0425]: cannot find value",
        code_terminal::ai::hooks::ErrorSeverity::Error,
    );
    manager.emit_event(event4).await?;
    println!("✓ Evento: ErrorDetected (CompileError)");

    // Evento customizado
    let event5 = AgentEvent::new(EventType::Custom {
        name: "my_custom_event".to_string(),
        payload: serde_json::json!({
            "key": "value",
            "number": 42
        }),
    })
    .with_data("source", "example")
    .with_metadata("important", true);
    manager.emit_event(event5).await?;
    println!("✓ Evento: Custom (my_custom_event)");

    // Aguardar processamento
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Estatísticas
    let stats = manager.statistics().await;
    println!("\n=== Estatísticas ===");
    println!("Total de agentes: {}", stats.total_agents);
    println!("Eventos processados: {}", stats.events_processed);
    println!("Erros: {}", stats.errors);

    // Demonstrar filtro de eventos
    println!("\n=== Filtro de Eventos ===");
    
    use code_terminal::ai::hooks::EventFilter;
    
    let filter = EventFilter::min_priority(EventPriority::High);
    let event = AgentEvent::terminal_start(); // High priority
    
    if filter.matches(&event) {
        println!("✓ Evento de alta prioridade passou no filtro");
    }

    let filter2 = EventFilter::tab(1);
    let event2 = AgentEvent::command_executed(1, "ls");
    
    if filter2.matches(&event2) {
        println!("✓ Evento da aba 1 passou no filtro");
    }

    // Desligar
    manager.shutdown().await?;
    println!("\n✓ Exemplo completado");

    Ok(())
}

/// Exemplo de handler personalizado
fn create_custom_handler() -> HookHandler {
    HookHandler::new(|event, context| {
        match &event.event_type {
            EventType::CommandExecuted { command, .. } => {
                println!("  [Handler] Comando executado: {}", command);
            }
            EventType::ErrorDetected { error_type, message, .. } => {
                println!("  [Handler] Erro detectado: {} - {}", error_type, message);
            }
            _ => {}
        }
        Ok(())
    })
}
