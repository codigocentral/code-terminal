# ADR-004: Nivel de Conformidade com Standards de Terminal

**Status:** Aceito
**Data:** 2026-03-12
**Autores:** Tech Lead, Codeterm Team

---

## Contexto

Um emulador de terminal precisa implementar escape sequences e protocolos para ser compativel com aplicacoes existentes. O nivel de conformidade determina quais aplicacoes funcionam corretamente dentro do terminal.

Niveis de complexidade (cumulativo):

| Nivel | Standard | Aplicacoes Desbloqueadas |
|-------|---------|--------------------------|
| 1 | VT100 basico | bash, zsh, fish, comandos simples |
| 2 | VT220/ECMA-48 | vim, nano, less, man, dialog |
| 3 | XTerm (subconjunto) | tmux, screen, htop, midnight commander |
| 4 | XTerm (completo) | neovim, emacs -nw, weechat, kakoune |
| 5 | Protocolos modernos | kitty graphics, sixel, OSC 52, bracketed paste |

O planejamento lista "100% VT100/XTerm compatibilidade" como metrica de sucesso. Porem, XTerm implementa centenas de escape sequences, muitas obsoletas ou raramente usadas. Implementar tudo e impraticavel no MVP.

Referencia: vttest (suite de testes VT100/VT220) e a ferramenta padrao de validacao. Alacritty passa ~85% do vttest; Ghostty passa ~95%.

## Decisao

Adotamos uma estrategia de conformidade **progressiva em 3 fases**, usando o parser `vte` como base e implementando handlers incrementalmente:

### Fase 1 -- MVP (Q1 2026): VT220 + XTerm essencial

Escape sequences obrigatorias:
- **CSI (Control Sequence Introducer):** cursor movement (CUU/CUD/CUF/CUB), erase (ED/EL), SGR (cores e atributos), scroll (SU/SD), cursor save/restore
- **OSC (Operating System Command):** set title (OSC 0/1/2), set clipboard (OSC 52), hyperlinks (OSC 8)
- **DCS (Device Control String):** DECRQSS basico
- **Modos:** DECCKM, DECOM, DECAWM, bracketed paste, alternate screen buffer, mouse tracking (SGR mode)
- **Encoding:** UTF-8 completo

Criterio de aceitacao: vttest score >= 80%, tmux/neovim/htop funcionam sem artefatos visuais.

### Fase 2 -- Features Core (Q2 2026): XTerm estendido

Adicionar:
- **Mouse:** todos os modos de tracking (normal, button, any-event, SGR extended)
- **SGR completo:** 256 cores, true color (24-bit), underline styles (curly, dotted, dashed), hyperlinks
- **OSC estendido:** OSC 4 (color palette), OSC 10-19 (default colors), OSC 7 (current directory), OSC 9/99/777 (notifications -- essencial para cmux-like features)
- **Synchronized output:** DCS para batch rendering (BSU/ESU, modo 2026)
- **DECRQM/DECSM:** report e set de modos

Criterio de aceitacao: vttest score >= 90%, todas as aplicacoes comuns funcionam, notificacoes OSC funcionam para AI agents.

### Fase 3 -- Conformidade Total (Q3-Q4 2026): Protocolos modernos

Adicionar:
- **Kitty graphics protocol:** inline images
- **Kitty keyboard protocol:** desambiguacao completa de teclas
- **Sixel graphics:** compatibilidade com aplicacoes legadas de imagem
- **iTerm2 image protocol:** compatibilidade adicional
- **Shell integration:** OSC 133 (prompt marks), OSC 633 (VS Code shell integration)
- **XTGETTCAP/DECRQSS:** report de capabilities

Criterio de aceitacao: vttest score >= 95%, Kitty graphics demo funciona, shell integration com zsh/fish/bash funciona.

### TERM e terminfo

- **Fase 1:** setar `TERM=xterm-256color` (seguro, amplamente suportado)
- **Fase 2+:** criar e distribuir entrada terminfo propria (`codeterm`) com capabilities reais do terminal
- Instalar automaticamente via `tic` no primeiro startup

## Consequencias

### Positivas

- **MVP funcional rapidamente.** A fase 1 cobre 95%+ dos casos de uso diarios. Usuarios podem usar o terminal para trabalho real desde o primeiro release.
- **Incrementos mensuráveis.** vttest fornece um score objetivo para medir progresso.
- **Prioridade correta.** Notificacoes OSC (essenciais para AI agent hooks no estilo cmux) entram na fase 2, alinhado com o roadmap do projeto.
- **vte como base solida.** O parser vte ja implementa toda a state machine de parsing -- so precisamos implementar os handlers de acao.

### Negativas

- **Fase 1 tera gaps.** Aplicacoes que usam features XTerm avancadas (ex: imagens inline, kitty keyboard protocol) nao funcionarao no MVP. Mitigacao: documentar limitacoes conhecidas e priorizar baseado em feedback.
- **Manter conformidade e trabalho continuo.** Novos protocolos surgem (ex: kitty keyboard protocol foi adotado recentemente por varios terminais). Mitigacao: monitorar o ecossistema e adicionar suporte conforme demanda.
- **Testes de conformidade sao trabalhosos.** vttest e semi-automatico e requer verificacao visual. Mitigacao: criar testes automatizados para escape sequences individuais usando fuzzing de sequences e comparacao com xterm de referencia.

## Alternativas Consideradas

### Usar alacritty_terminal como biblioteca

**Motivos de rejeicao:**
- Acopla fortemente ao modelo de dados do Alacritty (grid, indexing, event system)
- Limita customizacao do buffer e grid para features avancadas (notification detection, AI agent hooks)
- Alacritty nao implementa todos os protocolos que precisamos (ex: kitty graphics)
- Dependencia em projeto externo com filosofia diferente (minimalismo)

**Pontos fortes:**
- Milhares de horas de trabalho ja investidas em conformidade
- Testado em producao por milhoes de usuarios
- Pode ser usado como referencia para implementacao

### Implementar parser VT do zero (sem vte)

**Motivos de rejeicao:**
- Parser de escape sequences e complexo e cheio de edge cases
- vte implementa corretamente a state machine descrita por Paul Flo Williams
- Reinventar e desperdicio de tempo sem ganho de performance significativo

**Pontos fortes:**
- Controle total sobre o parser
- Possibilidade de otimizacoes SIMD especificas

### Conformidade completa desde o dia 1

**Motivos de rejeicao:**
- Atrasaria o MVP em meses
- Muitas sequences sao raramente usadas na pratica
- Melhor iterar com feedback de usuarios reais

---

**Notas de implementacao:**
- Struct `TerminalHandler` implementa `vte::Perform` -- cada metodo do trait corresponde a um tipo de acao (print, execute, csi_dispatch, osc_dispatch, etc.)
- Cada escape sequence implementada deve ter pelo menos um teste unitario com input/output esperado
- Criar um modulo `terminal::sequences` com submodulos por tipo: `csi.rs`, `osc.rs`, `dcs.rs`, `sgr.rs`
- Usar `tracing::debug!` para logar sequences nao implementadas (ajuda a descobrir o que falta)
- Ferramenta `--debug-sequences` que imprime todas as sequences recebidas (protocol inspector mencionado no planejamento)
