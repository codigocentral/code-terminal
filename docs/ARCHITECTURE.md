# Arquitetura do Code Terminal

**Versão:** 2.0.0  
**Última Atualização:** Março 2026  
**Status:** Implementado

---

## Índice

1. [Visão Geral](#visão-geral)
2. [Diagrama de Componentes](#diagrama-de-componentes)
3. [Fluxo de Dados](#fluxo-de-dados)
4. [Módulos Principais](#módulos-principais)
5. [Arquitetura de WindowManager](#arquitetura-de-windowmanager)
6. [Sistema de Splits](#sistema-de-splits)
7. [Sistema de Tabs](#sistema-de-tabs)
8. [Integração entre Módulos](#integração-entre-módulos)

---

## Visão Geral

O Code Terminal é um emulador de terminal moderno construído em Rust com arquitetura modular e multi-camadas. A arquitetura foi projetada para suportar:

- **Múltiplas janelas** com estado independente
- **Splits dinâmicos** (tmux-like)
- **Sistema de plugins** em WASM com sandbox
- **GPU Rendering** via wgpu
- **Multiplexer mode** (sessões persistentes)

### Princípios Arquiteturais

1. **Separação de Responsabilidades**: Cada módulo tem uma função clara
2. **Composição sobre Herança**: Componentes são combinados via traits
3. **Thread-Safety**: Uso extensivo de `Arc<RwLock<T>>` para dados compartilhados
4. **Zero-Copy**: Onde possível, dados são passados por referência
5. **Lazy Evaluation**: Renderização e processamento sob demanda

---

## Diagrama de Componentes

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              Application                                 │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                        WindowManager                            │   │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐             │   │
│  │  │  Window 1   │  │  Window 2   │  │  Window N   │             │   │
│  │  │             │  │             │  │             │             │   │
│  │  │ ┌─────────┐ │  │ ┌─────────┐ │  │ ┌─────────┐ │             │   │
│  │  │ │SplitTree│ │  │ │SplitTree│ │  │ │SplitTree│ │             │   │
│  │  │ │(Layout) │ │  │ │(Layout) │ │  │ │(Layout) │ │             │   │
│  │  │ └───┬─────┘ │  │ └───┬─────┘ │  │ └───┬─────┘ │             │   │
│  │  │     │       │  │     │       │  │     │       │             │   │
│  │  │ ┌───┴───┐   │  │ ┌───┴───┐   │  │ ┌───┴───┐   │             │   │
│  │  │ │ Panes │   │  │ │ Panes │   │  │ │ Panes │   │             │   │
│  │  │ │┌─────┐│   │  │ │┌─────┐│   │  │ │┌─────┐│   │             │   │
│  │  │ ││Tab 1││   │  │ ││Tab 1││   │  │ ││Tab 1││   │             │   │
│  │  │ │├─────┤│   │  │ │├─────┤│   │  │ │├─────┤│   │             │   │
│  │  │ ││Tab 2││   │  │ ││Tab 2││   │  │ ││Tab N││   │             │   │
│  │  │ │└─────┘│   │  │ │└─────┘│   │  │ │└─────┘│   │             │   │
│  │  │ └───────┘   │  │ └───────┘   │  │ └───────┘   │             │   │
│  │  └─────────────┘  └─────────────┘  └─────────────┘             │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                           Core Systems                                  │
├──────────────┬──────────────┬──────────────┬────────────────────────────┤
│   Terminal   │   Renderer   │   Plugin     │   Multiplexer              │
│   ─────────  │   ─────────  │   ─────────  │   ───────────              │
│  • Grid      │  • GPU       │  • WASM      │  • Session                 │
│  • Buffer    │  • Text      │  • Sandbox   │  • Client/Server           │
│  • Parser    │  • Pipeline  │  • Host API  │  • Socket API              │
│  • PTY       │  • Surface   │  • Manifest  │  • Persistence             │
└──────────────┴──────────────┴──────────────┴────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Infrastructure                                  │
├──────────────┬──────────────┬──────────────┬────────────────────────────┤
│   Config     │   Platform   │    Utils     │      AI                    │
│  (TOML)      │  (OS APIs)   │  (Logging)   │  (Providers)               │
└──────────────┴──────────────┴──────────────┴────────────────────────────┘
```

---

## Fluxo de Dados

### Input Flow (Teclado/Mouse → Terminal)

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  Input   │───▶│  winit   │───▶│   UI     │───▶│  Pane    │───▶│  PTY/    │
│ (User)   │    │  Events  │    │ Handler  │    │ Router   │    │ Terminal │
└──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
                                                                     │
                                                                     ▼
                                                              ┌──────────┐
                                                              │  Shell   │
                                                              └──────────┘
```

### Output Flow (Shell → Tela)

```
┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐
│  Shell   │───▶│   PTY    │───▶│  Parser  │───▶│   Grid   │───▶│  GPU     │
│  Output  │    │  (bytes) │    │  (VT)    │    │ (state)  │    │ Render   │
└──────────┘    └──────────┘    └──────────┘    └──────────┘    └──────────┘
                                                                       │
                                                                       ▼
                                                                ┌──────────┐
                                                                │  Screen  │
                                                                └──────────┘
```

---

## Módulos Principais

### 1. UI (`src/ui/`)

Responsável pela interface do usuário, gerenciamento de janelas, splits e tabs.

```rust
// Estrutura principal
pub mod window;      // WindowManager, multi-window support
pub mod splits;      // SplitTree, SplitManager, navegação
pub mod tabs;        // TabManager, abas por pane
pub mod layout;      // LayoutManager, posicionamento
pub mod manager;     // UIManager, coordenação geral
```

**Responsabilidades:**
- Gerenciar múltiplas janelas (WindowManager)
- Dividir tela em múltiplos panes (SplitTree)
- Gerenciar abas em cada pane (TabManager)
- Processar eventos de input (teclado/mouse)
- Coordenar com o renderer

### 2. Terminal (`src/terminal/`)

Emulação de terminal VT100/XTerm completa.

```rust
pub mod grid;        // Grade de caracteres
pub mod buffer;      // TerminalBuffer, integração PTY
pub mod parser;      // Parser de escape sequences
pub mod pty;         // Pseudo-terminal abstraction
pub mod scrollback;  // Buffer de scrollback
```

**Responsabilidades:**
- Manter estado do terminal (Grid)
- Parsear escape sequences (vte crate)
- Comunicar com shell via PTY
- Gerenciar scrollback

### 3. Renderer (`src/renderer/`)

Renderização GPU-accelerated via wgpu.

```rust
pub mod gpu;         // GpuState, setup wgpu
pub mod text;        // Text rendering com glyphon
pub mod pipeline;    // Render pipelines
pub mod surface;     // Surface management
pub mod render_thread; // Thread de renderização
```

**Responsabilidades:**
- Inicializar contexto GPU (wgpu)
- Renderizar texto (glyphon)
- Gerenciar surfaces e swap chains
- Executar em thread separada

### 4. Multiplexer (`src/multiplexer/`)

Modo tmux-like com sessões persistentes.

```rust
pub mod session;     // MultiplexerSession
pub mod server;      // SessionServer
pub mod client;      // SessionClient
pub mod socket;      // Comunicação socket
pub mod socket_api;  // API JSON-RPC
```

**Responsabilidades:**
- Manter sessões persistentes
- Permitir attach/detach
- Comunicação cliente-servidor
- Persistência de estado

### 5. Plugin (`src/plugin/`)

Sistema de extensão via WASM.

```rust
pub mod api;         // Plugin API versioning
pub mod host;        // Host functions (80+)
pub mod sandbox;     // Sandbox WASM
pub mod wasm_runtime;// wasmtime integration
pub mod manager;     // PluginManager
```

**Responsabilidades:**
- Carregar plugins WASM
- Executar em sandbox
- Expor Host Functions
- Gerenciar ciclo de vida

### 6. Pane (`src/pane/`)

Sistema unificado de painéis.

```rust
pub mod unified;     // UnifiedPane, UnifiedPaneManager
pub mod content;     // PaneContent trait
pub mod input;       // InputEvent, InputRouter
```

**Responsabilidades:**
- Unificar representação de panes
- Integrar multiplexer + UI + renderer
- Roteamento de input

---

## Arquitetura de WindowManager

O WindowManager implementa suporte a múltiplas janelas usando o padrão Factory do winit 0.30+.

### Estrutura

```rust
pub struct WindowManager {
    windows: HashMap<WindowId, Window>,
    winit_to_id: HashMap<WinitWindowId, WindowId>,
    active_window: Option<WindowId>,
    event_proxy: Option<EventLoopProxy<WindowEventMsg>>,
}
```

### Factory Pattern (winit 0.30+)

```rust
// Criação de janela via factory
pub fn create_window<F>(
    &mut self,
    create_info: WindowCreateInfo,
    window_factory: F,
) -> Result<WindowId>
where
    F: FnOnce() -> Result<Arc<WinitWindow>>,
{
    let window_id = WindowId::new();
    let winit_window = window_factory()?;
    let window = Window::new(window_id, winit_window, create_info.config)?;
    // ... registro
}
```

### Estados de Janela

```rust
pub enum WindowState {
    Active,      // Focada e visível
    Inactive,    // Visível mas não focada
    Minimized,   // Minimizada
    Maximized,   // Maximizada
    Closing,     // Em processo de fechamento
}
```

### Comunicação entre Janelas

```rust
pub enum WindowEventMsg {
    CreateWindow { position, size },
    CloseWindow(WindowId),
    FocusWindow(WindowId),
    MoveWindow(WindowId, x, y),
    ResizeWindow(WindowId, width, height),
    Broadcast(String),
}

// Uso do EventLoopProxy
pub fn send_event(&self, window_id: WindowId, event: WindowEventMsg) {
    if let Some(proxy) = &self.event_proxy {
        let _ = proxy.send_event(event);
    }
}
```

---

## Sistema de Splits

O sistema de splits permite dividir a tela em múltiplos panes usando uma árvore binária.

### SplitTree (Árvore Binária)

```rust
pub enum SplitNode {
    Split {
        direction: SplitDirection,  // Horizontal | Vertical
        ratio: f32,                  // 0.0 - 1.0
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
    Pane(PaneId),
}
```

### LayoutManager

```rust
pub struct LayoutManager {
    root: Option<SplitNode>,
    panes: HashMap<PaneId, Pane>,
    active_pane: Option<PaneId>,
    window_rect: Rect,
}
```

### Operações

```rust
// Criar split
pub fn split_horizontal(&mut self, tab_manager: TabManager) -> Option<PaneId>
pub fn split_vertical(&mut self, tab_manager: TabManager) -> Option<PaneId>

// Navegação
pub fn navigate(&mut self, direction: NavigationDirection)  // Alt+Arrow
pub fn next_pane(&mut self)
pub fn previous_pane(&mut self)

// Redimensionamento
pub fn resize_active_pane(&mut self, direction: ResizeDirection)
```

### Navegação com Alt+Arrow

```rust
pub fn find_pane_in_direction(&self, direction: SplitDirection) -> Option<PaneId> {
    let current = self.active_pane?;
    let current_pane = self.panes.get(&current)?;
    let current_center = current_pane.rect.center();

    // Encontra o pane mais próximo na direção especificada
    // considerando distância euclidiana
}
```

---

## Sistema de Tabs

Cada pane tem seu próprio TabManager com múltiplas abas.

### Estrutura

```rust
pub struct TabManager {
    tabs: Vec<Tab>,
    active_tab_index: usize,
    search_bar: SearchBar,
}

pub struct Tab {
    pub id: TabId,
    pub title_manager: TabTitle,
    pub buffer: Option<TerminalBuffer>,
    pub state: TabState,
}
```

### Operações

```rust
impl TabManager {
    pub fn create_tab(&mut self, shell: &str) -> TabId;
    pub fn close_tab(&mut self, index: usize) -> bool;
    pub fn switch_tab(&mut self, index: usize);
    pub fn next_tab(&mut self);
    pub fn previous_tab(&mut self);
    
    // Busca
    pub fn toggle_search(&mut self);
    pub fn update_search_query(&mut self, query: &str);
    pub fn next_search_match(&mut self) -> bool;
}
```

### Integração com Splits

```rust
pub struct Pane {
    pub id: PaneId,
    pub tab_manager: TabManager,  // Cada pane tem suas tabs
    pub rect: Rect,
}
```

---

## Integração entre Módulos

### Hierarquia de Estado

```
Application
    └── WindowManager
            └── Window
                    ├── TabManager
                    │       └── Tab
                    │               └── TerminalBuffer
                    │                           ├── Grid
                    │                           └── PTY
                    └── LayoutManager (splits)
                            └── Pane
                                    └── TabManager
```

### Comunicação entre Threads

```rust
// AppState é compartilhado via Arc<RwLock<>>
pub struct AppState {
    pub tab_manager: TabManager,
    pub config: Config,
    pub ssh_manager: Arc<RwLock<SshManager>>,
}

// Comunicação com render thread
pub enum RenderCommand {
    Render,
    Resize { width: u32, height: u32 },
    UpdateGrid(PaneId, Arc<Grid>),
    Shutdown,
}
```

### Event Flow

```
winit EventLoop
    ├── WindowEvent::KeyboardInput
    │       └── App::handle_key_input()
    │               ├── SplitManager::handle_action()
    │               ├── TabManager::switch_tab()
    │               └── TerminalBuffer::write()
    ├── WindowEvent::Resized
    │       └── LayoutManager::set_window_size()
    │               └── recalculate_layout()
    └── WindowEvent::RedrawRequested
            └── Renderer::render()
```

---

## Referências

- [PaneId Unificado](PANE_ID_ARCHITECTURE.md)
- [Sistema de Splits](SPLITS_SYSTEM.md)
- [Plugin API](PLUGIN_API.md)
- [Window Manager](WINDOW_MANAGER.md)
- [UI Window Implementation](UI_WINDOW_IMPLEMENTATION.md)

---

**Maintained by:** Development Team  
**Last Updated:** Março 2026
