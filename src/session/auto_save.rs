//! Sistema de auto-save para sessões
//!
//! Este módulo implementa salvamento automático de sessões com:
//! - Timer periódico configurável
//! - Salvamento em eventos específicos (criar/fechar tab, etc)
//! - Salvamento automático no shutdown
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::session::{
//!     AutoSaveConfig, AutoSaveEvent, AutoSaveManager, SessionData, SessionManager,
//! };
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let session_manager = SessionManager::default()?;
//! let config = AutoSaveConfig::default();
//! let auto_save = AutoSaveManager::new(config, session_manager);
//!
//! // Iniciar auto-save com callback de captura
//! let handle = auto_save.start(|name, manager| {
//!     // Capturar e salvar estado
//!     let result = manager
//!         .save(name, &SessionData::new())
//!         .map(|()| manager.sessions_dir().join(format!("{name}.json")));
//!     async move { result }
//! });
//!
//! if let Some(handle) = handle {
//!     // Notificar evento
//!     handle.notify_event(AutoSaveEvent::TabCreated);
//!
//!     // Parar no shutdown
//!     handle.shutdown().await;
//! }
//! # Ok(())
//! # }
//! ```

use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{interval, Instant};
use tracing::{debug, error, info, warn};

use crate::session::{SessionError, SessionManager, SessionResult};

/// Nome da sessão de auto-save padrão
pub const AUTO_SAVE_SESSION_NAME: &str = "__auto_save__";

/// Eventos que podem disparar auto-save
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutoSaveEvent {
    /// Uma nova aba foi criada
    TabCreated,
    /// Uma aba foi fechada
    TabClosed,
    /// O layout foi alterado (split criado/removido)
    LayoutChanged,
    /// Sessão SSH conectada/desconectada
    SshSessionChanged,
    /// Título da aba alterado
    TabTitleChanged,
    /// Diretório de trabalho alterado
    WorkingDirectoryChanged,
    /// Tamanho do terminal alterado
    TerminalResized,
    /// Timer periódico disparado
    TimerTick,
    /// Aplicação está fechando
    Shutdown,
}

impl AutoSaveEvent {
    /// Retorna a descrição do evento
    pub fn description(&self) -> &'static str {
        match self {
            AutoSaveEvent::TabCreated => "tab_created",
            AutoSaveEvent::TabClosed => "tab_closed",
            AutoSaveEvent::LayoutChanged => "layout_changed",
            AutoSaveEvent::SshSessionChanged => "ssh_session_changed",
            AutoSaveEvent::TabTitleChanged => "tab_title_changed",
            AutoSaveEvent::WorkingDirectoryChanged => "working_directory_changed",
            AutoSaveEvent::TerminalResized => "terminal_resized",
            AutoSaveEvent::TimerTick => "timer_tick",
            AutoSaveEvent::Shutdown => "shutdown",
        }
    }
}

/// Configuração do auto-save
#[derive(Debug, Clone)]
pub struct AutoSaveConfig {
    /// Habilita/desabilita auto-save
    pub enabled: bool,
    /// Intervalo entre salvamentos periódicos
    pub interval: Duration,
    /// Salvar ao criar nova aba
    pub on_tab_created: bool,
    /// Salvar ao fechar aba
    pub on_tab_closed: bool,
    /// Salvar ao alterar layout
    pub on_layout_changed: bool,
    /// Salvar ao alterar sessão SSH
    pub on_ssh_changed: bool,
    /// Salvar ao alterar título da aba
    pub on_title_changed: bool,
    /// Salvar ao alterar diretório de trabalho
    pub on_working_dir_changed: bool,
    /// Salvar ao redimensionar terminal
    pub on_resize: bool,
    /// Debounce time para eventos (evita múltiplos salvamentos rápidos)
    pub debounce_duration: Duration,
    /// Número máximo de sessões de auto-save a manter
    pub max_auto_save_sessions: usize,
    /// Nome da sessão de auto-save
    pub session_name: String,
}

impl Default for AutoSaveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: Duration::from_secs(300), // 5 minutos
            on_tab_created: true,
            on_tab_closed: true,
            on_layout_changed: true,
            on_ssh_changed: true,
            on_title_changed: false,
            on_working_dir_changed: false,
            on_resize: false,
            debounce_duration: Duration::from_secs(2),
            max_auto_save_sessions: 3,
            session_name: AUTO_SAVE_SESSION_NAME.to_string(),
        }
    }
}

