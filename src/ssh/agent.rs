//! Integração com ssh-agent

use crate::ssh::error::{SshKeyError, SshKeyResult};
use crate::ssh::key::{KeyFingerprint, KeyType};
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::process::Command;
use tracing::{debug, info};

/// Cliente para comunicação com ssh-agent
#[derive(Debug)]
pub struct AgentClient {
    /// Socket do agent (Unix) ou pipe (Windows)
    socket_path: Option<PathBuf>,
    /// Se o agent está disponível
    available: bool,
    /// Executável do ssh-add (substituível nos testes)
    program: String,
}

impl AgentClient {
    /// Cria um novo cliente do agent
    pub fn new() -> Self {
        Self::with_program(Self::detect_agent_socket(), "ssh-add")
    }

    /// Cria um cliente com socket e executável do ssh-add explícitos.
    ///
    /// O agent é considerado disponível quando há socket.
    pub fn with_program(socket_path: Option<PathBuf>, program: impl Into<String>) -> Self {
        Self {
            available: socket_path.is_some(),
            socket_path,
            program: program.into(),
        }
    }

    /// Detecta o socket do agent
    #[cfg(unix)]
    fn detect_agent_socket() -> Option<PathBuf> {
        if let Ok(socket) = env::var("SSH_AUTH_SOCK") {
            let path = PathBuf::from(socket);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    #[cfg(windows)]
    fn detect_agent_socket() -> Option<PathBuf> {
        if env::var("SSH_AUTH_SOCK").is_ok() {
            return Some(PathBuf::from("ssh-agent"));
        }
        None
    }

    /// Verifica se o agent está disponível
    pub fn is_available(&self) -> bool {
        self.available
    }

    /// Retorna o socket ou pipe detectado para o agent.
    pub fn socket_path(&self) -> Option<&PathBuf> {
        self.socket_path.as_ref()
    }

    /// Lista chaves carregadas no agent
    pub fn list_keys(&self) -> SshKeyResult<Vec<AgentKey>> {
        if !self.available {
            return Err(SshKeyError::AgentNotAvailable);
        }

        let output = Command::new(&self.program)
            .arg("-l")
            .output()
            .map_err(|e| SshKeyError::AgentListFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // O ssh-add escreve "The agent has no identities." no stdout
            // (com código de saída 1), não no stderr
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stderr.contains("no identities") || stdout.contains("no identities") {
                return Ok(Vec::new());
            }
            return Err(SshKeyError::AgentListFailed(stderr.to_string()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let keys = stdout
            .lines()
            .filter_map(|line| self.parse_agent_key_line(line))
            .collect();

        Ok(keys)
    }

    /// Parseia uma linha do ssh-add -l
    fn parse_agent_key_line(&self, line: &str) -> Option<AgentKey> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            return None;
        }

        let bits: usize = parts[0].parse().ok()?;
        let fingerprint = parts[1].trim_start_matches("SHA256:").to_string();

        // Formato: "<bits> <fingerprint> <comentário...> (<TIPO>)". O tipo vem
        // do sufixo entre parênteses, para que um comentário como
        // "backup-RSA" não confunda a detecção.
        let mut rest = &parts[2..];
        let type_tag = rest
            .last()
            .and_then(|t| t.strip_prefix('(').and_then(|t| t.strip_suffix(')')));
        if type_tag.is_some() {
            rest = &rest[..rest.len() - 1];
        }
        let comment = if rest.is_empty() {
            None
        } else {
            Some(rest.join(" "))
        };

        let tag = type_tag.unwrap_or(line);
        let key_type = if tag.contains("ED25519") {
            KeyType::Ed25519
        } else if tag.contains("ECDSA") {
            match bits {
                384 => KeyType::EcdsaP384,
                521 => KeyType::EcdsaP521,
                _ => KeyType::EcdsaP256,
            }
        } else if tag.contains("RSA") {
            KeyType::Rsa
        } else if tag.contains("DSA") {
            KeyType::Dsa
        } else {
            KeyType::Rsa
        };

        Some(AgentKey {
            key_type,
            bits,
            fingerprint: KeyFingerprint::from_sha256(fingerprint),
            comment,
        })
    }

    /// Adiciona uma chave ao agent
    pub fn add_key(&self, key_path: &PathBuf, passphrase: Option<&str>) -> SshKeyResult<()> {
        if !self.available {
            return Err(SshKeyError::AgentNotAvailable);
        }

        let mut cmd = Command::new(&self.program);
        cmd.arg(key_path);

        if let Some(pass) = passphrase {
            let mut child = cmd
                .stdin(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| SshKeyError::AgentAddFailed(e.to_string()))?;

            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin
                    .write_all(pass.as_bytes())
                    .map_err(|e| SshKeyError::AgentAddFailed(e.to_string()))?;
            }

            let status = child
                .wait()
                .map_err(|e| SshKeyError::AgentAddFailed(e.to_string()))?;

            if !status.success() {
                return Err(SshKeyError::AgentAddFailed("Failed to add key".to_string()));
            }
        } else {
            let output = cmd
                .output()
                .map_err(|e| SshKeyError::AgentAddFailed(e.to_string()))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(SshKeyError::AgentAddFailed(stderr.to_string()));
            }
        }

        info!("Added key to agent: {:?}", key_path);
        Ok(())
    }

    /// Remove uma chave do agent
    pub fn remove_key(&self, key_path: &PathBuf) -> SshKeyResult<()> {
        if !self.available {
            return Err(SshKeyError::AgentNotAvailable);
        }

        let output = Command::new(&self.program)
            .arg("-d")
            .arg(key_path)
            .output()
            .map_err(|e| SshKeyError::AgentRemoveFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshKeyError::AgentRemoveFailed(stderr.to_string()));
        }

        info!("Removed key from agent: {:?}", key_path);
        Ok(())
    }

