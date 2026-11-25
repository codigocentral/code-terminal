//! Hello World Plugin - Plugin de Exemplo
//!
//! Este plugin demonstra como criar um plugin funcional para o Codeterm.
//! Ele mostra:
//! - Estrutura básica de um plugin
//! - Como usar o contexto do plugin
//! - Como lidar com eventos
//! - Como usar configurações
//! - Como enviar notificações

use std::collections::HashMap;

use code_terminal::plugin::api::ApiCapability;
use code_terminal::plugin::error::PluginResult;
use code_terminal::plugin::{Plugin, PluginContext, PluginInfo, PluginState};

#[derive(Debug)]
pub struct HelloPlugin {
    state: PluginState,
    greeting_count: u64,
    config_greeting: String,
}

impl HelloPlugin {
    pub fn new() -> Self {
        Self {
            state: PluginState::Created,
            greeting_count: 0,
            config_greeting: "Hello, World!".to_string(),
        }
    }
}

impl Default for HelloPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for HelloPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "hello-world".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            author: "Codeterm Team".to_string(),
            description: "Um plugin de exemplo que demonstra as APIs do sistema de plugins"
                .to_string(),
            homepage: Some("https://github.com/anomalyco/codeterm".to_string()),
            repository: Some("https://github.com/anomalyco/codeterm".to_string()),
        }
    }

    fn init(&mut self, ctx: &mut PluginContext) -> PluginResult<()> {
        tracing::info!(plugin = %ctx.plugin_name(), "Inicializando Hello World Plugin...");

        self.state = PluginState::Initializing;

        ctx.enable_capability(ApiCapability::Notifications);

        if let Some(greeting) = ctx.get_config("greeting") {
            self.config_greeting = greeting.clone();
            tracing::debug!(plugin = %ctx.plugin_name(), "Saudação customizada: {}", greeting);
        }

        self.state = PluginState::Running;

        let _ = ctx.notify("Hello World Plugin", "Plugin carregado com sucesso!");

        tracing::info!(plugin = %ctx.plugin_name(), "Hello World Plugin inicializado!");
        Ok(())
    }

    fn update(&mut self, _ctx: &mut PluginContext, _delta_ms: u64) {
        // Ciclo de update - chamado periodicamente
    }

    fn shutdown(&mut self, ctx: &mut PluginContext) {
        tracing::info!(plugin = %ctx.plugin_name(), "Desligando Hello World Plugin...");

        self.state = PluginState::Stopping;
        tracing::info!(plugin = %ctx.plugin_name(), "Total de saudações: {}", self.greeting_count);
        self.state = PluginState::Stopped;

        tracing::info!(plugin = %ctx.plugin_name(), "Hello World Plugin desligado.");
    }

    fn state(&self) -> PluginState {
        self.state
    }

    fn on_input(&mut self, ctx: &mut PluginContext, input: &str) -> bool {
        if input.contains("hello") || input.contains("ola") {
            tracing::info!(plugin = %ctx.plugin_name(), "Detectada saudação no input!");
            self.greeting_count += 1;
            let _ = ctx.notify(
                "Hello!",
                format!("{} (#{})", self.config_greeting, self.greeting_count),
            );
            return false;
        }

        if input == "/hello-stats" {
            let _ = ctx.notify("Hello Stats", format!("Saudações: {}", self.greeting_count));
            return true;
        }

        if input == "/hello-help" {
            let _ = ctx.notify(
                "Hello Help",
                "Comandos: /hello-stats, /hello-help, hello/ola",
            );
            return true;
        }

        false
    }

    fn on_tab_created(&mut self, ctx: &mut PluginContext, tab_id: u64) {
        tracing::info!(plugin = %ctx.plugin_name(), "Nova aba criada: {}", tab_id);
    }

    fn on_tab_closed(&mut self, ctx: &mut PluginContext, tab_id: u64) {
        tracing::info!(plugin = %ctx.plugin_name(), "Aba fechada: {}", tab_id);
    }

    fn on_theme_changed(&mut self, ctx: &mut PluginContext, theme_name: &str) {
        tracing::info!(plugin = %ctx.plugin_name(), "Tema mudou para: {}", theme_name);

        match theme_name {
            "dark" => {
                self.config_greeting = "Olá do lado escuro!".to_string();
            }
            "light" => {
                self.config_greeting = "Olá do lado claro!".to_string();
            }
            _ => {
                self.config_greeting = format!("Olá com tema {}!", theme_name);
            }
        }
    }

    fn default_config(&self) -> HashMap<String, String> {
        let mut config = HashMap::new();
        config.insert("greeting".to_string(), "Hello, World!".to_string());
        config.insert("auto_greet".to_string(), "false".to_string());
        config
    }

    fn apply_config(&mut self, config: &HashMap<String, String>) {
        if let Some(greeting) = config.get("greeting") {
            self.config_greeting = greeting.clone();
        }
    }
}

