use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cache entry para informações Git
#[derive(Debug, Clone)]
struct GitCacheEntry {
    branch: Option<String>,
    is_dirty: bool,
    ahead: usize,
    behind: usize,
    timestamp: Instant,
}

/// Detector de branch Git
#[derive(Debug)]
pub struct GitBranchDetector {
    /// Cache de branches detectadas
    cache: Arc<Mutex<HashMap<PathBuf, GitCacheEntry>>>,
    /// Duração do cache
    cache_duration: Duration,
    /// Habilitado
    enabled: bool,
}

impl GitBranchDetector {
    /// Cria um novo detector
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            cache_duration: Duration::from_secs(5),
            enabled: true,
        }
    }

    /// Cria com duração de cache customizada
    pub fn with_cache_duration(duration: Duration) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            cache_duration: duration,
            enabled: true,
        }
    }

    /// Define se está habilitado
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Verifica se está habilitado
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Detecta a branch Git em um diretório
    pub fn detect_branch<P: AsRef<Path>>(&self, path: P) -> Option<String> {
        if !self.enabled {
            return None;
        }

        let path = path.as_ref().to_path_buf();

        // Verifica cache
        if let Some(cached) = self.get_from_cache(&path) {
            return cached;
        }

        // Detecta branch
        let branch = self.execute_git_command(&path);

        // Atualiza cache
        if let Some(ref b) = branch {
            self.update_cache(path, b.clone());
        }

        branch
    }

    /// Detecta informações detalhadas do Git
    pub fn detect_full_info<P: AsRef<Path>>(&self, path: P) -> Option<GitInfo> {
        if !self.enabled {
            return None;
        }

        let path = path.as_ref();

        // Verifica se é um repositório git
        if !self.is_git_repository(path) {
            return None;
        }

        let branch = self.get_branch_name(path)?;
        let is_dirty = self.is_dirty(path);
        let (ahead, behind) = self.get_ahead_behind(path, &branch);
        let last_commit = self.get_last_commit_message(path);

        Some(GitInfo {
            branch,
            is_dirty,
            ahead,
            behind,
            last_commit,
        })
    }

    /// Verifica se o diretório é um repositório Git
    pub fn is_git_repository<P: AsRef<Path>>(&self, path: P) -> bool {
        let path = path.as_ref();

        // Verifica diretório .git
        if path.join(".git").exists() {
            return true;
        }

        // Ou tenta executar git status
        self.execute_git(path, &["rev-parse", "--git-dir"])
            .is_some()
    }

    /// Encontra o repositório Git raiz a partir de um caminho
    pub fn find_git_root<P: AsRef<Path>>(&self, path: P) -> Option<PathBuf> {
        let path = path.as_ref();

        let output = self.execute_git(path, &["rev-parse", "--show-toplevel"])?;
        let root = output.trim();

        if root.is_empty() {
            None
        } else {
            Some(PathBuf::from(root))
        }
    }

    /// Limpa o cache
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Define a duração do cache
    pub fn set_cache_duration(&mut self, duration: Duration) {
        self.cache_duration = duration;
    }

    /// Retorna estatísticas do cache
    pub fn cache_stats(&self) -> CacheStats {
        if let Ok(cache) = self.cache.lock() {
            CacheStats {
                size: cache.len(),
                entries: cache.keys().cloned().collect(),
                details: cache
                    .iter()
                    .map(|(path, entry)| CacheEntryStats {
                        path: path.clone(),
                        branch: entry.branch.clone(),
                        is_dirty: entry.is_dirty,
                        ahead: entry.ahead,
                        behind: entry.behind,
                    })
                    .collect(),
            }
        } else {
            CacheStats {
                size: 0,
                entries: Vec::new(),
                details: Vec::new(),
            }
        }
    }

    /// Limpa entradas expiradas do cache
    pub fn cleanup_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            let now = Instant::now();
            cache.retain(|_, entry| now.duration_since(entry.timestamp) < self.cache_duration);
        }
    }

    /// Retorna do cache se válido
    fn get_from_cache(&self, path: &Path) -> Option<Option<String>> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(path) {
                if entry.timestamp.elapsed() < self.cache_duration {
                    return Some(entry.branch.clone());
                }
            }
        }
        None
    }

    /// Atualiza o cache
    fn update_cache(&self, path: PathBuf, branch: String) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                path,
                GitCacheEntry {
                    branch: Some(branch),
                    is_dirty: false,
                    ahead: 0,
                    behind: 0,
                    timestamp: Instant::now(),
                },
            );
        }
    }

    /// Executa comando git para obter branch
    fn execute_git_command(&self, path: &Path) -> Option<String> {
        self.get_branch_name(path)
    }

    /// Obtém o nome da branch atual
    fn get_branch_name(&self, path: &Path) -> Option<String> {
        // Tenta symbolic-ref primeiro (branch normal)
        if let Some(output) = self.execute_git(path, &["symbolic-ref", "--short", "HEAD"]) {
            let branch = output.trim();
            if !branch.is_empty() {
                return Some(branch.to_string());
            }
        }

        // Se falhou, pode ser detached HEAD - tenta descrição
        if let Some(output) = self.execute_git(path, &["describe", "--all", "--contains", "HEAD"]) {
            let desc = output.trim();
            if !desc.is_empty() {
                // Remove prefixo como "tags/" ou "remotes/"
                let clean = desc
                    .trim_start_matches("tags/")
                    .trim_start_matches("remotes/");
                return Some(clean.to_string());
            }
        }

        // Último recurso: rev-parse
        if let Some(output) = self.execute_git(path, &["rev-parse", "--short", "HEAD"]) {
            let hash = output.trim();
            if !hash.is_empty() {
                return Some(hash.to_string());
            }
        }

        None
    }

    /// Verifica se há mudanças não commitadas
    fn is_dirty(&self, path: &Path) -> bool {
        self.execute_git(path, &["status", "--porcelain"])
            .map(|output| !output.trim().is_empty())
            .unwrap_or(false)
    }

    /// Obtém ahead/behind em relação ao upstream
    fn get_ahead_behind(&self, path: &Path, branch: &str) -> (usize, usize) {
        let output = match self.execute_git(
            path,
            &[
                "rev-list",
                "--left-right",
                "--count",
                // upstream é "@{u}" (antes faltava o "@" e o comando sempre
                // falhava, dando 0/0)
                &format!("{}...@{{u}}", branch),
            ],
        ) {
            Some(o) => o,
            None => return (0, 0),
        };

        // Formato esperado: "ahead\tbehind\n"
        let parts: Vec<_> = output.trim().split('\t').collect();
        if parts.len() == 2 {
            let ahead = parts[0].parse().unwrap_or(0);
            let behind = parts[1].parse().unwrap_or(0);
            (ahead, behind)
        } else {
            (0, 0)
        }
    }

    /// Obtém mensagem do último commit
    fn get_last_commit_message(&self, path: &Path) -> Option<String> {
        self.execute_git(path, &["log", "-1", "--pretty=%s"])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Executa um comando git
    fn execute_git(&self, path: &Path, args: &[&str]) -> Option<String> {
        let output = Command::new("git").args(args).current_dir(path).output();

        match output {
            Ok(result) if result.status.success() => String::from_utf8(result.stdout).ok(),
            _ => None,
        }
    }
}

