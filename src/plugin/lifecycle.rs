//! Lifecycle Management para Plugins
//!
//! Gerencia o ciclo de vida dos plugins: Created → Initializing → Running → Stopping → Stopped

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Estados do ciclo de vida de um plugin
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PluginState {
    /// Plugin criado mas não inicializado
    #[default]
    Created,
    /// Plugin está inicializando
    Initializing,
    /// Plugin rodando normalmente
    Running,
    /// Plugin pausado (não recebe updates)
    Paused,
    /// Plugin está desligando
    Stopping,
    /// Plugin parado
    Stopped,
    /// Plugin em erro
    Error,
}

impl PluginState {
    /// Retorna true se o plugin está ativo (pode executar)
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }

    /// Retorna true se o plugin está rodando
    pub fn is_running(&self) -> bool {
        *self == Self::Running
    }

    /// Retorna true se o plugin pode ser iniciado
    pub fn can_start(&self) -> bool {
        matches!(self, Self::Created | Self::Stopped | Self::Error)
    }

    /// Retorna true se o plugin pode ser parado
    pub fn can_stop(&self) -> bool {
        matches!(self, Self::Running | Self::Paused | Self::Error)
    }

    /// Retorna true se o plugin pode ser pausado
    pub fn can_pause(&self) -> bool {
        *self == Self::Running
    }

    /// Retorna true se o plugin pode ser retomado
    pub fn can_resume(&self) -> bool {
        *self == Self::Paused
    }

    /// Retorna true se o plugin está em transição
    pub fn is_transitioning(&self) -> bool {
        matches!(self, Self::Initializing | Self::Stopping)
    }

    /// Retorna true se o plugin está em estado terminal
    pub fn is_terminal(&self) -> bool {
        *self == Self::Stopped
    }

    /// Retorna true se o plugin está inicializando ou rodando
    pub fn is_initializing_or_running(&self) -> bool {
        matches!(self, Self::Initializing | Self::Running)
    }

    /// Retorna a string representando o estado
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Initializing => "initializing",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for PluginState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Gerenciador de lifecycle thread-safe
pub struct PluginLifecycle {
    state: AtomicUsize,
}

impl PluginLifecycle {
    /// Cria um novo lifecycle no estado Created
    pub fn new() -> Self {
        Self {
            state: AtomicUsize::new(Self::state_to_usize(PluginState::Created)),
        }
    }

    /// Obtém o estado atual
    pub fn get(&self) -> PluginState {
        Self::usize_to_state(self.state.load(Ordering::Acquire))
    }

    /// Tenta transicionar para um novo estado
    ///
    /// # Returns
    /// * `Ok(())` - Transição bem-sucedida
    /// * `Err(PluginState)` - Transição inválida, retorna estado atual
    pub fn transition(&self, new_state: PluginState) -> Result<(), PluginState> {
        let current = self.get();

        // Valida transição
        if !Self::is_valid_transition(current, new_state) {
            return Err(current);
        }

        let new = Self::state_to_usize(new_state);
        let old = self.state.swap(new, Ordering::Release);

        if old == new {
            Ok(())
        } else {
            tracing::debug!(
                "Plugin lifecycle: {} -> {}",
                Self::usize_to_state(old),
                new_state
            );
            Ok(())
        }
    }

    /// Tenta transicionar para Running
    pub fn start(&self) -> Result<(), PluginState> {
        self.transition(PluginState::Running)
    }

    /// Tenta transicionar para Paused
    pub fn pause(&self) -> Result<(), PluginState> {
        self.transition(PluginState::Paused)
    }

    /// Tenta transicionar para Running (de Paused)
    pub fn resume(&self) -> Result<(), PluginState> {
        self.transition(PluginState::Running)
    }

    /// Tenta transicionar para Stopped
    pub fn stop(&self) -> Result<(), PluginState> {
        self.transition(PluginState::Stopped)
    }

    /// Define estado de erro
    pub fn set_error(&self) {
        let _ = self.transition(PluginState::Error);
    }

    /// Verifica se é uma transição válida
    fn is_valid_transition(from: PluginState, to: PluginState) -> bool {
        match (from, to) {
            // Criado pode ir para inicializando
            (PluginState::Created, PluginState::Initializing) => true,
            // Inicializando pode ir para rodando ou erro
            (PluginState::Initializing, PluginState::Running) => true,
            (PluginState::Initializing, PluginState::Error) => true,
            // Rodando pode ir para pausado, parando ou erro
            (PluginState::Running, PluginState::Paused) => true,
            (PluginState::Running, PluginState::Stopping) => true,
            (PluginState::Running, PluginState::Error) => true,
            // Pausado pode ir para rodando, parando ou erro
            (PluginState::Paused, PluginState::Running) => true,
            (PluginState::Paused, PluginState::Stopping) => true,
            (PluginState::Paused, PluginState::Error) => true,
            // Parando pode ir para parado ou erro
            (PluginState::Stopping, PluginState::Stopped) => true,
            (PluginState::Stopping, PluginState::Error) => true,
            // Erro pode ir para parado
            (PluginState::Error, PluginState::Stopped) => true,
            // Parado pode ir para inicializando (restart)
            (PluginState::Stopped, PluginState::Initializing) => true,
            // Mesmo estado é sempre válido (no-op)
            (a, b) if a == b => true,
            // Todos outros são inválidos
            _ => false,
        }
    }

