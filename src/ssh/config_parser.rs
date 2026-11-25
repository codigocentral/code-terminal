//! Parser de configuração SSH (~/.ssh/config)
//!
//! Este módulo implementa um parser completo para arquivos de configuração SSH,
//! suportando diretivas padrão do OpenSSH, includes, patterns e herança.
//!
//! # Exemplo de uso
//!
//! ```rust,no_run
//! use code_terminal::ssh::SshConfigParser;
//!
//! // Parse do arquivo ~/.ssh/config padrão
//! let parser = SshConfigParser::parse_default_config().unwrap();
//!
//! // Buscar configuração efetiva para um host
//! if let Some(host) = parser.get_effective_config("myserver") {
//!     println!("Conectar em: {}", host.connection_string());
//! }
//! ```

use crate::ssh::error::{SshError, SshResult};
use crate::ssh::host::{DynamicForward, LogLevel, PortForward, SshHost, StrictHostKeyChecking};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct SshConfigParser {
    hosts: Vec<SshHost>,
    includes: Vec<PathBuf>,
}

impl Default for SshConfigParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SshConfigParser {
    pub fn new() -> Self {
        Self {
            hosts: Vec::new(),
            includes: Vec::new(),
        }
    }

    pub fn parse_default_config() -> SshResult<Self> {
        let config_path = Self::default_config_path()?;
        Self::from_path(&config_path)
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> SshResult<Self> {
        let path = path.as_ref();
        debug!("Parsing SSH config from: {:?}", path);

        if !path.exists() {
            return Err(SshError::ConfigNotFound);
        }

        let content = fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    pub fn from_str(content: &str) -> SshResult<Self> {
        let mut parser = Self::new();

        let mut current_host: Option<SshHost> = None;

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let (directive, value) = Self::parse_line(trimmed, line_num + 1)?;

            match directive.to_lowercase().as_str() {
                "host" => {
                    if let Some(host) = current_host.take() {
                        parser.hosts.push(host);
                    }
                    current_host = Some(SshHost::new(value));
                }
                "match" => {
                    debug!("Match directive not yet supported: {}", value);
                }
                "include" => {
                    if let Ok(resolved) = Self::resolve_include_path(&value) {
                        parser.includes.push(resolved);
                    }
                }
                _ => {
                    if let Some(ref mut host) = current_host {
                        parser.apply_directive(host, &directive, &value)?;
                    } else {
                        debug!("Directive '{}' outside Host block, ignoring", directive);
                    }
                }
            }
        }

        if let Some(host) = current_host {
            parser.hosts.push(host);
        }

        debug!("Parsed {} host(s) from config", parser.hosts.len());
        Ok(parser)
    }

    fn parse_line(line: &str, line_num: usize) -> SshResult<(String, String)> {
        let line = line.split('#').next().unwrap_or("").trim();

        if line.is_empty() {
            return Err(SshError::ParseError(format!(
                "Line {}: Empty directive",
                line_num
            )));
        }

        match line.split_once(char::is_whitespace) {
            None => Err(SshError::MissingValue(line.to_string())),
            Some((directive, value)) => Ok((directive.to_string(), value.trim().to_string())),
        }
    }

    fn apply_directive(&self, host: &mut SshHost, directive: &str, value: &str) -> SshResult<()> {
        match directive.to_lowercase().as_str() {
            "hostname" => {
                host.host_name = Some(value.to_string());
            }
            "user" => {
                host.user = Some(value.to_string());
            }
            "port" => {
                host.port = Some(
                    value
                        .parse()
                        .map_err(|_| SshError::InvalidPort(value.to_string()))?,
                );
            }
            "identityfile" => {
                host.identity_file = Some(Self::expand_tilde(value));
            }
            "forwardagent" => {
                host.forward_agent = Some(Self::parse_yes_no(value)?);
            }
            "forwardx11" => {
                host.forward_x11 = Some(Self::parse_yes_no(value)?);
            }
            "stricthostkeychecking" => {
                host.strict_host_key_checking = Some(Self::parse_strict_host_key_checking(value)?);
            }
            "userknownhostsfile" => {
                host.user_known_hosts_file = Some(Self::expand_tilde(value));
            }
            "proxyjump" => {
                host.proxy_jump = Some(value.to_string());
            }
            "proxycommand" => {
                host.proxy_command = Some(value.to_string());
            }
            "serveraliveinterval" => {
                host.server_alive_interval = Some(value.parse().map_err(|_| {
                    SshError::ParseError(format!("Invalid ServerAliveInterval: {}", value))
                })?);
            }
            "serveralivecountmax" => {
                host.server_alive_count_max = Some(value.parse().map_err(|_| {
                    SshError::ParseError(format!("Invalid ServerAliveCountMax: {}", value))
                })?);
            }
            "connecttimeout" | "connectiontimeout" => {
                host.connection_timeout = Some(value.parse().map_err(|_| {
                    SshError::ParseError(format!("Invalid ConnectionTimeout: {}", value))
                })?);
            }
            "compression" => {
                host.compression = Some(Self::parse_yes_no(value)?);
            }
            "tcpkeepalive" => {
                host.tcp_keep_alive = Some(Self::parse_yes_no(value)?);
            }
            "localforward" => {
                if let Some(fwd) = Self::parse_port_forward(value, true)? {
                    host.local_forward.push(fwd);
                }
            }
            "remoteforward" => {
                if let Some(fwd) = Self::parse_port_forward(value, false)? {
                    host.remote_forward.push(fwd);
                }
            }
            "dynamicforward" => {
                if let Some(fwd) = Self::parse_dynamic_forward(value)? {
                    host.dynamic_forward.push(fwd);
                }
            }
            "gatewayports" => {
                host.gateway_ports = Some(Self::parse_yes_no(value)?);
            }
            "escapechar" => {
                host.escape_char = value.chars().next();
            }
            "loglevel" => {
                host.log_level = Some(Self::parse_log_level(value)?);
            }
            "preferredauthentications" => {
                host.preferred_authentications = Some(value.to_string());
            }
            "pubkeyauthentication" => {
                host.pub_key_authentication = Some(Self::parse_yes_no(value)?);
            }
            "passwordauthentication" => {
                host.password_authentication = Some(Self::parse_yes_no(value)?);
            }
            "hostkeyalgorithms" => {
                host.host_key_algorithms = Some(value.to_string());
            }
            "kexalgorithms" => {
                host.kex_algorithms = Some(value.to_string());
            }
            "ciphers" => {
                host.ciphers = Some(value.to_string());
            }
            "macs" => {
                host.macs = Some(value.to_string());
            }
            "addkeystoagent"
            | "identitiesonly"
            | "batchmode"
            | "checkhostip"
            | "clearallforwardings"
            | "enablesshkeysign"
            | "exitonforwardfailure"
            | "fingerprinthash"
            | "gssapiauthentication"
            | "gssapidelegatecredentials"
            | "hashknownhosts"
            | "hostbasedauthentication"
            | "kbdinteractiveauthentication"
            | "nohostauthenticationforlocalhost"
            | "numberofpasswordprompts"
            | "permitlocalcommand"
            | "protocol"
            | "rekeylimit"
            | "revokedhostkeys"
            | "rhostsrsaauthentication"
            | "rsaauthentication"
            | "sendenv"
            | "setenv"
            | "smartcarddevice"
            | "stdinonly"
            | "streamlocalbindmask"
            | "streamlocalbindunlink"
            | "syslogfacility"
            | "visualhostkey"
            | "xauthlocation" => {
                host.extra_options
                    .insert(directive.to_lowercase(), value.to_string());
            }
            _ => {
                warn!("Unknown SSH config directive: {} = {}", directive, value);
                host.extra_options
                    .insert(directive.to_lowercase(), value.to_string());
            }
        }
        Ok(())
    }

    fn parse_yes_no(value: &str) -> SshResult<bool> {
        match value.to_lowercase().as_str() {
            "yes" | "true" | "1" => Ok(true),
            "no" | "false" | "0" => Ok(false),
            _ => Err(SshError::ParseError(format!(
                "Invalid yes/no value: {}",
                value
            ))),
        }
    }

    fn parse_strict_host_key_checking(value: &str) -> SshResult<StrictHostKeyChecking> {
        match value.to_lowercase().as_str() {
            "yes" => Ok(StrictHostKeyChecking::Yes),
            "no" => Ok(StrictHostKeyChecking::No),
            "ask" => Ok(StrictHostKeyChecking::Ask),
            "accept-new" | "acceptnew" => Ok(StrictHostKeyChecking::AcceptNew),
            _ => Err(SshError::ParseError(format!(
                "Invalid StrictHostKeyChecking value: {}",
                value
            ))),
        }
    }

    fn parse_log_level(value: &str) -> SshResult<LogLevel> {
        match value.to_lowercase().as_str() {
            "quiet" => Ok(LogLevel::Quiet),
            "fatal" => Ok(LogLevel::Fatal),
            "error" => Ok(LogLevel::Error),
            "info" => Ok(LogLevel::Info),
            "verbose" => Ok(LogLevel::Verbose),
            "debug" => Ok(LogLevel::Debug),
            "debug1" => Ok(LogLevel::Debug1),
            "debug2" => Ok(LogLevel::Debug2),
            "debug3" => Ok(LogLevel::Debug3),
            _ => Err(SshError::ParseError(format!("Invalid LogLevel: {}", value))),
        }
    }

    fn parse_port_forward(value: &str, is_local: bool) -> SshResult<Option<PortForward>> {
        let parts: Vec<&str> = value.split_whitespace().collect();

        if parts.len() < 2 {
            return Ok(None);
        }

        let _ = is_local;
        let (bind_part, dest_part) = (parts[0], parts[1]);

        let (bind_address, local_port) = Self::parse_bind_address(bind_part)?;
        let (remote_host, remote_port) = Self::parse_remote_address(dest_part)?;

        Ok(Some(PortForward {
            local_port,
            bind_address,
            remote_host,
            remote_port,
        }))
    }

    fn parse_bind_address(value: &str) -> SshResult<(Option<String>, u16)> {
        if value.contains(':') {
            let parts: Vec<&str> = value.splitn(2, ':').collect();
            let port: u16 = parts[1].parse().map_err(|_| {
                SshError::ParseError(format!("Invalid port in bind address: {}", value))
            })?;
            Ok((Some(parts[0].to_string()), port))
        } else {
            let port: u16 = value
                .parse()
                .map_err(|_| SshError::ParseError(format!("Invalid port: {}", value)))?;
            Ok((None, port))
        }
    }

    fn parse_remote_address(value: &str) -> SshResult<(String, u16)> {
        if value.contains(':') {
            let (host, port_str) = value.rsplit_once(':').unwrap_or((value, ""));
            let port: u16 = port_str.parse().map_err(|_| {
                SshError::ParseError(format!("Invalid port in remote address: {}", value))
            })?;
            Ok((host.to_string(), port))
        } else {
            Err(SshError::ParseError(format!(
                "Remote address missing port: {}",
                value
            )))
        }
    }

    fn parse_dynamic_forward(value: &str) -> SshResult<Option<DynamicForward>> {
        let parts: Vec<&str> = value.split_whitespace().collect();

        if parts.is_empty() {
            return Ok(None);
        }

        let (bind_address, local_port) = Self::parse_bind_address(parts[0])?;

        Ok(Some(DynamicForward {
            local_port,
            bind_address,
        }))
    }

    fn expand_tilde(path: &str) -> PathBuf {
        match (path.strip_prefix('~'), dirs::home_dir()) {
            (Some(rest), Some(home)) => home.join(rest.strip_prefix('/').unwrap_or(rest)),
            _ => PathBuf::from(path),
        }
    }

    fn resolve_include_path(path: &str) -> SshResult<PathBuf> {
        let expanded = Self::expand_tilde(path);
        Ok(expanded)
    }

    pub fn default_config_path() -> SshResult<PathBuf> {
        let home = dirs::home_dir().ok_or(SshError::HomeNotFound)?;
        Ok(home.join(".ssh").join("config"))
    }

    pub fn hosts(&self) -> &[SshHost] {
        &self.hosts
    }

    pub fn into_hosts(self) -> Vec<SshHost> {
        self.hosts
    }

    pub fn find_host(&self, pattern: &str) -> Option<&SshHost> {
        self.hosts.iter().find(|h| h.pattern == pattern)
    }

    pub fn find_matching_hosts(&self, hostname: &str) -> Vec<&SshHost> {
        self.hosts.iter().filter(|h| h.matches(hostname)).collect()
    }

    pub fn get_effective_config(&self, hostname: &str) -> Option<SshHost> {
        let matching: Vec<&SshHost> = self.find_matching_hosts(hostname);

        if matching.is_empty() {
            return None;
        }

        let mut effective = SshHost::new(hostname);

        for host in matching {
            if let Some(ref val) = host.host_name {
                effective.host_name = Some(val.clone());
            }
            if let Some(ref val) = host.user {
                effective.user = Some(val.clone());
            }
            if let Some(val) = host.port {
                effective.port = Some(val);
            }
            if let Some(ref val) = host.identity_file {
                effective.identity_file = Some(val.clone());
            }
            if let Some(val) = host.forward_agent {
                effective.forward_agent = Some(val);
            }
            if let Some(val) = host.forward_x11 {
                effective.forward_x11 = Some(val);
            }
            if let Some(ref val) = host.strict_host_key_checking {
                effective.strict_host_key_checking = Some(*val);
            }
            if let Some(ref val) = host.user_known_hosts_file {
                effective.user_known_hosts_file = Some(val.clone());
            }
            if let Some(ref val) = host.proxy_jump {
                effective.proxy_jump = Some(val.clone());
            }
            if let Some(ref val) = host.proxy_command {
                effective.proxy_command = Some(val.clone());
            }
            if let Some(val) = host.server_alive_interval {
                effective.server_alive_interval = Some(val);
            }
            if let Some(val) = host.server_alive_count_max {
                effective.server_alive_count_max = Some(val);
            }
            if let Some(val) = host.connection_timeout {
                effective.connection_timeout = Some(val);
            }
            if let Some(val) = host.compression {
                effective.compression = Some(val);
            }
            if let Some(val) = host.tcp_keep_alive {
                effective.tcp_keep_alive = Some(val);
            }
            for fwd in &host.local_forward {
                effective.local_forward.push(fwd.clone());
            }
            for fwd in &host.remote_forward {
                effective.remote_forward.push(fwd.clone());
            }
            for fwd in &host.dynamic_forward {
                effective.dynamic_forward.push(fwd.clone());
            }
            if let Some(val) = host.gateway_ports {
                effective.gateway_ports = Some(val);
            }
            if let Some(val) = host.escape_char {
                effective.escape_char = Some(val);
            }
            if let Some(ref val) = host.log_level {
                effective.log_level = Some(*val);
            }
            if let Some(ref val) = host.preferred_authentications {
                effective.preferred_authentications = Some(val.clone());
            }
            if let Some(val) = host.pub_key_authentication {
                effective.pub_key_authentication = Some(val);
            }
            if let Some(val) = host.password_authentication {
                effective.password_authentication = Some(val);
            }
            if let Some(ref val) = host.host_key_algorithms {
                effective.host_key_algorithms = Some(val.clone());
            }
            if let Some(ref val) = host.kex_algorithms {
                effective.kex_algorithms = Some(val.clone());
            }
            if let Some(ref val) = host.ciphers {
                effective.ciphers = Some(val.clone());
            }
            if let Some(ref val) = host.macs {
                effective.macs = Some(val.clone());
            }
            for (k, v) in &host.extra_options {
                effective.extra_options.insert(k.clone(), v.clone());
            }
        }

        Some(effective)
    }

    pub fn includes(&self) -> &[PathBuf] {
        &self.includes
    }

    pub fn to_ssh_command(&self, host: &SshHost) -> Vec<String> {
        let mut args = Vec::new();

        if let Some(ref user) = host.user {
            args.push("-l".to_string());
            args.push(user.clone());
        }

        if let Some(port) = host.port {
            args.push("-p".to_string());
            args.push(port.to_string());
        }

        if let Some(ref identity_file) = host.identity_file {
            args.push("-i".to_string());
            args.push(identity_file.to_string_lossy().to_string());
        }

        if host.forward_agent.unwrap_or(false) {
            args.push("-A".to_string());
        }

        if host.forward_x11.unwrap_or(false) {
            args.push("-X".to_string());
        }

        if host.compression.unwrap_or(false) {
            args.push("-C".to_string());
        }

        if let Some(ref proxy_jump) = host.proxy_jump {
            args.push("-J".to_string());
            args.push(proxy_jump.clone());
        }

        if let Some(ref proxy_command) = host.proxy_command {
            args.push("-o".to_string());
            args.push(format!("ProxyCommand={}", proxy_command));
        }

        if let Some(interval) = host.server_alive_interval {
            args.push("-o".to_string());
            args.push(format!("ServerAliveInterval={}", interval));
        }

        if let Some(count) = host.server_alive_count_max {
            args.push("-o".to_string());
            args.push(format!("ServerAliveCountMax={}", count));
        }

        if let Some(timeout) = host.connection_timeout {
            args.push("-o".to_string());
            args.push(format!("ConnectTimeout={}", timeout));
        }

        for fwd in &host.local_forward {
            args.push("-L".to_string());
            let bind = fwd.bind_address.as_deref().unwrap_or("");
            if bind.is_empty() {
                args.push(format!(
                    "{}:{}:{}",
                    fwd.local_port, fwd.remote_host, fwd.remote_port
                ));
            } else {
                args.push(format!(
                    "{}:{}:{}:{}",
                    bind, fwd.local_port, fwd.remote_host, fwd.remote_port
                ));
            }
        }

        for fwd in &host.remote_forward {
            args.push("-R".to_string());
            let bind = fwd.bind_address.as_deref().unwrap_or("");
            if bind.is_empty() {
                args.push(format!(
                    "{}:{}:{}",
                    fwd.local_port, fwd.remote_host, fwd.remote_port
                ));
            } else {
                args.push(format!(
                    "{}:{}:{}:{}",
                    bind, fwd.local_port, fwd.remote_host, fwd.remote_port
                ));
            }
        }

        for fwd in &host.dynamic_forward {
            args.push("-D".to_string());
            if let Some(ref bind) = fwd.bind_address {
                args.push(format!("{}:{}", bind, fwd.local_port));
            } else {
                args.push(fwd.local_port.to_string());
            }
        }

        args.push(host.effective_host().to_string());

        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> &'static str {
        r#"
# This is a comment
Host myserver
    HostName example.com
    User myuser
    Port 2222
    IdentityFile ~/.ssh/id_rsa
    ForwardAgent yes
    ServerAliveInterval 60

Host *.internal.company.com
    User internal
    StrictHostKeyChecking no

Host github.com
    User git
    IdentityFile ~/.ssh/github_key
    IdentitiesOnly yes

Host jump-server
    HostName jump.example.com
    User jumper
    ProxyJump none

Host final-server
    HostName final.example.com
    ProxyJump jump-server
    User admin
"#
    }

    #[test]
    fn test_parse_basic_config() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        assert_eq!(parser.hosts().len(), 5);
    }

