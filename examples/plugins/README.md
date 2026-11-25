# Plugin Examples

## session-timer (comece por este)

Plugin WASM que roda no app hoje: mostra na sidebar o tempo de sessão e
quantos comandos foram digitados, e escreve uma saudação no painel. Usa as
funções do módulo `host` descritas em
[docs/PLUGIN_QUICKSTART.md](../../docs/PLUGIN_QUICKSTART.md).

```bash
cd session-timer
cargo build --release --target wasm32-unknown-unknown
mkdir -p ~/.config/code-terminal/plugins/session-timer
cp plugin.toml target/wasm32-unknown-unknown/release/session_timer.wasm \
   ~/.config/code-terminal/plugins/session-timer/
```

Os exemplos abaixo são anteriores à API atual (importam `host_*` do módulo
`env` ou dependem da crate do terminal) e servem só como referência de
estrutura.

## Hello Plugin

Plugin mínimo que demonstra o ciclo de vida básico.

```bash
cd hello-plugin
cargo build --release --target wasm32-unknown-unknown
```

### Funcionalidades
- Mensagem de inicialização
- Mensagem de encerramento

## Command Plugin

Plugin que registra comandos customizados.

```bash
cd command-plugin
cargo build --release --target wasm32-unknown-unknown
```

### Comandos Disponíveis

| Comando | Descrição |
|---------|-----------|
| `/hello [nome]` | Exibe saudação |
| `/time` | Mostra horário atual |
| `/echo <texto>` | Repete o texto |
| `/help` | Lista comandos |

### Configuração

```toml
[config]
prefix = "/"           # Prefixo dos comandos
case_sensitive = "false"  # Case sensitivity
```

## Build para WASM

Para compilar plugins para WASM:

```bash
# Adicionar target WASM (uma vez)
rustup target add wasm32-unknown-unknown

# Build
cargo build --release --target wasm32-unknown-unknown
```

O arquivo `.wasm` será gerado em `target/wasm32-unknown-unknown/release/`.

## Instalação

1. Copie o arquivo `.wasm` para `~/.config/codeterm/plugins/<plugin-name>/`
2. Copie o `plugin.toml` para o mesmo diretório
3. Reinicie o Codeterm

```
~/.config/codeterm/plugins/
└── hello-plugin/
    ├── plugin.toml
    └── hello_plugin.wasm
```

## Desenvolvimento

Veja [PLUGIN_API.md](../../docs/PLUGIN_API.md) para documentação completa da API.
