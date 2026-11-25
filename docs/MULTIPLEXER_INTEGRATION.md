# Integração do Multiplexer - Documentação

**Documento:** Integração Multiplexer + Pane Unificado  
**Status:** ✅ Implementado  
**Última Atualização:** Março 2026

---

## 📋 Visão Geral

Esta documentação descreve a integração entre o sistema de multiplexador (tipo tmux) e o sistema unificado de panes, permitindo:

- **Session attach/detach** funcional com rendering real
- **Unificação** de `multiplexer::Pane`, `ui::Pane` e renderização
- **Input routing** inteligente para o pane correto
- **Persistência** de estado entre sessões

---

## 🏗️ Arquitetura

### Camadas Integradas

```
┌─────────────────────────────────────────────────────────────────┐
│                    Application Layer                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
│  │   Session   │  │   Client    │  │      AttachManager      │ │
│  │   Manager   │  │   Handler   │  │   (attach/detach)       │ │
│  └──────┬──────┘  └──────┬──────┘  └───────────┬─────────────┘ │
│         │                │                      │               │
│         └────────────────┼──────────────────────┘               │
│                          │                                      │
├──────────────────────────┼──────────────────────────────────────┤
│                    Integration Layer                             │
│  ┌───────────────────────────────────────────────────────────┐ │
│  │            SessionPaneIntegration                          │ │
│  │  - Mapeamento multiplexer <-> unified                      │ │
│  │  - Sincronização de estado                                 │ │
│  │  - Persistência de sessão                                  │ │
│  └───────────────────────────────────────────────────────────┘ │
├──────────────────────────┼──────────────────────────────────────┤
│                    Unified Pane Layer                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
│  │ UnifiedPane │  │ InputRouter │  │   UnifiedPaneManager    │ │
│  │  (dados)    │  │  (routing)  │  │      (gestão)           │ │
│  └──────┬──────┘  └──────┬──────┘  └───────────┬─────────────┘ │
│         │                │                      │               │
│         └────────────────┼──────────────────────┘               │
│                          │                                      │
├──────────────────────────┼──────────────────────────────────────┤
│                     Trait Layer                                  │
│  ┌───────────────────────────────────────────────────────────┐ │
│  │                    PaneContent Trait                       │ │
│  │  - Interface comum para todo conteúdo de pane              │ │
│  │  - Implementado por: TerminalBuffer, UnifiedPane, etc      │ │
│  └───────────────────────────────────────────────────────────┘ │
├──────────────────────────┼──────────────────────────────────────┤
│                  Legacy Components                               │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
│  │Multiplexer  │  │   UI Pane   │  │    PaneRenderer         │ │
│  │   Pane      │  │   (layout)  │  │    (render)             │ │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

---

## 📦 Módulos

### 1. Trait PaneContent (`src/pane/content.rs`)

Interface comum para todo conteúdo de pane.

```rust
pub trait PaneContent: Send + Sync {
    fn pane_id(&self) -> PaneId;
    fn content_type(&self) -> PaneContentType;
    fn state(&self) -> PaneState;
    fn set_state(&mut self, state: PaneState);
    fn grid(&self) -> Option<&Grid>;
    fn grid_mut(&mut self) -> Option<&mut Grid>;
    fn process_input(&mut self, input: &InputEvent) -> PaneResult<bool>;
    fn update(&mut self) -> PaneResult<()>;
    // ... mais métodos
}
```

**Implementações:**
- `TerminalBuffer` - Terminal padrão
- `TerminalPane` - Wrapper com metadados
- `UnifiedPane` - Pane completo integrado

---

### 2. UnifiedPane (`src/pane/unified.rs`)

Estrutura que unifica todas as representações de pane.

```rust
pub struct UnifiedPane {
    pub id: PaneId,
    pub content_type: PaneContentType,
    pub state: PaneState,
    pub buffer: TerminalBuffer,
    pub tab_manager: TabManager,
    pub rect: Rect,
    pub cell_size: (u16, u16),
    pub metadata: PaneMetadata,
    pub render_flags: RenderFlags,
    pub is_dirty: bool,
}
```

**Features:**
- Integra `TerminalBuffer` (conteúdo)
- Integra `TabManager` (múltiplas tabs)
- Integra `Rect` (layout/posição)
- Implementa `PaneContent`

---

### 3. InputRouter (`src/pane/input.rs`)

Roteamento de input para o pane correto.

```rust
pub struct InputRouter {
    panes: HashMap<PaneId, Box<dyn PaneContent>>,
    focused_pane: Option<PaneId>,
    broadcast_mode: bool,
    auto_focus: bool,
}
```

**Features:**
- Foco por clique (auto_focus)
- Modo broadcast (input para todos)
- Seleção com mouse
- Scroll em hover

**Uso:**
```rust
let mut router = InputRouter::new();

