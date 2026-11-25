# Navegador Integrado (Built-in Browser)

Documentação do sistema de navegador web integrado ao code-terminal.

## Uso: navegador em janela própria

O navegador abre numa janela própria (`code-terminal-browser`), ao lado do
terminal, e é controlado por ele. Funciona no Linux com Wayland ou X11 (GTK +
WebKitGTK), no Windows (WebView2) e no macOS (WKWebView). No Wayland não dá
para embutir um WebView dentro da janela do terminal; a janela separada é o
caminho que funciona em todo lugar, e o gerenciador de janelas a organiza
como qualquer outra (`app_id`/classe `code-terminal-browser`).

- **Ctrl+clique** num link http/https no terminal abre o link na janela, que
  é criada se ainda não existir. Para usar o navegador do sistema:
  ```toml
  [browser]
  open_links_in = "system"   # padrão: "builtin"
  ```
- **Paleta → Open Browser Window** abre a janela (ou a traz para a frente).
- **CLI**, útil também para agentes de IA:
  ```sh
  code-terminal-cli browse localhost:3000     # abre (sobe a janela se preciso)
  code-terminal-cli browse docs/index.html    # arquivo local
  code-terminal-cli browser text              # texto visível da página
  code-terminal-cli browser eval "document.title"
  code-terminal-cli browser back | forward | reload | info | close
  ```

O controle é por socket local (`$XDG_RUNTIME_DIR/code-terminal-browser.sock`,
`0600`; no Windows, TCP local com token num arquivo do perfil). Só abre
`http`, `https`, `file` e `about`: um `javascript:` vindo do terminal é
recusado.

Com driver NVIDIA no Wayland, o renderizador DMA-BUF do WebKitGTK derruba a
janela ("Error 71 (Protocol error)"); o navegador desliga esse renderizador
sozinho (`WEBKIT_DISABLE_DMABUF_RENDERER=1`) quando detecta o driver.

## Visão Geral

O code-terminal inclui um navegador web embutido baseado em WebView nativo da plataforma:

- **Windows**: WebView2 (Edge/Chromium)
- **macOS**: WKWebView (WebKit)
- **Linux**: WebKitGTK

## Arquitetura

```
┌─────────────────────────────────────────────────────────┐
│                    BrowserManager                        │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────┐  │
│  │ BrowserTab  │  │ BrowserTab  │  │  AddressBar     │  │
│  │  (WebView)  │  │  (WebView)  │  │  (Input/URL)    │  │
│  └──────┬──────┘  └──────┬──────┘  └─────────────────┘  │
│         │                │                               │
│  ┌──────┴────────────────┴──────┐  ┌─────────────────┐  │
│  │     NavigationController      │  │  Config/State   │  │
│  │    (History/Back/Forward)     │  │                 │  │
│  └───────────────────────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Módulos

### Core

| Módulo | Descrição | Arquivo |
|--------|-----------|---------|
| `config` | Configurações do browser | `src/browser/config.rs` |
| `webview` | Abstração do WebView | `src/browser/webview.rs` |
| `navigation` | Controle de navegação/histórico | `src/browser/navigation.rs` |
| `address_bar` | Barra de endereços | `src/browser/address_bar.rs` |
| `events` | Sistema de eventos | `src/browser/events.rs` |

### Plataforma

| Plataforma | Arquivo | Engine |
|------------|---------|--------|
| Windows | `src/browser/platform_windows.rs` | WebView2 |
| macOS | `src/browser/platform_macos.rs` | WKWebView |
| Linux | `src/browser/platform_linux.rs` | WebKitGTK |

### Automação (Futuro)

| Módulo | Descrição |
|--------|-----------|
| `automation` | API de automação tipo Puppeteer |
| `click` | Ações de clique |
| `fill` | Preenchimento de formulários |
| `wait` | Condições de espera |
| `screenshot` | Captura de tela |

## Uso Básico

```rust
use code_terminal::browser::{
    BrowserConfig, BrowserWebView, WebViewHandle,
    NavigationController, AddressBar
};

// Criar configuração
let config = BrowserConfig::default()
    .with_homepage(HomepageConfig::Url("https://duckduckgo.com".to_string()))
    .with_privacy_mode(PrivacyMode::Standard);

// Criar WebView
let mut webview = BrowserWebView::new(WebViewHandle(1), config);
webview.initialize()?;

// Navegar
let url = url::Url::parse("https://example.com")?;
webview.navigate(&url)?;

// Executar JavaScript
let result = webview.execute_javascript("document.title")?;
```

## Configuração

### Modos de Privacidade

```rust
pub enum PrivacyMode {
    Standard,   // Cookies, histórico, cache habilitados
    Incognito,  // Sem persistência de dados
    Secure,     // JavaScript desabilitado, máxima privacidade
}
```

### Search Engines

```rust
pub enum SearchEngine {
    DuckDuckGo,  // Padrão (privacidade)
    Google,
    Bing,
    Custom(String),
}
```

## Eventos

```rust
pub enum BrowserEvent {
    Navigation(NavigationEvent),  // Início/fim navegação
    Load(LoadEvent),              // Progresso carregamento
    Console(ConsoleEvent),        // Mensagens console
    Error(String),                // Erros
    TitleChanged(String),         // Título mudou
}
```

## Integração com Terminal

O navegador pode ser integrado ao sistema de splits do terminal:

```rust
// Criar browser em split
let browser_pane = window.create_browser_split(
    current_pane,
    SplitDirection::Vertical,
    BrowserConfig::default()
)?;
```

## Roadmap

### Sprint 23 (Atual) - Parte 1
- ✅ Estrutura base do módulo
- ✅ WebView abstraction
- ✅ Configuração e preferências
- ✅ Navegação e histórico
- ✅ Barra de endereços
- ✅ Eventos do browser
- ✅ Implementações de plataforma

### Sprint 24 (Futuro) - Parte 2
- 🔄 DevTools integrado
- 🔄 Gerenciamento de downloads
- 🔄 Autenticação/password manager
- 🔄 Extensões/hooks
- 🔄 Automação completa (Playwright-style)

## API de Automação (Preview)

```rust
// Automação tipo Puppeteer (em desenvolvimento)
let page = browser.new_page().await?;
page.goto("https://example.com").await?;
page.click("#submit").await?;
page.fill("#search", "query").await?;
let screenshot = page.screenshot().await?;
```

## Requisitos de Sistema

### Windows
- Windows 10 ou superior
- Microsoft Edge WebView2 Runtime

### macOS
- macOS 10.10 ou superior
- Framework WebKit

### Linux
- GTK3 ou GTK4
- libwebkit2gtk-4.0 ou 4.1

## Configuração TOML

```toml
[browser]
homepage = "https://duckduckgo.com"
search_engine = "DuckDuckGo"
default_zoom = 1.0
enable_javascript = true
enable_cookies = true
enable_devtools = false
privacy_mode = "Standard"

[browser.security]
enable_ad_blocker = false
enable_tracking_protection = true
secure_dns = "Cloudflare"
```
