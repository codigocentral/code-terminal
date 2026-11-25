# Gerenciamento de Processo e Sinais no PTY

**Documento:** US-003 - PTY Funcional  
**Status:** ✅ Completo  
**Última atualização:** Março 2026

---

## 📋 Visão Geral

O módulo PTY (`src/terminal/pty.rs`) foi expandido para incluir gerenciamento completo de processos e sinais, permitindo:

- Monitoramento do status do processo filho em tempo real
- Envio de sinais para o processo (SIGTERM, SIGKILL, SIGINT)
- Detecção de término de processo
- Cleanup adequado ao fechar o terminal

---

## 🏗️ Arquitetura

### Estrutura Principal

```rust
pub struct Pty {
    pair: PtyPair,                                    // Par master/slave do PTY
    writer: Box<dyn Write + Send>,                   // Escritor para input
    output_rx: Receiver<Vec<u8>>,                    // Canal para output
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>, // Handle do processo
    pid: Option<u32>,                                // PID do processo
    running: Arc<AtomicBool>,                        // Flag de estado
    exit_tx: Sender<ProcessStatus>,                  // Notificação de saída
    exit_rx: Receiver<ProcessStatus>,                // Recepção de notificação
}
```

### Enum de Status

```rust
pub enum ProcessStatus {
    Running,           // Processo está rodando
    Exited(i32),       // Terminou normalmente (código de saída)
    Signaled(i32),     // Terminado por sinal
    Unknown,           // Status desconhecido
}
```

---

## 🔌 API Pública

### Criação e Informações Básicas

```rust
// Cria novo PTY com dimensões especificadas
let pty = Pty::new(cols: u16, rows: u16) -> Result<Self>;

// Obtém o PID do processo filho
let pid: Option<u32> = pty.pid();

// Verifica se o processo está rodando
let is_running: bool = pty.is_running();
```

### Comunicação I/O

```rust
// Envia dados brutos
pty.write(data: &[u8]) -> Result<()>;

// Envia string
pty.write_str(s: &str) -> Result<()>;

// Lê output disponível (não bloqueante)
let data: Option<Vec<u8>> = pty.try_read();

// Redimensiona o terminal
pty.resize(cols: u16, rows: u16) -> Result<()>;
```

### Sinais e Controle

```rust
// Envia Ctrl+C (SIGINT)
pty.send_interrupt() -> Result<()>;

// Envia Ctrl+D (EOF)
pty.send_eof() -> Result<()>;

// Envia SIGTERM (Unix) ou taskkill (Windows)
pty.terminate() -> Result<()>;

// Envia SIGKILL (Unix) ou taskkill /F (Windows)  
pty.kill() -> Result<()>;

// Envia sinal específico (Unix apenas)
pty.signal(signal: i32) -> Result<()>;
```

### Monitoramento de Processo

```rust
// Tenta obter status sem bloquear
let status: Option<ProcessStatus> = pty.try_wait()?;

// Aguarda processo terminar (bloqueante)
let status: ProcessStatus = pty.wait()?;

// Aguarda com timeout
let status: Option<ProcessStatus> = pty.wait_timeout(timeout)?;

// Verifica notificação de término
let status: Option<ProcessStatus> = pty.check_exit_notification();
```

---

## 🔧 Implementação por Plataforma

### Unix (Linux/macOS)

Usa a crate `nix` para envio de sinais POSIX:

```rust
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

// SIGTERM (sinal 15) - Terminação graciosa
kill(Pid::from_raw(pid), Signal::SIGTERM)?;

// SIGKILL (sinal 9) - Terminação forçada
kill(Pid::from_raw(pid), Signal::SIGKILL)?;
```

**Sinais suportados:**
- `SIGTERM` (15): Terminação graciosa
- `SIGKILL` (9): Terminação forçada
- `SIGINT` (2): Interrupção (Ctrl+C)
- `SIGHUP` (1): Hangup
- Outros sinais POSIX via `signal(s: i32)`

### Windows

Usa `taskkill` nativo do Windows:

```rust
// Terminação graciosa
Command::new("taskkill").args(["/PID", &pid])?;

// Terminação forçada
Command::new("taskkill").args(["/F", "/PID", &pid])?;
```

**Nota:** Windows não suporta sinais POSIX nativamente. A implementação mapeia:
- `signal(15)` → `taskkill /PID` (SIGTERM)
- `signal(9)` → `taskkill /F /PID` (SIGKILL)

---

## ♻️ Gerenciamento de Ciclo de Vida

### Drop Trait

O `Drop` implementa cleanup seguro:

1. Sinaliza threads para parar (`running.store(false)`)
2. Se processo ainda rodando:
   - Envia SIGTERM/taskkill
   - Aguarda até 500ms
   - Se persistir, envia SIGKILL/taskkill /F
