# Window Manager

**Versão:** 1.0.0  
**Última Atualização:** Março 2026  
**Status:** Implementado

---

## Índice

1. [Visão Geral](#visão-geral)
2. [Arquitetura](#arquitetura)
3. [Factory Pattern (winit 0.30+)](#factory-pattern-winit-030)
4. [EventLoopProxy](#eventloopproxy)
5. [Estados de Janela](#estados-de-janela)
6. [API do WindowManager](#api-do-windowmanager)
7. [Exemplos de Uso](#exemplos-de-uso)
8. [Integração com outros Módulos](#integração-com-outros-módulos)

---

## Visão Geral

O `WindowManager` gerencia múltiplas janelas do terminal, onde cada janela tem:
- Seu próprio `TabManager`
- Seu próprio `LayoutManager` (splits)
- Seu próprio `AppState`
- Sua própria instância `GpuState`

Isso permite um verdadeiro suporte multi-window, onde cada janela é independente.

### Features

- **Criação Dinâmica**: Criar novas janelas em tempo de execução
- **Comunicação**: Mensagens entre janelas via `EventLoopProxy`
- **Gerenciamento de Foco**: Tracking de janela ativa
- **Estados**: Active, Inactive, Minimized, Maximized, Closing
- **Mapeamento**: Mapeamento entre `WinitWindowId` e nosso `WindowId`

---

## Arquitetura

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           Application                                    │
│                                                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                      WindowManager                               │   │
│  │                                                                  │   │
│  │  windows: HashMap<WindowId, Window>                             │   │
│  │  winit_to_id: HashMap<WinitWindowId, WindowId>                  │   │
│  │  active_window: Option<WindowId>                                │   │
│  │  event_proxy: Option<EventLoopProxy<WindowEventMsg>>            │   │
│  │                                                                  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                     │
│              ┌─────────────────────┼─────────────────────┐               │
│              ▼                     ▼                     ▼               │
│  ┌─────────────────┐   ┌─────────────────┐   ┌─────────────────┐       │
│  │    Window 1     │   │    Window 2     │   │    Window N     │       │
│  │   (Ativa)       │   │   (Inativa)     │   │   (Minimizada)  │       │
│  ├─────────────────┤   ├─────────────────┤   ├─────────────────┤       │
│  │ winit_window    │   │ winit_window    │   │ winit_window    │       │
│  │ tab_manager     │   │ tab_manager     │   │ tab_manager     │       │
│  │ layout_manager  │   │ layout_manager  │   │ layout_manager  │       │
│  │ app_state       │   │ app_state       │   │ app_state       │       │
│  │ gpu_state       │   │ gpu_state       │   │ gpu_state       │       │
│  └─────────────────┘   └─────────────────┘   └─────────────────┘       │
│                                                                          │
└─────────────────────────────────────────────────────────────────────────┘
```

### Estrutura do WindowManager

```rust
pub struct WindowManager {
    /// Mapa de janelas por ID
    windows: HashMap<WindowId, Window>,
    /// Mapa de Winit WindowId para nosso WindowId
    winit_to_id: HashMap<WinitWindowId, WindowId>,
    /// ID da janela ativa
    active_window: Option<WindowId>,
    /// Contador de janelas abertas
    window_count: usize,
    /// Canal de comunicação entre janelas
    event_proxy: Option<EventLoopProxy<WindowEventMsg>>,
    /// Configuração global
    config: Config,
}
```

### Estrutura da Window

```rust
pub struct Window {
    /// ID único da janela
    pub id: WindowId,
    /// Referência à winit window
    pub winit_window: Arc<WinitWindow>,
    /// Gerenciador de tabs desta janela
    pub tab_manager: TabManager,
    /// Gerenciador de layout/splits desta janela
    pub layout_manager: LayoutManager,
    /// Estado atual da janela
    pub state: WindowState,
    /// Configuração da janela
    pub config: WindowConfig,
    /// Estado da aplicação específico desta janela
    pub app_state: Option<AppState>,
    /// GPU state para renderização
    pub gpu_state: Option<GpuState>,
}
```

---

## Factory Pattern (winit 0.30+)

O winit 0.30+ introduziu mudanças significativas na criação de janelas. Agora a criação é feita via `ActiveEventLoop` usando um padrão Factory.

### Criação de Janela

```rust
impl WindowManager {
    /// Cria a primeira janela (janela principal)
    pub fn create_primary_window<F>(
        &mut self,
        create_info: WindowCreateInfo,
        window_factory: F,
    ) -> Result<WindowId>
    where
        F: FnOnce() -> Result<Arc<WinitWindow>>,
    {
        let window_id = WindowId::new();
        
        // Usa a factory para criar a janela winit
        let winit_window = window_factory()
            .map_err(|e| anyhow::anyhow!("Failed to create window: {}", e))?;
        
        let mut window = Window::new(window_id, winit_window.clone(), create_info.config)?;
        
        // Inicializa AppState
        window.initialize_app_state(80, 24)?;
        window.update_title();
        
        // Registra mapeamento
        self.winit_to_id.insert(winit_window.id(), window_id);
        self.windows.insert(window_id, window);
        self.active_window = Some(window_id);
        self.window_count += 1;
        
        info!(window_id = ?window_id, "Primary window created");
        Ok(window_id)
    }
}
```

### Uso com EventLoop

```rust
use winit::event_loop::{EventLoop, ActiveEventLoop};
use winit::window::WindowAttributes;

fn main() -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut window_manager = WindowManager::new(Config::default());
    
    event_loop.run(move |event, active_event_loop| {
        match event {
            Event::Resumed => {
                // Cria janela principal usando factory
                let window_id = window_manager.create_primary_window(
                    WindowCreateInfo::default(),
                    || {
                        let attributes = WindowAttributes::default()
                            .with_title("Code Terminal")
                            .with_inner_size(LogicalSize::new(800, 600));
                        
                        active_event_loop.create_window(attributes)
                            .map(Arc::new)
                    },
                ).unwrap();
            }
            // ...
        }
    })?;
    
    Ok(())
}
```

### Criando Janelas Adicionais

```rust
// Cria nova janela em tempo de execução
fn criar_nova_janela(
    window_manager: &mut WindowManager,
    active_event_loop: &ActiveEventLoop,
) -> Result<WindowId> {
    let create_info = WindowCreateInfo {
        title: "Terminal 2".to_string(),
        size: (800, 600),
        position: Some((100, 100)),
        config: WindowConfig::default(),
    };
    
    window_manager.create_window(create_info, || {
        let attributes = WindowAttributes::default()
            .with_title(&create_info.title)
            .with_inner_size(PhysicalSize::new(create_info.size.0, create_info.size.1))
            .with_position(PhysicalPosition::new(
                create_info.position.unwrap_or((0, 0)).0,
                create_info.position.unwrap_or((0, 0)).1,
            ));
        
        active_event_loop.create_window(attributes)
            .map(Arc::new)
    })
}
```

---

## EventLoopProxy

O `EventLoopProxy` permite comunicação thread-safe entre janelas.

### Mensagens de Evento

```rust
#[derive(Debug, Clone)]
pub enum WindowEventMsg {
    /// Criar nova janela
    CreateWindow {
        position: Option<(i32, i32)>,
        size: (u32, u32),
    },
    /// Fechar janela
    CloseWindow(WindowId),
    /// Focar janela
    FocusWindow(WindowId),
    /// Mover janela para posição
    MoveWindow(WindowId, i32, i32),
    /// Redimensionar janela
    ResizeWindow(WindowId, u32, u32),
    /// Broadcast de evento para todas as janelas
    Broadcast(String),
}
```

### Configuração do Proxy

```rust
impl WindowManager {
    /// Define o proxy de eventos para comunicação
    pub fn set_event_proxy(&mut self, proxy: EventLoopProxy<WindowEventMsg>) {
        self.event_proxy = Some(proxy);
    }
}

// Durante inicialização
let event_loop = EventLoop::new()?;
let proxy = event_loop.create_proxy();
window_manager.set_event_proxy(proxy);
```

### Enviando Mensagens

```rust
impl WindowManager {
    /// Envia evento para uma janela específica
    pub fn send_event(&self, _window_id: WindowId, event: WindowEventMsg) {
        if let Some(proxy) = &self.event_proxy {
            let _ = proxy.send_event(event);
        }
    }
    
    /// Broadcast evento para todas as janelas
    pub fn broadcast_event(&self, event: WindowEventMsg) {
        if let Some(proxy) = &self.event_proxy {
            let _ = proxy.send_event(event);
        }
    }
}

// Uso
window_manager.send_event(window_id, WindowEventMsg::FocusWindow(window_id));

window_manager.broadcast_event(WindowEventMsg::Broadcast("config_updated".to_string()));
```

### Recebendo Mensagens

```rust
event_loop.run(move |event, active_event_loop| {
    match event {
        Event::UserEvent(window_event) => {
            match window_event {
                WindowEventMsg::CreateWindow { position, size } => {
                    // Cria nova janela
                }
                WindowEventMsg::CloseWindow(id) => {
                    window_manager.close_window(id);
                }
                WindowEventMsg::FocusWindow(id) => {
                    window_manager.set_active_window(id);
                }
                WindowEventMsg::Broadcast(msg) => {
                    // Processa broadcast
                }
                _ => {}
            }
        }
        // ...
    }
})?;
```

---

## Estados de Janela

### Enum WindowState

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// Janela está ativa (focada)
    Active,
    /// Janela está inativa
    Inactive,
    /// Janela está minimizada
    Minimized,
    /// Janela está maximizada
    Maximized,
    /// Janela está fechando
    Closing,
}

impl WindowState {
    /// Verifica se a janela está ativa
    pub fn is_active(&self) -> bool {
        matches!(self, WindowState::Active)
    }
}
```

### Transições de Estado

```
┌─────────┐     Focus      ┌─────────┐
│ Inactive│───────────────▶│ Active  │
│         │◀───────────────│         │
└────┬────┘    Unfocus     └────┬────┘
     │                          │
     │ Minimize                 │ Maximize
     ▼                          ▼
┌─────────┐              ┌─────────┐
│Minimized│              │Maximized│
└────┬────┘              └────┬────┘
     │                          │
     │ Restore                  │ Restore
     ▼                          ▼
└─────────┐              ┌─────────┐
│ Closing │◀─────────────│  Any    │
└─────────┘   Close      └─────────┘
```

### Atualização de Estado

```rust
impl Window {
    /// Define o estado da janela
    pub fn set_state(&mut self, state: WindowState) {
        self.state = state;
        debug!(window_id = ?self.id, state = ?state, "Window state changed");
    }
    
    /// Verifica se a janela está ativa
    pub fn is_active(&self) -> bool {
        self.state == WindowState::Active
    }
}

// Atualização via eventos do winit
pub fn handle_window_event(&mut self, event: &WindowEvent) {
    match event {
        WindowEvent::Focused(focused) => {
            self.set_state(if *focused {
                WindowState::Active
            } else {
                WindowState::Inactive
            });
        }
        WindowEvent::CloseRequested => {
            self.set_state(WindowState::Closing);
        }
        _ => {}
    }
}
```

---

## API do WindowManager

### Construção

```rust
impl WindowManager {
    /// Cria um novo gerenciador de janelas
    pub fn new(config: Config) -> Self;
    
    /// Define o proxy de eventos para comunicação
    pub fn set_event_proxy(&mut self, proxy: EventLoopProxy<WindowEventMsg>);
}
```

### Criação de Janelas

```rust
impl WindowManager {
    /// Cria a primeira janela (janela principal)
    pub fn create_primary_window<F>(
        &mut self,
        create_info: WindowCreateInfo,
        window_factory: F,
    ) -> Result<WindowId>
    where
        F: FnOnce() -> Result<Arc<WinitWindow>>;
    
    /// Cria uma nova janela adicional
    pub fn create_window<F>(
        &mut self,
        create_info: WindowCreateInfo,
        window_factory: F,
    ) -> Result<WindowId>
    where
        F: FnOnce() -> Result<Arc<WinitWindow>>;
}
```

### Acesso a Janelas

```rust
impl WindowManager {
    /// Obtém uma janela pelo ID
    pub fn get_window(&self, window_id: WindowId) -> Option<&Window>;
    
    /// Obtém uma janela mutável pelo ID
    pub fn get_window_mut(&mut self, window_id: WindowId) -> Option<&mut Window>;
    
    /// Obtém uma janela pelo Winit WindowId
    pub fn get_window_by_winit_id(&self, winit_id: WinitWindowId) -> Option<&Window>;
    
    /// Obtém uma janela mutável pelo Winit WindowId
    pub fn get_window_mut_by_winit_id(&mut self, winit_id: WinitWindowId) -> Option<&mut Window>;
    
    /// Retorna a janela ativa
    pub fn active_window(&self) -> Option<&Window>;
    
    /// Retorna a janela ativa (mutável)
    pub fn active_window_mut(&mut self) -> Option<&mut Window>;
    
    /// Retorna o ID da janela ativa
    pub fn active_window_id(&self) -> Option<WindowId>;
}
```

### Gerenciamento de Janelas

```rust
impl WindowManager {
    /// Fecha uma janela específica
    /// Retorna false se não há mais janelas abertas
    pub fn close_window(&mut self, window_id: WindowId) -> bool;
    
    /// Define a janela ativa
    pub fn set_active_window(&mut self, window_id: WindowId);
    
    /// Retorna o número de janelas abertas
    pub fn window_count(&self) -> usize;
    
    /// Verifica se há janelas abertas
    pub fn has_windows(&self) -> bool;
    
    /// Retorna iterador sobre todas as janelas
    pub fn windows(&self) -> impl Iterator<Item = (&WindowId, &Window)>;
    
    /// Retorna iterador mutável sobre todas as janelas
    pub fn windows_mut(&mut self) -> impl Iterator<Item = (&WindowId, &mut Window)>;
}
```

### Eventos

```rust
impl WindowManager {
    /// Processa eventos de janela do winit
    /// Retorna Some(WindowId) se o evento requer fechar a janela
    pub fn handle_window_event(
        &mut self,
        winit_id: WinitWindowId,
        event: &WindowEvent,
    ) -> Option<WindowId>;
    
    /// Envia evento para uma janela específica
    pub fn send_event(&self, window_id: WindowId, event: WindowEventMsg);
    
    /// Broadcast evento para todas as janelas
    pub fn broadcast_event(&self, event: WindowEventMsg);
    
    /// Atualiza o título de todas as janelas
    pub fn update_all_titles(&mut self);
}
```

---

## Exemplos de Uso

### Exemplo 1: Inicialização Básica

```rust
use winit::event_loop::EventLoop;
use code_terminal::ui::window::{WindowManager, WindowCreateInfo};

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut window_manager = WindowManager::new(Config::default());
    
    // Configura proxy para comunicação
    let proxy = event_loop.create_proxy();
    window_manager.set_event_proxy(proxy);
    
    event_loop.run(move |event, active_event_loop| {
        match event {
            winit::event::Event::Resumed => {
                if window_manager.window_count() == 0 {
                    // Cria janela principal
                    let _ = window_manager.create_primary_window(
                        WindowCreateInfo::default(),
                        || {
                            let attrs = WindowAttributes::default()
                                .with_title("Code Terminal")
                                .with_inner_size(LogicalSize::new(800, 600));
                            active_event_loop.create_window(attrs).map(Arc::new)
                        },
                    );
                }
            }
            // ...
        }
    })?;
    
    Ok(())
}
```

### Exemplo 2: Gerenciando Múltiplas Janelas

```rust
fn gerenciar_multiplas_janelas(window_manager: &mut WindowManager) {
    // Lista todas as janelas
    for (id, window) in window_manager.windows() {
        println!("Janela {:?}: {:?}", id, window.state);
    }
    
    // Alterna entre janelas
    let ids: Vec<_> = window_manager.windows().map(|(id, _)| *id).collect();
    if ids.len() > 1 {
        window_manager.set_active_window(ids[1]);
    }
    
    // Atualiza títulos
    window_manager.update_all_titles();
}
```

### Exemplo 3: Comunicação entre Janelas

```rust
fn comunicar_entre_janelas(window_manager: &WindowManager) {
    // Envia mensagem para janela específica
    if let Some(active_id) = window_manager.active_window_id() {
        window_manager.send_event(
            active_id,
            WindowEventMsg::Broadcast("hello_from_other_window".to_string())
        );
    }
    
    // Broadcast para todas
    window_manager.broadcast_event(
        WindowEventMsg::Broadcast("config_changed".to_string())
    );
}
```

### Exemplo 4: Processando Eventos

```rust
fn processar_eventos(
    window_manager: &mut WindowManager,
    event: winit::event::WindowEvent,
    winit_id: winit::window::WindowId,
) -> bool {
    // Delega para o WindowManager
    if let Some(window_id) = window_manager.handle_window_event(winit_id, &event) {
        // Evento solicitou fechar janela
        let has_windows = window_manager.close_window(window_id);
        
        if !has_windows {
            // Fecha aplicação se não há mais janelas
            return false; // Signal to exit
        }
    }
    
    true // Continue running
}
```

### Exemplo 5: Renderização Multi-Window

```rust
fn renderizar_todas_janelas(window_manager: &mut WindowManager) {
    for (_, window) in window_manager.windows_mut() {
        if window.state == WindowState::Minimized {
            continue; // Skip minimized windows
        }
        
        if let Err(e) = window.render() {
            eprintln!("Erro renderizando janela {:?}: {}", window.id, e);
        }
    }
}
```

### Exemplo 6: Callbacks por Janela

```rust
fn configurar_callbacks(window_manager: &mut WindowManager, window_id: WindowId) {
    if let Some(window) = window_manager.get_window_mut(window_id) {
        // Configura callbacks do layout manager
        window.layout_manager.on_pane_activated(|pane_id| {
            println!("Pane {:?} ativado na janela {:?}", pane_id, window_id);
        });
        
        // Atualiza título baseado na tab ativa
        window.update_title();
    }
}
```

---

## Integração com outros Módulos

### Com Sistema de Splits

```rust
// Cada janela tem seu próprio LayoutManager
pub struct Window {
    pub layout_manager: LayoutManager,
    // ...
}

// Uso
if let Some(window) = window_manager.active_window_mut() {
    // Cria split na janela ativa
    window.layout_manager.split_vertical(TabManager::new());
}
```

### Com Sistema de Tabs

```rust
// Cada janela tem seu próprio TabManager (no pane ativo)
if let Some(window) = window_manager.active_window_mut() {
    if let Some(pane) = window.layout_manager.active_pane_mut() {
        // Cria nova tab no pane ativo da janela ativa
        pane.tab_manager.create_tab("bash");
    }
}
```

### Com Renderer

```rust
// Cada janela tem seu próprio GpuState
impl Window {
    pub async fn initialize_gpu(&mut self) -> Result<()> {
        let gpu = GpuState::new(self.winit_window.clone()).await?;
        self.gpu_state = Some(gpu);
        Ok(())
    }
    
    pub fn render(&mut self) -> Result<()> {
        if let Some(ref mut gpu) = self.gpu_state {
            // Renderiza conteúdo da janela
            gpu.render()?;
        }
        Ok(())
    }
}
```

### Com AppState

```rust
// Cada janela tem seu próprio AppState
impl Window {
    pub fn initialize_app_state(&mut self, cols: u16, rows: u16) -> Result<()> {
        self.app_state = Some(AppState::new(cols, rows)?);
        Ok(())
    }
}

// Acesso
if let Some(window) = window_manager.active_window() {
    if let Some(app) = window.app_state() {
        // Usa AppState específico desta janela
        println!("Tabs: {}", app.tab_count());
    }
}
```

---

## Referências

- [Arquitetura Geral](ARCHITECTURE.md)
- [Sistema de Splits](SPLITS_SYSTEM.md)
- [UI Window Implementation](UI_WINDOW_IMPLEMENTATION.md)
- [winit 0.30 Migration Guide](https://docs.rs/winit/latest/winit/)

---

**Maintained by:** Development Team  
**Last Updated:** Março 2026
