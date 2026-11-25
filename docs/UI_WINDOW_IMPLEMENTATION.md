# UI Window - Implementação de Múltiplas Janelas

## Resumo

Implementação do sistema de múltiplas janelas para o code-terminal, onde cada janela possui suas próprias tabs e splits independentes.

## Arquitetura

```
Application
    |
    v
WindowManager
    |
    ├── Window 1 (ativa)
    |       ├── TabManager (próprio)
    |       ├── LayoutManager (próprio)
    |       └── winit Window
    |
    ├── Window 2
    |       ├── TabManager (próprio)
    |       ├── LayoutManager (próprio)
    |       └── winit Window
    |
    └── Window N
```

## Componentes

### WindowId
Identificador único de uma janela (`u64` atomically incremented).

### WindowState
Enumeração dos estados possíveis de uma janela:
- `Active` - Janela focada
- `Inactive` - Janela em background
- `Minimized` - Minimizada
- `Maximized` - Maximizada
- `Closing` - Em processo de fechamento

### Window
Estrutura que representa uma janela individual:
- `id: WindowId` - ID único
- `winit_window: Arc<WinitWindow>` - Referência à janela winit
- `tab_manager: TabManager` - Gerenciador de tabs próprio
- `layout_manager: LayoutManager` - Gerenciador de splits próprio
- `state: WindowState` - Estado atual
- `app_state: Option<AppState>` - Estado da aplicação

### WindowManager
Gerenciador central de múltiplas janelas:
- Mantém mapa de janelas (`HashMap<WindowId, Window>`)
- Gerencia janela ativa
- Processa eventos winit
- Comunicação entre janelas via `EventLoopProxy`

### WindowEventMsg
Eventos de comunicação entre janelas:
- `CreateWindow` - Criar nova janela
- `CloseWindow` - Fechar janela específica
- `FocusWindow` - Focar janela
- `MoveWindow` - Mover janela
- `ResizeWindow` - Redimensionar janela
- `Broadcast` - Enviar mensagem para todas

## Uso

### Criar Janela Principal
```rust
let mut window_manager = WindowManager::new(config);
let event_loop = EventLoop::with_user_event().build()?;

let window_id = window_manager.create_primary_window(
    WindowCreateInfo {
        title: "Code Terminal".to_string(),
        size: (800, 600),
        position: None,
        config: WindowConfig::default(),
    },
    || {
        // Factory function para criar winit window
        let window_attributes = winit::window::WindowAttributes::default()
            .with_title("Code Terminal")
            .with_inner_size(PhysicalSize::new(800, 600));
        Ok(Arc::new(event_loop.create_window(window_attributes)?))
    }
)?;
```

### Criar Janela Adicional
```rust
let new_window_id = window_manager.create_window(
    WindowCreateInfo::default(),
    || {
        let window_attributes = winit::window::WindowAttributes::default()
            .with_title("Code Terminal - New Window")
            .with_inner_size(PhysicalSize::new(800, 600));
        Ok(Arc::new(event_loop.create_window(window_attributes)?))
    }
)?;
```

### Processar Eventos
```rust
fn handle_event(&mut self, event: Event<WindowEventMsg>) {
    match event {
        Event::WindowEvent { window_id, event } => {
            if let Some(id) = self.window_manager.handle_window_event(window_id, &event) {
                if matches!(event, WindowEvent::CloseRequested) {
                    if !self.window_manager.close_window(id) {
                        // Última janela fechada, encerrar aplicação
                        self.exit();
                    }
                }
            }
        }
        Event::UserEvent(window_event) => {
            match window_event {
                WindowEventMsg::CreateWindow { position, size } => {
                    // Criar nova janela
                }
                WindowEventMsg::FocusWindow(id) => {
                    self.window_manager.set_active_window(id);
                }
                // ... outros eventos
            }
        }
        _ => {}
    }
}
```

### Acessar Janela Ativa
```rust
if let Some(window) = window_manager.active_window() {
    // Acessar tabs da janela ativa
    let tab_manager = &window.tab_manager;
    
    // Acessar layout da janela ativa
    let layout_manager = &window.layout_manager;
}
```

## Comunicação Entre Janelas

### Enviar Evento para Janela Específica
```rust
window_manager.send_event(
    target_window_id,
    WindowEventMsg::FocusWindow(target_window_id)
);
```

### Broadcast para Todas as Janelas
```rust
window_manager.broadcast_event(
    WindowEventMsg::Broadcast("Configuração atualizada".to_string())
);
```

## Event Loop Integration

```rust
let event_loop = EventLoop::with_user_event().build()?;
let event_loop_proxy = event_loop.create_proxy();

// Configura o proxy no WindowManager
window_manager.set_event_proxy(event_loop_proxy);

// Event loop principal
event_loop.run(|event, elwt| {
    match event {
        Event::WindowEvent { window_id, event } => {
            // Processa evento da janela
        }
        Event::UserEvent(window_event_msg) => {
            // Processa evento de comunicação entre janelas
        }
        _ => {}
    }
});
```

## Notas de Implementação

### winit 0.30+
A API do winit 0.30 mudou significativamente:
- `WindowBuilder` foi removido
- Criação de janelas é feita via `ActiveEventLoop::create_window()`
- Event loop é construído com `EventLoop::with_user_event().build()`

### Cada Janela é Independente
- Cada janela tem seu próprio `TabManager`
- Cada janela tem seu próprio `LayoutManager`  
- Cada janela pode ter seu próprio `AppState`
- Foco é gerenciado independentemente por janela

### Sincronização
- Comunicação entre janelas via `EventLoopProxy<WindowEventMsg>`
- Mensagens são enviadas de forma assíncrona
- Thread-safe através de Arc<Mutex<>> onde necessário

## Futuras Melhorias

1. **Drag & Drop de Tabs**: Mover tabs entre janelas
2. **Sincronização de Configurações**: Aplicar configurações em todas as janelas
3. **Workspace Management**: Salvar/restaurar todas as janelas de um workspace
4. **Shared State**: Compartilhar estado global (ex: histórico de comandos) entre janelas
5. **Parent-Child Relationships**: Janelas filhas sempre ficam sobre a pai

## Arquivos

- `src/ui/window.rs` - Implementação completa do sistema de janelas
- `src/ui/mod.rs` - Re-exports dos tipos principais

## Testes

```bash
# Testar módulo window
cargo test --lib ui::window::

# Verificar compilação
cargo check --lib
```
