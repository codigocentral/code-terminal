//! Ajuda da CLI em português (#32).
//!
//! Os textos da ajuda vêm dos comentários `///` das structs do clap, em
//! inglês. Com o idioma em pt-BR, [`localize`] percorre o comando montado e
//! troca cada texto pela tradução desta tabela, além dos títulos ("Uso",
//! "Comandos", "Opções") e da ajuda de `-h`/`-V`. O teste
//! `every_help_text_has_a_translation` (no binário) falha se um texto novo
//! ficar sem tradução.
//!
//! As mensagens de erro do próprio clap (argumento desconhecido etc.) e a
//! saída dos comandos (tabelas, JSON) continuam em inglês: scripts leem isso.

use crate::ui::i18n::Language;
use clap::{Arg, ArgAction, Command};

/// Modelo da ajuda com "Uso:" no lugar de "Usage:".
const TEMPLATE_PT: &str =
    "{before-help}{about-with-newline}\nUso: {usage}\n\n{all-args}{after-help}";

/// Tradução de um texto da ajuda (sem o ponto final), se houver.
pub fn translate(text: &str) -> Option<&'static str> {
    let key = text.trim().trim_end_matches('.');
    Some(match key {
        // Comando principal e opções globais.
        "CLI tool for controlling Code Terminal remotely" => {
            "Controla o Code Terminal pela linha de comando"
        }
        "Path to socket file (Unix) or TCP address" => {
            "Caminho do socket (Unix) ou endereço TCP"
        }
        "Configuration file path" => "Arquivo de configuração",
        "Output format" => "Formato da saída",
        "Verbose output" => "Saída detalhada",
        "Quiet mode (suppress output)" => "Modo silencioso (sem saída)",
        // Abas.
        "List all tabs" => "Lista as abas",
        "Show all details" => "Mostra todos os detalhes",
        "Create a new tab" => "Cria uma aba",
        "Tab title" => "Título da aba",
        "Working directory" => "Diretório de trabalho",
        "Shell command to execute" => "Comando a executar",
        "Command and arguments to execute" => "Comando e argumentos a executar",
        "Close a tab" => "Fecha uma aba",
        "Tab index (0-based)" => "Índice da aba (começa em 0)",
        "Rename a tab (the title stays fixed); without a title, back to automatic" => {
            "Renomeia uma aba (o título fica fixo); sem título, volta ao automático"
        }
        "New title" => "Título novo",
        "Switch to a specific tab" => "Vai para uma aba",
        "Send keys to the active tab (supports special keys like <C-c>, <Enter>, <Esc>)" => {
            "Envia teclas para a aba ativa (aceita teclas especiais como <C-c>, <Enter>, <Esc>)"
        }
        "Keys to send (use special notation like <C-c>, <Enter>, <Esc>)" => {
            "Teclas a enviar (notação especial: <C-c>, <Enter>, <Esc>)"
        }
        "Send text/input to the active tab" => "Envia texto para a aba ativa",
        "Text to input" => "Texto a enviar",
        "Get or set terminal size" => "Mostra ou muda o tamanho do terminal",
        "Set columns" => "Colunas",
        "Set rows" => "Linhas",
        "Show terminal info" => "Mostra informações do terminal",
        // Avisos e agentes.
        "Send system notification" => "Envia uma notificação",
        "Notification title" => "Título da notificação",
        "Notification message" => "Mensagem da notificação",
        "Notify Code Terminal from an AI agent hook (reads the hook JSON on stdin)" => {
            "Avisa o Code Terminal a partir do hook de um agente de IA (lê o JSON do hook na entrada)"
        }
        "Agent name shown in the notification" => "Nome do agente na notificação",
        "Print the Claude Code settings.json snippet that wires its hooks to Code Terminal" => {
            "Mostra o trecho do settings.json do Claude Code que liga os hooks ao Code Terminal"
        }
        // Navegador e prévia.
        "Open a URL (or local file) in the Code Terminal browser window" => {
            "Abre uma URL (ou arquivo local) na janela de navegador do Code Terminal"
        }
        "Address or path (empty opens a blank page)" => {
            "Endereço ou caminho (vazio abre uma página em branco)"
        }
        "Control the browser window" => "Controla a janela do navegador",
        "Previous page" => "Página anterior",
        "Next page" => "Próxima página",
        "Reload the page" => "Recarrega a página",
        "Current URL and title" => "URL e título atuais",
        "Visible text of the page" => "Texto visível da página",
        "Run JavaScript and print the result (JSON)" => {
            "Roda JavaScript e mostra o resultado (JSON)"
        }
        "Close the browser window" => "Fecha a janela do navegador",
        "Preview a file in the terminal: images inline, markdown and JSON formatted" => {
            "Mostra um arquivo no terminal: imagens na linha, markdown e JSON formatados"
        }
        "File to show" => "Arquivo a mostrar",
        "Image width in cells" => "Largura da imagem em células",
        // Painéis.
        "Print the visible text of a pane (the current one by default)" => {
            "Mostra o texto visível de um painel (o atual, por padrão)"
        }
        "Only the last N lines" => "Só as últimas N linhas",
        "Pane id (see list-panes); defaults to $CODE_TERMINAL_PANE_ID" => {
            "Id do painel (veja list-panes); padrão: $CODE_TERMINAL_PANE_ID"
        }
        "List all panes" => "Lista os painéis",
        "Split current pane" => "Divide o painel atual",
        "Split direction" => "Direção da divisão",
        // Configuração e utilidades.
        "Configuration management" => "Configuração",
        "Show current configuration" => "Mostra a configuração atual",
        "Set a configuration value" => "Muda um valor da configuração",
        "Get a configuration value" => "Mostra um valor da configuração",
        "Reset configuration to defaults" => "Volta a configuração ao padrão",
        "Ping the server" => "Testa a conexão com o servidor",
        "Show version information" => "Mostra a versão",
        "Generate shell completion scripts" => "Gera o autocompletar do shell",
        "Shell to generate completions for (bash, zsh, fish, power-shell, elvish)" => {
            "Shell do autocompletar (bash, zsh, fish, power-shell, elvish)"
        }
        // SSH.
        "List SSH hosts from ~/.ssh/config" => "Lista os hosts SSH do ~/.ssh/config",
        "Filter hosts by query" => "Filtra os hosts pelo texto",
        "Connect to an SSH host" => "Conecta a um host SSH",
        "Host to connect (can be user@host or host)" => {
            "Host a conectar (usuário@host ou host)"
        }
        "Port number (default: 22)" => "Porta (padrão: 22)",
        "User name (if not specified in host)" => "Usuário (se não estiver no host)",
        "Identity file path" => "Arquivo da chave",
        "List SSH sessions" => "Lista as sessões SSH",
        "Create a new SSH session" => "Cria uma sessão SSH",
        "Session name" => "Nome da sessão",
        "Host to connect" => "Host a conectar",
        "User name" => "Usuário",
        "Remove an SSH session" => "Remove uma sessão SSH",
        "Session ID" => "Id da sessão",
        // Multiplexador.
        "List multiplexer sessions" => "Lista as sessões do multiplexador",
        "Create a new multiplexer session" => "Cria uma sessão do multiplexador",
        "Terminal columns (`-w`, como em `size`: `-c` é o `--config` global)" => {
            "Colunas do terminal (`-w`, como em `size`: `-c` é o `--config` global)"
        }
        "Terminal rows" => "Linhas do terminal",
        "Attach to a multiplexer session (Ctrl+B d detaches; the shell keeps running)" => {
            "Entra numa sessão do multiplexador (Ctrl+B d sai; o shell continua rodando)"
        }
        "Session ID (or prefix) or name" => "Id (ou começo do id) ou nome da sessão",
        "Read-only mode" => "Só leitura",
        "Create the session (and start the daemon) if it does not exist" => {
            "Cria a sessão (e inicia o daemon) se ela não existir"
        }
        "Run the multiplexer daemon in the foreground (started on demand by mux-create and mux-attach --create)" => {
            "Roda o daemon do multiplexador em primeiro plano (o mux-create e o mux-attach --create o iniciam quando precisa)"
        }
        "Keep running with no sessions (systemd service: persistent tabs come back even before a window opens)" => {
            "Continua rodando sem sessões (serviço do systemd: as abas persistentes voltam antes de abrir uma janela)"
        }
        "Detach from current multiplexer session" => "Sai da sessão atual do multiplexador",
        "Rename a multiplexer session" => "Renomeia uma sessão do multiplexador",
        "New name" => "Nome novo",
        "Destroy a multiplexer session" => "Encerra uma sessão do multiplexador",
        "Save a session to disk" => "Salva uma sessão no disco",
        "List saved sessions" => "Lista as sessões salvas",
        "Restore a saved session" => "Restaura uma sessão salva",
        "Show multiplexer statistics" => "Mostra as estatísticas do multiplexador",
        "Manage session groups" => "Grupos de sessões",
        "Action (list, create, delete)" => "Ação (list, create, delete)",
        "Additional arguments" => "Argumentos adicionais",
        "Cleanup old sessions" => "Apaga sessões antigas",
        "Days to keep" => "Dias a manter",
        "Interactive session list" => "Lista interativa de sessões",
        "Sort by (name, created, activity, clients) (`-o`: `-s` é o `--socket` global)" => {
            "Ordem (name, created, activity, clients) (`-o`: `-s` é o `--socket` global)"
        }
        "Switch to another session" => "Vai para outra sessão",
        "Session name or index" => "Nome ou índice da sessão",
        "Switch to next session" => "Vai para a próxima sessão",
        "Switch to previous session" => "Vai para a sessão anterior",
        "Switch to last used session" => "Volta para a última sessão usada",
        // Do clap.
        "Print this message or the help of the given subcommand(s)" => {
            "Mostra esta ajuda ou a do subcomando"
        }
        _ => return None,
    })
}