impl Default for GitBranchDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Informações completas do Git
#[derive(Debug, Clone)]
pub struct GitInfo {
    pub branch: String,
    pub is_dirty: bool,
    pub ahead: usize,
    pub behind: usize,
    pub last_commit: Option<String>,
}

impl GitInfo {
    /// Formata para display compacto
    pub fn format_compact(&self) -> String {
        let mut result = self.branch.clone();

        if self.is_dirty {
            result.push('*');
        }

        if self.ahead > 0 || self.behind > 0 {
            result.push_str(&format!(" [+{} -{}]", self.ahead, self.behind));
        }

        result
    }

    /// Formata para display completo
    pub fn format_full(&self) -> String {
        let mut parts = vec![self.branch.clone()];

        if self.is_dirty {
            parts.push("(modified)".to_string());
        }

        if self.ahead > 0 {
            parts.push(format!("↑{}", self.ahead));
        }

        if self.behind > 0 {
            parts.push(format!("↓{}", self.behind));
        }

        if let Some(ref msg) = self.last_commit {
            parts.push(format!("\"{}\"", truncate(msg, 30)));
        }

        parts.join(" ")
    }

    /// Retorna ícone de status
    pub fn status_icon(&self) -> &'static str {
        if self.is_dirty {
            "●"
        } else if self.ahead > 0 || self.behind > 0 {
            "○"
        } else {
            "◉"
        }
    }
}

/// Estatísticas do cache
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub entries: Vec<PathBuf>,
    pub details: Vec<CacheEntryStats>,
}

/// Estatísticas detalhadas de uma entrada do cache
#[derive(Debug, Clone)]
pub struct CacheEntryStats {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_dirty: bool,
    pub ahead: usize,
    pub behind: usize,
}

/// Trunca uma string
fn truncate(s: &str, max_len: usize) -> String {
    // Conta caracteres, não bytes: `&s[..n]` entrava em pânico com acentos
    // (mensagens de commit em português)
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let kept: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", kept)
    }
}

/// Detecta branch git de forma síncrona (para uso simples)
pub fn detect_git_branch<P: AsRef<Path>>(path: P) -> Option<String> {
    let detector = GitBranchDetector::new();
    detector.detect_branch(path)
}

