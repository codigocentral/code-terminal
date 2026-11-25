//! Abas persistentes em disco: o que volta depois de reiniciar a máquina.
//!
//! Processo nenhum sobrevive a um reboot, mas a aba pode voltar com o título,
//! o grupo, o diretório e o **histórico da tela**, e oferecer retomar o
//! programa que rodava (o Claude Code na mesma conversa, o `ssh` para o mesmo
//! host, o último comando). Cada aba persistente vira dois arquivos em
//! `~/.local/state/code-terminal/tabs/` (pasta `0700`, arquivos `0600`):
//!
//! - `<chave>.json`: [`TabRecord`] (dados da aba e o programa);
//! - `<chave>.log`: o registro da saída, o mesmo que o daemon reenvia ao
//!   anexar (texto ANSI, já limitado pela compactação).
//!
//! `session.save_scrollback = false` desliga a gravação e apaga o que houver.

use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Programa de IA rodando na aba, informado pelos hooks dele.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    /// `claude` (único que sabe ser retomado por id, por ora).
    pub kind: String,
    /// Id da conversa (`claude --resume <id>`).
    pub session_id: String,
    /// Diretório onde o agente roda.
    #[serde(default)]
    pub cwd: Option<String>,
}

/// Dados de uma aba persistente gravados em disco.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabRecord {
    /// Nome da sessão no daemon.
    pub name: String,
    /// Dados da aba vindos do app (título, grupo, posição), como JSON.
    #[serde(default)]
    pub meta: String,
    #[serde(default)]
    pub hidden: bool,
    /// Retomar o programa sem perguntar (marcado na aba).
    #[serde(default)]
    pub auto_resume: bool,
    /// Diretório do shell.
    #[serde(default)]
    pub cwd: String,
    /// Quando foi gravado (segundos desde 1970).
    #[serde(default)]
    pub saved_at: u64,
    /// Programa em primeiro plano quando foi gravado.
    #[serde(default)]
    pub foreground: Option<Vec<String>>,
    /// Último programa visto em primeiro plano (o "Rodar de novo").
    #[serde(default)]
    pub last_command: Option<Vec<String>>,
    #[serde(default)]
    pub agent: Option<AgentInfo>,
}

/// Pasta das abas gravadas.
#[derive(Debug, Clone)]
pub struct TabStore {
    dir: PathBuf,
}

/// Pasta padrão: `$XDG_STATE_HOME/code-terminal/tabs` (no Linux,
/// `~/.local/state/...`). `CODE_TERMINAL_STATE_DIR` troca (testes).
pub fn default_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CODE_TERMINAL_STATE_DIR") {
        return Some(PathBuf::from(dir).join("tabs"));
    }
    let base = dirs::state_dir().or_else(dirs::data_local_dir)?;
    Some(base.join("code-terminal").join("tabs"))
}

/// Há abas gravadas esperando (para o app subir o daemon depois do reboot).
pub fn has_saved_tabs() -> bool {
    default_dir()
        .and_then(|dir| std::fs::read_dir(dir).ok())
        .is_some_and(|mut entries| {
            entries.any(|entry| {
                entry
                    .ok()
                    .is_some_and(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            })
        })
}

/// Nome de arquivo estável e seguro para uma sessão (o nome pode ter
/// qualquer caractere depois de um `mux-rename`).
pub fn file_key(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let safe: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    let digest = Sha256::digest(name.as_bytes());
    let hash: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!("{safe}-{hash}")
}

/// Grava `data` em `path` com permissão `0600`, de uma vez (arquivo
/// temporário + rename: quem lê nunca vê um arquivo pela metade).
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(data)?;
    file.sync_data()?;
    drop(file);
    std::fs::rename(tmp, path)
}

impl TabStore {
    /// Abre (criando com `0700`) a pasta.
    pub fn open(dir: PathBuf) -> io::Result<Self> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn paths(&self, name: &str) -> (PathBuf, PathBuf) {
        let key = file_key(name);
        (
            self.dir.join(format!("{key}.json")),
            self.dir.join(format!("{key}.log")),
        )
    }

    /// Grava a aba (o registro da saída primeiro: o `.json` é o que diz que
    /// a aba existe).
    pub fn save(&self, record: &TabRecord, log: &[u8]) -> io::Result<()> {
        let (json, log_path) = self.paths(&record.name);
        write_private(&log_path, log)?;
        let data = serde_json::to_vec_pretty(record).map_err(io::Error::other)?;
        write_private(&json, &data)
    }

