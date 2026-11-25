//! AI Command Suggestions UI
//!
//! Este módulo fornece componentes de UI para sugestões de comandos baseadas em AI,
//! permitindo que usuários recebam sugestões contextuais enquanto digitam.

use crate::ai::provider::TerminalContext;
use crate::ai::{
    AiConfig, AiResult, CommandSuggester, CommandSuggestion, SuggesterStats, SuggestionHistory,
};
use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Estado da UI de sugestões
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionUIState {
    /// Oculto, não mostrando sugestões
    Hidden,
    /// Mostrando sugestão pendente
    Showing,
    /// Aguardando resposta da AI
    Loading,
    /// Erro ao obter sugestão
    Error,
}

/// Componente de UI para sugestões de comandos AI
pub struct CommandSuggestionsUI {
    /// Suggester de comandos
    suggester: Option<Arc<CommandSuggester>>,
    /// Estado atual
    state: Arc<RwLock<SuggestionUIState>>,
    /// Sugestão atual
    current_suggestion: Arc<RwLock<Option<CommandSuggestion>>>,
    /// Input atual do usuário
    current_input: Arc<RwLock<String>>,
    /// Contexto terminal
    context: Arc<RwLock<TerminalContext>>,
    /// Canal para requests assíncronos
    request_tx: Option<mpsc::Sender<SuggestionRequest>>,
    /// Histórico de sugestões mostradas
    shown_suggestions: Arc<RwLock<VecDeque<ShownSuggestion>>>,
    /// Configuração
    config: SuggestionUIConfig,
}

#[derive(Debug, Clone)]
struct ShownSuggestion {
    input: String,
    suggestion: CommandSuggestion,
    accepted: bool,
}

/// Configuração da UI de sugestões
#[derive(Debug, Clone)]
pub struct SuggestionUIConfig {
    /// Habilitar sugestões automáticas
    pub auto_suggest: bool,
    /// Delay antes de solicitar sugestão (ms)
    pub delay_ms: u64,
    /// Número máximo de sugestões no histórico
    pub max_history: usize,
    /// Mostrar confiança da sugestão
    pub show_confidence: bool,
    /// Mostrar alternativas
    pub show_alternatives: bool,
    /// Tecla para aceitar sugestão
    pub accept_key: SuggestionAcceptKey,
    /// Tecla para rejeitar/recusar
    pub dismiss_key: SuggestionDismissKey,
}

impl Default for SuggestionUIConfig {
    fn default() -> Self {
        Self {
            auto_suggest: true,
            delay_ms: 500,
            max_history: 50,
            show_confidence: true,
            show_alternatives: true,
            accept_key: SuggestionAcceptKey::Tab,
            dismiss_key: SuggestionDismissKey::Escape,
        }
    }
}

/// Tecla para aceitar sugestão
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionAcceptKey {
    Tab,
    RightArrow,
    CtrlSpace,
    Custom(char),
}

/// Tecla para rejeitar sugestão
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionDismissKey {
    Escape,
    LeftArrow,
    CtrlC,
    Custom(char),
}

/// Request de sugestão
#[derive(Debug)]
struct SuggestionRequest {
    input: String,
    context: TerminalContext,
}

/// Resultado da sugestão
#[derive(Debug)]
pub struct SuggestionResult {
    pub input: String,
    pub suggestion: Option<CommandSuggestion>,
    pub error: Option<String>,
}

impl CommandSuggestionsUI {
    /// Cria uma nova UI de sugestões
    pub fn new() -> Self {
        Self {
            suggester: None,
            state: Arc::new(RwLock::new(SuggestionUIState::Hidden)),
            current_suggestion: Arc::new(RwLock::new(None)),
            current_input: Arc::new(RwLock::new(String::new())),
            context: Arc::new(RwLock::new(TerminalContext::default())),
            request_tx: None,
            shown_suggestions: Arc::new(RwLock::new(VecDeque::with_capacity(50))),
            config: SuggestionUIConfig::default(),
        }
    }

    /// Cria com configuração personalizada
    pub fn with_config(mut self, config: SuggestionUIConfig) -> Self {
        self.config = config;
        self
    }