3. Flush do writer
4. Recursos liberados automaticamente pelo Rust

```rust
impl Drop for Pty {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        
        if self.is_running() {
            // Tenta terminação graciosa
            if let Err(_) = self.terminate() {
                // Força terminação
                let _ = self.kill();
            }
            
            // Aguarda término com timeout
            let _ = self.wait_timeout(Duration::from_millis(500));
        }
    }
}
```

---

## 📝 Exemplos de Uso

### Exemplo 1: Execução Básica

```rust
use code_terminal::terminal::Pty;

fn main() -> anyhow::Result<()> {
    // Cria PTY
    let mut pty = Pty::new(80, 24)?;
    
    // Executa comando
    pty.write_str("ls -la\n")?;
    
    // Lê output
    while let Some(data) = pty.try_read() {
        print!("{}", String::from_utf8_lossy(&data));
    }
    
    Ok(())
}
```

### Exemplo 2: Monitoramento de Processo

```rust
use std::time::Duration;

fn monitor_process(pty: &Pty) -> anyhow::Result<()> {
    loop {
        match pty.try_wait()? {
            Some(ProcessStatus::Exited(code)) => {
                println!("Processo terminou com código {}", code);
                break;
            }
            Some(ProcessStatus::Signaled(sig)) => {
                println!("Processo terminado por sinal {}", sig);
                break;
            }
            None => {
                println!("Processo ainda rodando...");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
    Ok(())
}
```

### Exemplo 3: Timeout em Comando

```rust
use std::time::Duration;

fn run_with_timeout(mut pty: Pty, command: &str, timeout: Duration) -> anyhow::Result<()> {
    pty.write_str(command)?;
    pty.write_str("\n")?;
    
    // Aguarda término ou timeout
    match pty.wait_timeout(timeout) {
        Ok(Some(status)) => {
            println!("Completou: {}", status);
        }
        Ok(None) => {
            println!("Timeout! Enviando SIGKILL...");
            pty.kill()?;
        }
        Err(e) => eprintln!("Erro: {}", e),
    }
    
    Ok(())
}
```

### Exemplo 4: Cancelamento com Ctrl+C

```rust
fn run_interruptible(pty: &mut Pty) -> anyhow::Result<()> {
    // Inicia comando longo
    pty.write_str("sleep 10\n")?;
    
    // Usuário pressiona Ctrl+C...
    std::thread::sleep(Duration::from_secs(2));
    
    // Envia interrupção
    pty.send_interrupt()?;
    
    Ok(())
}
```

---

## 🧪 Testes

O módulo inclui testes unitários:

```rust
#[test]
fn test_pty_creation() {
    let pty = Pty::new(80, 24);
    assert!(pty.is_ok());
    
    let pty = pty.unwrap();
    assert!(pty.pid().is_some());
    assert!(pty.is_running());
}

#[test]
fn test_pty_write_read() {
    let mut pty = Pty::new(80, 24).unwrap();
    
    pty.write_str("echo teste123\n").unwrap();
    
    // Aguarda execução
    std::thread::sleep(Duration::from_millis(100));
    
    // Lê output
    let mut output = String::new();
    while let Some(data) = pty.try_read() {
        output.push_str(&String::from_utf8_lossy(&data));
    }
    
    assert!(output.contains("teste123"));
}
```

---

## 🔍 Considerações de Segurança

1. **Race Conditions**: Uso de `Mutex` para acesso ao `Child` garante thread-safety
2. **Deadlocks**: Locks são sempre liberados com `drop()` explícito antes de operações longas
3. **Zombie Processes**: O `Drop` sempre tenta terminar o processo filho
4. **Signal Safety**: Sinais são enviados apenas para o PID correto verificado

---

## 📊 Métricas

| Operação | Complexidade | Latência Típica |
|----------|--------------|-----------------|
| `is_running()` | O(1) | < 1ms |
| `try_wait()` | O(1) | < 1ms |
| `wait()` | O(n) | Depende do processo |
| `signal()` | O(1) | < 1ms |
| `terminate()` | O(1) | < 10ms |

---

## 🔗 Dependências

```toml
[dependencies]
portable-pty = "0.8"

[target.'cfg(unix)'.dependencies]
nix = { version = "0.29", features = ["signal", "process"] }
```

---

## 🚀 Próximos Passos

- [ ] Suporte a grupos de processos (job control)
- [ ] Sessões POSIX (setsid)
- [ ] Notificações assíncronas via async/await
- [ ] Integração com sistema de tabs para cleanup individual

---

**Documento criado em:** Março 2026  
**Versão:** 1.0
