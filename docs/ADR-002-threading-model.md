# ADR-002: Modelo de Threading

**Status:** Aceito
**Data:** 2026-03-12
**Autores:** Tech Lead, Codeterm Team

---

## Contexto

O Codeterm precisa processar multiplas operacoes concorrentes com latencia minima (< 5ms input-to-screen):

1. **Rendering GPU** -- composicao de frames via wgpu, submetendo command buffers a GPU
2. **Leitura de PTY** -- receber output de processos filhos (pode ser rajadas de MB/s, ex: `cat` de arquivo grande)
3. **Parsing de escape sequences** -- interpretar VT100/XTerm sequences do stream de bytes
4. **Input do usuario** -- teclado e mouse devem ter resposta imediata
5. **UI updates** -- atualizar tabs, sidebar, notificacoes, search
6. **IO de configuracao/plugins** -- leitura de arquivos, IPC com CLI/Socket API

O desafio e balancear throughput (processar grandes volumes de output) com latencia (responder ao input do usuario sem jank).

Referencia: Ghostty usa threads separadas para rendering e terminal; Alacritty usa um event loop sincrono na main thread com IO assincrono.

## Decisao

Adotamos um modelo hibrido com **3 threads principais** e **tokio para IO assincrono**:

### Thread 1: Main Thread (UI + Rendering)
- Event loop do winit (obrigatorio -- winit exige rodar na main thread)
- egui update e layout
- Submissao de command buffers wgpu
- Processamento de input do usuario
- **Nunca bloqueia**: todo IO e delegado para outras threads

### Thread 2: Terminal Thread (por sessao de terminal)
- Leitura do PTY via `tokio::io::AsyncReadExt`
- Parsing de escape sequences (vte)
- Atualizacao do buffer de tela (grid)
- Envia "dirty frames" para a main thread via channel

### Thread 3: Tokio Runtime (background IO)
- Configuracao hot reload (file watching via `notify`)
- Socket API / IPC
- Plugin execution
- Network (SSH, futuro)
- Desktop notifications

### Comunicacao entre threads
- **Terminal -> Main:** `crossbeam::channel` (bounded, para backpressure) enviando `TerminalEvent` (buffer atualizado, titulo mudou, bell, etc.)
- **Main -> Terminal:** `crossbeam::channel` (unbounded) enviando `InputEvent` (keypress, resize, etc.)
- **Tokio -> Main:** `winit::event_loop::EventLoopProxy` para acordar o event loop
- **Shared state:** `Arc<Mutex<TerminalBuffer>>` para o grid, com lock apenas durante swap de frames (< 1us)

### Diagrama

```
Main Thread (UI)          Terminal Thread (por tab)      Tokio Runtime
+------------------+      +----------------------+      +----------------+
| winit event loop |<---->| PTY read (async)     |      | Config reload  |
| egui update      |      | VTE parser           |      | Socket API     |
| wgpu render      |      | Grid/Buffer update   |      | Plugin exec    |
| input dispatch   |      |                      |      | File watch     |
+------------------+      +----------------------+      +----------------+
        ^                          |                           |
        |    TerminalEvent         |                           |
        +-------(channel)----------+                           |
        |                                                      |
        +-----------(EventLoopProxy)--------------------------+
```

## Consequencias

### Positivas

- **Latencia de input minimizada.** A main thread nunca bloqueia em IO de PTY. Keypresses sao processados imediatamente no event loop do winit e despachados via channel.
- **Throughput de PTY maximizado.** A terminal thread pode processar rajadas de output em velocidade maxima sem competir com rendering. Backpressure via bounded channel evita que a main thread acumule trabalho.
- **Escalavel com multiplas tabs.** Cada tab tem sua propria terminal thread. Tabs inativas podem ter suas threads pausadas (park) ate que haja output.
- **Modelo mental simples.** Tres responsabilidades claras, tres threads. Sem thread pool generico que dificulta debugging.
- **Frame skipping natural.** Se a terminal thread produzir updates mais rapido que 60fps, a main thread descarta frames intermediarios e renderiza apenas o mais recente.

### Negativas

- **Complexidade de sincronizacao.** Comunicacao via channels e shared state exige cuidado com deadlocks e race conditions. Mitigacao: usar patterns bem definidos (channel para eventos, Arc<Mutex> apenas para o grid buffer com locks ultra-curtos).
- **Uma thread por tab.** Com 50+ tabs abertas, pode haver pressao de memoria por stack de thread (~8MB cada por padrao no Linux). Mitigacao: reduzir stack size com `std::thread::Builder::stack_size()` para 1MB (suficiente para PTY read + parse).
- **Tokio runtime adiciona complexidade.** Ter tokio rodando em paralelo com threads manuais pode confundir. Mitigacao: isolar tokio para IO puro, nunca usar tokio na main thread ou terminal thread.

