//! Metadados mostrados na sidebar de abas verticais: diretório, branch do
//! git e portas abertas pelos processos de cada tab.
//!
//! Tudo é lido de arquivos (`.git/HEAD`, `/proc`), sem chamar `git` ou `ss`:
//! a coleta roda no caminho de desenho e precisa ser barata.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Estado do repositório além da branch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GitStatus {
    /// Há mudanças não commitadas (inclui arquivos novos).
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
}

impl GitStatus {
    /// Sufixo curto: `*` sujo, `↑n` à frente, `↓n` atrás.
    pub fn suffix(&self) -> String {
        let mut out = String::new();
        if self.dirty {
            out.push('*');
        }
        if self.ahead > 0 {
            out.push_str(&format!(" ↑{}", self.ahead));
        }
        if self.behind > 0 {
            out.push_str(&format!(" ↓{}", self.behind));
        }
        out
    }
}

/// Lê a saída de `git status --porcelain=v1 --branch`.
pub fn parse_git_status(output: &str) -> GitStatus {
    let mut status = GitStatus::default();
    for line in output.lines() {
        match line.strip_prefix("## ") {
            Some(header) => (status.ahead, status.behind) = ahead_behind(header),
            None => status.dirty |= !line.trim().is_empty(),
        }
    }
    status
}

/// `branch...origin/branch [ahead 2, behind 1]` -> (2, 1).
fn ahead_behind(header: &str) -> (u32, u32) {
    let Some(start) = header.find('[') else {
        return (0, 0);
    };
    let count = |prefix: &str| {
        header[start + 1..]
            .trim_end_matches(']')
            .split(", ")
            .find_map(|part| part.strip_prefix(prefix))
            .and_then(|n| n.parse().ok())
            .unwrap_or(0)
    };
    (count("ahead "), count("behind "))
}

/// Roda `git status` numa thread de fundo e guarda o resultado por pasta:
/// a sidebar só lê o cache e nunca espera o git.
#[derive(Clone, Default)]
pub struct GitStatusCache {
    inner: std::sync::Arc<std::sync::Mutex<GitCacheInner>>,
}

#[derive(Default)]
struct GitCacheInner {
    statuses: HashMap<PathBuf, (GitStatus, std::time::Instant)>,
    running: HashSet<PathBuf>,
}

/// Idade máxima de um resultado antes de pedir de novo.
const GIT_STATUS_TTL: std::time::Duration = std::time::Duration::from_secs(5);

impl GitStatusCache {
    /// Último estado conhecido de `dir`; agenda uma atualização se estiver velho.
    pub fn get(&self, dir: &Path) -> Option<GitStatus> {
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        let cached = inner.statuses.get(dir).copied();
        let stale = cached.is_none_or(|(_, at)| at.elapsed() > GIT_STATUS_TTL);
        if stale && inner.running.insert(dir.to_path_buf()) {
            let cache = self.clone();
            let dir = dir.to_path_buf();
            let spawned = std::thread::Builder::new()
                .name("git-status".into())
                .spawn(move || cache.refresh(dir));
            if spawned.is_err() {
                tracing::debug!("Could not start the git status thread");
            }
        }
        cached.map(|(status, _)| status)
    }

    fn refresh(&self, dir: PathBuf) {
        let output = std::process::Command::new("git")
            .args([
                "status",
                "--porcelain=v1",
                "--branch",
                "--untracked-files=normal",
            ])
            .current_dir(&dir)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output();
        let status = output
            .ok()
            .filter(|out| out.status.success())
            .map(|out| parse_git_status(&String::from_utf8_lossy(&out.stdout)));
        if let Ok(mut inner) = self.inner.lock() {
            inner.running.remove(&dir);
            if let Some(status) = status {
                inner
                    .statuses
                    .insert(dir, (status, std::time::Instant::now()));
            }
        }
    }
}

/// Informações de uma tab para a sidebar.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabSidebarInfo {
    pub title: String,
    /// Diretório do painel em foco, com `~` no lugar da home.
    pub cwd: Option<String>,
    pub branch: Option<String>,
    /// Mudanças não commitadas e commits à frente/atrás.
    pub git: Option<GitStatus>,
    /// Portas TCP em escuta abertas por processos da tab.
    pub ports: Vec<u16>,
    /// Notificações não lidas da tab.
    pub unread: usize,
    /// Quantos painéis a tab tem.
    pub panes: usize,
    /// Memória (kB, PSS) dos processos da tab; `None` se não medida ainda,
    /// escondida na configuração ou fora do Linux.
    pub memory_kb: Option<u64>,
    /// Tab persistente (alfinete no cartão): sobrevive a fechar a janela.
    pub persistent: bool,
}

