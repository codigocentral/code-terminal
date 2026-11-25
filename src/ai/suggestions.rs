use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::config::AiConfig;
use super::error::{AiError, AiResult};
use super::openai::OpenAIProvider;
use super::provider::{AIProvider, CommandSuggestion, TerminalContext};

const MAX_HISTORY_SIZE: usize = 100;
const MAX_SUGGESTION_CACHE: usize = 50;

#[derive(Debug, Clone)]
pub struct SuggestionHistory {
    pub input: String,
    pub suggestion: CommandSuggestion,
    pub accepted: bool,
    pub timestamp: std::time::Instant,
}

#[derive(Debug, Clone)]
struct CachedSuggestion {
    input_hash: u64,
    suggestion: CommandSuggestion,
    context_hash: u64,
}

pub struct CommandSuggester {
    provider: OpenAIProvider,
    context: Arc<RwLock<TerminalContext>>,
    history: Arc<RwLock<VecDeque<SuggestionHistory>>>,
    cache: Arc<RwLock<VecDeque<CachedSuggestion>>>,
    config: AiConfig,
    enabled: Arc<RwLock<bool>>,
}

impl CommandSuggester {
    pub fn new(config: AiConfig) -> AiResult<Self> {
        let provider = OpenAIProvider::new(config.clone())?;

        Ok(Self {
            provider,
            context: Arc::new(RwLock::new(TerminalContext::default())),
            history: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_HISTORY_SIZE))),
            cache: Arc::new(RwLock::new(VecDeque::with_capacity(MAX_SUGGESTION_CACHE))),
            config,
            enabled: Arc::new(RwLock::new(true)),
        })
    }

    pub fn enable(&self) {
        *self.enabled.write() = true;
        info!("Command suggester enabled");
    }

    pub fn disable(&self) {
        *self.enabled.write() = false;
        info!("Command suggester disabled");
    }

    pub fn is_enabled(&self) -> bool {
        *self.enabled.read()
    }

    pub fn update_directory(&self, dir: impl Into<String>) {
        self.context.write().current_directory = Some(dir.into());
    }

    pub fn update_shell(&self, shell: impl Into<String>) {
        self.context.write().shell_type = Some(shell.into());
    }

    pub fn add_command_to_history(&self, command: impl Into<String>) {
        self.context.write().add_command(command);
    }

    pub fn update_last_output(&self, output: impl Into<String>) {
        self.context.write().last_output = Some(output.into());
    }

    pub fn get_context(&self) -> TerminalContext {
        self.context.read().clone()
    }

    pub fn set_context(&self, context: TerminalContext) {
        *self.context.write() = context;
    }

    fn compute_hash<T: std::hash::Hash>(value: &T) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    fn compute_context_hash(context: &TerminalContext) -> u64 {
        Self::compute_hash(&(&context.current_directory, &context.shell_type))
    }

    fn check_cache(&self, input: &str, context: &TerminalContext) -> Option<CommandSuggestion> {
        let input_hash = Self::compute_hash(&input);
        let context_hash = Self::compute_context_hash(context);
        let cache = self.cache.read();

        cache
            .iter()
            .find(|c| c.input_hash == input_hash && c.context_hash == context_hash)
            .map(|c| c.suggestion.clone())
    }

    fn add_to_cache(&self, input: &str, context: &TerminalContext, suggestion: CommandSuggestion) {
        let input_hash = Self::compute_hash(&input);
        let context_hash = Self::compute_context_hash(context);
        let mut cache = self.cache.write();

        if cache.len() >= MAX_SUGGESTION_CACHE {
            cache.pop_front();
        }

        cache.push_back(CachedSuggestion {
            input_hash,
            suggestion,
            context_hash,
        });
    }

    fn validate_input(&self, input: &str) -> AiResult<()> {
        if input.trim().is_empty() {
            return Err(AiError::SuggestionFailed(
                "Input cannot be empty".to_string(),
            ));
        }

        if input.len() > 1000 {
            return Err(AiError::ContextTooLong("Input too long".to_string()));
        }

        Ok(())
    }

    fn apply_privacy_filters(&self, context: &mut TerminalContext) {
        if !self.config.privacy.send_shell_history {
            context.recent_commands.clear();
        }

        if !self.config.privacy.send_file_paths {
            context.current_directory = None;
        }

        if !self.config.privacy.send_environment_vars {
            context.environment.clear();
        }
    }

    pub async fn suggest(&self, input: &str) -> AiResult<CommandSuggestion> {
        if !self.is_enabled() {
            return Err(AiError::ConfigError(
                "Command suggester is disabled".to_string(),
            ));
        }

        if !self.provider.is_configured() {
            return Err(AiError::ConfigError(
                "AI provider is not configured".to_string(),
            ));
        }

        self.validate_input(input)?;

        let mut context = self.context.read().clone();

        // Filtra antes de consultar o cache: a entrada é gravada com o
        // contexto filtrado, então a busca precisa usar o mesmo contexto
        // (antes, com diretório definido e send_file_paths=false, o cache
        // nunca acertava)
        self.apply_privacy_filters(&mut context);

        if let Some(cached) = self.check_cache(input, &context) {
            debug!("Returning cached suggestion for: {}", input);
            return Ok(cached);
        }

        if self.config.privacy.require_confirmation {
            warn!("Privacy confirmation required for command suggestion");
        }

        debug!("Requesting suggestion for: {}", input);

        let suggestion = self.provider.suggest_command(&context, input).await?;

        self.add_to_cache(input, &context, suggestion.clone());

        self.add_to_history(input, suggestion.clone());

        info!(
            "Generated suggestion: {} (confidence: {:.2})",
            suggestion.command, suggestion.confidence
        );

        Ok(suggestion)
    }

    pub async fn suggest_with_context(
        &self,
        input: &str,
        additional_context: &TerminalContext,
    ) -> AiResult<CommandSuggestion> {
        let mut context = self.context.read().clone();

        if let Some(ref dir) = additional_context.current_directory {
            context.current_directory = Some(dir.clone());
        }
        if let Some(ref shell) = additional_context.shell_type {
            context.shell_type = Some(shell.clone());
        }
        if !additional_context.recent_commands.is_empty() {
            context
                .recent_commands
                .extend(additional_context.recent_commands.clone());
        }

        let old_context = self.context.read().clone();
        *self.context.write() = context;

        let result = self.suggest(input).await;

        *self.context.write() = old_context;

        result
    }

    fn add_to_history(&self, input: &str, suggestion: CommandSuggestion) {
        let mut history = self.history.write();

        if history.len() >= MAX_HISTORY_SIZE {
            history.pop_front();
        }

        history.push_back(SuggestionHistory {
            input: input.to_string(),
            suggestion,
            accepted: false,
            timestamp: std::time::Instant::now(),
        });
    }

    pub fn mark_suggestion_accepted(&self, input: &str) {
        let mut history = self.history.write();

        if let Some(entry) = history.iter_mut().rev().find(|h| h.input == input) {
            entry.accepted = true;
            info!("Suggestion accepted: {}", entry.suggestion.command);
        }
    }

    pub fn get_history(&self) -> Vec<SuggestionHistory> {
        self.history.read().iter().cloned().collect()
    }

    pub fn get_recent_suggestions(&self, count: usize) -> Vec<SuggestionHistory> {
        self.history
            .read()
            .iter()
            .rev()
            .take(count)
            .cloned()
            .collect()
    }

    pub fn clear_history(&self) {
        self.history.write().clear();
        info!("Suggestion history cleared");
    }

    pub fn clear_cache(&self) {
        self.cache.write().clear();
        info!("Suggestion cache cleared");
    }

    pub fn get_stats(&self) -> SuggesterStats {
        let history = self.history.read();
        let total = history.len();
        let accepted = history.iter().filter(|h| h.accepted).count();

        SuggesterStats {
            total_suggestions: total,
            accepted_suggestions: accepted,
            acceptance_rate: if total > 0 {
                accepted as f32 / total as f32
            } else {
                0.0
            },
            cache_size: self.cache.read().len(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SuggesterStats {
    pub total_suggestions: usize,
    pub accepted_suggestions: usize,
    pub acceptance_rate: f32,
    pub cache_size: usize,
}

pub struct SuggestionBuilder {
    input: String,
    context: TerminalContext,
    config: Option<AiConfig>,
}

impl SuggestionBuilder {
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
            context: TerminalContext::default(),
            config: None,
        }
    }

    pub fn with_directory(mut self, dir: impl Into<String>) -> Self {
        self.context.current_directory = Some(dir.into());
        self
    }

    pub fn with_shell(mut self, shell: impl Into<String>) -> Self {
        self.context.shell_type = Some(shell.into());
        self
    }

    pub fn with_recent_commands(mut self, commands: Vec<String>) -> Self {
        self.context.recent_commands = commands;
        self
    }

    pub fn with_config(mut self, config: AiConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn with_last_output(mut self, output: impl Into<String>) -> Self {
        self.context.last_output = Some(output.into());
        self
    }

    pub fn build(self) -> AiResult<CommandSuggester> {
        let config = self.config.unwrap_or_default();
        let suggester = CommandSuggester::new(config)?;
        let mut context = self.context;
        if !self.input.trim().is_empty() {
            context.recent_commands.push(self.input);
        }
        suggester.set_context(context);
        Ok(suggester)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> AiConfig {
        AiConfig::openai("test-api-key")
    }

    #[test]
    fn test_suggester_creation() {
        let config = create_test_config();
        let suggester = CommandSuggester::new(config);
        assert!(suggester.is_ok());
    }

    #[test]
    fn test_enable_disable() {
        let config = create_test_config();
        let suggester = CommandSuggester::new(config).unwrap();

        assert!(suggester.is_enabled());

        suggester.disable();
        assert!(!suggester.is_enabled());

        suggester.enable();
        assert!(suggester.is_enabled());
    }

    #[test]
    fn test_context_update() {
        let config = create_test_config();
        let suggester = CommandSuggester::new(config).unwrap();

        suggester.update_directory("/home/user");
        suggester.update_shell("bash");

        let context = suggester.get_context();
        assert_eq!(context.current_directory, Some("/home/user".to_string()));
        assert_eq!(context.shell_type, Some("bash".to_string()));
    }

    #[test]
    fn test_command_history() {
        let config = create_test_config();
        let suggester = CommandSuggester::new(config).unwrap();

        suggester.add_command_to_history("ls -la");
        suggester.add_command_to_history("cd /tmp");

        let context = suggester.get_context();
        assert_eq!(context.recent_commands.len(), 2);
    }

    #[test]
    fn test_history_management() {
        let config = create_test_config();
        let suggester = CommandSuggester::new(config).unwrap();

        suggester.clear_history();
        let history = suggester.get_history();
        assert!(history.is_empty());
    }

    #[test]
    fn test_stats() {
        let config = create_test_config();
        let suggester = CommandSuggester::new(config).unwrap();

        let stats = suggester.get_stats();
        assert_eq!(stats.total_suggestions, 0);
        assert_eq!(stats.acceptance_rate, 0.0);
    }

    #[test]
    fn test_suggestion_builder() {
        let config = create_test_config();
        let suggester = SuggestionBuilder::new("list files")
            .with_directory("/home/user")
            .with_shell("bash")
            .with_config(config)
            .build();

        assert!(suggester.is_ok());

        let suggester = suggester.unwrap();
        let context = suggester.get_context();
        assert_eq!(context.current_directory, Some("/home/user".to_string()));
    }

    #[test]
    fn test_validate_input() {
        let config = create_test_config();
        let suggester = CommandSuggester::new(config).unwrap();

        assert!(suggester.validate_input("ls").is_ok());
        assert!(suggester.validate_input("").is_err());
        assert!(suggester.validate_input(&"x".repeat(1001)).is_err());
    }

    #[test]
    fn test_privacy_filters() {
        let mut config = AiConfig::openai("test-api-key");
        config.privacy.send_shell_history = false;
        config.privacy.send_file_paths = false;

        let suggester = CommandSuggester::new(config).unwrap();

        let mut context = TerminalContext {
            current_directory: Some("/secret/path".to_string()),
            recent_commands: vec!["secret command".to_string()],
            ..Default::default()
        };

        suggester.apply_privacy_filters(&mut context);

        assert!(context.current_directory.is_none());
        assert!(context.recent_commands.is_empty());
    }

    use crate::ai::test_support::FakeOpenAi;

    async fn suggester_for(content: &str) -> (CommandSuggester, FakeOpenAi) {
        let server = FakeOpenAi::replying(content).await;
        let config = AiConfig::openai("k").with_base_url(&server.base_url);
        (CommandSuggester::new(config).unwrap(), server)
    }

    const JSON: &str = r#"{"command":"ls -la","explanation":"lista","confidence":0.9}"#;

    #[tokio::test]
    async fn test_suggest_calls_api_records_history_and_caches() {
        let (s, server) = suggester_for(JSON).await;
        s.update_directory("/srv/app");
        s.update_shell("bash");

        let first = s.suggest("listar").await.unwrap();
        assert_eq!(first.command, "ls -la");
        // Regressão: com diretório definido o cache nunca acertava
        let second = s.suggest("listar").await.unwrap();
        assert_eq!(second.command, "ls -la");
        assert_eq!(server.bodies().len(), 1);
        assert_eq!(s.get_stats().cache_size, 1);
        // o histórico registra só a chamada que foi à API
        assert_eq!(s.get_history().len(), 1);

        // outro input vai à API de novo
        s.suggest("outra coisa").await.unwrap();
        assert_eq!(server.bodies().len(), 2);
        assert_eq!(s.get_recent_suggestions(1)[0].input, "outra coisa");
        assert_eq!(s.get_recent_suggestions(10).len(), 2);

        s.clear_cache();
        s.suggest("listar").await.unwrap();
        assert_eq!(server.bodies().len(), 3);

        s.mark_suggestion_accepted("listar");
        s.mark_suggestion_accepted("nunca pedido");
        let stats = s.get_stats();
        assert_eq!(stats.total_suggestions, 3);
        assert_eq!(stats.accepted_suggestions, 1);
        assert!((stats.acceptance_rate - 1.0 / 3.0).abs() < 1e-6);
        // marca a entrada mais recente com aquele input
        let h = s.get_history();
        assert!(!h[0].accepted && h[2].accepted);
    }

    #[tokio::test]
    async fn test_suggest_sends_filtered_context() {
        let (s, server) = suggester_for(JSON).await;
        s.update_directory("/segredo");
        s.add_command_to_history("cat ~/.netrc");
        s.update_last_output("saida anterior");
        s.suggest("x").await.unwrap();
        let user = server.bodies()[0]["messages"][1]["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(!user.contains("/segredo"));
        assert!(!user.contains(".netrc"));
        assert!(user.contains("saida anterior"));
        // o contexto guardado não é alterado pelo filtro
        assert_eq!(
            s.get_context().current_directory.as_deref(),
            Some("/segredo")
        );
    }

    #[tokio::test]
    async fn test_suggest_rejections() {
        let (s, server) = suggester_for(JSON).await;
        assert!(matches!(
            s.suggest("   ").await,
            Err(AiError::SuggestionFailed(_))
        ));
        assert!(matches!(
            s.suggest(&"y".repeat(1001)).await,
            Err(AiError::ContextTooLong(_))
        ));
        s.disable();
        assert!(
            matches!(s.suggest("ls").await, Err(AiError::ConfigError(m)) if m.contains("disabled"))
        );
        assert!(server.bodies().is_empty());

        let mut config = AiConfig::openai("k").with_base_url(&server.base_url);
        config.enabled = false;
        let off = CommandSuggester::new(config).unwrap();
        assert!(
            matches!(off.suggest("ls").await, Err(AiError::ConfigError(m)) if m.contains("not configured"))
        );
    }

    #[tokio::test]
    async fn test_suggest_with_context_is_temporary() {
        let (_, server) = suggester_for(JSON).await;
        let mut config = AiConfig::openai("k").with_base_url(&server.base_url);
        config.privacy.send_file_paths = true;
        config.privacy.send_shell_history = true;
        let s2 = CommandSuggester::new(config).unwrap();
        s2.update_directory("/base");
        s2.add_command_to_history("make");
        let extra = TerminalContext {
            current_directory: Some("/extra".into()),
            shell_type: Some("fish".into()),
            recent_commands: vec!["cargo test".into()],
            ..Default::default()
        };
        s2.suggest_with_context("compilar", &extra).await.unwrap();
        let user = server.bodies()[0]["messages"][1]["content"]
            .as_str()
            .unwrap()
            .to_string();
        assert!(user.contains("Current directory: /extra"));
        assert!(user.contains("Shell: fish"));
        assert!(user.contains("make") && user.contains("cargo test"));
        // o contexto original volta depois da chamada
        let ctx = s2.get_context();
        assert_eq!(ctx.current_directory.as_deref(), Some("/base"));
        assert_eq!(ctx.shell_type, None);
        assert_eq!(ctx.recent_commands, vec!["make"]);

        // contexto extra vazio não muda nada, e erro também restaura
        s2.disable();
        assert!(s2
            .suggest_with_context("x", &TerminalContext::default())
            .await
            .is_err());
        assert_eq!(s2.get_context().current_directory.as_deref(), Some("/base"));
    }

    #[test]
    fn test_cache_and_history_are_bounded() {
        let s = CommandSuggester::new(AiConfig::openai("k")).unwrap();
        let ctx = TerminalContext::default();
        let sug = |c: &str| CommandSuggestion {
            command: c.into(),
            explanation: String::new(),
            alternatives: None,
            confidence: 1.0,
        };
        for i in 0..(MAX_SUGGESTION_CACHE + 5) {
            s.add_to_cache(&format!("i{i}"), &ctx, sug(&format!("c{i}")));
        }
        assert_eq!(s.cache.read().len(), MAX_SUGGESTION_CACHE);
        // as mais antigas saem primeiro
        assert!(s.check_cache("i0", &ctx).is_none());
        assert_eq!(s.check_cache("i54", &ctx).unwrap().command, "c54");
        // o contexto faz parte da chave
        let other = TerminalContext {
            shell_type: Some("zsh".into()),
            ..Default::default()
        };
        assert!(s.check_cache("i54", &other).is_none());

        for i in 0..(MAX_HISTORY_SIZE + 3) {
            s.add_to_history(&format!("h{i}"), sug("x"));
        }
        let h = s.get_history();
        assert_eq!(h.len(), MAX_HISTORY_SIZE);
        assert_eq!(h[0].input, "h3");
    }

    #[test]
    fn test_builder_adds_input_and_output() {
        let s = SuggestionBuilder::new("deploy")
            .with_recent_commands(vec!["git pull".into()])
            .with_last_output("ok")
            .with_config(AiConfig::openai("k"))
            .build()
            .unwrap();
        let ctx = s.get_context();
        assert_eq!(ctx.recent_commands, vec!["git pull", "deploy"]);
        assert_eq!(ctx.last_output.as_deref(), Some("ok"));

        // input em branco não entra no histórico; sem config falha (sem chave)
        let s = SuggestionBuilder::new("  ")
            .with_config(AiConfig::openai("k"))
            .build()
            .unwrap();
        assert!(s.get_context().recent_commands.is_empty());
        assert!(SuggestionBuilder::new("x").build().is_err());
    }
}