    fn state_to_usize(state: PluginState) -> usize {
        match state {
            PluginState::Created => 0,
            PluginState::Initializing => 1,
            PluginState::Running => 2,
            PluginState::Paused => 3,
            PluginState::Stopping => 4,
            PluginState::Stopped => 5,
            PluginState::Error => 6,
        }
    }

    fn usize_to_state(value: usize) -> PluginState {
        match value {
            0 => PluginState::Created,
            1 => PluginState::Initializing,
            2 => PluginState::Running,
            3 => PluginState::Paused,
            4 => PluginState::Stopping,
            5 => PluginState::Stopped,
            6 => PluginState::Error,
            _ => PluginState::Error,
        }
    }
}

impl Default for PluginLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for PluginLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginLifecycle")
            .field("state", &self.get())
            .finish()
    }
}

/// Eventos de lifecycle que plugins podem observar
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEvent {
    /// Plugin foi criado
    Created,
    /// Plugin está inicializando
    Initializing,
    /// Plugin começou a rodar
    Started,
    /// Plugin foi pausado
    Paused,
    /// Plugin foi retomado
    Resumed,
    /// Plugin está parando
    Stopping,
    /// Plugin parou
    Stopped,
    /// Erro no plugin
    Error { reason: String },
}

/// Handler para eventos de lifecycle
pub trait LifecycleHandler: Send + Sync {
    /// Chamado quando ocorre um evento de lifecycle
    fn on_lifecycle_event(&mut self, plugin_name: &str, event: LifecycleEvent);
}

type BoxedLifecycleHandler = Box<dyn LifecycleHandler>;

/// Gerenciador de eventos de lifecycle
pub struct LifecycleManager {
    handlers: Vec<BoxedLifecycleHandler>,
}

impl LifecycleManager {
    /// Cria um novo gerenciador
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Registra um handler
    pub fn register(&mut self, handler: BoxedLifecycleHandler) {
        self.handlers.push(handler);
    }

    /// Notifica todos os handlers sobre um evento
    pub fn notify(&mut self, plugin_name: &str, event: LifecycleEvent) {
        for handler in &mut self.handlers {
            handler.on_lifecycle_event(plugin_name, event.clone());
        }
    }
}