/// Verifica se é um repositório git
pub fn is_git_repo<P: AsRef<Path>>(path: P) -> bool {
    let detector = GitBranchDetector::new();
    detector.is_git_repository(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_detector_creation() {
        let detector = GitBranchDetector::new();
        assert!(detector.is_enabled());
    }

    #[test]
    fn test_detector_disabled() {
        let mut detector = GitBranchDetector::new();
        detector.set_enabled(false);
        assert!(!detector.is_enabled());

        // Não detecta quando desabilitado
        let path = PathBuf::from("/tmp");
        assert_eq!(detector.detect_branch(&path), None);
    }

    #[test]
    fn test_cache_duration() {
        let mut detector = GitBranchDetector::new();
        detector.set_cache_duration(Duration::from_secs(10));
        // A duração foi alterada (não podemos verificar diretamente)
    }

    #[test]
    fn test_cache_stats() {
        let detector = GitBranchDetector::new();
        let stats = detector.cache_stats();
        assert_eq!(stats.size, 0);
        assert!(stats.entries.is_empty());
    }

    #[test]
    fn test_clear_cache() {
        let detector = GitBranchDetector::new();

        // Adiciona entrada ao cache
        let path = PathBuf::from("/test/path");
        detector.update_cache(path.clone(), "main".to_string());

        let stats = detector.cache_stats();
        assert_eq!(stats.size, 1);

        detector.clear_cache();

        let stats = detector.cache_stats();
        assert_eq!(stats.size, 0);
    }

    #[test]
    fn test_git_info_format() {
        let info = GitInfo {
            branch: "feature/test".to_string(),
            is_dirty: true,
            ahead: 2,
            behind: 1,
            last_commit: Some("Add new feature".to_string()),
        };

        let compact = info.format_compact();
        assert!(compact.contains("feature/test"));
        assert!(compact.contains('*'));
        assert!(compact.contains("[+2 -1]"));

        let full = info.format_full();
        assert!(full.contains("feature/test"));
        assert!(full.contains("modified"));
        assert!(full.contains("Add new feature"));
    }

    #[test]
    fn test_git_info_clean() {
        let info = GitInfo {
            branch: "main".to_string(),
            is_dirty: false,
            ahead: 0,
            behind: 0,
            last_commit: None,
        };

        assert_eq!(info.format_compact(), "main");
        assert_eq!(info.status_icon(), "◉");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("this is a long message", 10), "this is...");
    }

    #[test]
    fn test_is_git_repository() {
        // Testa com diretório que não é git
        let detector = GitBranchDetector::new();

        // Cria diretório temporário sem .git
        let temp_dir = std::env::temp_dir();
        let is_git = detector.is_git_repository(&temp_dir);

        // Resultado depende se temp_dir é um repo git
        // Não fazemos assert específico pois depende do ambiente
        println!("Is git: {}", is_git);
    }

    #[test]
    fn test_cleanup_cache() {
        let detector = GitBranchDetector::new();

        // Adiciona entrada
        let path = PathBuf::from("/test/path");
        detector.update_cache(path.clone(), "main".to_string());

        // Limpa (entrada ainda é válida pois acabamos de criar)
        detector.cleanup_cache();

        let stats = detector.cache_stats();
        assert_eq!(stats.size, 1); // Ainda lá pois não expirou
    }

    // Testes que requerem git instalado são marcados como ignorados
    // pois dependem do ambiente
    #[test]
    #[ignore = "requires git repository"]
    fn test_detect_branch_in_repo() {
        let detector = GitBranchDetector::new();
        let path = PathBuf::from("."); // Diretório atual

        if detector.is_git_repository(&path) {
            let branch = detector.detect_branch(&path);
            assert!(branch.is_some());
            println!("Branch: {:?}", branch);
        }
    }

    #[test]
    #[ignore = "requires git repository"]
    fn test_find_git_root() {
        let detector = GitBranchDetector::new();
        let path = PathBuf::from(".");

        if let Some(root) = detector.find_git_root(&path) {
            assert!(root.exists());
            assert!(root.join(".git").exists());
        }
    }

    #[test]
    #[ignore = "requires git repository"]
    fn test_detect_full_info() {
        let detector = GitBranchDetector::new();
        let path = PathBuf::from(".");

        if let Some(info) = detector.detect_full_info(&path) {
            println!("Git info: {:?}", info);
            assert!(!info.branch.is_empty());
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(["-c", "user.name=Teste", "-c", "user.email=t@example.com"])
            .args([
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(status.status.success(), "git {:?}: {:?}", args, status);
    }

    /// Repositório com um commit em "main".
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]);
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "primeiro commit"]);
        dir
    }

    #[test]
    fn test_branch_detection_and_cache_in_temp_repo() {
        let r = repo();
        let d = GitBranchDetector::default();
        assert!(d.is_git_repository(r.path()));
        assert_eq!(d.detect_branch(r.path()).as_deref(), Some("main"));
        assert_eq!(d.cache_stats().size, 1);
        assert_eq!(d.cache_stats().details[0].branch.as_deref(), Some("main"));

        // dentro do cache a troca de branch não aparece
        git(r.path(), &["checkout", "-q", "-b", "feature"]);
        assert_eq!(d.detect_branch(r.path()).as_deref(), Some("main"));
        d.clear_cache();
        assert_eq!(d.detect_branch(r.path()).as_deref(), Some("feature"));
        assert_eq!(detect_git_branch(r.path()).as_deref(), Some("feature"));
        assert!(is_git_repo(r.path()));

        // subdiretório também é repositório e acha a raiz
        let sub = r.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        assert!(d.is_git_repository(&sub));
        assert_eq!(
            std::fs::canonicalize(d.find_git_root(&sub).unwrap()).unwrap(),
            std::fs::canonicalize(r.path()).unwrap()
        );

        // cache expirado é recalculado e limpo
        let mut short = GitBranchDetector::with_cache_duration(Duration::from_millis(0));
        assert_eq!(short.detect_branch(r.path()).as_deref(), Some("feature"));
        short.cleanup_cache();
        assert_eq!(short.cache_stats().size, 0);
        short.set_cache_duration(Duration::from_secs(60));
        short.set_enabled(false);
        assert!(!short.is_enabled());
        assert!(short.detect_branch(r.path()).is_none());
        assert!(short.detect_full_info(r.path()).is_none());
    }

    #[test]
    fn test_detached_head_uses_describe_or_hash() {
        let r = repo();
        git(r.path(), &["tag", "v1"]);
        git(r.path(), &["checkout", "-q", "--detach"]);
        let d = GitBranchDetector::new();
        let name = d.detect_branch(r.path()).unwrap();
        // "git describe --all --contains" acha a tag (sem o prefixo "tags/")
        assert!(!name.starts_with("tags/"));
        assert!(!name.is_empty());
    }

    #[test]
    fn test_full_info_dirty_and_ahead_behind() {
        let origin = repo();
        let clone = tempfile::tempdir().unwrap();
        git(
            clone.path(),
            &["clone", "-q", origin.path().to_str().unwrap(), "."],
        );
        // um commit local à frente e um no origin atrás
        std::fs::write(clone.path().join("b.txt"), "b").unwrap();
        git(clone.path(), &["add", "."]);
        git(
            clone.path(),
            &[
                "commit",
                "-q",
                "-m",
                "Correção de formatação da saída do terminal",
            ],
        );
        std::fs::write(origin.path().join("c.txt"), "c").unwrap();
        git(origin.path(), &["add", "."]);
        git(origin.path(), &["commit", "-q", "-m", "remoto"]);
        git(clone.path(), &["fetch", "-q"]);

        let d = GitBranchDetector::new();
        let info = d.detect_full_info(clone.path()).unwrap();
        assert_eq!(info.branch, "main");
        assert!(!info.is_dirty);
        // Regressão: faltava o "@" em "@{u}" e o resultado era sempre 0/0
        assert_eq!((info.ahead, info.behind), (1, 1));
        assert_eq!(info.status_icon(), "○");
        assert_eq!(info.format_compact(), "main [+1 -1]");
        // Regressão: truncar a mensagem acentuada não pode entrar em pânico
        assert_eq!(
            info.format_full(),
            "main ↑1 ↓1 \"Correção de formatação da s...\""
        );

        std::fs::write(clone.path().join("novo.txt"), "x").unwrap();
        let info = d.detect_full_info(clone.path()).unwrap();
        assert!(info.is_dirty);
        assert_eq!(info.status_icon(), "●");
        assert!(info.format_full().contains("(modified)"));

        // sem upstream: 0/0
        let info = d.detect_full_info(origin.path()).unwrap();
        assert_eq!((info.ahead, info.behind), (0, 0));
        assert_eq!(info.last_commit.as_deref(), Some("remoto"));
    }

    #[test]
    fn test_outside_repository() {
        let dir = tempfile::tempdir().unwrap();
        let d = GitBranchDetector::new();
        assert!(!d.is_git_repository(dir.path()));
        assert!(d.detect_full_info(dir.path()).is_none());
        assert!(d.find_git_root(dir.path()).is_none());
        assert!(d.detect_branch(dir.path()).is_none());
        // resultado negativo não é guardado no cache
        assert_eq!(d.cache_stats().size, 0);
    }

    #[test]
    fn test_truncate_counts_chars() {
        assert_eq!(truncate("curto", 10), "curto");
        assert_eq!(truncate("ação ação ação", 8), "ação ...");
        assert_eq!(truncate("abcdef", 2), "...");
    }
}