/// Texto longo (com exemplos): traduz o primeiro parágrafo e o título
/// "Examples:"; os exemplos ficam como estão.
fn translate_long(text: &str) -> String {
    let (first, rest) = text.split_once("\n\n").unwrap_or((text, ""));
    let first = translate(first).map_or_else(|| first.to_string(), str::to_string);
    if rest.is_empty() {
        first
    } else {
        format!("{first}\n\n{}", rest.replace("Examples:", "Exemplos:"))
    }
}

/// A ajuda do comando no idioma pedido (inglês fica como está).
pub fn localize(cmd: Command, language: Language) -> Command {
    match language {
        Language::English => cmd,
        Language::PortugueseBr => localize_pt(cmd, true),
    }
}

fn localize_pt(mut cmd: Command, root: bool) -> Command {
    if let Some(pt) = cmd.get_about().and_then(|a| translate(&a.to_string())) {
        cmd = cmd.about(pt);
    }
    if let Some(long) = cmd.get_long_about().map(|a| a.to_string()) {
        cmd = cmd.long_about(translate_long(&long));
    }
    cmd = cmd.mut_args(|arg| {
        let heading = if arg.is_positional() {
            "Argumentos"
        } else {
            "Opções"
        };
        let mut arg = arg.help_heading(heading);
        if let Some(pt) = arg.get_help().and_then(|h| translate(&h.to_string())) {
            arg = arg.help(pt);
        }
        if let Some(long) = arg.get_long_help().map(|h| h.to_string()) {
            arg = arg.long_help(translate_long(&long));
        }
        arg
    });
    cmd = cmd
        .help_template(TEMPLATE_PT)
        .subcommand_help_heading("Comandos")
        .disable_help_flag(true)
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::Help)
                .help("Mostra a ajuda")
                .help_heading("Opções"),
        );
    if root && cmd.get_version().is_some() {
        cmd = cmd.disable_version_flag(true).arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::Version)
                .help("Mostra a versão")
                .help_heading("Opções"),
        );
    }
    cmd.mut_subcommands(|sub| localize_pt(sub, false))
}

