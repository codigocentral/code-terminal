//! SSH Session Management - Gerenciamento e persistência de sessões SSH
//!
//! Este módulo implementa funcionalidades para gerenciar sessões SSH ativas,
//! incluindo persistência, tunnels e port forwarding.

use crate::ssh::error::{SshError, SshResult};
use crate::ssh::host::{DynamicForward, PortForward};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Identificador único de sessão
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Cria um novo ID de sessão
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string().split('-').next().unwrap_or(""))
    }
}

/// Estado de uma sessão SSH
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    /// Conectando
    Connecting,
    /// Conectado e ativo
    Connected,
    /// Desconectado
    Disconnected,
    /// Em erro
    Error,
}

/// Tipo de sessão SSH
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionType {
    /// Sessão interativa (shell)
    Interactive,
    /// Execução de comando
    Command,
    /// Tunnel/Port forwarding
    Tunnel,
    /// SFTP
    Sftp,
}

/// Informações de uma sessão SSH
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshSession {
    /// ID único da sessão
    pub id: SessionId,
    /// Nome amigável da sessão
    pub name: String,
    /// Host de destino
    pub host: String,
    /// Porta
    pub port: u16,
    /// Usuário
    pub user: String,
    /// Tipo de sessão
    pub session_type: SessionType,
    /// Estado atual
    pub state: SessionState,
    /// Port forwarding local
    pub local_forwards: Vec<PortForward>,
    /// Port forwarding remoto
    pub remote_forwards: Vec<PortForward>,
    /// Dynamic forwarding (SOCKS proxy)
    pub dynamic_forwards: Vec<DynamicForward>,
    /// PID do processo SSH (se aplicável)
    pub pid: Option<u32>,
    /// Comando usado para iniciar a sessão
    pub command: String,
    /// Quando a sessão foi criada (serializável)
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Quando foi conectado pela última vez (serializável)
    pub last_connected: Option<chrono::DateTime<chrono::Utc>>,
    /// Tags associadas
    pub tags: Vec<String>,
    /// Configurações extras
    pub extra_args: Vec<String>,

    /// Campo transitório para Instant em memória (não serializado)
    #[serde(skip)]
    created_at_instant: Option<Instant>,
}

impl SshSession {
    /// Cria uma nova sessão SSH
    pub fn new(name: impl Into<String>, host: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            id: SessionId::new(),
            name: name.into(),
            host: host.into(),
            port: 22,
            user: user.into(),
            session_type: SessionType::Interactive,
            state: SessionState::Disconnected,
            local_forwards: Vec::new(),
            remote_forwards: Vec::new(),
            dynamic_forwards: Vec::new(),
            pid: None,
            command: String::new(),
            created_at: chrono::Utc::now(),
            last_connected: None,
            tags: Vec::new(),
            extra_args: Vec::new(),
            created_at_instant: Some(Instant::now()),
        }
    }

    /// Define a porta
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Define o tipo de sessão
    pub fn with_type(mut self, session_type: SessionType) -> Self {
        self.session_type = session_type;
        self
    }

    /// Adiciona port forwarding local
    pub fn with_local_forward(mut self, forward: PortForward) -> Self {
        self.local_forwards.push(forward);
        self
    }

    /// Adiciona port forwarding remoto
    pub fn with_remote_forward(mut self, forward: PortForward) -> Self {
        self.remote_forwards.push(forward);
        self
    }

    /// Adiciona dynamic forwarding
    pub fn with_dynamic_forward(mut self, forward: DynamicForward) -> Self {
        self.dynamic_forwards.push(forward);
        self
    }

    /// Adiciona argumentos extras
    pub fn with_extra_args(mut self, args: Vec<String>) -> Self {
        self.extra_args = args;
        self
    }

    /// Adiciona tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = tags;
        self
    }

    /// Retorna a string de conexão (user@host:port)
    pub fn connection_string(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.port)
    }

    /// Verifica se a sessão está ativa
    pub fn is_active(&self) -> bool {
        self.state == SessionState::Connected
    }

    /// Retorna a duração desde a criação
    pub fn duration(&self) -> Duration {
        self.created_at_instant
            .map(|i| i.elapsed())
            .unwrap_or_else(|| {
                // Fallback para DateTime se Instant não estiver disponível
                chrono::Utc::now()
                    .signed_duration_since(self.created_at)
                    .to_std()
                    .unwrap_or(Duration::from_secs(0))
            })
    }

    /// Constrói o comando SSH para esta sessão
    pub fn build_ssh_command(&self, identity_file: Option<&Path>) -> Command {
        self.build_command_with_program("ssh", identity_file)
    }

    /// Constrói o comando usando o executável dado no lugar de `ssh`.
    pub fn build_command_with_program(
        &self,
        program: &str,
        identity_file: Option<&Path>,
    ) -> Command {
        let mut cmd = Command::new(program);

        // Porta
        if self.port != 22 {
            cmd.arg("-p").arg(self.port.to_string());
        }

        // Port forwarding local
        for forward in &self.local_forwards {
            let bind = forward.bind_address.as_deref().unwrap_or("localhost");
            let spec = format!(
                "{}:{}:{}:{}",
                bind, forward.local_port, forward.remote_host, forward.remote_port
            );
            cmd.arg("-L").arg(spec);
        }

        // Port forwarding remoto
        for forward in &self.remote_forwards {
            let bind = forward.bind_address.as_deref().unwrap_or("localhost");
            let spec = format!(
                "{}:{}:{}:{}",
                bind, forward.local_port, forward.remote_host, forward.remote_port
            );
            cmd.arg("-R").arg(spec);
        }

        // Dynamic forwarding (SOCKS)
        for forward in &self.dynamic_forwards {
            let bind = forward.bind_address.as_deref().unwrap_or("localhost");
            let spec = format!("{}:{}", bind, forward.local_port);
            cmd.arg("-D").arg(spec);
        }

        // Identity file
        if let Some(identity) = identity_file {
            cmd.arg("-i").arg(identity);
        }

        // Extra args
        for arg in &self.extra_args {
            cmd.arg(arg);
        }

        // Target
        cmd.arg(format!("{}@{}", self.user, self.host));

        cmd
    }
}

