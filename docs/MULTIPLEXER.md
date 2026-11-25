# 🔄 Multiplexer Mode

Documentação completa do sistema de multiplexador do Codeterm.

## Visão Geral

O modo multiplexador (`src/multiplexer/`) implementa funcionalidades similares ao tmux, permitindo:

- **Sessões persistentes**: Sessões que continuam rodando mesmo quando desconectadas
- **Múltiplas janelas**: Cada sessão pode ter várias janelas
- **Divisão de painéis**: Janelas podem ser divididas em múltiplos painéis
- **Sessões compartilhadas**: Múltiplos clientes podem se conectar à mesma sessão
- **Integração SSH**: Sessões remotas via SSH com todas as funcionalidades

## Uso: sessões persistentes

Um shell numa sessão do multiplexer continua rodando quando a tab, a janela
ou o próprio Code Terminal fecham. O daemon (`code-terminal-cli mux-server`)
sobe sozinho quando a primeira sessão é criada e sai quando a última termina.
Ele escuta num socket Unix `0600` (`$XDG_RUNTIME_DIR/code-terminal-mux.sock`),
então só o seu usuário anexa ou digita nas sessões.

No app: paleta de comandos → **New Persistent Tab**. A tab mostra `⟳ <nome>`.
Fechar a tab apenas desanexa; ao reabrir o app, o workspace anexa de novo à
mesma sessão, com a tela como estava. `exit` no shell encerra a sessão.

No terminal (qualquer terminal, inclusive via SSH):

```sh
code-terminal-cli mux-attach trabalho --create   # cria (se preciso) e anexa
#   Ctrl+B d        desanexa (o shell continua)
#   Ctrl+B Ctrl+B   manda um Ctrl+B ao programa
code-terminal-cli mux-list                       # sessões, clientes, estado
code-terminal-cli mux-attach trabalho --read-only
code-terminal-cli mux-rename trabalho deploy
code-terminal-cli mux-destroy deploy             # encerra o shell
```

Ao anexar, o cliente recebe a tela atual redesenhada a partir do grid do
daemon (texto, cores, cursor, tela alternativa, modos de teclado e mouse) e
depois só a saída nova. O tamanho segue o terminal do cliente.

Disponível no Linux e no macOS; no Windows o daemon ainda não existe.

## Arquitetura

```
┌─────────────────────────────────────────────────────────────┐
│                    Multiplexer Module                        │
├─────────────┬─────────────┬─────────────┬────────────────────┤
│   Session   │   Window    │    Pane     │     Client         │
│  Manager    │  Manager    │  Manager    │   Manager          │
├─────────────┼─────────────┼─────────────┼────────────────────┤
│ Multiplexer │   Window    │    Pane     │ ClientConnection   │
│  Session    │  (tabs)     │  (splits)   │ SharedClient       │
├─────────────┼─────────────┼─────────────┼────────────────────┤
│   Shared    │    SSH      │  Protocol   │   Persistence      │
│   Session   │  Compatibility│ (socket)   │   Monitor          │
└─────────────┴─────────────┴─────────────┴────────────────────┘
```

## Componentes

### 1. Session (`session.rs`)

A sessão é a unidade principal do multiplexador.

```rust
use code_terminal::multiplexer::{SessionId, MultiplexerSession, SessionFlags};

// Criar nova sessão
let session = MultiplexerSession::new(Some("my-session".to_string()), (80, 24));

// Verificar estado
if session.is_active() {
    println!("Sessão ativa com {} clientes", session.client_count);
}

// Flags
let flags = SessionFlags {
    destroy_on_exit: false,
    attachable: true,
    shared: true,
    locked: false,
};
```

#### Estados de Sessão

| Estado | Descrição |
|--------|-----------|
| `Creating` | Sessão sendo criada |
| `Active` | Com clientes conectados |
| `Background` | Sem clientes (rodando em background) |
| `Destroying` | Sendo destruída |
| `Destroyed` | Destruída |