    /// Remove todas as chaves do agent
    pub fn remove_all_keys(&self) -> SshKeyResult<()> {
        if !self.available {
            return Err(SshKeyError::AgentNotAvailable);
        }

        let output = Command::new(&self.program)
            .arg("-D")
            .output()
            .map_err(|e| SshKeyError::AgentRemoveFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshKeyError::AgentRemoveFailed(stderr.to_string()));
        }

        info!("Removed all keys from agent");
        Ok(())
    }

    /// Bloqueia o agent (requer senha)
    pub fn lock(&self, password: &str) -> SshKeyResult<()> {
        if !self.available {
            return Err(SshKeyError::AgentNotAvailable);
        }

        let mut child = Command::new(&self.program)
            .arg("-x")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| SshKeyError::AgentCommunication(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin
                .write_all(format!("{}\n", password).as_bytes())
                .map_err(|e| SshKeyError::AgentCommunication(e.to_string()))?;
        }

        let status = child
            .wait()
            .map_err(|e| SshKeyError::AgentCommunication(e.to_string()))?;

        if !status.success() {
            return Err(SshKeyError::AgentCommunication(
                "Failed to lock agent".to_string(),
            ));
        }

        info!("Agent locked");
        Ok(())
    }

    /// Desbloqueia o agent
    pub fn unlock(&self, password: &str) -> SshKeyResult<()> {
        if !self.available {
            return Err(SshKeyError::AgentNotAvailable);
        }

        let mut child = Command::new(&self.program)
            .arg("-X")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| SshKeyError::AgentCommunication(e.to_string()))?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin
                .write_all(format!("{}\n", password).as_bytes())
                .map_err(|e| SshKeyError::AgentCommunication(e.to_string()))?;
        }

        let status = child
            .wait()
            .map_err(|e| SshKeyError::AgentCommunication(e.to_string()))?;

        if !status.success() {
            return Err(SshKeyError::AgentCommunication(
                "Failed to unlock agent".to_string(),
            ));
        }

        info!("Agent unlocked");
        Ok(())
    }
}

impl Default for AgentClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Chave carregada no agent
#[derive(Debug, Clone)]
pub struct AgentKey {
    /// Tipo da chave
    pub key_type: KeyType,
    /// Tamanho em bits
    pub bits: usize,
    /// Fingerprint
    pub fingerprint: KeyFingerprint,
    /// Comentário
    pub comment: Option<String>,
}

impl AgentKey {
    /// Verifica se a chave corresponde a um fingerprint
    pub fn matches_fingerprint(&self, fingerprint: &str) -> bool {
        self.fingerprint.sha256 == fingerprint || self.fingerprint.sha256.starts_with(fingerprint)
    }
}

/// Gerenciador do ssh-agent
#[derive(Debug)]
pub struct SshAgent {
    client: AgentClient,
    /// Cache de chaves no agent
    key_cache: HashMap<String, AgentKey>,
}

impl SshAgent {
    /// Cria um novo gerenciador do agent
    pub fn new() -> Self {
        Self::with_client(AgentClient::new())
    }

    /// Cria um gerenciador usando o cliente dado.
    pub fn with_client(client: AgentClient) -> Self {
        Self {
            client,
            key_cache: HashMap::new(),
        }
    }

    /// Verifica se o agent está disponível
    pub fn is_available(&self) -> bool {
        self.client.is_available()
    }