    /// Inicializa o suggester com configuração
    pub fn initialize(&mut self, config: AiConfig) -> AiResult<()> {
        let suggester = Arc::new(CommandSuggester::new(config)?);
        self.suggester = Some(Arc::clone(&suggester));

        // Inicializa canal de comunicação
        let (tx, mut rx) = mpsc::channel::<SuggestionRequest>(10);
        self.request_tx = Some(tx);

        // Spawn task para processar requests
        let suggester_clone = suggester;
        let state_clone = self.state.clone();
        let suggestion_clone = self.current_suggestion.clone();

        tokio::spawn(async move {
            while let Some(request) = rx.recv().await {
                *state_clone.write() = SuggestionUIState::Loading;

                match suggester_clone
                    .suggest_with_context(&request.input, &request.context)
                    .await
                {
                    Ok(suggestion) => {
                        *suggestion_clone.write() = Some(suggestion);
                        *state_clone.write() = SuggestionUIState::Showing;
                        debug!("Suggestion received for: {}", request.input);
                    }
                    Err(e) => {
                        *suggestion_clone.write() = None;
                        *state_clone.write() = SuggestionUIState::Error;
                        warn!("Failed to get suggestion: {}", e);
                    }
                }
            }
        });

        info!("Command suggestions UI initialized");
        Ok(())
    }

    /// Define o contexto terminal
    pub fn set_context(&self, context: TerminalContext) {
        *self.context.write() = context;
    }

    /// Atualiza o diretório atual
    pub fn update_directory(&self, dir: impl Into<String>) {
        let dir_str = dir.into();
        self.context.write().current_directory = Some(dir_str.clone());
        if let Some(ref suggester) = self.suggester {
            suggester.update_directory(dir_str);
        }
    }

    /// Atualiza o tipo de shell
    pub fn update_shell(&self, shell: impl Into<String>) {
        let shell_str = shell.into();
        self.context.write().shell_type = Some(shell_str.clone());
        if let Some(ref suggester) = self.suggester {
            suggester.update_shell(shell_str);
        }
    }

    /// Adiciona comando ao histórico
    pub fn add_command_to_history(&self, command: impl Into<String>) {
        if let Some(ref suggester) = self.suggester {
            suggester.add_command_to_history(command);
        }
    }

    /// Atualiza o último output
    pub fn update_last_output(&self, output: impl Into<String>) {
        let output_str = output.into();
        self.context.write().last_output = Some(output_str.clone());
        if let Some(ref suggester) = self.suggester {
            suggester.update_last_output(output_str);
        }
    }

    /// Processa input do usuário
    pub fn on_input_changed(&self, input: &str) {
        *self.current_input.write() = input.to_string();

        if !self.config.auto_suggest || input.is_empty() {
            self.hide();
            return;
        }

        // Verifica se há suggester configurado
        if self.suggester.is_none() {
            return;
        }

        // Envia request assíncrono
        if let Some(ref tx) = self.request_tx {
            let request = SuggestionRequest {
                input: input.to_string(),
                context: self.context.read().clone(),
            };

            let tx_clone = tx.clone();
            // respeita o atraso configurado (antes era sempre 500 ms)
            let delay = tokio::time::Duration::from_millis(self.config.delay_ms);
            tokio::spawn(async move {
                // Delay antes de enviar
                tokio::time::sleep(delay).await;
                let _ = tx_clone.send(request).await;
            });
        }
    }

    /// Aceita a sugestão atual
    pub fn accept_suggestion(&self) -> Option<String> {
        let suggestion = self.current_suggestion.read().clone()?;
        let input = self.current_input.read().clone();

        // Registra no histórico
        self.shown_suggestions.write().push_back(ShownSuggestion {
            input: input.clone(),
            suggestion: suggestion.clone(),
            accepted: true,
        });

        // Limita histórico
        while self.shown_suggestions.read().len() > self.config.max_history {
            self.shown_suggestions.write().pop_front();
        }

        // Notifica o suggester
        if let Some(ref suggester) = self.suggester {
            suggester.mark_suggestion_accepted(&input);
        }

        info!("Suggestion accepted: {}", suggestion.command);
        self.hide();

        Some(suggestion.command)
    }

    /// Rejeita a sugestão atual
    pub fn dismiss_suggestion(&self) {
        if let Some(suggestion) = self.current_suggestion.read().clone() {
            let input = self.current_input.read().clone();

            self.shown_suggestions.write().push_back(ShownSuggestion {
                input,
                suggestion,
                accepted: false,
            });

            info!("Suggestion dismissed");
        }

        self.hide();
    }

    /// Oculta a sugestão
    pub fn hide(&self) {
        *self.state.write() = SuggestionUIState::Hidden;
        *self.current_suggestion.write() = None;
    }

    /// Retorna a sugestão atual
    pub fn current_suggestion(&self) -> Option<CommandSuggestion> {
        self.current_suggestion.read().clone()
    }

    /// Retorna o estado atual
    pub fn state(&self) -> SuggestionUIState {
        *self.state.read()
    }

