//! Multiplexer Commands - Comandos internos do modo multiplexer
//!
//! Este módulo implementa comandos similares aos do tmux,
//! acessíveis via keybindings ou CLI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::MultiplexerResult;

/// Tipo de comando
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MuxCommand {
    // Comandos de sessão
    /// Cria uma nova sessão
    NewSession { name: Option<String> },
    /// Anexa a uma sessão
    AttachSession { target: String },
    /// Desanexa da sessão atual
    DetachClient,
    /// Mata uma sessão
    KillSession { target: Option<String> },
    /// Renomeia a sessão atual
    RenameSession { name: String },
    /// Lista sessões
    ListSessions,
    /// Seleciona uma sessão
    SelectSession { target: String },
    /// Próxima sessão
    NextSession,
    /// Sessão anterior
    PreviousSession,
    /// Última sessão usada
    LastSession,

    // Comandos de janela (window)
    /// Cria uma nova janela
    NewWindow {
        name: Option<String>,
        command: Option<String>,
    },
    /// Fecha a janela atual
    KillWindow,
    /// Seleciona uma janela
    SelectWindow { target: WindowTarget },
    /// Próxima janela
    NextWindow,
    /// Janela anterior
    PreviousWindow,
    /// Última janela usada
    LastWindow,
    /// Renomeia a janela atual
    RenameWindow { name: String },
    /// Lista janelas
    ListWindows,
    /// Troca janelas de posição
    SwapWindow { target: WindowTarget },

    // Comandos de painel (pane)
    /// Divide o painel atual
    SplitPane {
        direction: SplitDirection,
        size: Option<String>,
    },
    /// Fecha o painel atual
    KillPane,
    /// Seleciona um painel
    SelectPane { direction: PaneDirection },
    /// Troca painéis de posição
    SwapPane { direction: PaneDirection },
    /// Redimensiona painel
    ResizePane {
        direction: ResizeDirection,
        amount: u16,
    },
    /// Divide painel em layout específico
    SelectLayout { layout: Layout },
    /// Lista painéis
    ListPanes,
    /// Envia comando para painel
    SendKeys {
        keys: String,
        target: Option<String>,
    },

    // Comandos de buffer
    /// Copia seleção
    CopySelection,
    /// Cola buffer
    PasteBuffer,
    /// Mostra buffer
    ShowBuffer,
    /// Limpa buffer
    ClearBuffer,

    // Comandos de configuração
    /// Define opção
    SetOption {
        key: String,
        value: String,
        global: bool,
    },
    /// Mostra opções
    ShowOptions,
    /// Recarrega configuração
    ReloadConfig,

    // Comandos de status
    /// Mostra mensagem
    DisplayMessage { text: String, duration: Option<u64> },
    /// Atualiza status
    UpdateStatus,
    /// Relógio
    ClockMode,

    // Comandos diversos
    /// Executa comando shell
    RunShell { command: String },
    /// Prompt de comando
    CommandPrompt,
    /// Menu de confirmação
    ConfirmBefore { prompt: String, command: String },
    /// Bloqueia cliente
    LockClient,
    /// Suspende cliente
    SuspendClient,
    /// Sair do multiplexer
    Exit,
    /// Sem operação
    Nop,
}

/// Alvo de janela
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowTarget {
    /// Índice numérico
    Index(usize),
    /// Nome da janela
    Name(String),
    /// Padrão de busca
    Pattern(String),
    /// Janela anterior (usada com swap)
    Last,
}

/// Direção de divisão
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    /// Horizontal (divide em cima/baixo)
    Horizontal,
    /// Vertical (divide em esquerda/direita)
    Vertical,
}

impl std::fmt::Display for SplitDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SplitDirection::Horizontal => write!(f, "horizontal"),
            SplitDirection::Vertical => write!(f, "vertical"),
        }
    }
}

/// Direção de painel
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaneDirection {
    /// Acima
    Up,
    /// Abaixo
    Down,
    /// Esquerda
    Left,
    /// Direita
    Right,
    /// Próximo
    Next,
    /// Anterior
    Previous,
    /// Último ativo
    Last,
    /// Por índice
    Index(usize),
}