impl AutoSaveConfig {
    /// Cria configuração com intervalo personalizado
    pub fn with_interval(interval: Duration) -> Self {
        Self {
            interval,
            ..Default::default()
        }
    }

    /// Desabilita todos os eventos, mantendo apenas timer
    pub fn timer_only() -> Self {
        Self {
            on_tab_created: false,
            on_tab_closed: false,
            on_layout_changed: false,
            on_ssh_changed: false,
            on_title_changed: false,
            on_working_dir_changed: false,
            on_resize: false,
            ..Default::default()
        }
    }

    /// Habilita todos os eventos
    pub fn all_events() -> Self {
        Self {
            on_tab_created: true,
            on_tab_closed: true,
            on_layout_changed: true,
            on_ssh_changed: true,
            on_title_changed: true,
            on_working_dir_changed: true,
            on_resize: true,
            ..Default::default()
        }
    }

    /// Verifica se um evento deve disparar auto-save
    pub fn should_save_on_event(&self, event: AutoSaveEvent) -> bool {
        if !self.enabled {
            return false;
        }

        match event {
            AutoSaveEvent::TimerTick => true,
            AutoSaveEvent::TabCreated => self.on_tab_created,
            AutoSaveEvent::TabClosed => self.on_tab_closed,
            AutoSaveEvent::LayoutChanged => self.on_layout_changed,
            AutoSaveEvent::SshSessionChanged => self.on_ssh_changed,
            AutoSaveEvent::TabTitleChanged => self.on_title_changed,
            AutoSaveEvent::WorkingDirectoryChanged => self.on_working_dir_changed,
            AutoSaveEvent::TerminalResized => self.on_resize,
            AutoSaveEvent::Shutdown => true,
        }
    }
}

/// Estado interno do auto-save
#[derive(Debug, Default)]
struct AutoSaveState {
    /// Último salvamento
    last_save: Option<Instant>,
    /// Contador de salvamentos
    save_count: u64,
    /// Erros consecutivos
    consecutive_errors: u32,
    /// Último erro
    last_error: Option<String>,
    /// Se está em debounce
    debounce_until: Option<Instant>,
}

impl AutoSaveState {
    /// Verifica se está em período de debounce
    fn is_debouncing(&self) -> bool {
        self.debounce_until
            .map(|t| Instant::now() < t)
            .unwrap_or(false)
    }

    /// Define o período de debounce
    fn set_debounce(&mut self, duration: Duration) {
        self.debounce_until = Some(Instant::now() + duration);
    }

    /// Registra um salvamento bem-sucedido
    fn record_success(&mut self) {
        self.last_save = Some(Instant::now());
        self.save_count += 1;
        self.consecutive_errors = 0;
        self.last_error = None;
    }

    /// Registra um erro
    fn record_error(&mut self, error: impl ToString) {
        self.consecutive_errors += 1;
        self.last_error = Some(error.to_string());
    }
}

/// Handle para controlar o auto-save em execução
#[derive(Debug)]
pub struct AutoSaveHandle {
    /// Canal para enviar comandos
    command_tx: mpsc::UnboundedSender<AutoSaveCommand>,
    /// Handle da task
    #[allow(dead_code)]
    task_handle: JoinHandle<()>,
}

/// Comandos para o auto-save
#[derive(Debug)]
enum AutoSaveCommand {
    /// Notificar evento
    NotifyEvent(AutoSaveEvent),
    /// Forçar salvamento imediato
    ForceSave,
    /// Pausar auto-save
    Pause,
    /// Resumir auto-save
    Resume,
    /// Parar auto-save (shutdown)
    Shutdown,
}

impl AutoSaveHandle {
    /// Notifica um evento que pode disparar auto-save
    pub fn notify_event(&self, event: AutoSaveEvent) {
        let _ = self.command_tx.send(AutoSaveCommand::NotifyEvent(event));
    }

    /// Força um salvamento imediato
    pub fn force_save(&self) {
        let _ = self.command_tx.send(AutoSaveCommand::ForceSave);
    }

    /// Pausa o auto-save temporariamente
    pub fn pause(&self) {
        let _ = self.command_tx.send(AutoSaveCommand::Pause);
    }

    /// Resume o auto-save
    pub fn resume(&self) {
        let _ = self.command_tx.send(AutoSaveCommand::Resume);
    }

