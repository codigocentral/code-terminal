# Abas persistentes

Uma aba persistente sobrevive a fechar a janela e a reiniciar o
`code-terminal-server`: o shell dela (e o que roda nele, como um Claude Code
no meio de uma tarefa) vive num processo separado, o daemon do multiplexer, e
a janela só anexa e desanexa. Aba comum continua como sempre: morre com a
janela.

Disponível no Linux. No macOS o código das abas novas persistentes é o
mesmo, mas ainda não foi testado lá; tornar persistente uma aba que já roda é
só no Linux. No Windows a opção não aparece nesta versão.

## Uso

- **Tornar persistente**: botão direito no cartão da aba › "Tornar
  persistente", ou "Toggle Persistent Tab (Tornar persistente)" na paleta. O
  shell que já está rodando passa para o daemon sem reiniciar (o mesmo
  processo, com o mesmo ambiente e o que já estava na tela). O cartão ganha
  um alfinete.
- **Deixar de ser persistente**: o mesmo item. A aba volta a morrer junto com
  a janela.
- **Aba nova já persistente**: "New Persistent Tab" na paleta. Para todas as
  abas novas serem persistentes:

  ```toml
  [session]
  persistent_tabs = true
  ```

- **Fechar a janela** (ou reiniciar o servidor): as abas persistentes só
  desanexam. Na próxima janela do Code Terminal elas voltam no mesmo lugar e
  no mesmo grupo, com título, layout dos painéis, o processo vivo e o
  histórico (scrollback).
- **Fechar a aba de propósito** (Ctrl+Shift+W, botão, menu) pergunta:
  `Enter` encerra o shell; `H` só esconde (a aba sai da janela e segue viva
  em segundo plano); `Esc` cancela.
- **Sessões em segundo plano**: "Show Background Sessions (Sessões em segundo
  plano)" na paleta traz de volta as abas escondidas.
- Uma aba persistente dividida em painéis: cada painel é uma sessão do
  daemon; os painéis novos dela também são persistentes.

## Depois de reiniciar a máquina

Processo nenhum sobrevive a um reboot (nem no tmux). O que a aba persistente
devolve:

1. a aba, o título, o grupo, o layout dos painéis e o diretório;
2. o **histórico da tela (scrollback)**, com uma linha `── aba reaberta
   (histórico até 21/11 18:40) ──` separando o de antes do shell novo;
3. o **programa que rodava**, quando ele sabe ser retomado. O comando fica
   **digitado no prompt, sem Enter**: a pessoa vê o que vai rodar e decide.
   - Claude Code: `claude --resume <id>` na mesma conversa (e no diretório
     dela). O id vem dos hooks do Claude Code: `code-terminal-cli
     agent-hook-config` mostra o trecho do `~/.claude/settings.json`, que
     agora inclui `SessionStart` e `SessionEnd`. Quem já tinha o trecho antigo
     também funciona a partir do primeiro `Stop`/`Notification` da conversa.
   - `ssh` aberto: o mesmo `ssh` (mesmos argumentos).
   - Outro programa: o último que rodou em primeiro plano ("Rodar de novo").

   Botão direito › "Retomar automaticamente: sim/não" marca a aba para
   retomar sozinha (o comando vai com Enter).

O daemon grava as abas em `~/.local/state/code-terminal/tabs/` (pasta `0700`,
arquivos `0600`): um `.json` com os dados e um `.log` com o registro da saída
(o mesmo que vai para a janela ao anexar, já limitado pela compactação).
Grava no máximo a cada 30 s, **só quando alguma aba mudou** (sem mudança a
thread de gravação fica parada, sem acordar), e de novo ao receber SIGTERM no
desligamento (o serviço usa `KillMode=mixed`: o SIGTERM vai só para o daemon,
que grava antes de os shells caírem). Encerrar a aba apaga os arquivos dela.

Para não gravar nada (e apagar o que houver):

```toml
[session]
save_scrollback = false
```

