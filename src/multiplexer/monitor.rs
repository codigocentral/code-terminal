//! Session Monitor - Monitoramento e estatísticas de sessões

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::session::{SessionId, SessionState};

/// Estatísticas de uma sessão
#[derive(Debug, Clone)]
pub struct SessionStats {
    /// ID da sessão
    pub session_id: SessionId,
    /// Bytes recebidos (input)
    pub bytes_received: u64,
    /// Bytes enviados (output)
    pub bytes_sent: u64,
    /// Número de comandos executados
    pub commands_executed: u64,
    /// Tempo total ativa
    pub total_active_time: Duration,
    /// Tempo em background
    pub total_background_time: Duration,
    /// Número de attaches
    pub attach_count: u64,
    /// Número de detaches
    pub detach_count: u64,
    /// Quando foi iniciada
    pub started_at: Instant,
    /// Uso de memória estimado (bytes)
    pub estimated_memory: u64,
}

impl SessionStats {
    /// Cria estatísticas para uma nova sessão
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            bytes_received: 0,
            bytes_sent: 0,
            commands_executed: 0,
            total_active_time: Duration::from_secs(0),
            total_background_time: Duration::from_secs(0),
            attach_count: 0,
            detach_count: 0,
            started_at: Instant::now(),
            estimated_memory: 0,
        }
    }

    /// Registra bytes recebidos
    pub fn record_received(&mut self, bytes: u64) {
        self.bytes_received += bytes;
    }

    /// Registra bytes enviados
    pub fn record_sent(&mut self, bytes: u64) {
        self.bytes_sent += bytes;
    }

    /// Registra um comando executado
    pub fn record_command(&mut self) {
        self.commands_executed += 1;
    }

    /// Registra um attach
    pub fn record_attach(&mut self) {
        self.attach_count += 1;
    }

    /// Registra um detach
    pub fn record_detach(&mut self) {
        self.detach_count += 1;
    }

    /// Atualiza o tempo ativo
    pub fn add_active_time(&mut self, duration: Duration) {
        self.total_active_time += duration;
    }

    /// Atualiza o tempo em background
    pub fn add_background_time(&mut self, duration: Duration) {
        self.total_background_time += duration;
    }

    /// Retorna o uptime total
    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Retorna a taxa de transferência média (bytes/segundo)
    pub fn throughput_bps(&self) -> f64 {
        let uptime_secs = self.uptime().as_secs_f64();
        if uptime_secs > 0.0 {
            (self.bytes_received + self.bytes_sent) as f64 / uptime_secs
        } else {
            0.0
        }
    }
}

/// Estatísticas globais do servidor
#[derive(Debug)]
pub struct GlobalStats {
    /// Total de sessões criadas
    pub total_sessions_created: AtomicU64,
    /// Total de sessões destruídas
    pub total_sessions_destroyed: AtomicU64,
    /// Total de clientes conectados
    pub total_clients_connected: AtomicU64,
    /// Total de bytes transferidos
    pub total_bytes_transferred: AtomicU64,
    /// Tempo de funcionamento do servidor
    pub server_start_time: Instant,
}

impl GlobalStats {
    /// Cria novas estatísticas globais
    pub fn new() -> Self {
        Self {
            total_sessions_created: AtomicU64::new(0),
            total_sessions_destroyed: AtomicU64::new(0),
            total_clients_connected: AtomicU64::new(0),
            total_bytes_transferred: AtomicU64::new(0),
            server_start_time: Instant::now(),
        }
    }

    /// Registra criação de sessão
    pub fn record_session_created(&self) {
        self.total_sessions_created.fetch_add(1, Ordering::SeqCst);
    }

    /// Registra destruição de sessão
    pub fn record_session_destroyed(&self) {
        self.total_sessions_destroyed.fetch_add(1, Ordering::SeqCst);
    }

    /// Registra conexão de cliente
    pub fn record_client_connected(&self) {
        self.total_clients_connected.fetch_add(1, Ordering::SeqCst);
    }

    /// Registra transferência de dados
    pub fn record_bytes_transferred(&self, bytes: u64) {
        self.total_bytes_transferred
            .fetch_add(bytes, Ordering::Relaxed);
    }

    /// Retorna o uptime do servidor
    pub fn server_uptime(&self) -> Duration {
        self.server_start_time.elapsed()
    }