    /// Para o auto-save e aguarda finalização
    pub async fn shutdown(self) {
        let _ = self.command_tx.send(AutoSaveCommand::Shutdown);
        // Aguardar a task finalizar
        if let Err(e) = self.task_handle.await {
            error!(error = ?e, "Erro ao aguardar task de auto-save");
        }
    }

    /// Para o auto-save sem aguardar (útil em sync contexts)
    pub fn shutdown_now(&self) {
        let _ = self.command_tx.send(AutoSaveCommand::Shutdown);
    }
}

/// Estatísticas do auto-save
#[derive(Debug, Clone, Default)]
pub struct AutoSaveStats {
    /// Número total de salvamentos
    pub save_count: u64,
    /// Último salvamento
    pub last_save: Option<Instant>,
    /// Erros consecutivos
    pub consecutive_errors: u32,
    /// Última mensagem de erro
    pub last_error: Option<String>,
    /// Se está pausado
    pub is_paused: bool,
}

/// Tipo do callback de salvamento
pub type SaveCallback = Box<dyn Fn(&str, &SessionManager) -> SessionResult<PathBuf> + Send + Sync>;

/// Gerenciador de auto-save
pub struct AutoSaveManager {
    /// Configuração
    config: AutoSaveConfig,
    /// Gerenciador de sessões
    session_manager: Arc<SessionManager>,
}

impl std::fmt::Debug for AutoSaveManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoSaveManager")
            .field("config", &self.config)
            .field("session_manager", &"<Arc<SessionManager>>")
            .finish()
    }
}

impl AutoSaveManager {
    /// Cria um novo gerenciador de auto-save
    pub fn new(config: AutoSaveConfig, session_manager: SessionManager) -> Self {
        Self {
            config,
            session_manager: Arc::new(session_manager),
        }
    }

    /// Cria com configuração padrão
    pub fn with_default_config(session_manager: SessionManager) -> Self {
        Self::new(AutoSaveConfig::default(), session_manager)
    }

    /// Inicia o auto-save e retorna um handle para controle
    pub fn start<F, Fut>(self, save_callback: F) -> Option<AutoSaveHandle>
    where
        F: Fn(&str, &SessionManager) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = SessionResult<PathBuf>> + Send + 'static,
    {
        if !self.config.enabled {
            info!("Auto-save está desabilitado");
            return None;
        }

        let (command_tx, command_rx) = mpsc::unbounded_channel();

        let task_handle = tokio::spawn(async move {
            self.run(command_rx, save_callback).await;
        });

        info!("Auto-save iniciado");

        Some(AutoSaveHandle {
            command_tx,
            task_handle,
        })
    }

    /// Loop principal do auto-save
    async fn run<F, Fut>(
        self,
        mut command_rx: mpsc::UnboundedReceiver<AutoSaveCommand>,
        save_callback: F,
    ) where
        F: Fn(&str, &SessionManager) -> Fut + Send + Sync,
        Fut: Future<Output = SessionResult<PathBuf>>,
    {
        let mut timer = interval(self.config.interval);
        let mut state = AutoSaveState::default();
        let mut paused = false;

        info!(
            interval = ?self.config.interval,
            "Loop de auto-save iniciado"
        );

        loop {
            tokio::select! {
                // Timer periódico
                _ = timer.tick() => {
                    if !paused {
                        self.handle_event(AutoSaveEvent::TimerTick, &mut state, &save_callback).await;
                    }
                }

                // Comandos recebidos
                Some(command) = command_rx.recv() => {
                    match command {
                        AutoSaveCommand::NotifyEvent(event) => {
                            if !paused {
                                self.handle_event(event, &mut state, &save_callback).await;
                            }
                        }
                        AutoSaveCommand::ForceSave => {
                            info!("Forçando salvamento de sessão");
                            self.perform_save(&mut state, &save_callback).await;
                        }
                        AutoSaveCommand::Pause => {
                            info!("Auto-save pausado");
                            paused = true;
                        }
                        AutoSaveCommand::Resume => {
                            info!("Auto-save resumido");
                            paused = false;
                        }
                        AutoSaveCommand::Shutdown => {
                            info!("Shutdown do auto-save solicitado");
                            self.handle_event(AutoSaveEvent::Shutdown, &mut state, &save_callback).await;
                            break;
                        }
                    }
                }
            }
        }

        info!(
            total_saves = state.save_count,
            "Loop de auto-save finalizado"
        );
    }

