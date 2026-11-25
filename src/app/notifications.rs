//! Central de avisos dos programas (OSC 9/99/777 e campainha).
//!
//! Guarda quais painéis têm aviso não lido (o anel colorido ao redor do
//! painel e o contador na sidebar), um histórico curto para o painel de
//! avisos e manda notificação do desktop quando o usuário não está olhando.

use crate::terminal::grid::TerminalNotification;
use crate::ui::pane_layout::PaneId;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime};

/// Quantos avisos o histórico guarda.
const HISTORY: usize = 100;

/// Um aviso recebido.
#[derive(Clone, Debug, PartialEq)]
pub struct NotificationEntry {
    pub tab_id: u32,
    pub pane: PaneId,
    pub title: String,
    pub body: String,
    /// `true` para a campainha (BEL), que não tem texto.
    pub bell: bool,
    pub at: SystemTime,
}

impl NotificationEntry {
    /// Texto curto: título e corpo, ou "Bell".
    pub fn summary(&self) -> String {
        match (self.title.is_empty(), self.body.is_empty()) {
            (false, false) => format!("{}: {}", self.title, self.body),
            (false, true) => self.title.clone(),
            (true, false) => self.body.clone(),
            (true, true) => "Bell".to_string(),
        }
    }
}

/// Avisos não lidos por painel e histórico.
#[derive(Debug, Default)]
pub struct NotificationCenter {
    /// Painel -> (tab, quantidade não lida)
    unread: HashMap<PaneId, (u32, usize)>,
    /// Quando chegou o aviso não lido mais recente de cada painel (as cores
    /// do anel animam por alguns segundos a partir daí).
    since: HashMap<PaneId, Instant>,
    history: VecDeque<NotificationEntry>,
}

impl NotificationCenter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um aviso de um painel. `seen` indica que o usuário já está
    /// olhando para ele (janela em foco, tab ativa e painel em foco): nesse
    /// caso entra só no histórico, sem anel nem contador.
    pub fn record(
        &mut self,
        tab_id: u32,
        pane: PaneId,
        notification: Option<TerminalNotification>,
        seen: bool,
    ) {
        let (title, body, bell) = match notification {
            Some(n) => (n.title, n.body, false),
            None => (String::new(), String::new(), true),
        };
        if !seen {
            let entry = self.unread.entry(pane).or_insert((tab_id, 0));
            entry.0 = tab_id;
            entry.1 += 1;
            self.since.insert(pane, Instant::now());
        }
        if self.history.len() >= HISTORY {
            self.history.pop_front();
        }
        self.history.push_back(NotificationEntry {
            tab_id,
            pane,
            title,
            body,
            bell,
            at: SystemTime::now(),
        });
    }

    /// Mantém só os painéis que ainda existem.
    pub fn retain_panes(&mut self, exists: impl Fn(PaneId) -> bool) {
        self.unread.retain(|pane, _| exists(*pane));
        let unread = &self.unread;
        self.since.retain(|pane, _| unread.contains_key(pane));
    }

    /// O painel tem aviso não lido? (anel)
    pub fn has_unread(&self, pane: PaneId) -> bool {
        self.unread.contains_key(&pane)
    }

    /// Avisos não lidos de uma tab (contador da sidebar).
    pub fn unread_for_tab(&self, tab_id: u32) -> usize {
        self.unread
            .values()
            .filter(|(tab, _)| *tab == tab_id)
            .map(|(_, count)| count)
            .sum()
    }

    /// Total de avisos não lidos.
    pub fn total_unread(&self) -> usize {
        self.unread.values().map(|(_, count)| count).sum()
    }

    /// Marca o painel como visto (apaga o anel).
    pub fn mark_seen(&mut self, pane: PaneId) -> bool {
        self.since.remove(&pane);
        self.unread.remove(&pane).is_some()
    }

    /// Esquece um painel que fechou.
    pub fn forget_pane(&mut self, pane: PaneId) {
        self.unread.remove(&pane);
        self.since.remove(&pane);
    }

    /// Há quanto tempo chegou o aviso não lido mais recente do painel.
    pub fn unread_age(&self, pane: PaneId) -> Option<Duration> {
        self.since.get(&pane).map(Instant::elapsed)
    }

    /// Há quanto tempo chegou o aviso não lido mais recente da tab.
    pub fn tab_unread_age(&self, tab_id: u32) -> Option<Duration> {
        self.unread
            .iter()
            .filter(|(_, (tab, _))| *tab == tab_id)
            .filter_map(|(pane, _)| self.unread_age(*pane))
            .min()
    }

    /// Painel com o aviso não lido mais recente: (tab, painel).
    pub fn latest_unread(&self) -> Option<(u32, PaneId)> {
        self.history
            .iter()
            .rev()
            .find(|entry| self.unread.contains_key(&entry.pane))
            .map(|entry| (entry.tab_id, entry.pane))
    }

    /// Histórico, do mais recente para o mais antigo.
    pub fn history(&self) -> impl Iterator<Item = &NotificationEntry> {
        self.history.iter().rev()
    }
}

