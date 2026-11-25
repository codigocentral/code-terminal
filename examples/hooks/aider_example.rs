//! Exemplo de integração com Aider
//!
//! Este exemplo demonstra como usar o hook do Aider para pair programming.

use code_terminal::ai::hooks::{
    AiderHook, AiderConfig, AiderConfigBuilder, AgentRequest, AgentCommand,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("=== Exemplo de Aider Hook ===\n");

    // Verificar se Aider está disponível
    if !AiderHook::detect() {
        println!("⚠ Aider não detectado no ambiente");
        println!("  Para usar este exemplo, instale: pip install aider-chat");
        return Ok(());
    }

    println!("✓ Aider detectado");

    // Configurar com builder
    let config = AiderConfigBuilder::new()
        .model("sonnet")
        .auto_commits(true)
        .lint_cmd("cargo clippy")
        .test_cmd("cargo test")
        .timeout(300)
        .build();

    let mut hook = AiderHook::new(config);

    // Obter versão
    match hook.get_version().await {
        Ok(version) => println!("✓ Versão: {}", version),
        Err(e) => println!("⚠ Erro ao obter versão: {}", e),
    }

    // Criar registro
    let registration = hook.create_registration().await?;
    println!("✓ Registro criado: {}", registration.info.name);
    println!("  Capacidades: {:?}", registration.info.capabilities);

    // Capacidades
    let caps = hook.capabilities();
    println!("\n=== Capacidades ===");
    println!("Editar arquivos: {}", caps.can_edit_files);
    println!("Analisar código: {}", caps.can_analyze_code);
    println!("Gerar testes: {}", caps.can_generate_tests);
    println!("Linguagens suportadas: {:?}", caps.supported_languages);

    // Exemplo de comandos
    println!("\n=== Comandos Disponíveis ===");
    println!("- send_message: Enviar mensagem para o Aider");
    println!("- code_change: Solicitar mudança no código");
    println!("- add_file: Adicionar arquivo ao contexto");
    println!("- commit: Gerar commit");
    println!("- lint: Rodar linter");
    println!("- test: Rodar testes");
    println!("- undo: Desfazer última mudança");

    println!("\n✓ Exemplo completado");

    Ok(())
}