    #[test]
    fn test_parse_host_myserver() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        let host = parser.find_host("myserver").unwrap();

        assert_eq!(host.pattern, "myserver");
        assert_eq!(host.host_name, Some("example.com".to_string()));
        assert_eq!(host.user, Some("myuser".to_string()));
        assert_eq!(host.port, Some(2222));
        assert_eq!(host.forward_agent, Some(true));
        assert_eq!(host.server_alive_interval, Some(60));
    }

    #[test]
    fn test_parse_wildcard_pattern() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        let host = parser.find_host("*.internal.company.com").unwrap();

        assert_eq!(host.user, Some("internal".to_string()));
        assert_eq!(
            host.strict_host_key_checking,
            Some(StrictHostKeyChecking::No)
        );
    }

    #[test]
    fn test_find_matching_hosts() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        let matches = parser.find_matching_hosts("api.internal.company.com");

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].user, Some("internal".to_string()));
    }

    #[test]
    fn test_get_effective_config() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        let config = parser.get_effective_config("api.internal.company.com");

        assert!(config.is_some());
        let config = config.unwrap();
        assert_eq!(config.user, Some("internal".to_string()));
    }

    #[test]
    fn test_parse_yes_no() {
        assert!(SshConfigParser::parse_yes_no("yes").unwrap());
        assert!(SshConfigParser::parse_yes_no("Yes").unwrap());
        assert!(SshConfigParser::parse_yes_no("YES").unwrap());
        assert!(!SshConfigParser::parse_yes_no("no").unwrap());
        assert!(!SshConfigParser::parse_yes_no("No").unwrap());
    }

    #[test]
    fn test_parse_strict_host_key_checking() {
        assert_eq!(
            SshConfigParser::parse_strict_host_key_checking("yes").unwrap(),
            StrictHostKeyChecking::Yes
        );
        assert_eq!(
            SshConfigParser::parse_strict_host_key_checking("no").unwrap(),
            StrictHostKeyChecking::No
        );
        assert_eq!(
            SshConfigParser::parse_strict_host_key_checking("ask").unwrap(),
            StrictHostKeyChecking::Ask
        );
        assert_eq!(
            SshConfigParser::parse_strict_host_key_checking("accept-new").unwrap(),
            StrictHostKeyChecking::AcceptNew
        );
    }

    #[test]
    fn test_parse_log_level() {
        assert_eq!(
            SshConfigParser::parse_log_level("quiet").unwrap(),
            LogLevel::Quiet
        );
        assert_eq!(
            SshConfigParser::parse_log_level("debug").unwrap(),
            LogLevel::Debug
        );
        assert_eq!(
            SshConfigParser::parse_log_level("DEBUG3").unwrap(),
            LogLevel::Debug3
        );
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = SshConfigParser::expand_tilde("~/.ssh/id_rsa");
        assert!(expanded.to_string_lossy().contains(".ssh"));
        assert!(expanded.to_string_lossy().contains("id_rsa"));
    }

    #[test]
    fn test_to_ssh_command() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        let host = parser.find_host("myserver").unwrap();
        let args = parser.to_ssh_command(host);

        assert!(args.contains(&"-l".to_string()));
        assert!(args.contains(&"myuser".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"2222".to_string()));
        assert!(args.contains(&"-A".to_string()));
        assert!(args.contains(&"example.com".to_string()));
    }

    #[test]
    fn test_empty_config() {
        let parser = SshConfigParser::from_str("").unwrap();
        assert_eq!(parser.hosts().len(), 0);
    }

    #[test]
    fn test_config_with_only_comments() {
        let parser = SshConfigParser::from_str("# Just a comment\n# Another comment").unwrap();
        assert_eq!(parser.hosts().len(), 0);
    }

    #[test]
    fn test_host_with_proxy_jump() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        let host = parser.find_host("final-server").unwrap();

        assert_eq!(host.proxy_jump, Some("jump-server".to_string()));
        assert_eq!(host.user, Some("admin".to_string()));
    }

    #[test]
    fn test_extra_options() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        let host = parser.find_host("github.com").unwrap();

        assert!(host.extra_options.contains_key("identitiesonly"));
        assert_eq!(
            host.extra_options.get("identitiesonly"),
            Some(&"yes".to_string())
        );
    }

    #[test]
    fn test_connection_string() {
        let parser = SshConfigParser::from_str(create_test_config()).unwrap();
        let host = parser.find_host("myserver").unwrap();

        assert_eq!(host.connection_string(), "myuser@example.com:2222");
    }

    const FULL: &str = r#"
