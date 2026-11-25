//! Ponte entre as host functions dos plugins WASM e o app.
//!
//! As host functions rodam dentro de chamadas do wasmtime e não têm acesso
//! ao `AppState`. A ponte tem duas metades:
//!
//! - **Visão** ([`HostView`]): o app publica, a cada frame em que há plugins,
//!   o que eles podem ler (tamanho, cursor, linhas visíveis, ids, seleção).
//! - **Comandos** ([`HostCommand`]): o plugin pede (escrever no painel, mandar
//!   texto ao shell, avisar, texto de status) e o app aplica no próximo frame.
//!
//! O texto digitado que o app repassa aos plugins fica numa fila lida por
//! `host.read_input`.

use std::collections::VecDeque;
use std::sync::{Mutex, RwLock};

/// Limite da fila de comandos: um plugin em laço não consome memória sem fim.
const MAX_PENDING_COMMANDS: usize = 1024;
/// Limite da fila de entrada lida por `read_input`.
const MAX_PENDING_INPUT: usize = 256;

/// O que os plugins enxergam do painel em foco.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostView {
    pub cols: u16,
    pub rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    /// Linhas visíveis do painel em foco, sem espaços à direita.
    pub lines: Vec<String>,
    pub tab_id: u64,
    pub pane_id: u64,
    pub selection: String,
    pub theme: String,
}

impl HostView {
    /// Texto do retângulo `[start_col, end_col)` × `[start_row, end_row]`,
    /// uma linha por `\n` (colunas contadas em caracteres).
    pub fn region(
        &self,
        start_col: usize,
        start_row: usize,
        end_col: usize,
        end_row: usize,
    ) -> String {
        let last = end_row.min(self.lines.len().saturating_sub(1));
        if self.lines.is_empty() || start_row > last {
            return String::new();
        }
        let width = end_col.saturating_sub(start_col);
        self.lines[start_row..=last]
            .iter()
            .map(|line| line.chars().skip(start_col).take(width).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Pedido de um plugin ao app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCommand {
    /// Texto exibido no painel em foco (passa pelo parser VT, não vai ao shell).
    Write(String),
    /// Texto na posição (coluna, linha), sem mexer no cursor do programa.
    WriteAt { col: u16, row: u16, text: String },
    /// Bytes mandados ao processo do painel, como se fossem digitados.
    /// Exige a permissão `pty` no manifesto.
    SendInput(String),
    /// Aviso na central de notificações (nível 0 info, 1 ok, 2 alerta, 3 erro).
    Notify { level: i32, message: String },
    /// Texto de status do plugin (mostrado na sidebar); vazio apaga.
    Status(String),
    /// Título da janela.
    SetTitle(String),
}

/// Comando com o plugin que pediu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRequest {
    pub plugin: String,
    pub command: HostCommand,
}

static VIEW: RwLock<Option<HostView>> = RwLock::new(None);
static COMMANDS: Mutex<Vec<PluginRequest>> = Mutex::new(Vec::new());
static INPUT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// O app publica a visão atual.
pub fn publish_view(view: HostView) {
    if let Ok(mut slot) = VIEW.write() {
        *slot = Some(view);
    }
}

/// Lê a visão atual (padrão 80×24 vazio antes da primeira publicação).
pub fn with_view<R>(read: impl FnOnce(&HostView) -> R) -> R {
    let default = HostView {
        cols: 80,
        rows: 24,
        ..HostView::default()
    };
    match VIEW.read() {
        Ok(slot) => read(slot.as_ref().unwrap_or(&default)),
        Err(_) => read(&default),
    }
}

/// Registra um pedido do plugin; devolve `false` se a fila estiver cheia.
pub fn push(plugin: &str, command: HostCommand) -> bool {
    let Ok(mut queue) = COMMANDS.lock() else {
        return false;
    };
    if queue.len() >= MAX_PENDING_COMMANDS {
        return false;
    }
    queue.push(PluginRequest {
        plugin: plugin.to_string(),
        command,
    });
    true
}

/// O app retira os pedidos pendentes, na ordem.
pub fn drain() -> Vec<PluginRequest> {
    COMMANDS
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

/// O app repassa texto digitado aos plugins (lido por `read_input`).
pub fn push_input(text: &str) {
    if let Ok(mut queue) = INPUT.lock() {
        if queue.len() >= MAX_PENDING_INPUT {
            queue.pop_front();
        }
        queue.push_back(text.to_string());
    }
}

/// Próximo texto digitado, se houver.
pub fn pop_input() -> Option<String> {
    INPUT.lock().ok().and_then(|mut queue| queue.pop_front())
}

/// Esvazia tudo (testes e fechamento do app).
pub fn reset() {
    if let Ok(mut slot) = VIEW.write() {
        *slot = None;
    }
    drain();
    if let Ok(mut queue) = INPUT.lock() {
        queue.clear();
    }
}

#[cfg(test)]
pub(crate) static TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_cuts_columns_and_rows() {
        let view = HostView {
            lines: vec!["hello world".into(), "ação 123".into(), "x".into()],
            ..HostView::default()
        };
        assert_eq!(view.region(0, 0, 5, 0), "hello");
        assert_eq!(view.region(1, 1, 4, 2), "ção\n");
        assert_eq!(view.region(0, 5, 10, 9), "");
        assert_eq!(view.region(0, 2, 80, 99), "x");
    }

    #[test]
    fn test_commands_and_input_queues() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert!(push("bridge-test", HostCommand::Write("oi".into())));
        assert!(push("bridge-test", HostCommand::Status("ok".into())));
        let drained: Vec<_> = drain()
            .into_iter()
            .filter(|r| r.plugin == "bridge-test")
            .collect();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].command, HostCommand::Write("oi".into()));

        // Fila cheia recusa em vez de crescer sem fim.
        let accepted = (0..=MAX_PENDING_COMMANDS)
            .filter(|i| push("bridge-test", HostCommand::Write(i.to_string())))
            .count();
        assert!(accepted <= MAX_PENDING_COMMANDS);
        reset();

        push_input("ls\n");
        assert_eq!(pop_input().as_deref(), Some("ls\n"));
        assert_eq!(pop_input(), None);
        for i in 0..=MAX_PENDING_INPUT {
            push_input(&i.to_string());
        }
        assert_eq!(pop_input().as_deref(), Some("1"));
        reset();
    }

    #[test]
    fn test_view_default_and_publish() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset();
        assert_eq!(with_view(|v| (v.cols, v.rows)), (80, 24));
        publish_view(HostView {
            cols: 100,
            rows: 30,
            tab_id: 7,
            ..HostView::default()
        });
        assert_eq!(with_view(|v| (v.cols, v.rows, v.tab_id)), (100, 30, 7));
        reset();
    }
}
