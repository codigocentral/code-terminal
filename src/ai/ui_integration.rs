//! AI UI Integration Module (US-021)
//!
//! Este módulo contém stubs para integração dos hooks de AI à UI principal.
//! Quando ativado, permite sugestões de comandos contextuais.

use crate::ai::AiConfig;
use crate::ui::ai_suggestions::CommandSuggestionsUI;

/// Estrutura de integração AI-UI
///
/// Conecta o sistema de sugestões AI à interface do usuário.
pub struct AiUiIntegration {
    suggestions_ui: Option<CommandSuggestionsUI>,
    enabled: bool,
}

impl AiUiIntegration {
    /// Cria nova integração (desabilitada por padrão)
    pub fn new() -> Self {
        Self {
            suggestions_ui: None,
            enabled: false,
        }
    }

    /// Inicializa com configuração
    pub fn initialize(&mut self, config: AiConfig) -> anyhow::Result<()> {
        let mut suggestions_ui = CommandSuggestionsUI::new();
        suggestions_ui.initialize(config)?;
        self.suggestions_ui = Some(suggestions_ui);
        self.enabled = true;
        Ok(())
    }

    /// Retorna referência à UI de sugestões
    pub fn suggestions_ui(&self) -> Option<&CommandSuggestionsUI> {
        self.suggestions_ui.as_ref()
    }

    /// Retorna referência mutável à UI de sugestões
    pub fn suggestions_ui_mut(&mut self) -> Option<&mut CommandSuggestionsUI> {
        self.suggestions_ui.as_mut()
    }

    /// Verifica se está habilitado
    pub fn is_enabled(&self) -> bool {
        self.enabled && self.suggestions_ui.is_some()
    }

    /// Habilita sugestões
    pub fn enable(&mut self) {
        self.enabled = true;
        if let Some(ref ui) = self.suggestions_ui {
            ui.enable();
        }
    }

    /// Desabilita sugestões
    pub fn disable(&mut self) {
        self.enabled = false;
        if let Some(ref ui) = self.suggestions_ui {
            ui.disable();
        }
    }
}

impl Default for AiUiIntegration {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_ui_integration_default() {
        let integration = AiUiIntegration::new();
        assert!(!integration.is_enabled());
        assert!(integration.suggestions_ui().is_none());
    }

    #[test]
    fn test_enable_disable() {
        let mut integration = AiUiIntegration::new();
        assert!(!integration.is_enabled());

        // Sem UI inicializada, enable não muda o estado efetivo
        integration.enable();
        assert!(!integration.is_enabled());
    }
}