### 2. Window (`window.rs`)

Janelas funcionam como "abas" dentro de uma sessão.

```rust
use code_terminal::multiplexer::{WindowId, Window, WindowManager, WindowLayout};

// Gerenciador de janelas
let mut manager = WindowManager::new();

// Criar janelas
let window1 = manager.create_window(Some("editor".to_string()), (80, 24));
let window2 = manager.create_window(Some("terminal".to_string()), (80, 24));

// Navegar entre janelas
manager.select_next();
manager.select_previous();
manager.select_last();

// Aplicar layout
if let Some(window) = manager.active_window_mut() {
    window.set_layout(WindowLayout::Tiled);
}
```

#### Layouts de Janela

| Layout | Descrição |
|--------|-----------|
| `Free` | Posicionamento livre dos painéis |
| `EvenHorizontal` | Painéis divididos horizontalmente |
| `EvenVertical` | Painéis divididos verticalmente |
| `MainHorizontal` | Um painel grande em cima, outros embaixo |
| `MainVertical` | Um painel grande à esquerda, outros à direita |
| `Tiled` | Layout em grid |

### 3. Pane (`pane.rs`)

Painéis são divisões individuais da tela dentro de uma janela.

```rust
use code_terminal::multiplexer::{PaneId, Pane, PaneManager, PaneContent};

// Criar painel
let pane = Pane::new((80, 24), (0, 0))
    .with_title("Shell")
    .with_command("/bin/zsh".to_string());

// Gerenciador
let mut manager = PaneManager::new();
let pane_id = manager.create_pane((40, 24), (0, 0));

// Dividir painel
manager.active_pane_mut().map(|p| {
    // Divisão vertical (lado a lado)
});

// Scrollback
pane.scroll_up(10);
pane.scroll_to_top();
pane.scroll_to_bottom();

// Modo de cópia
pane.enter_copy_mode();
let selected_text = pane.get_selection_text();
pane.exit_copy_mode();
```

### 4. Shared Sessions (`shared.rs`)

Sessões compartilhadas permitem múltiplos clientes simultâneos.

```rust
use code_terminal::multiplexer::{
    SharedSession, SharedSessionConfig, ClientPermissions, 
    SyncMode, SharedSessionManager
};

// Configuração
let config = SharedSessionConfig::default()
    .with_max_clients(5)
    .with_sync_mode(SyncMode::Independent)
    .with_password("secret123");

// Criar sessão compartilhada
let session = SharedSession::new(session_id, config);

// Cliente entra na sessão
let client_id = ClientId::new();
let perms = session.join(client_id, Some("secret123"))?;

// Permissões personalizadas
let readonly = ClientPermissions::read_only();
let admin = ClientPermissions::admin();

// Gerenciador
let mut manager = SharedSessionManager::new();
manager.create_shared_session(session_id, config);
manager.connect_client(client_id, session_id, None)?;
```

#### Modos de Sincronização

| Modo | Descrição |
|------|-----------|
| `Independent` | Cada cliente tem cursor independente |
| `Synchronized` | Todos os clientes veem o mesmo cursor |
| `Broadcast` | Input de um cliente vai para todos |

#### Permissões

| Permissão | Descrição |
|-----------|-----------|
| `write` | Pode enviar input |
| `resize` | Pode redimensionar terminal |
| `execute_commands` | Pode executar comandos |
| `manage_clients` | Pode gerenciar outros clientes |
| `read_only` | Apenas visualização |

### 5. SSH Compatibility (`ssh_compat.rs`)

Integração com o módulo SSH para sessões remotas.

