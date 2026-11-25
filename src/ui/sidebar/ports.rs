use std::collections::{HashMap, HashSet};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cache entry para portas
#[derive(Debug, Clone)]
struct PortCacheEntry {
    ports: Vec<u16>,
    timestamp: Instant,
}

/// Detector de portas
#[derive(Debug)]
pub struct PortDetector {
    cache: Arc<Mutex<HashMap<String, PortCacheEntry>>>,
    cache_duration: Duration,
    enabled: bool,
    common_ports: Vec<u16>,
    scan_range: (u16, u16),
}

impl PortDetector {
    /// Cria um novo detector
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            cache_duration: Duration::from_secs(30),
            enabled: true,
            common_ports: vec![3000, 8080, 8000, 4200, 5000, 9000, 3030, 5173, 8081],
            scan_range: (3000, 9000),
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

    /// Define portas comuns
    pub fn set_common_ports(&mut self, ports: Vec<u16>) {
        self.common_ports = ports;
    }

    /// Define range de scan
    pub fn set_scan_range(&mut self, start: u16, end: u16) {
        self.scan_range = (start, end);
    }

    /// Define duração do cache
    pub fn set_cache_duration(&mut self, duration: Duration) {
        self.cache_duration = duration;
    }

    /// Detecta portas em uso no sistema
    pub fn detect_system_ports(&self) -> Vec<PortInfo> {
        if !self.enabled {
            return Vec::new();
        }

        // Tenta obter portas do sistema
        #[cfg(target_os = "linux")]
        {
            self.detect_linux_ports()
        }

        #[cfg(target_os = "macos")]
        {
            self.detect_macos_ports()
        }

        #[cfg(target_os = "windows")]
        {
            self.detect_windows_ports()
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Vec::new()
        }
    }

    /// Detecta portas associadas a um diretório
    pub fn detect_ports_in_dir<P: AsRef<Path>>(&self, _path: P) -> Vec<u16> {
        if !self.enabled {
            return Vec::new();
        }

        // Verifica cache
        let path_str = _path.as_ref().to_string_lossy().to_string();
        if let Some(cached) = self.get_cached_ports(&path_str) {
            return cached;
        }

        // Detecta portas comuns que estão em uso
        let ports = self.scan_common_ports();

        // Atualiza cache
        self.update_cache(path_str, ports.clone());

        ports
    }

    /// Verifica se uma porta específica está em uso
    pub fn is_port_in_use(&self, port: u16) -> bool {
        TcpListener::bind(format!("127.0.0.1:{}", port)).is_err()
    }

    /// Encontra próxima porta disponível
    pub fn find_available_port(&self, start: u16, end: u16) -> Option<u16> {
        (start..=end).find(|&port| !self.is_port_in_use(port))
    }

    /// Scan de portas no range especificado
    pub fn scan_port_range(&self, start: u16, end: u16) -> Vec<u16> {
        let mut open_ports = Vec::new();

        for port in start..=end {
            if self.is_port_in_use(port) {
                open_ports.push(port);
            }
        }

        open_ports
    }

    /// Scan das portas comuns
    pub fn scan_common_ports(&self) -> Vec<u16> {
        self.common_ports
            .iter()
            .filter(|&&p| self.is_port_in_use(p))
            .copied()
            .collect()
    }

    /// Limpa o cache
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    /// Retorna estatísticas do cache
    pub fn cache_stats(&self) -> PortCacheStats {
        if let Ok(cache) = self.cache.lock() {
            PortCacheStats {
                size: cache.len(),
                entries: cache.keys().cloned().collect(),
            }
        } else {
            PortCacheStats {
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

    /// Formata informação de porta
    pub fn format_port_info(&self, port: u16) -> String {
        let service = self.guess_service(port);
        format!("{} ({})", port, service)
    }

    /// Tenta adivinhar o serviço baseado na porta
    pub fn guess_service(&self, port: u16) -> &'static str {
        match port {
            80 => "HTTP",
            443 => "HTTPS",
            22 => "SSH",
            21 => "FTP",
            25 => "SMTP",
            3306 => "MySQL",
            5432 => "PostgreSQL",
            27017 => "MongoDB",
            6379 => "Redis",
            3000 => "React/Node",
            8080 => "HTTP-Alt",
            8000 => "HTTP-Dev",
            4200 => "Angular",
            5000 => "Flask",
            5173 => "Vite",
            9000 => "PHP-FPM",
            _ => "Unknown",
        }
    }

    /// Obtém do cache
    fn get_cached_ports(&self, key: &str) -> Option<Vec<u16>> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.get(key) {
                if entry.timestamp.elapsed() < self.cache_duration {
                    return Some(entry.ports.clone());
                }
            }
        }
        None
    }

