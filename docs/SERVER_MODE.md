# Modo servidor

Abrir o Code Terminal do zero leva ~150 ms, e quase todo esse tempo é o driver
de vídeo ligando a GPU (~110 ms com NVIDIA no Linux). O modo servidor paga esse
custo uma vez só: um processo fica rodando sem janela, com a GPU e as fontes já
carregadas, e cada `code-terminal` só pede uma janela nova a ele.

| | Sem servidor | Com servidor |
|---|---|---|
| Janela nova (comando até o primeiro frame) | ~150–210 ms | ~40 ms |
| Memória por janela | ~73 MB (um processo cada) | ~4,5 MB a mais no servidor |

Medido numa GTX 1660 SUPER (driver 615, Vulkan, Hyprland). A memória base do
servidor (~70 MB privados) é a mesma de um processo normal: metade é o driver
da GPU.

## Uso

```bash
code-terminal --server &   # deixa o servidor de pé
code-terminal              # abre uma janela pelo servidor e sai
```

- Sem servidor rodando, `code-terminal` abre a janela do jeito normal.
- A janela nova começa no diretório de onde o `code-terminal` foi chamado.
- O servidor não restaura nem salva o workspace (tabs da última vez): cada
  janela dele é um terminal novo, como o `footclient`. A exceção são as abas
  persistentes ([PERSISTENT_TABS.md](PERSISTENT_TABS.md)): elas vivem no
  daemon do multiplexer e voltam na próxima janela, inclusive depois de
  reiniciar o servidor.
- Fechar a última janela não encerra o servidor. SIGTERM, SIGINT ou SIGHUP
  encerram e apagam o socket.
- `CODE_TERMINAL_NO_SERVER=1` ignora o servidor e abre um processo próprio.
- `CODE_TERMINAL_SERVER_SOCKET=/caminho` troca o socket (padrão:
  `$XDG_RUNTIME_DIR/code-terminal-server.sock`, permissão `0600`).

Disponível no Linux e no macOS. No Windows, `--server` avisa que não é
suportado.

## GPU solta sem janela

Sem nenhuma janela aberta, o servidor solta a GPU (instância, adaptador e
device do wgpu) depois de um prazo, e a próxima janela liga a GPU de novo.
Isso vale também logo depois de subir: o prewarm liga a GPU e, se ninguém
pedir janela no prazo, ela sai. Solta, o driver devolve a memória dele e, na
NVIDIA, some a thread do driver que acorda 100 vezes por segundo (#30).

O prazo é escolha de quem usa, porque a janela em ~35 ms é o motivo do modo
servidor:

```toml
[server]
release_gpu_after = "5m"   # padrão; "30s" economiza mais; "0" nunca solta
```

Também em **Configurações › Sobre › Soltar a GPU sem janela aberta
(servidor)** (30 s, 1 min, 5 min, 15 min, nunca). O valor é lido quando a
última janela fecha, então a troca vale sem reiniciar o servidor.
`CODE_TERMINAL_RELEASE_GPU_AFTER` (mesmo formato) tem precedência, para testes.

| Servidor sem janela (GTX 1660 SUPER, driver 615) | GPU ligada (dentro do prazo, ou `"0"`) | Solta (`"30s"` ou `"5m"`, depois do prazo) |
|---|---|---|
| Memória (Private / PSS) | 53–99 / 77–102 MB | **37–39 / 37–39 MB** (22 / 27 MB se nunca abriu janela) |
| Acordadas por segundo | 103–104 | **0** |
| Threads | 25 | 20 |
| Janela nova | 30–38 ms | 140–228 ms (a GPU liga de novo) |

Abrir uma janela dentro do prazo continua nos ~35 ms (a GPU ainda está
ligada). O Private com a GPU ligada varia porque as páginas do driver contam
como compartilhadas quando outro programa (compositor, navegador) usa as
mesmas bibliotecas da NVIDIA; o PSS é a medida estável. Guardar só a
instância e o adaptador deixaria a janela fria em ~120–145 ms, mas seguraria
~55 MB do driver; por isso sai tudo.

## Subir junto com a sessão (Linux, systemd)

O pacote `.deb` instala `code-terminal-server.service` como serviço de usuário:

```bash
systemctl --user enable --now code-terminal-server.service
```

Instalado em outro lugar (ex.: `~/.local/bin`), copie
`packaging/linux/code-terminal-server.service` para `~/.config/systemd/user/`
trocando o `ExecStart` para `%h/.local/bin/code-terminal --server`.

## Protocolo

Uma linha JSON por conexão no socket:

```json
{"cwd": "/home/eu/projeto"}
```

A resposta sai depois que a janela desenha o primeiro frame:

```json
{"ok": true, "first_frame_ms": 28}
```

`"close_after_first_frame": true` fecha a janela logo em seguida (usado para
medir a abertura: `CODE_TERMINAL_EXIT_AFTER_FIRST_FRAME=1 code-terminal`
imprime `first_frame_ms=N` também pelo servidor).

## Como funciona

- `GpuState::prewarm` liga instance, adapter e device antes de existir janela;
  as janelas reaproveitam o device e só criam a própria superfície (o mesmo vale
  para a segunda janela de um processo normal).
- O banco de fontes lido na primeira janela é clonado para as seguintes, em vez
  de varrer as fontes do sistema de novo.
- Os sinais de saída são tratados com um handler (tokio), não com bloqueio de
  sinais: a máscara bloqueada seria herdada pelos shells, que deixariam de
  receber Ctrl+C e SIGHUP. O teste `tests/server_mode_e2e.rs`
  (`CODE_TERMINAL_SERVER_E2E=1`, abre janelas) confere isso.