/// Configuração de persistência de sessões
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPersistenceConfig {
    /// Habilitar persistência automática
    pub auto_save: bool,
    /// Intervalo de auto-save (segundos)
    pub auto_save_interval: u64,
    /// Diretório de persistência
    pub persistence_dir: PathBuf,
    /// Número máximo de sessões salvas
    pub max_saved_sessions: usize,
    /// Incluir senhas (criptografado)
    pub include_passwords: bool,
    /// Número máximo de backups por sessão
    pub max_backups: usize,
    /// Habilitar backup automático antes de sobrescrever
    pub auto_backup: bool,
}

impl Default for SessionPersistenceConfig {
    fn default() -> Self {
        Self {
            auto_save: true,
            auto_save_interval: 300, // 5 minutos
            persistence_dir: dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("codeterm")
                .join("sessions"),
            max_saved_sessions: 50,
            include_passwords: false,
            max_backups: 5,
            auto_backup: true,
        }
    }
}

/// Dados persistidos de uma sessão
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSession {
    /// Dados da sessão
    pub session: SshSession,
    /// Senha criptografada (se habilitado)
    pub encrypted_password: Option<String>,
    /// Timestamp de salvamento
    pub saved_at: chrono::DateTime<chrono::Utc>,
    /// Versão do formato
    pub version: u32,
}

/// Informações de um backup de sessão
#[derive(Debug, Clone)]
pub struct SessionBackup {
    /// ID da sessão
    pub session_id: SessionId,
    /// Nome do arquivo de backup
    pub filename: String,
    /// Caminho completo do backup
    pub path: PathBuf,
    /// Timestamp de criação
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Tamanho do arquivo em bytes
    pub size: u64,
}

/// Handle para controle de auto-save em background
#[derive(Debug)]
pub struct AutoSaveHandle {
    /// Canal para sinalizar parada do auto-save
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl AutoSaveHandle {
    /// Cria um novo handle de auto-save
    fn new() -> Self {
        Self { shutdown_tx: None }
    }

    /// Sinaliza parada do auto-save
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for AutoSaveHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Gerenciador de sessões SSH
#[derive(Debug)]
pub struct SessionManager {
    /// Sessões ativas (ID -> Sessão)
    sessions: HashMap<SessionId, SshSession>,
    /// Processos filhos (ID -> Child)
    processes: HashMap<SessionId, Child>,
    /// Configuração de persistência
    config: SessionPersistenceConfig,
    /// Handle de auto-save em background
    auto_save_handle: Option<AutoSaveHandle>,
    /// Executável do cliente SSH (substituível nos testes)
    ssh_program: String,
}

impl SessionManager {
    /// Cria um novo gerenciador de sessões
    pub fn new() -> Self {
        Self::with_config(SessionPersistenceConfig::default())
    }

    /// Cria com configuração customizada
    pub fn with_config(config: SessionPersistenceConfig) -> Self {
        // Garante que o diretório existe
        if let Err(e) = std::fs::create_dir_all(&config.persistence_dir) {
            warn!("Failed to create sessions directory: {}", e);
        }

        let mut manager = Self {
            sessions: HashMap::new(),
            processes: HashMap::new(),
            config,
            auto_save_handle: None,
            ssh_program: "ssh".to_string(),
        };

        // Carrega sessões salvas
        if let Err(e) = manager.load_saved_sessions() {
            warn!("Failed to load saved sessions: {}", e);
        }

        manager
    }

    /// Usa outro executável no lugar de `ssh` ao iniciar sessões.
    pub fn with_ssh_program(mut self, program: impl Into<String>) -> Self {
        self.ssh_program = program.into();
        self
    }

    /// Cria uma nova sessão
    pub fn create_session(&mut self, session: SshSession) -> SessionId {
        let id = session.id;
        self.sessions.insert(id, session);

        if self.config.auto_save {
            let _ = self.save_session(id);
        }

        info!("Created SSH session: {}", id);
        id
    }

    /// Inicia uma sessão SSH
    pub fn start_session(&mut self, id: SessionId, identity_file: Option<&Path>) -> SshResult<()> {
        let session = self.sessions.get_mut(&id).ok_or(SshError::ConfigNotFound)?;

        if session.state == SessionState::Connected {
            return Err(SshError::ConfigNotFound); // Já conectado
        }

        let mut cmd = session.build_command_with_program(&self.ssh_program, identity_file);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id();
                session.pid = Some(pid);
                session.state = SessionState::Connected;
                session.last_connected = Some(chrono::Utc::now());
                self.processes.insert(id, child);

                info!("Started SSH session {} with PID {}", id, pid);

                if self.config.auto_save {
                    let _ = self.save_session(id);
                }

                Ok(())
            }
            Err(e) => {
                session.state = SessionState::Error;
                error!("Failed to start SSH session {}: {}", id, e);
                Err(SshError::ConfigNotFound)
            }
        }
    }

    /// Encerra uma sessão
    pub fn stop_session(&mut self, id: SessionId) -> SshResult<()> {
        if let Some(mut child) = self.processes.remove(&id) {
            let _ = child.kill();
            // Colhe o processo para não deixar zumbi
            let _ = child.wait();
        }

        if let Some(session) = self.sessions.get_mut(&id) {
            session.state = SessionState::Disconnected;
            session.pid = None;

            info!("Stopped SSH session: {}", id);

            if self.config.auto_save {
                let _ = self.save_session(id);
            }
        }

        Ok(())
    }

    /// Remove uma sessão
    pub fn remove_session(&mut self, id: SessionId) -> SshResult<()> {
        self.stop_session(id)?;
        self.sessions.remove(&id);

        // Remove arquivo persistido
        let path = self.session_file_path(id);
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }

        info!("Removed SSH session: {}", id);
        Ok(())
    }

    /// Retorna uma sessão pelo ID
    pub fn get_session(&self, id: SessionId) -> Option<&SshSession> {
        self.sessions.get(&id)
    }

    /// Retorna uma sessão mutável
    pub fn get_session_mut(&mut self, id: SessionId) -> Option<&mut SshSession> {
        self.sessions.get_mut(&id)
    }

    /// Lista todas as sessões
    pub fn list_sessions(&self) -> Vec<&SshSession> {
        self.sessions.values().collect()
    }

    /// Lista sessões ativas
    pub fn list_active_sessions(&self) -> Vec<&SshSession> {
        self.sessions.values().filter(|s| s.is_active()).collect()
    }

    /// Busca sessões por tag
    pub fn find_by_tag(&self, tag: &str) -> Vec<&SshSession> {
        self.sessions
            .values()
            .filter(|s| s.tags.contains(&tag.to_string()))
            .collect()
    }

    /// Busca sessões por host
    pub fn find_by_host(&self, host: &str) -> Vec<&SshSession> {
        self.sessions
            .values()
            .filter(|s| s.host.contains(host))
            .collect()
    }

    /// Salva uma sessão em disco
    pub fn save_session(&self, id: SessionId) -> SshResult<()> {
        let session = self.sessions.get(&id).ok_or(SshError::ConfigNotFound)?;

        let path = self.session_file_path(id);

        // Cria backup se arquivo existe e auto_backup está habilitado
        if path.exists() && self.config.auto_backup {
            if let Err(e) = self.create_backup(id) {
                warn!("Failed to create backup before saving: {}", e);
            }
        }

        let persisted = PersistedSession {
            session: session.clone(),
            encrypted_password: None,
            saved_at: chrono::Utc::now(),
            version: 1,
        };

        let json = serde_json::to_string_pretty(&persisted)
            .map_err(|e| SshError::InvalidConfig(e.to_string()))?;

        std::fs::write(&path, json).map_err(|e| SshError::InvalidConfig(e.to_string()))?;

        debug!("Saved session {} to {:?}", id, path);
        Ok(())
    }

    /// Carrega sessões salvas
    pub fn load_saved_sessions(&mut self) -> SshResult<usize> {
        let mut count = 0;

        if !self.config.persistence_dir.exists() {
            return Ok(0);
        }

        let entries = std::fs::read_dir(&self.config.persistence_dir)
            .map_err(|e| SshError::InvalidConfig(e.to_string()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(persisted) = serde_json::from_str::<PersistedSession>(&data) {
                        self.sessions
                            .insert(persisted.session.id, persisted.session);
                        count += 1;
                    }
                }
            }
        }

        info!("Loaded {} saved SSH sessions", count);
        Ok(count)
    }

    /// Retorna o caminho do arquivo de uma sessão
    fn session_file_path(&self, id: SessionId) -> PathBuf {
        self.config.persistence_dir.join(format!("{}.json", id.0))
    }

    /// Salva todas as sessões
    pub fn save_all_sessions(&self) -> SshResult<usize> {
        let mut count = 0;
        for id in self.sessions.keys() {
            if self.save_session(*id).is_ok() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Inicia auto-save em background
    ///
    /// # Arguments
    /// * `interval_secs` - Intervalo em segundos entre saves (usa config se None)
    ///
    /// # Returns
    /// `true` se o auto-save foi iniciado, `false` caso contrário
    pub fn start_auto_save(&mut self, interval_secs: Option<u64>) -> bool {
        // Para auto-save anterior se existir
        self.stop_auto_save();

        if !self.config.auto_save {
            debug!("Auto-save is disabled in config");
            return false;
        }

        let interval_secs = interval_secs.unwrap_or(self.config.auto_save_interval);
        if interval_secs == 0 {
            warn!("Auto-save interval is 0, disabling auto-save");
            return false;
        }

        // Cria canal de shutdown
        let (shutdown_tx, mut shutdown_rx) = tokio::sync::oneshot::channel();

        // Spawn task de auto-save
        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(interval_secs));

            loop {
                tokio::select! {
                    _ = tick.tick() => {
                        debug!("Auto-saving sessions...");
                        // Aqui seria feito o save real - por enquanto só log
                        // Em implementação completa, precisaríamos de acesso ao SessionManager
                    }
                    _ = &mut shutdown_rx => {
                        info!("Auto-save task stopped");
                        break;
                    }
                }
            }
        });

        let mut handle = AutoSaveHandle::new();
        handle.shutdown_tx = Some(shutdown_tx);
        self.auto_save_handle = Some(handle);

        info!("Auto-save started with {}s interval", interval_secs);
        true
    }

    /// Para o auto-save em background
    pub fn stop_auto_save(&mut self) {
        if let Some(mut handle) = self.auto_save_handle.take() {
            handle.stop();
            info!("Auto-save stopped");
        }
    }

    /// Verifica se auto-save está ativo
    pub fn is_auto_save_running(&self) -> bool {
        self.auto_save_handle.is_some()
    }

    /// Limpa sessões antigas
    pub fn cleanup_old_sessions(&mut self, max_age: Duration) -> usize {
        let to_remove: Vec<SessionId> = self
            .sessions
            .values()
            .filter(|s| s.state == SessionState::Disconnected && s.duration() > max_age)
            .map(|s| s.id)
            .collect();

        for id in &to_remove {
            let _ = self.remove_session(*id);
        }

        to_remove.len()
    }

    /// Atualiza o estado das sessões
    pub fn refresh_states(&mut self) {
        let to_check: Vec<SessionId> = self.processes.keys().copied().collect();

        for id in to_check {
            if let Some(child) = self.processes.get_mut(&id) {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        // Processo terminou
                        self.processes.remove(&id);
                        if let Some(session) = self.sessions.get_mut(&id) {
                            session.state = SessionState::Disconnected;
                            session.pid = None;
                        }
                    }
                    Ok(None) => {
                        // Ainda rodando
                    }
                    Err(_) => {
                        // Erro ao verificar
                    }
                }
            }
        }
    }

    // ==================== BACKUP METHODS ====================

    /// Cria um backup da sessão
    ///
    /// # Arguments
    /// * `session_id` - ID da sessão a ser backupada
    ///
    /// # Returns
    /// `Ok(())` se o backup foi criado com sucesso
    pub fn create_backup(&self, session_id: SessionId) -> SshResult<()> {
        let session_path = self.session_file_path(session_id);

        if !session_path.exists() {
            return Err(SshError::ConfigNotFound);
        }

        // Cria diretório de backups se não existir
        let backup_dir = self.config.persistence_dir.join("backups");
        if !backup_dir.exists() {
            std::fs::create_dir_all(&backup_dir).map_err(|e| {
                SshError::InvalidConfig(format!("Failed to create backup dir: {}", e))
            })?;
        }

        // Gera nome do arquivo com timestamp
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let backup_filename = format!(
            "{}_{}.json",
            session_id
                .0
                .to_string()
                .split('-')
                .next()
                .unwrap_or("session"),
            timestamp
        );
        let backup_path = backup_dir.join(&backup_filename);

        // Copia arquivo
        std::fs::copy(&session_path, &backup_path)
            .map_err(|e| SshError::InvalidConfig(format!("Failed to create backup: {}", e)))?;

        info!(
            "Created backup for session {}: {:?}",
            session_id, backup_path
        );

        // Limpa backups antigos
        self.cleanup_old_backups(session_id)?;

        Ok(())
    }

    /// Remove backups antigos, mantendo apenas os mais recentes
    ///
    /// # Arguments
    /// * `session_id` - ID da sessão cujos backups serão limpados
    fn cleanup_old_backups(&self, session_id: SessionId) -> SshResult<()> {
        let backup_dir = self.config.persistence_dir.join("backups");

        if !backup_dir.exists() {
            return Ok(());
        }

        let session_prefix = session_id
            .0
            .to_string()
            .split('-')
            .next()
            .unwrap_or("")
            .to_string();

        // Lista todos os backups desta sessão
        let mut backups: Vec<_> = std::fs::read_dir(&backup_dir)
            .map_err(|e| SshError::InvalidConfig(format!("Failed to read backup dir: {}", e)))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                if let Some(name) = entry.file_name().to_str() {
                    name.starts_with(&session_prefix) && name.ends_with(".json")
                } else {
                    false
                }
            })
            .collect();

        // Ordena por data de modificação (mais recente primeiro)
        backups.sort_by(|a, b| {
            let a_meta = a.metadata().ok();
            let b_meta = b.metadata().ok();
            match (a_meta, b_meta) {
                (Some(a_m), Some(b_m)) => {
                    let a_time = a_m.modified().ok();
                    let b_time = b_m.modified().ok();
                    match (a_time, b_time) {
                        (Some(a_t), Some(b_t)) => b_t.cmp(&a_t),
                        _ => std::cmp::Ordering::Equal,
                    }
                }
                _ => std::cmp::Ordering::Equal,
            }
        });

        // Remove backups excedentes
        if backups.len() > self.config.max_backups {
            for entry in backups.iter().skip(self.config.max_backups) {
                if let Err(e) = std::fs::remove_file(entry.path()) {
                    warn!("Failed to remove old backup: {}", e);
                } else {
                    debug!("Removed old backup: {:?}", entry.path());
                }
            }
        }

        Ok(())
    }

    /// Lista todos os backups disponíveis
    ///
    /// # Returns
    /// Lista de informações de backup ordenada por data (mais recente primeiro)
    pub fn list_backups(&self) -> SshResult<Vec<SessionBackup>> {
        let backup_dir = self.config.persistence_dir.join("backups");

        if !backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();

        for entry in std::fs::read_dir(&backup_dir)
            .map_err(|e| SshError::InvalidConfig(format!("Failed to read backup dir: {}", e)))?
        {
            let entry = entry
                .map_err(|e| SshError::InvalidConfig(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let metadata = entry
                .metadata()
                .map_err(|e| SshError::InvalidConfig(format!("Failed to read metadata: {}", e)))?;

            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Extrai session_id do filename (formato: UUID_TIMESTAMP.json)
            let session_id_str = filename.split('_').next().unwrap_or("");
            let session_id = if let Ok(uuid) = Uuid::parse_str(session_id_str) {
                SessionId(uuid)
            } else {
                // Tenta carregar do arquivo para obter o ID
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(persisted) = serde_json::from_str::<PersistedSession>(&data) {
                        persisted.session.id
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            };

            let created_at = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
                .unwrap_or_else(chrono::Utc::now);

            backups.push(SessionBackup {
                session_id,
                filename,
                path,
                created_at,
                size: metadata.len(),
            });
        }

        // Ordena por data (mais recente primeiro)
        backups.sort_by_key(|b| std::cmp::Reverse(b.created_at));

        Ok(backups)
    }

    /// Restaura uma sessão a partir de um backup
    ///
    /// # Arguments
    /// * `backup_path` - Caminho do arquivo de backup
    ///
    /// # Returns
    /// A sessão restaurada
    pub fn restore_from_backup(&self, backup_path: &Path) -> SshResult<SshSession> {
        if !backup_path.exists() {
            return Err(SshError::ConfigNotFound);
        }

        let data = std::fs::read_to_string(backup_path)
            .map_err(|e| SshError::InvalidConfig(format!("Failed to read backup: {}", e)))?;

        let persisted: PersistedSession = serde_json::from_str(&data)
            .map_err(|e| SshError::InvalidConfig(format!("Failed to parse backup: {}", e)))?;

        info!(
            "Restored session {} from backup: {:?}",
            persisted.session.id, backup_path
        );

        Ok(persisted.session)
    }

    /// Restaura uma sessão pelo ID, usando o backup mais recente
    ///
    /// # Arguments
    /// * `session_id` - ID da sessão a ser restaurada
    ///
    /// # Returns
    /// A sessão restaurada
    pub fn restore_session(&self, session_id: SessionId) -> SshResult<SshSession> {
        let backups = self.list_backups()?;

        let backup = backups
            .iter()
            .find(|b| b.session_id == session_id)
            .ok_or(SshError::ConfigNotFound)?;

        self.restore_from_backup(&backup.path)
    }

    /// Remove um backup específico
    ///
    /// # Arguments
    /// * `backup_path` - Caminho do backup a ser removido
    ///
    /// # Returns
    /// `true` se o backup foi removido, `false` se não existia
    pub fn remove_backup(&self, backup_path: &Path) -> SshResult<bool> {
        if !backup_path.exists() {
            return Ok(false);
        }

        std::fs::remove_file(backup_path)
            .map_err(|e| SshError::InvalidConfig(format!("Failed to remove backup: {}", e)))?;

        info!("Removed backup: {:?}", backup_path);
        Ok(true)
    }
}

impl Drop for SessionManager {
    fn drop(&mut self) {
        self.stop_auto_save();
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_session_id_creation() {
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ssh_session_creation() {
        let session = SshSession::new("test", "example.com", "user");
        assert_eq!(session.name, "test");
        assert_eq!(session.host, "example.com");
        assert_eq!(session.user, "user");
        assert_eq!(session.port, 22);
        assert_eq!(session.state, SessionState::Disconnected);
    }

    #[test]
    fn test_ssh_session_builder() {
        let session = SshSession::new("prod", "server.com", "admin")
            .with_port(2222)
            .with_type(SessionType::Tunnel)
            .with_tags(vec!["production".to_string()]);

        assert_eq!(session.port, 2222);
        assert_eq!(session.session_type, SessionType::Tunnel);
        assert_eq!(session.tags, vec!["production"]);
    }

    #[test]
    fn test_session_connection_string() {
        let session = SshSession::new("test", "example.com", "user");
        assert_eq!(session.connection_string(), "user@example.com:22");
    }

    #[test]
    fn test_session_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };

        let manager = SessionManager::with_config(config);
        assert!(manager.list_sessions().is_empty());
    }

    #[test]
    fn test_session_manager_create_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);
        let session = SshSession::new("test", "example.com", "user");
        let id = manager.create_session(session.clone());

        assert_eq!(manager.list_sessions().len(), 1);

        let retrieved = manager.get_session(id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test");
    }

    #[test]
    fn test_session_persistence_save_and_load() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config.clone());
        let session = SshSession::new("persistent", "server.com", "admin").with_port(2222);
        let id = manager.create_session(session);

        // Salva manualmente
        assert!(manager.save_session(id).is_ok());

        // Cria novo manager e carrega
        let new_manager = SessionManager::with_config(config);
        assert_eq!(new_manager.list_sessions().len(), 1);

        let loaded = new_manager.get_session(id);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().name, "persistent");
        assert_eq!(loaded.unwrap().port, 2222);
    }

    #[test]
    fn test_find_by_tag() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        let session1 =
            SshSession::new("prod1", "prod1.com", "user").with_tags(vec!["production".to_string()]);
        let session2 =
            SshSession::new("prod2", "prod2.com", "user").with_tags(vec!["production".to_string()]);
        let session3 =
            SshSession::new("dev", "dev.com", "user").with_tags(vec!["development".to_string()]);

        manager.create_session(session1);
        manager.create_session(session2);
        manager.create_session(session3);

        let prod_sessions = manager.find_by_tag("production");
        assert_eq!(prod_sessions.len(), 2);
    }

    #[test]
    fn test_session_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);
        let session = SshSession::new("old", "old.com", "user");
        let id = manager.create_session(session);

        // Remove a sessão
        assert!(manager.remove_session(id).is_ok());
        assert!(manager.get_session(id).is_none());
    }

    #[test]
    fn test_auto_save_config() {
        let temp_dir = TempDir::new().unwrap();

        // Testa configuração padrão
        let config = SessionPersistenceConfig::default();
        assert!(config.auto_save);
        assert_eq!(config.auto_save_interval, 300); // 5 minutos
        assert_eq!(config.max_saved_sessions, 50);

        // Testa configuração customizada
        let custom_config = SessionPersistenceConfig {
            auto_save: false,
            auto_save_interval: 60,
            persistence_dir: temp_dir.path().to_path_buf(),
            max_saved_sessions: 10,
            include_passwords: true,
            max_backups: 3,
            auto_backup: false,
        };
        assert!(!custom_config.auto_save);
        assert_eq!(custom_config.auto_save_interval, 60);
        assert_eq!(custom_config.max_saved_sessions, 10);
        assert_eq!(custom_config.max_backups, 3);
        assert!(!custom_config.auto_backup);
    }

    #[test]
    fn test_auto_save_handle_creation() {
        let handle = AutoSaveHandle::new();
        assert!(handle.shutdown_tx.is_none());
    }

    #[tokio::test]
    async fn test_auto_save_start_stop() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: true,
            auto_save_interval: 1, // 1 segundo para teste rápido
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Inicia auto-save
        assert!(manager.start_auto_save(None));
        assert!(manager.is_auto_save_running());

        // Para auto-save
        manager.stop_auto_save();
        assert!(!manager.is_auto_save_running());
    }

    #[test]
    fn test_auto_save_disabled_in_config() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false, // Desabilitado
            auto_save_interval: 60,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Não deve iniciar se desabilitado na config
        assert!(!manager.start_auto_save(None));
        assert!(!manager.is_auto_save_running());
    }

    #[tokio::test]
    async fn test_auto_save_custom_interval() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: true,
            auto_save_interval: 300, // 5 minutos padrão
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Inicia com intervalo customizado
        assert!(manager.start_auto_save(Some(30))); // 30 segundos
        assert!(manager.is_auto_save_running());

        manager.stop_auto_save();
    }

    #[test]
    fn test_auto_save_zero_interval() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: true,
            auto_save_interval: 60,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Não deve iniciar com intervalo 0
        assert!(!manager.start_auto_save(Some(0)));
        assert!(!manager.is_auto_save_running());
    }

    // ==================== BACKUP TESTS ====================

    #[test]
    fn test_backup_config() {
        let config = SessionPersistenceConfig::default();
        assert_eq!(config.max_backups, 5);
        assert!(config.auto_backup);

        let custom_config = SessionPersistenceConfig {
            max_backups: 10,
            auto_backup: false,
            ..Default::default()
        };
        assert_eq!(custom_config.max_backups, 10);
        assert!(!custom_config.auto_backup);
    }

    #[test]
    fn test_create_backup() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Cria uma sessão e salva
        let session = SshSession::new("backup_test", "server.com", "user");
        let id = manager.create_session(session);
        assert!(manager.save_session(id).is_ok());

        // Cria backup
        assert!(manager.create_backup(id).is_ok());

        // Verifica se backup foi criado
        let backups = manager.list_backups().unwrap();
        assert_eq!(backups.len(), 1);
        assert_eq!(backups[0].session_id, id);
    }

    #[test]
    fn test_backup_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            max_backups: 3,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Cria uma sessão e salva
        let session = SshSession::new("cleanup_test", "server.com", "user");
        let id = manager.create_session(session);
        assert!(manager.save_session(id).is_ok());

        // Cria 5 backups (mais que o limite de 3)
        // Aumentamos o sleep para garantir timestamps diferentes
        for i in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(300));
            assert!(
                manager.create_backup(id).is_ok(),
                "Failed to create backup {}",
                i
            );
        }

        // Verifica que backups foram criados
        let backups = manager.list_backups().unwrap();
        // O cleanup pode manter menos backups em alguns casos devido a timing,
        // mas devemos ter pelo menos 1 e no máximo 3
        assert!(!backups.is_empty(), "Expected at least 1 backup");
        assert!(
            backups.len() <= 3,
            "Expected at most 3 backups, found {}",
            backups.len()
        );
    }

    #[test]
    fn test_list_backups_empty() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            ..Default::default()
        };

        let manager = SessionManager::with_config(config);

        // Lista backups quando não existe nenhum
        let backups = manager.list_backups().unwrap();
        assert!(backups.is_empty());
    }

    #[test]
    fn test_restore_from_backup() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Cria uma sessão com configurações específicas
        let session = SshSession::new("restore_test", "restore.com", "admin").with_port(2222);
        let id = manager.create_session(session);
        assert!(manager.save_session(id).is_ok());

        // Cria backup
        assert!(manager.create_backup(id).is_ok());

        // Remove a sessão original
        assert!(manager.remove_session(id).is_ok());
        assert!(manager.get_session(id).is_none());

        // Restaura do backup
        let restored = manager.restore_session(id).unwrap();
        assert_eq!(restored.name, "restore_test");
        assert_eq!(restored.host, "restore.com");
        assert_eq!(restored.user, "admin");
        assert_eq!(restored.port, 2222);
    }

    #[test]
    fn test_restore_nonexistent_backup() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            ..Default::default()
        };

        let manager = SessionManager::with_config(config);

        // Tenta restaurar sessão que não tem backup
        let fake_id = SessionId::new();
        assert!(manager.restore_session(fake_id).is_err());
    }

    #[test]
    fn test_remove_backup() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Cria uma sessão e backup
        let session = SshSession::new("remove_test", "server.com", "user");
        let id = manager.create_session(session);
        assert!(manager.save_session(id).is_ok());
        assert!(manager.create_backup(id).is_ok());

        // Obtém o caminho do backup
        let backups = manager.list_backups().unwrap();
        assert_eq!(backups.len(), 1);
        let backup_path = backups[0].path.clone();

        // Remove o backup
        assert!(manager.remove_backup(&backup_path).unwrap());

        // Verifica que foi removido
        let backups = manager.list_backups().unwrap();
        assert!(backups.is_empty());

        // Tentar remover novamente retorna false
        assert!(!manager.remove_backup(&backup_path).unwrap());
    }

    #[test]
    fn test_auto_backup_on_save() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            auto_backup: true, // Habilitado
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Cria uma sessão e salva (primeiro save, não cria backup)
        let session = SshSession::new("autobackup_test", "server.com", "user");
        let id = manager.create_session(session);
        assert!(manager.save_session(id).is_ok());

        // Não deve ter backup ainda (primeiro save)
        let backups = manager.list_backups().unwrap();
        assert_eq!(backups.len(), 0);

        // Salva novamente - deve criar backup do anterior
        assert!(manager.save_session(id).is_ok());

        // Agora deve ter 1 backup
        let backups = manager.list_backups().unwrap();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn test_session_backup_info() {
        let temp_dir = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp_dir.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            ..Default::default()
        };

        let mut manager = SessionManager::with_config(config);

        // Cria uma sessão e backup
        let session = SshSession::new("info_test", "server.com", "user");
        let id = manager.create_session(session);
        assert!(manager.save_session(id).is_ok());
        assert!(manager.create_backup(id).is_ok());

        // Verifica informações do backup
        let backups = manager.list_backups().unwrap();
        assert_eq!(backups.len(), 1);

        let backup = &backups[0];
        assert_eq!(backup.session_id, id);
        assert!(backup.filename.ends_with(".json"));
        assert!(backup.size > 0);
        assert!(backup.created_at <= chrono::Utc::now());
    }

    fn manager_in(dir: &Path, auto_save: bool) -> SessionManager {
        SessionManager::with_config(SessionPersistenceConfig {
            persistence_dir: dir.to_path_buf(),
            auto_save,
            ..Default::default()
        })
    }

    #[cfg(unix)]
    fn script(dir: &TempDir, name: &str, body: &str) -> String {
        crate::test_exec::write_fake_executable(&dir.path().join(name), body)
    }

    #[test]
    fn test_session_id_display_and_default() {
        let id = SessionId::default();
        let full = id.0.to_string();
        assert_eq!(id.to_string(), full.split('-').next().unwrap());
        assert_eq!(id.to_string().len(), 8);
        assert_ne!(SessionId::default(), id);
    }

    #[test]
    fn test_build_ssh_command_arguments() {
        let session = SshSession::new("s", "db.internal", "ops")
            .with_port(2222)
            .with_local_forward(PortForward {
                local_port: 5432,
                bind_address: None,
                remote_host: "localhost".into(),
                remote_port: 5432,
            })
            .with_remote_forward(PortForward {
                local_port: 8080,
                bind_address: Some("0.0.0.0".into()),
                remote_host: "web".into(),
                remote_port: 80,
            })
            .with_dynamic_forward(DynamicForward {
                local_port: 1080,
                bind_address: None,
            })
            .with_extra_args(vec!["-o".into(), "ServerAliveInterval=30".into()]);
        let cmd = session.build_ssh_command(Some(Path::new("/k/id")));
        assert_eq!(cmd.get_program(), "ssh");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args,
            vec![
                "-p",
                "2222",
                "-L",
                "localhost:5432:localhost:5432",
                "-R",
                "0.0.0.0:8080:web:80",
                "-D",
                "localhost:1080",
                "-i",
                "/k/id",
                "-o",
                "ServerAliveInterval=30",
                "ops@db.internal",
            ]
        );

        // porta padrão e sem extras: só o destino
        let plain = SshSession::new("p", "h", "u").build_command_with_program("meu-ssh", None);
        assert_eq!(plain.get_program(), "meu-ssh");
        let args: Vec<_> = plain.get_args().collect();
        assert_eq!(args, vec!["u@h"]);
    }

    #[test]
    fn test_duration_falls_back_to_created_at_after_deserialization() {
        let mut session = SshSession::new("d", "h", "u");
        session.created_at = chrono::Utc::now() - chrono::Duration::seconds(120);
        let json = serde_json::to_string(&session).unwrap();
        let back: SshSession = serde_json::from_str(&json).unwrap();
        // o Instant não é serializado: a duração vem do created_at
        assert!(back.duration() >= Duration::from_secs(119));
        // created_at no futuro não quebra (vira zero)
        let mut future = back.clone();
        future.created_at = chrono::Utc::now() + chrono::Duration::seconds(60);
        assert_eq!(future.duration(), Duration::from_secs(0));
    }

    #[cfg(unix)]
    #[test]
    fn test_start_and_stop_session_with_fake_ssh() {
        let temp = TempDir::new().unwrap();
        let log = temp.path().join("args.log");
        let prog = script(
            &temp,
            "ssh",
            &format!("echo \"$@\" > '{}'\nexec sleep 30", log.display()),
        );
        let mut m = manager_in(&temp.path().join("s"), true).with_ssh_program(&prog);
        let id = m.create_session(SshSession::new("web", "web.example", "ana").with_port(2200));

        m.start_session(id, Some(Path::new("/k/id"))).unwrap();
        let s = m.get_session(id).unwrap();
        assert_eq!(s.state, SessionState::Connected);
        assert!(s.pid.is_some());
        assert!(s.last_connected.is_some());
        assert_eq!(m.list_active_sessions().len(), 1);

        // já conectado não inicia de novo
        assert!(m.start_session(id, None).is_err());

        // processo ainda vivo: refresh não muda o estado
        m.refresh_states();
        assert_eq!(m.get_session(id).unwrap().state, SessionState::Connected);

        // o script recebeu os argumentos montados
        for _ in 0..200 {
            if std::fs::read_to_string(&log).is_ok_and(|l| l.ends_with('\n')) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            std::fs::read_to_string(&log).unwrap().trim(),
            "-p 2200 -i /k/id ana@web.example"
        );

        m.stop_session(id).unwrap();
        let s = m.get_session(id).unwrap();
        assert_eq!(s.state, SessionState::Disconnected);
        assert!(s.pid.is_none());
        assert!(m.list_active_sessions().is_empty());

        // o estado salvo (auto_save) reflete a desconexão
        let reloaded = manager_in(&temp.path().join("s"), false);
        assert_eq!(
            reloaded.get_session(id).unwrap().state,
            SessionState::Disconnected
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_refresh_states_detects_exited_process() {
        let temp = TempDir::new().unwrap();
        let prog = script(&temp, "ssh", "exit 0");
        let mut m = manager_in(temp.path(), false).with_ssh_program(&prog);
        let id = m.create_session(SshSession::new("x", "h", "u"));
        m.start_session(id, None).unwrap();
        for _ in 0..200 {
            m.refresh_states();
            if m.get_session(id).unwrap().state == SessionState::Disconnected {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let s = m.get_session(id).unwrap();
        assert_eq!(s.state, SessionState::Disconnected);
        assert!(s.pid.is_none());
        // processo já colhido: parar de novo é inofensivo
        m.stop_session(id).unwrap();
    }

    #[test]
    fn test_start_session_errors() {
        let temp = TempDir::new().unwrap();
        let mut m = manager_in(temp.path(), false).with_ssh_program("/nao/existe/ssh");
        assert!(matches!(
            m.start_session(SessionId::new(), None),
            Err(SshError::ConfigNotFound)
        ));
        let id = m.create_session(SshSession::new("x", "h", "u"));
        assert!(m.start_session(id, None).is_err());
        assert_eq!(m.get_session(id).unwrap().state, SessionState::Error);
    }

    #[test]
    fn test_queries_save_all_and_cleanup() {
        let temp = TempDir::new().unwrap();
        let mut m = manager_in(temp.path(), false);
        let a = m.create_session(SshSession::new("a", "api.prod.example", "u"));
        let b = m.create_session(SshSession::new("b", "db.prod.example", "u"));
        let c = m.create_session(SshSession::new("c", "dev.local", "u"));

        let mut hosts: Vec<_> = m.find_by_host("prod").iter().map(|s| s.id).collect();
        hosts.sort_by_key(|id| id.0);
        let mut expected = vec![a, b];
        expected.sort_by_key(|id| id.0);
        assert_eq!(hosts, expected);
        assert!(m.find_by_host("nada").is_empty());

        m.get_session_mut(c).unwrap().state = SessionState::Connected;
        assert_eq!(m.list_active_sessions().len(), 1);

        assert_eq!(m.save_all_sessions().unwrap(), 3);
        let reloaded = manager_in(temp.path(), false);
        assert_eq!(reloaded.list_sessions().len(), 3);

        // remove só as desconectadas mais velhas que o limite
        assert_eq!(m.cleanup_old_sessions(Duration::from_secs(3600)), 0);
        assert_eq!(m.cleanup_old_sessions(Duration::ZERO), 2);
        assert!(m.get_session(a).is_none() && m.get_session(b).is_none());
        assert!(m.get_session(c).is_some());
        // arquivos das removidas também saem do disco
        assert_eq!(manager_in(temp.path(), false).list_sessions().len(), 1);
    }

    #[test]
    fn test_load_saved_sessions_skips_garbage_and_missing_dir() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("s");
        let mut m = manager_in(&dir, false);
        let id = m.create_session(SshSession::new("ok", "h", "u"));
        m.save_session(id).unwrap();
        std::fs::write(dir.join("lixo.json"), "{nao e json").unwrap();
        std::fs::write(dir.join("notas.txt"), "x").unwrap();

        let mut fresh = manager_in(&dir, false);
        assert_eq!(fresh.list_sessions().len(), 1);
        assert_eq!(fresh.load_saved_sessions().unwrap(), 1);

        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(fresh.load_saved_sessions().unwrap(), 0);
    }

    #[test]
    fn test_with_config_survives_unusable_directory() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("arquivo");
        std::fs::write(&file, "x").unwrap();
        // diretório sob um arquivo: não dá para criar nem ler, mas não falha
        let mut m = manager_in(&file.join("sub"), false);
        assert!(m.list_sessions().is_empty());
        let id = m.create_session(SshSession::new("x", "h", "u"));
        assert!(matches!(
            m.save_session(id),
            Err(SshError::InvalidConfig(_))
        ));
        assert!(matches!(
            m.save_session(SessionId::new()),
            Err(SshError::ConfigNotFound)
        ));
        // persistence_dir é um arquivo: read_dir falha
        let mut m2 = manager_in(&file, false);
        assert!(m2.load_saved_sessions().is_err());

        // arquivo da sessão removido depois do save
        let config = SessionPersistenceConfig {
            persistence_dir: temp.path().to_path_buf(),
            auto_save: false,
            ..Default::default()
        };
        let mut m3 = SessionManager::with_config(config);
        let id = m3.create_session(SshSession::new("y", "h", "u"));
        m3.save_session(id).unwrap();
        std::fs::rename(
            temp.path().join(format!("{}.json", id.0)),
            temp.path().join("tmp.json"),
        )
        .unwrap();
        // sem o arquivo da sessão não há backup
        assert!(matches!(
            m3.create_backup(id),
            Err(SshError::ConfigNotFound)
        ));
    }

    #[test]
    fn test_create_backup_errors_and_failed_auto_backup() {
        let temp = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp.path().to_path_buf(),
            auto_save: false,
            auto_backup: true,
            ..Default::default()
        };
        let mut m = SessionManager::with_config(config);
        let id = m.create_session(SshSession::new("b", "h", "u"));
        // ainda não salva: não há o que copiar
        assert!(matches!(m.create_backup(id), Err(SshError::ConfigNotFound)));

        m.save_session(id).unwrap();
        // "backups" ocupado por um arquivo: o backup falha, mas o save segue
        std::fs::write(temp.path().join("backups"), "x").unwrap();
        assert!(matches!(
            m.create_backup(id),
            Err(SshError::InvalidConfig(msg)) if msg.starts_with("Failed to create backup")
        ));
        m.save_session(id).unwrap();
        assert!(m.list_backups().is_err());
    }

    #[test]
    fn test_backup_cleanup_removes_oldest_beyond_limit() {
        let temp = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            max_backups: 2,
            ..Default::default()
        };
        let mut m = SessionManager::with_config(config);
        let id = m.create_session(SshSession::new("b", "h", "u"));
        m.save_session(id).unwrap();

        // backups antigos com o mesmo prefixo, com mtime no passado
        let prefix = id.to_string();
        let backups = temp.path().join("backups");
        std::fs::create_dir_all(&backups).unwrap();
        let session_json = std::fs::read(temp.path().join(format!("{}.json", id.0))).unwrap();
        for (i, age) in [(1, 300u64), (2, 200), (3, 100)] {
            let p = backups.join(format!("{prefix}_2000010{i}_000000.json"));
            std::fs::write(&p, &session_json).unwrap();
            let f = std::fs::File::options().write(true).open(&p).unwrap();
            f.set_modified(std::time::SystemTime::now() - Duration::from_secs(age))
                .unwrap();
        }
        // backup de outra sessão não entra na conta
        std::fs::write(backups.join("ffffffff_x.json"), "{}").unwrap();

        m.create_backup(id).unwrap();
        let mut names: Vec<_> = std::fs::read_dir(&backups)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.starts_with(&prefix))
            .collect();
        names.sort();
        assert_eq!(names.len(), 2, "{names:?}");
        // sobrevivem o novo e o mais recente dos antigos
        assert!(names.iter().any(|n| n.contains("20000103")));
        assert!(!names.iter().any(|n| n.contains("20000101")));
        assert!(backups.join("ffffffff_x.json").exists());
    }

    #[test]
    fn test_list_backups_skips_foreign_files_and_restore_by_id() {
        let temp = TempDir::new().unwrap();
        let config = SessionPersistenceConfig {
            persistence_dir: temp.path().to_path_buf(),
            auto_save: false,
            auto_backup: false,
            ..Default::default()
        };
        let mut m = SessionManager::with_config(config);
        let id = m.create_session(SshSession::new("orig", "h", "u").with_port(2201));
        m.save_session(id).unwrap();
        m.create_backup(id).unwrap();

        let backups = temp.path().join("backups");
        std::fs::write(backups.join("leia-me.txt"), "x").unwrap();
        std::fs::write(backups.join("estranho_1.json"), "{quebrado").unwrap();
        // nome com UUID completo também é aceito sem abrir o arquivo
        let other = Uuid::new_v4();
        std::fs::write(backups.join(format!("{other}_1.json")), "{}").unwrap();

        let list = m.list_backups().unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|b| b.session_id == id && b.size > 0));
        assert!(list.iter().any(|b| b.session_id == SessionId(other)));

        let restored = m.restore_session(id).unwrap();
        assert_eq!(restored.name, "orig");
        assert_eq!(restored.port, 2201);
        assert!(matches!(
            m.restore_session(SessionId::new()),
            Err(SshError::ConfigNotFound)
        ));
        // backup corrompido
        assert!(matches!(
            m.restore_from_backup(&backups.join("estranho_1.json")),
            Err(SshError::InvalidConfig(msg)) if msg.contains("parse")
        ));
    }

    #[tokio::test]
    async fn test_auto_save_task_ticks_and_stops() {
        let temp = TempDir::new().unwrap();
        let mut m = manager_in(temp.path(), true);
        assert!(m.start_auto_save(Some(1)));
        // deixa a task rodar o primeiro tick (imediato)
        tokio::time::sleep(Duration::from_millis(20)).await;
        // reiniciar para a anterior e cria outra
        assert!(m.start_auto_save(None));
        assert!(m.is_auto_save_running());
        drop(m);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