    /// Atualiza o cache
    fn update_cache(&self, key: String, ports: Vec<u16>) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                key,
                PortCacheEntry {
                    ports,
                    timestamp: Instant::now(),
                },
            );
        }
    }

    /// Detecta portas no Linux usando netstat ou ss
    #[cfg(target_os = "linux")]
    fn detect_linux_ports(&self) -> Vec<PortInfo> {
        // Tenta ss primeiro (mais moderno)
        let output = Command::new("ss").args(["-tlnp"]).output();

        if let Ok(result) = output {
            if result.status.success() {
                return self.parse_ss_output(&String::from_utf8_lossy(&result.stdout));
            }
        }

        // Fallback para netstat
        let output = Command::new("netstat").args(["-tlnp"]).output();

        if let Ok(result) = output {
            if result.status.success() {
                return self.parse_netstat_output(&String::from_utf8_lossy(&result.stdout));
            }
        }

        Vec::new()
    }

    /// Detecta portas no macOS usando netstat ou lsof
    #[cfg(target_os = "macos")]
    fn detect_macos_ports(&self) -> Vec<PortInfo> {
        let output = Command::new("netstat").args(["-anv", "-p", "tcp"]).output();

        if let Ok(result) = output {
            if result.status.success() {
                return self.parse_netstat_output(&String::from_utf8_lossy(&result.stdout));
            }
        }

        Vec::new()
    }

    /// Detecta portas no Windows usando netstat
    #[cfg(target_os = "windows")]
    fn detect_windows_ports(&self) -> Vec<PortInfo> {
        let output = Command::new("netstat").args(["-ano", "-p", "tcp"]).output();

        if let Ok(result) = output {
            if result.status.success() {
                return self.parse_netstat_output(&String::from_utf8_lossy(&result.stdout));
            }
        }

        Vec::new()
    }

    /// Parse da saída do ss
    #[cfg(target_os = "linux")]
    fn parse_ss_output(&self, output: &str) -> Vec<PortInfo> {
        let mut ports = Vec::new();
        let mut seen = HashSet::new();

        for line in output.lines().skip(1) {
            // Formato: State  Recv-Q  Send-Q  Local Address:Port  Peer Address:Port  Process
            // Colunas: State Recv-Q Send-Q Local:Porta Peer:Porta [Process].
            // Antes lia o endereço do peer ("0.0.0.0:*") e nunca achava porta.
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                if let Some(port) = self.extract_port_from_address(parts[3]) {
                    if seen.insert(port) {
                        ports.push(PortInfo {
                            port,
                            protocol: "TCP".to_string(),
                            process_name: parts.get(5).map(|s| s.to_string()),
                            is_listening: parts[0] == "LISTEN",
                        });
                    }
                }
            }
        }

        ports
    }

    /// Parse da saída do netstat
    fn parse_netstat_output(&self, output: &str) -> Vec<PortInfo> {
        let mut ports = Vec::new();
        let mut seen = HashSet::new();

        for line in output.lines() {
            // Procura por linhas com portas
            if line.contains("LISTEN") || line.contains("0.0.0.0:") || line.contains("127.0.0.1:") {
                // Tenta extrair porta
                for word in line.split_whitespace() {
                    if let Some(port) = self.extract_port_from_address(word) {
                        if seen.insert(port) {
                            ports.push(PortInfo {
                                port,
                                protocol: "TCP".to_string(),
                                process_name: None,
                                is_listening: true,
                            });
                        }
                    }
                }
            }
        }

        ports
    }

    /// Extrai número da porta de um endereço (ex: "127.0.0.1:8080")
    fn extract_port_from_address(&self, address: &str) -> Option<u16> {
        // Exige "host:porta": sem isso colunas numéricas como o Recv-Q ("0")
        // do netstat viravam a porta 0
        address.rsplit_once(':')?.1.parse().ok()
    }
}