impl PaneDirection {
    /// Retorna a direção oposta
    pub fn opposite(&self) -> Option<Self> {
        match self {
            PaneDirection::Up => Some(PaneDirection::Down),
            PaneDirection::Down => Some(PaneDirection::Up),
            PaneDirection::Left => Some(PaneDirection::Right),
            PaneDirection::Right => Some(PaneDirection::Left),
            _ => None,
        }
    }
}

impl std::fmt::Display for PaneDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaneDirection::Up => write!(f, "up"),
            PaneDirection::Down => write!(f, "down"),
            PaneDirection::Left => write!(f, "left"),
            PaneDirection::Right => write!(f, "right"),
            PaneDirection::Next => write!(f, "next"),
            PaneDirection::Previous => write!(f, "previous"),
            PaneDirection::Last => write!(f, "last"),
            PaneDirection::Index(i) => write!(f, "{}", i),
        }
    }
}

/// Direção de redimensionamento
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResizeDirection {
    /// Para cima (diminui altura inferior)
    Up,
    /// Para baixo (aumenta altura inferior)
    Down,
    /// Esquerda (diminui largura direita)
    Left,
    /// Direita (aumenta largura direita)
    Right,
    /// Aumenta
    Increase,
    /// Diminui
    Decrease,
}

/// Layout de painéis
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    /// Layout uniforme
    EvenHorizontal,
    EvenVertical,
    /// Layout principal (um grande, outros menores)
    MainHorizontal,
    MainVertical,
    /// Layout em tile
    Tiled,
    /// Layout personalizado
    Custom(String),
}

impl std::fmt::Display for Layout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Layout::EvenHorizontal => write!(f, "even-horizontal"),
            Layout::EvenVertical => write!(f, "even-vertical"),
            Layout::MainHorizontal => write!(f, "main-horizontal"),
            Layout::MainVertical => write!(f, "main-vertical"),
            Layout::Tiled => write!(f, "tiled"),
            Layout::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// Parser de comandos
pub struct CommandParser;

impl CommandParser {
    /// Parseia uma string de comando
    pub fn parse(input: &str) -> MultiplexerResult<MuxCommand> {
        let parts: Vec<&str> = input.split_whitespace().collect();

        if parts.is_empty() {
            return Ok(MuxCommand::Nop);
        }

        match parts[0].to_lowercase().as_str() {
            // Sessão
            "new-session" | "new" => Self::parse_new_session(&parts[1..]),
            "attach-session" | "attach" | "a" => Self::parse_attach(&parts[1..]),
            "detach-client" | "detach" | "d" => Ok(MuxCommand::DetachClient),
            "kill-session" | "kill-server" => Self::parse_kill_session(&parts[1..]),
            "rename-session" => Self::parse_rename_session(&parts[1..]),
            "list-sessions" | "ls" => Ok(MuxCommand::ListSessions),
            "switch-client" | "switchc" => Self::parse_switch_session(&parts[1..]),
            "next-session" | "ns" => Ok(MuxCommand::NextSession),
            "previous-session" | "ps" => Ok(MuxCommand::PreviousSession),
            "last-session" => Ok(MuxCommand::LastSession),

            // Janela
            "new-window" | "neww" => Self::parse_new_window(&parts[1..]),
            "kill-window" | "killw" => Ok(MuxCommand::KillWindow),
            "select-window" | "selectw" => Self::parse_select_window(&parts[1..]),
            "next-window" | "next" | "n" => Ok(MuxCommand::NextWindow),
            "previous-window" | "prev" | "p" => Ok(MuxCommand::PreviousWindow),
            "last-window" | "lastw" | "l" => Ok(MuxCommand::LastWindow),
            "rename-window" | "renamew" => Self::parse_rename_window(&parts[1..]),
            "list-windows" | "lsw" => Ok(MuxCommand::ListWindows),
            "swap-window" | "swapw" => Self::parse_swap_window(&parts[1..]),

            // Painel
            "split-window" | "splitw" => Self::parse_split(&parts[1..]),
            "kill-pane" | "killp" => Ok(MuxCommand::KillPane),
            "select-pane" | "selectp" => Self::parse_select_pane(&parts[1..]),
            "swap-pane" | "swapp" => Self::parse_swap_pane(&parts[1..]),
            "resize-pane" | "resizep" => Self::parse_resize(&parts[1..]),
            "select-layout" => Self::parse_layout(&parts[1..]),
            "list-panes" | "lsp" => Ok(MuxCommand::ListPanes),
            "send-keys" | "send" => Self::parse_send_keys(&parts[1..]),

            // Buffer
            "copy-mode" => Ok(MuxCommand::CopySelection),
            "paste-buffer" | "pasteb" => Ok(MuxCommand::PasteBuffer),
            "show-buffer" => Ok(MuxCommand::ShowBuffer),
            "clear-history" => Ok(MuxCommand::ClearBuffer),

            // Config
            "set-option" | "set" => Self::parse_set_option(&parts[1..]),
            "show-options" | "show" => Ok(MuxCommand::ShowOptions),
            "source-file" | "source" => Ok(MuxCommand::ReloadConfig),

            // Status
            "display-message" | "display" => Self::parse_display(&parts[1..]),
            "clock-mode" | "clock" => Ok(MuxCommand::ClockMode),

            // Misc
            "run-shell" | "run" => Self::parse_run_shell(&parts[1..]),
            "command-prompt" => Ok(MuxCommand::CommandPrompt),
            "confirm-before" | "confirm" => Self::parse_confirm(&parts[1..]),
            "lock-client" | "lockc" => Ok(MuxCommand::LockClient),
            "suspend-client" | "suspendc" => Ok(MuxCommand::SuspendClient),
            "exit" | "quit" | "q" => Ok(MuxCommand::Exit),

            _ => Err(super::MultiplexerError::ServerError(format!(
                "Unknown command: {}",
                parts[0]
            ))),
        }
    }