    /// Verifica se está mostrando sugestão
    pub fn is_showing(&self) -> bool {
        matches!(self.state(), SuggestionUIState::Showing)
    }

    /// Verifica se está carregando
    pub fn is_loading(&self) -> bool {
        matches!(self.state(), SuggestionUIState::Loading)
    }

    /// Retorna o texto da sugestão formatado para display
    pub fn format_suggestion(&self) -> Option<String> {
        let suggestion = self.current_suggestion.read().clone()?;
        let input = self.current_input.read();

        // Destaca a parte sugerida além do input
        if suggestion.command.starts_with(&*input) {
            let suffix = &suggestion.command[input.len()..];
            Some(format!("{}{}", input, suffix))
        } else {
            Some(suggestion.command.clone())
        }
    }

    /// Retorna estatísticas do suggester
    pub fn stats(&self) -> Option<SuggesterStats> {
        self.suggester.as_ref().map(|s| s.get_stats())
    }

    /// Retorna histórico de sugestões
    pub fn suggestion_history(&self) -> Vec<SuggestionHistory> {
        self.suggester
            .as_ref()
            .map(|s| s.get_history())
            .unwrap_or_default()
    }

    /// Habilita sugestões
    pub fn enable(&self) {
        if let Some(ref suggester) = self.suggester {
            suggester.enable();
        }
    }

    /// Desabilita sugestões
    pub fn disable(&self) {
        if let Some(ref suggester) = self.suggester {
            suggester.disable();
        }
        self.hide();
    }

    /// Verifica se sugestões estão habilitadas
    pub fn is_enabled(&self) -> bool {
        self.suggester
            .as_ref()
            .map(|s| s.is_enabled())
            .unwrap_or(false)
    }

    /// Limpa o histórico
    pub fn clear_history(&self) {
        if let Some(ref suggester) = self.suggester {
            suggester.clear_history();
        }
        self.shown_suggestions.write().clear();
    }

    /// Manipula tecla de input
    /// Retorna true se a tecla foi consumida
    pub fn handle_key(&self, key: SuggestionKey) -> bool {
        match key {
            SuggestionKey::Accept => {
                if self.is_showing() {
                    self.accept_suggestion();
                    true
                } else {
                    false
                }
            }
            SuggestionKey::Dismiss => {
                if self.is_showing() {
                    self.dismiss_suggestion();
                    true
                } else {
                    false
                }
            }
            SuggestionKey::ShowAlternatives => {
                // TODO: Implementar mostrar alternativas
                false
            }
            _ => false,
        }
    }

    /// Retorna taxa de aceitação
    pub fn acceptance_rate(&self) -> f32 {
        let history = self.shown_suggestions.read();
        if history.is_empty() {
            return 0.0;
        }

        let accepted = history.iter().filter(|s| s.accepted).count();
        accepted as f32 / history.len() as f32
    }

    /// Retorna resumo das sugestões já exibidas.
    pub fn shown_history(&self) -> Vec<ShownSuggestionSummary> {
        self.shown_suggestions
            .read()
            .iter()
            .map(|entry| ShownSuggestionSummary {
                input: entry.input.clone(),
                command: entry.suggestion.command.clone(),
                accepted: entry.accepted,
            })
            .collect()
    }
}

/// Resumo público de uma sugestão exibida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShownSuggestionSummary {
    pub input: String,
    pub command: String,
    pub accepted: bool,
}

impl Default for CommandSuggestionsUI {
    fn default() -> Self {
        Self::new()
    }
}

/// Teclas específicas para sugestões
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKey {
    /// Aceitar sugestão
    Accept,
    /// Rejeitar sugestão
    Dismiss,
    /// Mostrar alternativas
    ShowAlternatives,
    /// Solicitar sugestão manual
    Request,
}

/// Renderizador de sugestões
///
/// Esta trait deve ser implementada pelo renderizador da aplicação
/// para desenhar as sugestões na tela.
pub trait SuggestionRenderer {
    /// Renderiza a sugestão inline
    fn render_inline_suggestion(
        &mut self,
        input: &str,
        suggestion: &str,
        confidence: f32,
        position: SuggestionRenderPosition,
    );

    /// Renderiza tooltip com explicação
    fn render_suggestion_tooltip(
        &mut self,
        explanation: &str,
        alternatives: &[String],
        position: SuggestionRenderPosition,
    );

    /// Renderiza indicador de loading
    fn render_loading_indicator(&mut self, position: SuggestionRenderPosition);
}

/// Posição para renderização
#[derive(Debug, Clone, Copy)]
pub struct SuggestionRenderPosition {
    pub x: f32,
    pub y: f32,
    pub line_height: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> AiConfig {
        AiConfig::openai("test-api-key")
    }

