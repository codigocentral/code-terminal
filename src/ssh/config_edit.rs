//! Editar hosts do `~/.ssh/config` sem estragar o resto do arquivo (#32).
//!
//! O arquivo é tratado como linhas: só as linhas do bloco `Host <alias>`
//! editado mudam. Comentários, linhas em branco, ordem, `Include`, `Match` e
//! blocos com vários padrões ou curingas ficam exatamente como estavam. Só
//! blocos `Host` com um alias simples (sem `*`, `?` ou `!`) são editáveis.
//!
//! Gravar faz backup (`config.bak-<data>`) e escrita atômica: arquivo
//! temporário no mesmo diretório com permissão 0600, depois `rename`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Campos editáveis de um host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostEntry {
    pub alias: String,
    pub host_name: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<String>,
}

/// Por que uma edição foi recusada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// Alias vazio, com espaço ou com curinga.
    InvalidAlias,
    /// Já existe um bloco `Host` com esse alias.
    AliasExists,
    /// O host a editar não existe (ou não é editável).
    NotFound,
    /// Valor com quebra de linha (estragaria o arquivo).
    InvalidValue,
}

/// Palavras-chave que esta tela edita, na ordem em que entram num bloco novo.
const KEYS: [&str; 4] = ["HostName", "User", "Port", "IdentityFile"];

/// O `~/.ssh/config` como linhas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshConfigDocument {
    lines: Vec<String>,
    trailing_newline: bool,
}

/// Primeira palavra (a palavra-chave) e o resto (o valor), aceitando
/// `Chave valor` e `Chave=valor`.
fn split_directive(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let end = trimmed
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(trimmed.len());
    let (key, rest) = trimmed.split_at(end);
    let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    Some((key, rest.trim_end()))
}

fn is_block_start(line: &str) -> bool {
    split_directive(line).is_some_and(|(key, _)| {
        key.eq_ignore_ascii_case("Host") || key.eq_ignore_ascii_case("Match")
    })
}

fn valid_alias(alias: &str) -> bool {
    !alias.is_empty() && !alias.contains(|c: char| c.is_whitespace() || "*?!#\"".contains(c))
}

fn valid_value(value: &str) -> bool {
    !value.contains(['\n', '\r'])
}

/// O alias de uma linha `Host`, se ela tiver um alias simples só (sem
/// comentário no meio).
fn simple_host_alias(line: &str) -> Option<&str> {
    let (key, value) = split_directive(line)?;
    if !key.eq_ignore_ascii_case("Host") {
        return None;
    }
    let value = value.split('#').next().unwrap_or("").trim();
    let mut patterns = value.split_whitespace();
    let alias = patterns.next()?;
    (patterns.next().is_none() && valid_alias(alias)).then_some(alias)
}

impl SshConfigDocument {
    /// Lê o texto do arquivo.
    pub fn parse(text: &str) -> Self {
        Self {
            lines: text.lines().map(str::to_string).collect(),
            trailing_newline: text.is_empty() || text.ends_with('\n'),
        }
    }

    /// O texto para gravar.
    pub fn render(&self) -> String {
        let mut text = self.lines.join("\n");
        if self.trailing_newline && !self.lines.is_empty() {
            text.push('\n');
        }
        text
    }

    /// Linhas `[início, fim)` do bloco `Host <alias>` editável.
    fn block(&self, alias: &str) -> Option<(usize, usize)> {
        let start = self
            .lines
            .iter()
            .position(|line| simple_host_alias(line) == Some(alias))?;
        let end = self.lines[start + 1..]
            .iter()
            .position(|line| is_block_start(line))
            .map_or(self.lines.len(), |offset| start + 1 + offset);
        Some((start, end))
    }

    /// Hosts editáveis, na ordem do arquivo.
    pub fn editable_hosts(&self) -> Vec<HostEntry> {
        self.lines
            .iter()
            .filter_map(|line| simple_host_alias(line))
            .filter_map(|alias| self.host(alias))
            .collect()
    }

    /// Os campos de um host editável.
    pub fn host(&self, alias: &str) -> Option<HostEntry> {
        let (start, end) = self.block(alias)?;
        let mut entry = HostEntry {
            alias: alias.to_string(),
            ..Default::default()
        };
        for line in &self.lines[start + 1..end] {
            let Some((key, value)) = split_directive(line) else {
                continue;
            };
            let value = value.trim_matches('"').to_string();
            let set = |slot: &mut Option<String>| {
                if slot.is_none() {
                    *slot = Some(value.clone());
                }
            };
            if key.eq_ignore_ascii_case("HostName") {
                set(&mut entry.host_name);
            } else if key.eq_ignore_ascii_case("User") {
                set(&mut entry.user);
            } else if key.eq_ignore_ascii_case("IdentityFile") {
                set(&mut entry.identity_file);
            } else if key.eq_ignore_ascii_case("Port") && entry.port.is_none() {
                entry.port = value.parse().ok();
            }
        }
        Some(entry)
    }

