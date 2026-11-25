/// Exemplo: Gerenciamento de Processo e Sinais no PTY
///
/// Este exemplo demonstra as funcionalidades de gerenciamento de processo
/// implementadas na US-003: PTY Funcional
///
/// Funcionalidades demonstradas:
/// - Criação de PTY com shell
/// - Verificação de status do processo
/// - Envio de sinais (SIGTERM, SIGKILL)
/// - Controle de Ctrl+C (SIGINT)
/// - Monitoramento de término de processo
use std::{thread, time::Duration};

fn main() -> anyhow::Result<()> {
    println!("=== Exemplo: Gerenciamento de Processo PTY ===\n");

    // Cria um PTY com dimensões 80x24
    let pty = code_terminal::terminal::Pty::new(80, 24)?;

    // Obtém informações do processo
    let pid = pty.pid();
    println!("✓ PTY criado com sucesso");
    println!("  PID do processo: {:?}", pid);
    println!(
        "  Status: {}\n",
        if pty.is_running() {
            "rodando"
        } else {
            "parado"
        }
    );

    // Envia um comando simples
    println!("→ Enviando comando 'echo Hello World'...");
    pty.write_str("echo Hello World\n")?;

    // Aguarda e lê o output
    thread::sleep(Duration::from_millis(200));
    let mut output = String::new();
    while let Some(data) = pty.try_read() {
        output.push_str(&String::from_utf8_lossy(&data));
        thread::sleep(Duration::from_millis(50));
    }
    println!("  Output: {}", output.trim());

    // Demonstra verificação de status
    println!("\n→ Verificando status do processo...");
    if pty.is_running() {
        println!("  ✓ Processo está rodando");

        // Tenta obter status sem bloquear
        match pty.try_wait() {
            Ok(None) => println!("  ✓ Processo ainda ativo (try_wait)"),
            Ok(Some(status)) => println!("  ! Processo terminou: {}", status),
            Err(e) => println!("  ✗ Erro: {}", e),
        }
    }

    // Demonstra envio de Ctrl+C (SIGINT)
    println!("\n→ Iniciando processo longo (sleep 10)...");
    pty.write_str("sleep 10\n")?;
    thread::sleep(Duration::from_millis(500));

    println!("→ Enviando Ctrl+C (SIGINT)...");
    pty.send_interrupt()?;
    thread::sleep(Duration::from_millis(200));

    // Lê output após o sinal
    let mut sig_output = String::new();
    while let Some(data) = pty.try_read() {
        sig_output.push_str(&String::from_utf8_lossy(&data));
    }
    if !sig_output.is_empty() {
        println!("  Output após sinal: {:?}", sig_output.trim());
    }

    // Verifica se processo ainda está rodando
    if pty.is_running() {
        println!("  ✓ Processo ainda ativo (esperado após SIGINT no shell)");
    }

    // Demonstra redimensionamento
    println!("\n→ Redimensionando PTY para 100x30...");
    pty.resize(100, 30)?;
    println!("  ✓ PTY redimensionado");

    // Envia comando de saída
    println!("\n→ Enviando comando 'exit'...");
    pty.write_str("exit\n")?;

    // Aguarda o processo terminar com timeout
    println!("→ Aguardando processo terminar (timeout 2s)...");
    match pty.wait_timeout(Duration::from_secs(2)) {
        Ok(Some(status)) => println!("  ✓ Processo terminou: {}", status),
        Ok(None) => println!("  ! Timeout - processo ainda rodando"),
        Err(e) => println!("  ✗ Erro: {}", e),
    }

    // Status final
    println!("\n=== Status Final ===");
    println!("  PID: {:?}", pid);
    println!("  Rodando: {}", pty.is_running());

    if let Some(exit_status) = pty.check_exit_notification() {
        println!("  Notificação de saída: {}", exit_status);
    }

    println!("\n✓ Exemplo completo!");
    Ok(())
}