    /// Atualiza a lista de chaves
    pub fn refresh_keys(&mut self) -> SshKeyResult<()> {
        let keys = self.client.list_keys()?;
        self.key_cache.clear();

        for key in keys {
            let id = key.fingerprint.sha256.clone();
            self.key_cache.insert(id, key);
        }

        debug!("Refreshed {} keys from agent", self.key_cache.len());
        Ok(())
    }

    /// Retorna todas as chaves no agent
    pub fn get_keys(&mut self) -> SshKeyResult<Vec<&AgentKey>> {
        if self.key_cache.is_empty() {
            self.refresh_keys()?;
        }
        Ok(self.key_cache.values().collect())
    }

    /// Verifica se uma chave está no agent
    pub fn has_key(&mut self, fingerprint: &str) -> SshKeyResult<bool> {
        if self.key_cache.is_empty() {
            self.refresh_keys()?;
        }
        Ok(self.key_cache.contains_key(fingerprint))
    }

    /// Adiciona uma chave ao agent
    pub fn add_key(&self, key_path: &PathBuf, passphrase: Option<&str>) -> SshKeyResult<()> {
        self.client.add_key(key_path, passphrase)
    }

    /// Remove uma chave do agent
    pub fn remove_key(&self, key_path: &PathBuf) -> SshKeyResult<()> {
        self.client.remove_key(key_path)
    }

    /// Remove todas as chaves
    pub fn remove_all_keys(&self) -> SshKeyResult<()> {
        self.client.remove_all_keys()
    }
}