/// Troca o prefixo da home por `~`.
pub fn shorten_home(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if let Ok(rest) = path.strip_prefix(home) {
            return if rest.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rest.display())
            };
        }
    }
    path.display().to_string()
}

/// Branch do repositório que contém `dir`, lendo `.git/HEAD` (funciona em
/// worktrees, onde `.git` é um arquivo `gitdir: ...`). Em HEAD destacado,
/// devolve os 7 primeiros caracteres do commit.
pub fn git_branch(dir: &Path) -> Option<String> {
    let git_dir = find_git_dir(dir)?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(reference) => Some(
            reference
                .strip_prefix("refs/heads/")
                .unwrap_or(reference)
                .to_string(),
        ),
        None if head.len() >= 7 => Some(head[..7].to_string()),
        None => None,
    }
}

fn find_git_dir(dir: &Path) -> Option<PathBuf> {
    for ancestor in dir.ancestors() {
        let dot_git = ancestor.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        if dot_git.is_file() {
            let content = std::fs::read_to_string(&dot_git).ok()?;
            let target = content.trim().strip_prefix("gitdir:")?.trim();
            let target = PathBuf::from(target);
            return Some(if target.is_absolute() {
                target
            } else {
                ancestor.join(target)
            });
        }
    }
    None
}

/// Lê pares (pid, ppid) de `/proc`.
#[cfg(target_os = "linux")]
fn process_parents() -> Vec<(u32, u32)> {
    process_parents_in(Path::new("/proc"))
}

/// Lê pares (pid, ppid) de um diretório no formato de `/proc` (nos testes,
/// uma árvore falsa).
fn process_parents_in(proc_root: &Path) -> Vec<(u32, u32)> {
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat =
                std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
            Some((pid, parse_ppid(&stat)?))
        })
        .collect()
}

// === Memória por tab ===

/// Campo `Pss:` de `/proc/<pid>/smaps_rollup`, em kB.
///
/// PSS divide cada página compartilhada entre os processos que a usam: somar
/// o PSS do shell e dos filhos dá a memória que a tab de fato ocupa. Somar
/// RSS contaria várias vezes as bibliotecas compartilhadas entre eles.
pub fn parse_pss_kb(smaps_rollup: &str) -> Option<u64> {
    smaps_rollup.lines().find_map(|line| {
        line.strip_prefix("Pss:")?
            .trim()
            .strip_suffix("kB")?
            .trim()
            .parse()
            .ok()
    })
}

/// Memória residente (RSS) de `/proc/<pid>/statm`, em kB. Reserva para quando
/// `smaps_rollup` não existe (kernels antigos).
pub fn parse_statm_rss_kb(statm: &str, page_kb: u64) -> Option<u64> {
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * page_kb)
}

/// Memória (kB) de um processo: PSS se houver, senão RSS.
fn process_memory_kb(proc_root: &Path, pid: u32) -> Option<u64> {
    let dir = proc_root.join(pid.to_string());
    std::fs::read_to_string(dir.join("smaps_rollup"))
        .ok()
        .and_then(|text| parse_pss_kb(&text))
        .or_else(|| {
            let statm = std::fs::read_to_string(dir.join("statm")).ok()?;
            parse_statm_rss_kb(&statm, 4)
        })
}

/// Memória (kB) de `roots` e de todos os descendentes, lida de `proc_root`.
/// `None` se nenhum processo pôde ser lido.
pub fn tree_memory_kb(proc_root: &Path, roots: &[u32]) -> Option<u64> {
    if roots.is_empty() {
        return None;
    }
    let pids = descendants(roots, &process_parents_in(proc_root));
    let mut total = None;
    for pid in pids {
        if let Some(kb) = process_memory_kb(proc_root, pid) {
            *total.get_or_insert(0) += kb;
        }
    }
    total
}

/// Texto curto para a sidebar: `0.4 MB`, `38 MB`, `1.2 GB`.
pub fn format_memory(kb: u64) -> String {
    let mb = kb as f64 / 1024.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else if mb < 9.95 {
        // Um shell parado usa menos de 1 MB: sem a casa decimal, seria "0 MB".
        format!("{mb:.1} MB")
    } else {
        format!("{} MB", mb.round() as u64)
    }
}

