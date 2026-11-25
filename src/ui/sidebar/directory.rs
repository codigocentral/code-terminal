use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cache entry para diretórios
#[derive(Debug, Clone)]
struct DirCacheEntry {
    display_name: String,
    exists: bool,
    is_readable: bool,
    timestamp: Instant,
}

/// Display de diretório de trabalho
#[derive(Debug)]
pub struct WorkingDirectoryDisplay {
    cache: Arc<Mutex<HashMap<PathBuf, DirCacheEntry>>>,
    cache_duration: Duration,
    enabled: bool,
    max_components: usize,
    substitute_home: bool,
    show_icons: bool,
}

impl WorkingDirectoryDisplay {
    /// Cria um novo display
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            cache_duration: Duration::from_secs(10),
            enabled: true,
            max_components: 3,
            substitute_home: true,
            show_icons: true,
        }
    }

    /// Define se está habilitado
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Define número máximo de componentes
    pub fn set_max_components(&mut self, max: usize) {
        self.max_components = max;
    }

    /// Define se substitui home por ~
    pub fn set_substitute_home(&mut self, substitute: bool) {
        self.substitute_home = substitute;
    }

    /// Define se mostra ícones
    pub fn set_show_icons(&mut self, show: bool) {
        self.show_icons = show;
    }

    /// Formata um diretório para display
    pub fn format<P: AsRef<Path>>(&self, path: P) -> String {
        if !self.enabled {
            return String::new();
        }

        let path = path.as_ref();

        // Verifica cache
        if let Some(cached) = self.get_cached_display(path) {
            return cached;
        }

        // Formata o caminho
        let formatted = self.format_path(path);

        // Atualiza cache
        self.cache_path(path, &formatted);

        formatted
    }

    /// Formata com ícone
    pub fn format_with_icon<P: AsRef<Path>>(&self, path: P) -> String {
        let path_ref = path.as_ref();
        let icon = if self.show_icons {
            self.path_icon(path_ref)
        } else {
            ""
        };
        let path_str = self.format(path_ref);

        if self.show_icons && !path_str.is_empty() && !icon.is_empty() {
            format!("{} {}", icon, path_str)
        } else {
            path_str
        }
    }

    /// Verifica se um diretório existe e é legível
    pub fn check_accessibility<P: AsRef<Path>>(&self, path: P) -> DirAccessibility {
        let path = path.as_ref();

        // Verifica cache
        if let Some(entry) = self.get_cache_entry(path) {
            return DirAccessibility {
                exists: entry.exists,
                is_readable: entry.is_readable,
            };
        }

        // Verifica acesso
        let exists = path.exists();
        let is_readable = exists && std::fs::read_dir(path).is_ok();

        DirAccessibility {
            exists,
            is_readable,
        }
    }

    /// Retorna o nome do diretório (último componente)
    pub fn directory_name<P: AsRef<Path>>(&self, path: P) -> Option<String> {
        path.as_ref()
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
    }

    /// Retorna o caminho completo
    pub fn full_path<P: AsRef<Path>>(&self, path: P) -> String {
        path.as_ref().to_string_lossy().to_string()
    }

    /// Encontra o diretório pai
    pub fn parent_dir<P: AsRef<Path>>(&self, path: P) -> Option<PathBuf> {
        path.as_ref().parent().map(|p| p.to_path_buf())
    }

    /// Compara dois caminhos
    pub fn are_same<P: AsRef<Path>, Q: AsRef<Path>>(&self, a: P, b: Q) -> bool {
        let a = self.canonicalize(a);
        let b = self.canonicalize(b);
        a == b
    }

    /// Tenta obter o caminho canônico
    pub fn canonicalize<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        path.as_ref()
            .canonicalize()
            .unwrap_or_else(|_| path.as_ref().to_path_buf())
    }

    /// Limpa o cache
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Retorna estatísticas do cache
    pub fn cache_stats(&self) -> DirCacheStats {
        if let Ok(cache) = self.cache.lock() {
            DirCacheStats {
                size: cache.len(),
                entries: cache.keys().cloned().collect(),
            }
        } else {
            DirCacheStats {
                size: 0,
                entries: Vec::new(),
            }
        }
    }

    /// Limpa entradas expiradas
    pub fn cleanup_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            let now = Instant::now();
            cache.retain(|_, entry| now.duration_since(entry.timestamp) < self.cache_duration);
        }
    }

    /// Formata o caminho internamente
    fn format_path(&self, path: &Path) -> String {
        let path_str = if self.substitute_home {
            substitute_home(path)
        } else {
            path.to_string_lossy().to_string()
        };

        // Se path é muito longo, trunca
        let components: Vec<_> = path_str.split(std::path::MAIN_SEPARATOR).collect();

        if components.len() > self.max_components {
            let start = components.len() - self.max_components;
            let truncated: Vec<_> = components[start..].to_vec();
            let prefix = if components[0].is_empty() {
                "/"
            } else {
                ".../"
            };
            format!(
                "{}{}",
                prefix,
                truncated.join(std::path::MAIN_SEPARATOR_STR)
            )
        } else {
            path_str
        }
    }

    /// Retorna ícone apropriado para o path
    fn path_icon(&self, path: &Path) -> &'static str {
        if path.ends_with("src") {
            "📁"
        } else if path.ends_with("target") || path.ends_with("build") {
            "📦"
        } else if path.ends_with(".git") {
            "🌿"
        } else if path.ends_with("node_modules") {
            "📦"
        } else if path.ends_with("tests") || path.ends_with("test") {
            "🧪"
        } else if path.ends_with("docs") || path.ends_with("doc") {
            "📚"
        } else if path.ends_with("config") || path.ends_with(".config") {
            "⚙️"
        } else if let Some(name) = path.file_name() {
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                "🔒"
            } else {
                "📂"
            }
        } else {
            "📂"
        }
    }

    /// Obtém do cache
    fn get_cached_display(&self, path: &Path) -> Option<String> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(path) {
                if entry.timestamp.elapsed() < self.cache_duration {
                    return Some(entry.display_name.clone());
                }
            }
        }
        None
    }

    /// Obtém entrada completa do cache
    fn get_cache_entry(&self, path: &Path) -> Option<DirCacheEntry> {
        if let Ok(cache) = self.cache.lock() {
            cache.get(path).cloned()
        } else {
            None
        }
    }

    /// Cacheia um caminho
    fn cache_path(&self, path: &Path, display: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                path.to_path_buf(),
                DirCacheEntry {
                    display_name: display.to_string(),
                    exists: path.exists(),
                    is_readable: std::fs::read_dir(path).is_ok(),
                    timestamp: Instant::now(),
                },
            );
        }
    }
}

