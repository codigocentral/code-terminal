//! Exemplo de integração com OpenCode
//!
//! Este exemplo mostra como usar o hook do OpenCode via API.

use code_terminal::ai::hooks::{
    OpenCodeHook, OpenCodeConfig, AgentRequest, AgentCommand,
};
use std::env;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Exemplo de OpenCode Hook ===\n");

    // Verificar se API key está configurada
    let api_key = env::var("OPENCODE_API_KEY").unwrap_or_default();
    
    if api_key.is_empty() {
        println!("⚠ OPENCODE_API_KEY não configurada");
        println!("  Para usar este exemplo, configure a variável de ambiente:");
        println!("  export OPENCODE_API_KEY=sua_chave_aqui");
        return Ok(());
    }

    println!("✓ API key configurada");

    // Configurar hook
    let config = OpenCodeConfig {
        api_key,
        model: "opencode-pro".to_string(),
        streaming: true,
        ..Default::default()
    };

    let hook = OpenCodeHook::new(config)?;

    // Validar API key
    match hook.validate_api_key().await {
        Ok(true) => println!("✓ API key válida"),
        Ok(false) => println!("⚠ API key inválida"),
        Err(e) => println!("⚠ Erro na validação: {}", e),
    }

    // Criar registro
    let registration = hook.create_registration();
    println!("✓ Registro criado: {}", registration.info.name);
    println!("  Endpoint: {:?}", registration.endpoint);
    println!("  Capacidades: {:?}", registration.info.capabilities);

    // Capacidades
    let caps = hook.capabilities();
    println!("\n=== Capacidades ===");
    println!("Analisar código: {}", caps.can_analyze_code);
    println!("Streaming: {}", caps.supports_streaming);
    println!("Linguagens: {:?}", caps.supported_languages);

    // Estatísticas de uso
    if let Some(usage) = hook.get_usage() {
        println!("\nUso atual: {} tokens", usage);
    }
    if let Some(limit) = hook.get_rate_limit() {
        println!("Rate limit restante: {}", limit);
    }

    // Exemplo de requisição
    println!("\n=== Exemplo de Requisição ===");
    let request = AgentRequest::new(AgentCommand::GetSuggestion {
        context: "Como fazer um loop em Rust?".to_string(),
        max_suggestions: 3,
    });
    
    println!("Request ID: {}", request.id);
    println!("Tipo: GetSuggestion");

    // Para executar a requisição real (requer créditos):
    // let response = hook.process_request(request).await?;
    // println!("Sugestões: {:?}", response);

    println!("\n✓ Exemplo completado");

    Ok(())
}