    /// Cria (`original = None`) ou edita o host `original`.
    pub fn upsert(&mut self, original: Option<&str>, entry: &HostEntry) -> Result<(), EditError> {
        if !valid_alias(&entry.alias) {
            return Err(EditError::InvalidAlias);
        }
        let values = [
            entry.host_name.clone(),
            entry.user.clone(),
            entry.port.map(|p| p.to_string()),
            entry.identity_file.clone(),
        ];
        if values.iter().flatten().any(|v| !valid_value(v)) {
            return Err(EditError::InvalidValue);
        }
        let renamed = original != Some(entry.alias.as_str());
        if renamed && self.block(&entry.alias).is_some() {
            return Err(EditError::AliasExists);
        }
        match original {
            None => {
                self.append_block(entry, &values);
                Ok(())
            }
            Some(alias) => {
                let (start, _) = self.block(alias).ok_or(EditError::NotFound)?;
                if renamed {
                    self.lines[start] = rename_host_line(&self.lines[start], alias, &entry.alias);
                }
                for (key, value) in KEYS.iter().zip(values) {
                    self.set_key(&entry.alias, key, value.as_deref());
                }
                Ok(())
            }
        }
    }

    fn append_block(&mut self, entry: &HostEntry, values: &[Option<String>; 4]) {
        while self.lines.last().is_some_and(|line| line.trim().is_empty()) {
            self.lines.pop();
        }
        if !self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.lines.push(format!("Host {}", entry.alias));
        for (key, value) in KEYS.iter().zip(values) {
            if let Some(value) = value {
                self.lines.push(format!("    {key} {value}"));
            }
        }
        self.trailing_newline = true;
    }

    /// Troca, cria ou tira (`None`) a palavra-chave `key` no bloco do host.
    fn set_key(&mut self, alias: &str, key: &str, value: Option<&str>) {
        let Some((start, end)) = self.block(alias) else {
            return;
        };
        let existing = (start + 1..end).find(|&i| {
            split_directive(&self.lines[i]).is_some_and(|(k, _)| k.eq_ignore_ascii_case(key))
        });
        match (existing, value) {
            (Some(i), Some(value)) => {
                let line = &self.lines[i];
                let (current_key, current) = split_directive(line).unwrap_or((key, ""));
                if current.trim_matches('"') != value {
                    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                    self.lines[i] = format!("{indent}{current_key} {value}");
                }
            }
            (Some(i), None) => {
                self.lines.remove(i);
            }
            (None, Some(value)) => {
                // Depois da última diretiva do bloco (antes das linhas em
                // branco e comentários que separam do próximo bloco).
                let last = (start..end)
                    .rev()
                    .find(|&i| split_directive(&self.lines[i]).is_some())
                    .unwrap_or(start);
                let indent = (start + 1..end)
                    .find_map(|i| {
                        split_directive(&self.lines[i]).map(|_| {
                            self.lines[i]
                                .chars()
                                .take_while(|c| c.is_whitespace())
                                .collect::<String>()
                        })
                    })
                    .unwrap_or_else(|| "    ".to_string());
                self.lines
                    .insert(last + 1, format!("{indent}{key} {value}"));
            }
            (None, None) => {}
        }
    }
}

/// A linha `Host antigo` com o alias novo, mantendo recuo e comentário.
fn rename_host_line(line: &str, old: &str, new: &str) -> String {
    match line.find(old) {
        Some(at) => format!("{}{}{}", &line[..at], new, &line[at + old.len()..]),
        None => format!("Host {new}"),
    }
}

/// Caminho do `~/.ssh/config`. `CODE_TERMINAL_SSH_CONFIG` troca o caminho;
/// nos testes de unidade nunca é o arquivo do usuário.
pub fn ssh_config_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CODE_TERMINAL_SSH_CONFIG").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    if cfg!(test) {
        return Some(
            std::env::temp_dir()
                .join(format!("code-terminal-ssh-test-{}", std::process::id()))
                .join("config"),
        );
    }
    dirs::home_dir().map(|home| home.join(".ssh").join("config"))
}

