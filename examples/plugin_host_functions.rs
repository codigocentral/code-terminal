//! Exemplo: Host Functions Executáveis para Plugins WASM
//!
//! Este exemplo demonstra como as 5 host functions permitem que plugins
//! interajam com o terminal de forma segura e controlada.
//!
//! Para executar:
//!   cargo run --example plugin_host_functions

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

// Importar as structs das host functions
use code_terminal::plugin::host::executable::{
    ExecutableHostFunctions, ExecutionContext, NotificationLevel, SandboxConfig, SystemExec,
    TerminalClear, TerminalCommand, TerminalRead, TerminalWrite, UiShowNotification,
};
use code_terminal::plugin::host::{HostFunction, HostFunctionError, HostFunctionPermission};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Host Functions Executáveis - Exemplo ===\n");

    // ================================================================
    // Setup: Criar contexto de execução e canais
    // ================================================================

    let (command_tx, mut command_rx) = mpsc::channel::<TerminalCommand>(100);
    let input_buffer = Arc::new(RwLock::new(Vec::new()));

    let ctx = Arc::new(RwLock::new(
        ExecutionContext::new("example-plugin").with_permissions(vec![
            "terminal:write".to_string(),
            "terminal:read".to_string(),
            "terminal:clear".to_string(),
            "ui:notification".to_string(),
            "system:exec".to_string(),
        ]),
    ));

    {
        let mut ctx_write = ctx.write().await;
        ctx_write.command_tx = Some(command_tx);
    }

    // ================================================================
    // 1. Terminal Write - Escrever no terminal
    // ================================================================
    println!("1️⃣  Terminal Write");
    println!("   Permissão: {:?}", HostFunctionPermission::Restricted);

    let write_fn = TerminalWrite::new(ctx.clone());

    // Simular dados de entrada (string convertida em bytes)
    let data = "Hello, Terminal!\r\n";
    match write_fn.execute(data.as_bytes()) {
        Ok(result) => {
            let bytes_written = i32::from_le_bytes([result[0], result[1], result[2], result[3]]);
            println!("   ✅ Escritos {} bytes no terminal", bytes_written);
        }
        Err(e) => println!("   ❌ Erro: {:?}", e),
    }

    // Verificar comando recebido
    if let Ok(cmd) = command_rx.try_recv() {
        match cmd {
            TerminalCommand::Write(data) => {
                println!("   📤 Comando recebido: Write({} bytes)", data.len());
            }
            _ => println!("   ⚠️  Comando inesperado"),
        }
    }

    // ================================================================
    // 2. Terminal Read - Ler input do usuário
    // ================================================================
    println!("\n2️⃣  Terminal Read");
    println!("   Permissão: {:?}", HostFunctionPermission::Restricted);

    // Primeiro, adicionar dados ao buffer de input
    {
        let mut buffer = input_buffer.write().await;
        buffer.extend_from_slice(b"ls -la\r\n");
        println!("   📝 Buffer de input: {} bytes disponíveis", buffer.len());
    }

    let read_fn = TerminalRead::new(ctx.clone(), input_buffer.clone());

    // Parâmetro: max_len (i32 little-endian)
    let max_len = 100i32;
    match read_fn.execute(&max_len.to_le_bytes()) {
        Ok(result) => {
            let bytes_read = i32::from_le_bytes([result[0], result[1], result[2], result[3]]);
            let data = String::from_utf8_lossy(&result[4..]);
            println!("   ✅ Lidos {} bytes: {:?}", bytes_read, data.trim());
        }
        Err(e) => println!("   ❌ Erro: {:?}", e),
    }

    // ================================================================
    // 3. Terminal Clear - Limpar o terminal
    // ================================================================
    println!("\n3️⃣  Terminal Clear");
    println!("   Permissão: {:?}", HostFunctionPermission::Restricted);
    println!("   Modos: 0=Screen, 1=Scrollback, 2=All");

    let clear_fn = TerminalClear::new(ctx.clone());

    // Limpar scrollback (modo 1)
    let mode = 1i32;
    match clear_fn.execute(&mode.to_le_bytes()) {
        Ok(_) => {
            println!("   ✅ Terminal limpo (modo: Scrollback)");
        }
        Err(e) => println!("   ❌ Erro: {:?}", e),
    }

    // Verificar comando recebido
    if let Ok(cmd) = command_rx.try_recv() {
        match cmd {
            TerminalCommand::Clear(clear_mode) => {
                println!("   📤 Comando recebido: Clear({:?})", clear_mode);
            }
            _ => println!("   ⚠️  Comando inesperado"),
        }
    }

    // ================================================================
    // 4. UI Show Notification - Mostrar notificação
    // ================================================================
    println!("\n4️⃣  UI Show Notification");
    println!("   Permissão: {:?}", HostFunctionPermission::Restricted);

    let notify_fn = UiShowNotification::new(ctx.clone());

    // Construir parâmetros:
    // [level: i32] [title_len: i32] [title] [message_len: i32] [message]
    let level = NotificationLevel::Success as i32;
    let title = "Plugin Ativado";
    let message = "O plugin de exemplo foi carregado com sucesso!";

    let mut params = Vec::new();
    params.extend_from_slice(&level.to_le_bytes());
    params.extend_from_slice(&(title.len() as i32).to_le_bytes());
    params.extend_from_slice(title.as_bytes());
    params.extend_from_slice(&(message.len() as i32).to_le_bytes());
    params.extend_from_slice(message.as_bytes());

    match notify_fn.execute(&params) {
        Ok(_) => {
            println!("   ✅ Notificação enviada: {} - {}", title, message);
        }
        Err(e) => println!("   ❌ Erro: {:?}", e),
    }

    // Verificar comando recebido
    if let Ok(cmd) = command_rx.try_recv() {
        match cmd {
            TerminalCommand::ShowNotification {
                level,
                title,
                message,
            } => {
                println!(
                    "   📤 Comando recebido: ShowNotification(level={}, title='{}', message='{}')",
                    level,
                    title,
                    message.chars().take(20).collect::<String>() + "..."
                );
            }
            _ => println!("   ⚠️  Comando inesperado"),
        }
    }

    // ================================================================
    // 5. System Exec (Sandboxed) - Executar comando
    // ================================================================
    println!("\n5️⃣  System Exec (Sandboxed)");
    println!("   Permissão: {:?}", HostFunctionPermission::Dangerous);
    println!("   ⚠️  Executa comandos com restrições de segurança");

    let exec_fn = SystemExec::new(ctx.clone());

    // Configurar sandbox personalizado
    let _sandbox = SandboxConfig {
        allowed_commands: vec![
            "echo".to_string(),
            "date".to_string(),
            "whoami".to_string(),
            "pwd".to_string(),
        ],
        allowed_dirs: vec!["/tmp".to_string()],
        timeout_secs: 10,
        max_memory_mb: 32,
        allow_network: false,
    };

    // Parâmetros: [cmd_len: i32] [cmd] [args_len: i32] [args_json]
    let command = "echo";
    let args = vec!["Hello from WASM plugin!"];
    let args_json = serde_json::to_string(&args).unwrap();

    let mut params = Vec::new();
    params.extend_from_slice(&(command.len() as i32).to_le_bytes());
    params.extend_from_slice(command.as_bytes());
    params.extend_from_slice(&(args_json.len() as i32).to_le_bytes());
    params.extend_from_slice(args_json.as_bytes());

    match exec_fn.execute(&params) {
        Ok(result) => {
            let json: serde_json::Value = serde_json::from_slice(&result).unwrap();
            println!("   ✅ Comando submetido:");
            println!("      Command: {}", command);
            println!("      Args: {:?}", args);
            println!("      Status: {}", json["status"]);
        }
        Err(e) => println!("   ❌ Erro: {:?}", e),
    }

    // Verificar comando recebido
    if let Ok(cmd) = command_rx.try_recv() {
        match cmd {
            TerminalCommand::Execute { command, args } => {
                println!("   📤 Comando recebido: Execute('{}', {:?})", command, args);
            }
            _ => println!("   ⚠️  Comando inesperado"),
        }
    }

    // ================================================================
    // Teste: Tentar executar comando não permitido
    // ================================================================
    println!("\n6️⃣  System Exec - Tentativa de comando não permitido");

    let bad_command = "rm";
    let bad_args = vec!["-rf", "/"];
    let bad_args_json = serde_json::to_string(&bad_args).unwrap();

    let mut params = Vec::new();
    params.extend_from_slice(&(bad_command.len() as i32).to_le_bytes());
    params.extend_from_slice(bad_command.as_bytes());
    params.extend_from_slice(&(bad_args_json.len() as i32).to_le_bytes());
    params.extend_from_slice(bad_args_json.as_bytes());

    match exec_fn.execute(&params) {
        Ok(result) => {
            let json: serde_json::Value = serde_json::from_slice(&result).unwrap();
            let exit_code = json["exit_code"].as_i64().unwrap_or(-1);
            let stderr = json["stderr"].as_str().unwrap_or("");

            println!("   ✅ Comando bloqueado pelo sandbox:");
            println!("      Exit code: {} (126 = não permitido)", exit_code);
            println!("      Erro: {}", stderr);
        }
        Err(e) => println!("   ❌ Erro: {:?}", e),
    }

    // ================================================================
    // Teste: Sem permissão
    // ================================================================
    println!("\n7️⃣  Terminal Write - Sem permissão");

    let ctx_no_perm = Arc::new(RwLock::new(
        ExecutionContext::new("untrusted-plugin"), // Sem permissões!
    ));

    let write_no_perm = TerminalWrite::new(ctx_no_perm);

    match write_no_perm.execute(b"test data") {
        Ok(_) => println!("   ⚠️  Deveria ter falhado!"),
        Err(HostFunctionError::PermissionDenied) => {
            println!("   ✅ Permissão negada (comportamento esperado)");
        }
        Err(e) => println!("   ❌ Erro inesperado: {:?}", e),
    }

    // ================================================================
    // Factory: Criar todas as funções de uma vez
    // ================================================================
    println!("\n8️⃣  ExecutableHostFunctions Factory");

    let ctx_factory = Arc::new(RwLock::new(ExecutionContext::new("factory-test")));
    let input_factory = Arc::new(RwLock::new(Vec::new()));

    let functions = ExecutableHostFunctions::create_all(ctx_factory, input_factory);

    println!("   ✅ Criadas {} funções executáveis:", functions.len());
    for (i, func) in functions.iter().enumerate() {
        let id = func.id();
        println!(
            "      {}. {}::{} ({:?})",
            i + 1,
            id.category,
            id.name,
            func.permission()
        );
    }

    println!("\n=== Fim do Exemplo ===");
    Ok(())
}