Host full
    HostName full.example.com
    User u
    Port 22
    IdentityFile ~/.ssh/id
    ForwardAgent yes
    ForwardX11 no
    StrictHostKeyChecking accept-new
    UserKnownHostsFile ~/.ssh/kh
    ProxyJump bastion
    ProxyCommand nc %h %p
    ServerAliveInterval 30
    ServerAliveCountMax 4
    ConnectTimeout 9
    Compression true
    TCPKeepAlive 1
    LocalForward 8080 localhost:80
    LocalForward 127.0.0.1:8081 db:5432 extra
    RemoteForward 9090 localhost:90
    DynamicForward 1080
    DynamicForward 127.0.0.1:1081
    GatewayPorts no
    EscapeChar ~
    LogLevel DEBUG3
    PreferredAuthentications publickey
    PubkeyAuthentication yes
    PasswordAuthentication no
    HostKeyAlgorithms ssh-ed25519
    KexAlgorithms curve25519-sha256
    Ciphers aes256-ctr
    MACs hmac-sha2-256
    IdentitiesOnly yes
    Whatever custom
"#;

    #[test]
    fn test_all_directives_are_applied() {
        let parser = SshConfigParser::from_str(FULL).unwrap();
        let h = parser.find_host("full").unwrap();
        assert_eq!(h.host_name.as_deref(), Some("full.example.com"));
        assert_eq!(h.user.as_deref(), Some("u"));
        assert_eq!(h.port, Some(22));
        assert_eq!(h.forward_agent, Some(true));
        assert_eq!(h.forward_x11, Some(false));
        assert_eq!(
            h.strict_host_key_checking,
            Some(StrictHostKeyChecking::AcceptNew)
        );
        assert_eq!(h.proxy_jump.as_deref(), Some("bastion"));
        assert_eq!(h.proxy_command.as_deref(), Some("nc %h %p"));
        assert_eq!(h.server_alive_interval, Some(30));
        assert_eq!(h.server_alive_count_max, Some(4));
        assert_eq!(h.connection_timeout, Some(9));
        assert_eq!(h.compression, Some(true));
        assert_eq!(h.tcp_keep_alive, Some(true));
        assert_eq!(h.local_forward.len(), 2);
        assert_eq!(
            h.local_forward[1].bind_address.as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(h.remote_forward.len(), 1);
        assert_eq!(h.dynamic_forward.len(), 2);
        assert_eq!(h.gateway_ports, Some(false));
        assert_eq!(h.escape_char, Some('~'));
        assert_eq!(h.log_level, Some(LogLevel::Debug3));
        assert_eq!(h.preferred_authentications.as_deref(), Some("publickey"));
        assert_eq!(h.pub_key_authentication, Some(true));
        assert_eq!(h.password_authentication, Some(false));
        assert_eq!(h.host_key_algorithms.as_deref(), Some("ssh-ed25519"));
        assert_eq!(h.kex_algorithms.as_deref(), Some("curve25519-sha256"));
        assert_eq!(h.ciphers.as_deref(), Some("aes256-ctr"));
        assert_eq!(h.macs.as_deref(), Some("hmac-sha2-256"));
        assert_eq!(
            h.extra_options.get("identitiesonly").map(String::as_str),
            Some("yes")
        );
        assert_eq!(
            h.extra_options.get("whatever").map(String::as_str),
            Some("custom")
        );
        assert!(h.identity_file.is_some());
        assert!(h.user_known_hosts_file.is_some());
    }

    #[test]
    fn test_effective_config_merges_every_field_and_command_line() {
        let cfg = format!(
            "Host *\n    User base\n\n{}",
            FULL.replace("Host full", "Host full*")
        );
        let parser = SshConfigParser::from_str(&cfg).unwrap();
        assert!(parser.get_effective_config("nomatch-but-star").is_some());
        let eff = parser.get_effective_config("full").unwrap();
        assert_eq!(eff.user.as_deref(), Some("u"));
        assert_eq!(eff.local_forward.len(), 2);
        assert_eq!(eff.remote_forward.len(), 1);
        assert_eq!(eff.dynamic_forward.len(), 2);
        assert_eq!(eff.log_level, Some(LogLevel::Debug3));
        assert!(eff.extra_options.contains_key("whatever"));

        let args = parser.to_ssh_command(&eff);
        for needle in [
            "-l",
            "-p",
            "-i",
            "-A",
            "-C",
            "-J",
            "-L",
            "-R",
            "-D",
            "ProxyCommand=nc %h %p",
            "ServerAliveInterval=30",
            "ServerAliveCountMax=4",
            "ConnectTimeout=9",
            "8080:localhost:80",
            "127.0.0.1:8081:db:5432",
            "9090:localhost:90",
            "1080",
            "127.0.0.1:1081",
        ] {
            assert!(
                args.iter().any(|a| a == needle),
                "faltou {needle}: {args:?}"
            );
        }
        assert!(!args.iter().any(|a| a == "-X"));
        assert_eq!(args.last().map(String::as_str), Some("full.example.com"));

        let mut x11 = eff.clone();
        x11.forward_x11 = Some(true);
        assert!(parser.to_ssh_command(&x11).iter().any(|a| a == "-X"));

        let mut remote_bind = eff.clone();
        remote_bind.remote_forward[0].bind_address = Some("0.0.0.0".into());
        assert!(parser
            .to_ssh_command(&remote_bind)
            .iter()
            .any(|a| a == "0.0.0.0:9090:localhost:90"));
    }

    #[test]
    fn test_effective_config_none_for_unmatched() {
        let parser = SshConfigParser::from_str("Host only\n    User a\n").unwrap();
        assert!(parser.get_effective_config("other").is_none());
        assert_eq!(parser.hosts().len(), 1);
        assert_eq!(parser.clone().into_hosts().len(), 1);
    }

    #[test]
    fn test_directive_value_errors() {
        for bad in [
            "Port abc",
            "ServerAliveInterval x",
            "ServerAliveCountMax x",
            "ConnectTimeout x",
            "ForwardAgent maybe",
            "StrictHostKeyChecking sometimes",
            "LogLevel loud",
            "LocalForward abc host:1",
            "LocalForward 1:2 host",
            "LocalForward 1 host:abc",
            "LocalForward 1:abc host:1",
            "DynamicForward abc",
        ] {
            let cfg = format!("Host h\n    {}\n", bad);
            assert!(SshConfigParser::from_str(&cfg).is_err(), "{bad}");
        }
    }

    #[test]
    fn test_parse_helpers_directly() {
        assert!(SshConfigParser::parse_line("Key", 1).is_err());
        assert!(SshConfigParser::parse_line("# only", 3).is_err());
        assert_eq!(
            SshConfigParser::parse_line("Key  value # tail", 1).unwrap(),
            ("Key".to_string(), "value".to_string())
        );
        assert!(SshConfigParser::parse_port_forward("single", true)
            .unwrap()
            .is_none());
        assert!(SshConfigParser::parse_dynamic_forward("   ")
            .unwrap()
            .is_none());
        assert_eq!(
            SshConfigParser::parse_remote_address("h:1").unwrap(),
            ("h".to_string(), 1)
        );
        assert!(SshConfigParser::parse_remote_address("host").is_err());
        assert!(SshConfigParser::parse_remote_address("host:x").is_err());
        for level in [
            "quiet", "fatal", "error", "info", "verbose", "debug", "debug1", "debug2", "debug3",
        ] {
            assert!(SshConfigParser::parse_log_level(level).is_ok());
        }
        for v in ["yes", "no", "ask", "accept-new", "acceptnew"] {
            assert!(SshConfigParser::parse_strict_host_key_checking(v).is_ok());
        }
        assert!(!SshConfigParser::parse_yes_no("0").unwrap());
    }

    #[test]
    fn test_default_is_empty() {
        assert!(SshConfigParser::default().hosts().is_empty());
        assert_eq!(
            SshConfigParser::expand_tilde("~"),
            dirs::home_dir().unwrap()
        );
    }

    #[test]
    fn test_include_match_and_directives_outside_host() {
        let cfg = "User ignored\nMatch all\nInclude ~/.ssh/extra\nHost a\n    User x\n";
        let parser = SshConfigParser::from_str(cfg).unwrap();
        assert_eq!(parser.includes().len(), 1);
        assert_eq!(parser.hosts().len(), 1);
        assert_eq!(
            SshConfigParser::resolve_include_path("/abs/path").unwrap(),
            PathBuf::from("/abs/path")
        );
    }

    #[test]
    fn test_from_path_and_default_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let missing = dir.path().join("nope");
        assert!(matches!(
            SshConfigParser::from_path(&missing),
            Err(SshError::ConfigNotFound)
        ));
        let file = dir.path().join("config");
        std::fs::write(&file, "Host z\n    User q\n").unwrap();
        let parser = SshConfigParser::from_path(&file).unwrap();
        assert_eq!(parser.find_host("z").unwrap().user.as_deref(), Some("q"));
        assert!(SshConfigParser::default_config_path()
            .unwrap()
            .ends_with(".ssh/config"));
        // lê o ~/.ssh/config real apenas para exercitar o caminho; o resultado depende do host
        let _ = SshConfigParser::parse_default_config();
        assert!(SshConfigParser::expand_tilde("~/x").is_absolute());
        assert_eq!(
            SshConfigParser::expand_tilde("rel/x"),
            PathBuf::from("rel/x")
        );
    }
}