    /// Retorna estatísticas resumidas
    pub fn summary(&self) -> StatsSummary {
        StatsSummary {
            total_sessions_created: self.total_sessions_created.load(Ordering::SeqCst),
            total_sessions_destroyed: self.total_sessions_destroyed.load(Ordering::SeqCst),
            total_clients_connected: self.total_clients_connected.load(Ordering::SeqCst),
            total_bytes_transferred: self.total_bytes_transferred.load(Ordering::Relaxed),
            server_uptime_secs: self.server_uptime().as_secs(),
        }
    }
}

impl Default for GlobalStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Resumo de estatísticas
#[derive(Debug, Clone)]
pub struct StatsSummary {
    pub total_sessions_created: u64,
    pub total_sessions_destroyed: u64,
    pub total_clients_connected: u64,
    pub total_bytes_transferred: u64,
    pub server_uptime_secs: u64,
}

/// Monitor de sessões
#[derive(Debug)]
pub struct SessionMonitor {
    /// Estatísticas por sessão
    session_stats: HashMap<SessionId, SessionStats>,
    /// Estatísticas globais
    global_stats: GlobalStats,
    /// Histórico de estados
    state_history: Vec<StateChange>,
    /// Limite do histórico
    max_history: usize,
}

/// Mudança de estado
#[derive(Debug, Clone)]
pub struct StateChange {
    pub timestamp: Instant,
    pub session_id: SessionId,
    pub old_state: SessionState,
    pub new_state: SessionState,
}

impl SessionMonitor {
    /// Cria um novo monitor
    pub fn new() -> Self {
        Self::with_max_history(1000)
    }

    /// Cria com limite de histórico
    pub fn with_max_history(max_history: usize) -> Self {
        Self {
            session_stats: HashMap::new(),
            global_stats: GlobalStats::new(),
            state_history: Vec::with_capacity(max_history),
            max_history,
        }
    }

    /// Registra uma nova sessão
    pub fn register_session(&mut self, session_id: SessionId) {
        let stats = SessionStats::new(session_id);
        self.session_stats.insert(session_id, stats);
        self.global_stats.record_session_created();
    }

    /// Remove uma sessão do monitor
    pub fn unregister_session(&mut self, session_id: SessionId) {
        self.session_stats.remove(&session_id);
        self.global_stats.record_session_destroyed();
    }

    /// Obtém estatísticas de uma sessão
    pub fn get_session_stats(&self, session_id: SessionId) -> Option<&SessionStats> {
        self.session_stats.get(&session_id)
    }

    /// Obtém estatísticas mutáveis de uma sessão
    pub fn get_session_stats_mut(&mut self, session_id: SessionId) -> Option<&mut SessionStats> {
        self.session_stats.get_mut(&session_id)
    }

    /// Registra mudança de estado
    pub fn record_state_change(
        &mut self,
        session_id: SessionId,
        old_state: SessionState,
        new_state: SessionState,
    ) {
        let change = StateChange {
            timestamp: Instant::now(),
            session_id,
            old_state,
            new_state,
        };

        if self.state_history.len() >= self.max_history {
            self.state_history.remove(0);
        }
        self.state_history.push(change);
    }

    /// Registra atividade de I/O
    pub fn record_io(&mut self, session_id: SessionId, received: u64, sent: u64) {
        if let Some(stats) = self.session_stats.get_mut(&session_id) {
            stats.record_received(received);
            stats.record_sent(sent);
        }
        self.global_stats.record_bytes_transferred(received + sent);
    }

    /// Registra comando executado
    pub fn record_command(&mut self, session_id: SessionId) {
        if let Some(stats) = self.session_stats.get_mut(&session_id) {
            stats.record_command();
        }
    }

    /// Registra attach
    pub fn record_attach(&mut self, session_id: SessionId) {
        if let Some(stats) = self.session_stats.get_mut(&session_id) {
            stats.record_attach();
        }
    }

    /// Registra detach
    pub fn record_detach(&mut self, session_id: SessionId) {
        if let Some(stats) = self.session_stats.get_mut(&session_id) {
            stats.record_detach();
        }
    }

    /// Retorna estatísticas globais
    pub fn global_stats(&self) -> &GlobalStats {
        &self.global_stats
    }

    /// Retorna todas as estatísticas de sessões
    pub fn all_session_stats(&self) -> &HashMap<SessionId, SessionStats> {
        &self.session_stats
    }

