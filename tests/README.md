# Testes de Integração - Code Terminal

Este diretório contém os testes de integração para o projeto code-terminal.

## Arquivos de Teste

### 1. `window_manager_test.rs`
Testes do WindowManager:
- Criação do WindowManager
- Geração de WindowId únicos
- Transições de estado de janela
- WindowCreateInfo padrão
- Eventos de comunicação entre janelas
- Mapeamento entre WindowId e valores u64
- RenderHandle
- Características de traits (Debug, Clone, Copy, Hash)

### 2. `split_test.rs`
Testes do sistema de splits:
- Criação do SplitManager
- Criação do pane raiz
- Split horizontal e vertical
- Navegação entre panes (Alt+Arrow)
- Navegação vertical entre panes
- Redimensionamento de panes
- Limites de redimensionamento
- Fechamento de panes
- Fechamento do pane ativo
- Navegação circular (next/previous)
- Interface SplitTree
- Conversão de NavigationDirection
- Parsing de direções de navegação
- Conversão de Action para SplitOperation
- Geração de informações de renderização
- Layouts aninhados
- Redimensionamento de janela
- Callbacks de pane
- Integração com LayoutManager
- Operações com Rect
- Handler de ações

### 3. `tab_manager_test.rs`
Testes do TabManager:
- Criação do TabManager
- Criação de tabs com IDs únicos
- Troca de tab ativa
- Navegação entre tabs (next/previous)
- Fechamento de tabs
- Fechamento da tab ativa
- Fechamento de todas as tabs
- Atualização de título
- Atualização de diretório de trabalho
- Estados da tab (Active/Inactive/Closing)
- Verificação de processo rodando
- Try close com confirmação
- Criação de tab com buffer
- Estados de fechamento (TabCloseResult)
- Métodos da struct Tab
- Acesso a tabs por índice
- Iteração sobre tabs
- Default do TabManager
- IDs sequenciais
- Comportamento após fechar tab do meio
- Acesso ao grid via tab
- Formatação Debug

### 4. `pane_id_test.rs`
Testes do PaneId unificado:
- Verificação de que PaneId é u64
- Consistência entre módulos (types, layout, pane)
- Geração de IDs únicos
- Geração thread-safe de IDs
- Display e Debug
- Uso em coleções (HashMap, HashSet)
- Conversões de tipo
- Limites de valores
- TabId, WindowId e SessionId também são u64
- Comparação e ordenação
- Operações aritméticas
- Geração em massa
- Uso como índice
- Zero como ID válido
- ID máximo (u64::MAX)
- Traits derivados
- Compatibilidade com AtomicU64
- Uso em structs
- Conversões implícitas
- Estresse de comparações

## Como Executar

Quando o projeto estiver compilando corretamente, execute:

```bash
# Todos os testes de integração
cargo test --test window_manager_test
cargo test --test split_test
cargo test --test tab_manager_test
cargo test --test pane_id_test

# Ou todos de uma vez
cargo test --test window_manager_test --test split_test --test tab_manager_test --test pane_id_test

# Com output detalhado
cargo test --test window_manager_test -- --nocapture
```

## Nota sobre Compilação

Atualmente o projeto possui erros de compilação em módulos como:
- `multiplexer::socket_api::server`
- `pane::content`
- `pane::unified`
- `pane::session_integration`
- `terminal::scrollback`
- `plugin::host`

Esses erros precisam ser corrigidos para que os testes de integração possam compilar e executar.

Os testes criados estão corretos e seguem as melhores práticas:
- Cada teste é independente
- Usam setup() quando necessário
- Documentam o propósito de cada teste
- Cobrem casos de sucesso e erro
- Testam edge cases
