# Changelog - Code Terminal

Todas as mudanças notáveis deste projeto serão documentadas neste arquivo.

O formato é baseado em [Keep a Changelog](https://keepachangelog.com/pt-BR/1.0.0/),
e este projeto adere ao [Semantic Versioning](https://semver.org/lang/pt-BR/).

---

## [Unreleased]

### Added
- Logo do Code Terminal e créditos de autoria e publicação em Configurações → Sobre.
- Empacotamento para Mac App Store e Microsoft Store com o logo do app.
- DMG notarizado para macOS e jobs de assinatura no runner macOS protegido.
- Abas persistentes depois de reiniciar a máquina (#21, fatia 2): o daemon
  grava cada aba persistente em `~/.local/state/code-terminal/tabs/` (pasta
  `0700`, arquivos `0600`) quando ela muda (no máximo a cada 30 s) e no
  desligamento. A primeira janela depois do reboot reabre as abas no mesmo
  lugar, com título, grupo, diretório e histórico, e deixa digitado no prompt,
  sem Enter, o comando para retomar: `claude --resume <id>` (o id vem dos
  hooks, agora com `SessionStart`/`SessionEnd`), o `ssh` aberto ou o último
  programa. "Retomar automaticamente" no menu da aba manda com Enter.
  `session.save_scrollback = false` desliga e apaga.
- Hooks de agente numa aba persistente acham o painel pela sessão do mux,
  mesmo depois de a janela ser reaberta.

### Changed
- O daemon do multiplexer não acorda mais a cada 8 ms sem sessões de grid:
  parado, com abas persistentes abertas, acordou 2 vezes em 30 s.
- Abas persistentes por escolha (#21): botão direito › "Tornar persistente"
  (ou a paleta) passa o shell da aba, sem reiniciá-lo, para o daemon do
  multiplexer, e o cartão ganha um alfinete. Fechar a janela ou reiniciar o
  `code-terminal-server` só desanexa; a próxima janela traz as abas de volta
  no mesmo lugar e grupo, com o processo vivo e o histórico. Fechar a aba de
  propósito pergunta "Encerrar" ou "Só esconder" (volta pela paleta, em
  "Sessões em segundo plano"). `session.persistent_tabs = true` deixa todas
  as abas novas persistentes. Serviço de usuário próprio:
  `code-terminal-mux.service`. Detalhes e custo: `docs/PERSISTENT_TABS.md`.
- Copiar ao selecionar: soltar o mouse depois de arrastar, ou dar duplo ou
  triplo clique, copia a seleção (área de transferência e, no Linux, PRIMARY).
  Desliga com `copy_on_select = false` em `[terminal]`.
- Aviso rápido no canto inferior direito: "Copiado · N caracteres", "Nada de
  texto para colar" e "Não foi possível colar" (antes a falha só ia ao log).
- Shift+Insert cola e o clique do meio cola a seleção PRIMARY (Linux).
- Sidebar mostra a memória de cada tab (`38 MB`): soma do PSS do shell e dos
  processos filhos, medida em segundo plano a cada ~3 s. `sidebar.show_memory`
  desliga.
- Aviso novo anima: o anel do painel, a faixa e o contador da tab passeiam
  pelas cores de destaque do tema por 8 s e param. `window.animations = false`
  deixa tudo parado.
- 10 temas novos: catppuccin-mocha, catppuccin-latte, tokyo-night,
  gruvbox-dark, gruvbox-light, rose-pine, rose-pine-dawn, kanagawa,
  everforest-dark e ayu-dark (créditos em `THIRD_PARTY_NOTICES.md`).
- Renomear tab na sidebar: duplo clique no cartão, F2 (com a sidebar à vista;
  `keybindings.rename_tab`), "Renomear" no menu do botão direito ou "Rename
  Tab" na paleta. Edita no próprio cartão: Enter salva, Esc cancela, clicar
  fora salva, vazio volta ao título automático. O título dado fica fixo (o
  programa não troca) e volta com a sessão. Também `cli rename-tab` e
  `tab.rename` na Socket API.
- Grupos de tabs na sidebar (um nível): arrastar uma tab sobre outra a põe
  dentro; arrastar para o espaço entre tabs tira do grupo. Menu do botão
  direito com "Aninhar em ›", "Tirar do grupo", "Recolher/Expandir grupo" e
  "Fechar aba". Seta recolhe/abre o grupo (recolhido, o cartão de cima soma
  os avisos). Fechar a tab de cima não fecha as de dentro: elas sobem um
  nível. Ctrl+Tab e Ctrl+N seguem a ordem da sidebar. Os grupos voltam com a
  sessão. Socket API: `tab.nest` e `tab.unnest`.

### Changed
- Licença: de MIT ou Apache-2.0 para GPL-3.0-or-later. Plugins WASM que usam
  só a Plugin API pública podem ter qualquer licença
  (`LICENSE-PLUGIN-EXCEPTION.md`), e contribuições pedem o CLA (`CLA.md`).

### Fixed
- No macOS, `⌘V` cola texto da área de transferência no terminal e no título
  da aba em edição. `⌘C` copia a seleção do terminal.
- No Windows, `Ctrl+C` copia quando há uma seleção de texto e `Ctrl+V` cola
  na tela principal. Na tela alternativa, `Ctrl+V` chega ao programa e
  `Ctrl+Shift+V` cola da área de transferência.
- O menu de contexto da aba cobre apenas a parte do título que fica sob ele.
- Renderização: cores ANSI/256/RGB, negrito, itálico, sublinhado, fundos, seleção e
  destaque de busca agora aparecem (antes o grid saía numa cor só).
- Cores desbotadas: as cores do tema eram enviadas a uma superfície sRGB sem
  conversão para linear (o fundo `#1e1e1e` virava cinza claro).
- Painéis (boas-vindas, paleta, configurações) desenhavam por cima do próprio
  texto; agora há camadas e o painel é opaco.
- CPU em 100% parado: a janela redesenhava sem parar. Agora só redesenha
  quando chega saída do PTY ou um evento de entrada.
- Setas, Home/End, PageUp/PageDown, Delete, Insert e F1–F12 não mandavam nada
  ao programa; Alt e Ctrl+letra seguem o padrão do xterm (inclui modo DECCKM).
- Colagem respeita o bracketed paste.
- Colar no Wayland: a área de transferência agora usa o protocolo nativo
  (`wlr-data-control`, feature `wayland-data-control` do arboard), com o X11
  como reserva; antes ela passava só pela ponte do XWayland.
- Colunas/linhas calculadas com a fonte medida (antes uma célula fixa de
  10x20 px deixava boa parte da janela vazia e o mouse selecionava errado).
- O tamanho da fonte é em pontos (12pt = 16px em 96 dpi), usando a fonte
  configurada quando instalada (ex.: "JetBrains Mono" acha "JetBrainsMono Nerd Font").
- Dois shells eram iniciados na abertura (um era morto em seguida, custando 0,5 s).
- Fechar tab travava 0,5 s: o bash interativo ignora SIGTERM; agora recebe SIGHUP.
- `exit`/Ctrl+D no shell fecha a tab (e a janela, se for a última).
- Eventos de uma janela iam para a janela ativa; janelas novas abriam sem GPU.
- Arquivo de configuração era gravado como `config.toml.toml` (migrado sozinho).
- O aviso de boas-vindas e a paleta de comandos mostravam atalhos errados
  (Ctrl+T, Ctrl+W, Ctrl+F); agora batem com os atalhos configurados.
- Rodar os testes sobrescrevia a `~/.config/code-terminal/config.toml` de quem
  os rodava (o teste de configurações gravava o tema "light") e deixava um
  backup de sessão SSH de teste em `~/.config/codeterm/`. Os testes agora usam
  diretórios temporários; `CODE_TERMINAL_CONFIG_DIR` troca o diretório de
  configuração.

### Changed
- Binário do app de 30 MB para 24 MB: o wasmtime entra só com compilador,
  runtime e o coletor mínimo de GC (sem cache, component-model, async,
  threads) e o `image` só com PNG, JPEG, GIF, BMP, TIFF e WebP (saíram o
  codificador AV1 rav1e, EXR, DDS e afins). Plugins `.wasm` seguem
  funcionando; texto WAT direto no lugar do `.wasm` deixou de ser aceito.
- Atalhos que roubavam teclas do shell passaram a usar Shift: copiar
  `Ctrl+Shift+C`, colar `Ctrl+Shift+V`, buscar `Ctrl+Shift+F`, fechar tab
  `Ctrl+Shift+W` (fechar pane foi para `Ctrl+Shift+X`). Configurações com os
  valores antigos são atualizadas ao carregar. Copiar copia a seleção, não a tela.
- Shift+PageUp/PageDown rolam o histórico.
- A Socket API no Linux/macOS escuta só num Unix socket `0600` em
  `$XDG_RUNTIME_DIR/code-terminal.sock` (o caminho padrão do CLI), sem porta TCP.
  Uma segunda instância não rouba o socket da primeira.
- GPU: tenta Vulkan/Metal/DX12 antes de GL e prefere a placa de baixo consumo.
  `CODE_TERMINAL_GPU_BACKEND=gl|vulkan|metal|dx12` força um backend.
- Abertura: as fontes do sistema carregam em paralelo com a janela e a GPU
  (primeiro frame de ~1,7 s para ~150 ms com cache quente).

### Added
- **Modo servidor** (`code-terminal --server`): um processo fica de pé com a
  GPU e as fontes carregadas; `code-terminal` pede a janela a ele e sai. Janela
  nova em ~40 ms (antes ~150–210 ms) e ~4,5 MB por janela a mais, em vez de um
  processo de ~73 MB cada. Sem servidor, abre do jeito normal. Serviço de
  usuário do systemd no `.deb`. Ver [SERVER_MODE.md](SERVER_MODE.md).
- A segunda janela de um processo reaproveita o device da GPU e o banco de
  fontes da primeira.
- **Plugins WASM de verdade no app**: plugins em `<config>/plugins/<nome>/`
  são carregados ao abrir, recebem `update` a cada 250 ms e a digitação, e as
  funções do host agem no painel em foco (escrever, ler a tela, cursor,
  seleção, aviso, status na sidebar, título). Mandar texto ao shell exige a
  permissão `pty`. Exemplo: `examples/plugins/session-timer`. Guia:
  [PLUGIN_QUICKSTART.md](PLUGIN_QUICKSTART.md).
- `--snapshot … --plugins DIR` carrega plugins no modo sem janela.
- **Sessões persistentes (multiplexer de verdade)**: daemon
  `code-terminal-cli mux-server` (socket Unix `0600`, sobe sob demanda, sai
  com a última sessão) mantém os shells vivos; `mux-attach [--create]` anexa
  de verdade (modo raw, `Ctrl+B d` desanexa, tela redesenhada ao anexar,
  tamanho acompanha o terminal). Antes o attach só imprimia "Attached" e nada
  atendia na porta 9876. No app, "New Persistent Tab" abre uma tab anexada, e
  o workspace reanexa ao reabrir. Ver [MULTIPLEXER.md](MULTIPLEXER.md).

- **Navegador em janela própria**: `code-terminal-browser` (GTK/WebKitGTK no
  Linux, inclusive Wayland; WebView2/WKWebView no Windows/macOS), controlado
  pelo terminal por socket `0600`. Ctrl+clique em links abre nele
  (`[browser] open_links_in`), a paleta tem "Open Browser Window" e o CLI
  ganhou `browse <url>` e `browser text|eval|back|forward|reload|info|close`.
  Antes o navegador "embutido" não aparecia no Wayland e clicar em links não
  fazia nada. Ver [BROWSER.md](BROWSER.md).

### Fixed (temas)
- Seis dos oito temas oferecidos na configuração (dracula, monokai, nord,
  one-dark, solarized-dark, solarized-light) só trocavam o nome; agora têm as
  paletas próprias. "Toggle Theme" na paleta (antes sem efeito) passa ao
  próximo tema.
- A suíte de testes compila e passa no Windows (rodada numa VM Windows:
  testes que dependem de `/bin/sh`, `cat` e afins ficaram só para Unix).

---

## Sprint 18 - Session Save/Restore (Março 2026)

### Added
- **Session Manager**: Sistema completo de persistência de sessões
  - CRUD de sessões (Create, Read, Update, Delete)
  - Validação de nomes de sessão
  - Listagem com metadados (criação, modificação, tamanho)
  - Renomeação de sessões
- **Session Serialization**: Estruturas de dados serializáveis
  - `SessionData`: Container raiz com versão, layout, configuração
  - `LayoutData`: Informações de splits e panes
  - `TabData`: Estado individual de cada aba (título, diretório, scroll)
  - `PaneData` e `SplitData`: Componentes de layout
  - Formato JSON com schema versionado
- **Session Restore**: Captura e restauração de estado
  - `SessionRestorer::capture()`: Salva estado atual do AppState
  - `SessionRestorer::restore()`: Restaura estado em AppState
  - `SessionRestorer::try_restore_last_session()`: Restaura sessão padrão
  - Preservação de: tamanho do terminal, tabs, títulos, diretórios, scroll position
- **Auto-Save**: Salvamento automático periódico
  - Timer configurável (padrão: 5 minutos)
  - Eventos que disparam auto-save: tab created/closed, layout changed, SSH changed
  - Debounce para evitar salvamentos excessivos
  - Cleanup automático de sessões antigas
  - API assíncrona com controle (pause, resume, force_save, shutdown)
- **Session Backup**: Sistema de backup automático
  - Backup automático antes de sobrescrever (até 5 versões)
  - Criação manual de backups
  - Rotação automática de backups antigos
- **Session Migration**: Migração entre versões de formato
  - `VersionManager`: Gerenciador de migrações
  - `MigrationRegistry`: Registro de migradores
  - `VersionMigrator` trait: Interface para implementar migradores
  - Detecção automática de necessidade de migração
  - Migração de caminho (path finding) via BFS
  - Suporte a forward migration
- **Configuração**: Integração com sistema de config
  - Diretório de sessões configurável
  - Sessão padrão configurável
  - Opções de compressão e histórico
  - Configuração completa de auto-save
- **Documentação**: SESSION.md completo
  - Guia de uso da API
  - Exemplos de código
  - Configuração em TOML
  - Referência da API
  - Troubleshooting

### Modules
```
session/
├── mod.rs           (126 linhas) - Tipos principais e exports
├── error.rs         (71 linhas) - Erros do sistema
├── manager.rs       (359 linhas) - Gerenciador CRUD
├── serialization.rs (335 linhas) - Estruturas de dados
├── restore.rs       (407 linhas) - Captura/restauração
├── migration.rs     (527 linhas) - Migração de versões
└── auto_save.rs     (636 linhas) - Auto-save
```
**Total: ~2.500 linhas**

---

## Sprint 19 - Multiplexer Mode (Agosto 2026)

### Added
- **Session Server**: Servidor de sessões multiplexado tipo tmux
  - `SessionServer`: Estrutura thread-safe (Arc<RwLock<>>)
  - Gerenciamento de sessões (create, get, list, remove, rename)
  - Gerenciamento de clientes (add, remove, get)
  - Attach/detach de clientes a sessões
  - Contagem de clientes por sessão
- **Session Client**: Cliente de conexão ao servidor
  - `SessionClient`: Conexão via socket (Unix/TCP)
  - Comunicação via JSON
  - Comandos CLI (mux-list, mux-create, mux-attach, mux-detach)
  - Suporte a modo somente leitura
- **Multiplexer Commands**: Comandos tipo tmux
  - `MuxCommand` enum com 40+ comandos
  - `CommandParser`: Parse de comandos no estilo tmux
  - Aliases (new, attach, detach, ls, etc.)
  - Comandos de sessão, janela, painel, buffer
- **Multiplexer Keybindings**: Atalhos de teclado
  - `MultiplexerKeyBindings`: 24 keybindings configuráveis
  - Tecla prefixo padrão (Ctrl+B)
  - Controle de sessões, janelas, panes
  - Modo cópia (copiar/colar)
- **Session List**: Listagem e seleção de sessões
  - `SessionList`: Lista com navegação e ordenação
  - `SessionListItem`: Informações de sessão
  - Ordenação por nome, data, atividade, clientes
  - Filtros avançados
- **Session Switch**: Troca entre sessões
  - `SessionSwitcher` com histórico de navegação
  - Switch next/previous/last
  - Navegação back/forward no histórico
  - Busca por nome
- **Session Persistence**: Persistência em disco
  - `SessionPersistence`: Salvar/carregar sessões
  - Auto-save configurável
  - Import/export de sessões
  - Cleanup de sessões antigas
- **Session Monitor**: Monitoramento e estatísticas
  - `SessionMonitor`: Estatísticas em tempo real
  - `SessionStats`: Bytes, comandos, attach/detach
  - `GlobalStats`: Estatísticas globais do servidor
- **Group Manager**: Organização em grupos
  - `GroupManager`: Grupos de sessões
  - Adicionar/remover sessões de grupos
  - Mover sessões entre grupos
- **Testes**: 19 testes de integração
  - `tests/multiplexer_integration_tests.rs`
  - Cobertura de todos os módulos
  - Testes end-to-end

### Modules
```
multiplexer/
├── mod.rs           (66 linhas)   - Exports e erros
├── session.rs       (200+ linhas) - Sessões persistentes
├── client.rs        (150+ linhas) - Clientes conectados
├── server.rs        (200+ linhas) - SessionServer
├── client_session.rs (300+ linhas) - SessionClient
├── protocol.rs      (150+ linhas) - Protocolo de comunicação
├── socket.rs        (150+ linhas) - Socket/pipe
├── persistence.rs   (420+ linhas) - Persistência
├── monitor.rs       (440+ linhas) - Estatísticas
├── groups.rs        (430+ linhas) - Grupos
├── commands.rs      (850+ linhas) - Comandos tmux
├── session_list.rs  (850+ linhas) - Listagem/switch
└── keybindings.rs   (400+ linhas) - Atalhos
```
**Total: ~4.500 linhas**

---

## Sprint 15 - Image Protocol (Junho 2026)

### Added
- **Kitty Graphics Protocol**: Implementação completa do protocolo de imagens inline
  - Parser de sequências de escape gráficas (ESC _ G ... ESC \)
  - Modos de transmissão: Direct, File, TempFile, Stream
  - Ações: Transmit, Display, Delete, Clear
  - Decodificação base64
- **ImageDecoder**: Suporte a múltiplos formatos
  - PNG, JPEG, GIF, WebP, BMP, TIFF, RGBA, RGB
  - Limites configuráveis de dimensões (4096x4096 padrão)
  - Limites de tamanho de arquivo (100MB padrão)
  - Resize com manutenção de aspect ratio
- **ImageCache**: Cache LRU com gerenciamento de memória
  - Limite de imagens em cache (100 padrão)
  - Limite de memória total (100MB padrão)
  - TTL para expiração automática
  - Estatísticas de uso
  - Evicção LRU automática
- **ImageRenderer**: Renderização GPU com wgpu
  - Criação de texturas GPU
  - Cálculo de dimensões em células
  - Modos de scaling: Fit, Fill, Stretch, None
  - Batch rendering para performance
  - RenderQuad para vertex data
- **ImageManager**: Gerenciador central
  - Coordena decoder, cache e renderer
  - Geração de IDs únicos
  - Processamento de comandos do protocolo
  - Garbage collection
- **ImageGarbageCollector**: Sistema de GC em background
  - Execução automática em background via tokio
  - Políticas de evicção: LRU, LFU, TTL, Hybrid
  - Thresholds configuráveis de memória e contagem
  - Builder pattern para configuração
  - Estatísticas detalhadas de operação
  - Controle manual (force_collect, start, stop)
  - 15 testes unitários
- **Documentação**: IMAGE_PROTOCOL.md com:
  - Formato do protocolo e parâmetros
  - Exemplos em Shell, Python, Rust
  - API Reference completa
  - Configuração e troubleshooting

### Changed
- Dependência `base64 = "0.22"` adicionada ao Cargo.toml

---

## Sprint 7 - Copy/Paste, Search e Selection (Março 2026)

### Added
- **Text Selection**: Seleção de texto via teclado (Shift+setas)
  - Seleção de palavra (double-click)
  - Seleção de linha (triple-click)
  - Seleção de todo o texto (Ctrl+Shift+A)
  - Visualização de texto selecionado (highlight)
  - Extração de texto selecionado
- **Mouse Selection**: Seleção de texto via mouse
  - Click-and-drag para selecionar
  - Double-click para palavra
  - Triple-click para linha
  - Suporte a seleção retangular (Alt+drag)
  - Auto-scroll durante drag
- **URL Detection**: Detecção automática de URLs
  - Detecta HTTP, HTTPS, FTP, file://
  - Destaque visual de URLs
  - Ctrl+Click para abrir no navegador
  - API para verificar URL em posição
- Copy/Paste do clipboard (arboard)
- Search/Find no scrollback com highlighting

---

## Sprint 10 - Multi-threading (Março 2026)

### Added
- Arquitetura multi-threaded completa (IO, Parser, Render threads)
- ThreadManager para gerenciamento de ciclo de vida das threads
- TerminalChannels para comunicação entre threads via mpsc
- Estruturas lock-free: AtomicQueue, AtomicStack, AtomicRingBuffer, AtomicCounter
- Sincronização eficiente com ThreadNotifier (Condvar), ThreadBarrier, ThreadSemaphore
- WorkerStateControl para máquina de estados das threads
- ThreadPool para tarefas assíncronas
- ThreadMetrics para profiling de performance

### Changed
- Refatoração da arquitetura para separar IO, parsing e renderização
- Melhoria na performance sob carga (target: 60 FPS constante)

---

## Sprint 9 - Benchmarking (Março 2026)

### Added
- Suite completa de benchmarks com Criterion
- Integração com vtebench (ferramenta padrão da indústria)
- Benchmarks de throughput (> 1M chars/s target)
- Benchmarks de latência (< 5ms input-to-output)
- Benchmarks de memória (dhat/heaptrack)
- Comparação com concorrentes (Alacritty, Kitty, WezTerm)
- CI job para executar benchmarks automaticamente
- Documentação de baseline de performance

### Changed
- Estabelecimento de targets de performance:
  - Throughput: > 1,000,000 caracteres/segundo
  - Latência input-output: < 5ms
  - Frame time: < 16.67ms (60 FPS)
  - Memória base: < 50MB

---

## Sprint 8 - Themes (Março 2026)

### Added
- Sistema de temas visuais completo
- 12 temas built-in (Dracula, Nord, Solarized Dark/Light, Monokai, One Dark, etc.)
- Suporte a true color (24-bit)
- Carregamento de temas customizados via TOML
- Font ligatures (configurável)
- Font fallback com lista configurável
- Configuração de fonte via arquivo TOML

### Changed
- Estrutura ColorScheme refatorada para suportar temas
- Integração tema-fonte melhorada

---

## Sprint 7 - Copy/Paste e Search (Março 2026)

### Added
- Copy/Paste do clipboard (crate `arboard`)
  - `Ctrl+C`: Copiar conteúdo visível
  - `Ctrl+V`: Colar do clipboard
- Barra de busca (SearchBar) com UI
- Busca incremental em tempo real
- Highlight de matches na grid
- Navegação entre matches (F3/Shift+F3)
- Scroll automático para match encontrado
- Opções de busca: case sensitive, whole word, regex (preparado)

### Changed
- Integração clipboard com AppState
- Células do grid agora têm campo `highlight` para matches

---

## Sprint 6 - Splits/Pane Management (Março 2026)

### Added
- Sistema de splits (panes) estilo tmux
- Split horizontal (`Ctrl+B "`) - panes empilhados verticalmente
- Split vertical (`Ctrl+B %`) - panes lado a lado
- Navegação entre panes (`Alt+Setas`)
- Redimensionamento de panes
- Fechar pane atual (`Ctrl+B X`)
- Layout automático proporcional
- Cada pane com TabManager independente
- Layout tree com SplitNode (recursive)
- LayoutManager para gerenciamento de layout
- PaneRenderer para renderização multi-pane

### Changed
- Arquitetura de UI refatorada para suportar hierarquia Window → Layout → Pane → Tabs
- Renderização agora suporta múltiplos panes simultâneos

---

## Sprint 5 - Tabs (Março 2026)

### Added
- Sistema de múltiplas abas (tabs)
- Tab bar visual no topo da janela
- Criar nova tab (`Ctrl+Shift+T`)
- Fechar tab com confirmação opcional (`Ctrl+W`)
- Navegação entre tabs:
  - `Ctrl+Tab` / `Ctrl+Shift+Tab`: Próxima/Anterior
  - `Ctrl+1-9`: Ir para tab específica
- Cada tab com PTY próprio e independente
- Título dinâmico da tab (baseado em shell/hostname/processos)
- TabStateManagement para persistência de estado
- TabCloseConfirmation para diálogo de confirmação

### Changed
- TerminalBuffer agora gerencia múltiplas tabs
- AppState integrado com TabManager

---

## Sprint 4 - Scrollback e Configuração (Março 2026)

### Added
- Navegação no scrollback (`Shift+PageUp/Down`)
- Mouse wheel scroll
- Scroll offset tracking
- Auto-scroll para final quando novo output chega
- Parser de configuração TOML
- Hot reload de configuração (com debounce 500ms)
- Configuração de fonte (família, tamanho, line height, bold, italic)
- Configuração de cores (color schemes)
- Configuração de keybindings (estrutura base)
- Configuração de terminal (shell, working directory, scrollback limit)
- Arquivo de config em:
  - Linux/macOS: `~/.config/code-terminal/config.toml`
  - Windows: `%APPDATA%\code-terminal\config.toml`

### Changed
- Grid refatorado para suportar scrollback navegável
- Sistema de config integrado ao startup

---

## Sprint 3 - VT Parser e Escape Sequences (Março 2026)

### Added
- Integração com crate `vte` para parsing VT100/XTerm
- Grid de caracteres 2D com células ricas
- Cursor positioning (CUP, CUU, CUD, CUF, CUB, CNL, CPL, CHA)
- Cores ANSI 16 cores + 256 cores + true color (24-bit)
- Atributos de texto: bold, italic, underline, blink, reverse, hidden, strikethrough
- Clear screen (ED) e clear line (EL)
- Scroll regions
- Modos de terminal (DECCKM, DECCOLM, DECTCEM, alternate screen)
- Suporte a vim, htop, less, tmux
- 28 testes unitários para parser

### Changed
- Terminal agora interpreta escape sequences corretamente
- Buffer refatorado para usar Grid ao invés de string simples

---

## Sprint 2 - PTY e Input (Março 2026)

### Added
- Integração com `portable-pty` para PTY cross-platform
- Detecção automática de shell padrão (bash, zsh, fish, cmd, PowerShell)
- Conexão PTY ao renderer
- Input handling básico de teclado
- Teclas especiais: Enter, Backspace, Tab, Escape
- Modificadores: Ctrl, Alt, Shift
- Resize handling (redimensionamento de janela)
- Process cleanup ao fechar terminal
- Comunicação async com tokio

### Changed
- Arquitetura de event loop para suportar async
- Separação entre window events e PTY I/O

---

## Sprint 1 - Fundação (Março 2026)

### Added
- Estrutura inicial do projeto Rust
- Configuração do Cargo.toml com dependências principais
- Estrutura de pastas profissional
- CI/CD pipeline com GitHub Actions (build, test, lint)
- Integração com wgpu para renderização GPU
- Integração com winit para janelas
- Integração com glyphon para text rendering
- Font default carregada
- Janela abrindo com texto renderizado
- Redimensionamento de janela funciona
- README.md com instruções de build

### Tech Stack Inicial
- Rust 1.85+ (stable)
- wgpu 24.x (WebGPU)
- winit 0.30 (janelas)
- glyphon 0.8 (text rendering)
- tokio 1.x (async runtime)

---

## Resumo de Sprints

| Sprint | Tema | Pontos | Status |
|--------|------|--------|--------|
| 1 | Fundação | 20 | ✅ Completa |
| 2 | PTY e Input | 45 | ✅ Completa |
| 3 | VT Parser | 52 | ✅ Completa |
| 4 | Scrollback e Config | 36 | ✅ Completa |
| 5 | Tabs | 42 | ✅ Completa |
| 6 | Splits | 44 | ✅ Completa |
| 7 | Copy/Paste e Search | 25 | ✅ Completa |
| 8 | Themes | 37 | ✅ Completa |
| 9 | Benchmarking | 30 | ✅ Completa |
| 10 | Multi-threading | 42 | ✅ Completa |
| 15 | Image Protocol | 43 | ✅ Completa |
| 18 | Session Save/Restore | 35 | ✅ Completa |
| 19 | Multiplexer Mode | 52 | ✅ Completa |

**Total de Pontos Completos:** 460 pontos

---

## Próximos Passos

- Sprint 16: AI Integration
- Sprint 17: SSH Manager (Parte 2)
- Sprint 20: Multiplexer (Parte 2)
- Sprint 21+: Plugin System

---

*Última atualização: 13 de Março 2026 - Sprint 19 completa!*