/// Grava `text` em `path` com segurança: copia o arquivo atual para
/// `config.bak-<AAAAMMDD-HHMMSS>` (0600), escreve num temporário do mesmo
/// diretório (0600) e troca com `rename`. Devolve o caminho do backup, se
/// havia arquivo antes.
pub fn save_with_backup(path: &Path, text: &str) -> std::io::Result<Option<PathBuf>> {
    // `~/.ssh/config` como link (dotfiles): grava no arquivo de verdade; o
    // `rename` trocaria o link por um arquivo comum.
    let resolved = fs::canonicalize(path).ok();
    let path = resolved.as_deref().unwrap_or(path);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    if !dir.exists() {
        fs::create_dir_all(dir)?;
        restrict(dir, 0o700)?;
    }
    let name = path
        .file_name()
        .map_or_else(|| "config".into(), |n| n.to_string_lossy().into_owned());
    let backup = if path.exists() {
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let mut backup = dir.join(format!("{name}.bak-{stamp}"));
        let mut n = 1;
        while backup.exists() {
            backup = dir.join(format!("{name}.bak-{stamp}-{n}"));
            n += 1;
        }
        fs::copy(path, &backup)?;
        restrict(&backup, 0o600)?;
        Some(backup)
    } else {
        None
    };
    let temp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    {
        let mut file = create_private(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    Ok(backup)
}

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    fs::File::create(path)
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture com tudo o que não pode mudar: comentários, Include, Match,
    /// curinga, vários padrões, `Chave=valor`, recuo com tab.
    const FIXTURE: &str = "\
# Meu config
Include ~/.ssh/config.d/*

Host *
    ServerAliveInterval 30

Host prod # servidor principal
\tHostName 10.0.0.5
\tUser deploy
\tPort=2222
\t# chave antiga: id_rsa
\tIdentityFile ~/.ssh/id_prod

Host a b
    User dois

Match host *.interno
    User interno

Host dev
    HostName dev.exemplo.com
";

    #[test]
    fn only_simple_host_blocks_are_editable() {
        let doc = SshConfigDocument::parse(FIXTURE);
        let hosts = doc.editable_hosts();
        assert_eq!(
            hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(),
            vec!["prod", "dev"]
        );
        let prod = &hosts[0];
        assert_eq!(prod.host_name.as_deref(), Some("10.0.0.5"));
        assert_eq!(prod.user.as_deref(), Some("deploy"));
        assert_eq!(prod.port, Some(2222));
        assert_eq!(prod.identity_file.as_deref(), Some("~/.ssh/id_prod"));
        assert_eq!(hosts[1].user, None);
    }

    #[test]
    fn untouched_document_renders_byte_for_byte() {
        assert_eq!(SshConfigDocument::parse(FIXTURE).render(), FIXTURE);
        let no_newline = "Host x\n    User y";
        assert_eq!(SshConfigDocument::parse(no_newline).render(), no_newline);
    }

    #[test]
    fn editing_changes_only_the_block_lines() {
        let mut doc = SshConfigDocument::parse(FIXTURE);
        let mut prod = doc.host("prod").unwrap();
        prod.user = Some("admin".into());
        prod.port = None; // tira a linha Port
        prod.identity_file = Some("~/.ssh/id_novo".into());
        doc.upsert(Some("prod"), &prod).unwrap();
        let out = doc.render();
        // O resto do arquivo é o mesmo.
        assert!(out.starts_with("# Meu config\nInclude ~/.ssh/config.d/*\n\nHost *\n"));
        assert!(out.contains("Match host *.interno\n    User interno\n"));
        assert!(out.contains("Host a b\n    User dois\n"));
        // No bloco: recuo, chave e comentários preservados.
        assert!(out.contains(
            "Host prod # servidor principal\n\tHostName 10.0.0.5\n\tUser admin\n\t# chave antiga: id_rsa\n\tIdentityFile ~/.ssh/id_novo\n"
        ));
        assert!(!out.contains("Port=2222"));
        assert_eq!(doc.host("prod").unwrap(), prod);
    }

    #[test]
    fn adding_a_missing_key_goes_after_the_last_directive_of_the_block() {
        let mut doc = SshConfigDocument::parse(FIXTURE);
        let mut dev = doc.host("dev").unwrap();
        dev.user = Some("eu".into());
        dev.port = Some(2200);
        doc.upsert(Some("dev"), &dev).unwrap();
        assert!(doc
            .render()
            .ends_with("Host dev\n    HostName dev.exemplo.com\n    User eu\n    Port 2200\n"));
    }

    #[test]
    fn renaming_keeps_the_comment_on_the_host_line() {
        let mut doc = SshConfigDocument::parse(FIXTURE);
        let mut prod = doc.host("prod").unwrap();
        prod.alias = "producao".into();
        doc.upsert(Some("prod"), &prod).unwrap();
        assert!(doc
            .render()
            .contains("Host producao # servidor principal\n"));
        assert!(doc.host("prod").is_none());
        // Para um alias que já existe, recusa.
        let mut again = doc.host("producao").unwrap();
        again.alias = "dev".into();
        assert_eq!(
            doc.upsert(Some("producao"), &again),
            Err(EditError::AliasExists)
        );
    }

    #[test]
    fn new_host_is_appended_with_one_blank_line() {
        let mut doc = SshConfigDocument::parse(FIXTURE);
        let entry = HostEntry {
            alias: "novo".into(),
            host_name: Some("192.0.2.9".into()),
            user: Some("root".into()),
            port: Some(22),
            identity_file: None,
        };
        doc.upsert(None, &entry).unwrap();
        let out = doc.render();
        assert!(out.starts_with(FIXTURE.trim_end()));
        assert!(out.ends_with(
            "HostName dev.exemplo.com\n\nHost novo\n    HostName 192.0.2.9\n    User root\n    Port 22\n"
        ));
        assert_eq!(doc.host("novo").unwrap(), entry);
        // Arquivo vazio: sem linha em branco no começo.
        let mut empty = SshConfigDocument::parse("");
        empty.upsert(None, &entry).unwrap();
        assert!(empty.render().starts_with("Host novo\n"));
    }

    #[test]
    fn invalid_input_is_refused() {
        let mut doc = SshConfigDocument::parse(FIXTURE);
        let mut bad = HostEntry {
            alias: "com espaço".into(),
            ..Default::default()
        };
        assert_eq!(doc.upsert(None, &bad), Err(EditError::InvalidAlias));
        bad.alias = "web*".into();
        assert_eq!(doc.upsert(None, &bad), Err(EditError::InvalidAlias));
        bad.alias = "ok".into();
        bad.user = Some("a\nHost x".into());
        assert_eq!(doc.upsert(None, &bad), Err(EditError::InvalidValue));
        bad.user = None;
        assert_eq!(
            doc.upsert(
                None,
                &HostEntry {
                    alias: "dev".into(),
                    ..Default::default()
                }
            ),
            Err(EditError::AliasExists)
        );
        assert_eq!(
            doc.upsert(Some("nao-existe"), &bad),
            Err(EditError::NotFound)
        );
        assert_eq!(doc.render(), FIXTURE, "nada mudou");
    }

    #[test]
    fn save_makes_a_backup_and_writes_atomically_with_0600() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ssh").join("config");
        // Sem arquivo: cria o diretório e não há backup.
        assert_eq!(save_with_backup(&path, "Host a\n").unwrap(), None);
        assert_eq!(fs::read_to_string(&path).unwrap(), "Host a\n");
        // Com arquivo: o backup guarda o conteúdo anterior.
        let backup = save_with_backup(&path, "Host b\n").unwrap().unwrap();
        assert_eq!(fs::read_to_string(&backup).unwrap(), "Host a\n");
        assert_eq!(fs::read_to_string(&path).unwrap(), "Host b\n");
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("config.bak-"));
        // Dois backups no mesmo segundo não se sobrescrevem.
        let second = save_with_backup(&path, "Host c\n").unwrap().unwrap();
        assert_ne!(second, backup);
        assert_eq!(fs::read_to_string(&second).unwrap(), "Host b\n");
        // Nenhum temporário sobrando.
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in [&path, &backup] {
                let mode = fs::metadata(file).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{file:?}");
            }
            let dir_mode = fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dir_mode, 0o700);
        }
    }

    #[cfg(unix)]
    #[test]
    fn save_through_a_symlink_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("dotfiles-ssh-config");
        fs::write(&real, "Host a\n").unwrap();
        let link = dir.path().join("config");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let backup = save_with_backup(&link, "Host b\n").unwrap().unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "Host b\n");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "Host a\n");
    }

    #[test]
    fn unit_tests_never_point_at_the_users_file() {
        let path = ssh_config_path().unwrap();
        if std::env::var_os("CODE_TERMINAL_SSH_CONFIG").is_none() {
            assert!(path.starts_with(std::env::temp_dir()), "{path:?}");
        }
    }
}