    #[test]
    fn test_suggestion_ui_creation() {
        let ui = CommandSuggestionsUI::new();
        assert!(!ui.is_showing());
        assert!(!ui.is_loading());
        assert_eq!(ui.state(), SuggestionUIState::Hidden);
    }

    #[test]
    fn test_suggestion_ui_with_config() {
        let config = SuggestionUIConfig {
            auto_suggest: false,
            delay_ms: 1000,
            ..Default::default()
        };

        let ui = CommandSuggestionsUI::new().with_config(config);
        assert!(!ui.is_enabled());
    }

    #[tokio::test]
    async fn test_suggestion_ui_initialize() {
        let mut ui = CommandSuggestionsUI::new();
        let config = create_test_config();

        assert!(ui.initialize(config).is_ok());
        assert!(ui.suggester.is_some());
    }

    #[test]
    fn test_suggestion_ui_context() {
        let ui = CommandSuggestionsUI::new();

        ui.update_directory("/home/user");
        ui.update_shell("bash");
        ui.add_command_to_history("ls -la");
        ui.update_last_output("output");

        let context = ui.context.read();
        assert_eq!(context.current_directory, Some("/home/user".to_string()));
        assert_eq!(context.shell_type, Some("bash".to_string()));
        assert_eq!(context.last_output, Some("output".to_string()));
    }

    #[test]
    fn test_suggestion_ui_hide_show() {
        let ui = CommandSuggestionsUI::new();

        // Inicialmente hidden
        assert_eq!(ui.state(), SuggestionUIState::Hidden);
        assert!(!ui.is_showing());

        // Hide quando já está hidden
        ui.hide();
        assert_eq!(ui.state(), SuggestionUIState::Hidden);
    }

    #[test]
    fn test_suggestion_ui_default_config() {
        let config = SuggestionUIConfig::default();

        assert!(config.auto_suggest);
        assert_eq!(config.delay_ms, 500);
        assert!(config.show_confidence);
        assert!(config.show_alternatives);
    }

    #[test]
    fn test_suggestion_accept_key_variants() {
        assert_eq!(SuggestionAcceptKey::Tab, SuggestionAcceptKey::Tab);
        assert_ne!(SuggestionAcceptKey::Tab, SuggestionAcceptKey::RightArrow);

        let custom = SuggestionAcceptKey::Custom('a');
        assert!(matches!(custom, SuggestionAcceptKey::Custom('a')));
    }

    #[test]
    fn test_suggestion_key_variants() {
        assert_eq!(SuggestionKey::Accept, SuggestionKey::Accept);
        assert_ne!(SuggestionKey::Accept, SuggestionKey::Dismiss);
    }

    #[test]
    fn test_suggestion_render_position() {
        let pos = SuggestionRenderPosition {
            x: 100.0,
            y: 200.0,
            line_height: 16.0,
        };

        assert_eq!(pos.x, 100.0);
        assert_eq!(pos.y, 200.0);
        assert_eq!(pos.line_height, 16.0);
    }

    #[test]
    fn test_suggestion_ui_state_variants() {
        assert_eq!(SuggestionUIState::Hidden, SuggestionUIState::Hidden);
        assert_ne!(SuggestionUIState::Hidden, SuggestionUIState::Showing);
        assert_ne!(SuggestionUIState::Loading, SuggestionUIState::Error);
    }

    #[test]
    fn test_acceptance_rate_empty() {
        let ui = CommandSuggestionsUI::new();
        assert_eq!(ui.acceptance_rate(), 0.0);
    }

    #[test]
    fn test_handle_key_not_consumed() {
        let ui = CommandSuggestionsUI::new();

        // Sem sugestão mostrada, não deve consumir
        assert!(!ui.handle_key(SuggestionKey::Accept));
        assert!(!ui.handle_key(SuggestionKey::Dismiss));
    }

    use crate::ai::test_support::FakeOpenAi;

    const JSON: &str = r#"{"command":"ls -la","explanation":"lista","confidence":0.9}"#;

    async fn ready_ui(server: &FakeOpenAi, delay_ms: u64) -> CommandSuggestionsUI {
        let mut ui = CommandSuggestionsUI::new().with_config(SuggestionUIConfig {
            delay_ms,
            max_history: 2,
            ..Default::default()
        });
        ui.initialize(AiConfig::openai("k").with_base_url(&server.base_url))
            .unwrap();
        ui
    }