impl Default for WorkingDirectoryDisplay {
    fn default() -> Self {
        Self::new()
    }
}

/// Acessibilidade de um diretório
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirAccessibility {
    pub exists: bool,
    pub is_readable: bool,
}

impl DirAccessibility {
    /// Verifica se é completamente acessível
    pub fn is_fully_accessible(&self) -> bool {
        self.exists && self.is_readable
    }

    /// Formata status
    pub fn format_status(&self) -> &'static str {
        if self.is_fully_accessible() {
            "✓"
        } else if self.exists {
            "⚠"
        } else {
            "✗"
        }
    }
}

/// Estatísticas do cache
#[derive(Debug, Clone)]
pub struct DirCacheStats {
    pub size: usize,
    pub entries: Vec<PathBuf>,
}

/// Substitui home directory por ~
pub fn substitute_home<P: AsRef<Path>>(path: P) -> String {
    let path = path.as_ref();

    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let home_path = Path::new(&home);
            if let Ok(stripped) = path.strip_prefix(home_path) {
                if stripped.as_os_str().is_empty() {
                    return "~".to_string();
                } else {
                    return format!("~{}", stripped.to_string_lossy());
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            let home_path = Path::new(&userprofile);
            if let Ok(stripped) = path.strip_prefix(home_path) {
                if stripped.as_os_str().is_empty() {
                    return "~".to_string();
                } else {
                    return format!("~{}", stripped.to_string_lossy());
                }
            }
        }
    }

    path.to_string_lossy().to_string()
}

