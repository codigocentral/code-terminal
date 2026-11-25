# Sistema de Splits

**Versão:** 1.0.0  
**Última Atualização:** Março 2026  
**Status:** Implementado

---

## Índice

1. [Visão Geral](#visão-geral)
2. [Arquitetura](#arquitetura)
3. [SplitTree - Árvore Binária](#splittree---árvore-binária)
4. [SplitManager](#splitmanager)
5. [LayoutManager](#layoutmanager)
6. [Navegação](#navegação)
7. [Redimensionamento](#redimensionamento)
8. [Integração com TabManager](#integração-com-tabmanager)
9. [Exemplos de Uso](#exemplos-de-uso)

---

## Visão Geral

O sistema de splits permite dividir a tela do terminal em múltiplos panes, similar ao tmux ou vim. Cada pane contém seu próprio `TabManager`, permitindo múltiplas abas dentro de cada divisão.

### Features

- **Splits Horizontais** (`Ctrl+Shift+E`): Divide em cima/baixo
- **Splits Verticais** (`Ctrl+Shift+O`): Divide em esquerda/direita
- **Navegação** (`Alt+Arrow`): Navegação intuitiva entre panes
- **Redimensionamento**: Via teclado ou mouse
- **Layout Persistente**: Árvore binária mantém estrutura

---

## Arquitetura

```
┌─────────────────────────────────────────────────────────────────┐
│                     Window (800x600)                             │
│                                                                  │
│  ┌──────────────────────────┬──────────────────────────┐       │
│  │                          │                          │       │
│  │       Pane 1             │         Pane 2           │       │
│  │     (TabManager)         │      ┌──────────┐        │       │
│  │    ┌──────────┐          │      │  Pane 3  │        │       │
│  │    │ Tab 1    │          │      │(TabMgr)  │        │       │
│  │    ├──────────┤          │      ├──────────┤        │       │
│  │    │ Tab 2    │          │      │          │        │       │
│  │    └──────────┘          │      ├──────────┤        │       │
│  │                          │      │  Pane 4  │        │       │
│  │                          │      │(TabMgr)  │        │       │
│  │                          │      └──────────┘        │       │
│  │                          │                          │       │
│  └──────────────────────────┴──────────────────────────┘       │
│                                                                  │
│  SplitTree: V[P1, H[P2_idle, P3_active]]                         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Componentes Principais

| Componente | Arquivo | Descrição |
|------------|---------|-----------|
| `SplitNode` | `src/ui/layout.rs` | Nó da árvore (Split ou Pane) |
| `SplitTree` | `src/ui/splits.rs` | Interface compatível da árvore |
| `SplitManager` | `src/ui/splits.rs` | API pública de alto nível |
| `LayoutManager` | `src/ui/layout.rs` | Gerenciamento de layout |
| `Pane` | `src/ui/layout.rs` | Container com TabManager |

---

## SplitTree - Árvore Binária

### Estrutura de Dados

```rust
/// Nó da árvore de layout
#[derive(Debug, Clone)]
pub enum SplitNode {
    /// Split com dois filhos
    Split {
        direction: SplitDirection,
        /// Proporção do primeiro filho (0.0 - 1.0)
        ratio: f32,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
    /// Pane folha
    Pane(PaneId),
}

/// Direção do split
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    /// Panes empilhados verticalmente (divide altura)
    Horizontal,
    /// Panes lado a lado (divide largura)
    Vertical,
}
```

### Representação Visual

```
Split Vertical (ratio: 0.5)
┌─────────────────┬─────────────────┐
│                 │                 │
│    Pane 1       │    Pane 2       │
│   (first)       │   (second)      │
│                 │                 │
└─────────────────┴─────────────────┘
       50%              50%

Split Horizontal (ratio: 0.3)
┌───────────────────────────────────┐
│           Pane 1 (30%)            │
│           (first)                 │
├───────────────────────────────────┤
│           Pane 2 (70%)            │
│           (second)                │
└───────────────────────────────────┘
```

### Exemplo de Árvore Complexa

```rust
// Layout: P1 à esquerda, P2 e P3 empilhados à direita
//
// ┌──────────┬──────────┐
// │          │   Pane 2 │
// │  Pane 1  ├──────────┤
// │          │   Pane 3 │
// └──────────┴──────────┘

let tree = SplitNode::Split {
    direction: SplitDirection::Vertical,
    ratio: 0.5,
    first: Box::new(SplitNode::Pane(1)),  // Pane 1
    second: Box::new(SplitNode::Split {   // Split horizontal
        direction: SplitDirection::Horizontal,
        ratio: 0.5,
        first: Box::new(SplitNode::Pane(2)),   // Pane 2 (topo)
        second: Box::new(SplitNode::Pane(3)),  // Pane 3 (baixo)
    }),
};
```

---

## SplitManager

O `SplitManager` é a API de alto nível para gerenciamento de splits.

### Estrutura

```rust
pub struct SplitManager {
    layout: LayoutManager,
    /// Histórico de panes ativos (para navegação circular)
    pane_history: Vec<PaneId>,
    /// Configurações de teclas
    shortcuts: NativeSplitShortcuts,
    /// Callbacks
    on_pane_activated: Option<Box<dyn Fn(PaneId) + Send + Sync>>,
    on_pane_created: Option<Box<dyn Fn(PaneId) + Send + Sync>>,
    on_pane_closed: Option<Box<dyn Fn(PaneId) + Send + Sync>>,
}
```

### API Pública

```rust
impl SplitManager {
    /// Cria novo manager
    pub fn new(window_width: u32, window_height: u32) -> Self;
    
    /// Cria pane raiz inicial
    pub fn create_root_pane(&mut self, tab_manager: TabManager) -> PaneId;
    
    /// Cria split horizontal (empilha verticalmente)
    pub fn split_horizontal(&mut self, tab_manager: TabManager) -> Option<PaneId>;
    
    /// Cria split vertical (lado a lado)
    pub fn split_vertical(&mut self, tab_manager: TabManager) -> Option<PaneId>;
    
    /// Fecha pane atual
    pub fn close_current_pane(&mut self) -> bool;
    
    /// Fecha pane específico
    pub fn close_pane(&mut self, pane_id: PaneId) -> bool;
    
    /// Navega na direção especificada (Alt+Arrow)
    pub fn navigate(&mut self, direction: NavigationDirection);
    
    /// Ativa pane específico
    pub fn activate_pane(&mut self, pane_id: PaneId);
    
    /// Navegação circular
    pub fn next_pane(&mut self);
    pub fn previous_pane(&mut self);
    
    /// Redimensionamento
    pub fn increase_pane_size(&mut self);
    pub fn decrease_pane_size(&mut self);
    
    /// Processa actions do sistema de keybindings
    pub fn handle_action(&mut self, action: Action) -> bool;
    
    /// Processa input de teclado
    pub fn handle_key_input(&mut self, ctrl: bool, shift: bool, alt: bool, key: &str) -> bool;
}
```

---

## LayoutManager

O `LayoutManager` gerencia a árvore de splits e calcula posições.

### Estrutura

```rust
pub struct LayoutManager {
    /// Raiz da árvore de splits
    root: Option<SplitNode>,
    /// Mapa de panes por ID
    panes: HashMap<PaneId, Pane>,
    /// ID do próximo pane
    next_pane_id: PaneId,
    /// Pane ativo
    active_pane: Option<PaneId>,
    /// Retângulo da janela
    window_rect: Rect,
}
```

### Cálculo de Layout

```rust
impl LayoutManager {
    /// Recalcula posições de todos os panes
    pub fn recalculate_layout(&mut self) {
        let positions = self.calculate_all_positions();
        for (id, rect) in positions {
            if let Some(pane) = self.panes.get_mut(&id) {
                pane.rect = rect;
            }
        }
    }
    
    /// Calcula posições recursivamente
    fn collect_positions(
        &self,
        node: &SplitNode,
        rect: Rect,
        positions: &mut HashMap<PaneId, Rect>,
    ) {
        match node {
            SplitNode::Pane(id) => {
                positions.insert(*id, rect);
            }
            SplitNode::Split { direction, ratio, first, second } => {
                let (first_rect, second_rect) = match direction {
                    SplitDirection::Horizontal => {
                        // Divide na altura
                        let first_height = (rect.height as f32 * ratio) as u32;
                        let first = Rect::new(rect.x, rect.y, rect.width, first_height);
                        let second = Rect::new(
                            rect.x,
                            rect.y + first_height,
                            rect.width,
                            rect.height - first_height,
                        );
                        (first, second)
                    }
                    SplitDirection::Vertical => {
                        // Divide na largura
                        let first_width = (rect.width as f32 * ratio) as u32;
                        let first = Rect::new(rect.x, rect.y, first_width, rect.height);
                        let second = Rect::new(
                            rect.x + first_width,
                            rect.y,
                            rect.width - first_width,
                            rect.height,
                        );
                        (first, second)
                    }
                };
                
                self.collect_positions(first, first_rect, positions);
                self.collect_positions(second, second_rect, positions);
            }
        }
    }
}
```

---

## Navegação

### Direções de Navegação

```rust
pub enum NavigationDirection {
    Up,     // Alt+Up
    Down,   // Alt+Down
    Left,   // Alt+Left
    Right,  // Alt+Right
}
```

### Algoritmo de Navegação

```rust
impl SplitManager {
    /// Navega para o pane na direção especificada
    pub fn navigate(&mut self, direction: NavigationDirection) {
        let split_direction = match direction {
            NavigationDirection::Up | NavigationDirection::Down => {
                SplitDirection::Horizontal
            }
            NavigationDirection::Left | NavigationDirection::Right => {
                SplitDirection::Vertical
            }
        };
        
        if let Some(new_pane_id) = self.layout.find_pane_in_direction(split_direction) {
            // Verifica se a direção faz sentido
            let current = self.layout.active_pane().expect("Should have active pane");
            let new_pane = self.layout.panes().get(&new_pane_id).expect("Pane should exist");
            
            let should_navigate = match direction {
                NavigationDirection::Up => new_pane.rect.y < current.rect.y,
                NavigationDirection::Down => new_pane.rect.y > current.rect.y,
                NavigationDirection::Left => new_pane.rect.x < current.rect.x,
                NavigationDirection::Right => new_pane.rect.x > current.rect.x,
            };
            
            if should_navigate {
                self.activate_pane(new_pane_id);
            }
        }
    }
}
```

### Encontrando Pane na Direção

```rust
impl LayoutManager {
    /// Encontra o pane em uma direção a partir do pane atual
    pub fn find_pane_in_direction(&self, direction: SplitDirection) -> Option<PaneId> {
        let current = self.active_pane?;
        let current_pane = self.panes.get(&current)?;
        let current_center = current_pane.rect.center();
        
        let mut best_candidate: Option<(PaneId, f32)> = None;
        
        for (id, pane) in &self.panes {
            if *id == current {
                continue;
            }
            
            let center = pane.rect.center();
            let is_in_direction = match direction {
                SplitDirection::Horizontal => {
                    // Acima ou abaixo
                    let dy = center.1 as f32 - current_center.1 as f32;
                    let dx = (center.0 as f32 - current_center.0 as f32).abs();
                    dy != 0.0 && dx < dy.abs() // Mais vertical que horizontal
                }
                SplitDirection::Vertical => {
                    // Esquerda ou direita
                    let dx = center.0 as f32 - current_center.0 as f32;
                    let dy = (center.1 as f32 - current_center.1 as f32).abs();
                    dx != 0.0 && dy < dx.abs() // Mais horizontal que vertical
                }
            };
            
            if is_in_direction {
                let distance = ((center.0 as f32 - current_center.0 as f32).powi(2)
                    + (center.1 as f32 - current_center.1 as f32).powi(2))
                    .sqrt();
                
                match best_candidate {
                    None => best_candidate = Some((*id, distance)),
                    Some((_, d)) if distance < d => best_candidate = Some((*id, distance)),
                    _ => {}
                }
            }
        }
        
        best_candidate.map(|(id, _)| id)
    }
}
```

---

## Redimensionamento

### Via Teclado

```rust
impl SplitManager {
    /// Aumenta o tamanho do pane ativo
    pub fn increase_pane_size(&mut self) {
        self.layout.resize_active_pane(ResizeDirection::Increase);
    }
    
    /// Diminui o tamanho do pane ativo
    pub fn decrease_pane_size(&mut self) {
        self.layout.resize_active_pane(ResizeDirection::Decrease);
    }
}
```

### Algoritmo de Redimensionamento

```rust
impl LayoutManager {
    pub fn resize_active_pane(&mut self, direction: ResizeDirection) {
        const RESIZE_STEP: f32 = 0.05;  // 5% por passo
        const MIN_RATIO: f32 = 0.10;     // Mínimo 10%
        const MAX_RATIO: f32 = 0.90;     // Máximo 90%
        
        let Some(active_id) = self.active_pane else { return };
        
        let delta = match direction {
            ResizeDirection::Increase => RESIZE_STEP,
            ResizeDirection::Decrease => -RESIZE_STEP,
        };
        
        // Ajusta o ratio no split que contém o pane ativo
        if let Some(root) = self.root.as_ref() {
            if let Some(new_root) = self.adjust_split_ratio(
                root, active_id, delta, MIN_RATIO, MAX_RATIO
            ) {
                self.root = Some(new_root);
                self.recalculate_layout();
            }
        }
    }
    
    fn adjust_split_ratio(
        &self,
        node: &SplitNode,
        target_pane: PaneId,
        delta: f32,
        min: f32,
        max: f32,
    ) -> Option<SplitNode> {
        match node {
            SplitNode::Pane(_) => None,
            SplitNode::Split { direction, ratio, first, second } => {
                let target_in_first = self.node_contains_pane(first, target_pane);
                let target_in_second = self.node_contains_pane(second, target_pane);
                
                if target_in_first || target_in_second {
                    let new_ratio = if target_in_first {
                        (ratio + delta).clamp(min, max)
                    } else {
                        (ratio - delta).clamp(min, max)
                    };
                    
                    if (new_ratio - ratio).abs() > f32::EPSILON {
                        Some(SplitNode::Split {
                            direction: *direction,
                            ratio: new_ratio,
                            first: first.clone(),
                            second: second.clone(),
                        })
                    } else {
                        None // Ratio já está no limite
                    }
                } else {
                    // Procura nos filhos recursivamente
                    // ...
                }
            }
        }
    }
}
```

### Via Mouse

O redimensionamento via mouse é detectado quando o cursor está próximo à borda entre dois panes:

```rust
pub fn handle_mouse_resize(&mut self, x: u32, y: u32) -> Option<ResizeHandle> {
    // Verifica se o mouse está próximo de uma borda de split
    for (pane_id, pane) in &self.panes {
        // Verifica bordas direita e inferior
        let near_right = (x as i32 - (pane.rect.x + pane.rect.width) as i32).abs() < 5;
        let near_bottom = (y as i32 - (pane.rect.y + pane.rect.height) as i32).abs() < 5;
        
        if near_right {
            return Some(ResizeHandle::Vertical(pane_id));
        }
        if near_bottom {
            return Some(ResizeHandle::Horizontal(pane_id));
        }
    }
    None
}
```

---

## Integração com TabManager

Cada pane tem seu próprio `TabManager`, permitindo múltiplas abas dentro de cada divisão.

### Estrutura

```rust
pub struct Pane {
    pub id: PaneId,
    pub tab_manager: TabManager,  // Cada pane tem seu próprio TabManager
    pub rect: Rect,
}

pub struct TabManager {
    tabs: Vec<Tab>,
    active_tab_index: usize,
    search_bar: SearchBar,
}
```

### Criando Split com Tabs

```rust
// Cria split horizontal - cada novo pane tem seu próprio TabManager
let new_pane_id = split_manager.split_horizontal(TabManager::new());

// O pane original mantém seu TabManager
// O novo pane recebe um TabManager novo e vazio
```

### Hierarquia Completa

```
Window
├── LayoutManager
│   └── SplitTree
│       ├── Pane 1
│       │   ├── TabManager
│       │   │   ├── Tab 1 → TerminalBuffer
│       │   │   └── Tab 2 → TerminalBuffer
│       │   └── Rect (posição/tamanho)
│       └── Split (Vertical)
│           ├── Pane 2
│           │   ├── TabManager
│           │   │   └── Tab 1 → TerminalBuffer
│           │   └── Rect
│           └── Pane 3
│               ├── TabManager
│               │   └── Tab 1 → TerminalBuffer
│               └── Rect
```

---

## Exemplos de Uso

### Exemplo 1: Criar Layout Básico

```rust
use code_terminal::ui::splits::{SplitManager, NavigationDirection};
use code_terminal::ui::tabs::TabManager;

fn criar_layout() {
    let mut manager = SplitManager::new(800, 600);
    
    // Cria pane raiz
    let root_id = manager.create_root_pane(TabManager::new());
    
    // Divide verticalmente (lado a lado)
    let right_pane = manager.split_vertical(TabManager::new()).unwrap();
    
    // Divide o pane direito horizontalmente
    manager.activate_pane(right_pane);
    let bottom_right = manager.split_horizontal(TabManager::new()).unwrap();
    
    // Layout resultante:
    // ┌──────────┬──────────┐
    // │          │   Top    │
    // │   Root   ├──────────┤
    // │          │  Bottom  │
    // └──────────┴──────────┘
}
```

### Exemplo 2: Navegação Programática

```rust
fn navegar_entre_panes(manager: &mut SplitManager) {
    // Navega para direita
    manager.navigate(NavigationDirection::Right);
    
    // Navega para baixo
    manager.navigate(NavigationDirection::Down);
    
    // Navegação circular
    manager.next_pane();
    manager.previous_pane();
    
    // Ativa pane específico
    manager.activate_pane(pane_id);
}
```

### Exemplo 3: Redimensionamento

```rust
fn redimensionar_panes(manager: &mut SplitManager) {
    // Aumenta pane ativo
    manager.increase_pane_size();
    manager.increase_pane_size();
    
    // Diminui pane ativo
    manager.decrease_pane_size();
}
```

### Exemplo 4: Fechar Panes

```rust
fn fechar_panes(manager: &mut SplitManager) {
    // Fecha pane ativo
    let can_close = manager.close_current_pane();
    
    // Fecha pane específico
    manager.close_pane(pane_id);
    
    // Nota: não é possível fechar o último pane
    if manager.pane_count() == 1 {
        println!("Não pode fechar o último pane!");
    }
}
```

### Exemplo 5: Callbacks

```rust
fn configurar_callbacks(manager: &mut SplitManager) {
    manager.on_pane_created(|pane_id| {
        println!("Pane criado: {:?}", pane_id);
    });
    
    manager.on_pane_activated(|pane_id| {
        println!("Pane ativado: {:?}", pane_id);
    });
    
    manager.on_pane_closed(|pane_id| {
        println!("Pane fechado: {:?}", pane_id);
    });
}
```

### Exemplo 6: Renderização

```rust
use code_terminal::ui::splits::generate_split_render_info;

fn renderizar_splits(manager: &SplitManager) {
    let active_color = [1.0, 0.0, 0.0, 1.0];    // Vermelho
    let inactive_color = [0.5, 0.5, 0.5, 1.0];  // Cinza
    
    let render_info = generate_split_render_info(
        manager,
        active_color,
        inactive_color,
        2,  // border_thickness
    );
    
    for info in render_info {
        render_pane_border(
            info.rect,
            info.border_color,
            info.border_thickness,
            info.is_active,
        );
    }
}
```

---

## Referências

- [Arquitetura Geral](ARCHITECTURE.md)
- [PaneId Unificado](PANE_ID_ARCHITECTURE.md)
- [Window Manager](WINDOW_MANAGER.md)

---

**Maintained by:** Development Team  
**Last Updated:** Março 2026
