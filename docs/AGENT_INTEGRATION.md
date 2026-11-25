# Integração com agentes de código

O Code Terminal foi pensado para rodar vários agentes (Claude Code, Codex,
Aider, Gemini CLI...) lado a lado e avisar quando um deles precisa de você.

## O que o terminal faz sozinho

- **Avisos dos programas:** OSC 9 (iTerm2), OSC 777 (urxvt/foot), OSC 99
  (kitty) e a campainha (BEL) viram aviso do painel que os mandou.
- **Anel:** o painel que pediu atenção ganha uma borda colorida até você
  olhar para ele.
- **Sidebar:** a tab mostra quantos avisos não lidos tem (Ctrl+Shift+B
  mostra/esconde).
- **Desktop:** se você não está olhando (outra tab, outro painel ou janela
  sem foco), sai uma notificação do sistema (`notify-send` no Linux,
  `osascript` no macOS).
- **Atalhos:** Ctrl+Shift+U vai para o painel com o aviso mais recente;
  Ctrl+Shift+I mostra o histórico de avisos.

Para o Claude Code, ligar o canal de notificação já basta (`/config` →
"Notifications" → iTerm2, kitty ou ghostty).

## Variáveis em cada painel

| Variável | Conteúdo |
|---|---|
| `CODE_TERMINAL_PANE_ID` | id do painel (o CLI usa para avisar o painel certo) |
| `CODE_TERMINAL_SOCKET` | caminho do socket da Socket API |
| `TERM_PROGRAM` | `code-terminal` |

## CLI (`code-terminal-cli`)

```sh
code-terminal-cli notify -t "Build" "terminou sem erros"   # acende o painel atual
code-terminal-cli read -n 20                               # últimas 20 linhas do painel
code-terminal-cli list-tabs
code-terminal-cli list-panes
code-terminal-cli split vertical
code-terminal-cli input "make test\n"
code-terminal-cli preview foto.png      # imagem inline (também markdown e JSON formatados)
```

## Hooks do Claude Code

`code-terminal-cli agent-hook` lê o JSON que o Claude Code manda a cada
evento e avisa o painel do agente nos eventos que pedem atenção
(`Notification`, `Stop`, `SubagentStop`). Fora do Code Terminal ele não faz
nada e nunca falha, para não atrapalhar o agente.

Gere o trecho para o `~/.claude/settings.json`:

```sh
code-terminal-cli agent-hook-config
```

Saída:

```json
{
  "hooks": {
    "Notification": [{ "hooks": [{ "type": "command", "command": "code-terminal-cli agent-hook --agent 'Claude Code'" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "code-terminal-cli agent-hook --agent 'Claude Code'" }] }],
    "SubagentStop": [{ "hooks": [{ "type": "command", "command": "code-terminal-cli agent-hook --agent 'Claude Code'" }] }]
  }
}
```

Outros agentes que aceitam comando em eventos podem usar o mesmo comando
(`--agent "Nome"`), mandando um JSON com `hook_event_name` e `message`.

## Socket API

A Socket API (JSON-RPC 2.0 por socket Unix `0600`) controla a janela real.
Métodos: `tab.list`, `tab.create`, `tab.switch`, `tab.close`, `tab.rename`,
`tab.nest`, `tab.unnest`, `pane.list`,
`pane.split`, `pane.focus`, `pane.close`, `keyboard.send`, `keyboard.text`,
`terminal.read`, `notification.send`, `notification.list`,
`notification.jump`, `sidebar.toggle` e `state.get`. Detalhes em
[SOCKET_API.md](SOCKET_API.md).
