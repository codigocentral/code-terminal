# ADR-003: Estrategia de Fallback para wgpu em GPUs Fracas e Windows

**Status:** Aceito
**Data:** 2026-03-12
**Autores:** Tech Lead, Codeterm Team

---

## Contexto

O Codeterm usa wgpu 24.0 como backend de renderizacao GPU. wgpu abstrai multiplas APIs graficas:

| Backend    | Plataforma       | Requisito                  |
|-----------|------------------|----------------------------|
| Vulkan    | Windows, Linux   | Driver Vulkan 1.0+         |
| Metal     | macOS            | macOS 10.13+               |
| DX12      | Windows          | Windows 10+                |
| DX11      | Windows (legado) | Windows 7+                 |
| OpenGL ES | Fallback         | OpenGL ES 3.0+             |

Cenarios problematicos:
1. **Windows com GPUs integradas antigas** (Intel HD 3000/4000) -- podem nao suportar Vulkan/DX12 adequadamente
2. **Linux com drivers mesa antigos** -- Vulkan pode estar ausente ou bugado
3. **VMs e ambientes remotos** (RDP, VNC, WSL sem GPU passthrough) -- sem GPU real disponivel
4. **CI/CD e ambientes headless** -- testes automatizados sem GPU

O terminal **deve funcionar em todos esses cenarios**. Um terminal que nao abre e inutilizavel, independente do motivo.

## Decisao

Implementamos uma **cadeia de fallback em 3 niveis**, com deteccao automatica e override manual:

### Nivel 1: GPU nativa (padrao)
- wgpu seleciona o melhor backend automaticamente: Vulkan > DX12 > Metal
- Pipeline completo com shaders customizados para texto, notification rings, overlays
- Target: 60fps+ com < 5ms latencia

### Nivel 2: wgpu com backend de compatibilidade
- Se Vulkan/DX12/Metal falhar na inicializacao, tentar:
  - DX11 no Windows (via `wgpu::Backends::DX11`)
  - OpenGL ES no Linux (via `wgpu::Backends::GL`)
- Mesmos shaders, mas com features reduzidas:
  - Sem MSAA (ou MSAA 2x maximo)
  - Sem compute shaders (se usados para layout de texto)
  - Notification rings simplificados (sem blur/glow)

### Nivel 3: Software rasterizer (ultimo recurso)
- wgpu com `wgpu::Backends::GL` + mesa llvmpipe (Linux) ou SwiftShader/lavapipe
- Ou: fallback para renderizacao 2D via `softbuffer` + `tiny-skia` (CPU puro)
- Features visuais minimas: texto monocromatico, sem transparencia, sem efeitos
- Target: funcional a 30fps, priorizando corretude sobre beleza

### Deteccao e selecao

```rust
// Pseudocodigo da logica de inicializacao
fn create_renderer(config: &Config) -> Result<Box<dyn Renderer>> {
    // Override manual via config
    if let Some(backend) = config.rendering.force_backend {
        return create_specific_renderer(backend);
    }

    // Tentar nivel 1
    if let Ok(r) = GpuRenderer::new(wgpu::Backends::PRIMARY) {
        return Ok(Box::new(r));
    }

    // Tentar nivel 2
    if let Ok(r) = GpuRenderer::new(wgpu::Backends::SECONDARY) {
        tracing::warn!("Usando backend de compatibilidade GPU");
        return Ok(Box::new(r));
    }

    // Nivel 3
    tracing::warn!("GPU indisponivel, usando software renderer");
    Ok(Box::new(SoftwareRenderer::new()?))
}
```

### Configuracao do usuario

```toml
[rendering]
# Opcoes: "auto", "vulkan", "dx12", "dx11", "metal", "opengl", "software"
backend = "auto"

# Desabilitar efeitos visuais pesados
reduced_effects = false
```

## Consequencias

### Positivas

- **O terminal sempre abre.** Nenhum usuario fica travado por falta de GPU compativel.
- **Experiencia degradada graciosamente.** Usuarios com GPUs boas tem a experiencia completa; usuarios com hardware limitado ainda tem um terminal funcional.
- **Testabilidade.** O software renderer permite rodar testes de renderizacao em CI sem GPU.
- **Suporte a ambientes remotos.** RDP, VNC, SSH X11 forwarding, WSL -- todos funcionam.

### Negativas

- **Complexidade no renderer.** Precisamos de um trait `Renderer` com pelo menos duas implementacoes (GPU e software), e a implementacao GPU precisa lidar com features condicionais. Mitigacao: usar feature flags no trait e degradar graciosamente.
- **Teste de 3 caminhos.** Cada feature visual precisa ser testada nos 3 niveis. Mitigacao: testes automatizados com software renderer; testes manuais para GPU em CI com GPU (GitHub Actions com runners GPU).
- **Tamanho do binario.** Incluir `softbuffer` + `tiny-skia` aumenta o binario. Mitigacao: compilar o software renderer atras de uma feature flag Cargo (`--features software-fallback`), habilitada por padrao.
- **Performance do nivel 3.** Software rendering sera visivelmente mais lento. Mitigacao: aceitar isso como trade-off -- e um fallback, nao o modo principal. Reduzir FPS target para 30 e simplificar efeitos.

## Alternativas Consideradas

### Sem fallback (exigir GPU)

**Motivos de rejeicao:**
- Exclui usuarios com hardware antigo, VMs e ambientes remotos
- Um terminal que nao abre e o pior bug possivel
- Ghostty e criticado por nao suportar Windows; nao queremos criticas semelhantes por nao suportar GPUs fracas

### OpenGL direto como fallback (sem wgpu)

**Motivos de rejeicao:**
- Manter dois pipelines de renderizacao completamente diferentes (wgpu + raw OpenGL) dobra a complexidade
- wgpu ja abstrai OpenGL ES como backend, entao usar wgpu::Backends::GL atinge o mesmo resultado sem codigo duplicado
- OpenGL esta sendo depreciado em favor de Vulkan/Metal/DX12

### Crossterm/tui-rs como fallback texto-puro

**Motivos de rejeicao:**
- Renderizar um terminal-emulator dentro de outro terminal e confuso e limitado
- Perde todas as features visuais (tabs, splits, notification rings)
- Cenario muito nicho para justificar o esforco

**Ponto forte reconhecido:**
- Seria util para modo headless/CI, mas isso pode ser resolvido com um modo `--headless` separado que nao inicializa rendering

---

**Notas de implementacao:**
- Criar trait `Renderer` com metodos: `render_text()`, `render_ui()`, `render_effects()`, `supports_feature(Feature) -> bool`
- `GpuRenderer` implementa com wgpu
- `SoftwareRenderer` implementa com softbuffer + tiny-skia
- Feature detection no startup: logar GPU detectada, backend selecionado e features disponiveis via `tracing::info!`
- Expor info do backend na UI: Settings > About > "Renderer: Vulkan (NVIDIA RTX 4090)"