    /// Abas gravadas (as ilegíveis são ignoradas).
    pub fn load_all(&self) -> Vec<TabRecord> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut records: Vec<TabRecord> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| std::fs::read(entry.path()).ok())
            .filter_map(|data| serde_json::from_slice(&data).ok())
            .collect();
        records.sort_by(|a, b| a.name.cmp(&b.name));
        records
    }

    /// Registro da saída gravado de uma aba.
    pub fn read_log(&self, name: &str) -> io::Result<Vec<u8>> {
        std::fs::read(self.paths(name).1)
    }

    /// Esquece a aba (ela foi encerrada).
    pub fn remove(&self, name: &str) {
        let (json, log) = self.paths(name);
        let _ = std::fs::remove_file(json);
        let _ = std::fs::remove_file(log);
    }

    /// Apaga tudo (gravação desligada na configuração).
    pub fn clear(dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .is_some_and(|ext| ext == "json" || ext == "log" || ext == "tmp")
            {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Aspas de shell só quando precisa (`ssh -p 22 host` fica como está).
pub fn shell_quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:@=,+%".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn join_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Nome do programa (sem o caminho).
fn program_name(argv: &[String]) -> &str {
    argv.first()
        .map(|program| program.rsplit('/').next().unwrap_or(program))
        .unwrap_or_default()
}

/// O que oferecer para retomar a aba depois do reboot: o Claude Code na
/// mesma conversa, senão o `ssh` que estava aberto, senão o último comando.
pub fn resume_command(record: &TabRecord) -> Option<String> {
    if let Some(agent) = record
        .agent
        .as_ref()
        .filter(|agent| agent.kind == "claude" && !agent.session_id.is_empty())
    {
        return Some(format!(
            "claude --resume {}",
            shell_quote(&agent.session_id)
        ));
    }
    let foreground = record.foreground.as_deref().filter(|argv| !argv.is_empty());
    if let Some(ssh) = foreground.filter(|argv| program_name(argv) == "ssh") {
        return Some(join_argv(ssh));
    }
    foreground
        .or(record.last_command.as_deref())
        .filter(|argv| !argv.is_empty())
        .map(join_argv)
}

/// Linha posta entre o histórico de antes do reboot e o shell novo. Antes
/// dela, volta ao estado normal (o programa podia estar na tela alternativa,
/// com cursor escondido ou mouse ligado). Nada aqui move o cursor (o
/// `CSI r` levaria a linha para o topo da tela, por cima do histórico).
pub fn reopened_banner(saved_at: u64) -> Vec<u8> {
    let when = chrono::DateTime::from_timestamp(i64::try_from(saved_at).unwrap_or(0), 0)
        .map(|utc| {
            utc.with_timezone(&chrono::Local)
                .format("%d/%m %H:%M")
                .to_string()
        })
        .unwrap_or_default();
    format!(
        "\x1b[?1049l\x1b[0m\x1b[?25h\x1b[?1l\x1b[?2004l\x1b[?1004l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b>\
         \r\n\x1b[2m\u{2500}\u{2500} aba reaberta (histórico até {when}) \u{2500}\u{2500}\x1b[0m\r\n"
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn record(name: &str) -> TabRecord {
        TabRecord {
            name: name.into(),
            meta: "{\"tab\":\"x\"}".into(),
            cwd: "/tmp".into(),
            saved_at: 1_700_000_000,
            ..Default::default()
        }
    }

    #[test]
    fn test_save_load_remove_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let store = TabStore::open(dir.path().join("tabs")).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(store.dir()), 0o700);
        store.save(&record("t1"), b"historico").unwrap();
        store.save(&record("nome com/barra"), b"x").unwrap();
        let loaded = store.load_all();
        assert_eq!(loaded.len(), 2);
        assert!(loaded.contains(&record("t1")));
        assert_eq!(store.read_log("t1").unwrap(), b"historico");
        for entry in std::fs::read_dir(store.dir()).unwrap() {
            assert_eq!(mode(&entry.unwrap().path()), 0o600);
        }
        store.remove("t1");
        assert_eq!(store.load_all().len(), 1);
        TabStore::clear(store.dir());
        assert!(store.load_all().is_empty());
    }

    #[test]
    fn test_file_key_is_safe_and_stable() {
        assert_eq!(file_key("t1"), file_key("t1"));
        assert_ne!(file_key("a/b"), file_key("a_b"));
        assert!(!file_key("../../etc").contains('/'));
    }

    #[test]
    fn test_resume_prefers_claude_then_ssh_then_last_command() {
        let mut r = record("t");
        assert_eq!(resume_command(&r), None);
        r.last_command = Some(vec!["cargo".into(), "test".into(), "a b".into()]);
        assert_eq!(resume_command(&r).as_deref(), Some("cargo test 'a b'"));
        r.foreground = Some(vec![
            "/usr/bin/ssh".into(),
            "-p".into(),
            "2222".into(),
            "admin@example.com".into(),
        ]);
        assert_eq!(
            resume_command(&r).as_deref(),
            Some("/usr/bin/ssh -p 2222 admin@example.com")
        );
        r.agent = Some(AgentInfo {
            kind: "claude".into(),
            session_id: "abc-123".into(),
            cwd: None,
        });
        assert_eq!(
            resume_command(&r).as_deref(),
            Some("claude --resume abc-123")
        );
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn test_banner_resets_modes_and_says_when() {
        let banner = String::from_utf8(reopened_banner(1_700_000_000)).unwrap();
        assert!(banner.starts_with("\x1b[?1049l"));
        assert!(banner.contains("aba reaberta"));
    }
}