/// Idade máxima da medição de memória de uma tab.
const MEMORY_TTL: std::time::Duration = std::time::Duration::from_secs(3);

/// Mede a memória das tabs numa thread de fundo e guarda por conjunto de
/// processos: a sidebar só lê o cache e nunca espera o `/proc`.
#[derive(Clone, Default)]
pub struct MemoryCache {
    inner: std::sync::Arc<std::sync::Mutex<MemoryCacheInner>>,
}

#[derive(Default)]
struct MemoryCacheInner {
    values: HashMap<Vec<u32>, (Option<u64>, std::time::Instant)>,
    running: HashSet<Vec<u32>>,
}

impl MemoryCache {
    /// Última medição (kB) dos processos `roots`; agenda outra se estiver velha.
    pub fn get(&self, roots: &[u32]) -> Option<u64> {
        if !cfg!(target_os = "linux") || roots.is_empty() {
            return None;
        }
        self.get_from(Path::new("/proc"), roots)
    }

    fn get_from(&self, proc_root: &Path, roots: &[u32]) -> Option<u64> {
        let mut key = roots.to_vec();
        key.sort_unstable();
        let Ok(mut inner) = self.inner.lock() else {
            return None;
        };
        // Tabs fechadas: esquece medições que ninguém pede há tempo.
        inner
            .values
            .retain(|_, (_, at)| at.elapsed() < MEMORY_TTL * 20);
        let cached = inner.values.get(&key).copied();
        let stale = cached.is_none_or(|(_, at)| at.elapsed() > MEMORY_TTL);
        if stale && inner.running.insert(key.clone()) {
            let cache = self.clone();
            let root = proc_root.to_path_buf();
            let spawned = std::thread::Builder::new()
                .name("tab-memory".into())
                .spawn(move || cache.refresh(&root, key));
            if let Err(error) = spawned {
                tracing::warn!(error = %error, "Failed to measure tab memory");
            }
        }
        cached.and_then(|(kb, _)| kb)
    }

    fn refresh(&self, proc_root: &Path, key: Vec<u32>) {
        let kb = tree_memory_kb(proc_root, &key);
        let changed = match self.inner.lock() {
            Ok(mut inner) => {
                inner.running.remove(&key);
                let previous = inner
                    .values
                    .insert(key, (kb, std::time::Instant::now()))
                    .and_then(|(old, _)| old);
                shown_differently(previous, kb)
            }
            Err(_) => false,
        };
        // Acorda a janela só se o número mostrado mudou: com a tab parada, a
        // medição se repete igual e o app continua dormindo.
        if changed {
            crate::terminal::pty::request_redraw();
        }
    }
}

/// A sidebar mostraria outro texto?
fn shown_differently(previous: Option<u64>, current: Option<u64>) -> bool {
    previous.map(format_memory) != current.map(format_memory)
}

/// PPID de uma linha de `/proc/<pid>/stat` (o nome do processo pode ter
/// espaços e parênteses; o campo vem depois do último `)`).
pub fn parse_ppid(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// `roots` e todos os seus descendentes.
pub fn descendants(roots: &[u32], parents: &[(u32, u32)]) -> HashSet<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for &(pid, ppid) in parents {
        children.entry(ppid).or_default().push(pid);
    }
    let mut seen: HashSet<u32> = roots.iter().copied().collect();
    let mut stack: Vec<u32> = roots.to_vec();
    while let Some(pid) = stack.pop() {
        for &child in children.get(&pid).into_iter().flatten() {
            if seen.insert(child) {
                stack.push(child);
            }
        }
    }
    seen
}

/// Sockets TCP em escuta: inode -> porta, de um arquivo `/proc/net/tcp{,6}`.
pub fn parse_listening_sockets(table: &str) -> HashMap<u64, u16> {
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // local_address está em fields[1] ("0100007F:1F90"), estado em fields[3].
            if fields.len() < 10 || fields[3] != "0A" {
                return None;
            }
            let port = u16::from_str_radix(fields[1].rsplit(':').next()?, 16).ok()?;
            let inode = fields[9].parse().ok()?;
            Some((inode, port))
        })
        .collect()
}