impl Default for PortDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Informações de uma porta
#[derive(Debug, Clone)]
pub struct PortInfo {
    pub port: u16,
    pub protocol: String,
    pub process_name: Option<String>,
    pub is_listening: bool,
}

impl PortInfo {
    /// Formata para display
    pub fn format(&self) -> String {
        let process = self
            .process_name
            .as_ref()
            .map(|p| format!(" - {}", p))
            .unwrap_or_default();

        format!("{}:{}{}", self.protocol, self.port, process)
    }

    /// Retorna ícone
    pub fn icon(&self) -> &'static str {
        if self.is_listening {
            "📡"
        } else {
            "🔌"
        }
    }
}

/// Estatísticas do cache
#[derive(Debug, Clone)]
pub struct PortCacheStats {
    pub size: usize,
    pub entries: Vec<String>,
}

/// Verifica rapidamente se uma porta está em uso
pub fn is_port_available(port: u16) -> bool {
    TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok()
}

/// Encontra porta disponível no range
pub fn find_free_port(start: u16, end: u16) -> Option<u16> {
    let detector = PortDetector::new();
    detector.find_available_port(start, end)
}

/// Lista portas comuns em uso
pub fn list_common_ports_in_use() -> Vec<u16> {
    let detector = PortDetector::new();
    detector.scan_common_ports()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detector_creation() {
        let detector = PortDetector::new();
        assert!(detector.is_enabled());
    }

    #[test]
    fn test_detector_disabled() {
        let mut detector = PortDetector::new();
        detector.set_enabled(false);
        assert!(!detector.is_enabled());
        assert!(detector.detect_system_ports().is_empty());
    }

    #[test]
    fn test_common_ports() {
        let detector = PortDetector::new();
        let ports = detector.scan_common_ports();

        // Resultado depende do sistema, mas não deve falhar
        println!("Common ports in use: {:?}", ports);
    }

    #[test]
    fn test_is_port_in_use() {
        let detector = PortDetector::new();

        // Porta 1 geralmente não está em uso
        let port_1_in_use = detector.is_port_in_use(1);
        println!("Port 1 in use: {}", port_1_in_use);

        // Porta 0 é inválida
        let port_0_in_use = detector.is_port_in_use(0);
        println!("Port 0 in use: {}", port_0_in_use);
    }

    #[test]
    fn test_find_available_port() {
        let detector = PortDetector::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let start = listener.local_addr().unwrap().port();
        if start > u16::MAX - 10 {
            return;
        }

        let available = detector.find_available_port(start, start + 10);

        if let Some(port) = available {
            assert_ne!(port, start);
            assert!((start + 1..=start + 10).contains(&port));
        }
    }

    #[test]
    fn test_scan_port_range() {
        let detector = PortDetector::new();

        // Scan de range pequeno
        let open_ports = detector.scan_port_range(1, 10);

        // Resultado depende do sistema
        println!("Open ports 1-10: {:?}", open_ports);
    }

    #[test]
    fn test_guess_service() {
        let detector = PortDetector::new();

        assert_eq!(detector.guess_service(80), "HTTP");
        assert_eq!(detector.guess_service(443), "HTTPS");
        assert_eq!(detector.guess_service(22), "SSH");
        assert_eq!(detector.guess_service(3000), "React/Node");
        assert_eq!(detector.guess_service(9999), "Unknown");
    }

    #[test]
    fn test_format_port_info() {
        let detector = PortDetector::new();

        let formatted = detector.format_port_info(8080);
        assert!(formatted.contains("8080"));
        assert!(formatted.contains("HTTP-Alt"));
    }

    #[test]
    fn test_cache() {
        let detector = PortDetector::new();

        // Adiciona ao cache
        detector.update_cache("test".to_string(), vec![3000, 8080]);

        let stats = detector.cache_stats();
        assert_eq!(stats.size, 1);

        // Recupera do cache
        let cached = detector.get_cached_ports("test");
        assert_eq!(cached, Some(vec![3000, 8080]));

        // Limpa cache
        detector.clear_cache();
        let stats = detector.cache_stats();
        assert_eq!(stats.size, 0);
    }

    #[test]
    fn test_extract_port_from_address() {
        let detector = PortDetector::new();

        assert_eq!(
            detector.extract_port_from_address("127.0.0.1:8080"),
            Some(8080)
        );
        assert_eq!(
            detector.extract_port_from_address("0.0.0.0:3000"),
            Some(3000)
        );
        assert_eq!(detector.extract_port_from_address("[::]:80"), Some(80));
        assert_eq!(detector.extract_port_from_address("invalid"), None);
    }

    #[test]
    fn test_port_info() {
        let info = PortInfo {
            port: 8080,
            protocol: "TCP".to_string(),
            process_name: Some("node".to_string()),
            is_listening: true,
        };

        let formatted = info.format();
        assert!(formatted.contains("TCP:8080"));
        assert!(formatted.contains("node"));
        assert_eq!(info.icon(), "📡");
    }

    #[test]
    fn test_port_info_no_process() {
        let info = PortInfo {
            port: 3000,
            protocol: "TCP".to_string(),
            process_name: None,
            is_listening: false,
        };

        let formatted = info.format();
        assert_eq!(formatted, "TCP:3000");
        assert_eq!(info.icon(), "🔌");
    }

    #[test]
    fn test_is_port_available() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        assert!(!is_port_available(port));

        let detector = PortDetector::new();
        assert_eq!(is_port_available(port), !detector.is_port_in_use(port));
    }

    #[test]
    fn test_find_free_port_helper() {
        // Ocupa uma porta e confere que ela não é devolvida como livre. (Antes o
        // teste conferia de novo a porta achada, e outro teste paralelo podia
        // ocupá-la no meio do caminho.)
        let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = busy.local_addr().unwrap().port();
        assert_eq!(find_free_port(port, port), None);
        assert!(!is_port_available(port));
        drop(busy);
    }

    #[test]
    fn test_list_common_ports_in_use() {
        let ports = list_common_ports_in_use();
        println!("Common ports in use: {:?}", ports);
        // Resultado depende do sistema
    }

    #[test]
    fn test_cleanup_cache() {
        let detector = PortDetector::new();

        detector.update_cache("test".to_string(), vec![3000]);

        let stats = detector.cache_stats();
        assert_eq!(stats.size, 1);

        // Limpa (entrada ainda deve estar lá se não expirou)
        detector.cleanup_cache();

        let stats = detector.cache_stats();
        assert_eq!(stats.size, 1); // Ainda lá
    }

    #[cfg(target_os = "linux")]
    const SS_SAMPLE: &str = "\
State  Recv-Q Send-Q Local Address:Port  Peer Address:PortProcess
LISTEN 0      4096       127.0.0.1:18025      0.0.0.0:*    users:((\"mailpit\",pid=10,fd=3))
LISTEN 0      511             [::]:3000          [::]:*
LISTEN 0      511          0.0.0.0:3000       0.0.0.0:*
";

    const NETSTAT_SAMPLE: &str = "\
Active Internet connections (only servers)
Proto Recv-Q Send-Q Local Address           Foreign Address         State       PID/Program name
tcp        0      0 0.0.0.0:22              0.0.0.0:*               LISTEN      812/sshd
tcp        0      0 127.0.0.1:5432          0.0.0.0:*               LISTEN      900/postgres
tcp6       0      0 :::8080                 :::*                    LISTEN      1/java
";

    #[cfg(target_os = "linux")]
    #[test]
    fn test_parse_ss_output_reads_local_address() {
        // Regressão: lia a coluna do peer e a lista saía sempre vazia
        let d = PortDetector::new();
        let ports = d.parse_ss_output(SS_SAMPLE);
        let nums: Vec<u16> = ports.iter().map(|p| p.port).collect();
        assert_eq!(nums, vec![18025, 3000]);
        assert!(ports.iter().all(|p| p.is_listening && p.protocol == "TCP"));
        assert_eq!(
            ports[0].process_name.as_deref(),
            Some("users:((\"mailpit\",pid=10,fd=3))")
        );
        assert_eq!(ports[1].process_name, None);
        assert!(d.parse_ss_output("só cabeçalho").is_empty());
    }

    #[test]
    fn test_parse_netstat_output_ignores_numeric_columns() {
        // Regressão: "0" (Recv-Q) era aceito como porta 0
        let d = PortDetector::new();
        let nums: Vec<u16> = d
            .parse_netstat_output(NETSTAT_SAMPLE)
            .iter()
            .map(|p| p.port)
            .collect();
        assert_eq!(nums, vec![22, 5432, 8080]);
        assert_eq!(d.extract_port_from_address("0"), None);
        assert_eq!(d.extract_port_from_address("[::1]:9000"), Some(9000));
        assert_eq!(d.extract_port_from_address("host:*"), None);
    }

    #[test]
    fn test_detect_ports_in_dir_scans_and_caches() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut d = PortDetector::default();
        d.set_common_ports(vec![port]);
        d.set_scan_range(port, port);
        assert_eq!(d.detect_ports_in_dir("/projeto"), vec![port]);
        assert_eq!(d.cache_stats().size, 1);
        assert_eq!(d.cache_stats().entries, vec!["/projeto".to_string()]);
        // do cache, mesmo depois de liberar a porta
        drop(listener);
        assert_eq!(d.detect_ports_in_dir("/projeto"), vec![port]);
        d.set_cache_duration(Duration::from_millis(0));
        d.cleanup_cache();
        assert_eq!(d.cache_stats().size, 0);
        // Sem cache, a varredura é nova: a porta só aparece se alguém a abriu
        // de novo (outro teste em paralelo pode reaproveitar a porta efêmera
        // liberada acima, o que derrubava a pipeline de vez em quando).
        let rescanned = d.detect_ports_in_dir("/projeto");
        let reopened = std::net::TcpStream::connect(("127.0.0.1", port)).is_ok();
        assert!(rescanned.is_empty() || reopened, "{rescanned:?}");
        d.set_enabled(false);
        assert!(d.detect_ports_in_dir("/projeto").is_empty());
    }

    #[test]
    fn test_scan_range_and_free_port_helpers() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let d = PortDetector::new();
        assert!(d.is_port_in_use(port));
        assert_eq!(d.scan_port_range(port, port), vec![port]);
        assert!(!is_port_available(port));
        assert_ne!(find_free_port(port, port.saturating_add(20)), Some(port));
        let _ = list_common_ports_in_use();
        // no Linux usa ss/netstat; não pode falhar mesmo sem as ferramentas
        let _ = d.detect_system_ports();
    }

    #[test]
    fn test_service_names_and_port_info_format() {
        let d = PortDetector::new();
        for (p, name) in [
            (21, "FTP"),
            (25, "SMTP"),
            (3306, "MySQL"),
            (5432, "PostgreSQL"),
            (27017, "MongoDB"),
            (6379, "Redis"),
            (8000, "HTTP-Dev"),
            (4200, "Angular"),
            (5000, "Flask"),
            (5173, "Vite"),
            (9000, "PHP-FPM"),
            (12345, "Unknown"),
        ] {
            assert_eq!(d.guess_service(p), name);
        }
        assert_eq!(d.format_port_info(5432), "5432 (PostgreSQL)");
        let info = PortInfo {
            port: 80,
            protocol: "TCP".into(),
            process_name: Some("nginx".into()),
            is_listening: false,
        };
        assert_eq!(info.format(), "TCP:80 - nginx");
        assert_eq!(info.icon(), "🔌");
    }
}