Depois do reboot, a primeira janela do Code Terminal sobe o daemon (se o
serviço não estiver habilitado) e reabre as abas; as escondidas continuam em
"Sessões em segundo plano".

## O daemon

É o `code-terminal-cli mux-server` (o mesmo do `mux attach`). O pacote instala
o serviço de usuário `code-terminal-mux.service`, separado da interface:

```bash
systemctl --user enable --now code-terminal-mux.service
```

Sem o serviço, a primeira aba persistente sobe o daemon numa unidade
temporária do systemd (`systemd-run --user`), também fora do cgroup da
interface; sem systemd, como processo solto. Instalado em outro lugar (ex.:
`~/.local/bin`), copie `packaging/linux/code-terminal-mux.service` para
`~/.config/systemd/user/` trocando o `ExecStart`.

Parar ou reiniciar **o daemon** encerra as abas persistentes (trocar o daemon
sem derrubar os shells fica para uma versão futura). Reiniciar a interface
(`systemctl --user restart code-terminal-server`) não mexe nelas.

`code-terminal-cli mux-list` mostra as sessões, inclusive as das abas.

## Como funciona

- A janela e o daemon conversam pelo socket `$XDG_RUNTIME_DIR/code-terminal-mux.sock`
  (`0600`). Uma linha JSON abre a sessão (`OpenStream`) e depois a conexão
  passa a quadros binários (`[tipo][tamanho][dados]`): cada tecla vira um
  quadro que o daemon escreve no PTY na hora, cada leitura do PTY vira um
  quadro para a janela. Sem JSON nem varredura periódica nesse caminho.
- **O grid não fica duplicado.** Quem desenha é a janela; o daemon guarda só
  um registro da saída do programa. Ao anexar, a janela recebe o registro e o
  passa pelo próprio parser, o que devolve tela, histórico, cores, título,
  links (OSC 8) e marcas (OSC 133). As consultas antigas do programa (DSR, DA)
  não são respondidas de novo e os avisos não tocam outra vez.
- O registro é compactado quando passa de 2 MB (ou do dobro do tamanho
  depois da última compactação): o daemon passa os bytes por um grid
  temporário do tamanho do histórico configurado (`terminal.scrollback_limit`),
  escreve de volta como texto ANSI e devolve a memória ao sistema. Imagens
  inline não passam pela compactação.
- **Tornar persistente uma aba que já roda**: a leitura do PTY na interface
  para, o descritor do PTY vai ao daemon pelo socket (`SCM_RIGHTS`) junto com a
  tela atual, e a aba passa a conversar com o daemon. O daemon leva o shell
  (e o que ele já rodou) para o próprio cgroup: sem isso, reiniciar o
  `code-terminal-server` pelo systemd mataria o shell junto.
- O shell de uma aba persistente nova nasce no daemon com o ambiente da
  interface (o daemon roda fora da sessão gráfica) e com
  `CODE_TERMINAL_MUX_SESSION` apontando a sessão.
- Os dados da aba (título, grupo, posição, layout dos painéis) vão para o
  daemon a cada 2 s quando mudam e ao fechar a janela.

## Custo

Medido com `tests/typing_latency.rs` e `tests/persistent_memory.rs` (build de
teste, sem otimização; 160×40, 10 000 linhas de histórico por aba):

| | Aba comum | Persistente antiga (CLI no PTY) | Persistente |
|---|---|---|---|
| Eco de uma tecla (p50 / p99) | 13–19 µs / 35–50 µs | 8,1 ms / 8,2 ms | 37–51 µs / 63–100 µs |
| Interface, por aba a mais (privada) | ~38,6 MB | ~38,0 MB | ~38,5 MB |
| Daemon, por aba a mais (privada) | — | ~39,4 MB (grid inteiro) | ~1,1 MB (registro) |
| CLI por aba (privada) | — | ~2,8 MB | — |

O custo da interface é o grid da aba (igual para todas). O daemon com 5 abas
de 60 000 linhas cada (passando pela compactação) ficou em 8,5 MB.