## Alternativas Consideradas

### Single-threaded event loop (estilo Alacritty)

**Motivos de rejeicao:**
- Funciona bem para um terminal simples, mas nao escala para multiplas tabs com notification rings, socket API e plugins
- Uma rajada grande de PTY output bloquearia o rendering e causaria jank
- Alacritty nao tem tabs/splits/plugins justamente por essa limitacao

**Pontos fortes:**
- Mais simples de implementar e debugar
- Zero custo de sincronizacao

### Async puro com tokio (tudo em tasks)

**Motivos de rejeicao:**
- winit exige main thread, entao o event loop nao pode ser async
- wgpu submission precisa ser sincrono na main thread
- Misturar async com rendering e GUI adiciona complexidade sem beneficio claro
- Diffucil garantir latencia deterministica com scheduling de tasks

**Pontos fortes:**
- Modelo unificado de concorrencia
- Bom para IO-bound workloads

### Thread pool generico (rayon)

**Motivos de rejeicao:**
- Terminal threads sao long-lived, nao work-stealing tasks
- rayon e otimizado para paralelismo de dados, nao para IO concorrente
- Nao se encaixa no modelo de "uma thread por sessao PTY"

---

**Notas de implementacao:**
- Usar `crossbeam::channel::bounded(16)` para Terminal -> Main (16 frames de backpressure)
- Usar `crossbeam::channel::unbounded()` para Main -> Terminal (input nunca deve ser dropado)
- Terminal thread usa `std::thread::spawn`, nao tokio::spawn (para evitar competicao com o runtime)
- Tokio runtime: `tokio::runtime::Builder::new_multi_thread().worker_threads(2)` (2 workers suficientes para IO)
- Frame skipping: main thread drena o channel e usa apenas o ultimo `TerminalEvent::BufferUpdated`

---

## Atualização (21/11/2026): nomes das threads e acordadas com a janela parada (#30)

Toda thread que o Code Terminal cria tem nome (`std::thread::Builder::name`,
pelo `utils::spawn_named`), inclusive o pool global do rayon (`pool-N`), que o
cosmic-text usa para ler as fontes e o wasmtime para compilar plugins. Assim
`/proc/<pid>/task/*/comm`, o `top -H` e o gdb mostram quem é quem. Os nomes
são: `pty-reader`, `socket-api-unix`, `socket-api-conn`, `font-preload`,
`gpu-prewarm`, `git-status`, `toast-clock`, `plugin-clock`, `notify-reaper`,
`assistant`, `open-link`, `open-browser`, `pool-N`.

**Regra:** thread parada espera por evento (canal com `recv`, `Condvar`,
`poll` no fd, `EventLoopProxy` do winit), nunca por laço com `sleep` ou
`recv_timeout` curto. Prazo só quando há algo para vencer (o debounce do
watcher de config dorme até o próximo evento e só usa prazo com uma mudança
pendente).

### Janela parada (60 s, `/proc/<pid>/task/*/status`)

| Thread | Acordadas/s | De quem |
|---|---|---|
| principal (`code-terminal`) | ~0,7 | event loop do winit |
| `pty-reader`, `socket-api-unix`, `pool-N`, demais | 0 | nossas, esperam evento |
| `[vkps] Update` | ~4 | driver NVIDIA (cache de pipelines) |
| sem nome (`code-terminal`) | ~0,5 | driver NVIDIA |
| sem nome (`code-terminal`) | **~100** | driver NVIDIA |

As ~100 acordadas/s não são do Code Terminal: o driver NVIDIA (testado no
615.78, `libnvidia-glcore`) cria essa thread no `vkCreateDevice` e ela dorme
num `pthread_cond_timedwait` de 10 ms. Um programa de 40 linhas com o `ash`
que só cria a instância e o `VkDevice` e dorme mostra as mesmas 99,6
acordadas/s (só a instância: nenhuma). No modo servidor as duas threads do
driver aparecem como `gpu-prewarm`: no Linux uma thread nova herda o nome de
quem a criou, e quem chama o `vkCreateDevice` ali é a nossa `gpu-prewarm`
(na janela comum é a principal). Nenhuma variável do driver
(`__GL_THREADED_OPTIMIZATIONS`, `__GL_SHADER_DISK_CACHE`, `__GL_YIELD`,
`__NV_DISABLE_EXPLICIT_SYNC`, `__GL_FLUSH_CONTROL`) a desliga. Ela existe
enquanto houver um device Vulkan aberto, inclusive no modo servidor sem
janela (a GPU fica ligada para a próxima janela abrir rápido). Custo:
~0,1 % de uma CPU. Não foi medido em GPU Intel/AMD (Mesa) nem no macOS.