```rust
use code_terminal::multiplexer::{
    SshCompatibleSession, SshCompatManager, SshCompatConfig,
    SshTransport, AuthMethod
};

// Configuração SSH
let config = SshCompatConfig::default()
    .with_max_clients(10);

// Gerenciador
let mut manager = SshCompatManager::new()
    .with_default_config(config);

// Criar sessão SSH
let session_id = SessionId::new();
manager.create_session(session_id, "server.example.com", "user");
manager.get_session_mut(session_id)
    .map(|s| s.with_port(2222)
              .with_transport(SshTransport::ProxyJump)
              .with_auth_method(AuthMethod::Agent));

// Conectar
manager.connect(session_id)?;

// Criar a partir de ~/.ssh/config
manager.create_from_ssh_config(session_id, "myserver")?;
```

#### Métodos de Autenticação

| Método | Descrição |
|--------|-----------|
| `PublicKey` | Chave SSH pública |
| `Password` | Senha |
| `Agent` | SSH Agent |
| `Kerberos` | Autenticação Kerberos |
| `KeyboardInteractive` | Interativo |

#### Tipos de Transporte

| Tipo | Descrição |
|------|-----------|
| `Direct` | Conexão direta |
| `ProxyJump` | Via jump host |
| `SocksProxy` | Via proxy SOCKS |
| `Tunnel` | Via túnel SSH |

### 6. Commands (`commands.rs`)

Comandos do multiplexador similares ao tmux.

```rust
use code_terminal::multiplexer::{CommandParser, MuxCommand, SplitDirection};

// Parse de comandos
let cmd = CommandParser::parse("new-session -s mysession")?;
let cmd = CommandParser::parse("split-window -v")?;
let cmd = CommandParser::parse("select-pane -U")?;

// Keybindings padrão
let bindings = default_keybindings();
// C-b c: Nova janela
// C-b %: Dividir vertical
// C-b ": Dividir horizontal
// C-b d: Desanexar
```

#### Comandos Disponíveis

##### Sessão
- `new-session`, `new` - Cria nova sessão
- `attach-session`, `attach`, `a` - Anexa a sessão
- `detach-client`, `detach`, `d` - Desanexa
- `kill-session` - Mata sessão
- `rename-session` - Renomeia sessão
- `list-sessions`, `ls` - Lista sessões

##### Janela
- `new-window`, `neww` - Nova janela
- `kill-window`, `killw` - Fecha janela
- `select-window`, `selectw` - Seleciona janela
- `next-window`, `next`, `n` - Próxima janela
- `previous-window`, `prev`, `p` - Janela anterior
- `rename-window`, `renamew` - Renomeia janela
- `swap-window`, `swapw` - Troca janelas

##### Painel
- `split-window`, `splitw` - Divide painel
- `kill-pane`, `killp` - Fecha painel
- `select-pane`, `selectp` - Seleciona painel
- `resize-pane`, `resizep` - Redimensiona painel
- `swap-pane`, `swapp` - Troca painéis
- `select-layout` - Seleciona layout

### 7. Persistence (`persistence.rs`)

Persistência de sessões entre reinicializações.

```rust
use code_terminal::multiplexer::{SessionPersistence, PersistenceConfig};

let config = PersistenceConfig::default()
    .with_auto_save(true)
    .with_save_interval(60);

let persistence = SessionPersistence::with_config(config);
persistence.save_session(&session)?;
let restored = persistence.load_session(session_id)?;
```

### 8. Monitor (`monitor.rs`)

Monitoramento de saúde e estatísticas das sessões.

```rust
use code_terminal::multiplexer::{SessionMonitor, HealthReport};

let monitor = SessionMonitor::new();
let stats = monitor.global_stats();
let health = monitor.check_health();
```

## Exemplos Completos

### Exemplo 1: Sessão Básica

```rust
use code_terminal::multiplexer::{
    MultiplexerSession, SessionId, WindowManager, PaneManager
};

fn create_dev_session() -> MultiplexerResult<SessionId> {
    // Criar sessão
    let session = MultiplexerSession::new(
        Some("dev".to_string()),
        (80, 24)
    );
    
    // Criar janelas
    let mut windows = WindowManager::new();
    
    // Janela 1: Editor
    let editor = windows.create_window(
        Some("editor".to_string()),
        (80, 24)
    );
    
    // Janela 2: Terminal
    let terminal = windows.create_window(
        Some("terminal".to_string()),
        (80, 24)
    );
    
    // Janela 3: Logs
    let logs = windows.create_window(
        Some("logs".to_string()),
        (80, 24)
    );
    
    Ok(session.id)
}
```