    fn parse_new_session(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let mut name = None;
        let mut i = 0;

        while i < args.len() {
            match args[i] {
                "-s" | "--session-name" => {
                    if i + 1 < args.len() {
                        name = Some(args[i + 1].to_string());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }

        Ok(MuxCommand::NewSession { name })
    }

    /// Alvo de um comando: o valor de `-t` (sintaxe do tmux) ou, sem ele,
    /// o primeiro argumento que não é flag.
    fn target_arg<'a>(args: &[&'a str]) -> Option<&'a str> {
        match args
            .iter()
            .position(|&arg| arg == "-t" || arg == "--target")
        {
            Some(pos) => args.get(pos + 1).copied(),
            None => args.iter().copied().find(|arg| !arg.starts_with('-')),
        }
    }

    fn parse_attach(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let target = Self::target_arg(args).unwrap_or("0").to_string();

        Ok(MuxCommand::AttachSession { target })
    }

    fn parse_kill_session(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let target = Self::target_arg(args).map(|s| s.to_string());
        Ok(MuxCommand::KillSession { target })
    }

    fn parse_rename_session(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let name = args.join(" ");
        Ok(MuxCommand::RenameSession { name })
    }

    fn parse_switch_session(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let target = args
            .iter()
            .find(|&&arg| !arg.starts_with('-'))
            .map(|s| s.to_string())
            .unwrap_or_else(|| "0".to_string());

        Ok(MuxCommand::SelectSession { target })
    }

