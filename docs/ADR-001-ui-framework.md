# ADR-001: Escolha do UI Framework (egui vs iced)

**Status:** Aceito
**Data:** 2026-03-12
**Autores:** Tech Lead, Codeterm Team

---

## Contexto

O Codeterm precisa de um framework de UI em Rust para construir todos os elementos de interface acima do terminal: tabs verticais com sidebar (inspirado no cmux), split panes, search bar, command palette, settings UI, notification panels com aneis visuais ao redor dos paineis, e futuramente um theme editor visual.

Requisitos criticos:
- Integracao nativa com wgpu 24.0, que ja e o nosso backend de renderizacao GPU
- Latencia de input-to-screen inferior a 5ms
- Cross-platform: Windows, Linux e macOS
- Capacidade de customizacao total do rendering (notification rings, overlays customizados)
- Coexistencia com nosso pipeline wgpu customizado para renderizacao de texto do terminal

As duas opcoes viáveis no ecossistema Rust sao **egui** (immediate mode) e **iced** (retained mode, Elm-inspired).

## Decisao

Adotamos **egui** (versao 0.31+) com **egui-wgpu** como framework de UI para o Codeterm.

### Justificativas

1. **Immediate mode e natural para terminais.** Um terminal redesenha a tela a cada frame de qualquer forma (cursor piscando, output de processos, scrolling). O modelo immediate mode do egui alinha-se perfeitamente com esse comportamento -- nao ha necessidade de gerenciar estado de widgets retidos quando o conteudo muda a cada frame.

2. **Integracao nativa com wgpu via egui-wgpu.** O crate `egui-wgpu` fornece um render pass que se encaixa diretamente no nosso pipeline wgpu existente. Podemos renderizar a UI do egui como um passo adicional no mesmo `wgpu::RenderPass` onde desenhamos o texto do terminal, sem overhead de contextos graficos separados.

3. **Mais leve e menor footprint.** egui compila rapido, tem menos dependencias e consome menos memoria que iced. Para um terminal onde performance e critica, menos overhead e melhor.

4. **Comunidade maior e mais madura.** egui tem mais usuarios, mais exemplos, mais integrações com terceiros e um ciclo de releases mais frequente. Problemas sao encontrados e corrigidos mais rapido.

5. **Customizacao total do rendering.** egui permite interceptar o pipeline de renderizacao e desenhar primitivas customizadas. Isso e essencial para features como notification rings (aneis de notificacao ao redor dos paineis), indicadores visuais de agentes AI e overlays customizados.

6. **Coexistencia com render pipeline customizado.** Podemos usar egui apenas para a chrome da aplicacao (tabs, menus, dialogs) enquanto o conteudo do terminal continua sendo renderizado pelo nosso pipeline wgpu+glyphon customizado. egui nao tenta "possuir" a janela inteira.

7. **Prototipagem rapida.** Immediate mode permite iterar rapidamente em layouts de UI sem precisar definir modelos de dados complexos e mensagens. Ideal para a fase de MVP.

## Consequencias

### Positivas

- Pipeline de renderizacao unificado: egui-wgpu + nosso wgpu customizado compartilham o mesmo `wgpu::Device` e `wgpu::Queue`
- Desenvolvimento mais rapido de features de UI (command palette, search, settings)
- Menor curva de aprendizado para contribuidores
- Hot reload visual facilitado pelo modelo immediate mode
- Menor tamanho de binario final

### Negativas

- **Immediate mode redesenha tudo a cada frame.** Em cenarios idle (terminal parado, sem cursor piscando), precisamos implementar logica de "skip frame" para evitar uso desnecessario de GPU/CPU. Mitigacao: usar `egui::Context::request_repaint()` seletivamente e implementar frame skipping quando nao ha mudancas.
- **Acessibilidade limitada.** egui tem suporte basico a acessibilidade comparado com frameworks retained mode. Mitigacao: implementar accessibilidade via APIs nativas do OS quando necessario (fase futura).
- **Sem layout automatico sofisticado.** Layouts complexos como a sidebar de tabs verticais com metadados ricos exigem mais codigo manual comparado ao modelo declarativo do iced. Mitigacao: criar componentes reutilizaveis internos com logica de layout encapsulada.
- **Estado de widgets e responsabilidade do desenvolvedor.** Precisamos gerenciar estado de toggles, campos de texto, scroll positions manualmente. Mitigacao: usar `egui::Memory` e structs de estado dedicadas por componente.

## Alternativas Consideradas

### iced (Elm-inspired, retained mode)

**Motivos de rejeicao:**
- iced_wgpu existe mas a integracao com um pipeline wgpu customizado externo e mais complexa -- iced quer controlar o loop de renderizacao
- Modelo retained mode (Message/Update/View) adiciona overhead conceitual e de codigo para UI que muda a cada frame
- Ecossistema menor, menos exemplos e documentacao
- Mais pesado em compilacao e dependencias
- Dificuldade de misturar rendering customizado (nosso pipeline de texto) com widgets iced

**Pontos fortes reconhecidos do iced:**
- Modelo de estado mais previsivel para UIs complexas e pouco dinamicas
- Melhor separacao de concerns entre logica e apresentacao
- Suporte nativo a async

### Rendering customizado puro (sem framework de UI)

**Motivos de rejeicao:**
- Reinventar widgets basicos (botoes, campos de texto, scroll, menus) e um esforco enorme
- Atrasa significativamente o MVP
- Nao justificavel quando egui resolve bem o problema

---

**Notas de implementacao:**
- Usar `egui::CentralPanel` para a area do terminal, com rendering customizado via `egui::PaintCallback`
- Usar `egui::SidePanel` para a sidebar de tabs verticais
- Usar `egui::TopBottomPanel` para status bar e tab bar horizontal (se aplicavel)
- Usar `egui::Window` para dialogs flutuantes (settings, command palette)
- Criar um `EguiRenderer` que integre com nosso `GpuRenderer` existente, compartilhando o mesmo `wgpu::Device`
