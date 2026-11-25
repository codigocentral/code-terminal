# Counter Plugin Example

Exemplo completo de plugin WASM para o Code Terminal. Este plugin demonstra todas as capacidades do sistema de plugins.

## 📋 Funcionalidades

- ✅ **Contador de comandos**: Conta cada comando executado no terminal
- ✅ **Notificações**: Alerta a cada N comandos (configurável)
- ✅ **Comandos especiais**: `/counter-reset` e `/counter-status`
- ✅ **Event handlers**: Responde a tabs, temas, etc.
- ✅ **Logging**: Usa o sistema de logging do host
- ✅ **Configuração**: Threshold configurável via manifest

## 🚀 Quick Start

### Pré-requisitos

- [Rust](https://rustup.rs/) (1.70+)
- `wasm32-wasi` target:
  ```bash
  rustup target add wasm32-wasi
  ```

### Compilação

```bash
# Navegue até o diretório do plugin
cd examples/plugins/counter-plugin

# Compile para WASM
 cargo build --release --target wasm32-wasi

# Ou use o script de build
./build.sh  # Linux/macOS
build.bat   # Windows
```

### Instalação

1. Copie o arquivo compilado para a pasta de plugins:
   ```bash
   cp target/wasm32-wasi/release/counter-plugin.wasm ~/.config/code-terminal/plugins/
   cp plugin.toml ~/.config/code-terminal/plugins/counter-plugin/
   ```

2. Reinicie o Code Terminal ou recarregue os plugins

3. Verifique se o plugin está carregado:
   ```
   > plugin list
   counter-plugin v0.1.0 [running]
   ```

## 📝 Uso

### Comandos do Plugin

- `/counter-reset` - Reseta o contador para zero
- `/counter-status` - Mostra o contador atual e threshold

### Configuração

Edite `plugin.toml` para alterar o comportamento:

```toml
[config]
# Notificar a cada 5 comandos
threshold = "5"

# Desabilitar notificações
enable_notifications = "false"
```

### Exemplo de Uso

```bash
$ ls                    # Count: 1
$ cd /home              # Count: 2
$ pwd                   # Count: 3
...
$ ls -la                # Count: 10

[Counter Plugin] 🎉 10 commands executed! (threshold: 10)

$ /counter-status
[Counter Plugin] Count: 10 / Threshold: 10

$ /counter-reset
[Counter Plugin] Counter reset!
```

## 🏗️ Estrutura do Projeto

```
counter-plugin/
├── Cargo.toml          # Configuração Rust
├── plugin.toml         # Manifest do plugin
├── README.md           # Este arquivo
├── build.sh            # Script de build (Unix)
├── build.bat           # Script de build (Windows)
└── src/
    └── lib.rs          # Código do plugin
```

## 🔌 API do Host

O plugin usa as seguintes host functions:

### Terminal
- `host_write(ptr, len)` - Escreve no terminal
- `host_get_buffer_size(out_ptr)` - Obtém dimensões

### Logging
- `host_log_info(ptr, len)` - Log nível INFO
- `host_log_debug(ptr, len)` - Log nível DEBUG
- `host_log_warn(ptr, len)` - Log nível WARN
- `host_log_error(ptr, len)` - Log nível ERROR

### Config
- `host_get_plugin_config(ptr, len)` - Lê configuração

## 🔄 Ciclo de Vida

```
Carregamento
    ↓
_start() → init() → [Running]
    ↓                    ↓
    ←←←← on_input() ←←←←
    ↓                    ↓
    ←←←← update() ←←←←←←
    ↓                    ↓
shutdown() ←←← [Stopping]
```

## 🧪 Desenvolvimento

### Testes

```bash
# Testes unitários
cargo test

# Build de desenvolvimento
cargo build --target wasm32-wasi

# Inspecionar WASM
wasm2wat target/wasm32-wasi/debug/counter-plugin.wasm
```

### Debug

Habilite logging detalhado no Code Terminal:

```bash
# No terminal do Code Terminal
export RUST_LOG=plugin=debug
```

## 📦 Distribuição

Para distribuir o plugin:

1. Compile em release mode
2. Crie um pacote:
   ```bash
   mkdir counter-plugin-v0.1.0
   cp target/wasm32-wasi/release/counter-plugin.wasm counter-plugin-v0.1.0/
   cp plugin.toml counter-plugin-v0.1.0/
   cp README.md counter-plugin-v0.1.0/
   zip -r counter-plugin-v0.1.0.zip counter-plugin-v0.1.0/
   ```

3. Publique no registry de plugins

## 📝 Notas de Implementação

### Memory Safety
- Use apenas memória alocada pelo WASM
- Não faça pointers inválidos para o host
- Valide todos os inputs

### Performance
- Minimize alocações no `update()`
- Use `AtomicU64` para contadores thread-safe
- Evite cópias desnecessárias

### Compatibilidade
- Mantenha compatibilidade com WASI
- Não use APIs não-portáveis
- Teste em diferentes plataformas

## 📚 Referências

- [WebAssembly](https://webassembly.org/)
- [WASI](https://wasi.dev/)
- [Code Terminal Plugin API](../../docs/plugin-api.md)
- [Exemplos de Plugins](../README.md)

## 🤝 Contribuindo

Este é um exemplo oficial. Para sugestões:

1. Abra uma issue no repositório principal
2. Envie um PR com melhorias
3. Documente bem as mudanças

## 📄 Licença

Este exemplo é MIT: copie à vontade. Pela [exceção para plugins](../../../LICENSE-PLUGIN-EXCEPTION.md), o seu plugin pode ter qualquer licença.

---

**Desenvolvido para Code Terminal** 🚀
