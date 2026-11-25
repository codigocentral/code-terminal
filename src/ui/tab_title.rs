/// Gerenciador de títulos para tabs
/// Detecta o processo/shell rodando e atualiza o título dinamicamente
#[derive(Debug, Clone)]
pub struct TabTitle {
    /// Título atual
    title: String,
    /// Shell em uso
    shell: String,
    /// Processo atual (se detectável)
    current_process: Option<String>,
    /// Hostname
    hostname: String,
    /// Diretório atual (se detectável)
    working_dir: Option<String>,
}

impl TabTitle {
    /// Cria um novo gerenciador de título
    pub fn new(shell: &str) -> Self {
        Self {
            title: format!("Terminal — {}", shell),
            shell: shell.to_string(),
            current_process: None,
            hostname: Self::detect_hostname(),
            working_dir: None,
        }
    }

    /// Detecta o hostname do sistema
    fn detect_hostname() -> String {
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "localhost".to_string())
    }

    /// Atualiza o processo atual
    pub fn set_process(&mut self, process: &str) {
        self.current_process = Some(process.to_string());
        self.update_title();
    }

    /// Limpa o processo atual (volta ao shell)
    pub fn clear_process(&mut self) {
        self.current_process = None;
        self.update_title();
    }

    /// Atualiza o diretório de trabalho
    pub fn set_working_dir(&mut self, dir: &str) {
        self.working_dir = Some(dir.to_string());
        self.update_title();
    }

    /// Retorna o título atual
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Retorna o diretório de trabalho atual
    pub fn working_dir(&self) -> Option<&str> {
        self.working_dir.as_deref()
    }

    /// Atualiza o título baseado no estado atual
    fn update_title(&mut self) {
        self.title = if let Some(ref process) = self.current_process {
            match process.as_str() {
                "vim" | "nvim" | "vi" => format!("{} — {}", process, self.shell),
                "htop" | "top" => format!("{} — {}", process, self.shell),
                "ssh" => format!("ssh — {}", self.hostname),
                "git" => format!("git — {}", self.shell),
                _ => {
                    // Se tem diretório, mostra user@host:path
                    if let Some(ref dir) = self.working_dir {
                        format!("{}@{}: {}", whoami::username(), self.hostname, dir)
                    } else {
                        format!("{} — {}", process, self.shell)
                    }
                }
            }
        } else {
            // Apenas shell
            if let Some(ref dir) = self.working_dir {
                format!("{}@{}: {}", whoami::username(), self.hostname, dir)
            } else {
                format!("Terminal — {}", self.shell)
            }
        };
    }

    /// Detecta processo a partir do output do terminal
    /// Procura por padrões comuns de comando
    pub fn detect_from_output(&mut self, output: &str) {
        // Detecta prompts comuns para extrair diretório
        // Ex: "user@host:~/projects$"
        if let Some(captures) = self.parse_bash_prompt(output) {
            if let Some(dir) = captures.get(1) {
                self.set_working_dir(dir.as_str());
            }
        }
    }

    /// Parse de prompt bash/zsh: user@host:path$
    fn parse_bash_prompt<'a>(&self, output: &'a str) -> Option<regex::Captures<'a>> {
        // Regex simples para prompt: user@host:path$ ou user@host:path#
        let re = regex::Regex::new(r":([^$#]+)[$#]").ok()?;
        re.captures(output)
    }
}

impl Default for TabTitle {
    fn default() -> Self {
        Self::new("bash")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tab_title_default() {
        let title = TabTitle::new("bash");
        assert!(title.title().contains("bash"));
    }

    #[test]
    fn test_tab_title_with_process() {
        let mut title = TabTitle::new("bash");
        title.set_process("vim");
        assert!(title.title().contains("vim"));
    }

    #[test]
    fn test_tab_title_ssh() {
        let mut title = TabTitle::new("bash");
        title.set_process("ssh");
        // Deve conter hostname
        assert!(title.title().contains("ssh"));
    }

    #[test]
    fn test_tab_title_with_working_dir() {
        let mut title = TabTitle::new("bash");
        title.set_working_dir("~/projects");
        assert!(title.title().contains("~/projects"));
    }
}