/// Mostra uma notificação do desktop sem bloquear (notify-send, osascript).
pub fn send_desktop_notification(title: &str, body: &str) {
    let title = if title.is_empty() {
        "Code Terminal"
    } else {
        title
    };
    #[cfg(target_os = "linux")]
    let command = std::process::Command::new("notify-send")
        .args(["--app-name=Code Terminal", "--", title, body])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    #[cfg(target_os = "macos")]
    let command = {
        let script = format!(
            "display notification {:?} with title {:?}",
            body.replace('"', "'"),
            title.replace('"', "'")
        );
        std::process::Command::new("osascript")
            .args(["-e", &script])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let command: std::io::Result<std::process::Child> = {
        let _ = (title, body);
        Err(std::io::Error::other("desktop notifications unsupported"))
    };
    match command {
        // Recolhe o processo em segundo plano para não deixar zumbi.
        Ok(mut child) => {
            let reaped = std::thread::Builder::new()
                .name("notify-reaper".into())
                .spawn(move || {
                    let _ = child.wait();
                });
            if let Err(e) = reaped {
                tracing::debug!(error = %e, "Could not wait for the notification process");
            }
        }
        Err(e) => tracing::debug!(error = %e, "Desktop notification unavailable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(title: &str, body: &str) -> Option<TerminalNotification> {
        Some(TerminalNotification {
            title: title.into(),
            body: body.into(),
        })
    }

    #[test]
    fn test_unread_rings_counts_and_seen() {
        let mut center = NotificationCenter::new();
        center.record(1, 10, note("Claude", "needs input"), false);
        center.record(1, 10, None, false);
        center.record(2, 20, note("", "build ok"), false);
        center.record(2, 21, note("visible", ""), true);

        assert!(center.has_unread(10));
        assert!(center.has_unread(20));
        assert!(!center.has_unread(21));
        assert_eq!(center.unread_for_tab(1), 2);
        assert_eq!(center.unread_for_tab(2), 1);
        assert_eq!(center.total_unread(), 3);
        assert_eq!(center.latest_unread(), Some((2, 20)));

        assert!(center.mark_seen(20));
        assert!(!center.mark_seen(20));
        assert_eq!(center.latest_unread(), Some((1, 10)));
        center.forget_pane(10);
        assert_eq!(center.total_unread(), 0);
        assert_eq!(center.latest_unread(), None);

        let summaries: Vec<String> = center.history().map(|e| e.summary()).collect();
        assert_eq!(
            summaries,
            vec!["visible", "build ok", "Bell", "Claude: needs input"]
        );
    }

    #[test]
    fn test_history_is_bounded() {
        let mut center = NotificationCenter::new();
        for i in 0..(HISTORY + 10) {
            center.record(1, i as u64, note("t", ""), true);
        }
        assert_eq!(center.history().count(), HISTORY);
    }
}
