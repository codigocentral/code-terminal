# PaneId Unificado - Arquitetura de Identificadores

**Versão:** 1.0.0  
**Última Atualização:** Março 2026  
**Status:** Implementado

---

## Índice

1. [Motivação](#motivação)
2. [Tipos de Identificadores](#tipos-de-identificadores)
3. [Definição em Código](#definição-em-código)
4. [Uso em Cada Módulo](#uso-em-cada-módulo)
5. [Exemplos de Código](#exemplos-de-código)
6. [Migração de Código Antigo](#migração-de-código-antigo)
7. [Referências Cruzadas](#referências-cruzadas)

---

## Motivação

Antes da unificação, o sistema usava diferentes tipos de IDs em cada módulo:

- `multiplexer::PaneId` - Identificava panes no servidor
- `ui::layout::PaneId` - Identificava panes na UI
- `renderer::PaneId` - Identificava panes para renderização

Isso causava:
- **Confusão**: Desenvolvedores não sabiam qual tipo usar
- **Conversões**: Casting constante entre tipos
- **Bugs**: IDs de módulos diferentes sendo comparados

A solução foi criar um tipo `PaneId` único em `src/types.rs` que é re-exportado por todos os módulos.

---

## Tipos de Identificadores

### Visão Geral

```rust
// src/types.rs - Tipos unificados
pub type PaneId    = u64;  // Identifica painéis (pane)
pub type TabId     = u64;  // Identifica abas (tabs)
pub type WindowId  = u64;  // Identifica janelas
pub type SessionId = u64;  // Identifica sessões do multiplexador
```

### Diagrama de Relações

```
┌─────────────────────────────────────────────────────────────────┐
│                        Application                               │
│                                                                  │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐       │
│  │  WindowId   │────▶│  Window     │────▶│  Layout     │       │
│  │  (u64)      │     │             │     │  Manager    │       │
│  └─────────────┘     └─────────────┘     └──────┬──────┘       │
│                                                  │              │
│                                                  ▼              │
│  ┌─────────────┐     ┌─────────────┐     ┌─────────────┐       │
│  │  PaneId     │◀────│    Pane     │◀────│  SplitNode  │       │
│  │  (u64)      │     │             │     │  (Tree)     │       │
│  └──────┬──────┘     └──────┬──────┘     └─────────────┘       │
│         │                   │                                   │
│         │            ┌──────┴──────┐                          │
│         │            ▼             ▼                          │
│  ┌─────────────┐  ┌─────────┐  ┌─────────┐                    │
│  │ Multiplexer │  │ TabId   │  │ TabId   │                    │
│  │   Pane      │  │ Tab 1   │  │ Tab 2   │                    │
│  └─────────────┘  └────┬────┘  └────┬────┘                    │
│                        │            │                          │
│                        ▼            ▼                          │
│                   ┌─────────┐  ┌─────────┐                    │
│                   │Terminal │  │Terminal │                    │
│                   │ Buffer  │  │ Buffer  │                    │
│                   └─────────┘  └─────────┘                    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

---

## Definição em Código

### `src/types.rs` - Fonte da Verdade

```rust
//! Tipos compartilhados entre módulos do code-terminal
//!
//! Este módulo contém definições de tipos que são usados em múltiplos
//! módulos do projeto, garantindo consistência e evitando dependências
//! circulares.

/// Identificador único de um painel (pane)
///
/// Usado para identificar panes em todo o sistema, incluindo:
/// - Multiplexer
/// - Renderizador
/// - Sistema de notificações
/// - Layout da UI
///
/// # Exemplos
///
/// ```
/// use code_terminal::types::PaneId;
///
/// let pane_id: PaneId = 1;
/// let next_id = pane_id + 1;
/// ```
pub type PaneId = u64;

/// Identificador único de uma aba (tab)
///
/// Similar ao PaneId, mas para identificação de abas.
pub type TabId = u64;

/// Identificador único de uma janela
///
/// Usado para identificar janelas no sistema.
pub type WindowId = u64;

/// Identificador único de uma sessão
///
/// Usado para identificar sessões do multiplexador.
pub type SessionId = u64;

/// Gera um novo ID de painel a partir de um contador
///
/// Esta é uma função auxiliar para gerar IDs sequenciais.
/// Para IDs únicos globais, use um contador atômico.
pub fn next_pane_id(counter: &mut PaneId) -> PaneId {
    let id = *counter;
    *counter += 1;
    id
}
```

### Re-exportação nos Módulos

```rust
// src/ui/layout.rs
pub use crate::types::PaneId;

// src/multiplexer/mod.rs  
pub use crate::types::PaneId;

// src/pane/mod.rs
pub use crate::types::PaneId;

// src/renderer/pane.rs
pub use crate::types::PaneId;
```

---

## Uso em Cada Módulo

### 1. UI / Layout (`src/ui/layout.rs`)

```rust
use crate::types::PaneId;

#[derive(Debug)]
pub struct Pane {
    pub id: PaneId,           // ID unificado
    pub tab_manager: TabManager,
    pub rect: Rect,
}

pub struct LayoutManager {
    root: Option<SplitNode>,
    panes: HashMap<PaneId, Pane>,  // Mapa com PaneId
    next_pane_id: PaneId,
    active_pane: Option<PaneId>,
}

impl LayoutManager {
    pub fn create_root_pane(&mut self, tab_manager: TabManager) -> PaneId {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        
        let pane = Pane::new(id, tab_manager);
        self.panes.insert(id, pane);
        self.root = Some(SplitNode::Pane(id));
        self.active_pane = Some(id);
        id
    }
    
    pub fn split_pane(
        &mut self,
        pane_id: PaneId,  // Usa PaneId unificado
        direction: SplitDirection,
        new_tab_manager: TabManager,
    ) -> Option<PaneId> {
        // ...
    }
}
```

### 2. Pane System (`src/pane/unified.rs`)

```rust
use crate::types::PaneId;

/// Contador global para IDs de pane
static PANE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Gera um novo ID de pane único
fn generate_pane_id() -> PaneId {
    PANE_ID_COUNTER.fetch_add(1, Ordering::SeqCst)
}

#[derive(Debug)]
pub struct UnifiedPane {
    pub id: PaneId,  // ID unificado
    pub content_type: PaneContentType,
    pub buffer: TerminalBuffer,
    pub tab_manager: TabManager,
    // ...
}

impl UnifiedPane {
    pub fn new(cols: u16, rows: u16) -> PaneResult<Self> {
        let buffer = TerminalBuffer::new(cols, rows)?;
        let id = generate_pane_id();  // Gera ID único
        
        Ok(Self {
            id,
            content_type: PaneContentType::Shell,
            buffer,
            tab_manager: TabManager::new(),
            // ...
        })
    }
}

pub struct UnifiedPaneManager {
    panes: HashMap<PaneId, UnifiedPane>,  // Mapa com PaneId
    active_pane: Option<PaneId>,
}
```

### 3. Multiplexer (`src/multiplexer/pane.rs`)

```rust
use crate::types::PaneId;

pub struct Pane {
    pub id: PaneId,              // ID unificado
    pub content: PaneContent,
    pub state: PaneState,
    // ...
}

pub struct PaneManager {
    panes: HashMap<PaneId, Pane>,
    active_pane: Option<PaneId>,
}
```

### 4. Renderer (`src/renderer/pane.rs`)

```rust
use crate::types::PaneId;

pub struct PaneRenderState {
    pub pane_id: PaneId,  // ID unificado
    pub grid: Arc<RwLock<Grid>>,
    pub dirty: bool,
}

impl RenderThread {
    pub fn update_pane(&self, pane_id: PaneId, grid: Arc<RwLock<Grid>>) {
        self.command_tx.send(RenderCommand::UpdateGrid(pane_id, grid))
            .ok();
    }
}
```

---

## Exemplos de Código

### Criando e Gerenciando Panes

```rust
use code_terminal::types::PaneId;
use code_terminal::pane::{UnifiedPane, UnifiedPaneManager};
use code_terminal::ui::layout::{LayoutManager, SplitDirection};

fn exemplo_gerenciamento_panes() {
    // Cria manager de panes
    let mut pane_manager = UnifiedPaneManager::new();
    
    // Cria novo pane (ID gerado automaticamente)
    let pane_id: PaneId = pane_manager.create_pane(80, 24).unwrap();
    
    // Usa o ID para operações
    if let Some(pane) = pane_manager.get_pane_mut(pane_id) {
        pane.activate();
        pane.write(b"echo 'Hello World'\n").unwrap();
    }
    
    // Ativa outro pane
    pane_manager.set_active_pane(pane_id).unwrap();
}
```

### Navegação entre Panes

```rust
use code_terminal::ui::splits::{SplitManager, NavigationDirection};

fn exemplo_navegacao() {
    let mut split_manager = SplitManager::new(800, 600);
    
    // Cria panes
    let root = split_manager.create_root_pane(TabManager::new());
    let split = split_manager.split_vertical(TabManager::new()).unwrap();
    
    // Navegação com Alt+Arrow
    split_manager.navigate(NavigationDirection::Right);
    split_manager.navigate(NavigationDirection::Down);
    
    // Verifica pane ativo
    if let Some(active) = split_manager.active_pane() {
        println!("Pane ativo: {:?}", active.id);
    }
}
```

### Comunicação entre Módulos

```rust
use code_terminal::types::PaneId;
use std::sync::mpsc::{channel, Sender, Receiver};

enum AppEvent {
    PaneCreated(PaneId),
    PaneClosed(PaneId),
    PaneActivated(PaneId),
}

struct AppController {
    event_tx: Sender<AppEvent>,
}

impl AppController {
    fn on_pane_created(&self, pane_id: PaneId) {
        self.event_tx.send(AppEvent::PaneCreated(pane_id)).ok();
    }
}

// O mesmo PaneId é usado em todos os módulos
fn processar_evento(event: AppEvent) {
    match event {
        AppEvent::PaneCreated(id) => {
            // Notifica renderer
            renderer.update_pane(id);
            // Notifica multiplexer
            multiplexer.register_pane(id);
            // Atualiza UI
            ui.focus_pane(id);
        }
        // ...
    }
}
```

---

## Migração de Código Antigo

### Antes (Código Problemático)

```rust
// ui/layout.rs
pub type PaneId = u32;

// multiplexer/pane.rs  
pub type PaneId = usize;

// renderer/pane.rs
pub type PaneId = u64;

// Código que precisa converter:
fn conectar_ui_e_multiplexer(ui_id: ui::PaneId, mux_id: multiplexer::PaneId) {
    // Conversão perigosa!
    let id = ui_id as u64;
    // ...
}
```

### Depois (Código Unificado)

```rust
// Em todos os módulos:
pub use crate::types::PaneId;  // Todos usam o mesmo tipo

// Código limpo, sem conversões:
fn conectar_ui_e_multiplexer(pane_id: PaneId) {
    // Mesmo tipo em todos os módulos!
    ui.focus_pane(pane_id);
    multiplexer.register_pane(pane_id);
    renderer.update_pane(pane_id);
}
```

### Passos para Migração

1. **Substituir definições locais** por re-exportação:
   ```rust
   // Antes
   pub type PaneId = u32;
   
   // Depois
   pub use crate::types::PaneId;
   ```

2. **Remover casts desnecessários**:
   ```rust
   // Antes
   let id = pane_id as u64;
   
   // Depois
   let id = pane_id;  // Já é u64
   ```

3. **Atualizar assinaturas de função**:
   ```rust
   // Antes
   fn get_pane(&self, id: ui::PaneId) -> Option<&Pane>;
   
   // Depois
   fn get_pane(&self, id: PaneId) -> Option<&Pane>;
   ```

4. **Verificar comparações**:
   ```rust
   // Antes - podia comparar tipos diferentes!
   if ui_pane_id == mux_pane_id as u32 { }
   
   // Depois - sempre o mesmo tipo
   if pane_id == other_pane_id { }
   ```

---

## Referências Cruzadas

| Módulo | Arquivo | Uso Principal |
|--------|---------|---------------|
| types | `src/types.rs` | Definição dos tipos |
| ui/layout | `src/ui/layout.rs` | Pane em layout |
| ui/splits | `src/ui/splits.rs` | Navegação entre panes |
| pane/unified | `src/pane/unified.rs` | UnifiedPane |
| multiplexer/pane | `src/multiplexer/pane.rs` | Pane em sessão |
| renderer/pane | `src/renderer/pane.rs` | Renderização de pane |
| notification | `src/notification/` | Notificações por pane |

---

**Maintained by:** Development Team  
**Last Updated:** Março 2026