    fn parse_new_window(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let mut name = None;
        let mut command = None;
        let mut i = 0;

        while i < args.len() {
            match args[i] {
                "-n" | "--name" => {
                    if i + 1 < args.len() {
                        name = Some(args[i + 1].to_string());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                _ => {
                    command = Some(args[i..].join(" "));
                    break;
                }
            }
        }

        Ok(MuxCommand::NewWindow { name, command })
    }

    fn parse_select_window(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let target = if let Some(arg) = Self::target_arg(args) {
            if let Ok(idx) = arg.parse::<usize>() {
                WindowTarget::Index(idx)
            } else {
                WindowTarget::Name(arg.to_string())
            }
        } else {
            WindowTarget::Last
        };

        Ok(MuxCommand::SelectWindow { target })
    }

    fn parse_rename_window(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let name = args.join(" ");
        Ok(MuxCommand::RenameWindow { name })
    }

    fn parse_swap_window(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let target = if let Some(arg) = args.first() {
            if let Ok(idx) = arg.parse::<usize>() {
                WindowTarget::Index(idx)
            } else if *arg == "-t" && args.len() > 1 {
                if let Ok(idx) = args[1].parse::<usize>() {
                    WindowTarget::Index(idx)
                } else {
                    WindowTarget::Name(args[1].to_string())
                }
            } else {
                WindowTarget::Last
            }
        } else {
            WindowTarget::Last
        };

        Ok(MuxCommand::SwapWindow { target })
    }

    fn parse_split(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let mut direction = SplitDirection::Vertical;
        let mut size = None;
        let mut i = 0;

        while i < args.len() {
            match args[i] {
                "-h" | "--horizontal" => {
                    direction = SplitDirection::Horizontal;
                    i += 1;
                }
                "-v" | "--vertical" => {
                    direction = SplitDirection::Vertical;
                    i += 1;
                }
                "-l" | "--size" | "-p" | "--percentage" => {
                    if i + 1 < args.len() {
                        size = Some(args[i + 1].to_string());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }

        Ok(MuxCommand::SplitPane { direction, size })
    }

    fn parse_select_pane(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let direction = if let Some(arg) = args.first() {
            match *arg {
                "-U" | "--up" => PaneDirection::Up,
                "-D" | "--down" => PaneDirection::Down,
                "-L" | "--left" => PaneDirection::Left,
                "-R" | "--right" => PaneDirection::Right,
                "-l" | "--last" => PaneDirection::Last,
                s => {
                    if let Ok(idx) = s.parse::<usize>() {
                        PaneDirection::Index(idx)
                    } else {
                        PaneDirection::Next
                    }
                }
            }
        } else {
            PaneDirection::Next
        };

        Ok(MuxCommand::SelectPane { direction })
    }

    fn parse_swap_pane(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let direction = if let Some(arg) = args.first() {
            match *arg {
                "-U" => PaneDirection::Up,
                "-D" => PaneDirection::Down,
                "-L" => PaneDirection::Left,
                "-R" => PaneDirection::Right,
                _ => PaneDirection::Next,
            }
        } else {
            PaneDirection::Next
        };

        Ok(MuxCommand::SwapPane { direction })
    }

    fn parse_resize(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let mut direction = ResizeDirection::Right;
        let mut amount = 5;
        let mut i = 0;

        while i < args.len() {
            match args[i] {
                "-U" | "--up" => direction = ResizeDirection::Up,
                "-D" | "--down" => direction = ResizeDirection::Down,
                "-L" | "--left" => direction = ResizeDirection::Left,
                "-R" | "--right" => direction = ResizeDirection::Right,
                "-x" | "--width" => direction = ResizeDirection::Increase,
                "-y" | "--height" => direction = ResizeDirection::Increase,
                s => {
                    if let Ok(n) = s.parse::<u16>() {
                        amount = n;
                    }
                }
            }
            i += 1;
        }

        Ok(MuxCommand::ResizePane { direction, amount })
    }

    fn parse_layout(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let layout = if let Some(arg) = args.first() {
            match *arg {
                "even-horizontal" => Layout::EvenHorizontal,
                "even-vertical" => Layout::EvenVertical,
                "main-horizontal" => Layout::MainHorizontal,
                "main-vertical" => Layout::MainVertical,
                "tiled" => Layout::Tiled,
                s => Layout::Custom(s.to_string()),
            }
        } else {
            Layout::Tiled
        };

        Ok(MuxCommand::SelectLayout { layout })
    }

    fn parse_send_keys(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let mut target = None;
        let mut keys = Vec::new();
        let mut i = 0;

        while i < args.len() {
            match args[i] {
                "-t" | "--target" => {
                    if i + 1 < args.len() {
                        target = Some(args[i + 1].to_string());
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                k => {
                    keys.push(k.to_string());
                    i += 1;
                }
            }
        }

        Ok(MuxCommand::SendKeys {
            keys: keys.join(" "),
            target,
        })
    }

    fn parse_set_option(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let mut global = false;
        let mut key = String::new();
        let mut value = String::new();
        let mut i = 0;

        while i < args.len() {
            match args[i] {
                "-g" | "--global" => {
                    global = true;
                    i += 1;
                }
                "-s" | "--session" => {
                    i += 1;
                }
                k if key.is_empty() => {
                    key = k.to_string();
                    i += 1;
                }
                v if value.is_empty() => {
                    value = v.to_string();
                    i += 1;
                }
                _ => i += 1,
            }
        }

        Ok(MuxCommand::SetOption { key, value, global })
    }

    fn parse_display(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let text = args.join(" ");
        Ok(MuxCommand::DisplayMessage {
            text,
            duration: Some(2000),
        })
    }

    fn parse_run_shell(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let command = args.join(" ");
        Ok(MuxCommand::RunShell { command })
    }

    fn parse_confirm(args: &[&str]) -> MultiplexerResult<MuxCommand> {
        let prompt = args
            .first()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "Confirm?".to_string());

        Ok(MuxCommand::ConfirmBefore {
            prompt,
            command: "nop".to_string(),
        })
    }
}

/// Mapa de keybindings padrão
pub fn default_keybindings() -> HashMap<String, MuxCommand> {
    let mut bindings = HashMap::new();

    let prefix = "C-b"; // Ctrl+B (igual ao tmux)

    // Janelas
    bindings.insert(
        format!("{} c", prefix),
        MuxCommand::NewWindow {
            name: None,
            command: None,
        },
    );
    bindings.insert(format!("{} n", prefix), MuxCommand::NextWindow);
    bindings.insert(format!("{} p", prefix), MuxCommand::PreviousWindow);
    bindings.insert(format!("{} l", prefix), MuxCommand::LastWindow);
    bindings.insert(format!("{} w", prefix), MuxCommand::ListWindows);
    bindings.insert(format!("{} ,", prefix), MuxCommand::CommandPrompt); // Rename
    bindings.insert(format!("{} &", prefix), MuxCommand::KillWindow);

    // Painéis
    bindings.insert(
        format!("{} %", prefix),
        MuxCommand::SplitPane {
            direction: SplitDirection::Vertical,
            size: None,
        },
    );
    bindings.insert(
        format!("{} \"", prefix),
        MuxCommand::SplitPane {
            direction: SplitDirection::Horizontal,
            size: None,
        },
    );
    bindings.insert(
        format!("{} z", prefix),
        MuxCommand::SelectPane {
            direction: PaneDirection::Next,
        },
    );
    bindings.insert(format!("{} x", prefix), MuxCommand::KillPane);
    bindings.insert(
        format!("{} {{", prefix),
        MuxCommand::SwapPane {
            direction: PaneDirection::Previous,
        },
    );
    bindings.insert(
        format!("{} }}", prefix),
        MuxCommand::SwapPane {
            direction: PaneDirection::Next,
        },
    );
    bindings.insert(
        format!("{} ↑", prefix),
        MuxCommand::SelectPane {
            direction: PaneDirection::Up,
        },
    );
    bindings.insert(
        format!("{} ↓", prefix),
        MuxCommand::SelectPane {
            direction: PaneDirection::Down,
        },
    );
    bindings.insert(
        format!("{} ←", prefix),
        MuxCommand::SelectPane {
            direction: PaneDirection::Left,
        },
    );
    bindings.insert(
        format!("{} →", prefix),
        MuxCommand::SelectPane {
            direction: PaneDirection::Right,
        },
    );

    // Sessões
    bindings.insert(format!("{} d", prefix), MuxCommand::DetachClient);
    bindings.insert(format!("{} s", prefix), MuxCommand::ListSessions);
    bindings.insert(format!("{} $", prefix), MuxCommand::CommandPrompt); // Rename session
    bindings.insert(format!("{} )", prefix), MuxCommand::NextSession);
    bindings.insert(format!("{} (", prefix), MuxCommand::PreviousSession);

    // Modos
    bindings.insert(format!("{} [", prefix), MuxCommand::CopySelection);
    bindings.insert(format!("{} ]", prefix), MuxCommand::PasteBuffer);
    bindings.insert(format!("{} t", prefix), MuxCommand::ClockMode);

    // Misc
    bindings.insert(format!("{} :", prefix), MuxCommand::CommandPrompt);
    bindings.insert(
        format!("{} ?", prefix),
        MuxCommand::DisplayMessage {
            text: "Help: Prefix + key".to_string(),
            duration: Some(3000),
        },
    );
    bindings.insert(format!("{} r", prefix), MuxCommand::ReloadConfig);

    bindings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_new_session() {
        let cmd = CommandParser::parse("new-session -s mysession").unwrap();
        assert!(matches!(cmd, MuxCommand::NewSession { name: Some(n) } if n == "mysession"));
    }

    #[test]
    fn test_parse_attach() {
        let cmd = CommandParser::parse("attach mysession").unwrap();
        assert!(matches!(cmd, MuxCommand::AttachSession { target } if target == "mysession"));
    }

    #[test]
    fn test_parse_split_horizontal() {
        let cmd = CommandParser::parse("split-window -h").unwrap();
        assert!(matches!(
            cmd,
            MuxCommand::SplitPane {
                direction: SplitDirection::Horizontal,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_split_vertical() {
        let cmd = CommandParser::parse("split-window -v").unwrap();
        assert!(matches!(
            cmd,
            MuxCommand::SplitPane {
                direction: SplitDirection::Vertical,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_select_pane() {
        let cmd = CommandParser::parse("select-pane -U").unwrap();
        assert!(matches!(
            cmd,
            MuxCommand::SelectPane {
                direction: PaneDirection::Up
            }
        ));

        let cmd = CommandParser::parse("select-pane -D").unwrap();
        assert!(matches!(
            cmd,
            MuxCommand::SelectPane {
                direction: PaneDirection::Down
            }
        ));
    }

    #[test]
    fn test_parse_new_window() {
        let cmd = CommandParser::parse("new-window -n mywindow vim").unwrap();
        match cmd {
            MuxCommand::NewWindow { name, command } => {
                assert_eq!(name, Some("mywindow".to_string()));
                assert_eq!(command, Some("vim".to_string()));
            }
            _ => panic!("Expected NewWindow command"),
        }
    }

    #[test]
    fn test_parse_rename_session() {
        let cmd = CommandParser::parse("rename-session my new name").unwrap();
        assert!(matches!(cmd, MuxCommand::RenameSession { name } if name == "my new name"));
    }

    #[test]
    fn test_parse_select_window_by_index() {
        let cmd = CommandParser::parse("select-window 2").unwrap();
        assert!(matches!(
            cmd,
            MuxCommand::SelectWindow {
                target: WindowTarget::Index(2)
            }
        ));
    }

    #[test]
    fn test_default_keybindings() {
        let bindings = default_keybindings();

        // Verifica alguns bindings essenciais
        assert!(bindings.contains_key("C-b c")); // New window
        assert!(bindings.contains_key("C-b d")); // Detach
        assert!(bindings.contains_key("C-b %")); // Split vertical
        assert!(bindings.contains_key("C-b n")); // Next window
        assert!(bindings.contains_key("C-b p")); // Previous window
    }

    #[test]
    fn test_pane_direction_opposite() {
        assert_eq!(PaneDirection::Up.opposite(), Some(PaneDirection::Down));
        assert_eq!(PaneDirection::Down.opposite(), Some(PaneDirection::Up));
        assert_eq!(PaneDirection::Left.opposite(), Some(PaneDirection::Right));
        assert_eq!(PaneDirection::Right.opposite(), Some(PaneDirection::Left));
        assert_eq!(PaneDirection::Next.opposite(), None);
    }

    #[test]
    fn test_layout_display() {
        assert_eq!(Layout::Tiled.to_string(), "tiled");
        assert_eq!(Layout::EvenHorizontal.to_string(), "even-horizontal");
        assert_eq!(Layout::MainVertical.to_string(), "main-vertical");
    }

    #[test]
    fn test_parse_resize() {
        let cmd = CommandParser::parse("resize-pane -L 10").unwrap();
        assert!(matches!(
            cmd,
            MuxCommand::ResizePane {
                direction: ResizeDirection::Left,
                amount: 10
            }
        ));
    }

    #[test]
    fn test_parse_set_option() {
        let cmd = CommandParser::parse("set-option -g status on").unwrap();
        match cmd {
            MuxCommand::SetOption { key, value, global } => {
                assert_eq!(key, "status");
                assert_eq!(value, "on");
                assert!(global);
            }
            _ => panic!("Expected SetOption command"),
        }
    }

    #[test]
    fn test_parse_empty() {
        let cmd = CommandParser::parse("").unwrap();
        assert!(matches!(cmd, MuxCommand::Nop));
    }

    #[test]
    fn test_parse_unknown_command() {
        let result = CommandParser::parse("unknown-command");
        assert!(result.is_err());
    }

    fn parse(input: &str) -> MuxCommand {
        CommandParser::parse(input).unwrap()
    }

    #[test]
    fn test_display_impls() {
        assert_eq!(SplitDirection::Horizontal.to_string(), "horizontal");
        assert_eq!(SplitDirection::Vertical.to_string(), "vertical");
        let directions = [
            (PaneDirection::Up, "up"),
            (PaneDirection::Down, "down"),
            (PaneDirection::Left, "left"),
            (PaneDirection::Right, "right"),
            (PaneDirection::Next, "next"),
            (PaneDirection::Previous, "previous"),
            (PaneDirection::Last, "last"),
            (PaneDirection::Index(3), "3"),
        ];
        for (direction, text) in directions {
            assert_eq!(direction.to_string(), text);
        }
        assert_eq!(Layout::EvenVertical.to_string(), "even-vertical");
        assert_eq!(Layout::MainHorizontal.to_string(), "main-horizontal");
        assert_eq!(Layout::Custom("meu".to_string()).to_string(), "meu");
    }

    #[test]
    fn test_simple_commands_and_aliases() {
        let cases = [
            ("detach", MuxCommand::DetachClient),
            ("ls", MuxCommand::ListSessions),
            ("ns", MuxCommand::NextSession),
            ("ps", MuxCommand::PreviousSession),
            ("last-session", MuxCommand::LastSession),
            ("killw", MuxCommand::KillWindow),
            ("n", MuxCommand::NextWindow),
            ("prev", MuxCommand::PreviousWindow),
            ("lastw", MuxCommand::LastWindow),
            ("lsw", MuxCommand::ListWindows),
            ("killp", MuxCommand::KillPane),
            ("lsp", MuxCommand::ListPanes),
            ("copy-mode", MuxCommand::CopySelection),
            ("pasteb", MuxCommand::PasteBuffer),
            ("show-buffer", MuxCommand::ShowBuffer),
            ("clear-history", MuxCommand::ClearBuffer),
            ("show", MuxCommand::ShowOptions),
            ("source", MuxCommand::ReloadConfig),
            ("clock", MuxCommand::ClockMode),
            ("command-prompt", MuxCommand::CommandPrompt),
            ("lockc", MuxCommand::LockClient),
            ("suspendc", MuxCommand::SuspendClient),
            ("QUIT", MuxCommand::Exit),
        ];
        for (input, expected) in cases {
            assert_eq!(parse(input), expected, "{input}");
        }
    }

    #[test]
    fn test_session_commands_with_arguments() {
        assert_eq!(parse("new"), MuxCommand::NewSession { name: None });
        assert_eq!(parse("new -s"), MuxCommand::NewSession { name: None });
        assert_eq!(
            parse("attach"),
            MuxCommand::AttachSession {
                target: "0".to_string()
            }
        );
        assert_eq!(
            parse("kill-session"),
            MuxCommand::KillSession { target: None }
        );
        assert_eq!(
            parse("switchc -t work"),
            MuxCommand::SelectSession {
                target: "work".to_string()
            }
        );
        assert_eq!(
            parse("switch-client"),
            MuxCommand::SelectSession {
                target: "0".to_string()
            }
        );
        assert_eq!(
            parse("renamew editor principal"),
            MuxCommand::RenameWindow {
                name: "editor principal".to_string()
            }
        );
    }

    /// Regressão: `-t` (a forma usual no tmux) era lido como o próprio alvo.
    #[test]
    fn test_target_flag_is_honored() {
        assert_eq!(
            parse("attach -t dev"),
            MuxCommand::AttachSession {
                target: "dev".to_string()
            }
        );
        assert_eq!(
            parse("kill-session -t old"),
            MuxCommand::KillSession {
                target: Some("old".to_string())
            }
        );
        assert_eq!(
            parse("selectw -t 2"),
            MuxCommand::SelectWindow {
                target: WindowTarget::Index(2)
            }
        );
        assert_eq!(
            parse("select-window --target logs"),
            MuxCommand::SelectWindow {
                target: WindowTarget::Name("logs".to_string())
            }
        );
        // `-t` sem valor: sem alvo
        assert_eq!(
            parse("kill-session -t"),
            MuxCommand::KillSession { target: None }
        );
    }

    #[test]
    fn test_window_target_parsing() {
        assert_eq!(
            parse("selectw editor"),
            MuxCommand::SelectWindow {
                target: WindowTarget::Name("editor".to_string())
            }
        );
        assert_eq!(
            parse("selectw"),
            MuxCommand::SelectWindow {
                target: WindowTarget::Last
            }
        );
        let swaps = [
            ("swapw 3", WindowTarget::Index(3)),
            ("swapw -t 1", WindowTarget::Index(1)),
            ("swapw -t logs", WindowTarget::Name("logs".to_string())),
            ("swapw -t", WindowTarget::Last),
            ("swapw", WindowTarget::Last),
        ];
        for (input, target) in swaps {
            assert_eq!(parse(input), MuxCommand::SwapWindow { target }, "{input}");
        }
    }

    #[test]
    fn test_pane_commands() {
        assert_eq!(
            parse("splitw -v -p 30"),
            MuxCommand::SplitPane {
                direction: SplitDirection::Vertical,
                size: Some("30".to_string())
            }
        );
        assert_eq!(
            parse("splitw -l"),
            MuxCommand::SplitPane {
                direction: SplitDirection::Vertical,
                size: None
            }
        );
        let selects = [
            ("selectp -L", PaneDirection::Left),
            ("selectp -R", PaneDirection::Right),
            ("selectp -l", PaneDirection::Last),
            ("selectp 4", PaneDirection::Index(4)),
            ("selectp -X", PaneDirection::Next),
            ("selectp", PaneDirection::Next),
        ];
        for (input, direction) in selects {
            assert_eq!(
                parse(input),
                MuxCommand::SelectPane { direction },
                "{input}"
            );
        }
        let swaps = [
            ("swapp -U", PaneDirection::Up),
            ("swapp -D", PaneDirection::Down),
            ("swapp -L", PaneDirection::Left),
            ("swapp -R", PaneDirection::Right),
            ("swapp -X", PaneDirection::Next),
            ("swapp", PaneDirection::Next),
        ];
        for (input, direction) in swaps {
            assert_eq!(parse(input), MuxCommand::SwapPane { direction }, "{input}");
        }
        let layouts = [
            ("select-layout even-horizontal", Layout::EvenHorizontal),
            ("select-layout even-vertical", Layout::EvenVertical),
            ("select-layout main-horizontal", Layout::MainHorizontal),
            ("select-layout main-vertical", Layout::MainVertical),
            ("select-layout tiled", Layout::Tiled),
            ("select-layout meu", Layout::Custom("meu".to_string())),
            ("select-layout", Layout::Tiled),
        ];
        for (input, layout) in layouts {
            assert_eq!(parse(input), MuxCommand::SelectLayout { layout }, "{input}");
        }
    }

    #[test]
    fn test_send_keys_and_misc() {
        assert_eq!(
            parse("send-keys -t 1 ls -la Enter"),
            MuxCommand::SendKeys {
                keys: "ls -la Enter".to_string(),
                target: Some("1".to_string())
            }
        );
        assert_eq!(
            parse("send echo -t"),
            MuxCommand::SendKeys {
                keys: "echo".to_string(),
                target: None
            }
        );
        assert_eq!(
            parse("set -s -g status off extra"),
            MuxCommand::SetOption {
                key: "status".to_string(),
                value: "off".to_string(),
                global: true
            }
        );
        assert_eq!(
            parse("display ola mundo"),
            MuxCommand::DisplayMessage {
                text: "ola mundo".to_string(),
                duration: Some(2000)
            }
        );
        assert_eq!(
            parse("run make test"),
            MuxCommand::RunShell {
                command: "make test".to_string()
            }
        );
        assert_eq!(
            parse("confirm Sair?"),
            MuxCommand::ConfirmBefore {
                prompt: "Sair?".to_string(),
                command: "nop".to_string()
            }
        );
        assert_eq!(
            parse("confirm"),
            MuxCommand::ConfirmBefore {
                prompt: "Confirm?".to_string(),
                command: "nop".to_string()
            }
        );
    }
}