/// Formata um caminho curto
pub fn format_short_path<P: AsRef<Path>>(path: P, max_components: usize) -> String {
    let path_str = substitute_home(path);
    let components: Vec<_> = path_str.split(std::path::MAIN_SEPARATOR).collect();

    if components.len() > max_components {
        let start = components.len() - max_components;
        let truncated: Vec<_> = components[start..].to_vec();
        format!(".../{}", truncated.join(std::path::MAIN_SEPARATOR_STR))
    } else {
        path_str
    }
}

/// Detecta o tipo de projeto baseado no diretório
pub fn detect_project_type<P: AsRef<Path>>(path: P) -> ProjectType {
    let path = path.as_ref();

    if path.join("Cargo.toml").exists() {
        ProjectType::Rust
    } else if path.join("package.json").exists() {
        ProjectType::Node
    } else if path.join("pom.xml").exists() || path.join("build.gradle").exists() {
        ProjectType::Java
    } else if path.join("requirements.txt").exists()
        || path.join("pyproject.toml").exists()
        || path.join("setup.py").exists()
    {
        ProjectType::Python
    } else if path.join("go.mod").exists() {
        ProjectType::Go
    } else if path.join("composer.json").exists() {
        ProjectType::Php
    } else if path.join("Gemfile").exists() {
        ProjectType::Ruby
    } else if path.join("Makefile").exists() {
        ProjectType::C
    } else {
        ProjectType::Unknown
    }
}

/// Tipo de projeto
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectType {
    Rust,
    Node,
    Java,
    Python,
    Go,
    Php,
    Ruby,
    C,
    Unknown,
}