    /// Processa um evento de auto-save
    async fn handle_event<F, Fut>(
        &self,
        event: AutoSaveEvent,
        state: &mut AutoSaveState,
        save_callback: &F,
    ) where
        F: Fn(&str, &SessionManager) -> Fut + Send + Sync,
        Fut: Future<Output = SessionResult<PathBuf>>,
    {
        if !self.config.should_save_on_event(event) {
            return;
        }

        // Verifica debounce
        if state.is_debouncing() && event != AutoSaveEvent::Shutdown {
            debug!(
                event = event.description(),
                "Auto-save em debounce, ignorando evento"
            );
            return;
        }

        debug!(
            event = event.description(),
            "Processando evento de auto-save"
        );

        // Executa o salvamento
        self.perform_save(state, save_callback).await;

        // Define debounce para eventos não-timer
        if event != AutoSaveEvent::TimerTick && event != AutoSaveEvent::Shutdown {
            state.set_debounce(self.config.debounce_duration);
        }
    }

    /// Executa o salvamento da sessão
    async fn perform_save<F, Fut>(&self, state: &mut AutoSaveState, save_callback: &F)
    where
        F: Fn(&str, &SessionManager) -> Fut + Send + Sync,
        Fut: Future<Output = SessionResult<PathBuf>>,
    {
        match save_callback(&self.config.session_name, &self.session_manager).await {
            Ok(path) => {
                state.record_success();
                info!(
                    path = %path.display(),
                    count = state.save_count,
                    "Sessão auto-salva com sucesso"
                );

                // Limpa sessões antigas se necessário
                if let Err(e) = self.cleanup_old_sessions().await {
                    warn!(error = ?e, "Erro ao limpar sessões antigas");
                }
            }
            Err(e) => {
                state.record_error(&e);
                error!(
                    error = ?e,
                    consecutive = state.consecutive_errors,
                    "Falha ao auto-salvar sessão"
                );

                // Se houver muitos erros consecutivos, desabilita temporariamente
                if state.consecutive_errors >= 5 {
                    warn!(
                        errors = state.consecutive_errors,
                        "Muitos erros consecutivos no auto-save"
                    );
                }
            }
        }
    }

    /// Limpa sessões de auto-save antigas
    async fn cleanup_old_sessions(&self) -> SessionResult<()> {
        let sessions = self.session_manager.list_sessions()?;

        // Filtra apenas sessões de auto-save
        let auto_save_sessions: Vec<_> = sessions
            .into_iter()
            .filter(|s| s.name.starts_with(&self.config.session_name))
            .collect();

        if auto_save_sessions.len() > self.config.max_auto_save_sessions {
            // Ordena por data de modificação (mais antigas primeiro)
            let mut to_remove: Vec<_> = auto_save_sessions;
            to_remove.sort_by_key(|s| s.modified_at);

            // Remove as mais antigas
            let to_remove_count = to_remove.len() - self.config.max_auto_save_sessions;
            for session in to_remove.into_iter().take(to_remove_count) {
                debug!(name = %session.name, "Removendo sessão de auto-save antiga");
                if let Err(e) = self.session_manager.delete(&session.name) {
                    warn!(error = ?e, "Erro ao remover sessão antiga");
                }
            }
        }

        Ok(())
    }