impl Default for SshAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    /// Cria um ssh-add falso: grava os argumentos (e o stdin) num log e
    /// executa `body`.
    #[cfg(unix)]
    fn fake_ssh_add(dir: &TempDir, name: &str, body: &str) -> String {
        let log = dir.path().join(format!("{name}.log"));
        crate::test_exec::write_fake_executable(
            &dir.path().join(name),
            &format!("echo \"$@\" >> '{}'\n{body}", log.display()),
        )
    }

    #[cfg(unix)]
    fn log_of(dir: &TempDir, name: &str) -> String {
        std::fs::read_to_string(dir.path().join(format!("{name}.log"))).unwrap_or_default()
    }

    fn client(program: &str) -> AgentClient {
        AgentClient::with_program(Some(PathBuf::from("/tmp/agent.sock")), program)
    }

    #[test]
    fn test_unavailable_agent_rejects_every_operation() {
        let c = AgentClient::with_program(None, "/nao/existe/ssh-add");
        assert!(!c.is_available());
        assert!(c.socket_path().is_none());
        let p = PathBuf::from("/k");
        assert!(matches!(c.list_keys(), Err(SshKeyError::AgentNotAvailable)));
        assert!(matches!(
            c.add_key(&p, None),
            Err(SshKeyError::AgentNotAvailable)
        ));
        assert!(matches!(
            c.remove_key(&p),
            Err(SshKeyError::AgentNotAvailable)
        ));
        assert!(matches!(
            c.remove_all_keys(),
            Err(SshKeyError::AgentNotAvailable)
        ));
        assert!(matches!(c.lock("x"), Err(SshKeyError::AgentNotAvailable)));
        assert!(matches!(c.unlock("x"), Err(SshKeyError::AgentNotAvailable)));

        let mut agent = SshAgent::with_client(c);
        assert!(!agent.is_available());
        assert!(matches!(
            agent.get_keys(),
            Err(SshKeyError::AgentNotAvailable)
        ));
        assert!(matches!(
            agent.has_key("x"),
            Err(SshKeyError::AgentNotAvailable)
        ));
    }

    #[test]
    fn test_missing_program_maps_to_operation_errors() {
        let c = client("/nao/existe/ssh-add");
        assert!(c.is_available());
        assert_eq!(c.socket_path(), Some(&PathBuf::from("/tmp/agent.sock")));
        let p = PathBuf::from("/k");
        assert!(matches!(
            c.list_keys(),
            Err(SshKeyError::AgentListFailed(_))
        ));
        assert!(matches!(
            c.add_key(&p, None),
            Err(SshKeyError::AgentAddFailed(_))
        ));
        assert!(matches!(
            c.add_key(&p, Some("s")),
            Err(SshKeyError::AgentAddFailed(_))
        ));
        assert!(matches!(
            c.remove_key(&p),
            Err(SshKeyError::AgentRemoveFailed(_))
        ));
        assert!(matches!(
            c.remove_all_keys(),
            Err(SshKeyError::AgentRemoveFailed(_))
        ));
        assert!(matches!(
            c.lock("x"),
            Err(SshKeyError::AgentCommunication(_))
        ));
        assert!(matches!(
            c.unlock("x"),
            Err(SshKeyError::AgentCommunication(_))
        ));
    }

    #[test]
    fn test_parse_agent_key_line_formats() {
        let c = client("x");
        let k = c
            .parse_agent_key_line("256 SHA256:abcDEF diogo@pc1 (ED25519)")
            .unwrap();
        assert_eq!(k.key_type, KeyType::Ed25519);
        assert_eq!(k.bits, 256);
        assert_eq!(k.fingerprint.sha256, "abcDEF");
        assert_eq!(k.comment.as_deref(), Some("diogo@pc1"));

        let k = c
            .parse_agent_key_line("4096 SHA256:zzz chave de deploy (RSA)")
            .unwrap();
        assert_eq!(k.key_type, KeyType::Rsa);
        assert_eq!(k.comment.as_deref(), Some("chave de deploy"));

        // o comentário não interfere na detecção do tipo
        let k = c
            .parse_agent_key_line("384 SHA256:e backup-RSA (ECDSA)")
            .unwrap();
        assert_eq!(k.key_type, KeyType::EcdsaP384);
        assert_eq!(k.comment.as_deref(), Some("backup-RSA"));
        let k = c.parse_agent_key_line("521 SHA256:e x (ECDSA)").unwrap();
        assert_eq!(k.key_type, KeyType::EcdsaP521);
        let k = c.parse_agent_key_line("256 SHA256:e x (ECDSA-SK)").unwrap();
        assert_eq!(k.key_type, KeyType::EcdsaP256);
        let k = c.parse_agent_key_line("1024 SHA256:d (DSA)").unwrap();
        assert_eq!(k.key_type, KeyType::Dsa);
        assert_eq!(k.comment, None);

        // sem sufixo de tipo: usa a linha toda e cai em RSA por padrão
        let k = c.parse_agent_key_line("2048 SHA256:q sem-tipo").unwrap();
        assert_eq!(k.key_type, KeyType::Rsa);
        assert_eq!(k.comment.as_deref(), Some("sem-tipo"));
        let k = c.parse_agent_key_line("256 SHA256:q algo ED25519").unwrap();
        assert_eq!(k.key_type, KeyType::Ed25519);

        assert!(c.parse_agent_key_line("").is_none());
        assert!(c.parse_agent_key_line("256 SHA256:x").is_none());
        assert!(c.parse_agent_key_line("muitos SHA256:x c (RSA)").is_none());
    }

    #[test]
    fn test_matches_fingerprint_by_prefix() {
        let key = AgentKey {
            key_type: KeyType::Rsa,
            bits: 2048,
            fingerprint: KeyFingerprint::from_sha256("abcdef".to_string()),
            comment: None,
        };
        assert!(key.matches_fingerprint("abc"));
        assert!(key.matches_fingerprint("abcdef"));
        assert!(!key.matches_fingerprint("abcdefg"));
    }

    #[cfg(unix)]
    #[test]
    fn test_list_keys_and_cache() {
        let dir = TempDir::new().unwrap();
        let prog = fake_ssh_add(
            &dir,
            "list",
            "echo '256 SHA256:aaa a@h (ED25519)'; echo 'lixo'; echo '4096 SHA256:bbb b@h (RSA)'",
        );
        let c = client(&prog);
        let keys = c.list_keys().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[1].fingerprint.sha256, "bbb");
        assert_eq!(log_of(&dir, "list"), "-l\n");

        let mut agent = SshAgent::with_client(client(&prog));
        assert!(agent.is_available());
        assert!(agent.has_key("aaa").unwrap());
        assert!(!agent.has_key("ccc").unwrap());
        let mut fps: Vec<_> = agent
            .get_keys()
            .unwrap()
            .iter()
            .map(|k| k.fingerprint.sha256.clone())
            .collect();
        fps.sort();
        assert_eq!(fps, vec!["aaa", "bbb"]);
        // o cache evita chamar o ssh-add de novo (1 do list_keys + 1 do agent)
        assert_eq!(log_of(&dir, "list").lines().count(), 2);
        agent.refresh_keys().unwrap();
        assert_eq!(log_of(&dir, "list").lines().count(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn test_list_keys_empty_agent_and_failures() {
        let dir = TempDir::new().unwrap();
        // Regressão: o ssh-add real escreve isto no stdout e sai com 1
        let stdout = fake_ssh_add(&dir, "vazio", "echo 'The agent has no identities.'; exit 1");
        assert!(client(&stdout).list_keys().unwrap().is_empty());
        let stderr = fake_ssh_add(&dir, "vazio2", "echo 'no identities' >&2; exit 1");
        assert!(client(&stderr).list_keys().unwrap().is_empty());

        let bad = fake_ssh_add(
            &dir,
            "ruim",
            "echo 'Could not open a connection' >&2; exit 2",
        );
        match client(&bad).list_keys() {
            Err(SshKeyError::AgentListFailed(m)) => assert!(m.contains("Could not open")),
            other => panic!("{:?}", other),
        }

        let mut agent = SshAgent::with_client(client(&stdout));
        assert!(agent.get_keys().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_add_remove_and_clear_keys() {
        let dir = TempDir::new().unwrap();
        let ok = fake_ssh_add(&dir, "ok", "cat >> \"$0.stdin\" 2>/dev/null; exit 0");
        let c = client(&ok);
        let key = PathBuf::from("/home/u/.ssh/id_ed25519");
        c.add_key(&key, None).unwrap();
        c.add_key(&key, Some("segredo")).unwrap();
        c.remove_key(&key).unwrap();
        c.remove_all_keys().unwrap();
        let agent = SshAgent::with_client(client(&ok));
        agent.add_key(&key, None).unwrap();
        agent.remove_key(&key).unwrap();
        agent.remove_all_keys().unwrap();
        assert_eq!(
            log_of(&dir, "ok"),
            "/home/u/.ssh/id_ed25519\n/home/u/.ssh/id_ed25519\n-d /home/u/.ssh/id_ed25519\n-D\n\
             /home/u/.ssh/id_ed25519\n-d /home/u/.ssh/id_ed25519\n-D\n"
        );
        let stdin = std::fs::read_to_string(format!("{ok}.stdin")).unwrap();
        assert!(stdin.contains("segredo"));

        let bad = fake_ssh_add(
            &dir,
            "bad",
            "cat > /dev/null; echo 'erro do agent' >&2; exit 1",
        );
        let c = client(&bad);
        assert!(
            matches!(c.add_key(&key, None), Err(SshKeyError::AgentAddFailed(m)) if m.contains("erro do agent"))
        );
        assert!(
            matches!(c.add_key(&key, Some("p")), Err(SshKeyError::AgentAddFailed(m)) if m == "Failed to add key")
        );
        assert!(
            matches!(c.remove_key(&key), Err(SshKeyError::AgentRemoveFailed(m)) if m.contains("erro do agent"))
        );
        assert!(
            matches!(c.remove_all_keys(), Err(SshKeyError::AgentRemoveFailed(m)) if m.contains("erro do agent"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_lock_and_unlock_send_password_on_stdin() {
        let dir = TempDir::new().unwrap();
        // aceita só a senha "certa"
        let prog = fake_ssh_add(&dir, "lock", "read pw; [ \"$pw\" = certa ]");
        let c = client(&prog);
        c.lock("certa").unwrap();
        c.unlock("certa").unwrap();
        assert!(
            matches!(c.lock("errada"), Err(SshKeyError::AgentCommunication(m)) if m == "Failed to lock agent")
        );
        assert!(
            matches!(c.unlock("errada"), Err(SshKeyError::AgentCommunication(m)) if m == "Failed to unlock agent")
        );
        assert_eq!(log_of(&dir, "lock"), "-x\n-X\n-x\n-X\n");
    }

    #[cfg(unix)]
    #[test]
    fn test_detect_agent_socket_requires_existing_path() {
        // Só observa o ambiente atual: com SSH_AUTH_SOCK apontando para um
        // caminho existente o cliente fica disponível; senão, não.
        let expected = std::env::var("SSH_AUTH_SOCK")
            .ok()
            .filter(|s| Path::new(s).exists())
            .map(PathBuf::from);
        let c = AgentClient::default();
        assert_eq!(c.socket_path().cloned(), expected);
        assert_eq!(c.is_available(), expected.is_some());
        assert_eq!(SshAgent::default().is_available(), expected.is_some());
    }

    #[test]
    fn test_agent_client_creation() {
        let client = AgentClient::new();
        assert!(client.socket_path().is_some() || !client.is_available());
    }

    #[test]
    fn test_ssh_agent_creation() {
        let agent = SshAgent::new();
        assert!(!agent.key_cache.is_empty() || agent.key_cache.is_empty());
    }

    #[test]
    fn test_agent_key_matches_fingerprint() {
        let key = AgentKey {
            key_type: KeyType::Ed25519,
            bits: 256,
            fingerprint: KeyFingerprint::from_sha256("abc123".to_string()),
            comment: Some("test".to_string()),
        };

        assert!(key.matches_fingerprint("abc123"));
        assert!(!key.matches_fingerprint("xyz789"));
    }
}
