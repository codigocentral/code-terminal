# Browser Automation API - Sprint 24

## Visão Geral

A API de Automação do Browser permite que AI Agents controlem o navegador integrado de forma programática, possibilitando automação de tarefas web, extração de dados, testes e integração com sistemas externos.

## Módulos Implementados

### 1. Accessibility Tree API (TASK-186)

Extrai a árvore de acessibilidade da página para análise estrutural.

```rust
use code_terminal::browser::automation::AutomationController;

let automation = AutomationController::new(browser_id);
let tree = automation.get_accessibility_tree().await?;

println!("Título: {}", tree.title);
println!("Elementos interativos: {}", tree.interactive_count());

// Encontra todos os botões
let buttons = tree.find_by_role("button");
```

### 2. Click API (TASK-187)

Simula cliques do mouse com opções avançadas.

```rust
// Clique simples
automation.click("#submit-button").await?;

// Clique com opções
use code_terminal::browser::automation::click::{ClickOptions, MouseButton};

let options = ClickOptions::default()
    .with_button(MouseButton::Right)
    .with_delay_ms(100)
    .with_ctrl();

automation.click_with_options(".context-menu", options).await?;
```

### 3. Fill API (TASK-188)

Preenche formulários e interage com inputs.

```rust
// Preenche campo individual
automation.fill("#username", "john_doe").await?;

// Preenche formulário completo
use code_terminal::browser::automation::fill::FormData;

let form_data = FormData::new()
    .with_field("#firstName", "John")
    .with_field("#lastName", "Doe")
    .with_field("#email", "john@example.com");

automation.fill_form(form_data).await?;
```

### 4. JavaScript Evaluation API (TASK-189)

Executa código JavaScript na página.

```rust
// Executa script simples
let result = automation.execute_script("1 + 1").await?;
assert_eq!(result.as_number(), Some(2.0));

// Extrai dados
let title = automation.get_page_title().await?;
let url = automation.get_page_url().await?;
let html = automation.get_page_html().await?;
```

### 5. Screenshot API (TASK-190)

Captura screenshots da página.

```rust
// Página inteira
let screenshot = automation.screenshot_full().await?;

// Elemento específico
let chart = automation.screenshot_element("#chart").await?;

// Região
let region = automation.screenshot_region(0, 0, 800, 600).await?;

// Salva em arquivo
screenshot.save("screenshot.png").await?;
```

## Exemplo Completo

```rust
use code_terminal::browser::automation::{AutomationController, AutomationConfig};
use code_terminal::browser::BrowserManager;

async fn automate_login() -> anyhow::Result<()> {
    // Cria browser
    let manager = BrowserManager::new();
    let browser_id = manager.create_browser(None)?;
    
    // Configura automação
    let automation = AutomationController::new(browser_id);
    
    // Navega para a página
    automation.navigate("https://example.com/login").await?;
    
    // Espera carregamento
    automation.wait_for_navigation().await?;
    
    // Preenche formulário
    automation.fill("#username", "user@example.com").await?;
    automation.fill("#password", "secret123").await?;
    
    // Clica em login
    automation.click("button[type='submit']").await?;
    
    // Espera redirecionamento
    automation.wait_for_navigation().await?;
    
    // Verifica sucesso
    let url = automation.get_page_url().await?;
    assert!(url.contains("/dashboard"));
    
    // Tira screenshot
    let screenshot = automation.screenshot_full().await?;
    screenshot.save("dashboard.png").await?;
    
    Ok(())
}
```

## Configuração

```rust
use code_terminal::browser::automation::AutomationBuilder;

let automation = AutomationBuilder::new(browser_id)
    .with_timeout(Duration::from_secs(60))
    .with_action_delay(100)
    .with_auto_scroll(true)
    .with_screenshot_on_error(true)
    .with_human_like_behavior(true)
    .build();
```

## Integração com AI Agents

A API foi projetada para ser usada por AI Agents:

1. **Extração de Contexto**: Use `get_accessibility_tree()` para obter estrutura da página
2. **Interação**: Use `click()`, `fill()` para interagir com elementos
3. **Validação**: Use `element_exists()`, `element_visible()` para verificar estado
4. **Documentação**: Use `screenshot_full()` para documentar estado atual

## Arquitetura

```
AutomationController
├── accessibility::AccessibilityExtractor
├── click::ClickController
├── fill::FillController
├── javascript::JavaScriptEngine
├── screenshot::ScreenshotController
├── wait::WaitController
└── selector::SelectorEngine
```

## Status

- ✅ TASK-186: Accessibility Tree API
- ✅ TASK-187: Click API
- ✅ TASK-188: Fill API
- ✅ TASK-189: JavaScript Evaluation API
- ✅ TASK-190: Screenshot API
- ✅ TASK-191: API Documentation
- ✅ TASK-192: API Examples
- ✅ TASK-193: API Tests

## Próximos Passos

- Integração com WebView real (atualmente simulado)
- Suporte a frames e iframes
- Ações de drag-and-drop
- Suporte a múltiplas janelas
- Gravação e replay de sessões