impl Default for LifecycleManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_is_active() {
        assert!(!PluginState::Created.is_active());
        assert!(!PluginState::Initializing.is_active());
        assert!(PluginState::Running.is_active());
        assert!(PluginState::Paused.is_active());
        assert!(!PluginState::Stopped.is_active());
    }

    #[test]
    fn test_state_is_running() {
        assert!(!PluginState::Created.is_running());
        assert!(PluginState::Running.is_running());
        assert!(!PluginState::Paused.is_running());
    }

    #[test]
    fn test_state_can_start() {
        assert!(PluginState::Created.can_start());
        assert!(PluginState::Stopped.can_start());
        assert!(!PluginState::Running.can_start());
    }

    #[test]
    fn test_lifecycle_new() {
        let lifecycle = PluginLifecycle::new();
        assert_eq!(lifecycle.get(), PluginState::Created);
    }

    #[test]
    fn test_lifecycle_transition_valid() {
        let lifecycle = PluginLifecycle::new();

        assert!(lifecycle.transition(PluginState::Initializing).is_ok());
        assert_eq!(lifecycle.get(), PluginState::Initializing);

        assert!(lifecycle.transition(PluginState::Running).is_ok());
        assert_eq!(lifecycle.get(), PluginState::Running);
    }

    #[test]
    fn test_lifecycle_transition_invalid() {
        let lifecycle = PluginLifecycle::new();

        // Não pode ir direto de Created para Running
        assert!(lifecycle.transition(PluginState::Running).is_err());
    }

    #[test]
    fn test_lifecycle_start_stop() {
        let lifecycle = PluginLifecycle::new();

        lifecycle.transition(PluginState::Initializing).unwrap();
        lifecycle.transition(PluginState::Running).unwrap();
        lifecycle.transition(PluginState::Stopping).unwrap();

        assert!(lifecycle.stop().is_ok());
        assert_eq!(lifecycle.get(), PluginState::Stopped);
    }

    #[test]
    fn test_lifecycle_pause_resume() {
        let lifecycle = PluginLifecycle::new();

        lifecycle.transition(PluginState::Initializing).unwrap();
        lifecycle.transition(PluginState::Running).unwrap();

        assert!(lifecycle.pause().is_ok());
        assert_eq!(lifecycle.get(), PluginState::Paused);

        assert!(lifecycle.resume().is_ok());
        assert_eq!(lifecycle.get(), PluginState::Running);
    }

    #[test]
    fn test_lifecycle_set_error() {
        let lifecycle = PluginLifecycle::new();

        lifecycle.transition(PluginState::Initializing).unwrap();
        lifecycle.transition(PluginState::Running).unwrap();

        lifecycle.set_error();
        assert_eq!(lifecycle.get(), PluginState::Error);
    }

    #[test]
    fn test_state_display() {
        assert_eq!(PluginState::Running.to_string(), "running");
        assert_eq!(PluginState::Stopped.to_string(), "stopped");
    }

    const ALL: [PluginState; 7] = [
        PluginState::Created,
        PluginState::Initializing,
        PluginState::Running,
        PluginState::Paused,
        PluginState::Stopping,
        PluginState::Stopped,
        PluginState::Error,
    ];

    #[test]
    fn test_state_predicates_for_every_state() {
        // (as_str, active, can_start, can_stop, can_pause, can_resume, transitioning, terminal, init_or_run)
        let expected = [
            (
                "created", false, true, false, false, false, false, false, false,
            ),
            (
                "initializing",
                false,
                false,
                false,
                false,
                false,
                true,
                false,
                true,
            ),
            (
                "running", true, false, true, true, false, false, false, true,
            ),
            (
                "paused", true, false, true, false, true, false, false, false,
            ),
            (
                "stopping", false, false, false, false, false, true, false, false,
            ),
            (
                "stopped", false, true, false, false, false, false, true, false,
            ),
            (
                "error", false, true, true, false, false, false, false, false,
            ),
        ];
        for (state, e) in ALL.iter().zip(expected) {
            assert_eq!(state.as_str(), e.0);
            assert_eq!(state.to_string(), e.0);
            assert_eq!(state.is_active(), e.1, "{state}");
            assert_eq!(state.can_start(), e.2, "{state}");
            assert_eq!(state.can_stop(), e.3, "{state}");
            assert_eq!(state.can_pause(), e.4, "{state}");
            assert_eq!(state.can_resume(), e.5, "{state}");
            assert_eq!(state.is_transitioning(), e.6, "{state}");
            assert_eq!(state.is_terminal(), e.7, "{state}");
            assert_eq!(state.is_initializing_or_running(), e.8, "{state}");
        }
        assert_eq!(PluginState::default(), PluginState::Created);
    }

    #[test]
    fn test_transition_table() {
        let valid = [
            (PluginState::Created, PluginState::Initializing),
            (PluginState::Initializing, PluginState::Running),
            (PluginState::Initializing, PluginState::Error),
            (PluginState::Running, PluginState::Paused),
            (PluginState::Running, PluginState::Stopping),
            (PluginState::Running, PluginState::Error),
            (PluginState::Paused, PluginState::Running),
            (PluginState::Paused, PluginState::Stopping),
            (PluginState::Paused, PluginState::Error),
            (PluginState::Stopping, PluginState::Stopped),
            (PluginState::Stopping, PluginState::Error),
            (PluginState::Error, PluginState::Stopped),
            (PluginState::Stopped, PluginState::Initializing),
        ];
        for from in ALL {
            for to in ALL {
                let expected = from == to || valid.contains(&(from, to));
                assert_eq!(
                    PluginLifecycle::is_valid_transition(from, to),
                    expected,
                    "{from} -> {to}"
                );
            }
        }
        // ida e volta dos estados pelo armazenamento atômico
        for state in ALL {
            let raw = PluginLifecycle::state_to_usize(state);
            assert_eq!(PluginLifecycle::usize_to_state(raw), state);
        }
        assert_eq!(PluginLifecycle::usize_to_state(99), PluginState::Error);
    }

    #[test]
    fn test_invalid_transitions_keep_state() {
        let lifecycle = PluginLifecycle::default();
        // Created -> Running não é permitido
        assert_eq!(lifecycle.start(), Err(PluginState::Created));
        // set_error de Created é ignorado
        lifecycle.set_error();
        assert_eq!(lifecycle.get(), PluginState::Created);
        // Mesmo estado é no-op válido
        lifecycle.transition(PluginState::Created).unwrap();

        lifecycle.transition(PluginState::Initializing).unwrap();
        lifecycle.transition(PluginState::Running).unwrap();
        // stop direto de Running exige passar por Stopping
        assert_eq!(lifecycle.stop(), Err(PluginState::Running));
        assert_eq!(
            format!("{:?}", lifecycle),
            "PluginLifecycle { state: Running }"
        );
    }

    struct Collect(std::sync::Arc<std::sync::Mutex<Vec<(String, LifecycleEvent)>>>);
    impl LifecycleHandler for Collect {
        fn on_lifecycle_event(&mut self, plugin_name: &str, event: LifecycleEvent) {
            self.0.lock().unwrap().push((plugin_name.into(), event));
        }
    }

    #[test]
    fn test_manager_notifies_every_handler() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut manager = LifecycleManager::default();
        manager.notify("ninguem", LifecycleEvent::Created); // sem handlers
        manager.register(Box::new(Collect(log.clone())));
        manager.register(Box::new(Collect(log.clone())));
        manager.notify("p", LifecycleEvent::Error { reason: "x".into() });
        let events = log.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(
            |(n, e)| n == "p" && matches!(e, LifecycleEvent::Error { reason } if reason == "x")
        ));
    }
}