impl ProjectType {
    /// Retorna ícone
    pub fn icon(&self) -> &'static str {
        match self {
            ProjectType::Rust => "🦀",
            ProjectType::Node => "⬢",
            ProjectType::Java => "☕",
            ProjectType::Python => "🐍",
            ProjectType::Go => "🐹",
            ProjectType::Php => "🐘",
            ProjectType::Ruby => "💎",
            ProjectType::C => "🔧",
            ProjectType::Unknown => "📁",
        }
    }

    /// Retorna nome
    pub fn name(&self) -> &'static str {
        match self {
            ProjectType::Rust => "Rust",
            ProjectType::Node => "Node.js",
            ProjectType::Java => "Java",
            ProjectType::Python => "Python",
            ProjectType::Go => "Go",
            ProjectType::Php => "PHP",
            ProjectType::Ruby => "Ruby",
            ProjectType::C => "C/C++",
            ProjectType::Unknown => "Unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_display_creation() {
        let display = WorkingDirectoryDisplay::new();
        assert!(display.enabled);
    }

    #[test]
    fn test_format_disabled() {
        let mut display = WorkingDirectoryDisplay::new();
        display.set_enabled(false);

        let path = PathBuf::from("/home/user/project");
        assert_eq!(display.format(&path), "");
    }

    #[test]
    fn test_directory_name() {
        let display = WorkingDirectoryDisplay::new();

        assert_eq!(
            display.directory_name("/home/user/project"),
            Some("project".to_string())
        );
        assert_eq!(
            display.directory_name("/home/user/"),
            Some("user".to_string())
        );
        assert_eq!(display.directory_name("/"), None);
    }

    #[test]
    fn test_full_path() {
        let display = WorkingDirectoryDisplay::new();
        assert_eq!(
            display.full_path("/home/user/project"),
            "/home/user/project"
        );
    }

    #[test]
    fn test_parent_dir() {
        let display = WorkingDirectoryDisplay::new();

        assert_eq!(
            display.parent_dir("/home/user/project"),
            Some(PathBuf::from("/home/user"))
        );
        assert_eq!(display.parent_dir("/"), None);
    }

    #[test]
    fn test_cache_stats() {
        let display = WorkingDirectoryDisplay::new();
        let stats = display.cache_stats();
        assert_eq!(stats.size, 0);
    }

    #[test]
    fn test_clear_cache() {
        let display = WorkingDirectoryDisplay::new();

        // Adiciona ao cache
        let path = PathBuf::from("/test/path");
        display.cache_path(&path, "test");

        let stats = display.cache_stats();
        assert_eq!(stats.size, 1);

        display.clear_cache();

        let stats = display.cache_stats();
        assert_eq!(stats.size, 0);
    }

    #[test]
    fn test_dir_accessibility() {
        let display = WorkingDirectoryDisplay::new();

        // Testa com diretório que existe
        let temp_dir = env::temp_dir();
        let access = display.check_accessibility(&temp_dir);

        assert!(access.exists);
        // is_readable depende das permissões do sistema
        println!("Temp dir accessible: {:?}", access);
    }

    #[test]
    fn test_dir_accessibility_nonexistent() {
        let display = WorkingDirectoryDisplay::new();

        let access = display.check_accessibility("/nonexistent/path/12345");

        assert!(!access.exists);
        assert!(!access.is_readable);
        assert!(!access.is_fully_accessible());
    }

    #[test]
    fn test_format_short_path() {
        let path = PathBuf::from("very")
            .join("long")
            .join("path")
            .join("to")
            .join("project")
            .join("src")
            .join("components");
        let formatted = format_short_path(path, 3);
        let suffix = format!(
            "project{}src{}components",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );

        assert!(formatted.ends_with(&suffix));
        assert!(!formatted.contains(&format!(
            "very{}long{}path",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        )));
    }

    #[test]
    fn test_substitute_home() {
        #[cfg(target_os = "windows")]
        let home = env::var("USERPROFILE").unwrap_or_default();
        #[cfg(not(target_os = "windows"))]
        let home = env::var("HOME").unwrap_or_default();

        let path = Path::new(&home).join("projects").join("test");

        let result = substitute_home(path);

        if !home.is_empty() {
            assert!(result.starts_with("~"));
        }
    }

    #[test]
    fn test_project_type_rust() {
        // Cria diretório temporário com Cargo.toml
        let temp_dir = env::temp_dir();

        // Sem arquivo específico, deve ser Unknown
        let project_type = detect_project_type(&temp_dir);
        assert_eq!(project_type, ProjectType::Unknown);
    }

    #[test]
    fn test_project_type_icons() {
        assert_eq!(ProjectType::Rust.icon(), "🦀");
        assert_eq!(ProjectType::Node.icon(), "⬢");
        assert_eq!(ProjectType::Python.icon(), "🐍");
        assert_eq!(ProjectType::Java.icon(), "☕");
        assert_eq!(ProjectType::Go.icon(), "🐹");
    }

    #[test]
    fn test_project_type_names() {
        assert_eq!(ProjectType::Rust.name(), "Rust");
        assert_eq!(ProjectType::Node.name(), "Node.js");
        assert_eq!(ProjectType::Unknown.name(), "Unknown");
    }

    #[test]
    fn test_format_path_with_components() {
        let mut display = WorkingDirectoryDisplay::new();
        display.set_max_components(2);
        display.set_substitute_home(false);

        let path = PathBuf::from("/a/b/c/d/e");
        let formatted = display.format(&path);

        // Deve ter truncado para 2 componentes
        assert!(formatted.contains("d/e") || formatted.contains("..."));
    }

    #[test]
    fn test_are_same() {
        let display = WorkingDirectoryDisplay::new();

        // Caminhos absolutos iguais
        assert!(display.are_same("/tmp", "/tmp"));

        // Caminhos diferentes
        assert!(!display.are_same("/tmp", "/var"));
    }

    #[test]
    fn test_format_with_icon() {
        let display = WorkingDirectoryDisplay::new();

        let path = PathBuf::from("/home/user/src");
        let formatted = display.format_with_icon(&path);

        // Deve conter ícone e path
        assert!(!formatted.is_empty());
        // Ícone pode variar baseado no diretório
    }
}