    /// Retorna o histórico de estados
    pub fn state_history(&self) -> &[StateChange] {
        &self.state_history
    }

    /// Retorna sessões ordenadas por atividade
    pub fn sessions_by_activity(&self) -> Vec<(SessionId, &SessionStats)> {
        let mut sessions: Vec<_> = self
            .session_stats
            .iter()
            .map(|(id, stats)| (*id, stats))
            .collect();

        sessions.sort_by(|a, b| {
            let activity_a = a.1.bytes_received + a.1.bytes_sent;
            let activity_b = b.1.bytes_received + b.1.bytes_sent;
            activity_b.cmp(&activity_a)
        });

        sessions
    }

    /// Retorna sessões mais antigas
    pub fn oldest_sessions(&self, limit: usize) -> Vec<(SessionId, &SessionStats)> {
        let mut sessions: Vec<_> = self
            .session_stats
            .iter()
            .map(|(id, stats)| (*id, stats))
            .collect();

        sessions.sort_by_key(|(_, stats)| stats.started_at);
        sessions.truncate(limit);

        sessions
    }

    /// Limpa estatísticas antigas
    pub fn cleanup_old_stats(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.session_stats
            .retain(|_, stats| now.duration_since(stats.started_at) < max_age);
    }

    /// Gera relatório de saúde
    pub fn health_report(&self) -> HealthReport {
        let summary = self.global_stats.summary();

        HealthReport {
            healthy: true, // Simplificado - em uma implementação real, verificaria thresholds
            active_sessions: self.session_stats.len(),
            global_summary: summary,
            warnings: Vec::new(),
        }
    }
}

impl Default for SessionMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Relatório de saúde
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// Status de saúde
    pub healthy: bool,
    /// Sessões ativas
    pub active_sessions: usize,
    /// Resumo global
    pub global_summary: StatsSummary,
    /// Avisos
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_stats() {
        let id = SessionId::new();
        let mut stats = SessionStats::new(id);

        stats.record_received(100);
        stats.record_sent(50);
        stats.record_command();

        assert_eq!(stats.bytes_received, 100);
        assert_eq!(stats.bytes_sent, 50);
        assert_eq!(stats.commands_executed, 1);
    }

    #[test]
    fn test_global_stats() {
        let global = GlobalStats::new();

        global.record_session_created();
        global.record_session_created();
        global.record_client_connected();
        global.record_bytes_transferred(1000);

        let summary = global.summary();
        assert_eq!(summary.total_sessions_created, 2);
        assert_eq!(summary.total_clients_connected, 1);
        assert_eq!(summary.total_bytes_transferred, 1000);
    }

    #[test]
    fn test_session_monitor() {
        let mut monitor = SessionMonitor::new();
        let id = SessionId::new();

        monitor.register_session(id);
        monitor.record_io(id, 100, 50);
        monitor.record_command(id);
        monitor.record_attach(id);

        let stats = monitor.get_session_stats(id).unwrap();
        assert_eq!(stats.bytes_received, 100);
        assert_eq!(stats.bytes_sent, 50);
        assert_eq!(stats.commands_executed, 1);
        assert_eq!(stats.attach_count, 1);

        let global = monitor.global_stats();
        assert_eq!(global.total_sessions_created.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_state_change_history() {
        let mut monitor = SessionMonitor::with_max_history(10);
        let id = SessionId::new();

        monitor.record_state_change(id, SessionState::Creating, SessionState::Active);
        monitor.record_state_change(id, SessionState::Active, SessionState::Background);

        assert_eq!(monitor.state_history().len(), 2);
    }

    #[test]
    fn test_sessions_by_activity() {
        let mut monitor = SessionMonitor::new();
        let id1 = SessionId::new();
        let id2 = SessionId::new();

        monitor.register_session(id1);
        monitor.register_session(id2);

        monitor.record_io(id1, 100, 50);
        monitor.record_io(id2, 500, 200);

        let sorted = monitor.sessions_by_activity();
        assert_eq!(sorted[0].0, id2); // Mais ativa primeiro
        assert_eq!(sorted[1].0, id1);
    }

    #[test]
    fn test_health_report() {
        let mut monitor = SessionMonitor::new();
        let id = SessionId::new();

        monitor.register_session(id);

        let report = monitor.health_report();
        assert!(report.healthy);
        assert_eq!(report.active_sessions, 1);
    }
}
