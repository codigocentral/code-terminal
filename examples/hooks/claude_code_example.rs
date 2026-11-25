//! Exemplo de integração com Claude Code
//!
//! Este exemplo mostra como usar o hook do Claude Code.

use code_terminal::ai::hooks::{
    ClaudeCodeHook, ClaudeCodeConfig, AgentRequest,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Exemplo de Claude Code Hook ===\n");

    // Verificar se Claude Code está disponível
    if !ClaudeCodeHook::detect() {
        println!("⚠ Claude Code não detectado no ambiente");
        println!("  Para usar este exemplo, instale: npm install -g @anthropic-ai/claude-code");
        return Ok(());
    }

    println!("✓ Claude Code detectado");

    // Configurar hook
    let config = ClaudeCodeConfig::default();
    let hook = ClaudeCodeHook::new(config);

    // Obter versão
    match hook.get_version().await {
        Ok(version) => println!("✓ Versão: {}", version),
        Err(e) => println!("⚠ Erro ao obter versão: {}", e),
    }

    // Criar registro
    let registration = hook.create_registration().await?;
    println!("✓ Registro criado: {}", registration.info.name);
    println!("  Capacidades: {:?}", registration.info.capabilities);

    // Exemplo de requisição (não executa sem confirmação)
    println!("\n=== Exemplo de Requisição ===");
    let request = AgentRequest::get_suggestion("Como listar arquivos?");
    println!("Request ID: {}", request.id);
    println!("Command: {:?}", request.command);
    println!("Timeout: {:?}", request.timeout);

    // Para executar a requisição real (descomente com cuidado):
    // let response = hook.process_request(request).await?;
    // println!("Response: {:?}", response);

    println!("\n✓ Exemplo completado");

    Ok(())
}