// Registra pane
let id = router.register_pane(Box::new(pane));

// Processa input
let consumed = router.process_input(event)?;

// Navegação
router.focus_next();
router.focus_previous();
```

---

### 4. Session Integration (`src/pane/session_integration.rs`)

Integração com sistema de sessões do multiplexer.

```rust
pub struct SessionPaneIntegration {
    pane_manager: UnifiedPaneManager,
    session_panes: HashMap<String, Vec<PaneId>>,
    pane_id_mapping: HashMap<MpPaneId, PaneId>,
    renderer: Option<PaneRenderer>,
}
```

**Features:**
- Mapeamento de IDs (multiplexer ↔ unified)
- Sincronização bidirecional de estado
- Attach/detach de sessões
- Persistência para background

---

## 🔄 Fluxo de Attach/Detach

### Attach (Cliente conecta)

```
Cliente → AttachManager::attach()
              ↓
    SessionPaneIntegration::attach_session()
              ↓
    Para cada pane na sessão:
        - Cria UnifiedPane
        - Restaura estado
        - Registra mapeamento
              ↓
    Retorna pane_ids + session_data
```

### Detach (Cliente desconecta)

```
Cliente → AttachManager::detach()
              ↓
    SessionPaneIntegration::prepare_for_background()
              ↓
    Serializa estado de todos os panes
              ↓
    SessionPaneIntegration::detach_session()
              ↓
    Marca panes como Background
              ↓
    Retorna session_data para persistência
```

### Reattach (Cliente reconecta)

```
Cliente → AttachManager::attach() [sessão existente]
              ↓
    SessionPaneIntegration::restore_from_background()
              ↓
    Para cada pane nos dados:
        - Recria UnifiedPane
        - Restaura metadados
        - Restaura posição/tamanho
              ↓
    Retorna pane_ids restaurados
```

---

## 🎯 Uso Prático

### Criar e Gerenciar Panes

```rust
use code_terminal::pane::*;
use code_terminal::pane::unified::UnifiedPaneManager;

// Cria gerenciador
let mut manager = UnifiedPaneManager::new();

// Cria pane
let id = manager.create_pane(80, 24)?;

// Acessa pane
if let Some(pane) = manager.get_pane_mut(id) {
    pane.metadata.title = "My Terminal".to_string();
    pane.activate();
}

// Foca outro pane
manager.set_active_pane(other_id)?;
```

### Input Routing

```rust
use code_terminal::pane::input::{InputRouter, InputEvent};

let mut router = InputRouter::new();

// Registra panes
for pane in panes {
    router.register_pane(Box::new(pane));
}

// Processa eventos
match event {
    WindowEvent::KeyboardInput { input, .. } => {
        let event = InputEvent::Key { 
            data: key_data,
            key_code: Some(key),
            modifiers: mods,
        };
        router.process_input(event)?;
    }
    WindowEvent::MouseInput { state, button, .. } => {
        let event = InputEvent::Mouse { 
            kind: mouse_kind,
            position: (col, row),
            modifiers: mods,
        };
        router.process_input(event)?;
    }
    _ => {}
}
```

### Integração com Multiplexer

```rust
use code_terminal::pane::session_integration::{
    AttachManager, ClientAttachContext
};