    /// Verifica se há uma sessão de auto-save disponível
    pub fn has_auto_save_session(&self) -> SessionResult<bool> {
        match self.session_manager.load(&self.config.session_name) {
            Ok(_) => Ok(true),
            Err(SessionError::NotFound { .. }) => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// Helper para criar auto-save com configuração mínima
pub fn create_auto_save<F, Fut>(
    session_manager: SessionManager,
    save_callback: F,
) -> Option<AutoSaveHandle>
where
    F: Fn(&str, &SessionManager) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = SessionResult<PathBuf>> + Send + 'static,
{
    AutoSaveManager::with_default_config(session_manager).start(save_callback)
}

/// Helper para criar auto-save com intervalo específico
pub fn create_auto_save_with_interval<F, Fut>(
    session_manager: SessionManager,
    interval: Duration,
    save_callback: F,
) -> Option<AutoSaveHandle>
where
    F: Fn(&str, &SessionManager) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = SessionResult<PathBuf>> + Send + 'static,
{
    let config = AutoSaveConfig::with_interval(interval);
    AutoSaveManager::new(config, session_manager).start(save_callback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_session_manager() -> (SessionManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let config =
            crate::session::SessionConfig::with_directory(temp_dir.path().join("sessions"));
        let manager = SessionManager::new(config);
        manager.initialize().unwrap();
        (manager, temp_dir)
    }

    #[test]
    fn test_autosave_config_default() {
        let config = AutoSaveConfig::default();
        assert!(config.enabled);
        assert_eq!(config.interval, Duration::from_secs(300));
        assert!(config.on_tab_created);
        assert!(config.on_tab_closed);
        assert!(config.on_layout_changed);
        assert!(config.on_ssh_changed);
        assert!(!config.on_title_changed);
        assert_eq!(config.max_auto_save_sessions, 3);
    }

    #[test]
    fn test_autosave_config_should_save_on_event() {
        let config = AutoSaveConfig::default();

        assert!(config.should_save_on_event(AutoSaveEvent::TimerTick));
        assert!(config.should_save_on_event(AutoSaveEvent::TabCreated));
        assert!(config.should_save_on_event(AutoSaveEvent::Shutdown));

        let timer_only = AutoSaveConfig::timer_only();
        assert!(timer_only.should_save_on_event(AutoSaveEvent::TimerTick));
        assert!(!timer_only.should_save_on_event(AutoSaveEvent::TabCreated));
    }

    #[test]
    fn test_autosave_config_disabled() {
        let config = AutoSaveConfig {
            enabled: false,
            ..Default::default()
        };

        assert!(!config.should_save_on_event(AutoSaveEvent::TimerTick));
        assert!(!config.should_save_on_event(AutoSaveEvent::TabCreated));
    }

    #[test]
    fn test_autosave_state_debounce() {
        let mut state = AutoSaveState::default();

        assert!(!state.is_debouncing());

        state.set_debounce(Duration::from_secs(10));
        assert!(state.is_debouncing());

        // Simular passagem de tempo não implementado em teste unitário
        // mas a lógica é verificada
    }

    #[test]
    fn test_autosave_state_record_success() {
        let mut state = AutoSaveState::default();

        state.record_success();

        assert_eq!(state.save_count, 1);
        assert!(state.last_save.is_some());
        assert_eq!(state.consecutive_errors, 0);
    }

    #[test]
    fn test_autosave_state_record_error() {
        let mut state = AutoSaveState::default();

        state.record_error("test error");

        assert_eq!(state.consecutive_errors, 1);
        assert_eq!(state.last_error, Some("test error".to_string()));

        state.record_error("another error");
        assert_eq!(state.consecutive_errors, 2);
    }

    #[test]
    fn test_event_descriptions() {
        assert_eq!(AutoSaveEvent::TabCreated.description(), "tab_created");
        assert_eq!(AutoSaveEvent::TimerTick.description(), "timer_tick");
        assert_eq!(AutoSaveEvent::Shutdown.description(), "shutdown");
    }

    #[tokio::test]
    async fn test_autosave_manager_disabled() {
        let (manager, _temp) = create_test_session_manager();

        let config = AutoSaveConfig {
            enabled: false,
            ..Default::default()
        };

        let auto_save = AutoSaveManager::new(config, manager);
        let handle = auto_save.start(|_name, _manager| async { Ok(PathBuf::from("/tmp/test")) });

        assert!(handle.is_none());
    }

    #[tokio::test]
    async fn test_autosave_handle_commands() {
        let (manager, _temp) = create_test_session_manager();

        let auto_save = AutoSaveManager::with_default_config(manager);
        let handle = auto_save
            .start(|_name, _manager| async { Ok(PathBuf::from("/tmp/test")) })
            .expect("Auto-save deveria iniciar");

        // Testa comandos (não verifica efeitos, apenas que não panic)
        handle.notify_event(AutoSaveEvent::TabCreated);
        handle.pause();
        handle.resume();
        handle.force_save();

        // Shutdown
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn test_autosave_helpers() {
        let (manager, _temp) = create_test_session_manager();

        // Testa helpers (retornam Option, podem ser None se desabilitado)
        let _ = create_auto_save(manager.clone(), |_name, _manager| async {
            Ok(PathBuf::from("/tmp/test"))
        });
        let _ = create_auto_save_with_interval(
            manager,
            Duration::from_secs(60),
            |_name, _manager| async { Ok(PathBuf::from("/tmp/test")) },
        );
    }
}
