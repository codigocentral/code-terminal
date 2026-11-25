//! Exemplo de agente genérico personalizado
//!
//! Este exemplo mostra como criar um hook para um agente customizado.

use code_terminal::ai::hooks::{
    GenericAgentHook, GenericAgentConfig, GenericAgentConfigBuilder,
    DataFormat, ProtocolType, HookType, HookPriority,
    AgentRequest, AgentCommand,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Exemplo de Agente Genérico ===\n");

    // Exemplo 1: Agente via linha de comando
    println!("--- Configuração 1: Comando Local ---");
    let config1 = GenericAgentConfigBuilder::new()
        .command("/usr/local/bin/my-ai-agent")
        .arg("--json")
        .arg("--terminal-mode")
        .env("AGENT_MODE", "terminal")
        .timeout(120)
        .input_format(DataFormat::Json)
        .output_format(DataFormat::Json)
        .protocol(ProtocolType::Stdio)
        .capability("analyze")
        .capability("suggest")
        .build();

    let hook1 = GenericAgentHook::new("my-local-agent", config1);
    println!("✓ Agente criado: {}", hook1.name());
    println!("  Comando: {}", hook1.config().command);
    println!("  Protocolo: {:?}", hook1.config().protocol);

    // Exemplo 2: Agente via HTTP API
    println!("\n--- Configuração 2: API HTTP ---");
    let config2 = GenericAgentConfigBuilder::new()
        .command("curl") // curl como exemplo
        .arg("-X")
        .arg("POST")
        .arg("http://localhost:8080/api/agent")
        .env("API_TOKEN", "secret-token")
        .timeout(60)
        .protocol(ProtocolType::Http)
        .endpoint("http://localhost:8080")
        .capability("execute")
        .capability("edit")
        .capability("analyze")
        .build();

    let mut hook2 = GenericAgentHook::new("my-http-agent", config2);
    
    // Registrar handlers
    hook2.register_handler(
        HookType::OnCommandExecute,
        "handle_command",
        HookPriority::Normal,
    );
    hook2.register_handler(
        HookType::OnError,
        "handle_error",
        HookPriority::High,
    );
    
    println!("✓ Agente criado: {}", hook2.name());
    println!("  Endpoint: {:?}", hook2.config().endpoint);
    println!("  Handlers registrados");

    // Criar registros
    let reg1 = hook1.create_registration();
    let reg2 = hook2.create_registration();

    println!("\n=== Registros ===");
    println!("1. {} ({:?})", reg1.info.name, reg1.info.agent_type);
    println!("   Capacidades: {:?}", reg1.info.capabilities);
    println!("2. {} ({:?})", reg2.info.name, reg2.info.agent_type);
    println!("   Capacidades: {:?}", reg2.info.capabilities);

    // Capacidades
    println!("\n=== Capacidades Detalhadas ===");
    let caps1 = hook1.capabilities();
    let caps2 = hook2.capabilities();
    
    println!("Agente 1:");
    println!("  Executar comandos: {}", caps1.can_execute_commands);
    println!("  Editar arquivos: {}", caps1.can_edit_files);
    
    println!("Agente 2:");
    println!("  Executar comandos: {}", caps2.can_execute_commands);
    println!("  Editar arquivos: {}", caps2.can_edit_files);

    // Exemplo de requisição
    println!("\n=== Exemplo de Requisição ===");
    let request = AgentRequest::new(AgentCommand::Ping);
    println!("Request ID: {}", request.id);
    println!("  Este seria serializado como {:?}", hook2.config().input_format);

    println!("\n✓ Exemplo completado");

    Ok(())
}