### Exemplo 2: Sessão Compartilhada com SSH

```rust
use code_terminal::multiplexer::{
    SharedSessionManager, SshCompatManager, 
    SharedSessionConfig, ClientPermissions
};

fn create_shared_remote_session() -> MultiplexerResult<()> {
    // Configurar sessão compartilhada
    let shared_config = SharedSessionConfig::default()
        .with_max_clients(3)
        .with_default_permissions(ClientPermissions::read_only())
        .with_password("team-secret");
    
    // Criar gerenciadores
    let mut ssh_manager = SshCompatManager::new();
    let mut shared_manager = SharedSessionManager::new();
    
    // Criar sessão SSH compartilhada
    let session_id = SessionId::new();
    ssh_manager.create_shared_ssh_session(
        session_id,
        "prod-server.example.com",
        "deploy",
        shared_config
    )?;
    
    // Clientes podem conectar com diferentes permissões
    let client1 = ClientId::new();
    let client2 = ClientId::new();
    
    // Cliente 1: Administrador
    shared_manager.connect_client(client1, session_id, Some("team-secret"))?;
    if let Some(session) = shared_manager.get_client_session(client1) {
        session.update_permissions(client1, ClientPermissions::admin())?;
    }
    
    // Cliente 2: Somente leitura
    shared_manager.connect_client(client2, session_id, Some("team-secret"))?;
    
    Ok(())
}
```

### Exemplo 3: Layout de Desenvolvimento

```rust
use code_terminal::multiplexer::{
    WindowManager, WindowLayout, PaneManager
};

fn create_dev_layout() {
    let mut windows = WindowManager::new();
    
    // Janela principal com layout tiled
    let main = windows.create_window(Some("main".to_string()), (120, 40));
    
    if let Some(window) = windows.get_window_mut(main) {
        // Criar painéis
        window.split_active_pane(true, Some(30)).unwrap();  // 30% esquerda
        window.split_active_pane(false, Some(50)).unwrap(); // 50% baixo
        
        // Aplicar layout tiled
        window.set_layout(WindowLayout::Tiled);
    }
    
    // Janela de monitoramento
    let monitor = windows.create_window(Some("monitor".to_string()), (120, 40));
    if let Some(window) = windows.get_window_mut(monitor) {
        window.set_layout(WindowLayout::MainHorizontal);
    }
}
```

### Exemplo 4: Comandos Programáticos

```rust
use code_terminal::multiplexer::{
    CommandParser, MuxCommand, 
    WindowTarget, PaneDirection
};

fn execute_commands() -> MultiplexerResult<()> {
    // Criar sessão
    let cmd = CommandParser::parse("new-session -s work")?;
    
    // Nova janela com comando
    let cmd = CommandParser::parse("new-window -n vim vim")?;
    
    // Dividir painel
    let cmd = CommandParser::parse("split-window -h -p 30")?;
    
    // Navegar
    let cmd = CommandParser::parse("select-pane -L")?;
    let cmd = CommandParser::parse("next-window")?;
    
    // Renomear
    let cmd = CommandParser::parse("rename-window coding")?;
    
    // Listar
    let cmd = CommandParser::parse("list-panes")?;
    
    Ok(())
}
```

## Integração com Terminal

```rust
// Abrir terminal em modo multiplexador
terminal.enter_multiplexer_mode();

// Criar nova sessão
terminal.multiplexer_new_session(Some("work"));

// Anexar a sessão existente
terminal.multiplexer_attach("work");

// Executar comando do multiplexador
terminal.multiplexer_command("new-window");

// Sair do modo multiplexador (desanexar)
terminal.multiplexer_detach();
```