/// Textos da ajuda (sobre e argumentos) sem tradução, para o teste.
pub fn untranslated(cmd: &Command) -> Vec<String> {
    let mut missing = Vec::new();
    collect_untranslated(cmd, &mut missing);
    missing.sort();
    missing.dedup();
    missing
}

fn collect_untranslated(cmd: &Command, missing: &mut Vec<String>) {
    let mut check = |text: Option<String>| {
        if let Some(text) = text.filter(|t| !t.trim().is_empty()) {
            if translate(&text).is_none() {
                missing.push(text);
            }
        }
    };
    check(cmd.get_about().map(|a| a.to_string()));
    for arg in cmd.get_arguments() {
        if matches!(arg.get_id().as_str(), "help" | "version") {
            continue;
        }
        check(arg.get_help().map(|h| h.to_string()));
    }
    for sub in cmd.get_subcommands() {
        collect_untranslated(sub, missing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Command {
        Command::new("cli")
            .version("1.0")
            .about("CLI tool for controlling Code Terminal remotely")
            .arg(
                Arg::new("socket")
                    .long("socket")
                    .help("Path to socket file (Unix) or TCP address"),
            )
            .subcommand(
                Command::new("close")
                    .about("Close a tab.")
                    .long_about("Close a tab.\n\nExamples:\n    cli close 0")
                    .arg(Arg::new("index").help("Tab index (0-based)")),
            )
    }

    #[test]
    fn portuguese_help_translates_texts_and_headings() {
        let mut cmd = localize(sample(), Language::PortugueseBr);
        let help = cmd.render_help().to_string();
        assert!(help.contains("Controla o Code Terminal"), "{help}");
        assert!(help.contains("Uso:"), "{help}");
        assert!(help.contains("Comandos:"), "{help}");
        assert!(help.contains("Opções:"), "{help}");
        assert!(help.contains("Mostra a ajuda"), "{help}");
        assert!(help.contains("Mostra a versão"), "{help}");
        assert!(help.contains("Fecha uma aba"), "{help}");
        assert!(!help.contains("Usage:"), "{help}");
        let mut close = cmd.find_subcommand_mut("close").unwrap().clone();
        let long = close.render_long_help().to_string();
        assert!(long.contains("Exemplos:"), "{long}");
        assert!(long.contains("Índice da aba"), "{long}");
        assert!(long.contains("Argumentos:"), "{long}");
        // -h e -V continuam funcionando.
        let err = localize(sample(), Language::PortugueseBr)
            .try_get_matches_from(["cli", "-V"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
        let err = localize(sample(), Language::PortugueseBr)
            .try_get_matches_from(["cli", "close", "--help"])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn english_help_is_untouched() {
        let mut cmd = localize(sample(), Language::English);
        let help = cmd.render_help().to_string();
        assert!(help.contains("Usage:"));
        assert!(help.contains("CLI tool for controlling"));
    }

    #[test]
    fn untranslated_lists_missing_texts() {
        let cmd = sample().subcommand(Command::new("x").about("Something new"));
        assert_eq!(untranslated(&cmd), vec!["Something new".to_string()]);
    }
}
