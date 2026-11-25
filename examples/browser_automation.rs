//! Exemplo: Browser com Automação
//!
//! Este exemplo demonstra como usar o navegador integrado
//! com a API de automação para controlar páginas web.

use code_terminal::browser::{
    automation::{AutomationConfig, AutomationIntegration},
    BrowserConfig, BrowserManager, HomepageConfig,
};
use parking_lot::RwLock;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Inicializa logging
    tracing_subscriber::fmt::init();

    println!("🌐 Code Terminal - Browser Automation Demo\n");

    // Cria o gerenciador de browsers
    let browser_manager = Arc::new(RwLock::new(BrowserManager::new()));

    // Configura o browser
    let config = BrowserConfig {
        homepage: HomepageConfig::Url("https://example.com".to_string()),
        ..Default::default()
    };

    // Cria uma instância do browser
    println!("📱 Criando browser...");
    let browser_id = {
        let manager = browser_manager.read();
        manager.create_browser(Some(config))?
    };
    println!("✅ Browser criado: {}\n", browser_id);

    // Cria a integração de automação
    let automation =
        AutomationIntegration::new(Arc::clone(&browser_manager)).with_config(AutomationConfig {
            default_timeout: std::time::Duration::from_secs(30),
            ..Default::default()
        });

    // Demonstra navegação
    println!("🚀 Navegando para example.com...");
    automation
        .navigate(browser_id, "https://example.com")
        .await?;
    println!("✅ Navegação iniciada\n");

    // Espera carregar
    println!("⏳ Aguardando carregamento...");
    automation
        .wait_for_navigation(browser_id, Some(std::time::Duration::from_secs(10)))
        .await?;
    println!("✅ Página carregada\n");

    // Obtém título
    println!("📄 Obtendo HTML da página...");
    let html = automation.get_page_html(browser_id).await?;
    println!("✅ HTML obtido ({} bytes)\n", html.len());

    // Obtém texto
    println!("📝 Obtendo texto da página...");
    let text = automation.get_page_text(browser_id).await?;
    println!("📃 Texto: {}...\n", &text[..text.len().min(200)]);

    // Executa JavaScript
    println!("⚡ Executando JavaScript...");
    let script = r#"
        (() => {
            return {
                title: document.title,
                url: window.location.href,
                links: Array.from(document.querySelectorAll('a')).map(a => ({
                    text: a.textContent.trim(),
                    href: a.href
                })).slice(0, 5)
            };
        })()
    "#;

    let script_result = automation.execute_script(browser_id, script).await?;
    println!("✅ Resultado do script: {:?}\n", script_result);

    // Busca elementos
    println!("🔍 Buscando elementos...");
    let selector_result = automation.query_selector(browser_id, "h1").await?;
    if selector_result.found {
        println!(
            "✅ Elemento h1 encontrado: {:?}\n",
            selector_result.text_content
        );
    } else {
        println!("⚠️ Elemento h1 não encontrado\n");
    }

    // Obtém árvore de acessibilidade
    println!("♿ Obtendo árvore de acessibilidade...");
    let a11y_tree = automation.get_accessibility_tree(browser_id).await?;
    println!("✅ Árvore obtida ({} bytes)\n", a11y_tree.len());

    // Clica em um link (se houver)
    println!("🖱️ Tentando clicar no primeiro link...");
    match automation.click_element(browser_id, "a").await {
        Ok(result) => println!("✅ Clique: {:?}\n", result),
        Err(e) => println!("⚠️ Clique falhou: {}\n", e),
    }

    // Navegação de volta
    println!("⬅️ Voltando...");
    automation.go_back(browser_id).await?;
    println!("✅ Voltou\n");

    // Limpa dados
    println!("🧹 Limpando dados do browser...");
    // Nota: clear_data requer WebView real
    println!("⚠️ clear_data requer WebView real inicializado\n");

    // Destrói o browser
    println!("🗑️ Destruindo browser...");
    {
        let manager = browser_manager.read();
        manager.destroy_browser(browser_id)?;
    }
    println!("✅ Browser destruído\n");

    println!("✨ Demo completa!");
    Ok(())
}