    async fn wait_state(ui: &CommandSuggestionsUI, want: SuggestionUIState) {
        for _ in 0..500 {
            if ui.state() == want {
                return;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
        }
        panic!("estado {:?} não chegou (atual {:?})", want, ui.state());
    }

    #[tokio::test]
    async fn test_input_fetches_suggestion_and_accept() {
        let server = FakeOpenAi::replying(JSON).await;
        let ui = ready_ui(&server, 0).await;
        ui.update_directory("/srv");
        ui.update_shell("zsh");
        ui.add_command_to_history("make");
        ui.update_last_output("ok");
        assert!(ui.is_enabled());

        let started = std::time::Instant::now();
        ui.on_input_changed("ls");
        wait_state(&ui, SuggestionUIState::Showing).await;
        // Regressão: o atraso configurado (0 ms) era ignorado (sempre 500 ms)
        assert!(started.elapsed() < std::time::Duration::from_millis(450));
        assert_eq!(ui.current_suggestion().unwrap().command, "ls -la");
        assert_eq!(ui.format_suggestion().as_deref(), Some("ls -la"));
        assert_eq!(ui.stats().unwrap().total_suggestions, 1);
        assert_eq!(ui.suggestion_history().len(), 1);

        assert!(ui.handle_key(SuggestionKey::Accept));
        assert!(!ui.is_showing());
        assert_eq!(ui.shown_history()[0].command, "ls -la");
        assert!(ui.shown_history()[0].accepted);
        assert_eq!(ui.acceptance_rate(), 1.0);
        assert_eq!(ui.stats().unwrap().accepted_suggestions, 1);
        // sem sugestão aberta as teclas não são consumidas
        assert!(!ui.handle_key(SuggestionKey::Accept));
        assert!(!ui.handle_key(SuggestionKey::Dismiss));
        assert!(!ui.handle_key(SuggestionKey::ShowAlternatives));
        assert!(ui.accept_suggestion().is_none());
    }

    #[tokio::test]
    async fn test_dismiss_history_limit_and_clear() {
        let server = FakeOpenAi::replying(JSON).await;
        let ui = ready_ui(&server, 0).await;
        for input in ["a", "b", "c"] {
            ui.on_input_changed(input);
            wait_state(&ui, SuggestionUIState::Showing).await;
            ui.accept_suggestion();
        }
        // max_history = 2
        let h = ui.shown_history();
        assert_eq!(
            h.iter().map(|s| s.input.as_str()).collect::<Vec<_>>(),
            vec!["b", "c"]
        );

        ui.on_input_changed("d");
        wait_state(&ui, SuggestionUIState::Showing).await;
        // a sugestão que não começa com o input é mostrada inteira
        assert_eq!(ui.format_suggestion().as_deref(), Some("ls -la"));
        assert!(ui.handle_key(SuggestionKey::Dismiss));
        assert!(!ui.shown_history().last().unwrap().accepted);
        assert!(ui.acceptance_rate() < 1.0);
        ui.dismiss_suggestion();

        ui.clear_history();
        assert!(ui.shown_history().is_empty());
        assert!(ui.suggestion_history().is_empty());
        assert_eq!(ui.acceptance_rate(), 0.0);

        // input vazio ou auto_suggest desligado escondem sem pedir nada
        let before = server.bodies().len();
        ui.on_input_changed("");
        assert_eq!(ui.state(), SuggestionUIState::Hidden);
        ui.disable();
        assert!(!ui.is_enabled());
        ui.enable();
        assert!(ui.is_enabled());
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
        assert_eq!(server.bodies().len(), before);
    }

    #[tokio::test]
    async fn test_api_failure_sets_error_state() {
        let server = FakeOpenAi::start(
            400,
            r#"{"error":{"message":"ruim","type":"x","param":null,"code":null}}"#.into(),
        )
        .await;
        let ui = ready_ui(&server, 0).await;
        ui.on_input_changed("ls");
        wait_state(&ui, SuggestionUIState::Error).await;
        assert!(ui.current_suggestion().is_none());
        assert!(ui.format_suggestion().is_none());
    }

    #[test]
    fn test_without_suggester_everything_is_inert() {
        let ui = CommandSuggestionsUI::default();
        ui.set_context(TerminalContext {
            shell_type: Some("bash".into()),
            ..Default::default()
        });
        assert_eq!(ui.context.read().shell_type.as_deref(), Some("bash"));
        ui.add_command_to_history("x");
        ui.on_input_changed("x");
        assert_eq!(ui.state(), SuggestionUIState::Hidden);
        assert!(ui.stats().is_none());
        assert!(ui.suggestion_history().is_empty());
        ui.enable();
        ui.disable();
        ui.clear_history();
        assert!(ui.accept_suggestion().is_none());
    }
}