let mut attach_manager = AttachManager::new();

// Cliente conecta
let context = ClientAttachContext {
    client_id: "client-1".to_string(),
    session_id: "session-1".to_string(),
    terminal_size: (80, 24),
    features: vec!["256color".to_string()],
};

let result = attach_manager.attach(context, &session)?;

// Usa os panes
for pane_id in result.pane_ids {
    // Renderiza, processa input, etc
}

// Cliente desconecta
let session_data = attach_manager.detach("session-1")?;
// Salva session_data para persistência
```

---

## 📊 Comparação: Antes vs Depois

### Antes (Estruturas Separadas)

```rust
// Multiplexer Pane
let mp_pane = multiplexer::Pane::new((80, 24), (0, 0));

// UI Pane  
let ui_pane = ui::layout::Pane::new(id, tab_manager);

// Renderer
let mut renderer = PaneRenderer::new(...);
renderer.render_pane(&ui_pane, true, ...)?;

// Sincronização manual entre estruturas
// Código duplicado
// Inconsistências possíveis
```

### Depois (UnifiedPane)

```rust
// Pane unificado
let mut pane = UnifiedPane::new(80, 24)?;
pane.metadata.title = "Terminal".to_string();
pane.move_to(100, 100);
pane.set_size(800, 600);

// Implementa PaneContent
trait PaneContent {
    fn process_input(&mut self, event: &InputEvent) -> PaneResult<bool>;
    fn render_info(&self) -> RenderInfo;
    fn grid(&self) -> Option<&Grid>;
    // ...
}

// Gerenciamento unificado
let mut manager = UnifiedPaneManager::new();
let id = manager.create_pane(80, 24)?;
manager.set_active_pane(id)?;

// Input routing automático
let mut router = InputRouter::new();
router.register_pane(Box::new(pane));
router.process_input(event)?;
```

---

## 🧪 Testes

### Testes Unitários

```bash
cargo test pane::
```

### Testes de Integração

```bash
cargo test --test pane_integration_test
```

### Principais Cenários Testados

- ✅ Criação e gestão de panes
- ✅ Foco e navegação cíclica
- ✅ Input routing (teclado/mouse)
- ✅ Broadcast mode
- ✅ Session attach/detach
- ✅ Sincronização de estado

---

## 🔧 Integração com Renderer

O `UnifiedPane` fornece todas as informações necessárias para renderização:

```rust
impl UnifiedPane {
    pub fn render_info(&self) -> RenderInfo {
        RenderInfo {
            position: (self.rect.x, self.rect.y),
            size: (self.rect.width, self.rect.height),
            is_active: self.state == PaneState::Active,
            is_dirty: self.is_dirty,
        }
    }
    
    pub fn grid(&self) -> Option<&Grid> {
        Some(self.buffer.grid())
    }
}
```

O renderer pode usar:
- `render_info()` - Posição, tamanho, estado
- `grid()` - Conteúdo do terminal
- `is_dirty` - Se precisa re-renderizar

---

## 🚀 Próximos Passos

1. **Serialização completa**: Salvar/restaurar conteúdo do terminal
2. **Scrollback persistence**: Persistir histórico de scrollback
3. **Plugin integration**: Permitir panes de plugins
4. **Remote pane**: Panes em servidores remotos via SSH

---

## ✅ Checklist de Validação

- [x] `PaneContent` trait definido e implementado
- [x] `UnifiedPane` unifica as 3 representações
- [x] `InputRouter` roteia input corretamente
- [x] `SessionPaneIntegration` conecta multiplexer
- [x] Attach funciona com rendering real
- [x] Detach preserva estado
- [x] Reattach restaura estado
- [x] Testes de integração passando

---

**Maintained by:** Codeterm Team  
**Last Updated:** Março 2026