/// Portas TCP em escuta abertas pelos processos `roots` ou descendentes.
#[cfg(target_os = "linux")]
pub fn listening_ports_of(roots: &[u32]) -> Vec<u16> {
    if roots.is_empty() {
        return Vec::new();
    }
    let mut sockets = HashMap::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(content) = std::fs::read_to_string(table) {
            sockets.extend(parse_listening_sockets(&content));
        }
    }
    if sockets.is_empty() {
        return Vec::new();
    }
    let mut ports: Vec<u16> = descendants(roots, &process_parents())
        .into_iter()
        .filter_map(|pid| std::fs::read_dir(format!("/proc/{pid}/fd")).ok())
        .flat_map(|fds| fds.flatten())
        .filter_map(|fd| {
            let target = std::fs::read_link(fd.path()).ok()?;
            let inode = target
                .to_str()?
                .strip_prefix("socket:[")?
                .strip_suffix(']')?
                .parse::<u64>()
                .ok()?;
            sockets.get(&inode).copied()
        })
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Portas em escuta (não suportado fora do Linux).
#[cfg(not(target_os = "linux"))]
pub fn listening_ports_of(_roots: &[u32]) -> Vec<u16> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_home() {
        let home = Path::new("/home/example");
        assert_eq!(shorten_home(Path::new("/home/example"), Some(home)), "~");
        assert_eq!(
            shorten_home(Path::new("/home/example/proj/x"), Some(home)),
            "~/proj/x"
        );
        assert_eq!(shorten_home(Path::new("/tmp"), Some(home)), "/tmp");
        assert_eq!(shorten_home(Path::new("/tmp"), None), "/tmp");
    }

    #[test]
    fn test_git_branch_repo_worktree_and_detached() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("src/deep")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/feat/panes\n").unwrap();
        assert_eq!(
            git_branch(&repo.join("src/deep")).as_deref(),
            Some("feat/panes")
        );

        // Worktree: .git é um arquivo apontando para o gitdir.
        let wt_git = dir.path().join("gitdirs/wt");
        std::fs::create_dir_all(&wt_git).unwrap();
        std::fs::write(wt_git.join("HEAD"), "0123456789abcdef\n").unwrap();
        let wt = dir.path().join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", wt_git.display())).unwrap();
        assert_eq!(git_branch(&wt).as_deref(), Some("0123456"));

        assert_eq!(git_branch(dir.path()), None);
    }

    #[test]
    fn test_parse_git_status() {
        let clean = parse_git_status("## main...origin/main\n");
        assert_eq!(clean, GitStatus::default());
        assert_eq!(clean.suffix(), "");
        let busy = parse_git_status(
            "## feat...origin/feat [ahead 2, behind 1]\n M src/a.rs\n?? new.txt\n",
        );
        assert_eq!(
            busy,
            GitStatus {
                dirty: true,
                ahead: 2,
                behind: 1
            }
        );
        assert_eq!(busy.suffix(), "* ↑2 ↓1");
        assert_eq!(parse_git_status("## HEAD (no branch)\n").ahead, 0);
    }

    /// O cache devolve o estado real de um repositório depois que o git roda.
    #[test]
    fn test_git_status_cache_runs_in_background() {
        let dir = tempfile::tempdir().unwrap();
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return; // sem git no ambiente
        }
        std::fs::write(dir.path().join("novo.txt"), "x").unwrap();
        let cache = GitStatusCache::default();
        assert_eq!(cache.get(dir.path()), None);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut status = None;
        while status.is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
            status = cache.get(dir.path());
        }
        assert!(status.unwrap().dirty);
    }

    #[test]
    fn test_parse_ppid_with_spaces_in_name() {
        assert_eq!(parse_ppid("123 (my (weird) app) S 45 123 123 0"), Some(45));
        assert_eq!(parse_ppid("garbage"), None);
    }

    #[test]
    fn test_descendants() {
        let parents = [(2, 1), (3, 2), (4, 3), (5, 1), (6, 9)];
        let set = descendants(&[2], &parents);
        assert_eq!(set, HashSet::from([2, 3, 4]));
    }

    #[test]
    fn test_parse_listening_sockets() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 4242 1 0 100 0 0 10 0\n   1: 0100007F:0016 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 99 1 0 100 0 0 10 0\n";
        let sockets = parse_listening_sockets(table);
        assert_eq!(sockets, HashMap::from([(4242, 8080)]));
    }

    /// Uma porta aberta por um processo filho aparece para a tab dele.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_listening_ports_of_finds_child_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let ports = listening_ports_of(&[std::process::id()]);
        assert!(ports.contains(&port), "{ports:?} sem {port}");
        assert!(listening_ports_of(&[]).is_empty());
    }

    /// Monta um `/proc` falso: (pid, ppid, conteúdo de smaps_rollup ou None,
    /// conteúdo de statm ou None).
    fn fake_proc(entries: &[(u32, u32, Option<&str>, Option<&str>)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for (pid, ppid, rollup, statm) in entries {
            let dir = root.path().join(pid.to_string());
            std::fs::create_dir(&dir).unwrap();
            // Nome com espaço e parênteses, como acontece de verdade.
            std::fs::write(
                dir.join("stat"),
                format!("{pid} (my (proc)) S {ppid} 1 1 0"),
            )
            .unwrap();
            if let Some(rollup) = rollup {
                std::fs::write(dir.join("smaps_rollup"), rollup).unwrap();
            }
            if let Some(statm) = statm {
                std::fs::write(dir.join("statm"), statm).unwrap();
            }
        }
        std::fs::create_dir(root.path().join("self")).unwrap();
        root
    }

    fn rollup(pss_kb: u64) -> String {
        format!(
            "00400000-7ffe [rollup]\nRss:              {}\nPss:              {pss_kb} kB\nShared_Clean:          0 kB\n",
            "99999 kB"
        )
    }

    #[test]
    fn test_parse_pss_and_statm() {
        assert_eq!(parse_pss_kb(&rollup(1234)), Some(1234));
        assert_eq!(parse_pss_kb("Rss: 10 kB\n"), None);
        assert_eq!(parse_pss_kb("Pss: muito kB\n"), None);
        assert_eq!(parse_statm_rss_kb("100 25 3 1 0 20 0\n", 4), Some(100));
        assert_eq!(parse_statm_rss_kb("100\n", 4), None);
    }

    /// Soma o PSS do shell e de todos os descendentes, e só deles.
    #[test]
    fn test_tree_memory_sums_shell_and_children() {
        let (a, b, c, other) = (
            rollup(10_000),
            rollup(20_000),
            rollup(5_000),
            rollup(900_000),
        );
        let proc = fake_proc(&[
            (100, 1, Some(&a), None),     // shell da tab
            (200, 100, Some(&b), None),   // filho (ex.: claude)
            (300, 200, Some(&c), None),   // neto
            (400, 1, Some(&other), None), // outro programa, fora da tab
        ]);
        assert_eq!(tree_memory_kb(proc.path(), &[100]), Some(35_000));
        assert_eq!(tree_memory_kb(proc.path(), &[400]), Some(900_000));
        assert_eq!(tree_memory_kb(proc.path(), &[]), None);
        assert_eq!(tree_memory_kb(proc.path(), &[999]), None);
    }

    /// Sem smaps_rollup (kernel antigo ou processo de outro usuário), usa o
    /// RSS do statm; processos ilegíveis ficam de fora sem zerar a soma.
    #[test]
    fn test_tree_memory_falls_back_to_rss() {
        let a = rollup(8_000);
        let proc = fake_proc(&[
            (10, 1, Some(&a), None),
            (11, 10, None, Some("500 256 0 0 0 0 0")), // 256 páginas de 4 kB
            (12, 10, None, None),
        ]);
        assert_eq!(tree_memory_kb(proc.path(), &[10]), Some(8_000 + 1_024));
    }

    #[test]
    fn test_format_memory() {
        assert_eq!(format_memory(0), "0.0 MB");
        assert_eq!(format_memory(400), "0.4 MB");
        assert_eq!(format_memory(3 * 1024 + 200), "3.2 MB");
        assert_eq!(format_memory(10 * 1024), "10 MB");
        assert_eq!(format_memory(38 * 1024 + 300), "38 MB");
        assert_eq!(format_memory(1536 * 1024), "1.5 GB");
        assert!(!shown_differently(Some(38 * 1024), Some(38 * 1024 + 100)));
        assert!(shown_differently(Some(38 * 1024), Some(40 * 1024)));
        assert!(shown_differently(None, Some(1)));
    }

    /// A primeira leitura não bloqueia: devolve vazio e mede em segundo plano;
    /// a seguinte já tem o valor.
    #[test]
    fn test_memory_cache_measures_in_background() {
        let a = rollup(12_288);
        let proc = fake_proc(&[(50, 1, Some(&a), None)]);
        let cache = MemoryCache::default();
        assert_eq!(cache.get_from(proc.path(), &[50]), None);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut value = None;
        while value.is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
            value = cache.get_from(proc.path(), &[50]);
        }
        assert_eq!(value, Some(12_288));
        // A ordem dos pids não muda a chave.
        assert_eq!(cache.get_from(proc.path(), &[50]), Some(12_288));
    }
}