## Configuração

### Arquivo de Configuração

```toml
# ~/.config/codeterm/multiplexer.toml
[multiplexer]
enabled = true
auto_attach = true
default_session_name = "main"

[multiplexer.shared]
max_clients = 10
idle_timeout = 3600
sync_mode = "independent"

[multiplexer.ssh]
use_ssh_config = true
use_ssh_agent = true
connection_timeout = 30
strict_host_key_checking = true

[multiplexer.keybindings]
prefix = "C-b"  # Ctrl+B
detach = "C-b d"
new_window = "C-b c"
split_vertical = "C-b %"
split_horizontal = 'C-b "'
```

### Variáveis de Ambiente

| Variável | Descrição | Padrão |
|----------|-----------|--------|
| `CODETERM_MUX_SOCKET` | Caminho do socket | `~/.local/share/codeterm/mux.sock` |
| `CODETERM_MUX_DEFAULT_SESSION` | Sessão padrão | - |
| `CODETERM_MUX_PREFIX` | Prefixo de keybinding | `C-b` |
| `SSH_AUTH_SOCK` | Socket do SSH agent | - |

## Boas Práticas

### 1. Nomenclatura de Sessões

```rust
// Use nomes descritivos
let session = MultiplexerSession::new(
    Some("project-backend".to_string()),
    (80, 24)
);

// Ou use nomes por contexto
let session = MultiplexerSession::new(
    Some(format!("{}-dev", project_name)),
    (80, 24)
);
```

### 2. Gerenciamento de Recursos

```rust
// Limite o número de clientes
let config = SharedSessionConfig::default()
    .with_max_clients(5);

// Configure timeout de inatividade
let config = config.with_idle_timeout(1800); // 30 min

// Limpe sessões vazias periodicamente
manager.cleanup_empty_sessions();
```

### 3. Segurança em Sessões Compartilhadas

```rust
// Use senhas para sessões sensíveis
let config = SharedSessionConfig::default()
    .with_password("secure-password");

// Dê permissões mínimas por padrão
let config = config.with_default_permissions(
    ClientPermissions::read_only()
);

// Revogue permissões quando necessário
session.update_permissions(client_id, new_perms)?;
```

### 4. Organização

```rust
// Use grupos para organizar sessões
let mut groups = GroupManager::new();
let work_group = groups.create_group("work");
let personal_group = groups.create_group("personal");

groups.add_to_group(session_id, work_group)?;
```

## Erros Comuns

| Erro | Causa | Solução |
|------|-------|---------|
| `SessionNotFound` | Sessão não existe | Verifique o ID/nome |
| `ClientNotFound` | Cliente não conectado | Conecte primeiro |
| `AlreadyAttached` | Já anexado a uma sessão | Desanexe primeiro |
| `SessionLocked` | Sessão bloqueada | Desbloqueie ou use outra |
| `InvalidPassword` | Senha incorreta | Verifique a senha |
| `SessionFull` | Máximo de clientes atingido | Aumente limite ou espere |

## Referências

- [tmux](https://github.com/tmux/tmux) - Terminal Multiplexer
- [GNU Screen](https://www.gnu.org/software/screen/) - Screen Multiplexer
- [OpenSSH Multiplexing](https://en.wikibooks.org/wiki/OpenSSH/Cookbook/Multiplexing)

## Veja Também

- [`src/multiplexer/session.rs`](../src/multiplexer/session.rs) - Implementação de sessões
- [`src/multiplexer/window.rs`](../src/multiplexer/window.rs) - Sistema de janelas
- [`src/multiplexer/pane.rs`](../src/multiplexer/pane.rs) - Sistema de painéis
- [`src/multiplexer/shared.rs`](../src/multiplexer/shared.rs) - Sessões compartilhadas
- [`src/multiplexer/ssh_compat.rs`](../src/multiplexer/ssh_compat.rs) - Compatibilidade SSH