fn main() {
    println!("=== Hello World Plugin Example ===");
    println!();
    println!("Este é um plugin de exemplo para o Codeterm.");
    println!("Ele demonstra:");
    println!("  - Estrutura básica de um plugin");
    println!("  - Implementação do trait Plugin");
    println!("  - Uso do contexto do plugin (PluginContext)");
    println!("  - Manipulação de eventos (input, tema, abas)");
    println!("  - Configuração do plugin");
    println!("  - Notificações via notify()");
    println!();
    println!("Informações do Plugin:");

    let plugin = HelloPlugin::new();
    let info = plugin.info();

    println!("  Nome: {}", info.name);
    println!("  Versão: {}", info.version);
    println!("  Autor: {}", info.author);
    println!("  Descrição: {}", info.description);
    if let Some(ref homepage) = info.homepage {
        println!("  Homepage: {}", homepage);
    }
    if let Some(ref repo) = info.repository {
        println!("  Repositório: {}", repo);
    }
    println!();
    println!("Configuração padrão:");
    for (key, value) in plugin.default_config() {
        println!("  {}: {}", key, value);
    }
    println!();
    println!("APIs demonstradas:");
    println!("  - PluginContext::notify() - Envia notificações");
    println!("  - PluginContext::get_config() - Lê configurações");
    println!("  - PluginContext::enable_capability() - Habilita capabilities");
    println!("  - PluginContext::plugin_name() - Nome do plugin");
    println!();
    println!("Eventos manipulados:");
    println!("  - on_input() - Input do usuário");
    println!("  - on_tab_created() - Nova aba criada");
    println!("  - on_tab_closed() - Aba fechada");
    println!("  - on_theme_changed() - Tema alterado");
    println!();
    println!("Comandos disponíveis quando ativo:");
    println!("  /hello-stats - Mostra estatísticas de saudações");
    println!("  /hello-help  - Mostra ajuda do plugin");
    println!("  hello/ola    - Dispara uma saudação");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plugin_info() {
        let plugin = HelloPlugin::new();
        let info = plugin.info();

        assert_eq!(info.name, "hello-world");
        assert!(!info.version.is_empty());
        assert_eq!(info.author, "Codeterm Team");
    }

    #[test]
    fn test_plugin_state_initial() {
        let plugin = HelloPlugin::new();
        assert_eq!(plugin.state(), PluginState::Created);
    }

    #[test]
    fn test_default_config() {
        let plugin = HelloPlugin::new();
        let config = plugin.default_config();

        assert!(config.contains_key("greeting"));
        assert!(config.contains_key("auto_greet"));
    }

    #[test]
    fn test_apply_config() {
        let mut plugin = HelloPlugin::new();
        let mut config = HashMap::new();
        config.insert("greeting".to_string(), "Custom Greeting!".to_string());

        plugin.apply_config(&config);

        assert_eq!(plugin.config_greeting, "Custom Greeting!");
    }
}
